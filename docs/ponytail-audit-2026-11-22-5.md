# Ponytail audit, 2026-11-22 (follow-up, round 6: the changes since round 5, plus the parts only skimmed before)

This is the seventh pass of the audit, run with Claude Code using the ponytail audit skill. I asked for one more sweep after adopting rustfmt and Clippy, to check that the new code is good to go. Scope for this round: everything that changed since round 5 (the config, the formatting run, the `unwrap` replacements, the 29 let-chain collapses, the new `#[allow]` comments, and the README and `CLAUDE.md` claims), plus a line-by-line read of the middle of `radar()` and the Devices list in `main.rs`, which earlier rounds had only skimmed. This log is written as I go. Nothing here was run on hardware other than my own machine.

## Checked first: did the formatting run change behaviour?

I can't prove it from the diff, since it is thousands of lines, but `cargo fmt` only moves whitespace and comments around by design. What I could check: the tests pass before and after, and headless screenshots of the Spectrum screen, the Devices screen and the radar with an info box came out identical to the build from before the formatting run, apart from about 80 pixels in a million where the radar sweep sits at a different angle.

## Findings

1. **Reading numbers out of byte buffers was written out in four files** (`net.rs`, `nl80211.rs`, `hci.rs`, `snoop.rs`, minor, fixed). When I replaced the `try_into().unwrap()` calls in `net.rs` last round I added two small helpers there, but `nl80211.rs` had six of the same sums written inline, `hci.rs` five (little-endian) and `snoop.rs` its own `le16` that took a slice start instead of an offset. All of them now use one `src/bytes.rs` (`ne16`, `ne32`, `le16`, each with an offset), which has a small test. The three live hardware tests (`live_dump`, `net_dump`, `nl80211_dump`) still pass on my machine. **Fixed.**

2. **Two bits of code in `radar()` were written twice** (`main.rs`, minor, fixed). The colour of a group icon (the kind's colour if every member is the same kind, else a neutral blue-white) was written out for the collapsed icon and again for the open group's centre, and the list of connected Bluetooth devices in a group (for the link lines to the PC) was written out twice as well. They are now `App::group_col()` and `App::connected_bt_pos()`. Screenshots of the radar with groups closed and open match the build from before. **Fixed.**

3. **An unused field in the Devices list's group table** (`main.rs`, minor, fixed). The table had a first column ("YOURS", "NEARBY · not yours") that the drawing code ignored (`_section`), and "NEARBY" was out of date anyway since the tab is called "Others". Removed.

## Findings that needed a decision

4. **Per-device animation memory is never cleaned up** (`main.rs`, `radar()`, minor, fixed). Five maps keyed by device id (`offs`, `sdist`, `label_a`, `appear`, `last_world`) get an entry the first time a device is seen and were never pruned, although `latched` itself is. In a busy area with many Wi-Fi networks coming and going over days, they would only grow, though each entry is tiny. **Fixed.** The loop that handles a device leaving now removes its entry from all five (`last_world` is taken out as it is read for the burst effect). A device that comes back pops in fresh, as new devices already do. I haven't watched this live over a long run, only built it and run the tests.

5. **`cargo clippy --all-targets` warned 19 times about `unwrap` in tests, and nothing in CI ran fmt or clippy** (`Cargo.toml`, `.github/workflows/`, minor, fixed). Test code is meant to unwrap, and the README now says the code follows rustfmt and Clippy standards while the only workflow ran on release tags and only ran the tests. **Fixed.** `clippy.toml` has `allow-unwrap-in-tests = true`, and a new `.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` on every push to main and every pull request, leaving `release.yml` alone so a mistake in the new file can't block a release. I chose a separate file on purpose. I couldn't run the workflow here, only the same three commands locally (all clean), so the first push will be its real test. One thing to know: `-D warnings` means a newer Clippy on the runner could fail the check on a lint that didn't exist when I wrote this, which is what a lint check is for, but expect it now and then.

6. **The Devices list sorted with a key that copies each name** (`main.rs`, `devices()`, nice to have, fixed). `sort_by_key` calls the key function on every comparison, so each frame cloned the name strings a few hundred times. It is far too small to feel, but I'm fussy about avoidable work, so it is fixed. The list is now sorted with `sort_by`, comparing in place with the same order as before (connected first, strongest first, then by name). Screenshots of the Devices screen (collapsed and expanded) are pixel-identical to the build from before this round.

## Looked at and left alone

- **`key_of[ri].clone().unwrap_or_default()` in `radar()`.** It can't hit the default, since the two loops above it give every group a key, but it would now merge groups under an empty key instead of crashing if that ever changed. I left it, because the invariant is local and visible.
- **`total_cmp` instead of `partial_cmp().unwrap()`.** It orders -0.0 before 0.0 where the old code treated them as equal. That can only change the order of two nodes at exactly the same distance, so it is not worth a finding.
- **Group key inheritance when a group splits into two equal halves.** Which half keeps the old key (and so its open/closed state) depends on hash order. It happens once, at the split, and the result is harmless.
- **The module-level `#![allow(clippy::unwrap_used)]` in `data.rs` and `demo.rs`** also covers any future non-lock `unwrap` in those files. The comment says why for locks. If that bothers me later, the fix is a statement-level allow on each lock, which is noisy.
- **`radar()` is about 1,000 lines.** It reads top to bottom (view, sweep, placement, grouping, shifts, smoothing, labels, drawing, events), and I couldn't find a bug in it, so same call as `page()` and `spectrum()` before.

## Checked on screen and on hardware

Headless screenshots of the radar (groups closed, and a group opened), the Devices screen and the Spectrum screen match the build from before this round, apart from the radar sweep's usual 60 to 80 pixels in a million. The live tests for the HCI, neighbour and nl80211 readers pass on my machine after the byte-reading change. The new CI workflow is untested until it runs on GitHub.

## Not read

The parts of `main.rs` that other rounds already covered, and the end of `devices()` after the list (selection handling).

## Verdict

Six findings, all minor, and none of them a bug. All six are fixed. The new code from the rustfmt and Clippy work reads well, nothing in it changed behaviour that I could find, and I think it is good to go. What I haven't been able to check is the new CI workflow (it runs on the first push) and a long live session for the memory fix.
