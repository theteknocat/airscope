//! Data collectors. Each runs in its own thread, shells out to a system tool,
//! and publishes into `Shared`. The UI only ever reads the shared snapshot.
use std::collections::{HashMap, HashSet, VecDeque};
use crate::net;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Link quality (0..255 from the controller) as a percentage.
pub fn lq_pct(q: i32) -> i32 {
    (q.clamp(0, 255) * 100 + 127) / 255
}

pub fn hash64(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Wifi,
    Bt,
    Lan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Router,
    Wifi,
    Keyboard,
    Headphones,
    Phone,
    Gamepad,
    Mouse,
    Bluetooth,
    Computer,
    Tv,
    Server,
    Printer,
    Speaker,
    Camera,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Dev {
    pub id: String,
    pub cluster: String,
    pub kind: Kind,
    pub icon: Icon,
    pub known: bool,
    pub name: String,
    pub vendor: String,
    pub dbm: Option<f32>,
    pub dist_m: Option<f32>,
    pub connected: bool,
    pub online: bool,
    pub chan: Option<u32>,
    pub freq: Option<u32>,
    pub details: Vec<(String, String)>,
    pub why: String,
    pub lq: Option<i32>, // Bluetooth link quality as the controller reports it, 0..255; show with lq_pct (stands in for signal strength on connected Bluetooth devices)
    pub trouble: f32, // 0 = healthy .. 1 = real problem (red): drops, quality falling, reconnect loops
    pub caution: f32, // 0 .. 1 = early warning (amber): channels dropped, weak signal, low battery
    pub battery: Option<i32>,
    pub chan_use: Option<usize>, // Bluetooth channels in use of 79 (None = not adaptive: hops over all of them)
    pub rssi: Option<i32>, // Bluetooth RSSI relative to the ideal receive range (0 = inside it)
    pub issue: Vec<(String, String)>, // what is wrong and the evidence, shown behind the ISSUE button in the info box
}

impl Dev {
    fn new(id: &str, kind: Kind) -> Self {
        Dev {
            id: id.to_string(),
            cluster: id.to_string(),
            kind,
            icon: Icon::Unknown,
            known: false,
            name: String::new(),
            vendor: String::new(),
            dbm: None,
            dist_m: None,
            connected: false,
            online: true,
            chan: None,
            freq: None,
            details: vec![],
            why: String::new(),
            lq: None,
            trouble: 0.0,
            caution: 0.0,
            battery: None,
            chan_use: None,
            rssi: None,
            issue: vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub struct BtLink {
    pub mac: String,
    pub name: String,
    pub lq: Option<i32>,
    pub rssi: Option<i32>,
    pub tpl: Option<i32>,
    /// None = AFH disabled/unsupported (hops over all 79 channels).
    pub afh: Option<Vec<bool>>,
    pub lmp: String,
    pub mfr: String,
}

#[derive(Default)]
pub struct Shared {
    pub ver: u64,
    pub wifi: Vec<Dev>,
    pub wifi_at: Option<Instant>,
    pub bt: Vec<Dev>,
    pub links: Vec<BtLink>,
    pub bt_at: Option<Instant>,
    pub lan: Vec<Dev>,
    pub lan_at: Option<Instant>,
    pub errs: HashMap<&'static str, String>,
    pub rescan: bool,
    pub auto_rescan_secs: u64,
}

pub type Handle = Arc<Mutex<Shared>>;

// ---------- helpers ----------

pub fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let p: Vec<&str> = mac.split(|c| c == ':' || c == '-').collect();
    if p.len() != 6 {
        return None;
    }
    let mut b = [0u8; 6];
    for (i, x) in p.iter().enumerate() {
        b[i] = u8::from_str_radix(x, 16).ok()?;
    }
    Some(b)
}

const BUILT_IN_OUI: &str = include_str!("oui.tsv"); // regenerate with scripts/gen-oui.sh

/// Where an imported vendor list is kept.
pub fn user_oui_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".config/airscope/oui.tsv"))
}

fn csv_fields(l: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    for c in l.chars() {
        match c {
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Vendor prefixes from any of: IEEE oui.txt, IEEE oui.csv, Wireshark `manuf`, or plain "AABBCC<tab>Name".
/// Only 24-bit prefixes are used; other lines are skipped.
pub fn parse_oui(text: &str) -> Vec<(u32, String)> {
    let mut out = vec![];
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        if l.is_empty() || l.starts_with('#') || l.starts_with(char::is_whitespace) || l.contains("(base 16)") {
            continue;
        }
        let (key, name) = if let Some((a, b)) = l.split_once("(hex)") {
            (a.trim().to_string(), b.trim().to_string())
        } else if l.starts_with("MA-L,") {
            let f = csv_fields(l);
            if f.len() < 3 {
                continue;
            }
            (f[1].clone(), f[2].trim().to_string())
        } else if l.starts_with("MA-") || l.starts_with("IAB,") || l.starts_with("CID,") {
            continue;
        } else {
            match l.split_once(char::is_whitespace) {
                Some((k, r)) => (k.to_string(), r.rsplit('\t').next().unwrap_or("").trim().to_string()),
                None => continue,
            }
        };
        let hex: String = key.chars().filter(|c| *c != '-' && *c != ':').collect();
        if hex.len() == 6 && !name.is_empty() {
            if let Ok(v) = u32::from_str_radix(&hex, 16) {
                out.push((v, name));
            }
        }
    }
    out
}

/// Vendor names by MAC prefix. Layers, later wins: built-in list, the system's hwdata copy, the user's imported file.
pub struct Oui {
    map: std::sync::RwLock<HashMap<u32, String>>,
    counts: std::sync::RwLock<[usize; 3]>, // prefixes added by: built-in, system, user layer
}

static OUI: std::sync::OnceLock<Arc<Oui>> = std::sync::OnceLock::new();

/// The shared instance (for the UI).
pub fn oui() -> Option<Arc<Oui>> {
    OUI.get().cloned()
}

impl Oui {
    pub fn load() -> Self {
        let o = Oui { map: Default::default(), counts: Default::default() };
        o.reload();
        o
    }
    pub fn reload(&self) {
        let mut m = HashMap::new();
        let mut c = [0usize; 3];
        let layers = [Some(BUILT_IN_OUI.to_string()), std::fs::read_to_string("/usr/share/hwdata/oui.txt").ok(), user_oui_path().and_then(|p| std::fs::read_to_string(p).ok())];
        for (i, text) in layers.iter().enumerate() {
            if let Some(t) = text {
                for (k, v) in parse_oui(t) {
                    if m.insert(k, v).is_none() {
                        c[i] += 1; // only prefixes this layer adds on top of the earlier ones
                    }
                }
            }
        }
        *self.map.write().unwrap() = m;
        *self.counts.write().unwrap() = c;
    }
    /// (total vendors, [built-in, added by system file, added by user file])
    pub fn stats(&self) -> (usize, [usize; 3]) {
        (self.map.read().unwrap().len(), *self.counts.read().unwrap())
    }
    /// Read a downloaded list, keep it as the user's file and use it from now on. Returns the number of entries.
    pub fn import(&self, path: &str) -> Result<usize, String> {
        let bytes = std::fs::read(path.trim().trim_matches('"')).map_err(|e| format!("Can't read that file: {}", e))?;
        let list = parse_oui(&String::from_utf8_lossy(&bytes));
        if list.is_empty() {
            return Err("No vendor entries found. Expected the IEEE oui.txt / oui.csv or a Wireshark manuf file.".into());
        }
        let dest = user_oui_path().ok_or("HOME is not set")?;
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        let tsv: String = list.iter().map(|(k, v)| format!("{:06X}\t{}\n", k, v)).collect();
        std::fs::write(&dest, tsv).map_err(|e| format!("Can't save {}: {}", dest.display(), e))?;
        self.reload();
        Ok(list.len())
    }
    pub fn vendor(&self, mac: &str) -> String {
        match parse_mac(mac) {
            Some(b) => {
                if b[0] & 2 != 0 {
                    return "Private / randomized address".into();
                }
                let k = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
                self.map.read().unwrap().get(&k).cloned().unwrap_or_else(|| "Unknown vendor".into())
            }
            None => "Unknown".into(),
        }
    }
}

pub fn dist_from_dbm(dbm: f32) -> f32 {
    // log-distance path loss: -40 dBm at 1 m, exponent 3 (indoor, walls). Rough on purpose.
    10f32.powf((-40.0 - dbm) / 30.0).clamp(0.6, 70.0)
}

fn set(h: &Handle, f: impl FnOnce(&mut Shared)) {
    let mut s = h.lock().unwrap();
    f(&mut s);
    s.ver += 1;
}

// ---------- Wi-Fi ----------

/// One Wi-Fi access point as a device (shared by the live collector and the demo data).
pub fn wifi_dev(ap: &crate::nm::Ap, oui: &Oui) -> Dev {
        let (inuse, ssid, bssid) = (ap.in_use, ap.ssid.clone(), ap.bssid.clone());
        let chan = Some(ap.chan);
        let freq = Some(ap.freq);
        let pct = ap.strength as f32;
        let sec = ap.security.clone();
        let dbm = pct / 2.0 - 100.0;
        let dist = dist_from_dbm(dbm);
        let vendor = oui.vendor(&bssid);
        let hidden = ssid.is_empty();
        let randomized = vendor.starts_with("Private");
        let band = match freq {
            Some(f) if f < 3000 => "2.4 GHz",
            Some(f) if f < 5900 => "5 GHz",
            Some(_) => "6 GHz",
            None => "?",
        };
        let mut d = Dev::new(&bssid, Kind::Wifi);
        d.cluster = bssid.rsplitn(3, ':').take(2).collect::<Vec<_>>().join(":");
        d.known = !hidden;
        d.icon = if hidden { Icon::Unknown } else { Icon::Router };
        d.name = if hidden { "(hidden network)".into() } else { ssid.clone() };
        d.vendor = vendor.clone();
        d.dbm = Some(dbm);
        d.dist_m = Some(dist);
        d.connected = inuse;
        d.chan = chan;
        d.freq = freq;
        d.details = vec![
            ("Type".into(), "Wi-Fi access point".into()),
            ("SSID".into(), if hidden { "(hidden)".into() } else { ssid.clone() }),
            ("BSSID".into(), bssid.clone()),
            ("Vendor".into(), vendor),
            ("Band / channel".into(), format!("{} / ch {} ({} MHz)", band, chan.map(|c| c.to_string()).unwrap_or("?".into()), freq.unwrap_or(0))),
            ("Rate".into(), format!("{} Mbit/s", ap.rate_kbps / 1000)),
            ("Signal".into(), format!("{}%  (about {:.0} dBm{})", pct, dbm, if pct >= 100.0 { " or stronger; the scale saturates" } else { "" })),
            ("Distance (rough)".into(), format!("about {:.1} m, estimated from signal only", dist)),
            ("Security".into(), if sec.is_empty() { "open".into() } else { sec.clone() }),
        ];
        if let (Some(fr), true) = (freq, band == "2.4 GHz") {
            let lo = (fr as i32 - 10 - 2402).max(0);
            let hi = (fr as i32 + 10 - 2402).min(78);
            d.details.push(("Overlaps Bluetooth ch".into(), format!("{}-{} (20 MHz footprint)", lo, hi)));
        }
        d.why = if hidden && randomized {
            "Hidden network with a randomized address. Typically a phone hotspot or a virtual access point on a mesh node or ISP gateway.".into()
        } else if hidden {
            "Access point that does not broadcast its name. Could be a guest, IoT or backhaul network.".into()
        } else if sec.contains("802.1X") {
            "Enterprise authentication (802.1X): usually ISP community Wi-Fi, an office or a campus network.".into()
        } else if ssid.to_lowercase().contains("mesh") {
            "The name suggests a mesh or IoT access point.".into()
        } else if inuse {
            "The access point this PC is connected to.".into()
        } else {
            "Wi-Fi access point seen in the last scan (cached, not an active probe).".into()
        };
    d
}

fn wifi_loop(h: Handle, oui: Arc<Oui>) {
    let mut last_scan = Instant::now();
    let mut bus = None;
    loop {
        let (req, auto) = {
            let mut s = h.lock().unwrap();
            let r = s.rescan;
            s.rescan = false;
            (r, s.auto_rescan_secs)
        };
        if req || (auto > 0 && last_scan.elapsed().as_secs() >= auto) {
            if bus.is_none() {
                bus = crate::nm::connect();
            }
            if let Some(c) = &bus {
                crate::nm::request_scan(c);
            }
            last_scan = Instant::now();
            std::thread::sleep(Duration::from_secs(3));
        }
        // Cached scan results only: never trigger a scan implicitly (scans briefly disturb Bluetooth on a shared Wi-Fi/BT chip).
        if bus.is_none() {
            bus = crate::nm::connect();
        }
        let aps = bus.as_ref().and_then(|c| crate::nm::access_points(c));
        if aps.is_none() {
            bus = None; // reconnect next time
        }
        match aps {
            Some(aps) => {
                let mut v = vec![];
                for ap in aps {
                    v.push(wifi_dev(&ap, &oui));
                }
                set(&h, |s| {
                    s.wifi = v;
                    s.wifi_at = Some(Instant::now());
                    s.errs.remove("wifi");
                });
            }
            None => set(&h, |s| {
                s.errs.insert("wifi", "Wi-Fi scanning needs NetworkManager (not detected)".into());
            }),
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

// ---------- Bluetooth ----------

fn class_text(class: u32) -> String {
    let major = (class >> 8) & 0x1f;
    let minor = (class >> 2) & 0x3f;
    match major {
        1 => "Computer".into(),
        2 => "Phone".into(),
        3 => "Network access point".into(),
        4 => format!("Audio/Video (minor {})", minor),
        5 => match (class >> 6) & 3 {
            1 => "Peripheral: keyboard".into(),
            2 => "Peripheral: pointing device".into(),
            3 => "Peripheral: keyboard + pointer".into(),
            _ => "Peripheral (HID)".into(),
        },
        6 => "Imaging".into(),
        7 => "Wearable".into(),
        _ => format!("Class 0x{:06x}", class),
    }
}

fn modalias_vendor(m: &str) -> Option<&'static str> {
    let up = m.to_uppercase();
    for (k, v) in [
        ("V046D", "Logitech"),
        ("V054C", "Sony"),
        ("V28DE", "Valve"),
        ("V045E", "Microsoft"),
        ("V05AC", "Apple"),
    ] {
        if up.contains("USB:") && up.contains(k) {
            return Some(v);
        }
    }
    if up.contains("BLUETOOTH:V004C") {
        return Some("Apple");
    }
    None
}

pub fn parse_bt(b: &crate::bluez::BtDev, oui: &Oui) -> Dev {
    let mac = b.mac.as_str();
    let mut d = Dev::new(&b.mac, Kind::Bt);
    let (alias, name, icon, class, modalias) = (b.alias.clone(), b.name.clone(), b.icon.clone(), b.class, b.modalias.clone());
    let (paired, trusted, conn, batt) = (b.paired, b.trusted, b.connected, b.battery);
    let mut uuids = b.services.clone();
    let title = if !alias.is_empty() { alias } else if !name.is_empty() { name } else { mac.to_string() };
    let lname = title.to_lowercase();
    d.icon = if icon.contains("keyboard") || lname.contains("keyboard") {
        Icon::Keyboard
    } else if icon.contains("headset") || icon.contains("headphones") || lname.contains("airpods") || lname.contains("headphone") {
        Icon::Headphones
    } else if icon.contains("mouse") {
        Icon::Mouse
    } else if icon.contains("gaming") || lname.contains("controller") {
        Icon::Gamepad
    } else if icon.contains("phone") {
        Icon::Phone
    } else if icon.contains("computer") {
        Icon::Computer
    } else if icon.contains("audio") || icon.contains("speaker") {
        Icon::Speaker
    } else {
        Icon::Bluetooth
    };
    d.known = true;
    d.name = title;
    let mut vendor = oui.vendor(mac);
    if let Some(v) = modalias_vendor(&modalias) {
        vendor = format!("{} (from device ID)", v);
    }
    d.vendor = vendor.clone();
    d.connected = conn;
    d.online = conn;
    d.details = vec![
        ("Type".into(), "Bluetooth device".into()),
        ("Address".into(), mac.to_uppercase()),
        ("Vendor".into(), vendor),
        ("Connected".into(), if conn { "yes".into() } else { "no (paired but not connected)".into() }),
        ("Paired / trusted".into(), format!("{} / {}", if paired { "yes" } else { "no" }, if trusted { "yes" } else { "no" })),
    ];
    if class != 0 {
        d.details.push(("Device class".into(), class_text(class)));
    }
    d.battery = batt;
    if let Some(b) = batt {
        d.details.push(("Battery".into(), format!("{}%", b)));
    }
    if !uuids.is_empty() {
        uuids.dedup();
        d.details.push(("Services".into(), uuids.join(", ")));
    }
    d.why = format!(
        "Identified from what the device told BlueZ: icon hint \"{}\"{}{}.",
        if icon.is_empty() { "none" } else { &icon },
        if class != 0 { format!(", class {}", class_text(class)) } else { String::new() },
        if uuids.is_empty() { String::new() } else { format!(", services: {}", uuids.iter().take(3).cloned().collect::<Vec<_>>().join(", ")) }
    );
    d
}

// ---------- Bluetooth problems: drop events and link history ----------

#[derive(Clone, Debug)]
pub enum EvKind {
    Connected,
    ConnectFailed(u8),
    Disconnected(u8),
}

/// One thing that happened to a Bluetooth device's link, with the controller's reason code.
#[derive(Clone, Debug)]
pub struct DevEvent {
    pub at: SystemTime,
    pub kind: EvKind,
}

pub type Events = Arc<Mutex<HashMap<String, Vec<DevEvent>>>>;

/// A link-quality / channel-count reading, kept for a few minutes per device.
#[derive(Clone, Copy)]
pub struct Sample {
    pub at: Instant,
    pub lq: Option<i32>,
    pub afh: Option<usize>,
    pub rssi: Option<i32>,
}

pub type History = VecDeque<Sample>;

const HISTORY_SECS: u64 = 1800;
const TROUBLE_WINDOW_SECS: u64 = 900;
const FLAP_WINDOW_SECS: u64 = 600; // this many reconnects within this window count as a reconnect loop
const FLAP_COUNT: usize = 3;
const AFH_DROP: usize = 10; // channels lost versus the device's own recent maximum
const RSSI_WEAK: i32 = -10; // dB below the ideal receive range

fn events_loop(ev: Events) {
    crate::hci::watch(|e| {
        let Some(mac) = e.mac else { return };
        let kind = match (e.connected, e.code) {
            (true, 0) => EvKind::Connected,
            (true, c) => EvKind::ConnectFailed(c),
            (false, c) => EvKind::Disconnected(c),
        };
        let mut m = ev.lock().unwrap();
        let v = m.entry(mac).or_default();
        v.push(DevEvent { at: SystemTime::now(), kind });
        if v.len() > 40 {
            v.remove(0);
        }
    });
}

/// "HH:MM:SS" in local time.
fn clock(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn wifi_context(wifi: &[Dev]) -> String {
    let mut near: Vec<&Dev> = wifi.iter().filter(|w| w.freq.map_or(false, |f| f < 3000) && w.dbm.map_or(false, |d| d > -75.0)).collect();
    near.sort_by(|a, b| b.dbm.partial_cmp(&a.dbm).unwrap_or(std::cmp::Ordering::Equal));
    match near.first() {
        None => "No strong 2.4 GHz Wi-Fi network is visible, so Wi-Fi is an unlikely cause here.".into(),
        Some(s) => format!(
            "{} 2.4 GHz Wi-Fi network(s) above -75 dBm share the band; the strongest is {} (ch {}, {:.0} dBm). Other common sources: microwave ovens, cordless phones, USB 3 devices and other Bluetooth gear.",
            near.len(),
            if s.name.is_empty() { "(hidden)" } else { &s.name },
            s.chan.unwrap_or(0),
            s.dbm.unwrap_or(0.0)
        ),
    }
}

fn ev_code(e: &DevEvent) -> Option<u8> {
    match e.kind {
        EvKind::Disconnected(c) | EvKind::ConnectFailed(c) => Some(c),
        EvKind::Connected => None,
    }
}

/// Works out what is wrong with a device (if anything) from its drop events, link history and state:
/// red `trouble` for real problems (lost links, quality falling, reconnect loops), amber `caution` for early
/// warnings (channels dropped, weak signal, low battery). The explanation goes into `d.issue`.
pub fn apply_events(d: &mut Dev, ev: &[DevEvent], hist: &History, wifi: &[Dev]) {
    let now = SystemTime::now();
    let age = |e: &DevEvent| now.duration_since(e.at).map(|x| x.as_secs()).unwrap_or(0);
    let mut diagnosis: Option<(String, String)> = None;
    let mut notes: Vec<(String, String)> = vec![]; // flagged findings
    let mut rows: Vec<(String, String)> = vec![]; // supporting evidence

    // red: link lost for a radio-type reason
    let bad: Vec<&DevEvent> = ev.iter().filter(|e| age(e) < TROUBLE_WINDOW_SECS && ev_code(e).map_or(false, |c| crate::hci::reason(c).2)).collect();
    if let Some(last) = bad.last() {
        let code = ev_code(last).unwrap_or(0);
        let (name, meaning, _) = crate::hci::reason(code);
        let mut text = format!("Last problem: {}. {}", name, meaning);
        if bad.len() >= 2 {
            text += &format!(" It has happened {} times in the last 15 minutes, so this is a pattern rather than a one-off.", bad.len());
        }
        if matches!(code, 0x08 | 0x22 | 0x3E | 0x3D) {
            if let Some(b) = d.battery.filter(|b| *b <= 20) {
                text += &format!(" The battery is at {}%, which is a likelier cause than radio interference.", b);
            }
            text += &format!(" {}", wifi_context(wifi));
        }
        diagnosis = Some(("What it means".into(), text));
        let fresh = (1.0 - age(last) as f32 / TROUBLE_WINDOW_SECS as f32).max(0.25);
        d.trouble = d.trouble.max((0.5 + 0.25 * (bad.len() as f32 - 1.0)).min(1.0) * fresh);
    }
    // red: link quality fell well below this device's own recent best
    let peak = hist.iter().filter_map(|s| s.lq).max();
    if let (Some(cur), Some(pk)) = (d.lq, peak) {
        if hist.len() >= 5 && (cur as f32) < pk as f32 * LINK_DROP {
            d.trouble = d.trouble.max(((pk - cur) as f32 / (pk as f32 * 0.5)).clamp(0.25, 1.0));
            d.issue.push(("Link health".into(), format!("DEGRADED: link quality fell to {}% (it was {}% earlier). Packets are being lost or retried, which is what interference looks like.", lq_pct(cur), lq_pct(pk))));
        }
    }
    // red: reconnecting over and over
    let connects = ev.iter().filter(|e| matches!(e.kind, EvKind::Connected) && age(e) < FLAP_WINDOW_SECS).count();
    if connects >= FLAP_COUNT {
        d.trouble = d.trouble.max(0.7);
        notes.push(("Reconnecting".into(), format!("Connected {} times in the last 10 minutes. A healthy device stays connected; repeated reconnects mean the link keeps failing and recovering, so it is unreliable even if it seems to work.", connects)));
    }
    // amber: the controller stopped using channels it used earlier (it found noise there)
    let chans: Vec<usize> = hist.iter().filter_map(|s| s.afh).collect();
    if let (Some(&cur), Some(&hi)) = (chans.last(), chans.iter().max()) {
        if chans.len() >= 3 && hi >= cur + AFH_DROP {
            d.caution = d.caution.max(0.5);
            notes.push(("Channel map".into(), format!("Now using {} of 79 channels, down from {}. The device found noise on the channels it stopped using and is hopping around it.", cur, hi)));
        }
    }
    // amber: signal weak (RSSI is relative to the ideal range: 0 is fine, negative is too weak)
    let rs: Vec<i32> = hist.iter().rev().take(5).filter_map(|s| s.rssi).collect();
    if rs.len() >= 5 && rs.iter().sum::<i32>() / (rs.len() as i32) <= RSSI_WEAK {
        d.caution = d.caution.max(0.5);
        notes.push(("Weak signal".into(), format!("Signal is {} dB below the ideal receive range. Moving the device closer, or clearing what is between it and the Bluetooth antenna, usually helps.", -(rs.iter().sum::<i32>() / rs.len() as i32))));
    }
    // amber: low battery
    if let Some(b) = d.battery.filter(|b| *b <= 15) {
        d.caution = d.caution.max(0.5);
        notes.push(("Battery".into(), format!("Battery at {}%. A weak battery commonly causes dropouts and slow reconnects.", b)));
    }

    for e in ev.iter().rev().take(4) {
        let text = match e.kind {
            EvKind::Connected => "Connected".to_string(),
            EvKind::ConnectFailed(c) => format!("Connection attempt failed: {} (0x{:02x})", crate::hci::reason(c).0, c),
            EvKind::Disconnected(c) => format!("Disconnected: {} (0x{:02x})", crate::hci::reason(c).0, c),
        };
        rows.push((clock(e.at), text));
    }
    let lqs: Vec<i32> = hist.iter().filter_map(|s| s.lq).collect();
    if lqs.len() >= 3 {
        let (min, avg) = (*lqs.iter().min().unwrap(), lqs.iter().sum::<i32>() / lqs.len() as i32);
        let span = hist.back().zip(hist.front()).map(|(b, f)| b.at.duration_since(f.at).as_secs()).unwrap_or(0);
        rows.push(("Link history".into(), format!("last {} min: link quality lowest {}%, average {}%", (span / 60).max(1), lq_pct(min), lq_pct(avg))));
    }
    // only worth showing when something is flagged: health row, diagnosis, other findings, then the evidence
    if d.trouble > 0.0 || d.caution > 0.0 {
        d.issue.extend(diagnosis);
        d.issue.extend(notes);
        d.issue.extend(rows);
    } else {
        d.issue.clear();
    }
}

fn link_info(c: &crate::hci::Conn, cache: &mut HashMap<String, (String, String)>) -> BtLink {
    let d = crate::hci::link_data(c);
    let (lmp, mfr) = cache.entry(c.mac.clone()).or_insert_with(|| crate::hci::remote_version(c)).clone();
    BtLink { mac: c.mac.clone(), name: String::new(), lq: d.lq, rssi: d.rssi, tpl: d.tpl, afh: d.afh, lmp, mfr }
}

/// A link counts as degraded when its quality falls below this share of the device's own recent best.
/// (Link quality is vendor-defined and varies with distance, so a fixed number can't say what is normal.)
const LINK_DROP: f32 = 0.75;

/// Adds the live link data (quality, hopping map, versions) to a Bluetooth device.
pub fn attach_link(d: &mut Dev, l: &mut BtLink) {
        l.name = d.name.clone();
        d.connected = true;
        d.online = true;
        // Controller RSSI for a connected link is relative to the "golden range" (0 = fine),
        // so it is not a distance. Use it only to nudge the node outward for display.
        d.dist_m = Some(1.6 + l.rssi.unwrap_or(0).unsigned_abs() as f32 * 0.25);
        d.lq = l.lq;
        d.rssi = l.rssi;
        d.chan_use = l.afh.as_ref().map(|m| m.iter().filter(|x| **x).count());
        d.details.push((
            "RSSI (relative)".into(),
            l.rssi.map(|r| format!("{} (0 = inside the ideal range; not a distance)", r)).unwrap_or("?".into()),
        ));
        if let Some(t) = l.tpl {
            d.details.push(("TX power".into(), format!("{} dBm", t)));
        }
        if !l.lmp.is_empty() {
            d.details.push(("Bluetooth version".into(), format!("LMP {}", l.lmp)));
        }
        if !l.mfr.is_empty() {
            d.details.push(("Chip maker".into(), l.mfr.clone()));
        }
        d.details.push((
            "Channel hopping".into(),
            match &l.afh {
                Some(m) => format!("adaptive: {} of 79 channels in use", m.iter().filter(|x| **x).count()),
                None => "not adaptive: uses all 79 channels, cannot avoid busy ones".into(),
            },
        ));
}

fn bt_loop(h: Handle, oui: Arc<Oui>, events: Events) {
    let mut cache: HashMap<String, (String, String)> = HashMap::new();
    let mut hist: HashMap<String, History> = HashMap::new();
    let mut bus = None;
    loop {
        let mut devs = vec![];
        let mut links = vec![];
        if bus.is_none() {
            bus = crate::bluez::connect();
        }
        let listing = bus.as_ref().and_then(|c| crate::bluez::devices(c));
        if listing.is_none() {
            bus = None; // reconnect next time (BlueZ may have restarted)
        }
        if let Some(list) = &listing {
            devs.extend(list.iter().map(|b| parse_bt(b, &oui)));
        }
        {
            for conn in crate::hci::connections() {
                {
                    let mut l = link_info(&conn, &mut cache);
                    if let Some(d) = devs.iter_mut().find(|d| d.id == l.mac) {
                        attach_link(d, &mut l);
                    }
                    let hs = hist.entry(l.mac.clone()).or_default();
                    hs.push_back(Sample { at: Instant::now(), lq: l.lq, afh: l.afh.as_ref().map(|m| m.iter().filter(|x| **x).count()), rssi: l.rssi });
                    links.push(l);
                }
            }
        }
        // forget old readings, then explain any problems (drop reasons, link history, Wi-Fi around)
        for hs in hist.values_mut() {
            while hs.front().map_or(false, |x| x.at.elapsed().as_secs() > HISTORY_SECS) {
                hs.pop_front();
            }
        }
        {
            let wifi = h.lock().unwrap().wifi.clone();
            let evs = events.lock().unwrap().clone();
            for d in devs.iter_mut() {
                apply_events(d, evs.get(&d.id).map(|v| v.as_slice()).unwrap_or(&[]), hist.get(&d.id).unwrap_or(&History::new()), &wifi);
            }
        }
        set(&h, |s| {
            s.bt = devs;
            s.links = links;
            s.bt_at = Some(Instant::now());
            if listing.is_none() {
                s.errs.insert("bt", "BlueZ not reachable".into());
            } else {
                s.errs.remove("bt");
            }
        });
        std::thread::sleep(Duration::from_secs(3));
    }
}

// ---------- LAN ----------

type Mdns = Arc<Mutex<HashMap<String, (String, Vec<String>)>>>;

/// mDNS/DNS-SD discovery with the mdns-sd crate (replaces `avahi-browse`): ask which service types exist,
/// browse each one, and keep ip -> (host name, service names) up to date as services come and go.
fn mdns_loop(m: Mdns) {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    type Inst = HashMap<String, (Vec<String>, String, String)>; // full name -> (ipv4s, host, service)
    let Ok(daemon) = ServiceDaemon::new() else { return }; // no mDNS: LAN still works from ARP + DNS
    let Ok(meta) = daemon.browse("_services._dns-sd._udp.local.") else { return };
    let inst: Arc<Mutex<Inst>> = Arc::new(Mutex::new(HashMap::new()));
    let publish = |inst: &Inst, m: &Mdns| {
        let mut map: HashMap<String, (String, Vec<String>)> = HashMap::new();
        for (ips, host, svc) in inst.values() {
            for ip in ips {
                let e = map.entry(ip.clone()).or_insert((host.clone(), vec![]));
                if !svc.is_empty() && !e.1.contains(svc) {
                    e.1.push(svc.clone());
                }
            }
        }
        *m.lock().unwrap() = map;
    };
    let mut seen: HashSet<String> = HashSet::new();
    while let Ok(ev) = meta.recv() {
        let ServiceEvent::ServiceFound(_, ty) = ev else { continue };
        if !seen.insert(ty.clone()) {
            continue;
        }
        let Ok(rx) = daemon.browse(&ty) else { continue };
        let (inst, m) = (inst.clone(), m.clone());
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                let mut i = inst.lock().unwrap();
                match ev {
                    ServiceEvent::ServiceResolved(r) => {
                        let svc = r.ty_domain.trim_start_matches('_').split('.').next().unwrap_or("").to_string();
                        let host = r.host.trim_end_matches('.').trim_end_matches(".local").to_string();
                        i.insert(r.fullname.clone(), (r.get_addresses_v4().iter().map(|a| a.to_string()).collect(), host, svc));
                    }
                    ServiceEvent::ServiceRemoved(_, full) => {
                        i.remove(&full);
                    }
                    _ => continue,
                }
                publish(&i, &m);
            }
        });
    }
}

fn classify(vendor: &str, host: &str, svcs: &[String], gw: bool) -> (Icon, &'static str, String) {
    let has = |k: &str| svcs.iter().any(|s| s.contains(k));
    let v = vendor.to_lowercase();
    let hn = host.to_lowercase();
    if gw {
        return (Icon::Router, "Router / gateway", "This is the default gateway, so it routes this PC's traffic.".into());
    }
    if has("googlecast") {
        return (Icon::Tv, "Google Cast device", "Advertises Google Cast over mDNS (Chromecast, Android TV or a cast speaker).".into());
    }
    if has("airplay") || has("raop") {
        return (Icon::Speaker, "AirPlay device", "Advertises AirPlay over mDNS (Apple TV, HomePod or an AirPlay speaker/TV).".into());
    }
    if has("ipp") || has("printer") || has("pdl-datastream") {
        return (Icon::Printer, "Printer", "Advertises a printing service over mDNS.".into());
    }
    if has("smb") || has("afpovertcp") || has("nfs") || v.contains("synology") || v.contains("qnap") {
        return (Icon::Server, "File server / NAS", "Advertises a file-sharing service or is made by a NAS vendor.".into());
    }
    if has("hap") {
        return (Icon::Unknown, "HomeKit accessory", "Advertises HomeKit (HAP) over mDNS.".into());
    }
    if has("spotify-connect") {
        return (Icon::Speaker, "Spotify Connect speaker", "Advertises Spotify Connect over mDNS.".into());
    }
    if v.contains("raspberry") || hn.contains("pi") && hn.contains("hole") {
        return (Icon::Server, "Small server (Raspberry Pi / DNS)", "Vendor or hostname suggests a Raspberry Pi or Pi-hole.".into());
    }
    if has("workstation") || has("ssh") {
        return (Icon::Computer, "Computer", "Advertises a workstation or SSH service over mDNS.".into());
    }
    if v.contains("private") {
        return (Icon::Unknown, "Private/virtual address", "Randomized or virtual MAC: a phone with private Wi-Fi address, a container, or a VM. The vendor cannot be read.".into());
    }
    if v.contains("apple") {
        return (Icon::Phone, "Apple device", "Vendor is Apple; could be a phone, tablet, Mac or TV.".into());
    }
    (Icon::Unknown, "Unidentified device", "No services advertised and the vendor alone is not specific enough.".into())
}

/// One LAN neighbour as a device (shared by the live collector and the demo data).
pub fn lan_dev(ip: &str, dev: &str, mac: &str, state: &str, online: bool, is_gw: bool, dns: &str, mhost: String, svcs: Vec<String>, oui: &Oui) -> Dev {
    let host = if !mhost.is_empty() { mhost } else { dns.to_string() };
    let vendor = oui.vendor(mac);
    let (icon, what, why) = classify(&vendor, &host, &svcs, is_gw);
    let mut d = Dev::new(mac, Kind::Lan);
    d.known = icon != Icon::Unknown;
    d.icon = icon;
    d.name = if !host.is_empty() { host.clone() } else { ip.to_string() };
    d.vendor = vendor.clone();
    d.online = online;
    d.connected = is_gw;
    d.details = vec![
        ("Type".into(), what.into()),
        ("IP address".into(), ip.to_string()),
        ("MAC".into(), mac.to_string()),
        ("Vendor".into(), vendor),
        ("Hostname".into(), if host.is_empty() { "(none advertised)".into() } else { host }),
        ("Neighbour state".into(), format!("{} on {}", state, dev)),
        ("Services".into(), if svcs.is_empty() { "(none seen)".into() } else { svcs.join(", ") }),
        ("Signal".into(), "n/a: seen over the network, no radio signal measured".into()),
    ];
    d.why = why;
    d
}

fn lan_loop(h: Handle, oui: Arc<Oui>, mdns: Mdns) {
    let mut names: HashMap<String, String> = HashMap::new();
    loop {
        let gw = net::gateway().map(|g| g.to_string()).unwrap_or_default();
        let mut v = vec![];
        // keep real LAN neighbours: not VPN/container interfaces, not multicast/broadcast/failed entries
        let neigh: Vec<net::Neigh> = net::neighbours().into_iter().filter(|n| !net::is_virtual(&n.dev) && !matches!(n.state, "NOARP" | "FAILED" | "INCOMPLETE") && n.mac != "00:00:00:00:00:00").collect();
        // STALE only means "not talked to recently": ask those devices directly. If the system forbids
        // unprivileged ping (None), give them the benefit of the doubt.
        let answered: HashSet<String> = std::thread::scope(|sc| {
            let hs: Vec<_> = neigh
                .iter()
                .filter(|n| n.state == "STALE")
                .map(|n| {
                    let ip = n.ip;
                    sc.spawn(move || (ip.to_string(), net::ping(ip, Duration::from_secs(1)).unwrap_or(true)))
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).filter(|(_, ok)| *ok).map(|(ip, _)| ip).collect()
        });
        {
            let mm = mdns.lock().unwrap().clone();
            for n in &neigh {
                let (ip, dev, mac, state) = (n.ip.to_string(), n.dev.as_str(), n.mac.clone(), n.state);
                let ip = ip.as_str();
                let dns = names.entry(ip.to_string()).or_insert_with(|| net::reverse_name(n.ip).unwrap_or_default()).clone();
                let (mhost, svcs) = mm.get(ip).cloned().unwrap_or_default();
                let d = lan_dev(ip, dev, &mac, state, state != "STALE" || answered.contains(ip), ip == gw, &dns, mhost, svcs, &oui);
                v.push(d);
            }
        }
        set(&h, |s| {
            s.lan = v;
            s.lan_at = Some(Instant::now());
        });
        std::thread::sleep(Duration::from_secs(5));
    }
}

pub fn spawn() -> Handle {
    let h: Handle = Arc::new(Mutex::new(Shared::default()));
    let oui = Arc::new(Oui::load());
    let _ = OUI.set(oui.clone());
    if crate::demo::on() {
        crate::demo::start(&h, &oui);
        return h;
    }
    let mdns: Mdns = Arc::new(Mutex::new(HashMap::new()));
    let events: Events = Arc::new(Mutex::new(HashMap::new()));
    {
        let (h, o) = (h.clone(), oui.clone());
        std::thread::spawn(move || wifi_loop(h, o));
    }
    {
        let (h, o, e) = (h.clone(), oui.clone(), events.clone());
        std::thread::spawn(move || bt_loop(h, o, e));
    }
    {
        let e = events.clone();
        std::thread::spawn(move || events_loop(e));
    }
    {
        let m = mdns.clone();
        std::thread::spawn(move || mdns_loop(m));
    }
    {
        let (h, o) = (h.clone(), oui.clone());
        std::thread::spawn(move || lan_loop(h, o, mdns));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oui_formats() {
        let ieee = "28-6F-B9   (hex)\t\tNokia Shanghai Bell Co., Ltd.\n286FB9     (base 16)\t\tNokia Shanghai Bell Co., Ltd.\n\t\t\t\tNo.388 Road\n";
        assert_eq!(parse_oui(ieee), vec![(0x286FB9, "Nokia Shanghai Bell Co., Ltd.".to_string())]);
        let csv = "Registry,Assignment,Organization Name,Organization Address\nMA-L,286FB9,\"Nokia, Ltd.\",Road\nMA-S,70B3D5E0A,Small Co,Street\n";
        assert_eq!(parse_oui(csv), vec![(0x286FB9, "Nokia, Ltd.".to_string())]);
        let manuf = "# comment\n00:00:00\tXerox\tXerox Corporation\n00:1B:C5:00:00:00/36\tConverging\tConverging Systems\n";
        assert_eq!(parse_oui(manuf), vec![(0, "Xerox Corporation".to_string())]);
        assert_eq!(parse_oui("AABBCC\tSome Vendor\n"), vec![(0xAABBCC, "Some Vendor".to_string())]);
        assert!(parse_oui(BUILT_IN_OUI).len() > 30000);
    }

    #[test]
    fn lq_percent() {
        assert_eq!((lq_pct(255), lq_pct(0), lq_pct(132), lq_pct(300)), (100, 0, 52, 100));
    }

    #[test]
    fn link_trouble_is_relative() {
        let hist = |qs: &[i32]| -> History { qs.iter().map(|q| Sample { at: Instant::now(), lq: Some(*q), afh: None, rssi: None }).collect() };
        let run = |qs: &[i32]| {
            let mut d = Dev::new("AA:BB:CC:DD:EE:FF", Kind::Bt);
            d.lq = qs.last().copied();
            apply_events(&mut d, &[], &hist(qs), &[]);
            d.trouble
        };
        assert_eq!(run(&[132; 10]), 0.0, "a steady low reading is normal for that device");
        assert!(run(&[255, 255, 250, 240, 200, 150]) > 0.0, "a fall from its own best is a problem");
        assert_eq!(run(&[255, 250]), 0.0, "too little history to judge");
    }

    #[test]
    fn warnings_and_loops() {
        let now = SystemTime::now();
        let mut d = Dev::new("AA:BB:CC:DD:EE:FF", Kind::Bt);
        let flap: Vec<DevEvent> = (0..3).map(|i| DevEvent { at: now - Duration::from_secs(60 * i), kind: EvKind::Connected }).collect();
        apply_events(&mut d, &flap, &History::new(), &[]);
        assert!(d.trouble > 0.0 && d.caution == 0.0, "three reconnects in ten minutes is a problem");

        let mut d = Dev::new("AA:BB:CC:DD:EE:FF", Kind::Bt);
        d.battery = Some(10);
        apply_events(&mut d, &[], &History::new(), &[]);
        assert!(d.trouble == 0.0 && d.caution > 0.0, "low battery is only a warning");

        let mut d = Dev::new("AA:BB:CC:DD:EE:FF", Kind::Bt);
        let h: History = [26, 26, 14].iter().map(|n| Sample { at: Instant::now(), lq: None, afh: Some(*n), rssi: None }).collect();
        apply_events(&mut d, &[], &h, &[]);
        assert!(d.trouble == 0.0 && d.caution > 0.0, "losing 12 channels is a warning");

        let mut d = Dev::new("AA:BB:CC:DD:EE:FF", Kind::Bt);
        let h: History = (0..5).map(|_| Sample { at: Instant::now(), lq: None, afh: None, rssi: Some(-14) }).collect();
        apply_events(&mut d, &[], &h, &[]);
        assert!(d.caution > 0.0, "a sustained weak signal is a warning");
    }

    #[test]
    fn oui_import() {
        let dir = std::env::temp_dir().join(format!("airscope-oui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("HOME", &dir) }; // keep the user's real config untouched
        let src = dir.join("new.txt");
        std::fs::write(&src, "04-AB-CD   (hex)\t\tBrand New Vendor Inc\n").unwrap();
        let o = Oui::load();
        assert_eq!(o.vendor("04:AB:CD:00:00:01"), "Unknown vendor");
        assert_eq!(o.import(src.to_str().unwrap()), Ok(1));
        assert_eq!(o.vendor("04:AB:CD:00:00:01"), "Brand New Vendor Inc");
        assert!(o.import("/nonexistent").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Prints what mDNS discovery finds after 10 s; compare with `avahi-browse -a -r -t -p`.
    /// Run with: cargo test mdns_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn mdns_dump() {
        let m: Mdns = Arc::new(Mutex::new(HashMap::new()));
        let m2 = m.clone();
        std::thread::spawn(move || mdns_loop(m2));
        std::thread::sleep(Duration::from_secs(10));
        let mut v: Vec<_> = m.lock().unwrap().clone().into_iter().collect();
        v.sort();
        for (ip, (host, svcs)) in v {
            println!("{} {} {:?}", ip, host, svcs);
        }
    }
}
