# Airscope — plan

A native Linux app that shows the radio environment around this PC (Wi-Fi, Bluetooth, LAN) as a live, glowing "radar", with enough detail to troubleshoot interference. Built for peter-pc (Bazzite, KDE/Wayland, Intel AX200), but nothing is hard-wired to it.

## Goals (from the brief)

1. One main screen: devices and their radio signals around the PC, top-down, strength shown as distance.
2. Click any device: popup with what it is, based on what it advertises.
3. Details for devices connected to this PC (Bluetooth links, Wi-Fi state, LAN neighbours).
4. Known devices get icons; unknown signals are dots sized by strength.
5. Scan the local network and list what's on it.
6. Highlight likely interference sources and show them visually.
7. Drag devices to where they really are (persisted).
8. Fast, 60 fps, real time; looks colourful with depth (glow, bevel, shadow, shine), not flat.

## Stack decision

**Rust + egui/eframe (OpenGL).** One language for data collection and drawing, small binary, immediate-mode redraw each frame suits 60 fps animation, and custom painting gives full control of glow/bevel/shadow. Builds with the Homebrew Rust toolchain on this immutable system; runtime needs only libs already present (EGL, Wayland, xkbcommon).

Rejected for now: Qt/QML (best built-in effects, but needs Qt dev packages in a container and a heavier build), Python (fine for data, weaker for a 60 fps custom canvas).

## What the radios can and can't tell us

- **Wi-Fi (scan list via NetworkManager):** SSID, BSSID, channel, frequency, width, rate, signal, security. Signal gives a distance estimate; direction is unknown. Cached scans only by default, because active scans briefly disturb Bluetooth on the shared AX200 chip.
- **Bluetooth (BlueZ via `bluetoothctl`, plus `hcitool` for connected links):** name, class/icon, UUIDs, vendor, battery, RSSI where reported, link quality, and the **AFH channel map** (which of the 79 channels the controller avoids). No active discovery by default (it would add radio traffic); known/paired devices only, discovery is opt-in.
- **LAN (neighbour table + mDNS):** IP, MAC, vendor (from the OUI list), name and services when advertised. No signal strength (wired or Wi-Fi unknown); shown on their own ring.
- **Location:** a single receiver cannot get direction. Distance comes from signal strength (log-distance model, tunable); the angle is stable-hashed per device, and the user drags devices to their real spot. Positions are saved.
- **Interference:** non-Wi-Fi 2.4 GHz noise (microwaves, USB 3 hubs, cordless gear) is invisible without extra hardware. We can only score Wi-Fi networks whose channel overlaps the Bluetooth channels the controller has blocked, weighted by signal.

## Screens

1. **Radar** (default): rings at 1/3/10/30 m, rotating sweep, PC at the centre, device nodes, link lines with pulses for connected devices, interference halos on suspects.
2. **Spectrum**: 2.4 GHz axis with Wi-Fi channel humps, the 79-channel Bluetooth strip (used/blocked), interference candidates ranked.
3. **Devices**: table of everything seen, with the connected-to-this-PC details.
4. **Settings**: scan intervals, distance model, units, BT discovery opt-in.

## Data flow

Collector threads (Wi-Fi, Bluetooth, LAN, OUI loader) -> one shared snapshot behind a mutex -> UI reads each frame. UI never blocks on a subprocess. Collectors shell out to `nmcli`, `bluetoothctl`, `hcitool`, `ip`, `avahi-browse` for now (simple, robust); native D-Bus/HCI can replace them later if CPU shows up in profiling.

## Phases (iterative; feedback after each)

1. Scaffold + theme + Radar with real data + click popup + drag. **(first drop)**
2. Spectrum screen + interference scoring on the radar.
3. Devices screen, connected-device details, LAN scan button, settings.
4. Polish: animation, icons, perf pass, packaging (`.desktop` entry).

## Risks / open questions

- BT signal strength is unreliable for connected devices (the controller reports 0 near range); the radar will mark those as "distance unknown" rather than invent a number.
- Distance from RSSI is rough (walls, antennas). Treated as a hint, not a measurement.
- Scanning is only on the user's own network and radios.
