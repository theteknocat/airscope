//! Reads Bluetooth capture files written by `btmon -w` (btsnoop "monitor" format) without any external tool and
//! turns them into link events: connects, disconnects with reason, and sniff-exit requests with how long the
//! controller took to answer. The file may still be growing: call `feed` again with the larger file and it
//! carries on where it stopped.

use std::collections::HashMap;

/// Microseconds from the btsnoop epoch (year 0) to the Unix epoch.
const BTSNOOP_EPOCH_US: i64 = 0x00dc_ddb3_0f2f_8000;
const OP_COMMAND: u16 = 2;
const OP_EVENT: u16 = 3;
const OP_ACL_TX: u16 = 4;
const OP_ACL_RX: u16 = 5;
/// "Exit Sniff Mode" command (OGF 0x02, OCF 0x0004).
const SNIFF_EXIT: u16 = 0x0804;
/// A sniff exit the controller answers later than this is reported as slow.
pub const SLOW_EXIT_S: f64 = 2.0;
/// A drop this soon after a slow or unanswered sniff exit is counted as following it.
const STALL_WINDOW_S: f64 = 30.0;

pub type Addr = [u8; 6];
/// Stands in for the address of a link that was already up before the capture began.
pub const UNKNOWN: Addr = [0; 6];

pub fn mac(a: &Addr) -> String {
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", a[5], a[4], a[3], a[2], a[1], a[0])
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The device asked to connect to this PC.
    ConnRequest,
    Connected,
    /// `quiet` = seconds since the last data packet on the link; `lived` = how long the link had been up.
    Disconnected { reason: u8, quiet: Option<f64>, lived: Option<f64> },
    /// The PC asked the device to leave sniff mode; `wait` = seconds until the controller answered (None = no answer).
    SniffExit { wait: Option<f64> },
    /// The controller reported a mode change as failed (status code).
    ModeFailed { status: u8 },
}

#[derive(Clone, Debug)]
pub struct Ev {
    /// Seconds since the first record in the file.
    pub t: f64,
    /// Seconds since the Unix epoch (wall clock).
    pub unix: f64,
    pub addr: Addr,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Snoop {
    pub events: Vec<Ev>,
    pub records: u64,
    pub acl: u64,
    /// Data packets per device: (sent by this PC, received).
    pub acl_by_addr: HashMap<Addr, (u64, u64)>,
    pub first_us: Option<i64>,
    pub last_us: i64,
    pending: Vec<u8>,
    started: bool,
    handles: HashMap<u16, (Addr, f64)>,
    last_acl: HashMap<u16, f64>,
    pending_exit: HashMap<u16, (f64, usize)>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn addr6(b: &[u8]) -> Addr {
    [b[0], b[1], b[2], b[3], b[4], b[5]]
}

impl Snoop {
    /// Adds the next piece of the file (any size, in order). Whole records are parsed; a half-written one waits in
    /// `pending` for the next piece, so a file that is still growing and a huge one read in chunks work the same way.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), String> {
        let mut d = std::mem::take(&mut self.pending);
        d.extend_from_slice(chunk);
        let mut o = 0;
        if !self.started {
            if d.len() < 16 {
                self.pending = d;
                return Ok(());
            }
            if &d[..8] != b"btsnoop\0" {
                return Err("This is not a Bluetooth capture file (a btmon -w file starts with \"btsnoop\").".into());
            }
            if be32(&d[12..16]) != 2001 {
                return Err("This capture is not in btmon's monitor format, so it has no controller events.".into());
            }
            self.started = true;
            o = 16;
        }
        while o + 24 <= d.len() {
            let incl = be32(&d[o + 4..]) as usize;
            if o + 24 + incl > d.len() {
                break; // last record still being written
            }
            let flags = be32(&d[o + 8..]);
            let ts = i64::from_be_bytes([d[o + 16], d[o + 17], d[o + 18], d[o + 19], d[o + 20], d[o + 21], d[o + 22], d[o + 23]]);
            self.record((flags & 0xffff) as u16, ts, &d[o + 24..o + 24 + incl]);
            o += 24 + incl;
        }
        d.drain(..o);
        self.pending = d;
        Ok(())
    }

    pub fn duration(&self) -> f64 {
        self.first_us.map_or(0.0, |f| (self.last_us - f) as f64 / 1e6)
    }

    fn push(&mut self, t: f64, unix: f64, addr: Addr, kind: Kind) -> usize {
        self.events.push(Ev { t, unix, addr, kind });
        self.events.len() - 1
    }

    fn record(&mut self, op: u16, ts: i64, data: &[u8]) {
        self.records += 1;
        let first = *self.first_us.get_or_insert(ts);
        self.last_us = ts;
        let t = (ts - first) as f64 / 1e6;
        let unix = (ts - BTSNOOP_EPOCH_US) as f64 / 1e6;
        match op {
            OP_ACL_TX | OP_ACL_RX if data.len() >= 2 => {
                let h = le16(data) & 0x0fff;
                self.acl += 1;
                self.last_acl.insert(h, t);
                if let Some((a, _)) = self.handles.get(&h) {
                    let e = self.acl_by_addr.entry(*a).or_default();
                    if op == OP_ACL_TX { e.0 += 1 } else { e.1 += 1 }
                }
            }
            OP_COMMAND if data.len() >= 5 && le16(data) == SNIFF_EXIT => {
                let h = le16(&data[3..]) & 0x0fff;
                if let Some((a, _)) = self.handles.get(&h).copied() {
                    let i = self.push(t, unix, a, Kind::SniffExit { wait: None });
                    self.pending_exit.insert(h, (t, i));
                }
            }
            OP_EVENT if data.len() >= 2 => self.event(t, unix, data[0], &data[2..]),
            _ => {}
        }
    }

    fn event(&mut self, t: f64, unix: f64, code: u8, p: &[u8]) {
        match code {
            // Connection Complete: status, handle, address, link type, encryption
            0x03 if p.len() >= 10 && p[0] == 0 => {
                let h = le16(&p[1..]) & 0x0fff;
                let a = addr6(&p[3..]);
                self.handles.insert(h, (a, t));
                self.last_acl.remove(&h);
                if p[9] == 1 {
                    self.push(t, unix, a, Kind::Connected);
                }
            }
            // Connection Request: address, class, link type
            0x04 if p.len() >= 10 && p[9] == 1 => {
                self.push(t, unix, addr6(p), Kind::ConnRequest);
            }
            // Disconnection Complete: status, handle, reason
            0x05 if p.len() >= 4 && p[0] == 0 => {
                let h = le16(&p[1..]) & 0x0fff;
                match self.handles.remove(&h) {
                    Some((a, ct)) => {
                        let quiet = Some(t - self.last_acl.get(&h).copied().unwrap_or(ct));
                        self.push(t, unix, a, Kind::Disconnected { reason: p[3], quiet, lived: Some(t - ct) });
                    }
                    // the link was already up when the capture started: address and age unknown
                    None => {
                        let quiet = self.last_acl.get(&h).map(|l| t - l);
                        self.push(t, unix, UNKNOWN, Kind::Disconnected { reason: p[3], quiet, lived: None });
                    }
                }
                self.last_acl.remove(&h);
                self.pending_exit.remove(&h);
            }
            // Mode Change: status, handle, mode, interval
            0x14 if p.len() >= 6 => {
                let h = le16(&p[1..]) & 0x0fff;
                if p[0] != 0 {
                    if let Some((a, _)) = self.handles.get(&h).copied() {
                        self.push(t, unix, a, Kind::ModeFailed { status: p[0] });
                    }
                }
                if p[3] == 0 || p[0] != 0 {
                    if let Some((t0, i)) = self.pending_exit.remove(&h) {
                        self.events[i].kind = Kind::SniffExit { wait: Some(t - t0) };
                    }
                }
            }
            // LE Meta: connection complete (0x01) or enhanced connection complete (0x0a)
            0x3e if p.len() >= 12 && (p[0] == 0x01 || p[0] == 0x0a) && p[1] == 0 => {
                let h = le16(&p[2..]) & 0x0fff;
                let a = addr6(&p[6..]);
                self.handles.insert(h, (a, t));
                self.last_acl.remove(&h);
                self.push(t, unix, a, Kind::Connected);
            }
            _ => {}
        }
    }

    // ---------- analysis ----------

    pub fn drops(&self) -> impl Iterator<Item = &Ev> {
        self.events.iter().filter(|e| matches!(e.kind, Kind::Disconnected { .. }))
    }

    /// True for a sniff exit the controller answered late or not at all.
    pub fn slow_exit(e: &Ev) -> bool {
        matches!(e.kind, Kind::SniffExit { wait } if wait.map_or(true, |w| w > SLOW_EXIT_S))
    }

    /// Whether a slow or unanswered sniff exit for the same device came shortly before this drop (the cards count this
    /// in a single pass in `per_device`; this form is for tests).
    #[cfg(test)]
    pub fn after_stall(&self, drop: &Ev) -> bool {
        self.events.iter().any(|e| e.addr == drop.addr && Self::slow_exit(e) && e.t <= drop.t && drop.t - e.t <= STALL_WINDOW_S)
    }

    /// Groups of drops separated by quiet stretches longer than `gap` seconds: (start, end, drops).
    pub fn bursts(&self, gap: f64) -> Vec<(f64, f64, usize)> {
        let mut out: Vec<(f64, f64, usize)> = vec![];
        for e in self.drops() {
            match out.last_mut() {
                Some(b) if e.t - b.1 <= gap => {
                    b.1 = e.t;
                    b.2 += 1;
                }
                _ => out.push((e.t, e.t, 1)),
            }
        }
        out
    }

    pub fn per_device(&self) -> Vec<DevStat> {
        let mut addrs: Vec<Addr> = vec![];
        for e in &self.events {
            if !addrs.contains(&e.addr) {
                addrs.push(e.addr);
            }
        }
        let mut out = vec![];
        for a in addrs {
            let mine: Vec<&Ev> = self.events.iter().filter(|e| e.addr == a).collect();
            let drops: Vec<&&Ev> = mine.iter().filter(|e| matches!(e.kind, Kind::Disconnected { .. })).collect();
            let mut reasons: Vec<(u8, usize)> = vec![];
            let mut lives = vec![];
            let mut quiets = vec![];
            for e in &drops {
                if let Kind::Disconnected { reason, quiet, lived } = e.kind {
                    match reasons.iter_mut().find(|r| r.0 == reason) {
                        Some(r) => r.1 += 1,
                        None => reasons.push((reason, 1)),
                    }
                    lives.extend(lived);
                    quiets.extend(quiet);
                }
            }
            let gaps: Vec<f64> = drops.windows(2).map(|w| w[1].t - w[0].t).collect();
            let mut last_slow: Option<f64> = None;
            let mut after_stall = 0;
            for e in &mine {
                if Self::slow_exit(e) {
                    last_slow = Some(e.t);
                } else if matches!(e.kind, Kind::Disconnected { .. }) && last_slow.map_or(false, |t| e.t - t <= STALL_WINDOW_S) {
                    after_stall += 1;
                }
            }
            let (tx, rx) = self.acl_by_addr.get(&a).copied().unwrap_or((0, 0));
            out.push(DevStat {
                addr: a,
                connects: mine.iter().filter(|e| e.kind == Kind::Connected).count(),
                drops: drops.len(),
                after_stall,
                slow_exits: mine.iter().filter(|e| Self::slow_exit(e)).count(),
                exits: mine.iter().filter(|e| matches!(e.kind, Kind::SniffExit { .. })).count(),
                reasons,
                median_gap: median(gaps),
                median_life: median(lives.clone()),
                shortest_life: lives.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))),
                median_quiet: median(quiets),
                tx,
                rx,
            });
        }
        out
    }
}

pub struct DevStat {
    pub addr: Addr,
    pub connects: usize,
    pub drops: usize,
    /// Drops that came within 30 s after a slow or unanswered sniff exit.
    pub after_stall: usize,
    pub exits: usize,
    pub slow_exits: usize,
    pub reasons: Vec<(u8, usize)>,
    pub median_gap: Option<f64>,
    pub median_life: Option<f64>,
    pub shortest_life: Option<f64>,
    pub median_quiet: Option<f64>,
    pub tx: u64,
    pub rx: u64,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2])
}

/// "HH:MM:SS" local time for a Unix time in seconds.
pub fn clock(unix: f64) -> String {
    let secs = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// "YYYY-MM-DD" local date for a Unix time in seconds.
pub fn date(unix: f64) -> String {
    let secs = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

pub fn dur(s: f64) -> String {
    if s < 100.0 { format!("{s:.1} s") } else if s < 6000.0 { format!("{:.1} min", s / 60.0) } else { format!("{:.1} h", s / 3600.0) }
}

/// Seconds since the first record, for the "since start" column.
pub fn offset(e: &Ev) -> String {
    format!("{:.3} s", e.t)
}

/// What happened, without the time and device (the timeline shows those in their own columns).
pub fn message(e: &Ev) -> String {
    match &e.kind {
        Kind::ConnRequest => "asks to connect".into(),
        Kind::Connected => "connected".into(),
        Kind::Disconnected { reason, quiet, lived } => {
            let (name, _, _) = crate::hci::reason(*reason);
            let mut s = format!("DROPPED: {name} (0x{reason:02x})");
            if let Some(l) = lived {
                s += &format!(", link had been up {}", dur(*l));
            }
            if let Some(q) = quiet {
                s += &format!(", no data for the last {}", dur(*q));
            }
            s
        }
        Kind::SniffExit { wait: Some(w) } if *w <= SLOW_EXIT_S => format!("sniff exit answered in {:.2} s", w),
        Kind::SniffExit { wait: Some(w) } => format!("SLOW sniff exit: controller took {:.1} s to answer", w),
        Kind::SniffExit { wait: None } => "SNIFF EXIT UNANSWERED (link dropped, or no reply yet)".into(),
        Kind::ModeFailed { status } => format!("mode change failed (status 0x{status:02x})"),
    }
}

/// One plain-text line describing an event, used in the saved report.
pub fn line(e: &Ev, who: &str) -> String {
    format!("{}  {:>10}  {}  {}", clock(e.unix), offset(e), who, message(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(out: &mut Vec<u8>, op: u16, ts: i64, data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        out.extend((op as u32).to_be_bytes());
        out.extend(0u32.to_be_bytes());
        out.extend((ts + BTSNOOP_EPOCH_US).to_be_bytes());
        out.extend(data);
    }
    fn file() -> Vec<u8> {
        let mut f = b"btsnoop\0".to_vec();
        f.extend(1u32.to_be_bytes());
        f.extend(2001u32.to_be_bytes());
        let a = [1u8, 2, 3, 4, 5, 6];
        // connected on handle 0x100 at t=0
        let mut cc = vec![0x03, 11, 0, 0x00, 0x01];
        cc.extend(a);
        cc.extend([1, 0]);
        rec(&mut f, OP_EVENT, 1_000_000, &cc);
        // data at t=2 s
        rec(&mut f, OP_ACL_RX, 3_000_000, &[0x00, 0x21, 1, 0, 0xaa]);
        // sniff exit at t=5 s, never answered, drop at t=20 s (reason 0x08)
        rec(&mut f, OP_COMMAND, 6_000_000, &[0x04, 0x08, 2, 0x00, 0x01]);
        rec(&mut f, OP_EVENT, 21_000_000, &[0x05, 4, 0, 0x00, 0x01, 0x08]);
        f
    }

    #[test]
    fn parses_drop_after_unanswered_sniff_exit() {
        let f = file();
        let mut s = Snoop::default();
        s.feed(&f).unwrap();
        let drops: Vec<&Ev> = s.drops().collect();
        assert_eq!(drops.len(), 1);
        let Kind::Disconnected { reason, quiet, lived } = drops[0].kind else { panic!() };
        assert_eq!(reason, 0x08);
        assert!((quiet.unwrap() - 18.0).abs() < 1e-6, "last data at t=2 s, drop at t=20 s");
        assert!((lived.unwrap() - 20.0).abs() < 1e-6);
        assert!(s.after_stall(drops[0]));
        assert_eq!(s.acl, 1);
    }

    #[test]
    fn growing_file_and_bad_input() {
        let f = file();
        let mut s = Snoop::default();
        s.feed(&f[..f.len() - 3]).unwrap(); // last record half written
        assert_eq!(s.drops().count(), 0);
        s.feed(&f[f.len() - 3..]).unwrap();
        assert_eq!(s.drops().count(), 1);
        assert!(Snoop::default().feed(b"not a capture file at all").is_err());
    }

    /// `cargo test snoop_dump -- --ignored --nocapture` with AIRSCOPE_SNOOP=<file>
    #[test]
    #[ignore]
    fn snoop_dump() {
        let path = std::env::var("AIRSCOPE_SNOOP").unwrap();
        let mut s = Snoop::default();
        s.feed(&std::fs::read(path).unwrap()).unwrap();
        println!("records {} acl {} events {} drops {}", s.records, s.acl, s.events.len(), s.drops().count());
        for e in s.events.iter().filter(|e| !matches!(e.kind, Kind::SniffExit { .. })).take(40) {
            println!("{}", line(e, &mac(&e.addr)[12..]));
        }
    }
}
