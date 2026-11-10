//! The "System log" tab: Bluetooth problems from the system journal for a chosen time range, no recording needed.

use super::*;
use crate::syslog::{self, Entry, Sev, Who};
use std::sync::{Arc, Mutex};

/// What the range dropdown offers: label, and how it picks the window.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    SinceRecording,
    ThisBoot,
    Hours(i64),
    Custom,
}

const PICKS: [(&str, Pick); 8] = [
    ("Since last recording", Pick::SinceRecording),
    ("This boot", Pick::ThisBoot),
    ("Last hour", Pick::Hours(1)),
    ("Last 6 hours", Pick::Hours(6)),
    ("Last 24 hours", Pick::Hours(24)),
    ("Last 3 days", Pick::Hours(72)),
    ("Last 7 days", Pick::Hours(168)),
    ("Custom range", Pick::Custom),
];

/// What a result was fetched for: the dropdown choice and, for a custom range, its start and end.
type Key = (usize, Option<i64>, Option<i64>);
type Job = Arc<Mutex<Option<Result<(Vec<Entry>, Option<f64>), String>>>>;

#[derive(Default)]
pub struct LogState {
    choice: Option<usize>,
    start: Option<i64>,
    end: Option<i64>,
    entries: Vec<Entry>,
    /// When the journal's oldest Bluetooth entry was logged.
    oldest: Option<f64>,
    err: Option<String>,
    shown: Option<Key>,
    window: Option<(i64, i64)>,
    job: Option<(Key, Job)>,
    reload: bool,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// "Nov 9, 2026  18:42" for reports.
fn short_full(unix: i64) -> String {
    format!("{}  {}", snoop::date(unix as f64), &snoop::clock(unix as f64)[..5])
}

/// "Nov 9, 18:42" (this year's dates need no year).
fn short(unix: i64) -> String {
    let t = unix as f64;
    format!("{}, {}", snoop::date(t).split(',').next().unwrap_or(""), &snoop::clock(t)[..5])
}

impl Diag {
    /// When the newest finished recording stopped writing (unix seconds).
    fn last_recording_end(&self) -> Option<i64> {
        self.sessions
            .iter()
            .find(|s| !self.rec.as_ref().map_or(false, |r| r.file == s.capture))
            .and_then(|s| std::fs::metadata(&s.capture).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    }

    /// The time range the current choice stands for; None while a custom range has no start.
    fn log_window(&self, pick: Pick, now: i64) -> Option<(i64, i64)> {
        match pick {
            Pick::SinceRecording => self.last_recording_end().map(|e| (e, now)),
            Pick::ThisBoot => syslog::boot_time().map(|b| (b, now)),
            Pick::Hours(h) => Some((now - h * 3600, now)),
            Pick::Custom => self.log.start.map(|s| (s, self.log.end.unwrap_or(now))),
        }
    }

    pub(super) fn log_view(&mut self, ui: &mut egui::Ui, names: &Names) {
        let now = now_unix();
        let have_rec = self.last_recording_end().is_some();
        if std::env::var("AIRSCOPE_LOGPICK").is_ok() && self.log.choice.is_none() {
            // debug aid for screenshots: the custom range with the start calendar open
            self.log.choice = Some(7);
            self.log.start = Some(now - 2 * 86400);
            let id = egui::Id::new("log_start").with("open");
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }
        let ci = *self.log.choice.get_or_insert(if have_rec { 0 } else { 4 });
        let pick = PICKS[ci].1;
        let bad_range = matches!(pick, Pick::Custom) && self.log.start.map_or(false, |s| s >= self.log.end.unwrap_or(now));

        // finished read?
        if let Some((key, job)) = &self.log.job {
            let done = job.lock().ok().and_then(|mut g| g.take());
            if let Some(res) = done {
                let key = *key;
                self.log.job = None;
                match res {
                    Ok((v, oldest)) => {
                        self.log.entries = v;
                        self.log.oldest = oldest;
                        self.log.err = None;
                    }
                    Err(e) => {
                        self.log.entries.clear();
                        self.log.err = Some(e);
                    }
                }
                self.log.shown = Some(key);
            }
        }
        // start a read when the choice changed or Reload was pressed
        let want: Key = (ci, self.log.start, self.log.end);
        let window = self.log_window(pick, now);
        if self.log.job.is_none() && (self.log.shown != Some(want) || self.log.reload) {
            self.log.reload = false;
            if let (Some((s, u)), false) = (window, bad_range) {
                let job: Job = Arc::new(Mutex::new(None));
                let out = job.clone();
                std::thread::spawn(move || {
                    let r = syslog::read(s, u).map(|v| (v, syslog::oldest_entry()));
                    if let Ok(mut g) = out.lock() {
                        *g = Some(r);
                    }
                });
                self.log.window = Some((s, u));
                self.log.job = Some((want, job));
            } else {
                self.log.shown = Some(want); // nothing to read yet (custom range without a start)
                self.log.entries.clear();
                self.log.window = None;
            }
        }

        // header: range, custom fields, reload, the span in words, and a Record shortcut on the right
        let mut chosen = ci;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.spacing_mut().interact_size.y = 36.0; // the dropdown matches the buttons beside it
            let label = if ci == 0 { self.last_recording_end().map_or("Since last recording".to_string(), |e| format!("Since last recording · {}", short(e))) } else if matches!(pick, Pick::Custom) { "Custom".to_string() } else { PICKS[ci].0.to_string() };
            // the dropdown is only as wide as its label (just "Custom" when the date fields follow)
            let font = egui::TextStyle::Button.resolve(ui.style());
            let lw = ui.painter().layout_no_wrap(label.clone(), font, Color32::WHITE).size().x;
            egui::ComboBox::from_id_salt("log_range").selected_text(label).width(if matches!(pick, Pick::Custom) { lw + 44.0 } else { 260.0 }).show_ui(ui, |ui| {
                for (i, (l, p)) in PICKS.iter().enumerate() {
                    let off = matches!(p, Pick::SinceRecording) && !have_rec;
                    ui.add_enabled_ui(!off, |ui| ui.selectable_value(&mut chosen, i, if off { format!("{l} (no recordings yet)") } else { l.to_string() }));
                }
            });
            if matches!(pick, Pick::Custom) {
                crate::dtpick::field(ui, egui::Id::new("log_start"), &mut self.log.start, "Start (required)", bad_range, now);
                ui.label(RichText::new("to").color(DIM));
                crate::dtpick::field(ui, egui::Id::new("log_end"), &mut self.log.end, "Now", bad_range, now);
                if toggle_chip(ui, "Now", self.log.end.is_none(), CYAN, "End at the present moment") {
                    self.log.end = None;
                }
            }
            if action_button(ui, Some(Glyph::Refresh), "", BLUE, 0.0, self.log.job.is_none(), 0.0).hint("Read the log again").clicked() {
                self.log.reload = true;
            }
            if let (Some((s, u)), false) = (self.log.window, matches!(pick, Pick::Custom)) {
                let end = if matches!(pick, Pick::Custom) && self.log.end.is_some() { short(u) } else { "now".into() };
                ui.label(RichText::new(format!("{} to {}", short(s), end)).size(13.0).color(DIM));
            }
            if self.log.job.is_some() {
                lamp(ui, CYAN, "READING", true);
            }
        });
        if chosen != ci {
            self.log.choice = Some(chosen);
        }
        ui.add_space(12.0);
        if let Some(e) = &self.log.err {
            ui.label(RichText::new(e).color(RED));
            return;
        }
        let w = ui.available_width();
        let Some((ws, we)) = self.log.window.filter(|_| !bad_range) else {
            self.log_placeholder(ui, w, "PICK A START TIME", CYAN);
            return;
        };

        // tiles
        let count = |sev: Sev| self.log.entries.iter().filter(|e| e.sev == sev).count();
        let problems = count(Sev::Problem) + count(Sev::Symptom);
        let burst_n = syslog::bursts(&self.log.entries, 120.0);
        let last = self.log.entries.iter().rev().find(|e| e.sev != Sev::Info).map(|e| e.unix as i64);
        let multi_day = we - ws > 20 * 3600;
        let gap = 12.0;
        let tw = (w - gap * 2.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            tile(ui, tw, "PROBLEMS LOGGED", &problems.to_string(), if problems > 0 { RED } else { GREEN });
            tile(ui, tw, "BURSTS", &burst_n.to_string(), if burst_n > 0 { AMBER } else { GREEN });
            tile(ui, tw, "LAST PROBLEM", &last.map_or("-".to_string(), |u| if multi_day { short(u) } else { snoop::clock(u as f64)[..5].to_string() }), if last.is_some() { AMBER } else { DIM });
        });
        let who = |w: &Who| self.log_who(w, names);
        ui.add_space(14.0);
        // when the problems happened across the range (the same strip as for a capture), then how far back the log goes
        let spec = StripSpec { first: ws as f64, total: (we - ws) as f64, title: "PROBLEMS OVER TIME", legend: ("symptom", "link problem"), what: "problems" };
        let ticks: Vec<Tick> = self.log.entries.iter().filter(|e| e.sev != Sev::Info).map(|e| Tick { t: e.unix - ws as f64, unix: e.unix, radio: e.sev == Sev::Problem, who: who(&e.who), detail: Some(e.text.clone()) }).collect();
        let spans: Vec<(f64, f64, usize)> = syslog::burst_spans(&self.log.entries, 120.0).into_iter().map(|(a, b, n)| (a - ws as f64, b - ws as f64, n)).collect();
        super::strip(ui, w, &spec, &ticks, &spans, &self.tip);
        ui.add_space(12.0);
        if let Some(o) = self.log.oldest {
            // dim normally; amber when the log doesn't reach back to the start of the chosen range
            let short_log = o > ws as f64 + 60.0;
            let (lr, lresp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
            ui.painter().text(pos2(lr.left() + 16.0, lr.center().y), Align2::LEFT_CENTER, format!("Oldest log entry: {}  {}", snoop::date(o), snoop::clock(o)), FontId::proportional(13.0), if short_log { AMBER } else { DIM });
            if short_log {
                lresp.hint("The log does not go back to the start of this range, so earlier problems can't be shown.");
            }
        }
        ui.add_space(4.0);

        let shown: Vec<usize> = (0..self.log.entries.len()).rev().filter(|&i| !self.problems_only || self.log.entries[i].sev != Sev::Info).collect();
        if shown.is_empty() {
            if self.log.job.is_none() {
                self.log_placeholder(ui, w, if self.log.entries.is_empty() { "NO BLUETOOTH PROBLEMS LOGGED" } else { "ONLY ROUTINE EVENTS LOGGED" }, GREEN);
            } else {
                self.log_placeholder(ui, w, "READING THE LOG", CYAN);
            }
            return;
        }
        // the table (same look as the capture Timeline)
        let (x_time, x_dev, x_msg) = (16.0, 160.0, 400.0);
        let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
        let hf = FontId::proportional(11.5);
        for (x, t) in [(x_time, "TIME"), (x_dev, "DEVICE"), (x_msg, "EVENT")] {
            ui.painter().text(pos2(hr.left() + x, hr.center().y), Align2::LEFT_CENTER, t, hf.clone(), DIM);
        }
        ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.25)));
        let row_h = 34.0;
        let entries = &self.log.entries;
        super::scroll_in_margin(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, vp| {
                let (rect, _) = ui.allocate_exact_size(vec2(w, row_h * shown.len() as f32), Sense::hover());
                let p = ui.painter().clone();
                let (a, b) = ((vp.min.y / row_h) as usize, ((vp.max.y / row_h) as usize + 1).min(shown.len()));
                for (k, &i) in shown.iter().enumerate().take(b).skip(a) {
                    let e = &entries[i];
                    let top = rect.top() + k as f32 * row_h;
                    let col = match e.sev {
                        Sev::Problem => RED,
                        Sev::Symptom => AMBER,
                        Sev::Info => DIM,
                    };
                    let row = Rect::from_min_size(pos2(rect.left(), top), vec2(w, row_h));
                    if e.sev != Sev::Info {
                        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 2.0, top + 5.0), vec2(3.0, row_h - 10.0)), 1.5, col);
                    }
                    let when = if multi_day { format!("{}  {}", short(e.unix as i64).split(',').next().unwrap_or(""), snoop::clock(e.unix)) } else { snoop::clock(e.unix) };
                    let cy = row.center().y;
                    p.text(pos2(rect.left() + x_time, cy), Align2::LEFT_CENTER, when, FontId::monospace(12.5), DIM);
                    let dev = crate::fit_middle(&p, &who(&e.who), FontId::proportional(13.5), x_msg - x_dev - 14.0);
                    p.text(pos2(rect.left() + x_dev, cy), Align2::LEFT_CENTER, dev, FontId::proportional(13.5), TEXT);
                    let msg = crate::fit_middle(&p, &e.text, FontId::proportional(13.5), (w - x_msg - 14.0).max(120.0));
                    p.text(pos2(rect.left() + x_msg, cy), Align2::LEFT_CENTER, msg, FontId::proportional(13.5), col);
                    p.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, alpha(Color32::WHITE, 0.07)));
                    let rid = ui.id().with(("log_row", i));
                    if ui.interact(row.intersect(ui.clip_rect()), rid, Sense::hover()).hovered() {
                        let mut sub = vec![format!("Device: {}", who(&e.who)), format!("Time: {}  {}", snoop::date(e.unix), snoop::clock(e.unix))];
                        sub.push(if e.origin.is_empty() { format!("From: {}", e.source) } else { format!("From: {} ({})", e.source, e.origin) });
                        sub.push(format!("Logged as: {}", e.detail));
                        let anchor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(row.center());
                        let tc = crate::TipC { key: Some(rid), anchor, col, title: e.text.clone(), sub, hint: None, side: None };
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("hint_want"), Some(tc)));
                    }
                }
            });
        });
    }

    fn log_who(&self, w: &Who, names: &Names) -> String {
        match w {
            Who::Mac(m) => {
                let name = names.get(m).map(|n| n.0.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Device ..{}", &m[12..]));
                if self.addrs { format!("{name} ({m})") } else { name }
            }
            Who::Name(n) => n.clone(),
            Who::None => "(not stated)".into(),
        }
    }

    /// The shown range and its events as Markdown text (for Copy and Save).
    pub(super) fn log_report(&self, names: &Names) -> String {
        let mut r = String::from("# Bluetooth system log report\n\n");
        if let Some((s, u)) = self.log.window {
            r += &format!("Range: {} to {}\n", short_full(s), short_full(u));
        }
        if let Some(o) = self.log.oldest {
            r += &format!("Oldest log entry: {}  {}\n", snoop::date(o), snoop::clock(o));
        }
        let n = |sev: Sev| self.log.entries.iter().filter(|e| e.sev == sev).count();
        r += &format!("\nLink problems: {}  \u{00b7}  Symptoms: {}  \u{00b7}  Bursts: {}\n\n## Events\n\n```text\n", n(Sev::Problem), n(Sev::Symptom), syslog::bursts(&self.log.entries, 120.0));
        for e in self.log.entries.iter().filter(|e| !self.problems_only || e.sev != Sev::Info) {
            r += &format!("{} {}  {}  {}\n", snoop::date(e.unix), snoop::clock(e.unix), self.log_who(&e.who, names), e.text);
        }
        r += "```\n\nReporting only: this lists what BlueZ and the kernel logged; it does not say what to change.\n";
        if !self.addrs {
            r += "Device addresses are left out.\n";
        }
        r
    }

    /// Suggested file name for the log report, from its range.
    pub(super) fn log_report_name(&self) -> String {
        let stamp = |x: i64| format!("{}_{}", snoop::date_iso(x as f64), snoop::clock(x as f64)[..5].replace(':', ""));
        match self.log.window {
            Some((s, u)) => format!("system-log-{}-{}.md", stamp(s), stamp(u)),
            None => "system-log.md".into(),
        }
    }

    /// A centred title over ghost table rows: nothing to show (yet), and why.
    fn log_placeholder(&self, ui: &mut egui::Ui, w: f32, title: &str, col: Color32) {
        let (tr, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
        lcd(ui.painter(), tr.center(), Align2::CENTER_CENTER, title, 13.0, col);
        ui.add_space(8.0);
        let (x_time, x_dev, x_msg) = (16.0, 160.0, 400.0);
        let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
        let hf = FontId::proportional(11.5);
        for (x, t) in [(x_time, "TIME"), (x_dev, "DEVICE"), (x_msg, "EVENT")] {
            ui.painter().text(pos2(hr.left() + x, hr.center().y), Align2::LEFT_CENTER, t, hf.clone(), alpha(DIM, 0.7));
        }
        ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.15)));
        let bar = alpha(Color32::WHITE, 0.06);
        for k in 0..8 {
            let (rr, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
            let p = ui.painter();
            let cy = rr.center().y;
            p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_time, cy - 5.0), vec2(70.0, 10.0)), 5.0, bar);
            p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_dev, cy - 5.0), vec2([120.0, 90.0, 140.0][k % 3], 10.0)), 5.0, bar);
            p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_msg, cy - 5.0), vec2((w - x_msg - 14.0).max(160.0) * [0.7, 0.45, 0.6, 0.35][k % 4], 10.0)), 5.0, bar);
            p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, alpha(Color32::WHITE, 0.05)));
        }
    }
}
