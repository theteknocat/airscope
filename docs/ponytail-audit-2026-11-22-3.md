# Ponytail audit, 2026-11-22 (follow-up, round 4: Diagnostics)

This is the fifth pass of the audit, run with Claude Code using the ponytail audit skill, picking up from the "Not read" list in `ponytail-audit-2026-11-22-2.md`. Scope for this round: the Diagnostics screen, which is `src/diag.rs` and `src/diag/page.rs`. This log is written as I go, and both files are now read. As before, nothing here was run on hardware other than my own machine, and the findings come from reading the code unless I say otherwise.

## Findings

1. **The capture timestamp offset is written out twice as a bare number** (`src/diag.rs`, minor, fixed). `snoop.rs` has a named constant for the offset between the btsnoop file's clock and the Unix clock (`BTSNOOP_EPOCH_US`), and uses it when reading each record. `diag.rs` types the same number in by hand in two places, once in `summary()` and once in `first_unix()`, which do exactly the same sum. If the constant were ever corrected, `diag.rs` would quietly keep the old value and its times would be wrong. **Fixed.** `Snoop` now has one `first_unix()` method that uses the named constant, and `summary()`, the strip in `page.rs` and the old `first_unix` helper in `diag.rs` (now deleted) all use it. The tests pass.

2. **The "Start here" glow is drawn by two copies of the same code** (`src/diag.rs`, minor, fixed). `start_here()` and the "pointing at Record" pulse in `ui()` both expand the button area, draw a soft glow and then a pulsing outline with the same sizes and colours, differing only in how they compute the fade. A change to one has to be copied into the other. **Fixed.** One `pulse_outline()` draws the glow and outline and both places call it. The margin round the buttons (`pulse_area()`) was also typed in a third place, the check for whether the pointer is over them, and that uses the same function now. I prefer code that says each thing once, so I'm treating repeated code as worth fixing.

3. **The device card header and its colour rules are written out more than once** (`src/diag.rs`, `src/diag/page.rs`, minor, fixed). The full card in `diag.rs` (`card_at`) and the System log's info box in `page.rs` both draw the same header: the panel, the orb and icon, the name, a caption, and a big number in the accent colour with its label. The colour rules are repeated as well: red if a drop has a radio-type reason, amber for any other drop and blue for none (in `card_at` and `cap_view`), and "show the big number green when the accent is blue" (in `card_at`, `compact` and the log info box). **Fixed.** `diag.rs` now has `drop_accent()`, `number_colour()` and one `card_head()` that both cards use, and the System log's accent rule is a method on `LogDev`. About 25 lines fewer, and the cards look the same.

4. **The table's column headings are drawn in two places** (`src/diag/page.rs`, minor, fixed). `table()` and `placeholder()` both place the same four headings (TIME, SINCE START, DEVICE, EVENT) at the same column positions, differing only in colour. If a column moves, both have to change. **Fixed.** One `heading_row()` draws them for both.

5. **Six popup boxes start with the same four lines** (`src/diag/page.rs` three times, `src/main.rs` three times, minor, fixed). Each one opens a floating area at a fixed position, allocates the box's rectangle so clicks don't fall through, makes a painter that reaches 40 px past the edge, and (in five of the six) paints the panel. One small helper would hold that once. **Fixed.** `popover()` in `fx.rs` does those steps, with an opacity for the two that fade, and all six use it.

6. **An empty `else` branch** (`src/diag/page.rs`, minor, fixed). In the Library list, `if let Some(b) = bresp { ... } else { // can't delete a recording in progress }` has an `else` that does nothing, with only a comment in it. The comment would read better on the `if`. **Fixed.** The comment now sits above the `if` and the empty `else` is gone.

## Looked at and left alone

- **Reading the capture file.** It reads in 4 MB chunks, restarts if the file shrinks, and a failed read is simply tried again on the next check, so a half-written file can't crash it.
- **The time strip** (`strip`). The gridlines are bounded by the length of the span, which comes from the capture's own timestamps. A corrupt file with absurd timestamps could make that very long, but I couldn't find a normal way to produce one.
- **Saving a report with a plain write.** The report goes where the file dialog chose, which already asks before overwriting, and it can be saved again, so it doesn't need the write-then-rename care that the positions and vendor files do.

- **`page()` is a long function** (about 240 lines) that picks the view, filters the table, measures the text, draws the page and handles the clicks. It reads in a clear order and I couldn't find a bug in it, and splitting it by length alone wouldn't make it easier to read, so I left it.
- **Rebuilding the table list** (`tl`) whenever the number of events changes. That's a pass over the events, and the capture only grows every couple of seconds.
- **Debug switches read inside the page loop** (`AIRSCOPE_INFO`, `AIRSCOPE_FILTER`). They look up the environment every frame, but it's a few microseconds a frame, so it isn't worth caching.

## Checked on screen

I built the previous commit and the new code and rendered the same demo screens with both in a headless compositor: the radar's info box, Diagnostics with a capture, its device info box, its help box, the Library list and the Spectrum help box. Five of the six came out pixel-for-pixel identical, and the radar one differed by about 3 pixels out of over a million, which is just the sweep being at a slightly different angle. I couldn't render the System log's version of the device box, because it reads the real journal, so that one shares its header code with the capture box but I haven't seen it.

## Not read

`src/fx.rs`, `src/demo.rs`, and the drawing inside `spectrum()` and `popup()` in `main.rs`.

## Verdict

Six findings, all minor, and all of them repeated or tangled code with no bugs behind it. All six are fixed, and the screenshots show no visible change. The Diagnostics code reads well otherwise, and I found nothing that could lose data. What's left to read is `src/fx.rs`, `src/demo.rs` and the drawing inside `spectrum()` and `popup()` in `main.rs`.
