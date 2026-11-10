# Airscope — style guide

How the app looks and behaves, so new screens and changes stay consistent. The code in `src/fx.rs` (theme and painted widgets) is the source of truth for sizes and colours; this file records the rules and the reasons.

## Principles

1. **Show, don't explain.** Use colour, shape, position and icons before words. No help paragraphs, no "how to use" hints on a screen (the radar's "scroll · drag · double-click" hint and the "SHOW" label were removed for this reason). A short caption is fine when a picture truly can't carry the meaning.
2. **Tooltips carry what the picture can't:** the name of an icon-only button, the full text of something shortened, and the current state of a toggle ("Showing Wi-Fi networks. Click to hide them."). Never a tooltip that only repeats the label. That something is clickable is shown by the pointing-hand cursor together with a visible hover reaction (glow, cyan border or popup), not by words: no "click for details" on radar nodes or cards. The hand cursor is not a desktop-app convention, so it is only ever used with that hover reaction.
3. **Nothing moves under the pointer.** Feedback (copied, saved, failed) is shown on the control itself, not as a message that pushes the layout around.
3a. **Nothing flickers.** A value, text or popup that updates by itself (a chip's "7s" counting up, a refreshed list) changes in place: no fade restarts, no blank frame, no relayout jump. Anything keyed to what it is (the widget asking), not to its current text.
4. **Report, don't touch.** The app observes. Privileged work goes through the desktop's own password prompt (`pkexec`); our own code never runs as root. No active Wi-Fi scan unless the user presses Rescan (scans briefly disturb Bluetooth on the shared chip).
5. **Honest numbers.** Show measured values (true dBm from nl80211) and say "unknown"/"(unnamed)" rather than inventing a value. A reading's source can change, so don't hide limits (the old percentage-based dBm tops out at −50).

## Colour meaning

Defined in `fx.rs`; use the constant, never a new hex value.

| Colour | Means |
|---|---|
| `AMBER` | Wi-Fi; also "other" / warning level |
| `BLUE` | Bluetooth; neutral panels and primary buttons |
| `GREEN` | LAN / network devices; healthy, live, success |
| `RED` | A real problem (radio-type drop), recording, stop/danger |
| `CYAN` | The app's accent: titles, selection, hover border, active tab |
| `MAGENTA` | Secondary filter ("Hide unnamed") and unnamed networks |
| `TEXT` / `DIM` | Normal text / secondary text and labels |
| `BG`, `PANEL_TOP`→`PANEL_BOT` | Background and the panel gradient |

Colour always has the same meaning on every screen: a Bluetooth thing is blue everywhere, a Wi-Fi thing is amber everywhere. Never use red for something that is merely "other".

## Type

- Headings and big numbers: the LCD face (`lcd`), cyan unless the value has a status colour (red/amber/green).
- Body text: proportional, 12–15 pt; labels and legends 11–13 pt in `DIM`.
- Monospace for identifiers and codes (addresses, reason codes like `0x08`, the header chips).
- Sentence case, short. Names of things as the user knows them: "BT Diagnostics", "LAN" (not "Network"), "Wi-Fi", "(unnamed)" for hidden networks, "link drops", "radio-type drop".

## Dates and times

- Dates: `MMM d, YYYY` ("Nov 8, 2026"), everywhere in the UI and reports (`snoop::date`).
- Times: 24-hour `HH:MM:SS` (or `HH:MM` on axes where seconds add nothing).
- File and folder names keep ISO (`2026-11-08`, `2026-11-08_18-12-25`) via `snoop::date_iso`; `snoop::pretty_stamp` shows a session folder in the UI format.
- A time axis that crosses midnight shows the date under the first label and under the first label of each later day.

## Layout

- Window minimum 1000×640. Sidebar 210 px, header 70 px, page padding 18 px, gaps 12 px. Every screen keeps 18 px between its content and the sidebar, header, right edge and bottom (the radar tray and zoom control, the Spectrum cards and the Devices list included); check new screens against the same left/top/right edges. Scrolling lists run their scroll area into the 18 px right margin (`scroll_in_margin` on Diagnostics; the Devices list is inset inside its scroll area) so the floating scrollbar (egui draws its own and reserves no width) sits beside the cards, never over them, and nothing is subtracted from the content width for it.
- Screen toolbar pattern: **row 1** = what you work on (search / file / record) with actions or status on the right; **row 2** = tabs on the left, filters on the right. One row each; don't wrap by default.
- Panels are rounded glass cards (`panel`): radius 12–14 for cards, 9–10 for buttons, 14 for pill chips. A panel's border colour is its meaning colour; hover turns the border cyan.
- Anything that can be long gets a defined behaviour (below), never overflow into a neighbour.

## Widgets (use the shared ones in `fx.rs`)

- **`action_button`**: glass pill, icon + label centred, 36 px high. Primary actions use `BLUE`, record/stop `RED`.
- **`view_tab`**: underlined text tab (cyan underline when active) for switching views. Tabs and buttons must look different.
- **`toggle_chip`**: lamp-style filter chip (filled dot = on, ring = off), always with a state tooltip. Filters sit right of the tabs with no "SHOW" label.
- **Legends**: a small `dot` (r≈4–5, meaning colour) then the label in `DIM`, like the sidebar legend and the drops strip. Not the colour's name in words.
- **Icons** (`Glyph`): line icons, drawn, not fonts. Every button gets one; the icon changes to a tick or cross for the result of an action.
- **Save report** always opens the desktop's own "save as" dialog (`picker::Pick::save`, via the file-chooser portal) with a suggested name, so it is an export to wherever the user picks; recordings keep their own report in the library already. The button shows a tick and the path when done, nothing if the dialog is cancelled.
- **Count badge** (`view_tab` badge): a small solid cyan circle with the number to the right of a tab's name (the Library tab shows how many saved captures there are, the Devices tabs how many devices): a circle for one digit that grows into a pill as the number gets longer; hidden at 0. It is flat, with no glow: glow is for things that are live, active or hovered (lamps, selected tab, halos), not for static indicators.
- **Destructive actions** (deleting a capture): an icon-only red button; clicking it turns that card's own line and buttons into "Delete this capture and its analysis? This can't be undone." with Cancel and a red Delete, in place, no dialog and no layout shift.
- **Date and time picker** (`dtpick::field`): a glass field showing "Nov 9, 2026  18:30" (or a dim placeholder; red outline when the range is invalid) that opens a calendar card below it: month with `<` `>`, Monday-first grid, today ringed, selected day filled cyan, hour and minute steppers and a Now button. Changes apply at once; a click outside or Escape closes it. The end of a range may be "Now" (a chip beside the field resets it).
- **Range dropdown** (System log): the same dropdown style as the recording length; canned ranges first (Since last recording, This boot, Last hour … Last 7 days), then Custom range.
- **Empty state:** show the real layout as dimmed placeholders with a centred title saying why they are empty; point at the buttons that start things (cyan pulsing outline, arrow and "Start here"), shown for 30 s or until the pointer first enters them, then never again: the marker file `~/.local/share/airscope/seen-start-here` records it (delete it to test the pointer again). The placeholder matches the tab: stat tiles, strip and cards for Summary, a table skeleton with the real column headings for Timeline.
- **Status lamp** (`lamp`): coloured dot with short word (REC, LIVE).

## Interaction

- **Cursor:** the pointing hand only on things that react to a click. Status chips and read-only graphics keep the normal cursor (`hit` sets the hand; use `ui.interact` for hover-only).
- **Click feedback:** a halo (glow with the button's own corner radius) fades in over ~0.15 s, holds ~2 s, fades out ~0.5 s; the icon becomes a tick (green) or cross (red). No layout change.
- **Hover details** use the shared boxed hover popup (`TipC` → `tip`, the style of the Spectrum hover): title in white, lines beneath in `TEXT`, border in the meaning colour, fade in/out ~0.12/0.15 s. One style everywhere: Spectrum hills and dots, the drops strip, the radar nodes and groups, and every `.hint()` text. Max width 340 px, fits its text, 12 px padding. Plain egui hover text is not used: any widget gets this popup at the pointer with `.hint("Title. Details.")` (a short text is just a title; near the top of the window the box flips below the pointer).
- **Interaction hint** (only where the action isn't obvious, e.g. radar orbs: draggable as well as clickable): the last row of a popup, under a thin divider (the popup's border colour at ~25% alpha), dim 11.5 pt, terse verbs: "Click for details · Drag to move". Info above, how to interact below. Plain clickable things (Devices rows, buttons) get no hint: the hand cursor and hover style say it. Set with `TipC.hint`.
- **Cursors:** pointing hand over anything clickable (with a visible hover reaction); open hand over the empty radar (pans); closed hand only while dragging (the radar background, an orb or a group).
- **Grouping:** things too dense to hover individually are merged visually (drops within 6 px become one thicker tick with a dot) and the hover lists the members.
- **Disabled** = no panel and `DIM` text; say why in the tooltip if it isn't obvious.

## Responsive behaviour

Shrink the least important information first, never drop it, and keep it reachable by hover.

1. Header status chips: full text (`WI-FI 12 APs · 7s`) → count (`WI-FI 12`) → name only (`WI-FI`); always a tooltip with the full text; no hand cursor.
2. Header buttons ("Vendor list", "Rescan Wi-Fi") become icon-only with tooltips when even name-only chips don't fit.
3. Diagnostics row 1: Record/Stop/Open/Copy/Save become icon-only below about 930 px of content width; the file name stays inline.
4. Long names are cut **in the middle** (`fit_middle`: `Game C…oller`, `bt11…snoop`) with the full name in the hover; long status text is cut at the end.
5. Side panels give way before graphs do (Spectrum legend is 22% of the width, 150–300 px).

## Behaviour conventions

- Time-based views: newest first for event lists; the timeline table has Time / Since start / Device / Event headings and wraps the event text.
- Large files are read in chunks and drawn virtualised; the UI never blocks on data.
- Radar data is latched to the sweep and frozen while a group is open or a drag is active (see `CLAUDE.md`).
- Copy/Save results are shown on the buttons; saved reports and captures live under `~/.local/share/airscope/`.

## Signal scale

One scale for Wi-Fi strength everywhere (`data::signal_frac`): 0 at −100 dBm, 1 at −10 dBm. The Spectrum graph's gridlines are −80/−60/−40/−20 with the unit "dBm" (cyan) at the top of that number column; hill height, radar dot size and the strength bars use the same function. Values are true dBm from nl80211 when available.

## Known gaps (not yet in line with this guide)

- (none known)
