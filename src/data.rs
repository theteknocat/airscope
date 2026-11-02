//! Data collectors. Each runs in its own thread, shells out to a system tool,
//! and publishes into `Shared`. The UI only ever reads the shared snapshot.
use std::collections::{HashMap, HashSet};
use crate::net;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new("timeout").arg("8").arg(cmd).args(args).env("LC_ALL", "C").output().ok()?;
    if o.stdout.is_empty() && !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

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

pub struct Oui(HashMap<u32, String>);

impl Oui {
    pub fn load() -> Self {
        let mut m = HashMap::new();
        if let Ok(s) = std::fs::read_to_string("/usr/share/hwdata/oui.txt") {
            for l in s.lines() {
                if let Some((a, b)) = l.split_once("(hex)") {
                    if let Ok(v) = u32::from_str_radix(&a.trim().replace('-', ""), 16) {
                        m.insert(v, b.trim().to_string());
                    }
                }
            }
        }
        Oui(m)
    }
    pub fn vendor(&self, mac: &str) -> String {
        match parse_mac(mac) {
            Some(b) => {
                if b[0] & 2 != 0 {
                    return "Private / randomized address".into();
                }
                let k = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
                self.0.get(&k).cloned().unwrap_or_else(|| "Unknown vendor".into())
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
                    v.push(d);
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

fn parse_bt(b: &crate::bluez::BtDev, oui: &Oui) -> Dev {
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

fn link_info(c: &crate::hci::Conn, cache: &mut HashMap<String, (String, String)>) -> BtLink {
    let d = crate::hci::link_data(c);
    let (lmp, mfr) = cache.entry(c.mac.clone()).or_insert_with(|| crate::hci::remote_version(c)).clone();
    BtLink { mac: c.mac.clone(), name: String::new(), lq: d.lq, rssi: d.rssi, tpl: d.tpl, afh: d.afh, lmp, mfr }
}

fn bt_loop(h: Handle, oui: Arc<Oui>) {
    let mut cache: HashMap<String, (String, String)> = HashMap::new();
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
                        l.name = d.name.clone();
                        d.connected = true;
                        d.online = true;
                        // Controller RSSI for a connected link is relative to the "golden range" (0 = fine),
                        // so it is not a distance. Use it only to nudge the node outward for display.
                        d.dist_m = Some(1.6 + l.rssi.unwrap_or(0).unsigned_abs() as f32 * 0.25);
                        d.details.push(("Link quality".into(), l.lq.map(|q| format!("{} / 255", q)).unwrap_or("?".into())));
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
                    links.push(l);
                }
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

fn mdns_loop(m: Mdns) {
    loop {
        if let Some(out) = run("avahi-browse", &["-a", "-r", "-t", "-p"]) {
            let mut map: HashMap<String, (String, Vec<String>)> = HashMap::new();
            for l in out.lines() {
                let f: Vec<&str> = l.split(';').collect();
                if f.len() > 7 && f[0] == "=" && f[2] == "IPv4" {
                    let e = map.entry(f[7].to_string()).or_insert((f[6].trim_end_matches(".local").to_string(), vec![]));
                    let svc = f[4].trim_start_matches('_').split('.').next().unwrap_or("").to_string();
                    if !svc.is_empty() && !e.1.contains(&svc) {
                        e.1.push(svc);
                    }
                }
            }
            *m.lock().unwrap() = map;
        }
        std::thread::sleep(Duration::from_secs(45));
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
                let host = if !mhost.is_empty() { mhost } else { dns.clone() };
                let vendor = oui.vendor(&mac);
                let (icon, what, why) = classify(&vendor, &host, &svcs, ip == gw);
                let mut d = Dev::new(&mac, Kind::Lan);
                d.known = icon != Icon::Unknown;
                d.icon = icon;
                d.name = if !host.is_empty() { host.clone() } else { ip.to_string() };
                d.vendor = vendor.clone();
                d.online = state != "STALE" || answered.contains(ip);
                d.connected = ip == gw;
                d.details = vec![
                    ("Type".into(), what.into()),
                    ("IP address".into(), ip.to_string()),
                    ("MAC".into(), mac.clone()),
                    ("Vendor".into(), vendor),
                    ("Hostname".into(), if host.is_empty() { "(none advertised)".into() } else { host }),
                    ("Neighbour state".into(), format!("{} on {}", state, dev)),
                    ("Services".into(), if svcs.is_empty() { "(none seen)".into() } else { svcs.join(", ") }),
                    ("Signal".into(), "n/a: seen over the network, no radio signal measured".into()),
                ];
                d.why = why;
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
    let mdns: Mdns = Arc::new(Mutex::new(HashMap::new()));
    {
        let (h, o) = (h.clone(), oui.clone());
        std::thread::spawn(move || wifi_loop(h, o));
    }
    {
        let (h, o) = (h.clone(), oui.clone());
        std::thread::spawn(move || bt_loop(h, o));
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
