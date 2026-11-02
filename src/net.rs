//! LAN basics without external tools: IPv4 neighbour table (netlink), default gateway (/proc),
//! unprivileged ICMP echo, reverse DNS (libc). Replaces `ip neigh`, `ip route`, `ping`, `getent`.

use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

pub struct Neigh {
    pub ip: Ipv4Addr,
    pub mac: String, // upper case
    pub dev: String,
    pub state: &'static str, // "REACHABLE", "STALE", ...
}

fn state_name(s: u16) -> &'static str {
    match s {
        x if x & 0x02 != 0 => "REACHABLE",
        x if x & 0x04 != 0 => "STALE",
        x if x & 0x08 != 0 => "DELAY",
        x if x & 0x10 != 0 => "PROBE",
        x if x & 0x01 != 0 => "INCOMPLETE",
        x if x & 0x20 != 0 => "FAILED",
        x if x & 0x80 != 0 => "PERMANENT",
        _ => "NOARP",
    }
}

/// Virtual interfaces that are not "the LAN" (containers, VPNs, VMs). Plain bridges like br0 stay.
pub fn is_virtual(dev: &str) -> bool {
    ["br-", "docker", "veth", "virbr", "tailscale", "tun", "wg", "zt", "lxc", "cni", "flannel"].iter().any(|p| dev.starts_with(p))
}

/// IPv4 neighbour entries that have a hardware address (netlink RTM_GETNEIGH dump).
pub fn neighbours() -> Vec<Neigh> {
    let mut out = vec![];
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return out;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let mut req = [0u8; 28]; // nlmsghdr (16) + ndmsg (12)
        req[0..4].copy_from_slice(&28u32.to_ne_bytes());
        req[4..6].copy_from_slice(&30u16.to_ne_bytes()); // RTM_GETNEIGH
        req[6..8].copy_from_slice(&0x301u16.to_ne_bytes()); // REQUEST | DUMP
        req[16] = libc::AF_INET as u8;
        if libc::send(fd.as_raw_fd(), req.as_ptr() as *const _, req.len(), 0) < 0 {
            return out;
        }
        let tv = libc::timeval { tv_sec: 2, tv_usec: 0 };
        libc::setsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv as *const _ as *const _, std::mem::size_of::<libc::timeval>() as u32);
        let mut buf = vec![0u8; 65536];
        loop {
            let n = libc::recv(fd.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len(), 0);
            if n <= 0 {
                return out;
            }
            let b = &buf[..n as usize];
            let mut o = 0;
            while o + 16 <= b.len() {
                let len = u32::from_ne_bytes(b[o..o + 4].try_into().unwrap()) as usize;
                let ty = u16::from_ne_bytes(b[o + 4..o + 6].try_into().unwrap());
                if len < 16 || o + len > b.len() {
                    break;
                }
                match ty {
                    3 | 2 => return out, // DONE / ERROR
                    28 if len >= 28 => {
                        let m = &b[o + 16..o + len];
                        let ifindex = i32::from_ne_bytes(m[4..8].try_into().unwrap());
                        let state = u16::from_ne_bytes(m[8..10].try_into().unwrap());
                        let (mut ip, mut mac) = (None, None);
                        let mut a = 12;
                        while a + 4 <= m.len() {
                            let al = u16::from_ne_bytes(m[a..a + 2].try_into().unwrap()) as usize;
                            let at = u16::from_ne_bytes(m[a + 2..a + 4].try_into().unwrap());
                            if al < 4 || a + al > m.len() {
                                break;
                            }
                            let v = &m[a + 4..a + al];
                            match at {
                                1 if v.len() == 4 => ip = Some(Ipv4Addr::new(v[0], v[1], v[2], v[3])),
                                2 if v.len() == 6 => mac = Some(format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", v[0], v[1], v[2], v[3], v[4], v[5])),
                                _ => {}
                            }
                            a += (al + 3) & !3;
                        }
                        let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
                        if let (Some(ip), Some(mac)) = (ip, mac) {
                            if !libc::if_indextoname(ifindex as u32, name.as_mut_ptr()).is_null() {
                                let dev = CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
                                out.push(Neigh { ip, mac, dev, state: state_name(state) });
                            }
                        }
                    }
                    _ => {}
                }
                o += (len + 3) & !3;
            }
        }
    }
}

/// Default gateway from the kernel routing table (lowest metric wins).
pub fn gateway() -> Option<Ipv4Addr> {
    let t = std::fs::read_to_string("/proc/net/route").ok()?;
    t.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() > 6 && f[1] == "00000000").then(|| (f[6].parse::<u32>().unwrap_or(u32::MAX), u32::from_str_radix(f[2], 16).ok()))
        })
        .min_by_key(|(m, _)| *m)
        .and_then(|(_, g)| g)
        .map(|g| Ipv4Addr::from(u32::from_be(g))) // /proc prints the little-endian word
}

/// One ICMP echo over an unprivileged datagram socket. false on silence or if the system forbids it
/// (net.ipv4.ping_group_range) — callers should then not treat silence as "offline".
pub fn ping(ip: Ipv4Addr, wait: Duration) -> Option<bool> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, libc::IPPROTO_ICMP);
        if fd < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let tv = libc::timeval { tv_sec: wait.as_secs() as _, tv_usec: wait.subsec_micros() as _ };
        libc::setsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv as *const _ as *const _, std::mem::size_of::<libc::timeval>() as u32);
        let pkt = [8u8, 0, 0, 0, 0, 0, 0, 1]; // echo request; kernel fills id and checksum
        let mut sa: libc::sockaddr_in = std::mem::zeroed();
        sa.sin_family = libc::AF_INET as u16;
        sa.sin_addr.s_addr = u32::from(ip).to_be();
        if libc::sendto(fd.as_raw_fd(), pkt.as_ptr() as *const _, pkt.len(), 0, &sa as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as u32) < 0 {
            return Some(false);
        }
        let mut buf = [0u8; 64];
        let n = libc::recv(fd.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len(), 0);
        Some(n >= 1 && buf[0] == 0)
    }
}

/// Reverse DNS name (libc resolver, so /etc/hosts and nsswitch apply); None if there is none.
pub fn reverse_name(ip: Ipv4Addr) -> Option<String> {
    unsafe {
        let mut sa: libc::sockaddr_in = std::mem::zeroed();
        sa.sin_family = libc::AF_INET as u16;
        sa.sin_addr.s_addr = u32::from(ip).to_be();
        let mut host = [0 as libc::c_char; 256];
        let r = libc::getnameinfo(&sa as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as u32, host.as_mut_ptr(), host.len() as u32, std::ptr::null_mut(), 0, libc::NI_NAMEREQD);
        (r == 0).then(|| CStr::from_ptr(host.as_ptr()).to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_ifaces() {
        assert!(is_virtual("docker0") && is_virtual("br-1a2b") && is_virtual("tailscale0"));
        assert!(!is_virtual("enp5s0") && !is_virtual("br0") && !is_virtual("wlp6s0"));
    }

    /// Compare with `ip -4 neigh show` and `ip route`. Run: cargo test net_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn net_dump() {
        println!("gateway {:?}", gateway());
        for n in neighbours() {
            println!("{} {} {} {} ping={:?} name={:?}", n.ip, n.mac, n.dev, n.state, ping(n.ip, Duration::from_secs(1)), reverse_name(n.ip));
        }
    }
}
