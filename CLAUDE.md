# Airscope — project notes

Native Linux GUI (Rust, egui/eframe 0.37, glow) visualising Wi-Fi / Bluetooth / LAN around peter-pc as a radar. Repo: private `theteknocat/airscope`, branch `main`. Build `cargo build --release`; binary `target/release/airscope`. Plan: `docs/PLAN.md`.

## Layout
- `src/data.rs` — collectors on threads publishing to `Arc<Mutex<Shared>>`: nmcli Wi-Fi, bluetoothctl + `~/bt-tools/hcitool` (AFH/lq/rssi), `ip neigh`+avahi+getent LAN, OUI vendors. `Dev` (id = BSSID/MAC), `BtLink`.
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

## Not done yet
Interference suspects (red halos), settings panel, LAN scan button, `.desktop` launcher, reset-positions option.
