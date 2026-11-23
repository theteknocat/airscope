# Ponytail audit, 2026-11-22 (follow-up, round 8: the work from the hands-on testing)

This is the ninth pass of the audit, run with Claude Code using the ponytail audit skill. Scope for this round: the code I wrote after round 7 in response to testing on my own machine, which is the System log toolbar and rows refactor, the "yours" rule for the connected network, merging access points with one name into one network (`merge_networks`, `Dev.radios`), the Spectrum hills, legend and highlighting, the per-radio signal bars in the info box, and the Wayland drag text. This log is written as I go, and nothing here was run on hardware other than my own machine.

## Findings

1. **Spectrum could crash when a network vanished while hovering a channel tile dot** (`src/main.rs`, `spectrum()`, minor, fixed). Hovering a dot in one of the three channel tiles lights its hill, and the position of that hill in the network list is remembered from the previous frame. It was then used straight away (`nets[h]`) without checking that it still existed, so if a Wi-Fi scan dropped a network on that frame the list got shorter and the app panicked. The unchecked use was already there in the tooltip code, and the network highlighting I added today read it earlier in the frame. It needs a hover and a scan update at the same moment, so it is rare, but a long session on a busy network could hit it. **Fixed.** A remembered position past the end of the list is now ignored.

2. **The Spectrum tooltip listed a network twice when it had two hills in the same spot** (`src/main.rs`, `spectrum()`, minor, fixed). Now that a network has one hill per radio, two radios of one network on nearby channels both counted as "Also here" next to the hovered hill, so the tooltip could name the network you were already pointing at, or the same other network twice. The list now skips anything that is the hovered network and names each other network once. **Fixed.**

3. **The info box would run out of room for a network with many radios** (`src/main.rs`, `popup()`, minor, fixed). It drew one signal bar per radio and grew to fit, so a campus or office network with a dozen access points under one name would push the box taller than a small window. The box now draws at most six bars (the strongest, since radios are sorted that way) and a dim "+N weaker radios, listed below" line, and the detail rows underneath still list every radio. I added an eight-radio "Campus_Guest" network to the demo data to try this, and checked it on screen (six bars, "+2 weaker radios, listed below", eight rows). **Fixed.**

4. **The Spectrum's grouping of radios into hills had no test** (`src/main.rs`, `App::nets()`, minor, fixed). The rules (one hill per name and channel, 5 GHz left out, unnamed ones lumped per channel, every hill of a network pointing at its strongest one) lived inside a method that needs a whole `App`, so only my eyes checked them. They are now a plain function, `hills()`, with one test using the same merged-network data as the `data.rs` test. I made `Oui::at` crate-visible so the test can build a device without touching my real config. **Fixed.**

## Looked at and left alone

- **The Security row of a merged network shows only the strongest radio's.** Radios of one name nearly always share it, and the per-radio rows underneath carry the speed and address, so it would take a router with mixed security under one name to matter.
- **`same_network` matches by name prefix**, so a neighbour called "SkyNet Guest" next to my "SkyNet" counts as mine. Short names don't match by prefix, and I'd rather have that than miss Starlink-style names.
- **Saved radar positions of a merged network that were stored under a radio other than the lowest BSSID are lost.** It is a one-off, the node returns to its calculated spot, and CLAUDE.md already says so.
- **Two different routers that share a name merge into one network.** It is how my system's own Wi-Fi menu treats them, and the info box says how many radios it found.

## Checked on screen

Headless screenshots of the Devices screen with the eight-radio "Campus_Guest" info box (six bars, "+2 weaker radios, listed below", eight detail rows). Hover highlighting, tooltips and the tile-hover crash fix can't be tried headlessly, so those are checked by reading the code only.

## Not read

Nothing new beyond what earlier rounds listed.

## Verdict

Four findings, all minor, all fixed. One of them (the tile hover) was a rare crash. 38 tests pass, Clippy is clean, and I think the merged-network work is good to go.
