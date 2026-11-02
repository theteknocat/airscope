# Airscope

**See the wireless world around your PC as a radar.** Airscope shows the Wi-Fi networks, Bluetooth devices and
LAN devices near you, how crowded the 2.4 GHz band is, and which Bluetooth channels are being steered around
the noise. It is a native Linux app (Rust, egui) that runs on Wayland or X11.

[![Buy me a coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-teknocat-ffdd00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/teknocat)
&nbsp;If Airscope is useful to you, you can [buy me a coffee](https://buymeacoffee.com/teknocat).

![Airscope radar](docs/screenshots/radar.png)

> ### 🤖 Fully vibe coded
> **Every line of this project was written by AI.** I built Airscope entirely by describing what I wanted to
> [Claude Code](https://claude.com/claude-code) (Anthropic's AI coding tool) and testing the result by using it.
> I'm being upfront about that so anyone reading or reviewing the code knows what they are looking at. It works
> well for me, but it has not had a professional audit, and some of the code will look like what it is. Reviews,
> bug reports and pull requests are very welcome.

## What it does

- **Radar:** every device is an orb. Distance comes from signal strength (rough, and labelled as such); a sweep
  refreshes devices as it passes them. Devices that are close together fold into a group you can click to fan
  out. Drag any device or group to where it really is, and Airscope remembers.
- **Spectrum:** Wi-Fi networks drawn as hills across the 2.4 GHz channels, with each connected Bluetooth
  device's channel map underneath, so you can see whether Bluetooth is avoiding the busy channels (adaptive
  frequency hopping) or hopping straight through them.
- **Devices:** a searchable, filterable list of everything seen, with an info box per device explaining *how*
  Airscope identified it.
- **Vendor names** from a built-in MAC prefix list that you can update yourself (see below).

| | |
|---|---|
| ![Device info](docs/screenshots/radar-info.png) | ![Spectrum](docs/screenshots/spectrum.png) |
| ![Devices](docs/screenshots/devices.png) | ![Vendor list](docs/screenshots/vendor-list.png) |

*(All screenshots use the built-in demo data: invented networks and devices.)*

## Install

**AppImage (any distro from about 2022 on):** download `Airscope-x86_64.AppImage` from the
[Releases](../../releases) page, then:

```text
chmod +x Airscope-x86_64.AppImage
./Airscope-x86_64.AppImage
```

**From source:**

```text
cargo build --release
target/release/airscope
```

### Requirements

| For | Needs |
|---|---|
| The app itself | Wayland or X11, and OpenGL |
| Wi-Fi networks | NetworkManager (Airscope reads its cached scan results over D-Bus) |
| Bluetooth devices | BlueZ (over D-Bus) |
| Bluetooth link details (channel map, link quality) | A Bluetooth adapter and a connected device. These are read with read-only HCI commands, no root needed on the systems I tried |
| LAN devices | Nothing extra: ARP table, mDNS and a ping |

Without NetworkManager the Wi-Fi part shows a message and the rest keeps working. There is deliberately no Flatpak: the app needs
the system D-Bus, raw Bluetooth sockets and the LAN, which the sandbox would have to be opened up for anyway.

## Privacy and what it touches

- Airscope makes **no internet connections** and sends no telemetry. The vendor list update is manual: you
  download a file yourself and hand it to the app.
- On your local network it **listens and probes**: it reads the ARP table, sends mDNS discovery queries, and
  pings devices the kernel hasn't talked to recently to see if they are still there.
- By default it never triggers a Wi-Fi scan by itself (scans briefly disturb Bluetooth on chips that share an antenna).
  Press **Rescan Wi-Fi** when you want fresh results.
- It is read-only: it doesn't change any system or adapter settings.
- It stores a little in `~/.config/airscope/`: dragged device positions, an optional list of your own SSID
  prefixes (`mine.txt`), and an imported vendor list if you made one.

## Updating the vendor list

Airscope ships with about 40,000 MAC prefixes. New ones appear over time: click **Vendor list** in the header,
download one of the listed files in your browser, and drag it onto the dialog. It understands the IEEE
`oui.txt` and `oui.csv` and Wireshark's `manuf`. To regenerate the built-in list for a release:
`scripts/gen-oui.sh oui.txt`.

## Try it without your own network

```text
AIRSCOPE_DEMO=1 target/release/airscope
```

Demo mode fills the app with invented data and ignores your real hostname and saved positions. It is what the
screenshots above use (`scripts/demo-shots.sh` regenerates them in a headless compositor).

## Honest limitations

- Distances are estimated from signal strength and are rough on purpose; drag devices to where they really are.
- Wi-Fi needs NetworkManager. Other managers (iwd or wpa_supplicant alone) are not supported yet.
- Only 2.4 GHz is drawn on the spectrum; 5 and 6 GHz networks appear on the radar and in the list.
- Tested on one machine (Bazzite / KDE Plasma, Wayland, an Intel AX200). Other hardware may behave differently.

## Building a release

Releases are built by GitHub Actions when a tag is pushed: bump `version` in `Cargo.toml`, commit, then
`git tag vX.Y.Z && git push --tags`. To build the AppImage locally (needs podman): `packaging/build-appimage.sh`.

## License

[MIT](LICENSE)
