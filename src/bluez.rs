//! Paired/known Bluetooth devices straight from BlueZ over the system D-Bus (replaces parsing `bluetoothctl`).
//! One `GetManagedObjects` call returns every device with all its properties.

use std::collections::HashMap;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

type Props = HashMap<String, OwnedValue>;
type Managed = HashMap<OwnedObjectPath, HashMap<String, Props>>;

pub struct BtDev {
    pub mac: String, // upper case
    pub name: String,
    pub alias: String,
    pub icon: String,
    pub class: u32,
    pub modalias: String,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub battery: Option<i32>,
    pub services: Vec<String>, // human-readable names of the well-known service UUIDs
}

fn get<T: TryFrom<OwnedValue>>(p: &Props, k: &str) -> Option<T> {
    p.get(k)?.try_clone().ok()?.try_into().ok()
}

/// System-bus connection; keep it and reuse it between polls.
pub fn connect() -> Option<Connection> {
    Connection::system().ok()
}

/// All devices BlueZ knows about, or None if BlueZ can't be reached.
pub fn devices(conn: &Connection) -> Option<Vec<BtDev>> {
    let msg = conn.call_method(Some("org.bluez"), "/", Some("org.freedesktop.DBus.ObjectManager"), "GetManagedObjects", &()).ok()?;
    let all: Managed = msg.body().deserialize().ok()?;
    let mut out = vec![];
    for (_, ifaces) in all {
        let Some(p) = ifaces.get("org.bluez.Device1") else { continue };
        let Some(addr) = get::<String>(p, "Address") else { continue };
        let battery = ifaces.get("org.bluez.Battery1").and_then(|b| get::<u8>(b, "Percentage")).map(|v| v as i32);
        let mut services: Vec<String> = get::<Vec<String>>(p, "UUIDs").unwrap_or_default().iter().filter_map(|u| uuid_name(u)).map(|s| s.to_string()).collect();
        services.dedup();
        out.push(BtDev {
            mac: addr.to_uppercase(),
            name: get(p, "Name").unwrap_or_default(),
            alias: get(p, "Alias").unwrap_or_default(),
            icon: get(p, "Icon").unwrap_or_default(),
            class: get(p, "Class").unwrap_or(0),
            modalias: get(p, "Modalias").unwrap_or_default(),
            paired: get(p, "Paired").unwrap_or(false),
            trusted: get(p, "Trusted").unwrap_or(false),
            connected: get(p, "Connected").unwrap_or(false),
            battery,
            services,
        });
    }
    out.sort_by(|a, b| a.mac.cmp(&b.mac)); // D-Bus returns a hash map: keep the order stable
    Some(out)
}

/// Name of a standard 16-bit service UUID (0000xxxx-0000-1000-8000-00805f9b34fb); None for vendor-specific ones.
fn uuid_name(u: &str) -> Option<&'static str> {
    let u = u.to_lowercase();
    if !u.starts_with("0000") || !u.ends_with("-0000-1000-8000-00805f9b34fb") {
        return None;
    }
    Some(match &u[4..8] {
        "1000" => "Service Discovery Server",
        "1101" => "Serial Port",
        "1105" => "OBEX Object Push",
        "1106" => "OBEX File Transfer",
        "1108" => "Headset",
        "110a" => "Audio Source",
        "110b" => "Audio Sink",
        "110c" => "A/V Remote Control Target",
        "110d" => "Advanced Audio Distribution",
        "110e" => "A/V Remote Control",
        "110f" => "A/V Remote Control Controller",
        "1112" => "Headset AG",
        "1115" => "PANU",
        "1116" => "NAP",
        "111e" => "Handsfree",
        "111f" => "Handsfree Audio Gateway",
        "1124" | "1812" => "Human Interface Device",
        "112f" => "Phonebook Access Server",
        "1132" => "Message Access Server",
        "1200" => "PnP Information",
        "1800" => "Generic Access Profile",
        "1801" => "Generic Attribute Profile",
        "180a" => "Device Information",
        "180f" => "Battery Service",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_names() {
        assert_eq!(uuid_name("0000110B-0000-1000-8000-00805F9B34FB"), Some("Audio Sink"));
        assert_eq!(uuid_name("4715650b-5e9d-4ac2-b898-a4fc0aa5df78"), None);
        assert_eq!(uuid_name("00001124-0000-1000-8000-00805f9b34fb"), Some("Human Interface Device"));
    }

    /// Lists the devices BlueZ reports. Run with: cargo test bluez_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bluez_dump() {
        let c = connect().expect("system bus");
        for d in devices(&c).expect("bluez") {
            println!("{} {:?}/{:?} icon={:?} class={:#x} conn={} paired={} trusted={} batt={:?} svc={:?} modalias={:?}", d.mac, d.name, d.alias, d.icon, d.class, d.connected, d.paired, d.trusted, d.battery, d.services, d.modalias);
        }
    }
}
