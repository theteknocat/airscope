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
    tl_key: (usize, bool, u64),
    /// Timeline layout: measured width of each event's message at the table font (grows as events are measured,
    /// a few thousand per frame) and the line height.
    ev_px: Vec<f32>,
    line_h: f32,
    checked: Instant,
    tab: u8,
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
    /// A new recording to open as soon as btmon has written its header.
    follow: Option<PathBuf>,
    /// When new data last arrived from the file (drives the LIVE lamp).
    grew: Option<Instant>,
    /// A recording just ended: save its report and summary once the last bytes are read.
    finalize: bool,
    summarised: bool,
    sessions: Vec<capture::Session>,
    sessions_at: Option<Instant>,
    /// Library capture being analysed on a background thread, and whether it has finished.
    bg: Option<(PathBuf, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

impl Diag {
    pub fn new() -> Self {
        let mut d = Diag { path: std::env::var("AIRSCOPE_SNOOP").unwrap_or_default(), snoop: None, err: None, pos: 0, total: 0, stats: vec![], bursts: vec![], key: u64::MAX, tl: vec![], tl_key: (0, false, u64::MAX), ev_px: vec![], line_h: 0.0, checked: Instant::now(), tab: std::env::var("AIRSCOPE_TAB").ok().and_then(|v| v.parse().ok()).unwrap_or(0), addrs: false, problems_only: true, flash: None, pick: None, rec: None, cap_idx: 1, rec_msg: None, follow: None, grew: None, finalize: false, summarised: false, sessions: vec![], sessions_at: None, bg: None, tip: Default::default() };
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

    fn save(&mut self, s: &Snoop, names: &Names) {
        // a capture in the library keeps its report next to it; any other capture's report goes to the shared reports folder
        let file = match capture::session_dir_of(Path::new(&self.clean_path())) {
            Some(dir) => dir.join(capture::REPORT_FILE),
            None => {
                let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share")));
                let Some(base) = base else { return };
                let first = Self::first_unix(s);
                let stem = self.shown_name().trim_end_matches(".snoop").to_string();
                base.join("airscope/reports").join(format!("{stem}-{}.md", snoop::date_iso(first)))
            }
        };
        let res = match file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&file, self.report(s, names))) {
            Ok(()) => {
                self.sessions_at = None;
                Ok(format!("Saved to {}", file.display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1)))
            }
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

    /// Visual summary: headline tiles, a strip showing when the drops happened, then one card per device.
    fn summary_view(&self, ui: &mut egui::Ui, s: &Snoop, names: &Names) {
        let w = ui.available_width() - 6.0;
        let gap = 12.0;
        let drops = s.drops().count();
        let bursts = &self.bursts;
        let tw = (w - gap * 3.0) / 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            tile(ui, tw, "LINK DROPS", &drops.to_string(), if drops > 0 { RED } else { GREEN });
            tile(ui, tw, "CAPTURED", &dur(s.duration()), CYAN);
            tile(ui, tw, "DATA PACKETS", &s.acl.to_string(), BLUE);
            tile(ui, tw, "DROP BURSTS", &bursts.len().to_string(), if bursts.is_empty() { GREEN } else { AMBER });
        });
        ui.add_space(14.0);
        strip(ui, w, s, bursts, &|a| self.who(a, names), &self.tip);
        ui.add_space(14.0);
        let stats = &self.stats;
        let cols = if w >= 860.0 { 2 } else { 1 };
        let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
        let ch = stats.iter().map(card_h).fold(0.0, f32::max);
        for row in stats.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for d in row {
                    let name = self.who(&d.addr, names);
                    let ic = names.get(&mac(&d.addr)).map_or(Icon::Bluetooth, |n| n.1);
                    card(ui, vec2(cw, ch), d, &name, ic);
                }
            });
            ui.add_space(gap);
        }
    }

    /// The timeline as a table: Time, Since start, Device, Event. The event text wraps inside its own column. Rows have
    /// different heights, so each row's height is estimated from its text length (generously, so rows never overlap)
    /// and only the rows in view are drawn, which keeps a capture with a very long history fast.
    fn timeline(&mut self, ui: &mut egui::Ui, s: &Snoop, names: &Names) {
        let font = FontId::proportional(13.5);
        if self.line_h == 0.0 {
            self.line_h = ui.painter().layout_no_wrap("Ag".to_string(), font.clone(), Color32::WHITE).size().y;
        }
        for _ in 0..3000 {
            let Some(e) = s.events.get(self.ev_px.len()) else { break };
            let px = ui.painter().layout_no_wrap(snoop::message(e), font.clone(), Color32::WHITE).size().x;
            self.ev_px.push(px);
        }
        let w = ui.available_width() - 8.0;
        let (x_time, x_off, x_dev, x_msg) = (16.0, 196.0, 212.0, 436.0);
        let dev_w = x_msg - x_dev - 14.0;
        let msg_w = (w - x_msg - 14.0).max(160.0);
        // header
        let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
        let p = ui.painter();
        let hf = FontId::proportional(11.5);
        p.text(pos2(hr.left() + x_time, hr.center().y), Align2::LEFT_CENTER, "TIME", hf.clone(), DIM);
        p.text(pos2(hr.left() + x_off, hr.center().y), Align2::RIGHT_CENTER, "SINCE START", hf.clone(), DIM);
        p.text(pos2(hr.left() + x_dev, hr.center().y), Align2::LEFT_CENTER, "DEVICE", hf.clone(), DIM);
        p.text(pos2(hr.left() + x_msg, hr.center().y), Align2::LEFT_CENTER, "EVENT", hf, DIM);
        p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.25)));
        // row heights: one line if the text fits its column, otherwise the wrapped line count with a little slack
        let lines = |px: f32, col: f32| if px <= col - 2.0 { 1.0 } else { (px * 1.08 / col).ceil() };
        let dev_px: Vec<(snoop::Addr, f32)> = self.stats.iter().map(|d| (d.addr, ui.painter().layout_no_wrap(self.who(&d.addr, names), font.clone(), Color32::WHITE).size().x)).collect();
        let (ev_px, line_h) = (&self.ev_px, self.line_h);
        let height = |i: usize| -> f32 {
            let dp = dev_px.iter().find(|d| d.0 == s.events[i].addr).map_or(0.0, |d| d.1);
            lines(ev_px.get(i).copied().unwrap_or(0.0), msg_w).max(lines(dp, dev_w)) * line_h + 18.0
        };
        let total: f32 = self.tl.iter().map(|&i| height(i)).sum();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, vp| {
            let (rect, _) = ui.allocate_exact_size(vec2(w, total.max(1.0)), Sense::hover());
            let p = ui.painter().clone();
            let mut y = 0.0;
            for &i in &self.tl {
                let h = height(i);
                if y > vp.max.y {
                    break;
                }
                if y + h >= vp.min.y {
                    let e = &s.events[i];
                    let (top, col) = (rect.top() + y, Self::colour(e));
                    if Self::problem(e) {
                        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 2.0, top + 5.0), vec2(3.0, h - 10.0)), 1.5, col);
                    }
                    let ty = top + 9.0;
                    let mono = FontId::monospace(12.5);
                    p.text(pos2(rect.left() + x_time, ty), Align2::LEFT_TOP, snoop::clock(e.unix), mono.clone(), DIM);
                    p.text(pos2(rect.left() + x_off, ty), Align2::RIGHT_TOP, snoop::offset(e), mono, DIM);
                    let g = p.layout(self.who(&e.addr, names), FontId::proportional(13.5), TEXT, dev_w);
                    p.galley(pos2(rect.left() + x_dev, ty), g, TEXT);
                    let g = p.layout(snoop::message(e), FontId::proportional(13.5), col, msg_w);
                    p.galley(pos2(rect.left() + x_msg, ty), g, col);
                    p.line_segment([pos2(rect.left(), top + h), pos2(rect.right(), top + h)], Stroke::new(1.0, alpha(Color32::WHITE, 0.07)));
                }
                y += h;
            }
        });
    }

    /// The saved sessions, newest first: when each was started, how long, how big, how many drops, with Open and Copy path.
    fn library_view(&mut self, ui: &mut egui::Ui) {
        if self.sessions_at.map_or(true, |t| t.elapsed() > Duration::from_secs(3)) {
            self.sessions = capture::list();
            self.sessions_at = Some(Instant::now());
        }
        // finish the analysis of captures nobody has opened (e.g. one that ended while the app was closed), one at a time
        if self.bg.as_ref().map_or(false, |(_, d)| d.load(std::sync::atomic::Ordering::Relaxed)) {
            self.bg = None;
            self.sessions_at = None;
        }
        if self.bg.is_none() {
            let open = PathBuf::from(self.clean_path());
            let live = |c: &PathBuf| self.rec.as_ref().map_or(false, |r| &r.file == c) || *c == open || std::fs::metadata(c).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map_or(true, |e| e < Duration::from_secs(10));
            if let Some(s) = self.sessions.iter().find(|s| capture::needs_analysis(s) && !live(&s.capture)) {
                let (dir, cap) = (s.dir.clone(), s.capture.clone());
                let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = done.clone();
                self.bg = Some((cap.clone(), done));
                std::thread::spawn(move || {
                    capture::analyse(&dir, &cap);
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
        }
        if self.sessions.is_empty() {
            ui.label(RichText::new("No saved captures yet. A recording made with the Record button is kept here, with its analysis.").color(DIM));
            return;
        }
        let w = ui.available_width() - 8.0;
        let mut open: Option<PathBuf> = None;
        let mut copy: Option<PathBuf> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for sess in &self.sessions {
                let (r, _) = ui.allocate_exact_size(vec2(w, 84.0), Sense::hover());
                let recording = self.rec.as_ref().map_or(false, |x| x.file == sess.capture);
                let accent = if recording { RED } else if sess.summary.as_ref().map_or(false, |x| x.drops > 0) { AMBER } else { BLUE };
                let p = ui.painter().clone();
                panel(&p, r, 12.0, accent);
                lcd(&p, pos2(r.left() + 20.0, r.top() + 28.0), Align2::LEFT_CENTER, &snoop::pretty_stamp(&sess.name), 19.0, if recording { RED } else { CYAN });
                let sub = match &sess.summary {
                    Some(x) => format!("{}  ·  {}  ·  {} drops  ·  {} records{}", dur(x.duration), size_str(sess.size), x.drops, x.records, if sess.report { "  ·  report saved" } else { "" }),
                    None if self.bg.as_ref().map_or(false, |(c, _)| *c == sess.capture) => format!("{}  ·  analysing…", size_str(sess.size)),
                    None => format!("{}  ·  not analysed yet", size_str(sess.size)),
                };
                p.text(pos2(r.left() + 20.0, r.top() + 58.0), Align2::LEFT_CENTER, if recording { "recording now".to_string() } else { sub }, FontId::proportional(13.0), DIM);
                let bar = Rect::from_min_size(pos2(r.right() - 300.0, r.top() + 24.0), vec2(280.0, 36.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if action_button(ui, Some(Glyph::Open), "Open", BLUE, 0.0, true, 0.0).clicked() {
                        open = Some(sess.capture.clone());
                    }
                    if action_button(ui, Some(Glyph::Copy), "Copy path", BLUE, 0.0, true, 0.0).clicked() {
                        copy = Some(sess.dir.clone());
                    }
                });
                ui.advance_cursor_after_rect(r); // the button area above is a child Ui that moved the cursor to its own bottom
                ui.add_space(10.0);
            }
        });
        if let Some(c) = open {
            self.path = c.display().to_string();
            self.tab = 0;
            self.load();
        }
        if let Some(dir) = copy {
            ui.ctx().copy_text(dir.display().to_string());
            self.rec_msg = Some((false, "Folder path copied to the clipboard".into()));
        }
    }

    /// Open capture's name, loading progress, LIVE lamp and the latest status message.
    fn name_row(&mut self, ui: &mut egui::Ui) {
        let name = snoop::pretty_stamp(&self.shown_name());
        let reserve = 20.0 + if self.pos < self.total { 90.0 } else { 0.0 } + if self.grew.map_or(false, |t| t.elapsed() < Duration::from_secs(6)) { 80.0 } else { 0.0 };
        if name.is_empty() {
            ui.label(RichText::new("No capture opened").color(DIM));
        } else {
            let font = egui::FontId::proportional(15.0);
            let shown = crate::fit_middle(ui.painter(), &name, font, (ui.available_width() - reserve).max(60.0));
            let r = ui.label(RichText::new(&shown).color(TEXT).size(15.0));
            if shown != name {
                r.hint(name);
            }
        }
        if self.pos < self.total {
            ui.label(RichText::new(format!("loading {}%", self.pos * 100 / self.total)).color(DIM));
        }
        if self.grew.map_or(false, |t| t.elapsed() < Duration::from_secs(6)) {
            lamp(ui, GREEN, "LIVE", true);
        }
        if let Some((err, m)) = &self.rec_msg {
            ui.add(egui::Label::new(RichText::new(m).size(12.5).color(if *err { RED } else { DIM })).truncate());
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, names: &Names) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        if let Some(p) = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).find(|s| !s.is_empty())) {
            self.path = p;
            self.load();
        }
        if let Some(res) = self.pick.as_ref().and_then(|p| p.poll()) {
            self.pick = None;
            match res {
                Ok(Some(path)) => {
                    self.path = path.display().to_string();
                    self.load();
                }
                Ok(None) => {}
                Err(e) => self.err = Some(e),
            }
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
                self.path = f.display().to_string();
                self.tab = 0;
                self.load();
            }
        }
        self.pump();
        let s = self.snoop.take();
        if let Some(s) = &s {
            if self.key != s.records {
                self.stats = s.per_device();
                self.bursts = s.bursts(120.0);
                self.key = s.records;
            }
            let k = (s.events.len(), self.problems_only, s.records);
            if self.tl_key != k {
                self.tl = (0..s.events.len()).rev().filter(|&i| Self::problem(&s.events[i]) || !self.problems_only).collect();
                self.tl_key = k;
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
            // row 1: actions on the file (glass buttons)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                // the left group gets what the Copy/Save buttons leave, and its texts are cut short instead of running into them
                // Narrow window: the buttons shrink to icons (tooltips keep the names); the file name is cut in the middle if it still does not fit
                let avail = ui.available_width();
                let stacked = s.is_some() && avail < 930.0;
                let left_w = (avail - if s.is_none() { 0.0 } else if stacked { 100.0 } else { 330.0 }).max(200.0);
                ui.allocate_ui_with_layout(vec2(left_w, 34.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
                    if action_button(ui, Some(Glyph::Record), if stacked { "" } else { "Record" }, RED, 0.0, true, 0.0).hint("Record Bluetooth activity (asks for your password)").clicked() {
                        match Recording::start(Duration::from_secs(CAPS[self.cap_idx].1)) {
                            Ok(r) => {
                                self.follow = Some(r.file.clone());
                                self.rec = Some(r);
                                self.rec_msg = Some((false, "Waiting for your password...".into()));
                            }
                            Err(e) => self.rec_msg = Some((true, e)),
                        }
                    }
                    ui.label(RichText::new("for").color(DIM));
                    egui::ComboBox::from_id_salt("cap_len").selected_text(CAPS[self.cap_idx].0).show_ui(ui, |ui| {
                        for (i, (l, _)) in CAPS.iter().enumerate() {
                            ui.selectable_value(&mut self.cap_idx, i, *l);
                        }
                    });
                }
                ui.separator();
                let picking = self.pick.is_some();
                if action_button(ui, Some(Glyph::Open), if stacked { "" } else if picking { "Choosing..." } else { "Open" }, BLUE, 0.0, !picking, 0.0).hint("Open a capture file").clicked() {
                    self.pick = Some(Pick::start("Open a Bluetooth capture file"));
                }
                self.name_row(ui);
                });
                if let Some(s) = &s {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        // right to left: Save is added first, so it sits on the right
                        for (idx, normal, done) in [(1u8, "Save report", "Saved"), (0u8, "Copy report", "Copied")] {
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
                            let tip = f.map(|f| match &f.2 {
                                Ok(t) | Err(t) => t.clone(),
                            });
                            // the icon turns into a tick (or a cross) while the result shows
                            let ic = if label == normal { if idx == 1 { Glyph::Save } else { Glyph::Copy } } else if tint == RED { Glyph::Cross } else { Glyph::Check };
                            let r = action_button(ui, Some(ic), if stacked { "" } else { label }, mix(BLUE, tint, t), t, true, if stacked { 0.0 } else { 150.0 });
                            let r = match tip {
                                Some(t) => r.hint(t),
                                None if stacked => r.hint(normal),
                                None => r,
                            };
                            if r.clicked() {
                                if idx == 1 {
                                    self.save(s, names);
                                } else {
                                    ui.ctx().copy_text(self.report(s, names));
                                    self.flash = Some((Instant::now(), 0, Ok("Report copied to the clipboard".into())));
                                }
                            }
                        }
                    });
                }
            });
            ui.add_space(10.0);
            // row 2: which view (tabs, left) and what to show in it (toggle chips, right)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (i, l) in ["Summary", "Timeline", "Library"].iter().enumerate() {
                    if view_tab(ui, l, self.tab == i as u8) {
                        self.tab = i as u8;
                    }
                }
                if self.tab != 2 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if self.tab > 0 && toggle_chip(ui, "Problems only", self.problems_only, AMBER, if self.problems_only { "Showing only drops and slow answers. Click to show every event." } else { "Showing every event. Click to show only drops and slow answers." }) {
                            self.problems_only = !self.problems_only;
                        }
                        if toggle_chip(ui, "Device addresses", self.addrs, CYAN, if self.addrs { "Showing full device addresses. Click to hide them." } else { "Addresses are hidden. Click to show the full device addresses." }) {
                            self.addrs = !self.addrs;
                        }
                    });
                }
            });
            ui.separator();
            if let Some(e) = &self.err {
                ui.label(RichText::new(e).color(RED));
            }
            if self.tab == 2 {
                self.library_view(ui);
                return;
            }
            let Some(s) = &s else {
                if self.err.is_none() {
                    ui.label(RichText::new("Press Record to capture Bluetooth activity (you will be asked for your password), or Open (or drop a capture file on this window) to see what happened to each Bluetooth device: when the link dropped and why, and whether the controller was slow to answer.").color(DIM));
                    ui.label(RichText::new("Recordings are kept in the Library tab with their analysis. A capture made by hand (sudo btmon -w file) can be opened too, even while it is still running.").color(DIM));
                }
                return;
            };
            if self.tab == 0 {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.summary_view(ui, s, names));
            } else if self.tl.is_empty() {
                ui.label(RichText::new(if self.problems_only { "No problems in this capture." } else { "No events in this capture." }).color(DIM));
            } else {
                self.timeline(ui, s, names);
            }
        });
        // the Library's Open button calls load() while `s` is taken out: keep the freshly loaded capture, not the old one
        if self.snoop.is_none() && self.err.is_none() {
            self.snoop = s;
        }
    }
}

fn tile(ui: &mut egui::Ui, w: f32, label: &str, value: &str, col: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 78.0), Sense::hover());
    let p = ui.painter();
    panel(p, r, 12.0, col);
    p.text(pos2(r.left() + 18.0, r.top() + 20.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), DIM);
    lcd(p, pos2(r.left() + 18.0, r.top() + 49.0), Align2::LEFT_CENTER, value, 26.0, col);
}

/// When the drops happened across the capture: one tick per drop (red = radio-type reason, amber = other), bursts shaded.
fn strip(ui: &mut egui::Ui, w: f32, s: &Snoop, bursts: &[(f64, f64, usize)], who: &dyn Fn(&snoop::Addr) -> String, tip: &std::cell::RefCell<Option<crate::TipC>>) {
    let first = Diag::first_unix(s);
    let total = s.duration().max(1.0);
    let crosses = snoop::date(first) != snoop::date(first + total); // spans midnight: dates are shown under some times
    let extra = if crosses { 14.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 96.0 + extra), Sense::hover());
    let p = ui.painter();
    panel(p, r, 12.0, BLUE);
    p.text(pos2(r.left() + 18.0, r.top() + 18.0), Align2::LEFT_CENTER, "DROPS OVER TIME", FontId::proportional(12.0), DIM);
    // legend: a coloured dot and a short label, like the sidebar legend (right to left)
    let mut lx = r.right() - 18.0;
    for (col, label) in [(AMBER, "other"), (RED, "radio-type drop")] {
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
    let radio = |e: &snoop::Ev| matches!(e.kind, Kind::Disconnected { reason, .. } if crate::hci::reason(reason).2);
    let mut groups: Vec<(f32, f32, Vec<&snoop::Ev>)> = vec![];
    for e in s.drops() {
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
        let full = |e: &snoop::Ev| format!("{} {}", snoop::date(e.unix), snoop::clock(e.unix));
        let (title, lines) = if g.2.len() == 1 {
            (who(&g.2[0].addr), vec![full(g.2[0])])
        } else {
            let day = snoop::date(g.2[0].unix);
            let one_day = g.2.iter().all(|e| snoop::date(e.unix) == day);
            let mut lines: Vec<String> = g.2.iter().take(10).map(|e| format!("{}  \u{00b7}  {}", if one_day { snoop::clock(e.unix) } else { full(e) }, who(&e.addr))).collect();
            if g.2.len() > 10 {
                lines.push(format!("and {} more", g.2.len() - 10));
            }
            (if one_day { format!("{} link drops \u{00b7} {day}", g.2.len()) } else { format!("{} link drops", g.2.len()) }, lines)
        };
        *tip.borrow_mut() = Some(crate::TipC { anchor: pos2(cx, a.top()), col, title, sub: lines, hint: None, side: None });
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

fn card(ui: &mut egui::Ui, size: egui::Vec2, d: &snoop::DevStat, name: &str, ic: Icon) {
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let radio = d.reasons.iter().any(|(c, _)| crate::hci::reason(*c).2);
    let accent = if radio { RED } else if d.drops > 0 { AMBER } else { BLUE };
    let p = ui.painter();
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
