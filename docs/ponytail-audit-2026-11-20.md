# Ponytail audit, 2026-11-20

This is a log of a code audit I ran with Claude Code using the ponytail audit skill, kept here so anyone reading the repo can see what was looked at, what turned up and what I did about it. It is a partial audit. So far it covered the recording code (`src/capture.rs`), the file saving, the LAN collector in `src/data.rs` and the Bluetooth HCI code in `src/hci.rs`. The big `src/main.rs` UI file, most of the rest of `src/data.rs`, and the 31 uses of `unsafe` (most of them local-time calls) have not been read line by line. I also didn't run anything on hardware other than my own machine.

## Findings

1. **Stop could kill the wrong process as root** (`src/capture.rs`, minor). Stop looked up btmon's process id and then asked for my password before killing it, so if the recording ended on its own while the prompt was open, the id could have been handed to something else. **Fixed** in 86f036c. Stop now asks `pkill` to match btmon's command line at the moment it runs. I tried the Stop button by hand and it ended a recording after about 17 seconds.

2. **A failed save could wipe saved device positions** (`src/main.rs`, `src/data.rs`, minor). Positions and the imported vendor list were written straight over the real file, so a full disk or a crash halfway through would leave it empty, with no error shown. **Fixed** in 8729215. Both now write to a temporary file and rename it over the real one, and there is a small test for it.

3. **A LAN device could keep another device's name** (`src/data.rs`, minor). The reverse DNS cache was keyed on the IP address alone, so a different device that got the same address kept the old name until Airscope was restarted. **Fixed** in 5d34272 by keying on IP and MAC together. I did not change the blocking DNS lookups, because I haven't seen the LAN list stay empty and a slow DNS server was only a guess.

4. **Bluetooth Low Energy devices get no link details or drop events** (`src/hci.rs`, `src/data.rs`, minor). Only classic Bluetooth connections are listed and only classic connect and disconnect events are subscribed to, so a device that only uses Low Energy would show up in the device list but never get a drop history. **Documented rather than fixed.** Full support would probably need root, since an unprivileged socket only sees those two events, and the only Low Energy device I own is a Steam Controller that I use through its own dongle. My keyboard is classic and shows its drops fine. It's now listed under the honest limitations in the README.

5. **A crash while running restarted the app in a worse mode** (`src/main.rs`, minor). If anything panicked while the window was open, the start-up fallback that turns off multisampling mistook it for a graphics problem, blamed multisampling in its message and relaunched with jagged edges, hiding the real crash. **Fixed** in the next commit. The retry now only happens if the failure came before the window and graphics setup finished. I checked that the start-up fallback still works by forcing a bad multisampling value in a headless compositor. I did not trigger a real crash to test the new path.

6. **A capture with two Bluetooth adapters mixes up their connections** (`src/snoop.rs`, minor). The capture reader ignores which adapter a record came from and tracks connections by handle number alone, so two adapters using the same handle number can get confused and a drop can end up under the wrong device. **Documented rather than fixed.** I only have one adapter, so I couldn't test a proper fix, and I haven't confirmed the capture format detail it rests on. A note is now in the README next to the existing single-adapter limitation.

7. **Huge system-log ranges lost their newest entries** (`src/syslog.rs`, minor). The system log reader keeps at most 400,000 lines but read them oldest first, so a very long range on a chatty machine would silently drop the most recent days. **Fixed** by asking `journalctl` for newest first (`-r`) so the cap drops the oldest instead, and the existing sort puts them back in order. I haven't tried it against a journal big enough to hit the cap.

## Skipped

Nothing has been skipped outright. Findings 4 and 6 were handled by documenting the limit instead of building support.

## Looked at and left alone

- **Hand-edited positions file.** `positions.tsv` is read without checking that the numbers are finite, so a line with `nan` in it would panic the radar sort on every frame. The app never writes such a value (drags are clamped), so it would take someone editing the file by hand, which I didn't count as a bug.
- **Wi-Fi rescan every 60 seconds.** This is a deliberate shortcut already marked in the code, and the README now says a scan can briefly disturb Bluetooth on chips that share an antenna.

## Not read

Most of the drawing code in `src/main.rs`, `src/fx.rs`, `src/dtpick.rs` and `src/diag/page.rs`, and the `unsafe` blocks in `hci.rs` and `net.rs` beyond the netlink parsing. Nothing was run on hardware other than my own machine.

## Verdict

Seven findings, all minor, none of them a way to lose data in normal use. Five were fixed and two were documented as limits. The privileged recording path was the riskiest area and it was the first thing I fixed.
