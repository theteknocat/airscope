//! The Diagnostics page. One layout for every source: a recording (live or opened from the Library) or the system log.
//! Source line, tiles, over-time strip, compact device cards, then the event table. The whole page scrolls.
//! A click on a card opens an info box (like the radar's) with that device's details and a "show only this device" filter.

use super::*;
use crate::syslog::{self, Entry, Sev, Who};

const GAP: f32 = 12.0;
const CARD_W: f32 = 300.0;
const CARD_H: f32 = 84.0; // room for a name and a two-line caption; text is top-aligned so every card lines up
pub(super) const HEADER_H: f32 = 52.0;
pub(super) const CTL_H: f32 = 28.0;
/// How many table rows are listed at first, and added each time the scroll nears the end.
const PAGE: usize = 1000;
const BOX_W: f32 = 520.0;

/// One compact device card.
pub(super) struct Card {
    key: String,
    name: String,
    ic: Icon,
    accent: Color32,
    big: String,
    label: &'static str,
    caption: String,
}

/// What the system log says about one device (cached; see `refresh_log_devs`).
pub(super) struct LogDev {
    key: String,
    name: String,
    ic: Icon,
    problems: usize,
    symptoms: usize,
    reconnects: usize,
    last: Option<f64>,
}

/// Everything above the table, for either source.
struct View {
    tiles: Vec<(&'static str, String, Color32)>,
    spec: StripSpec,
    ticks: Vec<Tick>,
    bursts: Vec<(f64, f64, usize)>,
    cards: Vec<Card>,
}

/// One table row, as text.
pub(super) struct Row {
    time: String,
    since: Option<String>,
    who: String,
    msg: String,
    col: Color32,
    bar: bool,
    tip: Option<(String, Vec<String>)>,
}

/// Table column positions: time, since start, device, event.
fn cols(since: bool) -> (f32, f32, f32, f32) {
    if since { (16.0, 196.0, 212.0, 436.0) } else { (16.0, 0.0, 160.0, 400.0) }
}

fn log_key(w: &Who) -> String {
    match w {
        Who::Mac(m) => m.clone(),
        Who::Name(n) => format!("name:{n}"),
        Who::None => String::new(),
    }
}

fn compact(ui: &mut egui::Ui, c: &Card, on: bool, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w, CARD_H), Sense::click());
    let p = ui.painter();
    panel(p, r, 12.0, c.accent);
    if resp.hovered() || on {
        p.rect_stroke(r, 12.0, Stroke::new(1.4, alpha(CYAN, if on { 0.9 } else { 0.55 })), egui::StrokeKind::Inside);
    }
    let oc = pos2(r.left() + 34.0, r.top() + 36.0);
    orb(p, oc, 16.0, BLUE, 0.0);
    icon(p, c.ic, oc, 8.0, shade(BLUE, 0.12));
    let avail = (w - 62.0 - 18.0 - 64.0).max(60.0);
    let name = crate::fit_middle(p, &c.name, FontId::proportional(15.0), avail);
    p.text(pos2(r.left() + 62.0, r.top() + 24.0), Align2::LEFT_CENTER, name, FontId::proportional(15.0), Color32::WHITE);
    // the caption's parts fill up to two lines; the first line sits level with the "PROBLEMS" label
    let cf = FontId::proportional(12.0);
    let (mut lines, mut cur): (Vec<String>, String) = (vec![], String::new());
    for part in c.caption.split("  \u{00b7}  ") {
        let joined = if cur.is_empty() { part.to_string() } else { format!("{cur}  \u{00b7}  {part}") };
        if cur.is_empty() || p.layout_no_wrap(joined.clone(), cf.clone(), DIM).size().x <= avail {
            cur = joined;
        } else {
            lines.push(std::mem::replace(&mut cur, part.to_string()));
        }
    }
    lines.push(cur);
    for (i, l) in lines.iter().take(2).enumerate() {
        let l = crate::fit_middle(p, l, cf.clone(), avail);
        p.text(pos2(r.left() + 62.0, r.top() + 50.0 + i as f32 * 16.0), Align2::LEFT_CENTER, l, cf.clone(), DIM);
    }
    lcd(p, pos2(r.right() - 18.0, r.top() + 27.0), Align2::RIGHT_CENTER, &c.big, 26.0, if c.accent == BLUE { GREEN } else { c.accent });
    p.text(pos2(r.right() - 18.0, r.top() + 50.0), Align2::RIGHT_CENTER, c.label, FontId::proportional(10.5), DIM);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Measures the width of each text at the table font, a few thousand per frame, so rows can be sized without
/// laying out every row every frame.
fn measure(ui: &egui::Ui, px: &mut Vec<f32>, n: usize, text: &dyn Fn(usize) -> String) {
    let font = FontId::proportional(13.5);
    for _ in 0..3000 {
        if px.len() >= n {
            break;
        }
        let w = ui.painter().layout_no_wrap(text(px.len()), font.clone(), Color32::WHITE).size().x;
        px.push(w);
    }
    if px.len() < n {
        ui.ctx().request_repaint();
    }
}

/// The table: header and only the rows in view (rows differ in height as long messages wrap, estimated from the
/// measured text widths, generously so rows never overlap).
fn table(ui: &mut egui::Ui, w: f32, idx: &[usize], px: &[f32], line_h: f32, since: bool, row: &dyn Fn(usize) -> Row) -> bool {
    let (x_time, x_off, x_dev, x_msg) = cols(since);
    let dev_w = x_msg - x_dev - 14.0;
    let msg_w = (w - x_msg - 14.0).max(160.0);
    let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    let p = ui.painter();
    let hf = FontId::proportional(11.5);
    p.text(pos2(hr.left() + x_time, hr.center().y), Align2::LEFT_CENTER, "TIME", hf.clone(), DIM);
    if since {
        p.text(pos2(hr.left() + x_off, hr.center().y), Align2::RIGHT_CENTER, "SINCE START", hf.clone(), DIM);
    }
    p.text(pos2(hr.left() + x_dev, hr.center().y), Align2::LEFT_CENTER, "DEVICE", hf.clone(), DIM);
    p.text(pos2(hr.left() + x_msg, hr.center().y), Align2::LEFT_CENTER, "EVENT", hf, DIM);
    p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.25)));
    let lines = |px: f32| if px <= msg_w - 2.0 { 1.0 } else { (px * 1.08 / msg_w).ceil() };
    let height = |i: usize| lines(px.get(i).copied().unwrap_or(0.0)) * line_h + 18.0;
    let total: f32 = idx.iter().map(|&i| height(i)).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(w, total.max(1.0)), Sense::hover());
    let p = ui.painter().clone();
    let clip = ui.clip_rect();
    let mut y = 0.0;
    for &i in idx {
        let h = height(i);
        if rect.top() + y > clip.bottom() {
            break;
        }
        if rect.top() + y + h >= clip.top() {
            let r = row(i);
            let top = rect.top() + y;
            if r.bar {
                p.rect_filled(Rect::from_min_size(pos2(rect.left() + 2.0, top + 5.0), vec2(3.0, h - 10.0)), 1.5, r.col);
            }
            let ty = top + 9.0;
            let mono = FontId::monospace(12.5);
            p.text(pos2(rect.left() + x_time, ty), Align2::LEFT_TOP, &r.time, mono.clone(), DIM);
            if let Some(s) = &r.since {
                p.text(pos2(rect.left() + x_off, ty), Align2::RIGHT_TOP, s, mono, DIM);
            }
            let who = crate::fit_middle(&p, &r.who, FontId::proportional(13.5), dev_w);
            p.text(pos2(rect.left() + x_dev, ty), Align2::LEFT_TOP, who, FontId::proportional(13.5), TEXT);
            let g = p.layout(r.msg.clone(), FontId::proportional(13.5), r.col, msg_w);
            p.galley(pos2(rect.left() + x_msg, ty), g, r.col);
            p.line_segment([pos2(rect.left(), top + h), pos2(rect.right(), top + h)], Stroke::new(1.0, alpha(Color32::WHITE, 0.07)));
            if let Some((title, sub)) = r.tip {
                let rid = ui.id().with(("row", i));
                let rr = Rect::from_min_size(pos2(rect.left(), top), vec2(w, h)).intersect(clip);
                if ui.interact(rr, rid, Sense::hover()).hovered() {
                    let anchor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rr.center());
                    let tc = crate::TipC { key: Some(rid), anchor, col: r.col, title, sub, hint: None, side: None };
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("hint_want"), Some(tc)));
                }
            }
        }
        y += h;
    }
    rect.bottom() - clip.bottom() < 1500.0 // close to the end of what is listed
}

/// A centred title over ghost table rows: nothing to show (yet), and why.
pub(super) fn placeholder(ui: &mut egui::Ui, w: f32, title: &str, col: Color32, since: bool) {
    let (tr, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
    lcd(ui.painter(), tr.center(), Align2::CENTER_CENTER, title, 13.0, col);
    ui.add_space(8.0);
    let (x_time, x_off, x_dev, x_msg) = cols(since);
    let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    let hf = FontId::proportional(11.5);
    let hc = alpha(DIM, 0.7);
    for (x, t, right) in [(x_time, "TIME", false), (x_off, "SINCE START", true), (x_dev, "DEVICE", false), (x_msg, "EVENT", false)] {
        if right && !since {
            continue;
        }
        ui.painter().text(pos2(hr.left() + x, hr.center().y), if right { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER }, t, hf.clone(), hc);
    }
    ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.15)));
    let bar = alpha(Color32::WHITE, 0.06);
    for k in 0..8 {
        let (rr, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
        let p = ui.painter();
        let cy = rr.center().y;
        p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_time, cy - 5.0), vec2(70.0, 10.0)), 5.0, bar);
        if since {
            p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_off - 52.0, cy - 5.0), vec2(52.0, 10.0)), 5.0, bar);
        }
        p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_dev, cy - 5.0), vec2([120.0, 90.0, 140.0][k % 3], 10.0)), 5.0, bar);
        p.rect_filled(Rect::from_min_size(pos2(rr.left() + x_msg, cy - 5.0), vec2((w - x_msg - 14.0).max(160.0) * [0.7, 0.45, 0.6, 0.35][k % 4], 10.0)), 5.0, bar);
        p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, alpha(Color32::WHITE, 0.05)));
    }
}

/// The kernel logs "X registered as a new input device" with only the device's name (the address on that line is the
/// adapter's). Gives such lines the address of the device they are about: first the nearest input-profile BlueZ line
/// naming an address within 30 s, then (for lines without one) the address that name was paired with elsewhere in the
/// log, then the one known device with that name. Several devices sharing a name with no timing clue stay name-only.
/// `names` = (address, name). Returns true when something changed.
fn resolve_input_names(es: &mut [Entry], names: &[(String, String)]) -> bool {
    let is_reg = |e: &Entry| matches!(e.who, Who::Name(_)) && e.text.contains("registered as a new input device");
    let near = |i: usize| -> Option<String> {
        let t = es[i].unix;
        let mac = |e: &Entry| match &e.who { Who::Mac(m) if e.origin.starts_with("profiles/input/") => Some(m.clone()), _ => None };
        let before = (0..i).rev().take_while(|&j| t - es[j].unix <= 30.0).find_map(|j| mac(&es[j]).map(|m| (t - es[j].unix, m)));
        let after = (i + 1..es.len()).take_while(|&j| es[j].unix - t <= 30.0).find_map(|j| mac(&es[j]).map(|m| (es[j].unix - t, m)));
        match (before, after) {
            (Some(b), Some(a)) => Some(if b.0 <= a.0 { b.1 } else { a.1 }),
            (b, a) => b.or(a).map(|x| x.1),
        }
    };
    let regs: Vec<(usize, String, Option<String>)> = (0..es.len()).filter(|&i| is_reg(&es[i])).map(|i| (i, log_key(&es[i].who)[5..].to_string(), near(i))).collect();
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    for (_, n, m) in &regs {
        if let Some(m) = m {
            let v = seen.entry(n.clone()).or_default();
            if !v.contains(m) {
                v.push(m.clone());
            }
        }
    }
    let mut changed = false;
    for (i, n, m) in regs {
        let known: Vec<&String> = names.iter().filter(|(_, nm)| *nm == n).map(|(a, _)| a).collect();
        let mac = m.or_else(|| match seen.get(&n).map(|v| v.as_slice()) {
            Some([one]) => Some(one.clone()),
            Some(_) => None,
            None => if let [one] = known.as_slice() { Some((*one).clone()) } else { None },
        });
        if let Some(mac) = mac {
            es[i].who = Who::Mac(mac);
            changed = true;
        }
    }
    changed
}

impl Diag {
    /// Per-device counts from the system log, recomputed only when the entries or the naming change.
    fn refresh_log_devs(&mut self, names: &Names) {
        let key = (self.log.ver, self.addrs, names.len());
        if self.log_devs_key == Some(key) {
            return;
        }
        let pairs: Vec<(String, String)> = names.iter().map(|(a, n)| (a.clone(), n.0.clone())).collect();
        if resolve_input_names(&mut self.log.entries, &pairs) {
            self.log.ver += 1; // rows and filter keys changed: the table list is rebuilt
            return self.refresh_log_devs(names);
        }
        let mut out: Vec<LogDev> = vec![];
        let mut at: HashMap<String, usize> = HashMap::new();
        for e in &self.log.entries {
            if e.who == Who::None {
                continue;
            }
            let k = log_key(&e.who);
            let i = *at.entry(k.clone()).or_insert_with(|| {
                let ic = match &e.who {
                    Who::Mac(m) => names.get(m).map_or(Icon::Bluetooth, |n| n.1),
                    _ => Icon::Bluetooth,
                };
                out.push(LogDev { key: k, name: self.log_who(&e.who, names), ic, problems: 0, symptoms: 0, reconnects: 0, last: None });
                out.len() - 1
            });
            let d = &mut out[i];
            match e.sev {
                Sev::Problem => d.problems += 1,
                Sev::Symptom => d.symptoms += 1,
                Sev::Info => d.reconnects += 1,
            }
            if e.sev != Sev::Info {
                d.last = Some(e.unix);
            }
        }
        out.sort_by_key(|d| std::cmp::Reverse((d.problems + d.symptoms, d.reconnects)));
        self.log_devs = out;
        self.log_devs_key = Some(key);
    }

    fn cap_view(&self, s: &Snoop, names: &Names) -> View {
        let drops = s.drops().count();
        let tiles = vec![
            ("LINK DROPS", drops.to_string(), if drops > 0 { RED } else { GREEN }),
            ("BURSTS", self.bursts.len().to_string(), if self.bursts.is_empty() { GREEN } else { AMBER }),
            ("COVERED", dur(s.duration()), CYAN),
            ("DATA PACKETS", s.acl.to_string(), BLUE),
        ];
        let spec = StripSpec { first: Self::first_unix(s), total: s.duration(), title: "DROPS OVER TIME", legend: ("other", "radio-type drop"), what: "link drops" };
        let ticks = s.drops().map(|e| Tick { t: e.t, unix: e.unix, radio: matches!(e.kind, Kind::Disconnected { reason, .. } if crate::hci::reason(reason).2), who: self.who(&e.addr, names), detail: None }).collect();
        let cards = self
            .stats
            .iter()
            .map(|d| {
                let radio = d.reasons.iter().any(|(c, _)| crate::hci::reason(*c).2);
                Card {
                    key: mac(&d.addr),
                    name: self.who(&d.addr, names),
                    ic: names.get(&mac(&d.addr)).map_or(Icon::Bluetooth, |n| n.1),
                    accent: if radio { RED } else if d.drops > 0 { AMBER } else { BLUE },
                    big: d.drops.to_string(),
                    label: "DROPS",
                    caption: format!("{} connects  \u{00b7}  {} packets", d.connects, d.tx + d.rx),
                }
            })
            .collect();
        View { tiles, spec, ticks, bursts: self.bursts.clone(), cards }
    }

    fn log_data(&self, ws: i64, we: i64, names: &Names) -> View {
        let es = &self.log.entries;
        let count = |sev: Sev| es.iter().filter(|e| e.sev == sev).count();
        let problems = count(Sev::Problem) + count(Sev::Symptom);
        let bursts = syslog::bursts(es, 120.0);
        let tiles = vec![
            ("PROBLEMS LOGGED", problems.to_string(), if problems > 0 { RED } else { GREEN }),
            ("BURSTS", bursts.to_string(), if bursts > 0 { AMBER } else { GREEN }),
            ("COVERED", dur((we - ws) as f64), CYAN),
            ("DATA PACKETS", String::new(), DIM), // ghost: only a recording has them
        ];
        let spec = StripSpec { first: ws as f64, total: (we - ws) as f64, title: "PROBLEMS OVER TIME", legend: ("symptom", "link problem"), what: "problems" };
        let ticks = es.iter().filter(|e| e.sev != Sev::Info).map(|e| Tick { t: e.unix - ws as f64, unix: e.unix, radio: e.sev == Sev::Problem, who: self.log_who(&e.who, names), detail: Some(e.text.clone()) }).collect();
        let spans = syslog::burst_spans(es, 120.0).into_iter().map(|(a, b, n)| (a - ws as f64, b - ws as f64, n)).collect();
        let cards = self
            .log_devs
            .iter()
            .map(|d| Card {
                key: d.key.clone(),
                name: d.name.clone(),
                ic: d.ic,
                accent: if d.problems > 0 { RED } else if d.symptoms > 0 { AMBER } else { BLUE },
                big: (d.problems + d.symptoms).to_string(),
                label: "PROBLEMS",
                caption: {
                    let parts: Vec<String> = [(d.problems, "link problems"), (d.symptoms, "symptoms"), (d.reconnects, "reconnects")].into_iter().filter(|p| p.0 > 0).map(|p| format!("{} {}", p.0, if p.0 == 1 { p.1.trim_end_matches('s') } else { p.1 })).collect();
                    if parts.is_empty() { "nothing logged".to_string() } else { parts.join("  \u{00b7}  ") }
                },
            })
            .collect();
        View { tiles, spec, ticks, bursts: spans, cards }
    }

    /// The page header: a small heading over the main line on the left, controls on the right. Always the same height.
pub(super) fn header(ui: &mut egui::Ui, sub: &str, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let lr = Rect::from_min_max(r.min, pos2(r.right(), r.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(lr).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.spacing_mut().interact_size.y = CTL_H;
        right(ui);
        let rest = Rect::from_min_max(lr.min, pos2(ui.min_rect().left().min(lr.right()) - 12.0, lr.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(rest).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space((HEADER_H - 38.0) / 2.0);
            ui.label(RichText::new(sub.to_uppercase()).size(11.5).color(DIM));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                left(ui);
            });
        });
    });
    ui.add_space(12.0);
}

/// The whole page below the toolbar.
    pub(super) fn page(&mut self, ui: &mut egui::Ui, s: Option<&Snoop>, names: &Names) {
        let cap = s.is_some() || !self.path.is_empty() || self.rec.is_some();
        let bounds = ui.max_rect();
        let before = self.info.clone();
        let mut window = None;
        if cap {
            self.name_row(ui);
        } else {
            window = self.log_controls(ui);
        }
        let w0 = ui.available_width();
        // what there is to show
        let (view, since, multi_day) = if cap {
            let Some(s) = s else {
                if self.err.is_none() {
                    placeholder(ui, w0, if self.rec.is_some() { "RECORDING IS STARTING" } else { "OPENING THE CAPTURE" }, CYAN, true);
                }
                return;
            };
            (self.cap_view(s, names), true, false)
        } else {
            if let Some(e) = &self.log.err {
                ui.label(RichText::new(e).color(RED));
                return;
            }
            let Some((ws, we)) = window else {
                placeholder(ui, w0, "PICK A START TIME", CYAN, false);
                return;
            };
            self.refresh_log_devs(names);
            (self.log_data(ws, we, names), false, we - ws > 20 * 3600)
        };
        if let (Some(i), false) = (std::env::var("AIRSCOPE_FILTER").ok().and_then(|v| v.parse::<usize>().ok()), self.dbg_done) {
            // debug aid for screenshots: start with the table filtered to this card's device
            self.dbg_done = true;
            self.dev_filter = view.cards.get(i).map(|c| c.key.clone());
        }
        // table rows: newest first, filtered
        let src = if cap { 0u8 } else { 1 };
        let (n, ver) = if cap { (s.map_or(0, |s| s.events.len()), s.map_or(0, |s| s.records)) } else { (self.log.entries.len(), self.log.ver) };
        let tkey = (src, n, ver, self.problems_only, self.dev_filter.clone());
        if (self.tl_key.0, self.tl_key.3, &self.tl_key.4) != (tkey.0, tkey.3, &tkey.4) {
            self.tl_limit = PAGE; // another list: start from the newest page again (a growing capture keeps its place)
        }
        if self.tl_key != tkey {
            let f = self.dev_filter.clone();
            self.tl = match (s, cap) {
                (Some(s), true) => (0..n).rev().filter(|&i| (Self::problem(&s.events[i]) || !self.problems_only) && f.as_ref().map_or(true, |k| mac(&s.events[i].addr) == *k)).collect(),
                _ => (0..n).rev().filter(|&i| { let e = &self.log.entries[i]; (!self.problems_only || e.sev != Sev::Info) && f.as_ref().map_or(true, |k| log_key(&e.who) == *k) }).collect(),
            };
            self.tl_key = tkey;
        }
        if self.line_h == 0.0 {
            self.line_h = ui.painter().layout_no_wrap("Ag".to_string(), FontId::proportional(13.5), Color32::WHITE).size().y;
        }
        match (s, cap) {
            (Some(s), true) => measure(ui, &mut self.ev_px, n, &|i| snoop::message(&s.events[i])),
            _ => {
                let es = &self.log.entries;
                measure(ui, &mut self.log_px, n, &|i| es[i].text.clone());
            }
        }

        // the page
        let mut click: Option<(String, Rect)> = None;
        let (mut flip_problems, mut flip_addrs, mut clear_filter) = (false, false, false);
        let mut grow = false;
        let mut pulse = false;
        let mut card_rect: Option<Rect> = None;
        let info = self.info.clone();
        let filter_name = self.dev_filter.as_ref().map(|k| view.cards.iter().find(|c| &c.key == k).map_or(k.clone(), |c| c.name.clone()));
        scroll_in_margin(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width() - SCROLL_MARGIN;
                let nt = view.tiles.len() as f32;
                let tw = (w - GAP * (nt - 1.0)) / nt;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for (l, v, c) in &view.tiles {
                        if tile(ui, tw, l, v, *c) {
                            pulse = true;
                        }
                    }
                });
                ui.add_space(14.0);
                strip(ui, w, &view.spec, &view.ticks, &view.bursts, &self.tip);
                ui.add_space(14.0);
                if !view.cards.is_empty() {
                    let cols_n = (((w + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
                    let cw = (w - GAP * (cols_n as f32 - 1.0)) / cols_n as f32;
                    for (ri, row) in view.cards.chunks(cols_n).enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for (ci, c) in row.iter().enumerate() {
                                let on = info.as_ref() == Some(&c.key);
                                let resp = compact(ui, c, on, cw);
                                if on {
                                    card_rect = Some(resp.rect);
                                }
                                let dbg = std::env::var("AIRSCOPE_INFO").ok().and_then(|v| v.parse::<usize>().ok()) == Some(ri * cols_n + ci);
                                if resp.clicked() || (dbg && !self.dbg_done) {
                                    click = Some((c.key.clone(), resp.rect));
                                }
                            }
                        });
                        ui.add_space(GAP);
                    }
                }
                // table toolbar: what it lists, then the filters
                ui.allocate_ui_with_layout(vec2(w, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.label(RichText::new("EVENTS").size(12.0).color(DIM));
                    if !cap {
                        if let (Some(o), Some((ws, _))) = (self.log.oldest, self.log.window) {
                            // dim normally; amber when the log doesn't reach back to the start of the chosen range
                            let short_log = o > ws as f64 + 60.0;
                            let r = ui.label(RichText::new(format!("Oldest log entry: {}  {}", snoop::date(o), snoop::clock(o))).size(13.0).color(if short_log { AMBER } else { DIM }));
                            if short_log {
                                r.hint("The log does not go back to the start of this range, so earlier problems can't be shown.");
                            }
                        }
                    }
                    if !cap && self.log_devs.iter().any(|d| d.problems + d.symptoms > 0) {
                        let t = "Record for additional details";
                        let g = ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(12.5), alpha(TEXT, 0.8));
                        if ui.available_width() > g.size().x + 40.0 + 330.0 {
                            let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 40.0, 28.0), Sense::click());
                            let hot = resp.hovered();
                            ui.painter().rect_filled(r, 14.0, alpha(Color32::WHITE, if hot { 0.09 } else { 0.04 }));
                            ui.painter().rect_stroke(r, 14.0, Stroke::new(1.0, alpha(Color32::WHITE, if hot { 0.4 } else { 0.18 })), egui::StrokeKind::Inside);
                            glyph(ui.painter(), Glyph::Record, pos2(r.left() + 17.0, r.center().y), 4.0, RED);
                            ui.painter().galley(pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, TEXT);
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint("The system log shows that problems happened, not why. A recording adds the reason for each drop and the packet timing. Click to see where to start one.").clicked() {
                                pulse = true;
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if toggle_chip(ui, "Device addresses", self.addrs, CYAN, if self.addrs { "Showing full device addresses. Click to hide them." } else { "Addresses are hidden. Click to show the full device addresses." }) {
                            flip_addrs = true;
                        }
                        let tip = match (cap, self.problems_only) {
                            (true, true) => "Showing only drops and slow answers. Click to show every event.",
                            (true, false) => "Showing every event. Click to show only drops and slow answers.",
                            (false, true) => "Showing only problems. Click to include routine events, such as a keyboard reconnecting.",
                            (false, false) => "Showing routine events too. Click to show only problems.",
                        };
                        if toggle_chip(ui, "Problems only", self.problems_only, AMBER, tip) {
                            flip_problems = true;
                        }
                        if let Some(n) = &filter_name {
                            let n = crate::fit_middle(ui.painter(), n, FontId::proportional(12.5), 220.0);
                            if toggle_chip(ui, &format!("Only: {n}"), true, CYAN, "Showing one device. Click to show all devices.") {
                                clear_filter = true;
                            }
                        }
                    });
                });
                if self.tl.is_empty() {
                    let title = if self.dev_filter.is_some() {
                        "NOTHING TO SHOW FOR THIS DEVICE"
                    } else if !cap && self.log.job.is_some() {
                        "READING THE LOG"
                    } else if !cap {
                        if self.log.entries.is_empty() { "NO BLUETOOTH PROBLEMS LOGGED" } else { "ONLY ROUTINE EVENTS LOGGED" }
                    } else if self.problems_only {
                        "NO PROBLEMS IN THIS CAPTURE"
                    } else {
                        "NO EVENTS IN THIS CAPTURE"
                    };
                    placeholder(ui, w, title, if !cap && self.log.job.is_some() { CYAN } else { GREEN }, since);
                } else if let (Some(s), true) = (s, cap) {
                    let row = |i: usize| {
                        let e = &s.events[i];
                        Row { time: snoop::clock(e.unix), since: Some(snoop::offset(e)), who: self.who(&e.addr, names), msg: snoop::message(e), col: Self::colour(e), bar: Self::problem(e), tip: None }
                    };
                    let shown = self.tl_limit.min(self.tl.len());
                    grow = table(ui, w, &self.tl[..shown], &self.ev_px, self.line_h, true, &row);
                } else {
                    let row = |i: usize| {
                        let e = &self.log.entries[i];
                        let col = match e.sev {
                            Sev::Problem => RED,
                            Sev::Symptom => AMBER,
                            Sev::Info => DIM,
                        };
                        let who = self.log_who(&e.who, names);
                        let mut sub = vec![format!("Device: {who}"), format!("Time: {}  {}", snoop::date(e.unix), snoop::clock(e.unix))];
                        sub.push(if e.origin.is_empty() { format!("From: {}", e.source) } else { format!("From: {} ({})", e.source, e.origin) });
                        sub.push(format!("Logged as: {}", e.detail));
                        let time = if multi_day { format!("{}  {}", logview::short(e.unix as i64).split(',').next().unwrap_or(""), snoop::clock(e.unix)) } else { snoop::clock(e.unix) };
                        Row { time, since: None, who, msg: e.text.clone(), col, bar: e.sev != Sev::Info, tip: Some((e.text.clone(), sub)) }
                    };
                    let shown = self.tl_limit.min(self.tl.len());
                    grow = table(ui, w, &self.tl[..shown], &self.log_px, self.line_h, false, &row);
                }
                if self.tl_limit < self.tl.len() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Showing the newest {} of {} events \u{00b7} scroll down for more", self.tl_limit, self.tl.len())).size(12.5).color(DIM));
                    ui.add_space(8.0);
                }
            });
        });
        if pulse {
            self.rec_pulse = Some(Instant::now());
        }
        if grow && self.tl_limit < self.tl.len() {
            self.tl_limit += PAGE;
        }
        if flip_problems {
            self.problems_only = !self.problems_only;
        }
        if flip_addrs {
            self.addrs = !self.addrs;
        }
        if clear_filter {
            self.dev_filter = None;
        }
        if let Some((k, r)) = click {
            self.dbg_done = true;
            self.info = if self.info.as_ref() == Some(&k) { None } else { Some(k) };
            self.info_card = Some(r);
        } else if card_rect.is_some() {
            self.info_card = card_rect;
        }
        // the info box: closes on Esc or a click outside it (a click on a card already switched it above)
        if self.info.is_some() {
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let outside = ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().map_or(false, |q| !self.info_rect.map_or(false, |r| r.contains(q))));
            if esc || (outside && self.info == before) {
                self.info = None;
            }
        }
        if self.info.is_some() {
            if let Some(k) = self.info_box(ui.ctx(), bounds, cap, names) {
                self.dev_filter = if self.dev_filter.as_ref() == Some(&k) { None } else { Some(k) };
                self.info = None;
            }
        } else {
            self.info_rect = None;
        }
    }

    /// The info box for the selected card. Returns the device key when "Show only this device" was pressed.
    fn info_box(&mut self, ctx: &egui::Context, bounds: Rect, cap: bool, names: &Names) -> Option<String> {
        let key = self.info.clone()?;
        let card_r = self.info_card?;
        // size by content
        enum What<'a> {
            Cap(&'a snoop::DevStat),
            Log(&'a LogDev, Vec<&'a syslog::Entry>),
        }
        let what = if cap {
            What::Cap(self.stats.iter().find(|d| mac(&d.addr) == key)?)
        } else {
            let d = self.log_devs.iter().find(|d| d.key == key)?;
            let recent: Vec<&syslog::Entry> = self.log.entries.iter().rev().filter(|e| e.sev != Sev::Info && log_key(&e.who) == key).take(6).collect();
            What::Log(d, recent)
        };
        let h = match &what {
            What::Cap(d) => card_h(d) + 64.0,
            What::Log(_, recent) => 78.0 + 4.0 * 22.0 + 14.0 + if recent.is_empty() { 0.0 } else { 26.0 + 22.0 * recent.len() as f32 } + 34.0 + 62.0,
        };
        let w = BOX_W.min(bounds.width() - 20.0);
        let x = card_r.left().clamp(bounds.left() + 10.0, (bounds.right() - w - 10.0).max(bounds.left() + 10.0));
        let below = card_r.bottom() + 8.0;
        let y = if below + h <= bounds.bottom() - 10.0 { below } else { (card_r.top() - h - 8.0).max(bounds.top() + 10.0) };
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        let mut pressed = false;
        let filtered = self.dev_filter.as_ref() == Some(&key);
        egui::Area::new(egui::Id::new("diag_info")).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min).show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(r.size(), Sense::click());
            let p = ui.painter_at(r.expand(40.0));
            match &what {
                What::Cap(d) => {
                    let name = self.who(&d.addr, names);
                    let ic = names.get(&mac(&d.addr)).map_or(Icon::Bluetooth, |n| n.1);
                    // the full per-device card fills the top; the button sits in the room below it
                    card_at(&p, r, d, &name, ic);
                }
                What::Log(d, recent) => {
                    let accent = if d.problems > 0 { RED } else if d.symptoms > 0 { AMBER } else { BLUE };
                    panel(&p, r, 14.0, accent);
                    let c = pos2(r.left() + 36.0, r.top() + 38.0);
                    orb(&p, c, 17.0, BLUE, 0.0);
                    icon(&p, d.ic, c, 8.5, shade(BLUE, 0.12));
                    p.text(pos2(r.left() + 68.0, r.top() + 28.0), Align2::LEFT_CENTER, &d.name, FontId::proportional(17.0), Color32::WHITE);
                    p.text(pos2(r.left() + 68.0, r.top() + 50.0), Align2::LEFT_CENTER, "from the system log", FontId::proportional(12.0), DIM);
                    lcd(&p, pos2(r.right() - 20.0, r.top() + 30.0), Align2::RIGHT_CENTER, &(d.problems + d.symptoms).to_string(), 30.0, if accent == BLUE { GREEN } else { accent });
                    p.text(pos2(r.right() - 20.0, r.top() + 54.0), Align2::RIGHT_CENTER, "PROBLEMS", FontId::proportional(11.0), DIM);
                    let mut yy = r.top() + 78.0;
                    for (l, v) in [("Link problems", d.problems.to_string()), ("Symptoms", d.symptoms.to_string()), ("Reconnects", d.reconnects.to_string()), ("Last problem", d.last.map_or("none".to_string(), |u| logview::short(u as i64)))] {
                        p.text(pos2(r.left() + 20.0, yy + 8.0), Align2::LEFT_CENTER, l, FontId::proportional(12.5), DIM);
                        p.text(pos2(r.right() - 20.0, yy + 8.0), Align2::RIGHT_CENTER, v, FontId::proportional(13.5), TEXT);
                        yy += 22.0;
                    }
                    yy += 14.0;
                    if !recent.is_empty() {
                        p.text(pos2(r.left() + 20.0, yy + 6.0), Align2::LEFT_CENTER, "MOST RECENT PROBLEMS", FontId::proportional(10.5), DIM);
                        yy += 26.0;
                        for e in recent {
                            let col = if e.sev == Sev::Problem { RED } else { AMBER };
                            p.text(pos2(r.left() + 20.0, yy + 8.0), Align2::LEFT_CENTER, logview::short(e.unix as i64), FontId::monospace(12.0), DIM);
                            let t = crate::fit_middle(&p, &e.text, FontId::proportional(13.0), w - 40.0 - 120.0);
                            p.text(pos2(r.left() + 140.0, yy + 8.0), Align2::LEFT_CENTER, t, FontId::proportional(13.0), col);
                            yy += 22.0;
                        }
                    }
                    p.text(pos2(r.left() + 20.0, yy + 14.0), Align2::LEFT_CENTER, "Record to see reason codes and timing.", FontId::proportional(12.0), DIM);
                }
            }
            let br = Rect::from_min_size(pos2(r.left() + 20.0, r.bottom() - 52.0), vec2(r.width() - 40.0, 36.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                if action_button(ui, Some(Glyph::List), if filtered { "Show all devices" } else { "Show only this device" }, BLUE, 0.0, true, 0.0).clicked() {
                    pressed = true;
                }
            });
        });
        self.info_rect = Some(r);
        pressed.then_some(key)
    }

    /// Re-reads the Library list now and then and finishes the analysis of captures nobody has opened (e.g. one that
    /// ended while the app was closed), one at a time.
    fn library_tick(&mut self) {
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
    }

    /// The Library picker under its toolbar button: "Open file…" first, then the saved recordings, newest first.
    pub(super) fn library_popover(&mut self, ctx: &egui::Context, bounds: Rect) {
        let Some(btn) = self.lib_btn else { return };
        if !self.lib_open {
            self.lib_rect = None;
            return;
        }
        self.library_tick();
        let n = self.sessions.len();
        let (pad, open_h, row_h) = (10.0, 38.0, 40.0);
        let list_h = if n == 0 { 44.0 } else { n.min(8) as f32 * row_h };
        let w = 470.0f32.min(bounds.width() - 20.0);
        let h = pad + open_h + 9.0 + list_h + pad;
        let r = Rect::from_min_size(pos2((btn.right() - w).max(bounds.left() + 10.0), btn.bottom() + 8.0), vec2(w, h));
        self.lib_rect = Some(r);
        let (mut open, mut del, mut ask, mut pick) = (None::<PathBuf>, None::<PathBuf>, None::<PathBuf>, false);
        let mut cancel = false;
        egui::Area::new(egui::Id::new("diag_library")).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min).show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(r.size(), Sense::click());
            let p = ui.painter_at(r.expand(40.0));
            panel(&p, r, 14.0, BLUE);
            let inner = r.shrink(pad);
            // "Open file…": a smaller row, so it stays at hand above a long list
            let orow = Rect::from_min_size(inner.min, vec2(inner.width(), open_h));
            let oresp = ui.interact(orow, ui.id().with("lib_open_file"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if oresp.hovered() {
                p.rect_filled(orow, 8.0, alpha(Color32::WHITE, 0.07));
            }
            glyph(&p, Glyph::Open, pos2(orow.left() + 22.0, orow.center().y), 7.0, Color32::WHITE);
            p.text(pos2(orow.left() + 42.0, orow.center().y), Align2::LEFT_CENTER, if self.pick.is_some() { "Choosing…" } else { "Open file…" }, FontId::proportional(13.5), Color32::WHITE);
            if oresp.clicked() && self.pick.is_none() {
                pick = true;
            }
            let sy = orow.bottom() + 4.0;
            p.line_segment([pos2(inner.left() + 6.0, sy), pos2(inner.right() - 6.0, sy)], Stroke::new(1.0, alpha(CYAN, 0.2)));
            let list = Rect::from_min_size(pos2(inner.left(), sy + 5.0), vec2(inner.width(), list_h));
            if n == 0 {
                p.text(list.center(), Align2::CENTER_CENTER, "No saved captures", FontId::proportional(13.5), DIM);
                return;
            }
            let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
            lui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut lui, |ui| {
                for sess in &self.sessions {
                    let recording = self.rec.as_ref().map_or(false, |x| x.file == sess.capture);
                    let asking = self.del_pending.as_ref() == Some(&sess.dir);
                    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), if asking { Sense::hover() } else { Sense::click() });
                    let resp = if asking { resp } else { resp.on_hover_cursor(egui::CursorIcon::PointingHand) };
                    let p = ui.painter().clone();
                    let bin = Rect::from_center_size(pos2(rr.right() - 22.0, rr.center().y), vec2(28.0, 28.0));
                    let ctw = p.layout_no_wrap("Confirm".into(), FontId::proportional(12.5), RED).size().x;
                    let conf = Rect::from_min_size(pos2(rr.right() - 8.0 - (28.0 + ctw + 6.0), rr.center().y - 14.0), vec2(28.0 + ctw + 6.0, 28.0));
                    let canc = Rect::from_min_size(pos2(conf.left() - 8.0 - 28.0, conf.top()), vec2(28.0, 28.0));
                    let bresp = if recording { None } else { Some(ui.interact(if asking { conf } else { bin }, ui.id().with(("lib_bin", &sess.dir)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)) };
                    let cresp = asking.then(|| ui.interact(canc, ui.id().with(("lib_undo", &sess.dir)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).hint("Keep this capture"));
                    let hot = resp.hovered();
                    if hot {
                        p.rect_filled(rr, 8.0, alpha(Color32::WHITE, 0.07));
                    }
                    let g = p.layout_no_wrap(snoop::pretty_stamp(&sess.name), FontId::proportional(14.0), if recording { RED } else { CYAN });
                    let gw = g.size().x;
                    p.galley(pos2(rr.left() + 14.0, rr.center().y - g.size().y / 2.0), g, CYAN);
                    let (detail, dcol) = match &sess.summary {
                        _ if recording => ("recording now".to_string(), RED),
                        Some(x) => (format!("{}  \u{00b7}  {}  \u{00b7}  {} drops", dur(x.duration), size_str(sess.size), x.drops), if x.drops > 0 { AMBER } else { DIM }),
                        None if self.bg.as_ref().map_or(false, |(c, _)| *c == sess.capture) => (format!("{}  \u{00b7}  analysing\u{2026}", size_str(sess.size)), DIM),
                        None => (format!("{}  \u{00b7}  not analysed yet", size_str(sess.size)), DIM),
                    };
                    p.text(pos2(rr.left() + 14.0 + gw + 16.0, rr.center().y), Align2::LEFT_CENTER, detail, FontId::proportional(12.5), dcol);
                    if asking {
                        let ch = cresp.as_ref().map_or(false, |c| c.hovered());
                        p.rect_filled(canc, 8.0, alpha(Color32::WHITE, if ch { 0.14 } else { 0.06 }));
                        glyph(&p, Glyph::Undo, canc.center(), 6.5, if ch { Color32::WHITE } else { DIM });
                        let bh = bresp.as_ref().map_or(false, |b| b.hovered());
                        p.rect_filled(conf, 8.0, alpha(RED, if bh { 0.35 } else { 0.18 }));
                        glyph(&p, Glyph::Trash, pos2(conf.left() + 14.0, conf.center().y), 6.5, RED);
                        p.text(pos2(conf.left() + 28.0, conf.center().y), Align2::LEFT_CENTER, "Confirm", FontId::proportional(12.5), RED);
                        if let Some(b) = bresp.clone() {
                            b.hint("Delete this capture");
                        }
                        if bresp.as_ref().map_or(false, |b| b.clicked()) {
                            del = Some(sess.dir.clone());
                        }
                        if cresp.as_ref().map_or(false, |c| c.clicked()) {
                            cancel = true;
                        }
                        continue;
                    }
                    if hot && bresp.as_ref().map_or(true, |b| !b.hovered()) {
                        p.text(pos2(bin.left() - 8.0, rr.center().y), Align2::RIGHT_CENTER, "Click to open", FontId::proportional(12.0), alpha(TEXT, 0.55));
                    }
                    let bh = bresp.as_ref().map_or(false, |b| b.hovered());
                    glyph(&p, Glyph::Trash, bin.center(), 6.5, if recording { alpha(DIM, 0.5) } else if bh { RED } else { alpha(RED, 0.65) });
                    if let Some(b) = bresp {
                        let b = b.hint("Delete this capture");
                        if b.clicked() {
                            ask = Some(sess.dir.clone());
                        }
                    } else {
                        // can't delete a recording in progress
                    }
                    if resp.clicked() && !bh {
                        open = Some(sess.capture.clone());
                    }
                }
            });
        });
        if cancel {
            self.del_pending = None;
        }
        if let Some(d) = ask {
            self.del_pending = Some(d);
        }
        if pick {
            self.pick = Some(Pick::start("Open a Bluetooth capture file"));
            self.lib_open = false;
        }
        if let Some(c) = open {
            self.open_path(c.display().to_string());
            self.lib_open = false;
        }
        if let Some(dir) = del {
            self.del_pending = None;
            let cap = dir.join(capture::CAPTURE_FILE);
            match capture::delete_session(&dir) {
                Ok(()) => {
                    if PathBuf::from(self.clean_path()) == cap {
                        self.close_capture(); // it was the open capture
                    }
                    self.rec_msg = Some((false, "Capture deleted".into()));
                }
                Err(e) => self.rec_msg = Some((true, e)),
            }
            self.sessions = capture::list();
            self.sessions_at = Some(Instant::now());
        }
    }
}

impl Diag {
    /// The "About this screen" box under the header's help button. Returns its rectangle (for outside-click closing).
    pub fn help_box(ctx: &egui::Context, btn: Rect) -> Rect {
        let w = 640.0f32;
        let rows: [(&str, (bool, &str), (bool, &str)); 5] = [
            ("Looks back in time", (true, "days, from the journal"), (false, "only while it runs")),
            ("Reason for each drop", (false, ""), (true, "")),
            ("Packet timing and silence before a drop", (false, ""), (true, "")),
            ("Data packet counts", (false, ""), (true, "")),
            ("Starts at once, no password", (true, ""), (false, "asks for your password")),
        ];
        let bullets = [
            "System log: look here to see what already happened.",
            "Recording: start one while the trouble is happening, to learn why.",
            "Click a device card for its details; \"Show only this device\" filters the table.",
            "Library reopens or deletes saved recordings. Export saves the report as a file.",
        ];
        let (row_h, pad) = (30.0, 20.0);
        let h = pad + 22.0 + 14.0 + 30.0 + rows.len() as f32 * row_h + 18.0 + bullets.len() as f32 * 24.0 + pad;
        let r = Rect::from_min_size(pos2((btn.right() - w).max(10.0), btn.bottom() + 8.0), vec2(w, h));
        egui::Area::new(egui::Id::new("diag_help")).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min).show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(r.size(), Sense::click());
            let p = ui.painter_at(r.expand(40.0));
            panel(&p, r, 14.0, BLUE);
            let (l, rt) = (r.left() + pad, r.right() - pad);
            let (c1, c2) = (r.left() + 310.0, r.left() + 470.0);
            lcd(&p, pos2(l, r.top() + pad + 11.0), Align2::LEFT_CENTER, "BT DIAGNOSTICS", 16.0, CYAN);
            let hy = r.top() + pad + 22.0 + 14.0 + 15.0;
            let hf = FontId::proportional(12.0);
            glyph(&p, Glyph::List, pos2(c1 - 4.0, hy), 5.5, CYAN);
            p.text(pos2(c1 + 8.0, hy), Align2::LEFT_CENTER, "SYSTEM LOG", hf.clone(), CYAN);
            glyph(&p, Glyph::Record, pos2(c2 - 4.0, hy), 5.0, RED);
            p.text(pos2(c2 + 8.0, hy), Align2::LEFT_CENTER, "RECORDING", hf, RED);
            p.line_segment([pos2(l, hy + 16.0), pos2(rt, hy + 16.0)], Stroke::new(1.0, alpha(CYAN, 0.2)));
            let mut y = hy + 18.0;
            for (label, a, b) in rows {
                let cy = y + row_h / 2.0;
                p.text(pos2(l, cy), Align2::LEFT_CENTER, label, FontId::proportional(13.5), TEXT);
                for (x, (yes, note)) in [(c1, a), (c2, b)] {
                    glyph(&p, if yes { Glyph::Check } else { Glyph::Cross }, pos2(x - 4.0, cy), 5.0, if yes { GREEN } else { alpha(DIM, 0.7) });
                    if !note.is_empty() {
                        p.text(pos2(x + 8.0, cy), Align2::LEFT_CENTER, note, FontId::proportional(11.5), DIM);
                    }
                }
                y += row_h;
            }
            y += 10.0;
            p.line_segment([pos2(l, y), pos2(rt, y)], Stroke::new(1.0, alpha(CYAN, 0.2)));
            y += 10.0;
            for b in bullets {
                dot(&p, pos2(l + 4.0, y + 12.0), 2.5, CYAN, 0.0);
                p.text(pos2(l + 16.0, y + 12.0), Align2::LEFT_CENTER, b, FontId::proportional(13.0), DIM);
                y += 24.0;
            }
        });
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_devices_get_distinct_filter_keys() {
        assert_eq!(log_key(&Who::Mac("AA:BB:CC:DD:EE:FF".into())), "AA:BB:CC:DD:EE:FF");
        assert_ne!(log_key(&Who::Name("K380 Keyboard".into())), log_key(&Who::Mac("K380 Keyboard".into())));
        assert_eq!(log_key(&Who::None), "");
    }

    #[test]
    fn keyboard_registrations_join_their_device() {
        let e = |t: f64, who: Who, origin: &str, text: &str| Entry { unix: t, sev: if text.contains("registered") { Sev::Info } else { Sev::Symptom }, who, text: text.into(), source: String::new(), origin: origin.into(), detail: String::new() };
        let reg = "Keyboard K380 registered as a new input device (it reconnected)";
        let k = || Who::Name("Keyboard K380".into());
        let mac = |m: &str| Who::Mac(m.into());
        let mut es = vec![e(100.0, mac("AA"), "profiles/input/device.c, hidp", "timed out"), e(105.0, k(), "", reg), e(500.0, k(), "", reg)];
        // timing pairs the first, the name seen once covers the second
        assert!(resolve_input_names(&mut es, &[("AA".into(), "Keyboard K380".into()), ("BB".into(), "Keyboard K380".into())]));
        assert_eq!((es[1].who.clone(), es[2].who.clone()), (mac("AA"), mac("AA")));
        // no timing clue and two devices with that name: stays name-only
        let mut es = vec![e(500.0, k(), "", reg)];
        assert!(!resolve_input_names(&mut es, &[("AA".into(), "Keyboard K380".into()), ("BB".into(), "Keyboard K380".into())]));
        // no timing clue, one device with that name
        assert!(resolve_input_names(&mut es, &[("AA".into(), "Keyboard K380".into())]));
        assert_eq!(es[0].who, mac("AA"));
    }

    #[test]
    fn table_columns_leave_room_for_the_event_text() {
        for since in [true, false] {
            let (t, _, d, m) = cols(since);
            assert!(t < d && d + 100.0 < m);
        }
    }
}
