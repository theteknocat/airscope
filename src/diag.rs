//! Diagnostics screen: opens a Bluetooth capture file written by `btmon -w` (finished, or still growing) and reports
//! what happened to each device: drops with their reason, sniff-exit requests the controller answered late, bursts.
//! Reporting only: it never changes anything on the system.

use crate::data::Icon;
use crate::fx::*;
use crate::picker::Pick;
use crate::snoop::{self, dur, line, mac, Ev, Kind, Snoop};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Known Bluetooth devices by upper-case address: (name, icon).
pub type Names = HashMap<String, (String, Icon)>;

pub struct Diag {
    path: String,
    snoop: Option<Snoop>,
    err: Option<String>,
    size: u64,
    checked: Instant,
    tab: u8,
    addrs: bool,
    problems_only: bool,
    /// Brief confirmation shown on a button: (when, which button: 0 copy / 1 save, outcome text).
    flash: Option<(Instant, u8, Result<String, String>)>,
    pick: Option<Pick>,
}

impl Diag {
    pub fn new() -> Self {
        let mut d = Diag { path: std::env::var("AIRSCOPE_SNOOP").unwrap_or_default(), snoop: None, err: None, size: 0, checked: Instant::now(), tab: 0, addrs: false, problems_only: true, flash: None, pick: None };
        if !d.path.is_empty() {
            d.load();
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
        let p = self.clean_path();
        match std::fs::read(&p) {
            Ok(d) => {
                let mut s = Snoop::default();
                match s.feed(&d) {
                    Ok(()) => {
                        self.size = d.len() as u64;
                        self.snoop = Some(s);
                    }
                    Err(e) => self.err = Some(e),
                }
            }
            Err(e) => self.err = Some(format!("Cannot read {p}: {e}")),
        }
        self.checked = Instant::now();
    }

    /// A capture that is still being written grows: pick up the new records every couple of seconds.
    fn refresh(&mut self) {
        if self.snoop.is_none() || self.checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.checked = Instant::now();
        let p = self.clean_path();
        let Ok(m) = std::fs::metadata(&p) else { return };
        if m.len() < self.size {
            self.load();
        } else if m.len() > self.size {
            if let (Ok(d), Some(s)) = (std::fs::read(&p), self.snoop.as_mut()) {
                self.size = d.len() as u64;
                let _ = s.feed(&d);
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
        let mut r = format!("# Bluetooth capture report\n\nFile: {}\n\n## Summary\n\n", self.clean_path().rsplit('/').next().unwrap_or(""));
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
        let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".local/share")));
        let Some(base) = base else { return };
        let dir = base.join("airscope/reports");
        let first = s.first_us.map_or(0.0, |f| (f - 0x00dc_ddb3_0f2f_8000i64) as f64 / 1e6);
        let stem = self.clean_path().rsplit('/').next().unwrap_or("capture").trim_end_matches(".snoop").to_string();
        let file = dir.join(format!("{stem}-{}.md", snoop::date(first)));
        let res = match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, self.report(s, names))) {
            Ok(()) => Ok(format!("Saved to {}", file.display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1))),
            Err(e) => Err(format!("Could not save: {e}")),
        };
        self.flash = Some((Instant::now(), 1, res));
    }

    fn first_unix(s: &Snoop) -> f64 {
        s.first_us.map_or(0.0, |f| (f - 0x00dc_ddb3_0f2f_8000i64) as f64 / 1e6)
    }

    /// Visual summary: headline tiles, a strip showing when the drops happened, then one card per device.
    fn summary_view(&self, ui: &mut egui::Ui, s: &Snoop, names: &Names) {
        let w = ui.available_width() - 6.0;
        let gap = 12.0;
        let drops = s.drops().count();
        let bursts = s.bursts(120.0);
        let tw = (w - gap * 3.0) / 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            tile(ui, tw, "LINK DROPS", &drops.to_string(), if drops > 0 { RED } else { GREEN });
            tile(ui, tw, "CAPTURED", &dur(s.duration()), CYAN);
            tile(ui, tw, "DATA PACKETS", &s.acl.to_string(), BLUE);
            tile(ui, tw, "DROP BURSTS", &bursts.len().to_string(), if bursts.is_empty() { GREEN } else { AMBER });
        });
        ui.add_space(14.0);
        strip(ui, w, s, &bursts);
        ui.add_space(14.0);
        let stats = s.per_device();
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
        self.refresh();
        let s = self.snoop.take();
        egui::Frame::NONE.inner_margin(18.0).show(ui, |ui| {
            // row 1: actions on the file (glass buttons)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let picking = self.pick.is_some();
                if action_button(ui, if picking { "Choosing..." } else { "Open" }, BLUE, 0.0, !picking, 0.0).clicked() {
                    self.pick = Some(Pick::start("Open a Bluetooth capture file"));
                }
                let name = self.clean_path().rsplit('/').next().unwrap_or("").to_string();
                ui.label(if name.is_empty() { RichText::new("No capture opened").color(DIM) } else { RichText::new(name).color(TEXT).size(15.0) });
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
                            let r = action_button(ui, label, mix(BLUE, tint, t), t, true, 128.0);
                            let r = match tip {
                                Some(t) => r.on_hover_text(t),
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
                for (i, l) in ["Summary", "Timeline"].iter().enumerate() {
                    if view_tab(ui, l, self.tab == i as u8) {
                        self.tab = i as u8;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if self.tab > 0 && toggle_chip(ui, "Problems only", self.problems_only, AMBER) {
                        self.problems_only = !self.problems_only;
                    }
                    if toggle_chip(ui, "Device addresses", self.addrs, CYAN) {
                        self.addrs = !self.addrs;
                    }
                    ui.label(RichText::new("SHOW").size(11.0).color(DIM));
                });
            });
            ui.separator();
            if let Some(e) = &self.err {
                ui.label(RichText::new(e).color(RED));
            }
            let Some(s) = &s else {
                if self.err.is_none() {
                    ui.label(RichText::new("Press Open (or drop a capture file on this window) to see what happened to each Bluetooth device: when the link dropped and why, and whether the controller was slow to answer.").color(DIM));
                    ui.label(RichText::new("A capture is made with: sudo btmon -w ~/capture.snoop   (it can still be running).").color(DIM));
                }
                return;
            };
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                0 => {
                    self.summary_view(ui, s, names);
                }
                _ => {
                    for e in s.events.iter().filter(|e| Self::problem(e) || !self.problems_only) {
                        ui.label(RichText::new(line(e, &self.who(&e.addr, names))).monospace().color(Self::colour(e)));
                    }
                }
            });
        });
        self.snoop = s;
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
fn strip(ui: &mut egui::Ui, w: f32, s: &Snoop, bursts: &[(f64, f64, usize)]) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 96.0), Sense::hover());
    let p = ui.painter();
    panel(p, r, 12.0, BLUE);
    p.text(pos2(r.left() + 18.0, r.top() + 18.0), Align2::LEFT_CENTER, "DROPS OVER TIME", FontId::proportional(12.0), DIM);
    p.text(pos2(r.right() - 18.0, r.top() + 18.0), Align2::RIGHT_CENTER, "red: radio-type drop   amber: other", FontId::proportional(11.5), DIM);
    let a = Rect::from_min_max(pos2(r.left() + 18.0, r.top() + 34.0), pos2(r.right() - 18.0, r.bottom() - 26.0));
    let total = s.duration().max(1.0);
    let x = |t: f64| a.left() + (t / total) as f32 * a.width();
    p.rect_filled(a, 4.0, alpha(Color32::WHITE, 0.04));
    for (t0, t1, _) in bursts {
        p.rect_filled(Rect::from_min_max(pos2(x(*t0) - 3.0, a.top()), pos2((x(*t1) + 3.0).max(x(*t0) + 3.0), a.bottom())), 3.0, alpha(RED, 0.14));
    }
    for e in s.drops() {
        let col = match e.kind {
            Kind::Disconnected { reason, .. } if crate::hci::reason(reason).2 => RED,
            _ => AMBER,
        };
        p.line_segment([pos2(x(e.t), a.top() + 4.0), pos2(x(e.t), a.bottom() - 4.0)], Stroke::new(2.0, col));
    }
    let first = Diag::first_unix(s);
    for (f, al) in [(0.0, Align2::LEFT_TOP), (0.5, Align2::CENTER_TOP), (1.0, Align2::RIGHT_TOP)] {
        p.text(pos2(a.left() + a.width() * f as f32, a.bottom() + 6.0), al, snoop::clock(first + total * f), FontId::proportional(11.5), DIM);
    }
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
