//! Wi-Fi access points straight from NetworkManager over the system D-Bus (replaces parsing `nmcli`).
//! One `GetManagedObjects` call returns every access point NetworkManager has cached from its last scan.

use std::collections::HashMap;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

type Props = HashMap<String, OwnedValue>;
type Managed = HashMap<OwnedObjectPath, HashMap<String, Props>>;

const NM: &str = "org.freedesktop.NetworkManager";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";

pub struct Ap {
    pub ssid: String, // empty = hidden
    pub bssid: String, // upper case
    pub freq: u32,    // MHz
    pub chan: u32,
    pub strength: u8, // percent, as nmcli's SIGNAL
    pub rate_kbps: u32,
    pub security: String, // like nmcli: "WPA2", "WPA1 WPA2 802.1X", "WPA2 WPA3", ... ("" = open)
    pub in_use: bool,
}

fn get<T: TryFrom<OwnedValue>>(p: &Props, k: &str) -> Option<T> {
    p.get(k)?.try_clone().ok()?.try_into().ok()
}

pub fn connect() -> Option<Connection> {
    Connection::system().ok()
}

fn managed(conn: &Connection) -> Option<Managed> {
    let msg = conn.call_method(Some(NM), "/org/freedesktop", Some("org.freedesktop.DBus.ObjectManager"), "GetManagedObjects", &()).ok()?;
    msg.body().deserialize().ok()
}

/// Channel number from a frequency in MHz.
pub fn channel(freq: u32) -> u32 {
    match freq {
        2484 => 14,
        2400..=2483 => (freq - 2407) / 5,
        5000..=5924 => (freq - 5000) / 5,
        5925..=7125 => (freq - 5950) / 5, // 6 GHz
        _ => 0,
    }
}

/// Same wording as nmcli's SECURITY column.
pub fn security(flags: u32, wpa: u32, rsn: u32) -> String {
    const PSK: u32 = 0x100;
    const EAP: u32 = 0x200;
    const SAE: u32 = 0x400;
    const OWE: u32 = 0x800 | 0x1000;
    let mut s: Vec<&str> = vec![];
    if flags & 1 != 0 && wpa == 0 && rsn == 0 {
        s.push("WEP");
    }
    if wpa != 0 {
        s.push("WPA1");
    }
    if rsn & (PSK | EAP) != 0 {
        s.push("WPA2");
    }
    if rsn & SAE != 0 {
        s.push("WPA3");
    }
    if rsn & OWE != 0 {
        s.push("OWE");
    }
    if (wpa | rsn) & EAP != 0 {
        s.push("802.1X");
    }
    s.join(" ")
}

/// All access points from the last scan, or None if NetworkManager can't be reached.
pub fn access_points(conn: &Connection) -> Option<Vec<Ap>> {
    let all = managed(conn)?;
    // the AP each wireless device is connected to
    let active: Vec<String> = all.values().filter_map(|i| i.get(WIRELESS)).filter_map(|p| get::<OwnedObjectPath>(p, "ActiveAccessPoint")).map(|p| p.to_string()).collect();
    let mut out = vec![];
    for (path, ifaces) in &all {
        let Some(p) = ifaces.get(AP) else { continue };
        let freq: u32 = get(p, "Frequency").unwrap_or(0);
        out.push(Ap {
            ssid: String::from_utf8_lossy(&get::<Vec<u8>>(p, "Ssid").unwrap_or_default()).into_owned(),
            bssid: get::<String>(p, "HwAddress").unwrap_or_default().to_uppercase(),
            freq,
            chan: channel(freq),
            strength: get(p, "Strength").unwrap_or(0),
            rate_kbps: get(p, "MaxBitrate").unwrap_or(0),
            security: security(get(p, "Flags").unwrap_or(0), get(p, "WpaFlags").unwrap_or(0), get(p, "RsnFlags").unwrap_or(0)),
            in_use: active.iter().any(|a| a == path.as_str()),
        });
    }
    // strongest first, like nmcli; the order must be stable between polls
    out.sort_by(|a, b| b.strength.cmp(&a.strength).then(a.bssid.cmp(&b.bssid)));
    Some(out)
}

/// Ask every Wi-Fi device to scan (NetworkManager may refuse if it just did; that's fine).
pub fn request_scan(conn: &Connection) {
    let Some(all) = managed(conn) else { return };
    for (path, ifaces) in &all {
        if ifaces.contains_key(WIRELESS) {
            let opts: HashMap<String, Value> = HashMap::new();
            let _ = conn.call_method(Some(NM), path.as_str(), Some(WIRELESS), "RequestScan", &(opts,));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels() {
        assert_eq!(channel(2412), 1);
        assert_eq!(channel(2462), 11);
        assert_eq!(channel(2484), 14);
        assert_eq!(channel(5180), 36);
        assert_eq!(channel(5785), 157);
        assert_eq!(channel(5955), 1);
    }

    #[test]
    fn security_text() {
        assert_eq!(security(1, 0, 0x100), "WPA2");
        assert_eq!(security(1, 0x100, 0x300), "WPA1 WPA2 802.1X");
        assert_eq!(security(1, 0, 0x500), "WPA2 WPA3");
        assert_eq!(security(0, 0, 0), "");
        assert_eq!(security(1, 0, 0), "WEP");
    }

    /// Requests a real scan and prints NetworkManager's LastScan before and after (ms since boot).
    /// Run with: cargo test nm_rescan -- --ignored --nocapture
    #[test]
    #[ignore]
    fn nm_rescan() {
        let c = connect().expect("system bus");
        let last = |c: &Connection| -> i64 {
            let all = managed(c).expect("nm");
            all.values().filter_map(|i| i.get(WIRELESS)).filter_map(|p| get::<i64>(p, "LastScan")).max().unwrap_or(-1)
        };
        let before = last(&c);
        request_scan(&c);
        std::thread::sleep(std::time::Duration::from_secs(5));
        let after = last(&c);
        println!("LastScan before={} after={} advanced_by={} ms", before, after, after - before);
    }

    /// Lists what NetworkManager reports. Compare with `nmcli dev wifi list --rescan no`.
    /// Run with: cargo test nm_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn nm_dump() {
        let c = connect().expect("system bus");
        for a in access_points(&c).expect("networkmanager") {
            println!("{}|{:?}|{}|{}|{}|{}|{} Mbit/s|{}", if a.in_use { "*" } else { " " }, a.ssid, a.bssid, a.chan, a.freq, a.strength, a.rate_kbps / 1000, a.security);
        }
    }
}
