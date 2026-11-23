# Ponytail audit, 2026-11-22 (follow-up, round 7: closing the gaps)

This is the eighth pass of the audit, run with Claude Code using the ponytail audit skill. After round 6 I asked what was still unread across all the logs, and this round covers that list: `nav()`, `header()`, `vendor_window()` and the body of `node_widget()`, the tray, the end of `devices()` and the tooltip drawing in `main.rs`, `src/dtpick.rs`, and the tests at the bottom of `src/data.rs`. This log is written as I go. Nothing here was run on hardware other than my own machine.

## Findings

1. **Four doc comments were on the wrong function or out of date** (`src/main.rs`, minor, fixed). The first line of the comment above `on_radar` ("Default placement: (angle, metres, dimmed)...") described `layout_of`. The comment above `dialog_scrim` started with "Dialog for importing a newer vendor list", which describes `vendor_window` (I split that comment myself when I added `dialog_scrim` earlier today). The comment above `bt_rows` still said it returns the y position after the rows, which round 5 removed. And `is_mine` had a pair of brackets that did nothing since last round's rewrite. Each comment now sits on the function it describes.

2. **`header()` took an argument it never used** (`src/main.rs`, minor, fixed). Same as `spectrum()` in round 5. Removed.

3. **The X that closes a box was drawn out by hand twice** (`src/main.rs`, minor, fixed). The vendor list dialog and the device info box each placed a hit area, picked white or grey for hover, drew the two strokes and checked the click. One `close_button()` does it now.

4. **A field was named for something it doesn't do** (`src/main.rs`, minor, fixed). `vendor_copied` remembers which download link was just opened in the browser, and nothing is copied. It is `vendor_opened` now, with its local variable and widget id to match.

5. **The radar sweep's glow was worked out in three places** (`src/main.rs`, minor, fixed). `node_widget` and `sweep_hot` both had the same "1 as the sweep passes, fading over 0.9 radians" sum, and the radar drew its sweep trail as 36 steps of 0.025, which is the same 0.9 written another way. There is one `SWEEP_TRAIL` constant and one `sweep_glow()` now, with `sweep_hot` calling it.

6. **The date picker repeated two things** (`src/dtpick.rs`, minor, fixed). It had its own list of short month names, a copy of the one in `snoop.rs`, and the small square button with a one-character label was drawn out twice, for the month arrows and for the hour and minute steppers. It uses `snoop::MONTHS` and one `step_button()` now. I also dropped a pointless `let _ =`.

7. **The `data.rs` tests had some repeated set-up and one misplaced line** (`src/data.rs`, minor, fixed). The same `Sample { at: Instant::now(), ... }` was written out four times, the test MAC address seven times, and `use super::*;` sat in the middle of the module after the first test. There is a `sample()` helper and a `MAC` constant now, and the `use` is at the top. The tests all still pass, and I read each one to check that its assertions would fail if the code broke.

## Open, waiting on a decision

8. **`cargo fmt` silently skips code that has a long string in it** (about 90 lines across `main.rs`, `diag/page.rs`, `data.rs`, `hci.rs`, `syslog.rs`, `diag.rs` and a few more). When one line can't be wrapped to 100 columns because of a long string literal, rustfmt leaves the whole surrounding statement alone and says nothing, and `cargo fmt --check` still passes. The worst case is the vendor dialog in `vendor_window()`, about 80 lines of one-line `p.text(...)` calls that are not formatted at all. So the README's claim of following rustfmt is true by the tool's rules, but a reader sees code that looks unformatted.

## Looked at and left alone

- **`save_over()` ignores write errors.** If `~/.config/airscope` isn't writable, dragged positions silently aren't saved. That is the only way it can fail, and a failure message for it would be more code than the problem is worth. The write itself is atomic and was covered in round 1.
- **`mac_bytes()` slices a `&str` at byte 17.** That would panic if byte 17 were in the middle of a multi-byte character, but every device id is a MAC address, so it is always ASCII.
- **The tray and `devices()` list** read fine. Their ids are unique and every click path ends in the same selection toggle.
- **The hour stepper wraps 23 to 0 without changing the date.** That's a choice, not a bug, since the day is picked in the calendar above.

## Checked on screen

Headless screenshots of the vendor dialog, the radar with an info box open, Spectrum and Diagnostics match the build from before this round. Those that animate (radar and Diagnostics) differ by about 200 to 350 pixels in a million, and the old build compared with itself on a second run differs by about the same (300 to 350), so it is timing noise. The tests pass, Clippy is clean with `-D warnings`, and `cargo fmt --check` passes. I couldn't try the date picker's calendar on screen, because it only opens from the System log's custom range, which reads the real journal.

## Not read

Nothing in the code is unread now, apart from what I have said was only skimmed: the middle of `diag/page.rs` and `data.rs` (`lan_loop`, `mdns_loop`, `classify`) were read in earlier rounds and not again.
