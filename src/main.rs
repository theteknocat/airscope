mod data;
mod fx;

use data::{hash64, BtLink, Dev, Handle, Icon, Kind};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use fx::*;
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Instant;

const DMAX: f32 = 80.0;
const SIGNAL_LEEWAY_DB: f32 = 4.0; // a pinged device only moves if its signal changed by at least this much
const GROUP_PX: f32 = 38.0; // nodes closer than this on screen merge into one group icon

struct N {
    i: usize,
    pos: Pos2,
    ang: f32,
    dist: f32,
    dim: bool,
}

struct G {
    key: String,
    members: Vec<usize>,
    center0: Pos2,  // natural centre of the members
    center: Pos2,   // displayed centre (eased shift applied)
    center_t: Pos2, // where the centre is heading
    open: bool,
    fan_r: f32,
    halo_r: f32,
}

struct Ev {
    clicked: Option<String>,
    over: Vec<(String, (f32, f32))>,
    drag_end: bool,
    hover: Option<usize>,
    dragging: Option<String>,
}

struct Burst {
    pos: Pos2,
    col: Color32,
    t0: f32,
}

/// Orbit slot `k` of `nn` around a group icon.
fn slot(c: Pos2, k: usize, nn: usize, t: f32, s: f32) -> Pos2 {
    let a = t * 0.35 + k as f32 * TAU / nn.max(1) as f32;
    c + vec2(a.cos(), a.sin()) * 27.0 * s
}

/// An interactive area with the pointing-hand cursor (like a link on a web page).
fn hit(ui: &egui::Ui, rect: Rect, id: egui::Id, sense: Sense) -> egui::Response {
    ui.interact(rect, id, sense).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Soft-edged, semi-transparent dark disc that obscures what is behind an open group.
fn soft_halo(p: &egui::Painter, c: Pos2, r: f32, amt: f32) {
    // translucent dark disc with a short, subtle shadow-like fade just outside the edge
    let feather = 14.0;
    const SEGS: usize = 64;
    let k = 0.68 * amt;
    let inner = Color32::from_rgba_premultiplied((5.0 * k) as u8, (10.0 * k) as u8, (22.0 * k) as u8, (255.0 * k) as u8);
    let mut m = egui::Mesh::default();
    m.colored_vertex(c, inner);
    for (rr, col) in [(r, inner), (r + feather, Color32::TRANSPARENT)] {
        for i in 0..SEGS {
            let a = i as f32 / SEGS as f32 * TAU;
            m.colored_vertex(c + vec2(a.cos(), a.sin()) * rr, col);
        }
    }
    let n = SEGS as u32;
    for i in 0..n {
        let (a0, a1) = (1 + i, 1 + (i + 1) % n);
        m.add_triangle(0, a0, a1);
        m.add_triangle(a0, a0 + n, a1);
        m.add_triangle(a1, a0 + n, a1 + n);
    }
    p.add(egui::Shape::mesh(m));
    p.circle_stroke(c, r, Stroke::new(1.0, alpha(CYAN, 0.22 * amt)));
}

#[allow(clippy::too_many_arguments)]
fn node_widget(ui: &egui::Ui, p: &egui::Painter, d: &Dev, n: &N, ni: usize, pos: Pos2, t: f32, sw: f32, sel: bool, label: bool, interactive: bool, can_drag: bool, scale: f32, c: Pos2, big: f32, ev: &mut Ev) {
    let col = if n.dim { shade(kind_col(d.kind), 0.5) } else { kind_col(d.kind) };
    let diff = (sw - n.ang).rem_euclid(TAU);
    let hot = if diff < 0.9 { 1.0 - diff / 0.9 } else { 0.0 };
    let rad = scale * if d.known { 17.0 } else { 4.0 + ((d.dbm.unwrap_or(-85.0) + 100.0) / 50.0).clamp(0.0, 1.0) * 8.0 };
    let mut hovered = false;
    if interactive {
        let sense = if can_drag { Sense::click_and_drag() } else { Sense::click() };
        let resp = hit(ui, Rect::from_center_size(pos, Vec2::splat(rad * 2.0 + 12.0)), ui.scope_id().with(("node", &d.id)), sense);
        hovered = resp.hovered();
        if hovered {
            ev.hover = Some(ni);
        }
        if can_drag && resp.dragged() {
            ev.dragging = Some(d.id.clone());
            if let Some(pp) = resp.interact_pointer_pos() {
                let v = pp - c;
                ev.over.push((d.id.clone(), (v.y.atan2(v.x), dmap(v.length(), big).clamp(0.6, DMAX))));
            }
        }
        if resp.drag_stopped() {
            ev.drag_end = true;
        }
        if resp.clicked() {
            ev.clicked = Some(d.id.clone());
        }
    }
    let boost = hot * 0.8 + if sel || hovered { 0.9 } else { 0.0 };
    if d.connected {
        let pulse = (t * 2.6).sin() * 0.5 + 0.5;
        p.circle_stroke(pos, rad + 5.0 + pulse * 5.0, Stroke::new(1.5, add(col, 0.35 * (1.0 - pulse))));
    }
    if d.known {
        orb(p, pos, rad, col, boost);
        icon(p, d.icon, pos, rad * 0.5, shade(col, 0.12));
    } else {
        dot(p, pos, rad, col, boost);
    }
    if sel {
        for kk in 0..4 {
            let a = t * 1.4 + kk as f32 * TAU / 4.0;
            arc(p, pos, rad + 10.0, a, a + 0.9, Stroke::new(2.0, Color32::WHITE));
        }
    }
    if (label && d.known && !n.dim) || sel || hovered {
        let tp = pos + vec2(0.0, rad + 13.0);
        p.text(tp + vec2(1.0, 1.0), Align2::CENTER_CENTER, &d.name, FontId::proportional(11.5), Color32::from_black_alpha(200));
        p.text(tp, Align2::CENTER_CENTER, &d.name, FontId::proportional(11.5), alpha(TEXT, 0.9));
    }
}

fn pill(p: &egui::Painter, anchor: Pos2, title: &str, sub: &str, col: Color32) {
    let g1 = p.layout_no_wrap(title.to_string(), FontId::proportional(14.0), Color32::WHITE);
    let g2 = p.layout_no_wrap(sub.to_string(), FontId::proportional(11.5), DIM);
    let w = g1.size().x.max(g2.size().x) + 28.0;
    let r = Rect::from_min_size(pos2(anchor.x - w / 2.0, anchor.y - 46.0), vec2(w, 46.0));
    panel(p, r, 10.0, col);
    p.galley(pos2(r.left() + 14.0, r.top() + 7.0), g1, Color32::WHITE);
    p.galley(pos2(r.left() + 14.0, r.top() + 26.0), g2, DIM);
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Radar,
    Spectrum,
    Devices,
}

struct App {
    h: Handle,
    ver: u64,
    devs: Vec<Dev>,
    links: Vec<BtLink>,
    wifi_at: Option<Instant>,
    bt_at: Option<Instant>,
    lan_at: Option<Instant>,
    errs: Vec<String>,
    screen: Screen,
    sel: Option<String>,
    over: HashMap<String, (f32, f32)>, // id -> (angle rad, metres)
    host: String,
    t0: Instant,
    zoom: f32,
    pan: Vec2,
    open: HashSet<String>,         // group keys currently open (click to open/close)
    offs: HashMap<String, Vec2>,   // smoothed offset of each device from its base position
    halo: HashMap<String, f32>,    // 0..1 fade of each group's obscuring circle
    gscale: HashMap<String, f32>,  // 0..1 pop-in of each group icon
    reveal: Option<String>,        // device picked in the list: open its group once
    reset_view: bool,              // easing back to the default zoom/pan
    latched: Vec<Dev>,             // device data as of each device's last radar ping
    sw_prev: f32,
    sdist: HashMap<String, f32>,   // eased displayed distance
    appear: HashMap<String, f32>,  // 0..1 pop-in of new devices
    bursts: Vec<Burst>,
    last_pos: HashMap<String, Pos2>,
    dragging: Option<String>,
    prev_groups: Vec<(String, Vec<String>)>,
    formed: HashSet<String>,       // groups whose icon is already showing
    gshift: HashMap<String, Vec2>, // eased push-away shift per group
    mine: Vec<String>,             // optional extra SSID prefixes counted as yours (~/.config/airscope/mine.txt)
    q: String,                     // device list filters
    show: [bool; 3],
    scope: u8,
    hide_unnamed: bool,
    collapsed: HashSet<usize>,
}

fn cfg_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".config/airscope/positions.tsv"))
}

fn kind_col(k: Kind) -> Color32 {
    match k {
        Kind::Wifi => AMBER,
        Kind::Bt => BLUE,
        Kind::Lan => GREEN,
    }
}

fn rmap(d: f32, big: f32) -> f32 {
    big * (1.0 + d).ln() / (1.0 + DMAX).ln()
}
fn dmap(r: f32, big: f32) -> f32 {
    ((r / big) * (1.0 + DMAX).ln()).exp() - 1.0
}

/// Default placement: (angle, metres, dimmed). Angle is a stable hash (no direction info from one receiver).
fn layout_of(d: &Dev) -> (f32, f32, bool) {
    let a = (hash64(&d.cluster) % 3600) as f32 / 3600.0 * TAU + ((hash64(&d.id) % 1000) as f32 / 1000.0 - 0.5) * 0.9;
    let j = (hash64(&d.id) % 1000) as f32 / 1000.0;
    match d.kind {
        Kind::Wifi => (a, d.dist_m.unwrap_or(30.0) * (0.8 + j * 0.45), false),
        Kind::Bt if d.connected => (a, d.dist_m.unwrap_or(1.8), false),
        Kind::Bt => (a, 62.0 + j * 8.0, true),
        Kind::Lan => (a, 40.0 + j * 14.0, !d.online),
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.override_text_color = Some(TEXT);
        cc.egui_ctx.set_visuals(v);
        let mut over = HashMap::new();
        if let Some(s) = cfg_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            for l in s.lines() {
                let f: Vec<&str> = l.split('\t').collect();
                if f.len() == 3 {
                    if let (Ok(a), Ok(d)) = (f[1].parse(), f[2].parse()) {
                        over.insert(f[0].to_string(), (a, d));
                    }
                }
            }
        }
        let host = std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().to_string();
        App {
            h: data::spawn(),
            ver: u64::MAX,
            devs: vec![],
            links: vec![],
            wifi_at: None,
            bt_at: None,
            lan_at: None,
            errs: vec![],
            screen: match std::env::var("AIRSCOPE_SCREEN").as_deref() { Ok("spectrum") => Screen::Spectrum, Ok("devices") => Screen::Devices, _ => Screen::Radar }, // debug aid
            sel: None,
            over,
            host: if host.is_empty() { "this PC".into() } else { host },
            t0: Instant::now(),
            zoom: std::env::var("AIRSCOPE_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0), // debug aid
            pan: Vec2::ZERO,
            open: HashSet::new(),
            offs: HashMap::new(),
            halo: HashMap::new(),
            gscale: HashMap::new(),
            reveal: None,
            reset_view: false,
            latched: vec![],
            sw_prev: 0.0,
            sdist: HashMap::new(),
            appear: HashMap::new(),
            bursts: vec![],
            last_pos: HashMap::new(),
            dragging: None,
            prev_groups: vec![],
            formed: HashSet::new(),
            gshift: HashMap::new(),
            mine: std::env::var("HOME").ok().and_then(|h| std::fs::read_to_string(format!("{}/.config/airscope/mine.txt", h)).ok()).map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default(),
            q: String::new(),
            show: [true; 3],
            scope: 0,
            hide_unnamed: false,
            collapsed: HashSet::from([4]),
        }
    }

    fn save_over(&self) {
        if let Some(p) = cfg_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let s: String = self.over.iter().map(|(k, (a, d))| format!("{}\t{}\t{}\n", k, a, d)).collect();
            let _ = std::fs::write(p, s);
        }
    }

    fn sync(&mut self) {
        let s = self.h.lock().unwrap();
        if s.ver == self.ver {
            return;
        }
        self.ver = s.ver;
        self.devs.clear();
        self.devs.extend(s.wifi.iter().cloned());
        self.devs.extend(s.bt.iter().cloned());
        self.devs.extend(s.lan.iter().cloned());
        self.links = s.links.clone();
        self.wifi_at = s.wifi_at;
        self.bt_at = s.bt_at;
        self.lan_at = s.lan_at;
        self.errs = s.errs.values().cloned().collect();
    }

    // ---------- chrome ----------

    fn nav(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, shade(BG, 1.5));
        p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.18)));
        lcd(&p, pos2(rect.left() + 22.0, rect.top() + 38.0), Align2::LEFT_CENTER, "AIRSCOPE", 21.0, CYAN);
        let items = [(Screen::Radar, "Radar", 0u8), (Screen::Spectrum, "Spectrum", 1), (Screen::Devices, "Devices", 2)];
        for (i, (s, label, ic)) in items.iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + 14.0, rect.top() + 84.0 + i as f32 * 52.0), vec2(rect.width() - 28.0, 42.0));
            let resp = hit(ui, r, ui.scope_id().with(label), Sense::click());
            let on = self.screen == *s;
            if on {
                panel(&p, r, 10.0, BLUE);
                p.rect_stroke(r, 10.0, Stroke::new(2.0, BLUE), StrokeKind::Outside);
                p.rect_stroke(r.expand(3.0), 12.0, Stroke::new(3.0, add(BLUE, 0.12)), StrokeKind::Outside);
            } else if resp.hovered() {
                p.rect_filled(r, 10.0, alpha(BLUE, 0.12));
            }
            nav_icon(&p, *ic, pos2(r.left() + 26.0, r.center().y), 8.0, if on { Color32::WHITE } else { DIM });
            p.text(pos2(r.left() + 52.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(16.0), if on { Color32::WHITE } else { TEXT });
            if resp.clicked() {
                self.screen = *s;
            }
        }
        // legend
        let mut y = rect.bottom() - 110.0;
        for (c, l) in [(AMBER, "Wi-Fi"), (BLUE, "Bluetooth"), (GREEN, "Network (LAN)"), (RED, "Interference")] {
            dot(&p, pos2(rect.left() + 28.0, y), 5.0, c, 0.0);
            p.text(pos2(rect.left() + 46.0, y), Align2::LEFT_CENTER, l, FontId::proportional(13.0), DIM);
            y += 22.0;
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, _t: f32) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, BG);
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, alpha(CYAN, 0.12)));
        let title = match self.screen {
            Screen::Radar => "RADAR",
            Screen::Spectrum => "SPECTRUM",
            Screen::Devices => "DEVICES",
        };
        lcd(&p, pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, title, 28.0, CYAN);

        let age = |t: Option<Instant>| t.map(|t| format!("{}s", t.elapsed().as_secs())).unwrap_or("--".into());
        let wifi_n = self.devs.iter().filter(|d| d.kind == Kind::Wifi).count();
        let bt_n = self.links.len();
        let lan_n = self.devs.iter().filter(|d| d.kind == Kind::Lan).count();
        let chips = [
            (AMBER, format!("WI-FI {} APs · {}", wifi_n, age(self.wifi_at))),
            (BLUE, format!("BT {} linked · {}", bt_n, age(self.bt_at))),
            (GREEN, format!("LAN {} · {}", lan_n, age(self.lan_at))),
        ];
        let mut x = rect.right() - 20.0;
        // rescan button
        {
            let r = Rect::from_min_size(pos2(x - 130.0, rect.center().y - 17.0), vec2(130.0, 34.0));
            let resp = hit(ui, r, ui.scope_id().with("rescan"), Sense::click());
            panel(&p, r, 9.0, if resp.hovered() { CYAN } else { BLUE });
            p.text(r.center(), Align2::CENTER_CENTER, "Rescan Wi-Fi", FontId::proportional(14.0), Color32::WHITE);
            if resp.clicked() {
                self.h.lock().unwrap().rescan = true;
            }
            resp.on_hover_text("Active scan. It briefly disturbs Bluetooth on the shared Wi-Fi/BT chip, so it only runs when you ask.");
            x -= 146.0;
        }
        for (c, txt) in chips.iter().rev() {
            let g = p.layout_no_wrap(txt.clone(), FontId::monospace(12.0), TEXT);
            let w = g.size().x + 40.0;
            let r = Rect::from_min_size(pos2(x - w, rect.center().y - 15.0), vec2(w, 30.0));
            panel(&p, r, 15.0, *c);
            dot(&p, pos2(r.left() + 16.0, r.center().y), 3.5, *c, 0.2);
            p.galley(pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, TEXT);
            x -= w + 10.0;
        }
        if !self.errs.is_empty() {
            p.text(pos2(rect.left() + 220.0, rect.center().y), Align2::LEFT_CENTER, self.errs.join(" · "), FontId::monospace(12.0), RED);
        }
    }

    // ---------- radar ----------

    /// Radar behaviour: a device's data (signal, position, details) is only refreshed when the
    /// sweep passes it ("ping"). New devices appear at once. Nothing refreshes while a group is
    /// open or a drag is in progress, so the layout never changes under the user's hands.
    fn latch(&mut self, sw_prev: f32, sw: f32, t: f32) {
        let freeze = !self.open.is_empty() || self.dragging.is_some();
        let span = (sw - sw_prev).rem_euclid(TAU);
        let crossed = |a: f32| {
            let d1 = (a - sw_prev).rem_euclid(TAU);
            d1 <= span && span < 1.0
        };
        let angle = |d: &Dev, over: &HashMap<String, (f32, f32)>| over.get(&d.id).map(|o| o.0).unwrap_or_else(|| layout_of(d).0).rem_euclid(TAU);
        let idx: HashMap<String, usize> = self.latched.iter().enumerate().map(|(i, d)| (d.id.clone(), i)).collect();
        for d in &self.devs {
            match idx.get(&d.id) {
                None => {
                    self.appear.insert(d.id.clone(), 0.0);
                    self.latched.push(d.clone());
                }
                Some(&i) => {
                    if !freeze && crossed(angle(d, &self.over)) {
                        // leeway: ignore small signal wiggles so dots only move on a real change
                        let old = &self.latched[i];
                        let mut nd = d.clone();
                        if let (Some(a), Some(b)) = (old.dbm, d.dbm) {
                            if (a - b).abs() < SIGNAL_LEEWAY_DB {
                                nd.dbm = old.dbm;
                                nd.dist_m = old.dist_m;
                            }
                        }
                        self.latched[i] = nd;
                    }
                }
            }
        }
        if !freeze {
            let live: HashSet<&str> = self.devs.iter().map(|d| d.id.as_str()).collect();
            let mut gone: Vec<(String, Kind)> = vec![];
            let over = &self.over;
            self.latched.retain(|d| {
                if !live.contains(d.id.as_str()) && crossed(angle(d, over)) {
                    gone.push((d.id.clone(), d.kind));
                    false
                } else {
                    true
                }
            });
            for (id, kind) in gone {
                if let Some(&pos) = self.last_pos.get(&id) {
                    self.bursts.push(Burst { pos, col: kind_col(kind), t0: t });
                }
            }
        }
    }

    fn radar(&mut self, ui: &mut egui::Ui, t: f32) {
        let rect = ui.max_rect();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);

        // --- view: zoom + pan. The background is registered first (and has no hand cursor).
        let base = (rect.width().min(rect.height()) * 0.5 - 28.0).max(120.0);
        let bg = ui.interact(rect, ui.scope_id().with("radar_bg"), Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let dt = ui.input(|i| i.stable_dt).min(0.05);
        let k = 1.0 - (-dt * 18.0).exp(); // ~150 ms ease for node transitions
        let k_slow = 1.0 - (-dt * 5.0).exp(); // ~0.5 s ease for signal-driven moves
        if bg.dragged() {
            self.pan += bg.drag_delta();
            self.reset_view = false; // taking over cancels a running reset
        }
        if bg.double_clicked() {
            self.reset_view = true;
        }
        if self.reset_view {
            // ease zoom in log space (feels even) and pan linearly, ~400 ms
            let kv = 1.0 - (-dt * 9.0).exp();
            self.zoom = (self.zoom.ln() * (1.0 - kv)).exp();
            self.pan *= 1.0 - kv;
            if (self.zoom - 1.0).abs() < 0.003 && self.pan.length() < 0.6 {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
                self.reset_view = false;
            }
        }
        if let Some(ptr) = pointer {
            if rect.contains(ptr) {
                let sc = ui.input(|i| i.smooth_scroll_delta.y);
                if sc != 0.0 {
                    self.reset_view = false;
                    let z0 = self.zoom;
                    let z1 = (z0 * (1.0 + sc * 0.0015)).clamp(0.4, 14.0);
                    let c0 = rect.center() + self.pan;
                    self.pan += (ptr - c0) * (1.0 - z1 / z0); // keep the point under the cursor fixed
                    self.zoom = z1;
                }
            }
        }
        let c = rect.center() + self.pan;
        let big = base * self.zoom;
        let ro = rmap(DMAX, big);

        // --- sweep + pings
        let sw = (t * 0.9) % TAU;
        let sw_prev = std::mem::replace(&mut self.sw_prev, sw);
        self.latch(sw_prev, sw, t);
        if !ui.input(|i| i.pointer.any_down()) {
            self.dragging = None; // failsafe: a drag cannot outlive the mouse button
        }

        // --- backdrop
        for i in 0..18 {
            let kk = i as f32 / 18.0;
            p.circle_filled(c, big * 1.2 * (1.0 - kk), mix(BG, Color32::from_rgb(11, 28, 56), kk * kk));
        }
        for i in 0..12 {
            let a = i as f32 / 12.0 * TAU;
            p.line_segment([c, c + vec2(a.cos(), a.sin()) * ro], Stroke::new(1.0, alpha(CYAN, 0.05)));
        }
        for (d, lbl) in [(1.0, "1 m"), (3.0, "3 m"), (10.0, "10 m"), (30.0, "30 m")] {
            let r = rmap(d, big);
            p.circle_stroke(c, r, Stroke::new(3.0, add(CYAN, 0.025)));
            p.circle_stroke(c, r, Stroke::new(1.0, alpha(CYAN, 0.2)));
            p.text(pos2(c.x + r * 0.7071 + 4.0, c.y - r * 0.7071 - 2.0), Align2::LEFT_BOTTOM, lbl, FontId::monospace(10.0), alpha(CYAN, 0.6));
        }
        p.circle_stroke(c, ro, Stroke::new(1.5, alpha(CYAN, 0.3)));
        p.text(
            pos2(rect.center().x, rect.top() + 14.0),
            Align2::CENTER_CENTER,
            "OUTER BAND: network devices (no signal reading) and paired-but-not-connected Bluetooth",
            FontId::monospace(10.0),
            alpha(CYAN, 0.45),
        );
        let mut m = egui::Mesh::default();
        let n = 36;
        for kk in 0..n {
            let a0 = sw - kk as f32 * 0.025;
            let a1 = sw - (kk + 1) as f32 * 0.025;
            let col = add(CYAN, 0.16 * (1.0 - kk as f32 / n as f32).powi(2));
            let i0 = m.vertices.len() as u32;
            m.colored_vertex(c, col);
            m.colored_vertex(c + vec2(a0.cos(), a0.sin()) * ro, col);
            m.colored_vertex(c + vec2(a1.cos(), a1.sin()) * ro, col);
            m.add_triangle(i0, i0 + 1, i0 + 2);
        }
        p.add(egui::Shape::mesh(m));
        p.line_segment([c, c + vec2(sw.cos(), sw.sin()) * ro], Stroke::new(1.5, alpha(CYAN, 0.55)));

        // --- placement (screen space, far first). Distance eases toward the latest pinged value.
        let drag_id = self.dragging.clone();
        let mut nodes: Vec<N> = self
            .latched
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let (mut a, mut dd, dim) = layout_of(d);
                let over = self.over.get(&d.id).copied();
                if let Some((oa, od)) = over {
                    a = oa;
                    dd = od;
                }
                let sd = self.sdist.entry(d.id.clone()).or_insert(dd);
                if over.is_some() {
                    *sd = dd; // user-placed: follows the pointer exactly
                } else {
                    *sd += (dd - *sd) * k_slow;
                }
                N { i, pos: c + vec2(a.cos(), a.sin()) * rmap(*sd, big), ang: a.rem_euclid(TAU), dist: *sd, dim }
            })
            .collect();
        nodes.sort_by(|a, b| b.dist.partial_cmp(&a.dist).unwrap());
        let cnt = nodes.len();
        let drag_ni = drag_id.as_ref().and_then(|id| nodes.iter().position(|n| &self.latched[n.i].id == id));

        // --- grouping: nodes closer than GROUP_PX on screen merge (union-find).
        // A node being dragged is left out until it is dropped.
        let mut parent: Vec<usize> = (0..cnt).collect();
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for i in 0..cnt {
            for j in (i + 1)..cnt {
                if Some(i) == drag_ni || Some(j) == drag_ni {
                    continue;
                }
                if (nodes[i].pos - nodes[j].pos).length() < GROUP_PX {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a] = b;
                    }
                }
            }
        }
        let mut by_root: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..cnt {
            let r = find(&mut parent, i);
            by_root.entry(r).or_default().push(i);
        }
        let mut raw: Vec<Vec<usize>> = by_root.into_values().filter(|m| m.len() > 1).collect();
        for m in raw.iter_mut() {
            m.sort_by(|&a, &b| self.latched[nodes[a].i].id.cmp(&self.latched[nodes[b].i].id));
        }
        // stable group identity: inherit the key of the previous group with the most members in common
        let mut cand: Vec<(usize, usize, usize)> = vec![];
        for (ri, m) in raw.iter().enumerate() {
            let ids: HashSet<&str> = m.iter().map(|&i| self.latched[nodes[i].i].id.as_str()).collect();
            for (pi, (_, pids)) in self.prev_groups.iter().enumerate() {
                let ov = pids.iter().filter(|x| ids.contains(x.as_str())).count();
                if ov > 0 {
                    cand.push((ov, ri, pi));
                }
            }
        }
        cand.sort_by(|a, b| b.0.cmp(&a.0));
        let mut key_of: Vec<Option<String>> = vec![None; raw.len()];
        let mut used_prev: HashSet<usize> = HashSet::new();
        let mut used_keys: HashSet<String> = HashSet::new();
        for (_, ri, pi) in cand {
            if key_of[ri].is_none() && !used_prev.contains(&pi) {
                used_prev.insert(pi);
                used_keys.insert(self.prev_groups[pi].0.clone());
                key_of[ri] = Some(self.prev_groups[pi].0.clone());
            }
        }
        for (ri, m) in raw.iter().enumerate() {
            if key_of[ri].is_none() {
                let mut key = self.latched[nodes[m[0]].i].id.clone();
                while used_keys.contains(&key) {
                    key.push('\'');
                }
                used_keys.insert(key.clone());
                key_of[ri] = Some(key);
            }
        }
        self.prev_groups = raw.iter().enumerate().map(|(ri, m)| (key_of[ri].clone().unwrap(), m.iter().map(|&i| self.latched[nodes[i].i].id.clone()).collect())).collect();

        let reveal = self.reveal.take();
        let mut groups: Vec<G> = vec![];
        for (ri, mem) in raw.into_iter().enumerate() {
            let center0 = pos2(mem.iter().map(|&i| nodes[i].pos.x).sum::<f32>() / mem.len() as f32, mem.iter().map(|&i| nodes[i].pos.y).sum::<f32>() / mem.len() as f32);
            let key = key_of[ri].clone().unwrap();
            if let Some(r) = &reveal {
                if mem.iter().any(|&i| &self.latched[nodes[i].i].id == r) {
                    self.open.insert(key.clone()); // a device picked from the list must be visible
                }
            }
            if mem.len() >= 3 && std::env::var_os("AIRSCOPE_PIN_BIG").is_some() {
                self.open.insert(key.clone()); // debug aid
            }
            let open = self.open.contains(&key);
            let fan_r = (mem.len() as f32 * 46.0 / TAU).max(58.0);
            groups.push(G { key, members: mem, center0, center: center0, center_t: center0, open, fan_r, halo_r: fan_r + 40.0 });
        }
        groups.sort_by(|a, b| a.key.cmp(&b.key));
        self.open.retain(|key| groups.iter().any(|g| &g.key == key));
        for g in &groups {
            if g.open {
                self.formed.remove(&g.key); // an opened group's icon re-forms after it collapses again
            }
        }
        let mut group_of: Vec<Option<usize>> = vec![None; cnt];
        for (gi, g) in groups.iter().enumerate() {
            for &i in &g.members {
                group_of[i] = Some(gi);
            }
        }

        // --- make room: an open group keeps clear of the PC (which never moves), and everything else
        // that would sit under its circle is pushed out of the way.
        let pc_clear = 48.0;
        let mut shift_t: Vec<Vec2> = vec![Vec2::ZERO; groups.len()];
        let mut open_circles: Vec<(Pos2, f32)> = vec![];
        for (gi, g) in groups.iter().enumerate().filter(|(_, g)| g.open) {
            let v = g.center0 - c;
            let need = g.halo_r + pc_clear;
            if v.length() < need {
                let dir = if v.length() > 1.0 { v / v.length() } else { vec2(0.0, -1.0) };
                shift_t[gi] = c + dir * need - g.center0;
            }
            open_circles.push((g.center0 + shift_t[gi], g.halo_r));
        }
        let push = |q: Pos2| -> Vec2 {
            let mut q2 = q;
            let mut moved = false;
            for _ in 0..2 {
                for (gc, hr) in &open_circles {
                    let v = q2 - *gc;
                    let d = v.length();
                    let lim = hr + 26.0;
                    if d < lim {
                        let dir = if d > 1.0 { v / d } else { vec2(1.0, 0.0) };
                        q2 = *gc + dir * lim;
                        moved = true;
                    }
                }
                if moved {
                    let vc = q2 - c;
                    let dc = vc.length();
                    if dc < pc_clear {
                        q2 = c + if dc > 1.0 { vc / dc * pc_clear } else { vec2(pc_clear, 0.0) };
                    }
                }
            }
            q2 - q
        };
        for (gi, g) in groups.iter().enumerate().filter(|(_, g)| !g.open) {
            shift_t[gi] = push(g.center0);
        }
        let mut push_node = vec![Vec2::ZERO; cnt];
        for i in 0..cnt {
            if group_of[i].is_none() && Some(i) != drag_ni {
                push_node[i] = push(nodes[i].pos);
            }
        }
        for (gi, g) in groups.iter_mut().enumerate() {
            let s = self.gshift.entry(g.key.clone()).or_insert(shift_t[gi]);
            *s += (shift_t[gi] - *s) * k;
            g.center = g.center0 + *s;
            g.center_t = g.center0 + shift_t[gi];
        }
        self.gshift.retain(|key, _| groups.iter().any(|g| &g.key == key));

        // --- targets: free nodes sit at their base (plus push); collapsed members orbit their icon;
        // open members fan out around the (shifted) centre.
        let mut target: Vec<Pos2> = (0..cnt).map(|i| nodes[i].pos + push_node[i]).collect();
        let mut hidden = vec![false; cnt];
        let mut in_open = vec![false; cnt];
        for g in &groups {
            let nn = g.members.len().min(10);
            if g.open {
                let mut ordered = g.members.clone();
                ordered.sort_by(|&a, &b| {
                    let (va, vb) = (nodes[a].pos - g.center0, nodes[b].pos - g.center0);
                    va.y.atan2(va.x).partial_cmp(&vb.y.atan2(vb.x)).unwrap()
                });
                let v0 = nodes[ordered[0]].pos - g.center0;
                let a0 = if v0.length() > 0.5 { v0.y.atan2(v0.x) } else { -1.2 };
                for (kk, &i) in ordered.iter().enumerate() {
                    let a = a0 + kk as f32 * TAU / ordered.len() as f32;
                    target[i] = g.center_t + vec2(a.cos(), a.sin()) * g.fan_r;
                    in_open[i] = true;
                }
            } else {
                for (kk, &i) in g.members.iter().enumerate() {
                    target[i] = slot(g.center_t, kk, nn, t, 1.0);
                    hidden[i] = true;
                }
            }
        }

        // --- smooth positions: each node eases its offset from its base position toward its target.
        // Pan/zoom move the base instantly (no lag); fan-out, collapse, drops and pings glide.
        let mut disp: Vec<Pos2> = vec![Pos2::ZERO; cnt];
        let mut settled = vec![false; cnt];
        let mut fscale = vec![1.0f32; cnt];
        for ni in 0..cnt {
            let id = self.latched[nodes[ni].i].id.clone();
            let tgt = target[ni] - nodes[ni].pos;
            let off = self.offs.entry(id.clone()).or_insert(tgt);
            if Some(ni) == drag_ni {
                *off = Vec2::ZERO; // follows the pointer exactly
            } else {
                *off += (tgt - *off) * k;
            }
            disp[ni] = nodes[ni].pos + *off;
            let remain = (disp[ni] - target[ni]).length();
            settled[ni] = hidden[ni] && remain < 3.0;
            if hidden[ni] {
                fscale[ni] = (remain / 70.0).clamp(0.15, 1.0); // shrink into the orbit dot
            }
            let ap = self.appear.entry(id).or_insert(1.0);
            *ap += (1.0 - *ap) * k;
        }
        for g in &groups {
            let a = self.halo.entry(g.key.clone()).or_insert(0.0);
            *a += ((if g.open { 1.0 } else { 0.0 }) - *a) * k;
        }
        self.halo.retain(|key, _| groups.iter().any(|g| &g.key == key));

        // --- when does a collapsed group show its icon? once its members arrive (and thereafter, so
        // a newcomer flying in does not make the icon vanish)
        let arrived: Vec<bool> = groups.iter().map(|g| !g.open && g.members.iter().all(|&i| settled[i])).collect();
        for (gi, g) in groups.iter().enumerate() {
            if arrived[gi] {
                self.formed.insert(g.key.clone());
            }
        }
        self.formed.retain(|key| groups.iter().any(|g| &g.key == key));
        let iconshown: Vec<bool> = groups.iter().enumerate().map(|(gi, g)| !g.open && (arrived[gi] || self.formed.contains(&g.key))).collect();
        let vis1: Vec<bool> = (0..cnt).map(|i| !in_open[i] && !group_of[i].map_or(false, |gi| iconshown[gi] && settled[i])).collect();
        let closing: Vec<bool> = groups.iter().enumerate().map(|(gi, g)| !g.open && !iconshown[gi] && self.halo.get(&g.key).copied().unwrap_or(0.0) >= 0.01).collect();
        for (gi, g) in groups.iter().enumerate() {
            if !iconshown[gi] {
                self.gscale.insert(g.key.clone(), 0.0); // icon pops in from nothing
            }
        }

        // anything sitting under an open group's circle is obscured and not clickable
        let masked: Vec<bool> = (0..cnt).map(|i| !in_open[i] && groups.iter().filter(|g| g.open).any(|g| (disp[i] - g.center).length() < g.halo_r)).collect();

        // --- which labels fit (connected and strongest first)
        let mut allow: HashSet<usize> = Default::default();
        {
            let mut placed: Vec<Rect> = (0..cnt).filter(|&i| vis1[i]).map(|i| Rect::from_center_size(disp[i], Vec2::splat(30.0))).collect();
            placed.extend(groups.iter().enumerate().filter(|(gi, _)| iconshown[*gi]).map(|(_, g)| Rect::from_center_size(g.center, Vec2::splat(44.0))));
            placed.push(Rect::from_center_size(c, Vec2::splat(120.0)));
            let mut order: Vec<usize> = (0..cnt).filter(|&i| !hidden[i] && !masked[i] && self.latched[nodes[i].i].known && !nodes[i].dim).collect();
            order.sort_by(|&a, &b| {
                let (da, db) = (&self.latched[nodes[a].i], &self.latched[nodes[b].i]);
                db.connected.cmp(&da.connected).then(db.dbm.unwrap_or(-120.0).partial_cmp(&da.dbm.unwrap_or(-120.0)).unwrap())
            });
            for i in order {
                let g = p.layout_no_wrap(self.latched[nodes[i].i].name.clone(), FontId::proportional(11.5), TEXT);
                let r = Rect::from_center_size(disp[i] + vec2(0.0, 30.0), g.size() + vec2(8.0, 4.0));
                if placed.iter().all(|q| !q.intersects(r)) {
                    placed.push(r);
                    allow.insert(i);
                }
            }
        }

        let view = rect.expand(70.0);
        let mut ev = Ev { clicked: None, over: vec![], drag_end: false, hover: None, dragging: None };
        let sel_id = self.sel.clone();

        // --- links from the PC to connected Bluetooth devices, with travelling pulses
        for (ni, n) in nodes.iter().enumerate() {
            let d = &self.latched[n.i];
            if d.kind == Kind::Bt && d.connected && vis1[ni] {
                let col = kind_col(d.kind);
                p.line_segment([c, disp[ni]], Stroke::new(5.0, add(col, 0.06)));
                p.line_segment([c, disp[ni]], Stroke::new(1.5, alpha(col, 0.55)));
                for kk in 0..3 {
                    let u = (t * 0.6 + kk as f32 / 3.0).fract();
                    let q = c + (disp[ni] - c) * u;
                    glow(&p, q, 2.5, col, 0.8);
                    p.circle_filled(q, 2.0, Color32::WHITE);
                }
            }
        }

        // --- layer 1: everything that is not part of an open group
        for (ni, n) in nodes.iter().enumerate() {
            if !vis1[ni] || group_of[ni].map_or(false, |gi| closing[gi]) || !view.contains(disp[ni]) {
                continue;
            }
            let d = &self.latched[n.i];
            self.last_pos.insert(d.id.clone(), disp[ni]);
            let interactive = !hidden[ni] && (!masked[ni] || Some(ni) == drag_ni);
            let sel = sel_id.as_deref() == Some(d.id.as_str());
            let sc = fscale[ni] * self.appear.get(&d.id).copied().unwrap_or(1.0);
            node_widget(ui, &p, d, n, ni, disp[ni], t, sw, sel, allow.contains(&ni), interactive, true, sc, c, big, &mut ev);
        }
        let mut toggle: Vec<(String, bool)> = vec![]; // (key, open?)
        let mut group_hover: Option<usize> = None;
        for (gi, g) in groups.iter().enumerate() {
            if !iconshown[gi] || !view.contains(g.center) {
                continue;
            }
            let kinds: Vec<Kind> = g.members.iter().map(|&i| self.latched[nodes[i].i].kind).collect();
            let col = if kinds.iter().all(|x| *x == kinds[0]) { kind_col(kinds[0]) } else { Color32::from_rgb(190, 212, 240) };
            let masked_icon = groups.iter().filter(|o| o.open).any(|o| (g.center - o.center).length() < o.halo_r);
            let mut hovered = false;
            if !masked_icon {
                let resp = hit(ui, Rect::from_center_size(g.center, Vec2::splat(46.0)), ui.scope_id().with(("gi", &g.key)), Sense::click());
                hovered = resp.hovered();
                if resp.clicked() {
                    toggle.push((g.key.clone(), true));
                }
            }
            let sc = self.gscale.entry(g.key.clone()).or_insert(0.0);
            *sc += (1.0 - *sc) * k;
            let sc = *sc;
            let r = 19.0 * (0.55 + 0.45 * sc);
            orb(&p, g.center, r, col, if hovered { 0.9 } else { 0.25 });
            let nn = g.members.len().min(10);
            for kk in 0..nn {
                if !settled[g.members[kk]] {
                    continue; // still flying in: drawn as a shrinking node instead
                }
                let kc = kind_col(self.latched[nodes[g.members[kk]].i].kind);
                p.circle_filled(slot(g.center, kk, nn, t, 0.55 + 0.45 * sc), 2.6, kc);
            }
            p.text(g.center, Align2::CENTER_CENTER, g.members.len().to_string(), FontId::monospace(15.0), Color32::from_rgb(8, 16, 30));
            if hovered {
                group_hover = Some(gi);
            }
        }

        // --- while dragging: show which group/device the drop would join
        if let Some(di) = drag_ni {
            let q = nodes[di].pos;
            let mut best: Option<(f32, Pos2)> = None;
            for (ni, n) in nodes.iter().enumerate() {
                if ni != di && group_of[ni].is_none() {
                    let d = (n.pos - q).length();
                    if d < GROUP_PX * 1.15 && best.map_or(true, |b| d < b.0) {
                        best = Some((d, disp[ni]));
                    }
                }
            }
            for (gi, g) in groups.iter().enumerate() {
                if iconshown[gi] {
                    let d = (g.center - q).length();
                    if d < GROUP_PX * 1.15 && best.map_or(true, |b| d < b.0) {
                        best = Some((d, g.center));
                    }
                }
            }
            if let Some((_, tp)) = best {
                let pulse = (t * 6.0).sin() * 0.5 + 0.5;
                p.circle_stroke(tp, 28.0 + pulse * 4.0, Stroke::new(2.0, add(CYAN, 0.55)));
                p.circle_stroke(tp, 34.0 + pulse * 6.0, Stroke::new(1.0, add(CYAN, 0.2)));
            }
        }

        // --- this PC (never moves)
        let pc_r = 25.0;
        let pulse = (t * 1.3).sin() * 0.5 + 0.5;
        p.circle_stroke(c, pc_r + 8.0 + pulse * 8.0, Stroke::new(1.5, add(CYAN, 0.3 * (1.0 - pulse))));
        orb(&p, c, pc_r, Color32::from_rgb(120, 220, 255), 0.35);
        icon(&p, Icon::Computer, c, pc_r * 0.46, Color32::from_rgb(8, 24, 40));
        lcd(&p, c + vec2(0.0, pc_r + 16.0), Align2::CENTER_CENTER, &self.host.to_uppercase(), 12.0, CYAN);

        // --- layer 2 (on top of everything): open groups, each inside a soft, obscuring circle
        for (gi, g) in groups.iter().enumerate() {
            let amt = self.halo.get(&g.key).copied().unwrap_or(0.0);
            if amt < 0.01 || !view.expand(g.halo_r).contains(g.center) {
                continue;
            }
            soft_halo(&p, g.center, g.halo_r, amt);
            if !g.open {
                if closing[gi] {
                    // reverse of the fan-out: members glide back toward the icon, circle fading
                    for &i in &g.members {
                        p.line_segment([g.center, disp[i]], Stroke::new(1.2, alpha(CYAN, 0.5 * amt)));
                    }
                    for &i in &g.members {
                        let d = &self.latched[nodes[i].i];
                        let sel = sel_id.as_deref() == Some(d.id.as_str());
                        node_widget(ui, &p, d, &nodes[i], i, disp[i], t, sw, sel, false, false, false, fscale[i], c, big, &mut ev);
                    }
                }
                continue;
            }
            for &i in &g.members {
                p.line_segment([g.center, disp[i]], Stroke::new(4.0, add(CYAN, 0.05)));
                p.line_segment([g.center, disp[i]], Stroke::new(1.2, alpha(CYAN, 0.5)));
            }
            for &i in &g.members {
                let d = &self.latched[nodes[i].i];
                let sel = sel_id.as_deref() == Some(d.id.as_str());
                self.last_pos.insert(d.id.clone(), disp[i]);
                node_widget(ui, &p, d, &nodes[i], i, disp[i], t, sw, sel, allow.contains(&i), true, true, 1.0, c, big, &mut ev);
            }
            let kinds: Vec<Kind> = g.members.iter().map(|&i| self.latched[nodes[i].i].kind).collect();
            let col = if kinds.iter().all(|x| *x == kinds[0]) { kind_col(kinds[0]) } else { Color32::from_rgb(190, 212, 240) };
            let resp = hit(ui, Rect::from_center_size(g.center, Vec2::splat(40.0)), ui.scope_id().with(("gc", &g.key)), Sense::click());
            orb(&p, g.center, 11.0, col, if resp.hovered() { 0.9 } else { 0.2 });
            p.line_segment([g.center - vec2(5.0, 0.0), g.center + vec2(5.0, 0.0)], Stroke::new(2.2, shade(col, 0.15)));
            if resp.clicked() {
                toggle.push((g.key.clone(), false));
            }
        }

        // --- bursts (a device left the air)
        self.bursts.retain(|b| t - b.t0 < 0.6);
        for b in &self.bursts {
            let age = (t - b.t0) / 0.6;
            p.circle_stroke(b.pos, 8.0 + age * 46.0, Stroke::new(2.0, add(b.col, 0.6 * (1.0 - age))));
        }

        // --- hover pills (always on top)
        if let Some(ni) = ev.hover {
            let d = &self.latched[nodes[ni].i];
            let sub = match d.dbm {
                Some(x) => format!("{} · {:.0} dBm", d.vendor, x),
                None => d.vendor.clone(),
            };
            pill(&p, disp[ni] + vec2(0.0, -24.0), &d.name, &sub, kind_col(d.kind));
        } else if let Some(gi) = group_hover {
            let g = &groups[gi];
            let names: Vec<String> = g.members.iter().take(4).map(|&i| self.latched[nodes[i].i].name.clone()).collect();
            let more = if g.members.len() > 4 { format!(" +{}", g.members.len() - 4) } else { String::new() };
            pill(&p, g.center + vec2(0.0, -34.0), &format!("{} devices close together", g.members.len()), &format!("{}{} · click to expand", names.join(", "), more), CYAN);
        }

        // --- click outside an open group's circle closes it
        if ui.input(|i| i.pointer.primary_clicked()) {
            if let Some(ptr) = ui.input(|i| i.pointer.interact_pos()) {
                if rect.contains(ptr) && ui.ctx().layer_id_at(ptr) == Some(ui.layer_id()) {
                    for g in groups.iter().filter(|g| g.open) {
                        if (ptr - g.center).length() > g.halo_r {
                            toggle.push((g.key.clone(), false));
                        }
                    }
                }
            }
        }

        // --- apply events
        for (id, v) in ev.over {
            self.over.insert(id, v);
        }
        if let Some(id) = ev.dragging {
            self.dragging = Some(id);
        }
        if ev.drag_end {
            self.dragging = None;
            self.save_over();
        }
        for (key, open) in toggle {
            if open {
                self.open.insert(key);
            } else {
                self.open.remove(&key);
            }
        }
        if let Some(id) = ev.clicked {
            self.sel = if self.sel.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        }
        if let Some(id) = self.sel.clone() {
            if let Some(ni) = nodes.iter().enumerate().find(|(_, n)| self.latched[n.i].id == id).map(|(ni, _)| ni) {
                if !settled[ni] {
                    let d = self.latched[nodes[ni].i].clone();
                    let mut close = false;
                    popup(ui.ctx(), &d, disp[ni], rect, &mut close, kind_col(d.kind));
                    if close {
                        self.sel = None;
                    }
                }
            }
        }

        // --- zoom HUD
        let hud = Rect::from_min_size(pos2(rect.right() - 318.0, rect.bottom() - 48.0), vec2(300.0, 34.0));
        panel(&p, hud, 17.0, CYAN);
        lcd(&p, pos2(hud.left() + 18.0, hud.center().y), Align2::LEFT_CENTER, &format!("ZOOM {:.1}x", self.zoom), 12.0, CYAN);
        p.text(pos2(hud.left() + 116.0, hud.center().y), Align2::LEFT_CENTER, "scroll · drag · double-click", FontId::proportional(11.0), DIM);
        let rb = Rect::from_center_size(pos2(hud.right() - 28.0, hud.center().y), vec2(42.0, 22.0));
        let rr = hit(ui, rb, ui.scope_id().with("reset_view"), Sense::click());
        p.rect_filled(rb, 9.0, alpha(CYAN, if rr.hovered() { 0.35 } else { 0.15 }));
        p.text(rb.center(), Align2::CENTER_CENTER, "1:1", FontId::monospace(11.0), Color32::WHITE);
        if rr.clicked() {
            self.reset_view = true;
        }
    }

    // ---------- shared Wi-Fi / Bluetooth helpers ----------

    fn is_mine(&self, d: &Dev) -> bool {
        match d.kind {
            Kind::Wifi => {
                // an AP radio of a device on our LAN (e.g. the router): its BSSID is derived from the device's MAC
                let b = mac_bytes(&d.id);
                (b.is_some() && self.devs.iter().filter(|x| x.kind == Kind::Lan).filter_map(|x| mac_bytes(&x.id)).any(|m| same_device(b.unwrap(), m)))
                    || (d.known && self.mine.iter().any(|m| d.name.starts_with(m.as_str())))
            }
            _ => true,
        }
    }

    /// 2.4 GHz networks (one per name+channel; unnamed access points are lumped per channel).
    fn nets(&self) -> Vec<Net> {
        let mut m: HashMap<(String, u32), Net> = HashMap::new();
        for d in self.devs.iter().filter(|d| d.kind == Kind::Wifi) {
            let (Some(f), Some(dbm), Some(ch)) = (d.freq, d.dbm, d.chan) else { continue };
            if f >= 3000 {
                continue;
            }
            let hidden = !d.known;
            let name = if hidden { "(unnamed)".to_string() } else { d.name.clone() };
            let mine = self.is_mine(d);
            let e = m.entry((name.clone(), ch)).or_insert(Net { name, chan: ch, f: f as f32, dbm, aps: 0, mine, hidden });
            e.aps += 1;
            e.mine |= mine;
            e.dbm = e.dbm.max(dbm);
        }
        let mut v: Vec<Net> = m.into_values().collect();
        v.sort_by(|a, b| b.dbm.partial_cmp(&a.dbm).unwrap().then(a.name.cmp(&b.name)));
        v
    }

    /// Bluetooth channel i (0..78) sits at 2402+i MHz. How crowded is Wi-Fi there? (0 = quiet)
    fn load_at(nets: &[Net], f: f32) -> f32 {
        nets.iter().map(|n| n.strength() * hill((f - n.f).abs())).sum()
    }

    /// `crowd(i)` = estimated Wi-Fi crowding (0..1) on Bluetooth channel i: busier channels are drawn dimmer.
    fn bt_rows(&self, p: &egui::Painter, label_x: f32, xl: &dyn Fn(usize) -> f32, cw: f32, end_x: f32, mut y: f32, crowd: &dyn Fn(usize) -> f32) -> f32 {
        for l in &self.links {
            p.text(pos2(label_x, y + 9.0), Align2::RIGHT_CENTER, if l.name.is_empty() { &l.mac } else { &l.name }, FontId::proportional(12.0), TEXT);
            for i in 0..79usize {
                let r = Rect::from_min_size(pos2(xl(i), y), vec2((cw - 1.5).max(1.0), 18.0));
                if l.afh.as_ref().map(|m| m[i]).unwrap_or(true) {
                    let q = 0.5 + 0.5 * (1.0 - crowd(i)); // brightness floor: a used channel never looks "off"
                    glow_rect(p, r, 5.0, GREEN, 0.22 * q * q);
                    p.rect_filled(r, 2.0, mix(Color32::from_rgb(24, 70, 48), GREEN, (q - 0.5) * 2.0 * 0.85 + 0.15));
                    p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 5.0)), 2.0, add(Color32::WHITE, 0.22 * q));
                } else {
                    p.rect_stroke(r, 2.0, Stroke::new(1.0, alpha(DIM, 0.45)), StrokeKind::Inside);
                }
            }
            let txt = match &l.afh {
                Some(m) => format!("{}/79 in use", m.iter().filter(|x| **x).count()),
                None => "no AFH · hops on all".to_string(),
            };
            p.text(pos2(end_x + 6.0, y + 9.0), Align2::LEFT_CENTER, txt, FontId::monospace(10.0), DIM);
            y += 26.0;
        }
        y
    }

    // ---------- spectrum ----------

    fn spectrum(&mut self, ui: &mut egui::Ui, _t: f32) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, BG);
        let nets = self.nets();
        let mut names: Vec<&str> = nets.iter().filter(|n| !n.hidden).map(|n| n.name.as_str()).collect();
        names.sort();
        names.dedup();
        let col_of = |n: &Net| -> Color32 {
            if n.hidden {
                return Color32::from_gray(140);
            }
            const PAL: [(u8, u8, u8); 8] = [(90, 220, 255), (255, 140, 90), (200, 120, 255), (120, 255, 160), (255, 220, 90), (255, 110, 170), (110, 150, 255), (170, 230, 80)];
            let i = names.iter().position(|x| *x == n.name).unwrap_or(0) % PAL.len();
            Color32::from_rgb(PAL[i].0, PAL[i].1, PAL[i].2)
        };
        let l = rect.left() + 24.0;
        p.text(pos2(l, rect.top() + 26.0), Align2::LEFT_CENTER, "2.4 GHz: Wi-Fi networks as hills on the frequencies they use, with Bluetooth's channels directly underneath", FontId::proportional(12.0), DIM);

        let legend_w = 300.0;
        let n_links = self.links.len().max(1) as f32;
        let has_bt = !self.links.is_empty();
        let bt_h = if has_bt { n_links * 26.0 + 104.0 } else { 44.0 };
        let gh = (rect.height() - 60.0 - 40.0 - bt_h - 200.0).clamp(150.0, 330.0);
        let gx0 = l + 130.0;
        let gx1 = rect.right() - legend_w - 170.0;
        let gw = gx1 - gx0;
        let base = rect.top() + 60.0 + gh;
        let fx = |f: f32| gx0 + (f - 2399.0) / 87.0 * gw;
        let frame = Rect::from_min_max(pos2(l, rect.top() + 52.0), pos2(gx1 + 134.0, base + 30.0 + bt_h));
        panel(&p, frame, 14.0, CYAN);
        // gridlines + axes
        for (dbm, lbl) in [(-85.0, "-85"), (-70.0, "-70"), (-55.0, "-55")] {
            let y = base - ((dbm + 95.0) / 55.0) * (gh - 20.0);
            p.line_segment([pos2(gx0, y), pos2(gx1, y)], Stroke::new(1.0, alpha(CYAN, 0.07)));
            p.text(pos2(gx0 - 6.0, y), Align2::RIGHT_CENTER, lbl, FontId::monospace(9.5), DIM);
        }
        p.text(pos2(l + 8.0, rect.top() + 66.0), Align2::LEFT_TOP, "dBm", FontId::monospace(9.5), DIM);
        for n in 1..=13 {
            let f = 2407.0 + 5.0 * n as f32;
            p.line_segment([pos2(fx(f), rect.top() + 62.0), pos2(fx(f), base)], Stroke::new(1.0, alpha(CYAN, if [1, 6, 11].contains(&n) { 0.16 } else { 0.06 })));
            p.text(pos2(fx(f), base + 9.0), Align2::CENTER_CENTER, n.to_string(), FontId::monospace(11.0), if [1, 6, 11].contains(&n) { CYAN } else { DIM });
        }
        p.text(pos2(gx0 - 6.0, base + 9.0), Align2::RIGHT_CENTER, "Wi-Fi ch", FontId::proportional(10.5), DIM);
        p.line_segment([pos2(gx0, base), pos2(gx1, base)], Stroke::new(1.5, alpha(CYAN, 0.35)));

        // hover target: from the legend, else from the graph
        let ptr = ui.input(|i| i.pointer.hover_pos());
        let hh = |n: &Net, dx: f32| n.strength() * (gh - 20.0) * hill(dx);
        let mut hover: Option<usize> = None;
        let lx = gx1 + 154.0;
        let mut ly = rect.top() + 62.0;
        let mut legend: Vec<(Rect, usize)> = vec![];
        let max_rows = (((base + 30.0 - ly) / 24.0) as usize).saturating_sub(2).max(4);
        for (i, _) in nets.iter().enumerate().take(max_rows) {
            legend.push((Rect::from_min_size(pos2(lx, ly), vec2(rect.right() - lx - 20.0, 22.0)), i));
            ly += 24.0;
        }
        for (r, i) in &legend {
            if hit(ui, *r, ui.scope_id().with(("lg", *i)), Sense::hover()).hovered() {
                hover = Some(*i);
            }
        }
        if hover.is_none() {
            if let Some(q) = ptr.filter(|q| q.x > gx0 && q.x < gx1 && q.y < base && q.y > rect.top() + 62.0) {
                let f = 2399.0 + (q.x - gx0) / gw * 87.0;
                for (i, n) in nets.iter().enumerate() {
                    if base - q.y <= hh(n, f - n.f) && hover.map_or(true, |_| false) {
                        hover = Some(i); // strongest first
                    }
                }
            }
        }
        // hills, weakest first so strong ones sit on top
        for (i, n) in nets.iter().enumerate().rev() {
            let col = col_of(n);
            let dim = hover.map_or(1.0, |h| if h == i { 1.0 } else { 0.28 });
            let pts: Vec<Pos2> = (-22..=22).map(|k| {
                let dx = k as f32 / 2.0;
                pos2(fx(n.f + dx), base - hh(n, dx))
            }).collect();
            let mut m = egui::Mesh::default();
            for q in &pts {
                m.colored_vertex(*q, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), (95.0 * dim) as u8));
                m.colored_vertex(pos2(q.x, base), Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), (14.0 * dim) as u8));
            }
            for k in 0..pts.len() as u32 - 1 {
                let a = k * 2;
                m.add_triangle(a, a + 1, a + 2);
                m.add_triangle(a + 1, a + 3, a + 2);
            }
            p.add(egui::Shape::mesh(m));
            let w = if n.mine { 2.6 } else { 1.6 };
            p.add(egui::Shape::line(pts, Stroke::new(w, alpha(col, 0.95 * dim))));
        }
        // labels above the peaks (strongest first, shifted up when they would collide)
        let mut placed: Vec<Rect> = vec![];
        for (i, n) in nets.iter().enumerate().filter(|(_, n)| !n.hidden) {
            let g = p.layout_no_wrap(if n.mine { format!("{} · yours", n.name) } else { n.name.clone() }, FontId::proportional(11.5), Color32::WHITE);
            let mut y = base - hh(n, 0.0) - 12.0;
            let mut r = Rect::from_center_size(pos2(fx(n.f), y), g.size() + vec2(10.0, 4.0));
            let mut tries = 0;
            while placed.iter().any(|q| q.intersects(r)) && tries < 6 {
                y -= 15.0;
                r = Rect::from_center_size(pos2(fx(n.f), y), g.size() + vec2(10.0, 4.0));
                tries += 1;
            }
            if tries >= 6 && hover != Some(i) || r.top() < rect.top() + 56.0 {
                continue;
            }
            placed.push(r);
            let col = col_of(n);
            p.rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(6, 12, 26, 190));
            p.rect_stroke(r, 6.0, Stroke::new(1.0, alpha(col, if hover.map_or(true, |h| h == i) { 0.9 } else { 0.3 })), StrokeKind::Inside);
            p.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
        }
        // Bluetooth channels aligned under the same frequency axis
        let bcw = gw / 87.0;
        let crowd = |i: usize| (Self::load_at(&nets, 2402.0 + i as f32) / 2.2).clamp(0.0, 1.0);
        let mut yc = base + 26.0;
        if has_bt {
            // Wi-Fi crowding on each Bluetooth channel: how much Wi-Fi is transmitting on that frequency
            p.text(pos2(gx0 - 12.0, yc + 9.0), Align2::RIGHT_CENTER, "Wi-Fi crowding", FontId::proportional(12.0), AMBER);
            for i in 0..79usize {
                let v = crowd(i);
                let heat = mix(AMBER, RED, ((v - 0.4) * 1.7).clamp(0.0, 1.0));
                let r = Rect::from_min_size(pos2(fx(2401.5 + i as f32), yc), vec2((bcw - 1.5).max(1.0), 18.0));
                if v > 0.12 {
                    glow_rect(&p, r, 5.0, heat, 0.22 * v);
                }
                p.rect_filled(r, 2.0, mix(shade(BG, 1.8), heat, 0.12 + 0.88 * v));
                if v > 0.12 {
                    p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 5.0)), 2.0, add(Color32::WHITE, 0.12 * v));
                }
            }
            // key: quiet -> crowded
            let kr = Rect::from_min_size(pos2(gx1 + 10.0, yc + 3.0), vec2(64.0, 12.0));
            let mut km = egui::Mesh::default();
            let (c0, c1) = (shade(BG, 1.8), RED);
            for (x, c) in [(kr.left(), c0), (kr.center().x, mix(AMBER, c0, 0.2)), (kr.right(), c1)] {
                km.colored_vertex(pos2(x, kr.top()), c);
                km.colored_vertex(pos2(x, kr.bottom()), c);
            }
            km.add_triangle(0, 1, 2);
            km.add_triangle(1, 3, 2);
            km.add_triangle(2, 3, 4);
            km.add_triangle(3, 5, 4);
            p.add(egui::Shape::mesh(km));
            p.text(pos2(kr.left(), kr.bottom() + 9.0), Align2::LEFT_CENTER, "quiet", FontId::proportional(9.5), DIM);
            p.text(pos2(kr.right(), kr.bottom() + 9.0), Align2::RIGHT_CENTER, "crowded", FontId::proportional(9.5), DIM);
            yc += 34.0;
        }
        let title = if has_bt { format!("CONNECTED BLUETOOTH DEVICES: {}", self.links.len()) } else { "NO BLUETOOTH DEVICES".to_string() };
        let mid = (gx0 + gx1) / 2.0;
        lcd(&p, pos2(mid, yc + 8.0), Align2::CENTER_CENTER, &title, 12.0, BLUE);
        if has_bt {
            let yend = self.bt_rows(&p, gx0 - 12.0, &|i| fx(2401.5 + i as f32), bcw, gx1, yc + 26.0, &crowd);
            p.text(pos2(mid, yend + 6.0), Align2::CENTER_CENTER, "brighter channel = less Wi-Fi expected on it (estimate) · dark outline = avoided", FontId::proportional(10.5), DIM);
        }

        // legend
        lcd(&p, pos2(lx, rect.top() + 36.0 + 12.0), Align2::LEFT_CENTER, "NETWORKS", 12.0, CYAN);
        for (r, i) in &legend {
            let n = &nets[*i];
            let col = col_of(n);
            if hover == Some(*i) {
                p.rect_filled(*r, 6.0, alpha(col, 0.16));
            }
            p.circle_filled(pos2(r.left() + 10.0, r.center().y), 5.5, col);
            let nm = if n.mine { format!("{} · yours", n.name) } else if n.aps > 1 && n.hidden { format!("{} ×{}", n.name, n.aps) } else { n.name.clone() };
            p.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, nm, FontId::proportional(12.0), if n.hidden { DIM } else { TEXT });
            p.text(pos2(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, format!("ch{} {:.0}", n.chan, n.dbm), FontId::monospace(10.5), DIM);
        }
        if nets.len() > legend.len() {
            p.text(pos2(lx, ly + 8.0), Align2::LEFT_CENTER, format!("+ {} weaker networks (hover the graph)", nets.len() - legend.len()), FontId::proportional(11.0), DIM);
        }
        let n5 = self.devs.iter().filter(|d| d.kind == Kind::Wifi && d.freq.map_or(false, |f| f >= 3000)).count();
        p.text(pos2(lx, base + 14.0), Align2::LEFT_CENTER, format!("{} access points on 5 GHz not shown", n5), FontId::proportional(10.5), DIM);
        p.text(pos2(lx, base + 28.0), Align2::LEFT_CENTER, "(5 GHz doesn't affect Bluetooth)", FontId::proportional(10.5), DIM);

        // hover tooltip
        if let (Some(h), Some(q)) = (hover, ptr) {
            let n = &nets[h];
            let ov: Vec<&Net> = nets.iter().enumerate().filter(|(j, o)| *j != h && (o.f - n.f).abs() < 22.0 && o.dbm > -85.0).map(|(_, o)| o).collect();
            let same = ov.iter().filter(|o| (o.f - n.f).abs() < 1.0).count();
            let sub = format!("ch {} · {:.0} dBm · {} on same channel, {} partly overlapping", n.chan, n.dbm, same, ov.len() - same);
            pill(&p, pos2(q.x.clamp(rect.left() + 200.0, rect.right() - 200.0), q.y - 14.0), &format!("{}{}", n.name, if n.aps > 1 { format!(" ({} access points)", n.aps) } else { String::new() }), &sub, col_of(n));
        }

        // explanation cards
        let cy = frame.bottom() + 12.0;
        let ch = (rect.bottom() - cy - 12.0).clamp(60.0, 190.0);
        let cwid = (rect.width() - 48.0 - 24.0) / 3.0;
        let card = |i: usize, title: &str, text: &str, col: Color32| {
            let r = Rect::from_min_size(pos2(l + i as f32 * (cwid + 12.0), cy), vec2(cwid, ch));
            panel(&p, r, 12.0, col);
            lcd(&p, pos2(r.left() + 14.0, r.top() + 16.0), Align2::LEFT_CENTER, title, 11.5, col);
            let g = p.layout(text.to_string(), FontId::proportional(12.0), TEXT, r.width() - 28.0);
            p.galley(pos2(r.left() + 14.0, r.top() + 32.0), g, TEXT);
        };
        card(0, "HOW TO READ IT", "Each hill is one Wi-Fi network. A hill is as wide as the radio frequencies it occupies (about 22 MHz, five channels' worth) and as tall as its signal is strong at this PC. Grey hills are access points that don't broadcast a name.", CYAN);
        card(1, "WHAT OVERLAP MEANS", "Where hills overlap, those networks share the same airwaves. Same channel: they politely take turns, so each gets less speed. Partly overlapping channels are worse: they can't coordinate and just garble each other. Only channels 1, 6 and 11 never overlap. Bluetooth hops across all of it, so channels under a tall, crowded hill are the ones that get disturbed.", AMBER);
        let mine_txt = match nets.iter().find(|n| n.mine && !n.name.contains("Guest")).or_else(|| nets.iter().find(|n| n.mine)) {
            None => "None of your own networks are visible on 2.4 GHz.".to_string(),
            Some(m) => {
                let others: Vec<&Net> = nets.iter().filter(|o| !std::ptr::eq(*o, m) && !o.mine && o.dbm > -85.0 && (o.f - m.f).abs() < 22.0).collect();
                let same: Vec<&str> = others.iter().filter(|o| (o.f - m.f).abs() < 1.0).map(|o| o.name.as_str()).collect();
                let part: Vec<&str> = others.iter().filter(|o| (o.f - m.f).abs() >= 1.0).map(|o| o.name.as_str()).collect();
                let q = [(1, 2412.0), (6, 2437.0), (11, 2462.0)].into_iter().min_by(|a, b| Self::load_at(&nets, a.1).partial_cmp(&Self::load_at(&nets, b.1)).unwrap()).unwrap().0;
                let list = |v: &[&str]| v.iter().take(4).copied().collect::<Vec<_>>().join(", ");
                format!("{} is on channel {}. It shares that channel with {} other network{}{} and partly overlaps {} more{}. The quietest of the clean channels (1, 6, 11) right now is channel {}.", m.name, m.chan, same.len(), if same.len() == 1 { "" } else { "s" }, if same.is_empty() { String::new() } else { format!(" ({})", list(&same)) }, part.len(), if part.is_empty() { String::new() } else { format!(" ({})", list(&part)) }, q)
            }
        };
        card(2, "YOUR NETWORK", &mine_txt, GREEN);
    }

    // ---------- devices ----------

    fn devices(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        // --- toolbar: search + filter chips
        let (tb, _) = ui.allocate_exact_size(vec2(rect.width(), 74.0), Sense::hover());
        let p = ui.painter().clone();
        let search = Rect::from_min_size(tb.min + vec2(18.0, 10.0), vec2(300.0, 28.0));
        ui.put(search, egui::TextEdit::singleline(&mut self.q).hint_text("Search name, vendor, MAC…"));
        let mut x = search.right() + 18.0;
        let chip = |ui: &egui::Ui, x: &mut f32, y: f32, label: &str, on: bool, col: Color32| -> bool {
            let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), Color32::WHITE);
            let r = Rect::from_min_size(pos2(*x, y), vec2(g.size().x + 26.0, 28.0));
            *x = r.right() + 8.0;
            let resp = hit(ui, r, ui.scope_id().with(("chip", label)), Sense::click());
            let pp = ui.painter();
            pp.rect_filled(r, 14.0, if on { alpha(col, if resp.hovered() { 0.42 } else { 0.30 }) } else { alpha(Color32::WHITE, if resp.hovered() { 0.10 } else { 0.04 }) });
            pp.rect_stroke(r, 14.0, Stroke::new(1.2, if on { alpha(col, 0.9) } else { alpha(Color32::WHITE, 0.18) }), StrokeKind::Inside);
            pp.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.5), if on { Color32::WHITE } else { DIM });
            resp.clicked()
        };
        let y1 = tb.top() + 10.0;
        for (k, (lbl, col)) in [("Wi-Fi", AMBER), ("Bluetooth", BLUE), ("Network", GREEN)].into_iter().enumerate() {
            if chip(ui, &mut x, y1, lbl, self.show[k], col) {
                self.show[k] = !self.show[k];
            }
        }
        x += 14.0;
        for (k, lbl) in ["All", "Mine", "Others"].into_iter().enumerate() {
            if chip(ui, &mut x, y1, lbl, self.scope == k as u8, CYAN) {
                self.scope = k as u8;
            }
        }
        x += 14.0;
        if chip(ui, &mut x, y1, "Hide unnamed", self.hide_unnamed, MAGENTA) {
            self.hide_unnamed = !self.hide_unnamed;
        }
        // --- filter + group
        let q = self.q.to_lowercase();
        let groups: [(&str, &str, Color32); 5] = [
            ("YOURS", "Bluetooth devices", BLUE),
            ("YOURS", "Network devices (LAN)", GREEN),
            ("YOURS", "Your Wi-Fi", AMBER),
            ("NEARBY · not yours", "Neighbouring Wi-Fi networks", AMBER),
            ("NEARBY · not yours", "Unnamed access points", DIM),
        ];
        let mut items: Vec<Vec<usize>> = vec![vec![]; 5];
        let mut total = 0;
        for (i, d) in self.devs.iter().enumerate() {
            let mine = self.is_mine(d);
            let ok = self.show[d.kind as usize]
                && match self.scope {
                    1 => mine,
                    2 => !mine,
                    _ => true,
                }
                && !(self.hide_unnamed && !d.known)
                && (q.is_empty() || d.name.to_lowercase().contains(&q) || d.vendor.to_lowercase().contains(&q) || d.id.to_lowercase().contains(&q) || d.details.iter().any(|(_, v)| v.to_lowercase().contains(&q)));
            if !ok {
                continue;
            }
            total += 1;
            let g = match d.kind {
                Kind::Bt => 0,
                Kind::Lan => 1,
                Kind::Wifi if mine => 2,
                Kind::Wifi if d.known => 3,
                _ => 4,
            };
            items[g].push(i);
        }
        for v in items.iter_mut() {
            v.sort_by_key(|&i| (!self.devs[i].connected as u8, std::cmp::Reverse(self.devs[i].dbm.map(|x| x as i32).unwrap_or(-200)), self.devs[i].name.clone()));
        }
        p.text(pos2(tb.right() - 24.0, y1 + 14.0), Align2::RIGHT_CENTER, format!("Showing {} of {} devices", total, self.devs.len()), FontId::proportional(12.0), DIM);
        // --- list
        let mut pick = None;
        let mut toggle = None;
        let collapsed = self.collapsed.clone();
        let devs = &self.devs;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width() - 34.0;
            let mut last_section = "";
            for (g, (section, title, col)) in groups.iter().enumerate() {
                if items[g].is_empty() {
                    continue;
                }
                if *section != last_section {
                    last_section = section;
                    ui.add_space(10.0);
                    let (r, _) = ui.allocate_exact_size(vec2(w + 14.0, 24.0), Sense::hover());
                    lcd(ui.painter(), pos2(r.left() + 18.0, r.center().y), Align2::LEFT_CENTER, section, 14.0, CYAN);
                    ui.painter().line_segment([pos2(r.left() + 20.0 + section.len() as f32 * 10.0, r.center().y), pos2(r.right(), r.center().y)], Stroke::new(1.0, alpha(CYAN, 0.2)));
                }
                let open = !collapsed.contains(&g);
                let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let r = r.translate(vec2(14.0, 0.0));
                let pp = ui.painter();
                pp.rect_filled(r, 8.0, alpha(*col, if resp.hovered() { 0.16 } else { 0.08 }));
                {
                    let c0 = pos2(r.left() + 18.0, r.center().y);
                    let pts = if open { vec![c0 + vec2(-5.0, -3.0), c0 + vec2(5.0, -3.0), c0 + vec2(0.0, 4.0)] } else { vec![c0 + vec2(-3.0, -5.0), c0 + vec2(-3.0, 5.0), c0 + vec2(4.0, 0.0)] };
                    pp.add(egui::Shape::convex_polygon(pts, *col, Stroke::NONE));
                }
                pp.text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, *title, FontId::proportional(13.5), Color32::WHITE);
                pp.text(pos2(r.right() - 14.0, r.center().y), Align2::RIGHT_CENTER, items[g].len().to_string(), FontId::monospace(12.0), *col);
                if resp.clicked() {
                    toggle = Some(g);
                }
                ui.add_space(6.0);
                if !open {
                    continue;
                }
                for &i in &items[g] {
                    let d = &devs[i];
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let r = r.translate(vec2(14.0, 0.0));
                    let col = kind_col(d.kind);
                    let p = ui.painter();
                    panel(p, r, 12.0, if resp.hovered() { Color32::WHITE } else { col });
                    if d.known {
                        orb(p, pos2(r.left() + 34.0, r.center().y), 15.0, col, 0.0);
                        icon(p, d.icon, pos2(r.left() + 34.0, r.center().y), 7.5, shade(col, 0.12));
                    } else {
                        dot(p, pos2(r.left() + 34.0, r.center().y), 6.0, col, 0.0);
                    }
                    p.text(pos2(r.left() + 66.0, r.center().y - 9.0), Align2::LEFT_CENTER, &d.name, FontId::proportional(15.0), Color32::WHITE);
                    let kindname = d.details.first().map(|x| x.1.clone()).unwrap_or_default();
                    p.text(pos2(r.left() + 66.0, r.center().y + 10.0), Align2::LEFT_CENTER, format!("{} · {}", kindname, d.vendor), FontId::proportional(12.0), DIM);
                    if let Some(dbm) = d.dbm {
                        led_bar(p, Rect::from_min_size(pos2(r.right() - 190.0, r.center().y - 5.0), vec2(120.0, 10.0)), ((dbm + 100.0) / 55.0).clamp(0.0, 1.0), col, 12);
                        p.text(pos2(r.right() - 20.0, r.center().y), Align2::RIGHT_CENTER, format!("{:.0} dBm", dbm), FontId::monospace(12.0), TEXT);
                    } else if d.connected {
                        p.text(pos2(r.right() - 20.0, r.center().y), Align2::RIGHT_CENTER, "CONNECTED", FontId::monospace(12.0), GREEN);
                    }
                    ui.add_space(8.0);
                    if resp.clicked() {
                        pick = Some(d.id.clone());
                    }
                }
            }
            if total == 0 {
                ui.add_space(30.0);
                ui.centered_and_justified(|ui| ui.label("No devices match these filters."));
            }
        });
        if let Some(g) = toggle {
            if !self.collapsed.remove(&g) {
                self.collapsed.insert(g);
            }
        }
        if let Some(id) = pick {
            self.reveal = Some(id.clone());
            self.sel = Some(id);
            self.screen = Screen::Radar;
        }
    }
}

struct Net {
    name: String,
    chan: u32,
    f: f32, // centre MHz
    dbm: f32,
    aps: usize,
    mine: bool,
    hidden: bool,
}
impl Net {
    fn strength(&self) -> f32 {
        ((self.dbm + 95.0) / 55.0).clamp(0.05, 1.0)
    }
}

/// Footprint of a 20 MHz Wi-Fi channel: 1 at the centre, 0 at ±11 MHz.
fn mac_bytes(s: &str) -> Option<[u8; 6]> {
    let v: Vec<u8> = s.split(':').filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
    v.try_into().ok()
}

/// Do two MACs look like radios of one device? Same vendor prefix (ignoring the locally-administered bit)
/// and the device-specific tail equal or a few steps apart (routers number their radios consecutively
/// or flip a high nibble, e.g. B0:82:E2:B4:3A:56 / B2:82:E2:94:3A:56).
fn same_device(a: [u8; 6], b: [u8; 6]) -> bool {
    if (a[0] & !2, a[1], a[2]) != (b[0] & !2, b[1], b[2]) {
        return false;
    }
    let tail = |m: [u8; 6]| ((m[3] as i32) << 16) | ((m[4] as i32) << 8) | m[5] as i32;
    let low = |m: [u8; 6]| ((m[3] & 0x0f) as i32) << 16 | ((m[4] as i32) << 8) | m[5] as i32;
    (tail(a) - tail(b)).abs() <= 8 || low(a) == low(b)
}

fn hill(dx: f32) -> f32 {
    if dx.abs() >= 11.0 {
        0.0
    } else {
        (1.0 - (dx / 11.0).powi(2)).powf(0.7)
    }
}

fn popup(ctx: &egui::Context, d: &Dev, anchor: Pos2, bounds: Rect, close: &mut bool, col: Color32) {
    let w = 380.0;
    let lp = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("popup_measure")));
    let kw = 118.0;
    let rows: Vec<_> = d
        .details
        .iter()
        .map(|(k, v)| (lp.layout_no_wrap(k.to_uppercase(), FontId::monospace(10.5), alpha(CYAN, 0.8)), lp.layout(v.clone(), FontId::proportional(13.0), TEXT, w - 44.0 - kw)))
        .collect();
    let why = lp.layout(d.why.clone(), FontId::proportional(12.5), TEXT, w - 44.0);
    let rows_h: f32 = rows.iter().map(|(_, g)| g.size().y.max(14.0) + 7.0).sum();
    let h = 92.0 + rows_h + 30.0 + why.size().y + 34.0;
    let mut x = anchor.x + 40.0;
    if x + w > bounds.right() - 10.0 {
        x = anchor.x - 40.0 - w;
    }
    let y = (anchor.y - 50.0).clamp(bounds.top() + 10.0, (bounds.bottom() - h - 10.0).max(bounds.top() + 10.0));
    egui::Area::new(egui::Id::new("popup")).order(egui::Order::Foreground).fixed_pos(pos2(x.max(bounds.left() + 10.0), y)).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        let p = ui.painter_at(r.expand(40.0));
        panel(&p, r, 16.0, col);
        // header
        let ic = pos2(r.left() + 42.0, r.top() + 44.0);
        if d.known {
            orb(&p, ic, 22.0, col, 0.4);
            icon(&p, d.icon, ic, 11.0, shade(col, 0.12));
        } else {
            dot(&p, ic, 11.0, col, 0.4);
        }
        p.text(pos2(r.left() + 80.0, r.top() + 32.0), Align2::LEFT_CENTER, &d.name, FontId::proportional(19.0), Color32::WHITE);
        let kindname = d.details.first().map(|x| x.1.clone()).unwrap_or_default();
        p.text(pos2(r.left() + 80.0, r.top() + 53.0), Align2::LEFT_CENTER, kindname, FontId::proportional(12.5), col);
        // signal
        let by = r.top() + 72.0;
        match d.dbm {
            Some(dbm) => {
                led_bar(&p, Rect::from_min_size(pos2(r.left() + 22.0, by), vec2(w - 130.0, 9.0)), ((dbm + 100.0) / 55.0).clamp(0.0, 1.0), col, 24);
                p.text(pos2(r.right() - 22.0, by + 4.5), Align2::RIGHT_CENTER, format!("{:.0} dBm", dbm), FontId::monospace(13.0), TEXT);
            }
            None => {
                p.text(pos2(r.left() + 22.0, by + 4.5), Align2::LEFT_CENTER, "no signal reading for this device", FontId::proportional(11.5), DIM);
            }
        }
        // rows
        let mut yy = r.top() + 100.0;
        p.line_segment([pos2(r.left() + 20.0, yy - 6.0), pos2(r.right() - 20.0, yy - 6.0)], Stroke::new(1.0, alpha(CYAN, 0.15)));
        for (k, v) in rows {
            let rh = v.size().y.max(14.0);
            p.galley(pos2(r.left() + 22.0, yy + 2.0), k, CYAN);
            p.galley(pos2(r.left() + 22.0 + kw, yy), v, TEXT);
            yy += rh + 7.0;
        }
        yy += 6.0;
        lcd(&p, pos2(r.left() + 22.0, yy + 6.0), Align2::LEFT_CENTER, "HOW WE KNOW", 11.0, col);
        p.galley(pos2(r.left() + 22.0, yy + 20.0), why, TEXT);
        p.text(pos2(r.left() + 22.0, r.bottom() - 14.0), Align2::LEFT_CENTER, "Drag the node to where this device really is.", FontId::proportional(11.0), DIM);
        // close
        let cr = Rect::from_center_size(pos2(r.right() - 24.0, r.top() + 24.0), Vec2::splat(24.0));
        let resp = hit(ui, cr, ui.scope_id().with("close"), Sense::click());
        let cc = if resp.hovered() { Color32::WHITE } else { DIM };
        p.line_segment([cr.center() + vec2(-5.0, -5.0), cr.center() + vec2(5.0, 5.0)], Stroke::new(2.0, cc));
        p.line_segment([cr.center() + vec2(5.0, -5.0), cr.center() + vec2(-5.0, 5.0)], Stroke::new(2.0, cc));
        if resp.clicked() {
            *close = true;
        }
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        self.sync();
        ui.ctx().request_repaint();
        let t = self.t0.elapsed().as_secs_f32();
        egui::Panel::left("nav").exact_size(210.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| self.nav(ui));
        egui::Panel::top("hdr").exact_size(70.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| self.header(ui, t));
        egui::CentralPanel::no_frame().show(ui, |ui| match self.screen {
            Screen::Radar => self.radar(ui, t),
            Screen::Spectrum => self.spectrum(ui, t),
            Screen::Devices => self.devices(ui),
        });
    }
}

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([960.0, 640.0]).with_title("Airscope").with_app_id("airscope"),
        ..Default::default()
    };
    eframe::run_native("Airscope", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn router_radios_match_neighbours_do_not() {
        let gw = mac_bytes("B0:82:E2:B4:3A:56").unwrap();
        for m in ["B0:82:E2:B4:3A:56", "B2:82:E2:94:3A:56", "B2:82:E2:A4:3A:56", "B2:82:E2:B4:3A:56"] {
            assert!(same_device(mac_bytes(m).unwrap(), gw), "{m}");
        }
        for m in ["C0:94:35:EA:5D:8D", "70:F1:96:79:0A:62", "B0:82:E2:11:22:33"] {
            assert!(!same_device(mac_bytes(m).unwrap(), gw), "{m}");
        }
    }
}
