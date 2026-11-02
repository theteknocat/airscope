//! Native Bluetooth HCI access: connection list, AFH channel map, link quality, RSSI, TX power and
//! remote version, using the kernel's HCI socket. Replaces the deprecated `hcitool`.
//!
//! Only read-only commands are used. The kernel lets unprivileged sockets send these (same as `hcitool`),
//! so no root is needed.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

const AF_BLUETOOTH: i32 = 31;
const BTPROTO_HCI: i32 = 1;
const SOL_HCI: i32 = 0;
const HCI_FILTER: i32 = 2;
const HCIGETCONNLIST: u64 = 0x800448D4; // _IOR('H', 212, int)

const EVT_REMOTE_VERSION: u8 = 0x0C;
const EVT_CMD_COMPLETE: u8 = 0x0E;
const EVT_CMD_STATUS: u8 = 0x0F;

const OP_READ_AFH_MAP: u16 = 0x1406;
const OP_READ_LINK_QUALITY: u16 = 0x1403;
const OP_READ_RSSI: u16 = 0x1405;
const OP_READ_TX_POWER: u16 = 0x0C2D;
const OP_READ_REMOTE_VERSION: u16 = 0x041D;

#[repr(C)]
struct SockaddrHci {
    family: u16,
    dev: u16,
    channel: u16,
}

#[repr(C)]
struct HciFilter {
    type_mask: u32,
    event_mask: [u32; 2],
    opcode: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ConnInfo {
    handle: u16,
    bdaddr: [u8; 6],
    kind: u8,
    out: u8,
    state: u16,
    link_mode: u32,
}

#[repr(C)]
struct ConnList {
    dev_id: u16,
    conn_num: u16,
    info: [ConnInfo; 16],
}

/// A connected (ACL) Bluetooth link.
pub struct Conn {
    pub dev: u16,
    pub handle: u16,
    pub mac: String, // upper case, colon separated
}

#[derive(Default)]
pub struct LinkData {
    /// None when adaptive frequency hopping is off or unsupported (the link hops over all 79 channels).
    pub afh: Option<Vec<bool>>,
    pub lq: Option<i32>,
    pub rssi: Option<i32>,
    pub tpl: Option<i32>,
}

fn open(dev: u16) -> Option<OwnedFd> {
    unsafe {
        let fd = libc::socket(AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC, BTPROTO_HCI);
        if fd < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let addr = SockaddrHci { family: AF_BLUETOOTH as u16, dev, channel: 0 };
        if libc::bind(fd.as_raw_fd(), &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<SockaddrHci>() as u32) < 0 {
            return None;
        }
        // only the events we wait for
        let f = HciFilter { type_mask: 1 << 4, event_mask: [(1 << EVT_REMOTE_VERSION) | (1 << EVT_CMD_COMPLETE) | (1 << EVT_CMD_STATUS), 0], opcode: 0 };
        libc::setsockopt(fd.as_raw_fd(), SOL_HCI, HCI_FILTER, &f as *const _ as *const libc::c_void, std::mem::size_of::<HciFilter>() as u32);
        Some(fd)
    }
}

fn mac_str(b: [u8; 6]) -> String {
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[5], b[4], b[3], b[2], b[1], b[0])
}

/// All connected ACL links on all local adapters.
pub fn connections() -> Vec<Conn> {
    let mut out = vec![];
    for dev in 0..4u16 {
        let Some(fd) = open(dev) else { continue };
        let mut req = ConnList { dev_id: dev, conn_num: 16, info: [ConnInfo::default(); 16] };
        if unsafe { libc::ioctl(fd.as_raw_fd(), HCIGETCONNLIST as _, &mut req as *mut ConnList) } < 0 {
            continue;
        }
        for i in 0..(req.conn_num as usize).min(16) {
            let c = req.info[i];
            if c.kind == 1 {
                out.push(Conn { dev, handle: c.handle, mac: mac_str(c.bdaddr) });
            }
        }
    }
    out
}

/// Send a command and wait for its answer: the Command Complete return parameters, or (when `want_evt`
/// is given) the data of that event once the command was accepted.
fn exchange(fd: RawFd, opcode: u16, params: &[u8], want_evt: Option<u8>) -> Option<Vec<u8>> {
    let mut pkt = vec![1u8, opcode as u8, (opcode >> 8) as u8, params.len() as u8];
    pkt.extend_from_slice(params);
    if unsafe { libc::write(fd, pkt.as_ptr() as *const libc::c_void, pkt.len()) } < 0 {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut buf = [0u8; 300];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?.as_millis() as i32;
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut p, 1, left) } <= 0 {
            return None;
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 3 || buf[0] != 4 {
            continue;
        }
        let (evt, plen) = (buf[1], buf[2] as usize);
        let data = &buf[3..(3 + plen).min(n as usize)];
        match evt {
            EVT_CMD_COMPLETE if want_evt.is_none() && data.len() >= 3 && u16::from_le_bytes([data[1], data[2]]) == opcode => return Some(data[3..].to_vec()),
            EVT_CMD_STATUS if data.len() >= 4 && u16::from_le_bytes([data[2], data[3]]) == opcode && data[0] != 0 => return None,
            e if Some(e) == want_evt => return Some(data.to_vec()),
            _ => {}
        }
    }
}

fn read_i8(fd: RawFd, opcode: u16, params: &[u8]) -> Option<i32> {
    let r = exchange(fd, opcode, params, None)?;
    (r.len() >= 4 && r[0] == 0).then(|| r[3] as i8 as i32)
}

/// Decode the 10-byte AFH channel map (bit i = Bluetooth channel i is in use).
pub fn decode_map(b: &[u8]) -> Vec<bool> {
    (0..79).map(|i| b[i / 8] >> (i % 8) & 1 == 1).collect()
}

pub fn link_data(c: &Conn) -> LinkData {
    let Some(fd) = open(c.dev) else { return LinkData::default() };
    let fd = fd.as_raw_fd();
    let h = c.handle.to_le_bytes();
    let afh = exchange(fd, OP_READ_AFH_MAP, &h, None).filter(|r| r.len() >= 14 && r[0] == 0 && r[3] == 1).map(|r| decode_map(&r[4..14]));
    LinkData {
        afh,
        lq: exchange(fd, OP_READ_LINK_QUALITY, &h, None).filter(|r| r.len() >= 4 && r[0] == 0).map(|r| r[3] as i32),
        rssi: read_i8(fd, OP_READ_RSSI, &h),
        tpl: read_i8(fd, OP_READ_TX_POWER, &[h[0], h[1], 0]),
    }
}

/// (Bluetooth version text, chip maker text) as reported by the remote controller.
pub fn remote_version(c: &Conn) -> (String, String) {
    let Some(fd) = open(c.dev) else { return Default::default() };
    let h = c.handle.to_le_bytes();
    let Some(d) = exchange(fd.as_raw_fd(), OP_READ_REMOTE_VERSION, &h, Some(EVT_REMOTE_VERSION)) else { return Default::default() };
    if d.len() < 8 || d[0] != 0 {
        return Default::default();
    }
    let ver = d[3];
    let company = u16::from_le_bytes([d[4], d[5]]);
    (format!("{} (0x{:x})", version_name(ver), ver), format!("{} ({})", company_name(company), company))
}

fn version_name(v: u8) -> &'static str {
    ["1.0b", "1.1", "1.2", "2.0", "2.1", "3.0", "4.0", "4.1", "4.2", "5.0", "5.1", "5.2", "5.3", "5.4", "6.0", "6.1"].get(v as usize).copied().unwrap_or("?")
}

fn company_name(id: u16) -> String {
    // the common chip makers; unknown ids show their number (full list: bluetooth.com/specifications/assigned-numbers)
    let n = match id {
        0 => "Ericsson Technology Licensing",
        1 => "Nokia Mobile Phones",
        2 => "Intel Corp.",
        4 => "Toshiba Corp.",
        6 => "Microsoft",
        8 => "Motorola",
        9 => "Infineon Technologies AG",
        10 => "Qualcomm Technologies International, Ltd. (formerly CSR)",
        13 => "Texas Instruments Inc.",
        15 => "Broadcom Corporation",
        29 => "Qualcomm",
        34 => "NEC Corporation",
        37 => "NXP Semiconductors (formerly Philips Semiconductors)",
        48 => "ST Microelectronics",
        70 => "MediaTek, Inc.",
        76 => "Apple, Inc.",
        89 => "Nordic Semiconductor ASA",
        93 => "Realtek Semiconductor Corporation",
        117 => "Samsung Electronics Co. Ltd.",
        224 => "Google",
        305 => "Cypress Semiconductor",
        474 => "Logitech International SA",
        _ => return format!("Company 0x{:04x}", id),
    };
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_bits() {
        // channels 0 and 9 and 78 set
        let mut b = [0u8; 10];
        b[0] = 1;
        b[1] = 1 << 1;
        b[9] = 1 << 6;
        let m = decode_map(&b);
        assert_eq!(m.len(), 79);
        assert_eq!(m.iter().filter(|x| **x).count(), 3);
        assert!(m[0] && m[9] && m[78]);
    }

    #[test]
    fn mac_is_reversed() {
        assert_eq!(mac_str([0x2F, 0x00, 0xAA, 0x35, 0x73, 0xF4]), "F4:73:35:AA:00:2F");
    }

    /// Prints live data for connected devices; compare with `hcitool con/afh/lq/rssi/tpl/info`.
    /// Run with: cargo test live_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_dump() {
        for c in connections() {
            let d = link_data(&c);
            println!("{} handle {} afh={:?} lq={:?} rssi={:?} tpl={:?} ver={:?}", c.mac, c.handle, d.afh.as_ref().map(|m| m.iter().filter(|x| **x).count()), d.lq, d.rssi, d.tpl, remote_version(&c));
        }
    }
}
