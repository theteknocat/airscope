//! True signal strength (dBm) of Wi-Fi access points, read from the kernel's nl80211 scan cache over generic netlink
//! (the same list `iw dev <if> scan dump` prints; no root, no tool, and it never starts a scan). NetworkManager only
//! reports a 0–100 percentage that tops out at about -50 dBm, so strong nearby sources all look the same.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const NETLINK_GENERIC: i32 = 16;
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const NL80211_CMD_GET_SCAN: u8 = 32;
const NL80211_ATTR_IFINDEX: u16 = 3;
const NL80211_ATTR_BSS: u16 = 47;
const NL80211_BSS_BSSID: u16 = 1;
const NL80211_BSS_SIGNAL_MBM: u16 = 7;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;

fn attr(buf: &mut Vec<u8>, ty: u16, data: &[u8]) {
    buf.extend(((4 + data.len()) as u16).to_ne_bytes());
    buf.extend(ty.to_ne_bytes());
    buf.extend(data);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn message(ty: u16, flags: u16, cmd: u8, attrs: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(20 + attrs.len());
    m.extend(((20 + attrs.len()) as u32).to_ne_bytes());
    m.extend(ty.to_ne_bytes());
    m.extend(flags.to_ne_bytes());
    m.extend(1u32.to_ne_bytes()); // sequence
    m.extend(0u32.to_ne_bytes()); // port id: the kernel fills it in
    m.extend([cmd, 1, 0, 0]); // generic netlink header: command, version, reserved
    m.extend(attrs);
    m
}

/// The (type, payload) pairs of a run of netlink attributes.
fn attrs(mut b: &[u8]) -> Vec<(u16, &[u8])> {
    let mut v = vec![];
    while b.len() >= 4 {
        let len = u16::from_ne_bytes([b[0], b[1]]) as usize;
        let ty = u16::from_ne_bytes([b[2], b[3]]) & 0x3fff;
        if len < 4 || len > b.len() {
            break;
        }
        v.push((ty, &b[4..len]));
        let step = (len + 3) & !3;
        if step >= b.len() {
            break;
        }
        b = &b[step..];
    }
    v
}

/// Sends `req` and collects the attribute bytes of every reply message (for a dump, until the kernel says it is done).
fn request(fd: &OwnedFd, req: &[u8], dump: bool) -> Option<Vec<Vec<u8>>> {
    let tv = libc::timeval { tv_sec: 2, tv_usec: 0 };
    unsafe {
        libc::setsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv as *const _ as *const libc::c_void, std::mem::size_of::<libc::timeval>() as u32);
        if libc::send(fd.as_raw_fd(), req.as_ptr() as *const libc::c_void, req.len(), 0) < 0 {
            return None;
        }
    }
    let mut out = vec![];
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
        if n <= 0 {
            return None;
        }
        let buf = &buf[..n as usize];
        let mut off = 0;
        while off + 16 <= buf.len() {
            let len = u32::from_ne_bytes(buf[off..off + 4].try_into().ok()?) as usize;
            let ty = u16::from_ne_bytes([buf[off + 4], buf[off + 5]]);
            if len < 16 || off + len > buf.len() {
                return None;
            }
            match ty {
                NLMSG_ERROR => return None,
                NLMSG_DONE => return Some(out),
                _ if len >= 20 => {
                    out.push(buf[off + 20..off + len].to_vec());
                    if !dump {
                        return Some(out);
                    }
                }
                _ => {}
            }
            off += (len + 3) & !3;
        }
    }
}

fn open() -> Option<OwnedFd> {
    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, NETLINK_GENERIC) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Interface indexes of the wireless interfaces (/sys/class/net/<if>/wireless exists).
fn wireless_ifindexes() -> Vec<u32> {
    std::fs::read_dir("/sys/class/net")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("wireless").exists())
        .filter_map(|e| std::fs::read_to_string(e.path().join("ifindex")).ok()?.trim().parse().ok())
        .collect()
}

/// Reads a scan dump's reply messages into BSSID (upper case, "AA:BB:...") -> dBm.
fn parse_scan(msgs: &[Vec<u8>]) -> HashMap<String, f32> {
    let mut out = HashMap::new();
    for m in msgs {
        for (ty, bss) in attrs(m) {
            if ty != NL80211_ATTR_BSS {
                continue;
            }
            let (mut mac, mut dbm) = (None, None);
            for (t, d) in attrs(bss) {
                match t {
                    NL80211_BSS_BSSID if d.len() == 6 => mac = Some(d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")),
                    NL80211_BSS_SIGNAL_MBM if d.len() == 4 => dbm = Some(i32::from_ne_bytes([d[0], d[1], d[2], d[3]]) as f32 / 100.0),
                    _ => {}
                }
            }
            if let (Some(mac), Some(dbm)) = (mac, dbm) {
                out.insert(mac, dbm);
            }
        }
    }
    out
}

/// Signal strength in dBm of every access point in the kernel's scan cache; empty if it cannot be read.
pub fn signals() -> HashMap<String, f32> {
    let ifs = wireless_ifindexes();
    let Some(fd) = open().filter(|_| !ifs.is_empty()) else { return HashMap::new() };
    let mut name = vec![];
    attr(&mut name, CTRL_ATTR_FAMILY_NAME, b"nl80211\0");
    let Some(reply) = request(&fd, &message(GENL_ID_CTRL, NLM_F_REQUEST, CTRL_CMD_GETFAMILY, &name), false) else { return HashMap::new() };
    let Some(family) = reply.first().and_then(|m| attrs(m).into_iter().find(|(t, d)| *t == CTRL_ATTR_FAMILY_ID && d.len() >= 2).map(|(_, d)| u16::from_ne_bytes([d[0], d[1]]))) else { return HashMap::new() };
    let mut out = HashMap::new();
    for i in ifs {
        let mut a = vec![];
        attr(&mut a, NL80211_ATTR_IFINDEX, &i.to_ne_bytes());
        if let Some(msgs) = request(&fd, &message(family, NLM_F_REQUEST | NLM_F_DUMP, NL80211_CMD_GET_SCAN, &a), true) {
            out.extend(parse_scan(&msgs));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bssid_and_signal_from_a_bss_attribute() {
        let mut bss = vec![];
        attr(&mut bss, NL80211_BSS_BSSID, &[0xb0, 0x82, 0xe2, 0xb4, 0x3a, 0x56]);
        attr(&mut bss, NL80211_BSS_SIGNAL_MBM, &(-3400i32).to_ne_bytes());
        let mut msg = vec![];
        attr(&mut msg, NL80211_ATTR_BSS, &bss);
        let got = parse_scan(&[msg]);
        assert_eq!(got.get("B0:82:E2:B4:3A:56"), Some(&-34.0));
    }

    /// Live read on this machine: `cargo test nl80211_dump -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn nl80211_dump() {
        for (m, d) in signals() {
            println!("{m} {d} dBm");
        }
    }
}
