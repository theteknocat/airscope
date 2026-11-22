# Ponytail audit, 2026-11-22 (follow-up, round 2)

This is the third pass of the audit, run with Claude Code using the ponytail audit skill, picking up from the "Not read" list in `ponytail-audit-2026-11-21.md`. Scope for this round: the rest of `src/data.rs` (the Wi-Fi and Bluetooth collectors, the vendor list parser, and the code that works out what is wrong with a Bluetooth device), `src/syslog.rs` and `src/diag/logview.rs`. The LAN collector in `data.rs` was covered on 2026-11-20 and I didn't read it again. As before, nothing here was run on hardware other than my own machine, and the findings come from reading the code unless I say otherwise.

## Findings

1. **A neighbour's radios start in different directions on the radar** (`src/data.rs`, minor, skipped). Each Wi-Fi access point gets a "cluster" key that the radar hashes into a starting direction, so the access points of one router are meant to start out together. The key is built from the last two octets of the BSSID (`bssid.rsplitn(3, ':').take(2)`). I checked it against my own scan. My own Starlink's two radios (`5A:40:1A:2D:A8:5C` and `5A:40:1A:3D:A8:5C`) differ only in the fourth octet, so they share a key and start together, which is what it's for. A neighbour's router (`70:97:41:98:A5:19`, `…:1A`, `…:1B`, plus two unnamed ones that differ in the first or last octet) gets four different keys and starts scattered. **Skipped.** The obvious fix, using the middle four octets, would group the neighbour's radios and split mine, and no simple rule fits both. It's cosmetic anyway, since the radar lets me drag and group things by hand. I first misread the neighbour's router as mine here, and only caught it when I checked which network was the one in use.

2. **The system log reads the whole journal into memory before applying its 400,000 line cap** (`src/syslog.rs`, minor, skipped). `journal()` uses `Command::output()`, which collects everything `journalctl` prints before the cap is applied, so the cap limits how much gets parsed but not how much is held. A long range (Last 7 days or a custom start) on a machine whose kernel log is noisy could pull in a lot of text first. I haven't hit this, and `-k` on my machine is small (about 6,000 lines in 7 days, and 411 lines for Bluetooth). This is a follow-on to finding 7 of the 2026-11-20 audit, which was about which lines the cap drops, and it rests on the same machine I don't have. **Skipped.** The fix would be to read `journalctl` line by line and stop at the cap, but it's invisible on a quiet machine and I'd be guessing at the problem. If someone else uses Airscope and a long range turns out slow or heavy on memory, they can report it and I'll deal with it then.

3. **A few dead branches in the log view and the Wi-Fi hint** (`src/diag/logview.rs`, `src/data.rs`, minor, fixed). The range dropdown's width is `if matches!(pick, Pick::Custom) { lw + 44.0 } else { lw + 44.0 }`, the same value either way. Its label has an `if ci == 0` branch that gives exactly what `PICKS[ci].0` already gives. In `wifi_context` the `if s.name.is_empty() { "(hidden)" }` can't happen, because a hidden network is already named "(hidden network)". Nothing breaks, it's just code that says more than it does. **Fixed.** I removed all three, and tidied a misindented block in the same function while I was there. The output is the same as before and the tests pass.

## Looked at and left alone

- **`apply_events` and the drop and flap rules** (`src/data.rs`). Every `unwrap` and division is behind a length check, the time comparisons handle clocks going either way, and the "shared cause" check can't repeat a name because a Bluetooth device's name falls back to its address and is never empty.
- **The vendor list parser** (`parse_oui`). It skips masked Wireshark prefixes and everything that isn't 24 bits, and it keeps the last entry for a repeated prefix as the layer counts expect.
- **`find_mac` and `parse_line`** (`src/syslog.rs`). The string slicing only happens on windows that are already checked to be plain ASCII, so a non-ASCII log line can't cut a character in half.
- **The collector threads.** They only hold the shared lock for plain assignments, so a panic can't poison it for the others.

## Not read

`lan_loop`, `mdns_loop` and `classify` in `src/data.rs` (covered on 2026-11-20), the tests at the bottom of `data.rs`, and all of the drawing code (`src/main.rs`, `src/fx.rs`, `src/diag.rs`, `src/diag/page.rs`, `src/demo.rs`).

## Verdict

Three findings, all minor, and nothing that could lose data or break a user's setup. One was fixed (the dead branches) and two were skipped on purpose: the radar grouping, because the obvious fix would split my own router's two radios to group a neighbour's, and the system log's memory use, because I can't reproduce it and would only be guessing. The Wi-Fi and Bluetooth collectors and the problem rules in `data.rs` held up well. What's left to read is the drawing code, listed above.
