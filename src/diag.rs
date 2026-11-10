//! Diagnostics screen: opens a Bluetooth capture file written by `btmon -w` (finished, or still growing) and reports
//! what happened to each device: drops with their reason, sniff-exit requests the controller answered late, bursts.
//! Reporting only: it never changes anything on the system.
use crate::Hint;

use crate::capture::{self, Outcome, Recording};
use crate::data::Icon;
use crate::fx::*;
use crate::picker::Pick;
use crate::snoop::{self, dur, line, mac, Ev, Kind, Snoop};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod logview;
mod page;

/// How far a scroll area reaches into the page's right margin, so its floating scrollbar sits beside the content, not over it.
const SCROLL_MARGIN: f32 = 18.0;

/// Runs `f` (a scroll area) in a child region that extends `SCROLL_MARGIN` past the right edge of the page content.
fn scroll_in_margin<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) {
    let r = ui.available_rect_before_wrap();
    let wide = Rect::from_min_max(r.min, pos2(r.right() + SCROLL_MARGIN, r.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(wide), |ui| {
        f(ui);
    });
}

/// Capture lengths offered when recording: label, seconds.
const CAPS: [(&str, u64); 4] = [("15 minutes", 900), ("1 hour", 3600), ("4 hours", 14400), ("12 hours", 43200)];

/// Known Bluetooth devices by upper-case address: (name, icon).
pub type Names = HashMap<String, (String, Icon)>;

pub struct Diag {
    path: String,
    snoop: Option<Snoop>,
    err: Option<String>,
    /// Bytes of the file read so far / its length (they differ while a big file is still loading).
    pos: u64,
    total: u64,
    /// Analysis kept between frames and redone only when new records arrive (`key` = records parsed).
    stats: Vec<snoop::DevStat>,
    bursts: Vec<(f64, f64, usize)>,
    key: u64,
    /// Timeline rows, newest first, as indices into the events (`tl_key` = events, problems-only, records).
    tl: Vec<usize>,
    tl_key: (u8, usize, u64, bool, Option<String>),
    /// How many of `tl` the table lists (grows a page at a time as the scroll nears the end).
    tl_limit: usize,
    /// Timeline layout: measured width of each event's message at the table font (grows as events are measured,
    /// a few thousand per frame) and the line height.
    ev_px: Vec<f32>,
    line_h: f32,
    checked: Instant,
    addrs: bool,
    problems_only: bool,
    /// Brief confirmation shown on a button: (when, which button: 0 copy / 1 save, outcome text).
    /// Hover box requested by the drops strip this frame; main.rs draws it with the shared tooltip style.
    pub tip: std::cell::RefCell<Option<crate::TipC>>,
    flash: Option<(Instant, u8, Result<String, String>)>,
    pick: Option<Pick>,
    /// A capture being recorded through pkexec (None when idle), the length chosen, and the last status line (error?, text).
    rec: Option<Recording>,
    cap_idx: usize,
    rec_msg: Option<(bool, String)>,
    /// When the status message was first shown (plain messages fade after a few seconds).
    msg_seen: Option<(String, Instant)>,
    /// A new recording to open as soon as btmon has written its header.
    follow: Option<PathBuf>,
    /// When new data last arrived from the file (drives the LIVE lamp).
    grew: Option<Instant>,
    /// A recording just ended: save its report and summary once the last bytes are read.
    finalize: bool,
    summarised: bool,
    sessions: Vec<capture::Session>,
    sessions_at: Option<Instant>,
    /// Rectangle around the Record and Open buttons (set each frame while drawing the toolbar; the empty state highlights it).
    cta: Option<Rect>,
    /// The system log source.
    log: logview::LogState,
    log_px: Vec<f32>,
    log_devs: Vec<page::LogDev>,
    log_devs_key: Option<(u64, bool, usize)>,
    /// Only this device's events are listed (card key), the card whose info box is open, where its card and box are on screen.
    dev_filter: Option<String>,
    info: Option<String>,
    info_card: Option<Rect>,
    info_rect: Option<Rect>,
    /// The Library picker: open?, the toolbar button it hangs from, where it is on screen.
    lib_open: bool,
    lib_btn: Option<Rect>,
    lib_rect: Option<Rect>,
    dbg_done: bool,
    /// A "save report as" dialog that is open, and whether it is for the System log (else the open capture).
    save_pick: Option<(Pick, bool)>,
    /// Library session folder waiting for the delete confirmation.
    del_pending: Option<PathBuf>,
    /// The "Start here" pointer: when it first showed, and when it began fading (30 s, or the first hover over the buttons; once per run).
    cta_since: Option<Instant>,
    /// Set when something points at Record (the ghost tile, the hint chip): the buttons glow for a few seconds.
    rec_pulse: Option<Instant>,
    cta_off: Option<Instant>,
    /// The "Start here" pointer was already shown in an earlier run (marker file `seen-start-here` in the data folder).
    cta_seen: Option<bool>,
    /// Library capture being analysed on a background thread, and whether it has finished.
    bg: Option<(PathBuf, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

impl Diag {
    pub fn new() -> Self {
        let mut d = Diag { path: std::env::var("AIRSCOPE_SNOOP").unwrap_or_default(), snoop: None, err: None, pos: 0, total: 0, stats: vec![], bursts: vec![], key: u64::MAX, tl: vec![], tl_limit: 1000, tl_key: (0, 0, u64::MAX, false, None), ev_px: vec![], line_h: 0.0, checked: Instant::now(), addrs: false, problems_only: true, flash: None, pick: None, rec: None, cap_idx: 1, rec_msg: None, msg_seen: None, follow: None, grew: None, finalize: false, summarised: false, sessions: vec![], sessions_at: None, cta: None, log: Default::default(), log_px: vec![], log_devs: vec![], log_devs_key: None, dev_filter: None, info: None, info_card: None, info_rect: None, lib_open: std::env::var("AIRSCOPE_LIBRARY").is_ok(), lib_btn: None, lib_rect: None, dbg_done: false, save_pick: None, del_pending: None, cta_since: None, rec_pulse: None, cta_off: None, cta_seen: None, bg: None, tip: Default::default() };
        if !d.path.is_empty() {
            d.load();
        } else if let Some(r) = Recording::adopt() {
            // a capture started by an earlier run of the app is still going: pick it up again
            d.follow = Some(r.file.clone());
            d.rec = Some(r);
        }
        d
    }

    fn clean_path(&self) -> String {
        let p = self.path.trim().trim_matches('\'').trim_matches('"');
        p.strip_prefix("file://").unwrap_or(p).to_string()
    }

    fn load(&mut self) {
        self.err = None;
        self.snoop = None;
        self.pos = 0;
        self.total = 0;
        self.key = u64::MAX;
        self.ev_px.clear();
        self.summarised = false;
        let p = self.clean_path();
        match std::fs::metadata(&p) {
            Ok(m) if m.is_file() => {
                self.snoop = Some(Snoop::default());
                self.total = m.len();
            }
            Ok(_) => self.err = Some(format!("{p} is not a file")),
            Err(e) => self.err = Some(format!("Cannot read {p}: {e}")),
        }
        self.checked = Instant::now();
    }

    /// Reads the file in chunks, one per frame while there is more to read, so a huge capture loads without freezing
    /// the window; once caught up it looks for new data every 2 s (the capture may still be running) and reads only
    /// the new bytes.
    fn pump(&mut self) {
        const CHUNK: u64 = 4 << 20;
        if self.snoop.is_none() {
            return;
        }
        if self.pos >= self.total && self.checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.checked = Instant::now();
        let p = self.clean_path();
        let Ok(m) = std::fs::metadata(&p) else { return };
        if m.len() < self.pos {
            self.load(); // replaced or truncated: start over
            return;
        }
        self.total = m.len();
        if self.pos >= self.total {
            return;
        }
        let pos = self.pos;
        let mut buf = vec![0u8; (self.total - pos).min(CHUNK) as usize];
        if std::fs::File::open(&p).and_then(|mut f| f.seek(SeekFrom::Start(pos)).and_then(|_| f.read_exact(&mut buf))).is_err() {
            return;
        }
        self.pos += buf.len() as u64;
        self.grew = Some(Instant::now());
        if let Some(s) = self.snoop.as_mut() {
            if let Err(e) = s.feed(&buf) {
                self.err = Some(e);
                self.snoop = None;
            }
        }
    }

    fn who(&self, a: &snoop::Addr, names: &Names) -> String {
        if *a == snoop::UNKNOWN {
            return "Link from before the capture started".into();
        }
        let m = mac(a);
        let name = names.get(&m).map(|n| n.0.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Device ..{}", &m[12..]));
        if self.addrs { format!("{name} ({m})") } else { name }
    }

    fn problem(e: &Ev) -> bool {
        match &e.kind {
            Kind::Disconnected { .. } | Kind::ModeFailed { .. } => true,
            _ => Snoop::slow_exit(e),
        }
    }

    fn colour(e: &Ev) -> egui::Color32 {
        match &e.kind {
            Kind::Disconnected { reason, .. } => if crate::hci::reason(*reason).2 { RED } else { AMBER },
            Kind::ModeFailed { .. } => AMBER,
            _ if Snoop::slow_exit(e) => AMBER,
            _ => DIM,
        }
    }

    /// The summary as plain text lines, used both on screen and in the saved report.
    fn summary(&self, s: &Snoop, names: &Names) -> Vec<(egui::Color32, String)> {
        let mut out: Vec<(egui::Color32, String)> = vec![];
        let first = s.first_us.map_or(0.0, |f| (f - 0x00dc_ddb3_0f2f_8000i64) as f64 / 1e6);
        out.push((TEXT, format!("{} records, {} data packets, spanning {} ({} {} to {})", s.records, s.acl, dur(s.duration()), snoop::date(first), snoop::clock(first), snoop::clock(first + s.duration()))));
        let drops = s.drops().count();
        out.push((if drops > 0 { RED } else { TEXT }, format!("{drops} link drops in total")));
        let b = s.bursts(120.0);
        if drops > 0 {
            out.push((TEXT, format!("{} burst(s) of drops (separated by quiet stretches of over 2 minutes):", b.len())));
            for (a, z, n) in b.iter().take(12) {
                out.push((DIM, format!("   {} to {}: {n} drop(s)", snoop::clock(first + a), snoop::clock(first + z))));
            }
            if b.len() > 12 {
                out.push((DIM, format!("   ... and {} more", b.len() - 12)));
            }
        }
        for d in s.per_device() {
            out.push((CYAN, String::new()));
            out.push((CYAN, self.who(&d.addr, names)));
            out.push((TEXT, format!("   {} connects, {} drops, data packets: {} sent / {} received", d.connects, d.drops, d.tx, d.rx)));
            for (r, n) in &d.reasons {
                let (name, why, radio) = crate::hci::reason(*r);
                out.push((if radio { RED } else { AMBER }, format!("   {n} x {name} (0x{r:02x}): {why}")));
            }
            if let Some(g) = d.median_gap {
                out.push((TEXT, format!("   typical time between drops {}; links lived {} typically, {} at the shortest", dur(g), d.median_life.map_or("?".into(), dur), d.shortest_life.map_or("?".into(), dur))));
            }
            if let Some(q) = d.median_quiet {
                out.push((TEXT, format!("   typically {} without any data before a drop", dur(q))));
            }
            if d.exits > 0 {
                out.push((if d.slow_exits > 0 { AMBER } else { TEXT }, format!("   sniff-exit requests: {} sent, {} answered late or not at all; {} of {} drops came within 30 s after one", d.exits, d.slow_exits, d.after_stall, d.drops)));
            }
        }
        out
    }

    fn report(&self, s: &Snoop, names: &Names) -> String {
        let mut r = format!("# Bluetooth capture report\n\nFile: {}\n\n## Summary\n\n", snoop::pretty_stamp(&self.shown_name()));
        for (_, l) in self.summary(s, names) {
            r += &format!("{l}\n");
        }
        r += "\n## Events\n\n```text\n";
        for e in s.events.iter().filter(|e| Self::problem(e) || !self.problems_only) {
            r += &line(e, &self.who(&e.addr, names));
            r.push('\n');
        }
        r += "```\n\nReporting only: this lists what the capture shows; it does not say what to change.\n";
        if !self.addrs {
            r += "Device addresses are left out.\n";
        }
        r
    }

    /// Opens the desktop's save dialog for the report (the capture's, or the System log's).
    fn begin_save(&mut self, log: bool) {
        if self.save_pick.is_some() {
            return;
        }
        let name = if log { self.log_report_name() } else { format!("{}-report.md", self.shown_name().trim_end_matches(".snoop")) };
        self.save_pick = Some((Pick::save("Save report as", &name), log));
    }

    /// Writes the report where the user chose, and says so on the Save button.
    fn finish_save(&mut self, file: PathBuf, log: bool, s: Option<&Snoop>, names: &Names) {
        let text = if log { Some(self.log_report(names)) } else { s.map(|s| self.report(s, names)) };
        let Some(text) = text else { return };
        let res = match std::fs::write(&file, text) {
            Ok(()) => Ok(format!("Saved to {}", file.display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1))),
            Err(e) => Err(format!("Could not save: {e}")),
        };
        self.flash = Some((Instant::now(), 1, res));
    }

    /// The capture's name for display: a library capture is named by when it was started.
    fn shown_name(&self) -> String {
        let p = self.clean_path();
        let path = Path::new(&p);
        let f = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if f == capture::CAPTURE_FILE {
            path.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()).unwrap_or(f).to_string()
        } else {
            f.to_string()
        }
    }

    fn first_unix(s: &Snoop) -> f64 {
        s.first_us.map_or(0.0, |f| (f - 0x00dc_ddb3_0f2f_8000i64) as f64 / 1e6)
    }

    /// Opens a capture file (and forgets the device filter and info box of the one before).
    fn open_path(&mut self, path: String) {
        self.path = path;
        self.dev_filter = None;
        self.info = None;
        self.load();
    }

    /// Closes the open capture: the page goes back to the system log.
    fn close_capture(&mut self) {
        self.path.clear();
        self.snoop = None;
        self.err = None;
        self.pos = 0;
        self.total = 0;
        self.key = u64::MAX;
        self.ev_px.clear();
        self.rec_msg = None;
        self.dev_filter = None;
        self.info = None;
    }

    /// Page header for a capture: its date and time with loading progress, the LIVE lamp (recordings only) and the
    /// latest status message; the Close button on the right.
    fn name_row(&mut self, ui: &mut egui::Ui) {
        let name = snoop::pretty_stamp(&self.shown_name());
        let sub = if self.rec.is_some() { "Live recording" } else { "Recording" };
        let loading = (self.pos < self.total).then(|| self.pos * 100 / self.total);
        let live = self.rec.is_some() && self.grew.map_or(false, |t| t.elapsed() < Duration::from_secs(6));
        if let Some((err, m)) = self.rec_msg.clone() {
            if self.msg_seen.as_ref().map_or(true, |(s, _)| *s != m) {
                self.msg_seen = Some((m.clone(), Instant::now()));
            }
            let age = self.msg_seen.as_ref().map_or(0.0, |(_, t)| t.elapsed().as_secs_f32());
            if !err && age > 6.0 && !m.starts_with("Waiting") {
                self.rec_msg = None;
            } else if !err {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        } else {
            self.msg_seen = None;
        }
        let msg = self.rec_msg.clone();
        let can_close = self.rec.is_none() && !self.path.is_empty();
        let mut close = false;
        let aw = ui.available_width();
        Diag::header(ui, sub, |ui| {
            if name.is_empty() {
                ui.label(RichText::new("\u{2026}").color(DIM));
            } else {
                let font = egui::FontId::proportional(15.0);
                let shown = crate::fit_middle(ui.painter(), &name, font, (aw - 400.0).max(120.0));
                let r = ui.label(RichText::new(&shown).color(TEXT).size(15.0));
                if shown != name {
                    r.hint(name.clone());
                }
            }
            if let Some(l) = loading {
                ui.label(RichText::new(format!("loading {l}%")).color(DIM));
            }
            if live {
                lamp(ui, GREEN, "LIVE", true);
            }
            if let Some((err, m)) = &msg {
                ui.add(egui::Label::new(RichText::new(m).size(12.5).color(if *err { RED } else { DIM })).truncate());
            }
        }, |ui| {
            if can_close && action_button_h(ui, Some(Glyph::Back), "System log", BLUE, 0.0, true, 0.0, page::CTL_H).hint("Back to system log").clicked() {
                close = true;
            }
        });
        if close {
            self.close_capture();
        }
    }

    fn start_record(&mut self) {
        match Recording::start(Duration::from_secs(CAPS[self.cap_idx].1)) {
            Ok(r) => {
                self.follow = Some(r.file.clone());
                self.rec = Some(r);
                self.rec_msg = Some((false, "Waiting for your password...".into()));
            }
            Err(e) => self.rec_msg = Some((true, e)),
        }
    }

    /// The "Start here" pointer: a pulsing outline round the Record and Open buttons with an arrow and label, shown for
    /// 30 s or until the pointer first enters them (once per run). Not while a recording is starting.
    fn start_here(&mut self, ui: &mut egui::Ui) {
        if self.rec.is_some() {
            return;
        }
        let marker = capture::captures_dir().and_then(|d| d.parent().map(|p| p.join("seen-start-here")));
        if *self.cta_seen.get_or_insert_with(|| marker.as_ref().map_or(false, |m| m.exists())) {
            return;
        }
        // highlight the toolbar buttons and point at them: shown for 30 s, or until the pointer first enters them
        if let Some(b) = self.cta {
            let since = *self.cta_since.get_or_insert_with(Instant::now);
            let hot = ui.input(|i| i.pointer.hover_pos()).map_or(false, |q| b.expand2(vec2(10.0, 7.0)).contains(q));
            if self.cta_off.is_none() && (hot || since.elapsed() > Duration::from_secs(30)) {
                self.cta_off = Some(Instant::now());
                if let Some(m) = &marker {
                    if let Some(d) = m.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(m, "shown\n"); // remembered: delete this file to see the pointer again
                }
            }
            let fade = self.cta_off.map_or(1.0, |t| 1.0 - (t.elapsed().as_secs_f32() / 0.5).min(1.0));
            if fade > 0.0 {
                let p = ui.painter().clone();
                let pulse = 0.5 + 0.5 * (ui.input(|i| i.time) as f32 * 2.4).sin();
                let hr = b.expand2(vec2(10.0, 7.0));
                glow_rect_r(&p, hr, 14.0, 14.0, CYAN, (0.25 + 0.2 * pulse) * fade);
                p.rect_stroke(hr, 14.0, Stroke::new(1.6, alpha(CYAN, (0.55 + 0.35 * pulse) * fade)), egui::StrokeKind::Outside);
                let (ax, ay) = (hr.right() + 22.0, hr.center().y);
                let st = Stroke::new(2.0, alpha(CYAN, fade));
                p.line_segment([pos2(ax, ay), pos2(ax + 22.0, ay)], st);
                p.line_segment([pos2(ax, ay), pos2(ax + 7.0, ay - 6.0)], st);
                p.line_segment([pos2(ax, ay), pos2(ax + 7.0, ay + 6.0)], st);
                p.text(pos2(ax + 32.0, ay), Align2::LEFT_CENTER, "Start here", FontId::proportional(17.0), alpha(CYAN, fade));
                ui.ctx().request_repaint();
            }
        }
    }

    /// Width of a toolbar button with this label (icon, gap, text, padding), as `action_button` makes it.
    fn button_w(ui: &egui::Ui, label: &str, min_w: f32) -> f32 {
        (ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(14.0), Color32::WHITE).size().x + 66.0).max(min_w)
    }

    /// Row 1: Record (or Stop) with its length on the left; Library, Copy report and Export on the right. When the
    /// window is too narrow for the labels the buttons shrink to icons; the breakpoint comes from the real label widths.
    fn toolbar(&mut self, ui: &mut egui::Ui, s: Option<&Snoop>, names: &Names) {
        let cap_mode = s.is_some() || !self.path.is_empty() || self.rec.is_some();
        let reports = if cap_mode { s.is_some() } else { true };
        let w_of = |t: &str| ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(14.0), Color32::WHITE).size().x;
        let widest = CAPS.iter().map(|c| w_of(c.0)).fold(0.0, f32::max);
        let left_full = Self::button_w(ui, "Record", 0.0) + 12.0 + w_of("for") + 12.0 + widest + 40.0;
        let right_full = Self::button_w(ui, "Library", 0.0) + 12.0 + if reports { Self::button_w(ui, "Copy report", 0.0) + Self::button_w(ui, "Export", 0.0) + 24.0 } else { 0.0 };
        let stacked = ui.available_width() < left_full.max(260.0) + right_full + 30.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if self.rec.is_some() {
                if action_button(ui, Some(Glyph::Stop), if stacked { "" } else { "Stop" }, RED, 0.6, true, 0.0).hint("Stop the recording (asks for your password)").clicked() {
                    if let Some(r) = self.rec.as_mut() {
                        self.rec_msg = Some(match r.stop() {
                            Ok(()) => (false, "Enter your password to end the capture.".to_string()),
                            Err(e) => (true, e),
                        });
                    }
                }
                if let Some(r) = &self.rec {
                    lamp(ui, RED, &format!("REC  {} left", hms(r.left().as_secs())), true);
                }
            } else {
                let rb = action_button(ui, Some(Glyph::Record), if stacked { "" } else { "Record" }, RED, 0.0, true, 0.0).hint("Record Bluetooth activity (asks for your password)");
                if rb.clicked() {
                    self.start_record();
                }
                ui.label(RichText::new("for").color(DIM));
                let cb = egui::ComboBox::from_id_salt("cap_len").selected_text(CAPS[self.cap_idx].0).show_ui(ui, |ui| {
                    for (i, (l, _)) in CAPS.iter().enumerate() {
                        ui.selectable_value(&mut self.cap_idx, i, *l);
                    }
                });
                self.cta = Some(rb.rect.union(cb.response.rect));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                // right to left: Export is added first, so it sits on the right
                if reports {
                    for (idx, normal, done, tip) in [(1u8, "Export", "Saved", "Export the report to a file"), (0u8, "Copy report", "Copied", "Copy the report to the clipboard")] {
                        let f = self.flash.as_ref().filter(|f| f.1 == idx && f.0.elapsed() < Duration::from_millis(2500));
                        let el = f.map_or(0.0, |f| f.0.elapsed().as_secs_f32());
                        // fade in over 0.15 s, hold, fade out over the last 0.5 s
                        let fade = if f.is_none() { 0.0 } else if el < 0.15 { el / 0.15 } else if el < 2.0 { 1.0 } else { 1.0 - (el - 2.0) / 0.5 };
                        let t = fade * fade * (3.0 - 2.0 * fade);
                        let tint = if matches!(f, Some((_, _, Err(_)))) { RED } else { GREEN };
                        let label = match f {
                            Some((_, _, Err(_))) if el > 0.1 && el < 2.35 => "Failed",
                            Some(_) if el > 0.1 && el < 2.35 => done,
                            _ => normal,
                        };
                        let result = f.map(|f| match &f.2 {
                            Ok(t) | Err(t) => t.clone(),
                        });
                        // the icon turns into a tick (or a cross) while the result shows
                        let ic = if label == normal { if idx == 1 { Glyph::Save } else { Glyph::Copy } } else if tint == RED { Glyph::Cross } else { Glyph::Check };
                        let r = action_button(ui, Some(ic), if stacked { "" } else { label }, mix(BLUE, tint, t), t, true, if stacked { 0.0 } else { [normal, done, "Failed"].iter().map(|l| Self::button_w(ui, l, 0.0)).fold(0.0, f32::max) });
                        let r = r.hint(result.unwrap_or_else(|| tip.to_string()));
                        if r.clicked() {
                            if !cap_mode {
                                if idx == 1 {
                                    self.begin_save(true);
                                } else {
                                    ui.ctx().copy_text(self.log_report(names));
                                    self.flash = Some((Instant::now(), 0, Ok("Report copied to the clipboard".into())));
                                }
                            } else if let Some(s) = s {
                                if idx == 1 {
                                    self.begin_save(false);
                                } else {
                                    ui.ctx().copy_text(self.report(s, names));
                                    self.flash = Some((Instant::now(), 0, Ok("Report copied to the clipboard".into())));
                                }
                            }
                        }
                    }
                }
                let lb = action_button(ui, Some(Glyph::Open), if stacked { "" } else { "Library" }, BLUE, if self.lib_open { 0.0 } else { 0.0 }, true, 0.0).hint("Open a saved recording, or a capture file");
                self.lib_btn = Some(lb.rect);
                if lb.clicked() {
                    self.lib_open = !self.lib_open;
                }
            });
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, names: &Names) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        if let Some(p) = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).find(|s| !s.is_empty())) {
            self.open_path(p);
        }
        if let Some(res) = self.pick.as_ref().and_then(|p| p.poll()) {
            self.pick = None;
            match res {
                Ok(Some(path)) => self.open_path(path.display().to_string()),
                Ok(None) => {}
                Err(e) => self.err = Some(e),
            }
        }
        self.cta = None;
        if self.sessions_at.map_or(true, |t| t.elapsed() > Duration::from_secs(3)) {
            self.sessions = capture::list();
            self.sessions_at = Some(Instant::now());
        }
        let outcome = self.rec.as_mut().map(|r| r.poll());
        match outcome {
            Some(Outcome::Finished) => {
                self.rec = None;
                self.finalize = true;
                self.checked = Instant::now() - Duration::from_secs(3); // look at the file again right away
                self.rec_msg = Some((false, "Capture finished and saved to the library.".into()));
            }
            Some(Outcome::Failed(m)) => {
                self.rec = None;
                self.follow = None;
                self.rec_msg = Some((true, m));
            }
            _ => {}
        }
        if let Some(f) = self.follow.clone() {
            if std::fs::metadata(&f).map_or(false, |m| m.len() > 16) {
                self.follow = None;
                self.rec_msg = None;
                self.open_path(f.display().to_string());
            }
        }
        self.pump();
        let s = self.snoop.take();
        if let Some(res) = self.save_pick.as_ref().and_then(|(p, _)| p.poll()) {
            let log = self.save_pick.take().map_or(false, |x| x.1);
            match res {
                Ok(Some(path)) => self.finish_save(path, log, s.as_ref(), names),
                Ok(None) => {} // cancelled
                Err(e) => self.flash = Some((Instant::now(), 1, Err(e))),
            }
        }
        if let Some(s) = &s {
            if self.key != s.records {
                self.stats = s.per_device();
                self.bursts = s.bursts(120.0);
                self.key = s.records;
            }
            // a capture in the library: keep its summary (and, when a recording has just ended, its report) up to date
            if self.pos >= self.total {
                if let Some(dir) = capture::session_dir_of(Path::new(&self.clean_path())) {
                    if self.finalize {
                        let _ = std::fs::write(dir.join(capture::REPORT_FILE), self.report(s, names));
                        self.finalize = false;
                        self.summarised = false;
                    }
                    if self.rec.is_none() && !self.summarised {
                        capture::write_summary(&dir, &capture::Summary { records: s.records, drops: s.drops().count(), duration: s.duration() });
                        self.summarised = true;
                        self.sessions_at = None;
                    }
                } else {
                    self.finalize = false;
                }
            }
        }
        egui::Frame::NONE.inner_margin(18.0).show(ui, |ui| {
            self.toolbar(ui, s.as_ref(), names);
            ui.add_space(10.0);
            ui.separator();
            if let Some(e) = &self.err {
                ui.label(RichText::new(e).color(RED));
            }
            self.page(ui, s.as_ref(), names);
        });
        // the Library picker hangs from the toolbar button; a click elsewhere or Esc closes it
        if self.lib_open {
            let was_open = self.lib_rect.is_some();
            self.library_popover(ui.ctx(), rect);
            let (esc, outside) = ui.input(|i| {
                let q = i.pointer.interact_pos();
                (i.key_pressed(egui::Key::Escape), i.pointer.primary_clicked() && q.map_or(false, |q| !self.lib_rect.map_or(false, |r| r.contains(q)) && !self.lib_btn.map_or(false, |r| r.contains(q))))
            });
            if self.lib_open && was_open && (esc || outside) {
                self.lib_open = false;
                self.del_pending = None;
            }
        }
        if self.sessions.is_empty() && s.is_none() && self.path.is_empty() && !self.lib_open {
            self.start_here(ui);
        }
        if let (Some(t), Some(b)) = (self.rec_pulse, self.cta) {
            let age = t.elapsed().as_secs_f32();
            if age > 3.5 || self.rec.is_some() {
                self.rec_pulse = None;
            } else {
                let fade = (3.5 - age).min(1.0);
                let pulse = 0.5 + 0.5 * (ui.input(|i| i.time) as f32 * 2.4).sin();
                let hr = b.expand2(vec2(10.0, 7.0));
                glow_rect_r(ui.painter(), hr, 14.0, 14.0, CYAN, (0.25 + 0.2 * pulse) * fade);
                ui.painter().rect_stroke(hr, 14.0, Stroke::new(1.6, alpha(CYAN, (0.55 + 0.35 * pulse) * fade)), egui::StrokeKind::Outside);
                ui.ctx().request_repaint();
            }
        }
        // opening or closing a capture above replaced `self.snoop`; otherwise put the one taken out back
        if self.snoop.is_none() && self.err.is_none() && !self.path.is_empty() {
            self.snoop = s;
        }
    }
}

/// A tile. An empty `value` makes a ghost: what a recording would show here but the system log can't. True when clicked.
fn tile(ui: &mut egui::Ui, w: f32, label: &str, value: &str, col: Color32) -> bool {
    let ghost = value.is_empty();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 78.0), if ghost { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if ghost {
        let hot = resp.hovered();
        p.rect_filled(r, 12.0, alpha(Color32::WHITE, if hot { 0.06 } else { 0.025 }));
        p.rect_stroke(r, 12.0, Stroke::new(1.0, alpha(DIM, if hot { 0.7 } else { 0.4 })), egui::StrokeKind::Inside);
        p.text(pos2(r.left() + 18.0, r.top() + 20.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), alpha(DIM, 0.8));
        glyph(p, Glyph::Record, pos2(r.left() + 24.0, r.top() + 49.0), 5.5, RED);
        p.text(pos2(r.left() + 38.0, r.top() + 49.0), Align2::LEFT_CENTER, "Needs a recording", FontId::proportional(14.0), alpha(DIM, if hot { 1.0 } else { 0.8 }));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint("The system log can't count data packets. A recording can: click to see where to start one.");
        return resp.clicked();
    }
    panel(p, r, 12.0, col);
    p.text(pos2(r.left() + 18.0, r.top() + 20.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), DIM);
    lcd(p, pos2(r.left() + 18.0, r.top() + 49.0), Align2::LEFT_CENTER, value, 26.0, col);
    false
}

/// What a time strip is drawn for: where it starts (unix seconds) and how long it runs, its title and legend, and what to
/// call a group of ticks ("link drops", "problems").
pub struct StripSpec {
    pub first: f64,
    pub total: f64,
    pub title: &'static str,
    pub legend: (&'static str, &'static str),
    pub what: &'static str,
}

/// One mark on the strip: seconds from its start, its moment, red or amber, who it is about, an extra line for the popup.
pub struct Tick {
    pub t: f64,
    pub unix: f64,
    pub radio: bool,
    pub who: String,
    pub detail: Option<String>,
}

/// When the events happened across a span: one tick per event (red = a real link problem, amber = other), bursts
/// shaded. Used for a capture's drops and for the system log's problems.
fn strip(ui: &mut egui::Ui, w: f32, spec: &StripSpec, ticks: &[Tick], bursts: &[(f64, f64, usize)], tip: &std::cell::RefCell<Option<crate::TipC>>) {
    let first = spec.first;
    let total = spec.total.max(1.0);
    let crosses = snoop::date(first) != snoop::date(first + total); // spans midnight: dates are shown under some times
    let extra = if crosses { 14.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 96.0 + extra), Sense::hover());
    let p = ui.painter();
    panel(p, r, 12.0, BLUE);
    p.text(pos2(r.left() + 18.0, r.top() + 18.0), Align2::LEFT_CENTER, spec.title, FontId::proportional(12.0), DIM);
    // legend: a coloured dot and a short label, like the sidebar legend (right to left)
    let mut lx = r.right() - 18.0;
    for (col, label) in [(AMBER, spec.legend.0), (RED, spec.legend.1)] {
        let g = p.layout_no_wrap(label.to_string(), FontId::proportional(11.5), DIM);
        let wd = g.size().x;
        p.galley(pos2(lx - wd, r.top() + 18.0 - g.size().y / 2.0), g, DIM);
        dot(p, pos2(lx - wd - 12.0, r.top() + 18.0), 4.0, col, 0.0);
        lx -= wd + 12.0 + 4.0 + 18.0;
    }
    let a = Rect::from_min_max(pos2(r.left() + 18.0, r.top() + 34.0), pos2(r.right() - 18.0, r.bottom() - 26.0 - extra));
    let x = |t: f64| a.left() + (t / total) as f32 * a.width();
    p.rect_filled(a, 4.0, alpha(Color32::WHITE, 0.04));
    // faint vertical gridlines on round local times; the step is chosen from the whole span so there are 5-10 of them
    let off = utc_offset(first);
    let step = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0, 10800.0, 21600.0, 43200.0, 86400.0].into_iter().find(|st| total / st <= f64::from((a.width() / 90.0).clamp(4.0, 10.0))).unwrap_or(86400.0);
    let mut grid: Vec<(f32, f64)> = vec![];
    let mut k = ((first + off) / step).ceil();
    while k * step - off <= first + total {
        let u = k * step - off;
        grid.push((a.left() + ((u - first) / total) as f32 * a.width(), u));
        k += 1.0;
    }
    for (gx, _) in &grid {
        p.line_segment([pos2(*gx, a.top()), pos2(*gx, a.bottom())], Stroke::new(1.0, alpha(Color32::WHITE, 0.07)));
    }
    for (t0, t1, _) in bursts {
        p.rect_filled(Rect::from_min_max(pos2(x(*t0) - 3.0, a.top()), pos2((x(*t1) + 3.0).max(x(*t0) + 3.0), a.bottom())), 3.0, alpha(RED, 0.14));
    }
    // Drops that fall within a few pixels of each other are drawn as one (wider) tick; hovering a tick lists what is in it.
    let radio = |e: &Tick| e.radio;
    let mut groups: Vec<(f32, f32, Vec<&Tick>)> = vec![];
    for e in ticks {
        let px = x(e.t);
        match groups.last_mut() {
            Some(g) if px - g.1 <= 6.0 => {
                g.1 = px;
                g.2.push(e);
            }
            _ => groups.push((px, px, vec![e])),
        }
    }
    let ptr = resp.hover_pos().filter(|q| a.expand2(vec2(8.0, 6.0)).contains(*q));
    let hot = ptr.and_then(|q| groups.iter().position(|g| q.x >= g.0 - 5.0 && q.x <= g.1 + 5.0));
    let hot = hot.or(std::env::var("AIRSCOPE_STRIP_HOVER").ok().and_then(|v| v.parse::<usize>().ok()).filter(|i| *i < groups.len())); // debug aid: pretend the pointer is on this tick
    for (i, g) in groups.iter().enumerate() {
        let col = if g.2.iter().any(|e| radio(e)) { RED } else { AMBER };
        let cx = (g.0 + g.1) / 2.0;
        let (y0, y1) = (a.top() + 4.0, a.bottom() - 4.0);
        if hot == Some(i) {
            p.line_segment([pos2(cx, y0), pos2(cx, y1)], Stroke::new(8.0, alpha(col, 0.25)));
        }
        let many = g.2.len() > 1;
        p.line_segment([pos2(cx, y0), pos2(cx, y1)], Stroke::new(if many { 3.2 } else { 2.0 }, col));
        if many {
            p.circle_filled(pos2(cx, y0), 3.4, col);
        }
    }
    if let Some(i) = hot {
        let g = &groups[i];
        let cx = (g.0 + g.1) / 2.0;
        let col = if g.2.iter().any(|e| radio(e)) { RED } else { AMBER };
        let full = |e: &Tick| format!("{} {}", snoop::date(e.unix), snoop::clock(e.unix));
        let (title, lines) = if g.2.len() == 1 {
            (g.2[0].who.clone(), { let mut l = vec![full(g.2[0])]; l.extend(g.2[0].detail.clone()); l })
        } else {
            let day = snoop::date(g.2[0].unix);
            let one_day = g.2.iter().all(|e| snoop::date(e.unix) == day);
            let mut lines: Vec<String> = g.2.iter().take(10).map(|e| format!("{}  \u{00b7}  {}", if one_day { snoop::clock(e.unix) } else { full(e) }, e.who)).collect();
            if g.2.len() > 10 {
                lines.push(format!("and {} more", g.2.len() - 10));
            }
            (if one_day { format!("{} {} \u{00b7} {day}", g.2.len(), spec.what) } else { format!("{} {}", g.2.len(), spec.what) }, lines)
        };
        *tip.borrow_mut() = Some(crate::TipC { anchor: pos2(cx, a.top()), col, title, sub: lines, key: None, hint: None, side: None });
    }
    // times under the gridlines (as many as fit without touching), the exact start and end at the edges; when the span
    // crosses midnight the start and the first label of each later day also carry the date
    let font = FontId::proportional(11.5);
    let small = FontId::proportional(10.0);
    let y = a.bottom() + 6.0;
    let width_of = |t: &str| p.layout_no_wrap(t.to_string(), font.clone(), DIM).size().x;
    let mut used: Vec<(f32, f32)> = vec![];
    let mut put = |x: f32, align: Align2, text: String, date: Option<String>| {
        let wdt = width_of(&text).max(date.as_ref().map_or(0.0, |d| p.layout_no_wrap(d.clone(), small.clone(), DIM).size().x));
        let (l, rr) = match align {
            Align2::LEFT_TOP => (x, x + wdt),
            Align2::RIGHT_TOP => (x - wdt, x),
            _ => (x - wdt / 2.0, x + wdt / 2.0),
        };
        if used.iter().any(|(ul, ur)| l < *ur + 10.0 && rr > *ul - 10.0) || l < r.left() + 4.0 || rr > r.right() - 4.0 {
            return false;
        }
        used.push((l, rr));
        p.text(pos2(x, y), align, text, font.clone(), DIM);
        if let Some(dt) = date {
            p.text(pos2(x, y + 14.0), align, dt, small.clone(), DIM);
        }
        true
    };
    put(a.left(), Align2::LEFT_TOP, snoop::clock(first), crosses.then(|| snoop::date(first)));
    put(a.right(), Align2::RIGHT_TOP, snoop::clock(first + total), None);
    let mut day = snoop::date(first);
    for (gx, u) in &grid {
        let dt = snoop::date(*u);
        let new_day = dt != day;
        day = dt.clone();
        if step >= 86400.0 {
            // whole days: the date is the label (no time of day to show)
            put(*gx, Align2::CENTER_TOP, dt.split(',').next().unwrap_or("").to_string(), None);
            continue;
        }
        let text = if step < 60.0 { snoop::clock(*u) } else { snoop::clock(*u)[..5].to_string() };
        put(*gx, Align2::CENTER_TOP, text, (crosses && new_day).then_some(dt));
    }
}

/// Seconds east of UTC for local time at `unix`.
fn utc_offset(unix: f64) -> f64 {
    let secs = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    tm.tm_gmtoff as f64
}

fn card_h(d: &snoop::DevStat) -> f32 {
    78.0 + 22.0 * d.reasons.len().max(1) as f32 + 10.0 + 66.0 + if d.exits > 0 { 66.0 } else { 0.0 } + 14.0
}

/// The full per-device card, painted into `r` (the info box gives it extra room below for its button).
fn card_at(p: &egui::Painter, r: Rect, d: &snoop::DevStat, name: &str, ic: Icon) {
    let radio = d.reasons.iter().any(|(c, _)| crate::hci::reason(*c).2);
    let accent = if radio { RED } else if d.drops > 0 { AMBER } else { BLUE };
    panel(p, r, 14.0, accent);
    let c = pos2(r.left() + 36.0, r.top() + 38.0);
    orb(p, c, 17.0, BLUE, 0.0);
    icon(p, ic, c, 8.5, shade(BLUE, 0.12));
    p.text(pos2(r.left() + 68.0, r.top() + 28.0), Align2::LEFT_CENTER, name, FontId::proportional(17.0), Color32::WHITE);
    p.text(pos2(r.left() + 68.0, r.top() + 50.0), Align2::LEFT_CENTER, format!("{} connects  ·  {} data packets sent, {} received", d.connects, d.tx, d.rx), FontId::proportional(12.0), DIM);
    lcd(p, pos2(r.right() - 20.0, r.top() + 30.0), Align2::RIGHT_CENTER, &d.drops.to_string(), 30.0, if radio { RED } else if d.drops > 0 { AMBER } else { GREEN });
    p.text(pos2(r.right() - 20.0, r.top() + 54.0), Align2::RIGHT_CENTER, "DROPS", FontId::proportional(11.0), DIM);
    let mut y = r.top() + 78.0;
    if d.reasons.is_empty() {
        p.text(pos2(r.left() + 20.0, y + 8.0), Align2::LEFT_CENTER, "no drops", FontId::proportional(13.5), GREEN);
        y += 22.0;
    }
    for (code, n) in &d.reasons {
        let (nm, _, rad) = crate::hci::reason(*code);
        let col = if rad { RED } else { AMBER };
        dot(p, pos2(r.left() + 26.0, y + 8.0), 4.5, col, 0.0);
        p.text(pos2(r.left() + 40.0, y + 8.0), Align2::LEFT_CENTER, format!("{n} \u{00d7} {nm}"), FontId::proportional(13.5), col);
        p.text(pos2(r.right() - 20.0, y + 8.0), Align2::RIGHT_CENTER, format!("0x{code:02x}"), FontId::monospace(12.0), DIM);
        y += 22.0;
    }
    y += 10.0;
    let fmt = |o: Option<f64>| o.map_or("n/a".to_string(), dur);
    for (l, v) in [
        ("Typical time between drops", fmt(d.median_gap)),
        ("Link lived: typical / shortest", format!("{} / {}", fmt(d.median_life), fmt(d.shortest_life))),
        ("No data before a drop (typical)", fmt(d.median_quiet)),
    ] {
        p.text(pos2(r.left() + 20.0, y + 8.0), Align2::LEFT_CENTER, l, FontId::proportional(12.5), DIM);
        p.text(pos2(r.right() - 20.0, y + 8.0), Align2::RIGHT_CENTER, v, FontId::proportional(13.5), TEXT);
        y += 22.0;
    }
    if d.exits > 0 {
        y += 8.0;
        p.text(pos2(r.left() + 20.0, y + 6.0), Align2::LEFT_CENTER, "SNIFF-EXIT REQUESTS ANSWERED LATE OR NOT AT ALL", FontId::proportional(10.5), DIM);
        led_bar(p, Rect::from_min_size(pos2(r.left() + 20.0, y + 18.0), vec2(r.width() - 40.0, 10.0)), d.slow_exits as f32 / d.exits as f32, if d.slow_exits > 0 { AMBER } else { GREEN }, 28);
        p.text(pos2(r.left() + 20.0, y + 42.0), Align2::LEFT_CENTER, format!("{} of {} requests  ·  {} of {} drops came within 30 s after one", d.slow_exits, d.exits, d.after_stall, d.drops), FontId::proportional(12.0), TEXT);
    }
}

/// A small status lamp with a label, optionally pulsing.
fn lamp(ui: &mut egui::Ui, col: Color32, label: &str, pulse: bool) {
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), col);
    let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 22.0, 22.0), Sense::hover());
    let t = if pulse { 0.5 + 0.5 * (ui.input(|i| i.time) as f32 * 4.0).sin() } else { 1.0 };
    let p = ui.painter();
    dot(p, pos2(r.left() + 7.0, r.center().y), 4.5, col, 0.4 * t);
    p.galley(pos2(r.left() + 20.0, r.center().y - g.size().y / 2.0), g, col);
}

fn hms(secs: u64) -> String {
    if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) } else { format!("{}:{:02}", secs / 60, secs % 60) }
}

fn size_str(b: u64) -> String {
    if b < 1 << 20 { format!("{:.0} KB", b as f64 / 1024.0) } else if b < 1 << 30 { format!("{:.1} MB", b as f64 / (1 << 20) as f64) } else { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) }
}
