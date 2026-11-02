# Airscope — project notes

Native Linux GUI (Rust, egui/eframe 0.37, glow) visualising Wi-Fi / Bluetooth / LAN around peter-pc as a radar. Repo: private `theteknocat/airscope`, branch `main`. Build `cargo build --release`; binary `target/release/airscope`. Plan: `docs/PLAN.md`.

## Layout
- `src/data.rs` — collectors on threads publishing to `Arc<Mutex<Shared>>`: Wi-Fi via `nm.rs`, BlueZ device list via `bluez.rs`, LAN (neighbours via `net.rs`, mDNS via `mdns-sd` crate, stale devices pinged), OUI vendors (`Oui`: built-in `src/oui.tsv` < system `/usr/share/hwdata/oui.txt` < user `~/.config/airscope/oui.tsv`; `parse_oui` reads IEEE txt/csv, Wireshark manuf, plain TSV; header "Vendor list" button opens an import window — drop a file or paste a path; `AIRSCOPE_VENDOR=1` opens it at start). Regenerate built-in list: `scripts/gen-oui.sh [oui.txt]`. `Dev` (id = BSSID/MAC), `BtLink`.
- `src/hci.rs` — native Bluetooth HCI (kernel socket, read-only commands, no root): connections, AFH map, link quality, RSSI, TX power, remote version. Replaced `hcitool`. Test: `cargo test live_dump -- --ignored --nocapture`.
- `src/bluez.rs` — BlueZ over system D-Bus (zbus, blocking): one `GetManagedObjects` call gives all devices + properties (replaced `bluetoothctl`). Test: `cargo test bluez_dump -- --ignored --nocapture`.
- `src/nm.rs` — NetworkManager over system D-Bus (one `GetManagedObjects` at `/org/freedesktop`): cached scan results, channel/security text like nmcli, `request_scan`. Replaced `nmcli`. Test: `cargo test nm_dump -- --ignored --nocapture` (diffed against nmcli: identical).
- `src/net.rs` — LAN basics without tools: IPv4 neighbours via netlink, default gateway (/proc/net/route), unprivileged ICMP ping (needs `net.ipv4.ping_group_range`; if forbidden STALE counts as online), reverse DNS (getnameinfo), virtual-interface name filter. Test: `cargo test net_dump -- --ignored --nocapture`. mDNS test: `cargo test mdns_dump -- --ignored --nocapture`.
- `src/fx.rs` — theme colours + custom painting: mesh gradients (`sphere`, `glow`, `glow_rect`), `panel`, `orb`, `dot`, `icon`, `lcd`, `led_bar`.
- `src/main.rs` — everything else: `App`, `radar()` (latch/ping, grouping, layout shifts, drag), `spectrum()`, `devices()`, `popup()`, tooltips (`TipC`), `nets()`/`hill()` Wi-Fi model.

## Behaviour to keep in mind
- Radar data is *latched*: a device updates only when the sweep passes it, signal changes < `SIGNAL_LEEWAY_DB` (4 dB) are ignored; frozen while a group is open or a drag is active.
- Distances are signal-derived (log radial map); user drags saved in `~/.config/airscope/positions.tsv` (id, angle, metres). Optional extra "mine" SSID prefixes: `~/.config/airscope/mine.txt`. "Mine" Wi-Fi = BSSID derived from a LAN device MAC (`same_device`).
- Groups: union-find, GROUP_PX=38 on screen; stable keys via member overlap; click expands, drag moves all members; dragged node/group is excluded from merging until dropped.
- Ids must be unique: a router's BSSID can equal its LAN MAC; `sync()` suffixes the later duplicate (`~lan`). Animation state maps are keyed by id — duplicates broke grouping once.
- Info box (`popup`) and tooltips are shared across screens and fade (`pops`, `tip_want`).
- Debug env switches: `AIRSCOPE_ZOOM`, `AIRSCOPE_SCREEN=spectrum|devices`, `AIRSCOPE_SEL=<id>`, `AIRSCOPE_PIN_BIG`, `AIRSCOPE_AUTO`, `AIRSCOPE_SLOW`.
- Testing: screenshot with `spectacle -b -n -a -o <file>`; don't screenshot while Peter is using the screen (once captured his browser — deleted).

## Portability roadmap (goal: no external tools)
Done: HCI (`hci.rs`), BlueZ D-Bus, NetworkManager D-Bus (stated requirement for Wi-Fi; nl80211 fallback only if someone asks), native LAN (`net.rs`, mdns-sd). Embedded OUI DB + user import done. Next: package (AppImage, Flatpak for Bazzite).

## Not done yet
Interference suspects (red halos), settings panel, LAN scan button, `.desktop` launcher, reset-positions option.
