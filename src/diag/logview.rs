//! The system log source of the Diagnostics page: Bluetooth problems from the system journal for a chosen time range, no recording needed.

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
    pub(super) entries: Vec<Entry>,
    /// Counts up whenever `entries` is replaced.
    pub(super) ver: u64,
    /// When the journal's oldest Bluetooth entry was logged.
    pub(super) oldest: Option<f64>,
    pub(super) err: Option<String>,
    shown: Option<Key>,
    pub(super) window: Option<(i64, i64)>,
    pub(super) job: Option<(Key, Job)>,
    reload: bool,
    /// The next read is a background refresh of a live range (no READING lamp).
    poll: bool,
    quiet: bool,
    last_read: Option<std::time::Instant>,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// "Nov 9, 2026  18:42" for reports.
fn short_full(unix: i64) -> String {
    format!("{}  {}", snoop::date(unix as f64), &snoop::clock(unix as f64)[..5])
}

/// "Nov 9, 18:42" (this year's dates need no year).
pub(super) fn short(unix: i64) -> String {
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

    /// Reads the log when the range changed and draws the range controls. Returns the range to show (None: nothing yet).
    pub(super) fn log_controls(&mut self, ui: &mut egui::Ui) -> Option<(i64, i64)> {
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
                        self.log.ver += 1;
                        self.log_px.clear();
                        self.log.oldest = oldest;
                        self.log.err = None;
                    }
                    Err(e) => {
                        self.log.entries.clear();
                        self.log.ver += 1;
                        self.log_px.clear();
                        self.log.err = Some(e);
                    }
                }
                self.log.shown = Some(key);
                self.log.last_read = Some(std::time::Instant::now());
            }
        }
        // start a read when the choice changed or Reload was pressed
        let want: Key = (ci, self.log.start, self.log.end);
        let window = self.log_window(pick, now);
        // a range that ends at "Now" follows the journal: read again every 10 s
        let live = !bad_range && window.is_some() && (!matches!(pick, Pick::Custom) || self.log.end.is_none());
        if live {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            if self.log.job.is_none() && self.log.shown == Some(want) && self.log.last_read.map_or(false, |t| t.elapsed().as_secs() >= 10) {
                self.log.reload = true;
                self.log.poll = true;
            }
        }
        if self.log.job.is_none() && (self.log.shown != Some(want) || self.log.reload) {
            self.log.reload = false;
            self.log.quiet = std::mem::take(&mut self.log.poll);
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
                        self.log.ver += 1;
                        self.log_px.clear();
                self.log.window = None;
            }
        }

        // header: "System log" and the span in words on the left; range, custom fields and reload on the right
        let mut chosen = ci;
        let span = match (self.log.window, pick) {
            (Some((s, u)), _) => format!("{} to {}", short(s), if live { "now".to_string() } else { short(u) }),
            _ => "pick a start time".to_string(),
        };
        let reading = self.log.job.is_some() && !self.log.quiet;
        let h = super::page::CTL_H;
        let mut reload = false;
        Diag::header(ui, "System log", |ui| {
            ui.label(RichText::new(span).size(15.0).color(TEXT));
            if live {
                lamp(ui, GREEN, "LIVE", true);
            } else if reading {
                lamp(ui, CYAN, "READING", true);
            }
        }, |ui| {
            // right to left: reload, then the custom fields, then the dropdown
            if action_button_h(ui, Some(Glyph::Refresh), "", BLUE, 0.0, self.log.job.is_none(), 0.0, h).hint("Read the log again").clicked() {
                reload = true;
            }
            if matches!(pick, Pick::Custom) {
                if toggle_chip(ui, "Now", self.log.end.is_none(), CYAN, "End at the present moment") {
                    self.log.end = None;
                }
                crate::dtpick::field_h(ui, egui::Id::new("log_end"), &mut self.log.end, "Now", bad_range, now, h);
                ui.label(RichText::new("to").color(DIM));
                crate::dtpick::field_h(ui, egui::Id::new("log_start"), &mut self.log.start, "Start (required)", bad_range, now, h);
            }
            let label = if ci == 0 { "Since last recording".to_string() } else if matches!(pick, Pick::Custom) { "Custom".to_string() } else { PICKS[ci].0.to_string() };
            let font = egui::TextStyle::Button.resolve(ui.style());
            let lw = ui.painter().layout_no_wrap(label.clone(), font, Color32::WHITE).size().x;
            egui::ComboBox::from_id_salt("log_range").selected_text(label).width(if matches!(pick, Pick::Custom) { lw + 44.0 } else { lw + 44.0 }).show_ui(ui, |ui| {
                for (i, (l, p)) in PICKS.iter().enumerate() {
                    let off = matches!(p, Pick::SinceRecording) && !have_rec;
                    ui.add_enabled_ui(!off, |ui| ui.selectable_value(&mut chosen, i, if off { format!("{l} (no recordings yet)") } else { l.to_string() }));
                }
            });
        });
        if reload {
            self.log.reload = true;
        }
        if chosen != ci {
            self.log.choice = Some(chosen);
        }
        if self.log.err.is_some() {
            return None;
        }
        self.log.window.filter(|_| !bad_range)
    }


    pub(super) fn log_who(&self, w: &Who, names: &Names) -> String {
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

}
