//! Demo mode (`AIRSCOPE_DEMO=1`): invented Wi-Fi, Bluetooth and LAN data instead of the real sources, for
//! screenshots and trying the app out. Nothing here comes from the machine it runs on. MAC prefixes are real
//! vendor prefixes (so vendors resolve) with made-up endings; SSIDs, names and addresses are fictional.
//! The data goes through the same conversion code as live data, so the info boxes read the same.

use crate::bluez::BtDev;
use crate::data::{attach_link, lan_dev, parse_bt, wifi_dev, BtLink, Handle, Oui};
use crate::nm::Ap;
use std::time::{Duration, Instant};

/// `AIRSCOPE_DEMO=bad` also gives the keyboard a degraded link, to show the red interference halo.
fn degraded() -> bool {
    std::env::var("AIRSCOPE_DEMO").as_deref() == Ok("bad")
}

pub fn on() -> bool {
    std::env::var_os("AIRSCOPE_DEMO").is_some()
}

fn ap(ssid: &str, bssid: &str, chan: u32, strength: u8, rate: u32, security: &str, in_use: bool) -> Ap {
    let freq = match chan {
        1..=13 => 2407 + chan * 5,
        _ => 5000 + chan * 5,
    };
    Ap { ssid: ssid.into(), bssid: bssid.into(), freq, chan, strength, rate_kbps: rate * 1000, security: security.into(), in_use }
}

#[allow(clippy::too_many_arguments)]
fn bt(mac: &str, name: &str, icon: &str, class: u32, modalias: &str, connected: bool, battery: Option<i32>, services: &[&str]) -> BtDev {
    BtDev { mac: mac.into(), name: name.into(), alias: name.into(), icon: icon.into(), class, modalias: modalias.into(), paired: true, trusted: true, connected, battery, services: services.iter().map(|s| s.to_string()).collect() }
}

fn afh(channels: &[std::ops::RangeInclusive<usize>]) -> Vec<bool> {
    (0..79).map(|i| channels.iter().any(|r| r.contains(&i))).collect()
}

/// Fill the shared state once and keep the "last updated" times fresh.
pub fn start(h: &Handle, oui: &Oui) {
    let wifi: Vec<_> = [
        ap("Maple House", "00:0A:EB:3C:91:A0", 1, 94, 286, "WPA2 WPA3", false),
        ap("Maple House", "00:0A:EB:3C:91:A1", 44, 88, 866, "WPA2 WPA3", true),
        ap("Maple House Guest", "00:0A:EB:3C:91:A2", 1, 90, 286, "WPA2", false),
        ap("Cedar Lane", "00:09:5B:2A:7C:10", 6, 68, 144, "WPA2", false),
        ap("Cedar Lane 5G", "00:09:5B:2A:7C:11", 149, 60, 433, "WPA2", false),
        ap("Hiking Club", "00:14:78:55:E2:3C", 11, 58, 144, "WPA2", false),
        ap("CoffeeNook Free WiFi", "00:0F:B5:11:22:33", 6, 44, 72, "", false),
        ap("Riverside_Mesh", "00:1A:11:9B:4E:02", 3, 52, 144, "WPA2", false),
        ap("Studio 7", "00:0A:EB:88:19:6F", 9, 48, 144, "WPA3", false),
        ap("Community WiFi", "00:07:AB:77:10:5E", 11, 40, 72, "WPA2 802.1X", false),
        ap("", "5A:1F:2C:90:44:D1", 11, 36, 72, "WPA2", false),
        ap("Oak & Ivy", "00:09:5B:6E:30:A4", 36, 46, 433, "WPA2", false),
    ]
    .iter()
    .map(|a| wifi_dev(a, oui))
    .collect();

    const AUDIO: [&str; 4] = ["Audio Sink", "Advanced Audio Distribution", "A/V Remote Control", "Handsfree"];
    let mut devs: Vec<_> = [
        bt("AC:80:0A:3D:62:C0", "Studio Headphones", "audio-headphones", 0x240404, "usb:v054Cp0CE6d0100", true, Some(80), &AUDIO),
        bt("E4:11:5B:72:08:4D", "Desk Keyboard", "input-keyboard", 0x002540, "usb:v046DpB342d0100", true, Some(70), &["Human Interface Device", "Battery Service"]),
        bt("5C:BA:37:A1:4E:90", "Game Controller", "input-gaming", 0x002508, "usb:v054Cp09CCd0100", false, None, &["Human Interface Device"]),
        bt("08:EB:ED:19:C3:5A", "Pocket Speaker", "audio-speakers", 0x240414, "", false, None, &["Audio Sink", "Advanced Audio Distribution"]),
    ]
    .iter()
    .map(|b| parse_bt(b, oui))
    .collect();
    let mut links = vec![
        BtLink { mac: "AC:80:0A:3D:62:C0".into(), name: String::new(), lq: Some(255), rssi: Some(0), tpl: Some(4), afh: Some(afh(&[0..=3, 21..=24, 46..=49, 73..=78])), lmp: "5.2 (0xb)".into(), mfr: "Sony (0x012d)".into() },
        BtLink { mac: "E4:11:5B:72:08:4D".into(), name: String::new(), lq: Some(if degraded() { 168 } else { 255 }), rssi: Some(-3), tpl: Some(0), afh: None, lmp: "3.0 (0x5)".into(), mfr: "Broadcom Corporation (15)".into() },
    ];
    for l in links.iter_mut() {
        if let Some(d) = devs.iter_mut().find(|d| d.id == l.mac) {
            attach_link(d, l);
        }
    }

    let svc = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // (ip, mac, host, services, state, online, gateway)
    let lan: Vec<_> = [
        ("192.168.1.1", "00:0A:EB:3C:91:A0", "maple-router", svc(&[]), "REACHABLE", true, true),
        ("192.168.1.5", "D8:3A:DD:19:70:2E", "dns-box", svc(&["ssh"]), "REACHABLE", true, false),
        ("192.168.1.20", "00:11:32:6A:0B:7C", "maple-nas", svc(&["smb", "http", "workstation"]), "REACHABLE", true, false),
        ("192.168.1.41", "00:07:AB:41:D0:62", "Living-Room-TV", svc(&["airplay", "raop"]), "REACHABLE", true, false),
        ("192.168.1.52", "7A:3C:19:E4:5B:08", "Alexs-iPhone", svc(&["remotepairing"]), "STALE", true, false),
        ("192.168.1.60", "00:03:93:8C:11:F2", "Studio-MacBook-Air", svc(&["smb", "airplay", "rfb"]), "REACHABLE", true, false),
        ("192.168.1.70", "00:01:E6:50:9A:3D", "Office-Printer", svc(&["ipp", "printer", "scanner"]), "STALE", true, false),
        ("192.168.1.81", "34:7E:5C:2B:60:9F", "Kitchen-Speaker", svc(&["airplay", "spotify-connect"]), "REACHABLE", true, false),
        ("192.168.1.90", "00:04:1F:AA:10:77", "", svc(&[]), "STALE", true, false),
        ("192.168.1.101", "00:4B:12:6C:90:11", "", svc(&[]), "STALE", false, false),
    ]
    .into_iter()
    .map(|(ip, mac, host, s, state, online, gw)| lan_dev(ip, "eth0", mac, state, online, gw, "", host.to_string(), s, oui))
    .collect();

    let h = h.clone();
    std::thread::spawn(move || loop {
        {
            let mut s = h.lock().unwrap();
            let now = Some(Instant::now());
            s.wifi = wifi.clone();
            s.bt = devs.clone();
            s.links = links.clone();
            s.lan = lan.clone();
            (s.wifi_at, s.bt_at, s.lan_at) = (now, now, now);
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}
