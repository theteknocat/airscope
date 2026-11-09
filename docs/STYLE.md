# Airscope — style guide

How the app looks and behaves, so new screens and changes stay consistent. The code in `src/fx.rs` (theme and painted widgets) is the source of truth for sizes and colours; this file records the rules and the reasons.

## Principles

1. **Show, don't explain.** Use colour, shape, position and icons before words. No help paragraphs, no "how to use" hints on a screen (the radar's "scroll · drag · double-click" hint and the "SHOW" label were removed for this reason). A short caption is fine when a picture truly can't carry the meaning.
2. **Tooltips carry what the picture can't:** the name of an icon-only button, the full text of something shortened, and the current state plus what a click does ("Showing Wi-Fi networks. Click to hide them."). Never a tooltip that only repeats the label.
3. **Nothing moves under the pointer.** Feedback (copied, saved, failed) is shown on the control itself, not as a message that pushes the layout around.
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

- Window minimum 1000×640. Sidebar 210 px, header 70 px, page padding 18 px, gaps 12 px.
- Screen toolbar pattern: **row 1** = what you work on (search / file / record) with actions or status on the right; **row 2** = tabs on the left, filters on the right. One row each; don't wrap by default.
- Panels are rounded glass cards (`panel`): radius 12–14 for cards, 9–10 for buttons, 14 for pill chips. A panel's border colour is its meaning colour; hover turns the border cyan.
- Anything that can be long gets a defined behaviour (below), never overflow into a neighbour.

## Widgets (use the shared ones in `fx.rs`)

- **`action_button`**: glass pill, icon + label centred, 36 px high. Primary actions use `BLUE`, record/stop `RED`.
- **`view_tab`**: underlined text tab (cyan underline when active) for switching views. Tabs and buttons must look different.
- **`toggle_chip`**: lamp-style filter chip (filled dot = on, ring = off), always with a state tooltip. Filters sit right of the tabs with no "SHOW" label.
- **Legends**: a small `dot` (r≈4–5, meaning colour) then the label in `DIM`, like the sidebar legend and the drops strip. Not the colour's name in words.
- **Icons** (`Glyph`): line icons, drawn, not fonts. Every button gets one; the icon changes to a tick or cross for the result of an action.
- **Status lamp** (`lamp`): coloured dot with short word (REC, LIVE).

## Interaction

- **Cursor:** the pointing hand only on things that react to a click. Status chips and read-only graphics keep the normal cursor (`hit` sets the hand; use `ui.interact` for hover-only).
- **Click feedback:** a halo (glow with the button's own corner radius) fades in over ~0.15 s, holds ~2 s, fades out ~0.5 s; the icon becomes a tick (green) or cross (red). No layout change.
- **Hover details** use the shared boxed hover popup (`TipC` → `tip`, the style of the Spectrum hover): title in white, lines beneath in `TEXT`, border in the meaning colour, fade in/out ~0.12/0.15 s. One style everywhere: Spectrum hills, the drops strip. Radar nodes use the small pill version of the same. Plain egui hover text is for short tooltips only.
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

## Known gaps (not yet in line with this guide)

- The Spectrum screen still carries explanatory text: the footer line under the Bluetooth bars ("brighter channel = less Wi-Fi expected…"), the "quiet / crowded" gradient words and the three "How to read it" cards. They predate the principle; convert to visual cues where possible.
- The sidebar legend says "Network (LAN)" while chips say "LAN".
- Some tooltips (radar, Devices rows) predate the "state + click result" wording.
