# Ponytail audit, 2026-11-22 (follow-up, round 5: the drawing code)

This is the sixth pass of the audit, run with Claude Code using the ponytail audit skill, picking up from the "Not read" list in `ponytail-audit-2026-11-22-3.md`. Scope for this round: `src/fx.rs`, `src/demo.rs`, and the drawing inside `spectrum()` and `popup()` in `src/main.rs`. This log is written as I go. Nothing here was run on hardware other than my own machine, and the findings come from reading the code unless I say otherwise.

## Findings

1. **The Refresh and Undo icons were the same drawing written out twice** (`src/fx.rs`, minor, fixed). `Glyph::Undo` was a copy of `Glyph::Refresh` with the x axis flipped, about eight lines each, and both also worked out the arrowhead points twice over. Fixed. They share one match arm now, with a `flip` of 1 or -1, and the arrowhead is a small closure.

2. **A doc comment sat on the wrong function** (`src/fx.rs`, minor, fixed). "Subtle glow around a rectangle…" was stacked above `rounded_outline`, which has its own comment, rather than above `glow_rect`, which it describes. Moved.

3. **The demo's two device addresses were typed three times each** (`src/demo.rs`, minor, fixed). The headphones' and keyboard's MACs each appeared in the device list, the link list and the problem-injection check. A typo in one would quietly stop the demo problem showing. They are named constants now, and `start` reads the `AIRSCOPE_DEMO=bad` switch once instead of seven times.

4. **The 2.4 GHz channel to frequency sum was written twice, and the "clean channels" list in two forms** (`src/main.rs`, minor, fixed). The axis used `2407 + 5 * ch` and the tiles used a closure with the same sum, and `[1, 6, 11]` appeared twice for the axis next to a `chans` array for the tiles. There is now one `chan_f()` and one `CLEAN_CHANNELS`.

5. **Leftovers in `spectrum()`** (`src/main.rs`, minor, fixed). `bt_rows` returned an end position that the one caller threw away with `let _ = yend;`, `spectrum` took a `_t` argument it never used, and the pointer position was read a second time for the tiles when `ptr` already held it. All three removed.

6. **The info box laid out and drew its detail rows twice** (`src/main.rs`, minor, fixed). The main rows and the issue rows each built key/value text, summed the row heights and ran a draw loop, differing only in the key colour and the gap. `kv_layout()`, `kv_height()` and `kv_draw()` do that once now.

I also had the same slip as in round 4 while doing finding 4: my first edit put the new constants between `hill`'s doc comment and `hill`. I caught it on the next read and moved it back.

## Looked at and left alone

- **`spectrum()` is long** (about 330 lines) and `popup()` about 170. Both read in a clear order, I couldn't find a bug in either, and splitting by length alone wouldn't help, same call as `page()` last round.
- **The legend, labels and tiles each work out colours with `col_of`** several times a frame. It's a lookup in a list of at most a dozen names, so it isn't worth caching.
- **`Icon::Camera` is never constructed.** Already noted in an earlier round, still undecided, so I left it.
- **`demo.rs` clones all its data into the shared state every second.** It's demo only and costs nothing noticeable.

## Checked on screen

I built the last commit and the new code and rendered the same screens with both in a headless compositor. The Spectrum screen came out pixel-for-pixel identical. The radar with the headphones' info box and its inner issue box open (so both row loops ran) differed by about 150 pixels in over a million, which is the radar sweep being at a slightly different angle, and I looked at the new picture to make sure both boxes drew properly. The tests pass (32 passed, 8 ignored hardware ones).

## Not read

Nothing in the scope I set is left unread. Across all rounds, the only parts I'd call lightly read are the middle of `radar()` and the Devices list drawing in `main.rs`, which got a pass in round 3 but not a line-by-line one.

## Verdict

Six findings, all minor, and all of them repeated, misplaced or leftover code with no bugs behind it. All six are fixed and the screenshots show no visible change. That is the end of the scope list from round 2, so I think another full pass would mostly find more of the same kind of thing.
