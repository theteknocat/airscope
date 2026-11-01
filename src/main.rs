mod data;
mod fx;

use data::{hash64, BtLink, Dev, Handle, Icon, Kind};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use fx::*;
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Instant;

const DMAX: f32 = 80.0;
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
    center: Pos2,
    open: bool,
    pinned: bool,
    fan_r: f32,
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
    open: HashSet<String>,       // group keys pinned open
    prev_key: Option<String>,    // group currently hovered for peeking
    prev_since: f32,
    no_preview: Option<String>,  // just collapsed: do not re-peek until the pointer leaves
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
            screen: Screen::Radar,
            sel: None,
            over,
            host: if host.is_empty() { "this PC".into() } else { host },
            t0: Instant::now(),
            zoom: std::env::var("AIRSCOPE_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0), // debug aid
            pan: Vec2::ZERO,
            open: HashSet::new(),
            prev_key: None,
            prev_since: 0.0,
            no_preview: None,
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
            let resp = ui.interact(r, ui.scope_id().with(label), Sense::click());
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
            let resp = ui.interact(r, ui.scope_id().with("rescan"), Sense::click());
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

    fn radar(&mut self, ui: &mut egui::Ui, t: f32) {
        let rect = ui.max_rect();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);

        // --- view: zoom + pan. The background is registered first so nodes win the hit test.
        let base = (rect.width().min(rect.height()) * 0.5 - 28.0).max(120.0);
        let bg = ui.interact(rect, ui.scope_id().with("radar_bg"), Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.hover_pos());
        if bg.dragged() {
            self.pan += bg.drag_delta();
        }
        if bg.double_clicked() {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
        }
        if let Some(ptr) = pointer {
            if rect.contains(ptr) {
                let sc = ui.input(|i| i.smooth_scroll_delta.y);
                if sc != 0.0 {
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

        // --- backdrop
        for i in 0..18 {
            let k = i as f32 / 18.0;
            p.circle_filled(c, big * 1.2 * (1.0 - k), mix(BG, Color32::from_rgb(11, 28, 56), k * k));
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

        // sweep
        let sw = (t * 0.9) % TAU;
        let mut m = egui::Mesh::default();
        let n = 36;
        for k in 0..n {
            let a0 = sw - k as f32 * 0.025;
            let a1 = sw - (k + 1) as f32 * 0.025;
            let col = add(CYAN, 0.16 * (1.0 - k as f32 / n as f32).powi(2));
            let i0 = m.vertices.len() as u32;
            m.colored_vertex(c, col);
            m.colored_vertex(c + vec2(a0.cos(), a0.sin()) * ro, col);
            m.colored_vertex(c + vec2(a1.cos(), a1.sin()) * ro, col);
            m.add_triangle(i0, i0 + 1, i0 + 2);
        }
        p.add(egui::Shape::mesh(m));
        p.line_segment([c, c + vec2(sw.cos(), sw.sin()) * ro], Stroke::new(1.5, alpha(CYAN, 0.55)));

        // --- placement (screen space)
        let mut nodes: Vec<N> = self
            .devs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let (mut a, mut dd, dim) = layout_of(d);
                if let Some(&(oa, od)) = self.over.get(&d.id) {
                    a = oa;
                    dd = od;
                }
                N { i, pos: c + vec2(a.cos(), a.sin()) * rmap(dd, big), ang: a.rem_euclid(TAU), dist: dd, dim }
            })
            .collect();
        nodes.sort_by(|a, b| b.dist.partial_cmp(&a.dist).unwrap());
        let cnt = nodes.len();

        // --- grouping: nodes closer than GROUP_PX on screen merge (union-find)
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
        let mut groups: Vec<G> = vec![];
        let mut eff: Vec<Pos2> = nodes.iter().map(|n| n.pos).collect();
        let mut hidden = vec![false; cnt];
        let mut in_open = vec![false; cnt];
        for (_, mem) in by_root.into_iter().filter(|(_, m)| m.len() > 1) {
            let center = pos2(mem.iter().map(|&i| nodes[i].pos.x).sum::<f32>() / mem.len() as f32, mem.iter().map(|&i| nodes[i].pos.y).sum::<f32>() / mem.len() as f32);
            let key = mem.iter().map(|&i| self.devs[nodes[i].i].id.clone()).min().unwrap();
            let sel_inside = self.sel.as_deref().map_or(false, |s| mem.iter().any(|&i| self.devs[nodes[i].i].id == s));
            if sel_inside {
                self.open.insert(key.clone()); // a selected device must be visible
            }
            if std::env::var_os("AIRSCOPE_PIN_ALL").is_some() {
                self.open.insert(key.clone()); // debug aid: pin every group open
            }
            let pinned = self.open.contains(&key);
            let previewing = self.prev_key.as_deref() == Some(key.as_str()) && t - self.prev_since > 0.45;
            let open = pinned || previewing;
            let fan_r = (mem.len() as f32 * 46.0 / TAU).max(58.0);
            let mut ordered = mem.clone();
            ordered.sort_by(|&a, &b| {
                let (va, vb) = (nodes[a].pos - center, nodes[b].pos - center);
                va.y.atan2(va.x).partial_cmp(&vb.y.atan2(vb.x)).unwrap()
            });
            if open {
                let v0 = nodes[ordered[0]].pos - center;
                let a0 = if v0.length() > 0.5 { v0.y.atan2(v0.x) } else { -1.2 };
                for (k, &i) in ordered.iter().enumerate() {
                    let a = a0 + k as f32 * TAU / ordered.len() as f32;
                    eff[i] = center + vec2(a.cos(), a.sin()) * fan_r;
                    in_open[i] = true;
                }
            } else {
                for &i in &mem {
                    eff[i] = center;
                    hidden[i] = true;
                }
            }
            groups.push(G { key, members: mem, center, open, pinned, fan_r });
        }
        groups.sort_by(|a, b| a.key.cmp(&b.key));

        // --- hover-to-peek bookkeeping (a peeked group stays open while the pointer is inside its fan)
        let mut new_prev: Option<String> = None;
        let mut near_any_icon = false;
        if let Some(ptr) = pointer {
            for g in &groups {
                let d = (ptr - g.center).length();
                let near_icon = d < 24.0;
                near_any_icon |= near_icon;
                let in_fan = g.open && !g.pinned && self.prev_key.as_deref() == Some(g.key.as_str()) && d < g.fan_r + 34.0;
                if (near_icon && self.no_preview.as_deref() != Some(g.key.as_str())) || in_fan {
                    new_prev = Some(g.key.clone());
                }
            }
        }
        if !near_any_icon {
            self.no_preview = None;
        }
        if new_prev != self.prev_key {
            self.prev_since = t;
            self.prev_key = new_prev;
        }

        // --- which labels fit (connected and strongest first)
        let mut allow: std::collections::HashSet<usize> = Default::default();
        {
            let mut placed: Vec<Rect> = (0..cnt).filter(|&i| !hidden[i]).map(|i| Rect::from_center_size(eff[i], Vec2::splat(30.0))).collect();
            placed.extend(groups.iter().filter(|g| !g.open).map(|g| Rect::from_center_size(g.center, Vec2::splat(44.0))));
            placed.push(Rect::from_center_size(c, Vec2::splat(120.0)));
            let mut order: Vec<usize> = (0..cnt).filter(|&i| !hidden[i] && self.devs[nodes[i].i].known && !nodes[i].dim).collect();
            order.sort_by(|&a, &b| {
                let (da, db) = (&self.devs[nodes[a].i], &self.devs[nodes[b].i]);
                db.connected.cmp(&da.connected).then(db.dbm.unwrap_or(-120.0).partial_cmp(&da.dbm.unwrap_or(-120.0)).unwrap())
            });
            for i in order {
                let g = p.layout_no_wrap(self.devs[nodes[i].i].name.clone(), FontId::proportional(11.5), TEXT);
                let r = Rect::from_center_size(eff[i] + vec2(0.0, 30.0), g.size() + vec2(8.0, 4.0));
                if placed.iter().all(|q| !q.intersects(r)) {
                    placed.push(r);
                    allow.insert(i);
                }
            }
        }

        // --- links from the PC to connected Bluetooth devices, with travelling pulses
        for (ni, n) in nodes.iter().enumerate() {
            let d = &self.devs[n.i];
            if d.kind == Kind::Bt && d.connected {
                let col = kind_col(d.kind);
                p.line_segment([c, eff[ni]], Stroke::new(5.0, add(col, 0.06)));
                p.line_segment([c, eff[ni]], Stroke::new(1.5, alpha(col, 0.55)));
                for k in 0..3 {
                    let u = (t * 0.6 + k as f32 / 3.0).fract();
                    let q = c + (eff[ni] - c) * u;
                    glow(&p, q, 2.5, col, 0.8);
                    p.circle_filled(q, 2.0, Color32::WHITE);
                }
            }
        }
        // spokes of open groups
        for g in groups.iter().filter(|g| g.open) {
            p.circle_stroke(g.center, g.fan_r, Stroke::new(1.0, alpha(CYAN, 0.12)));
            for &i in &g.members {
                p.line_segment([g.center, eff[i]], Stroke::new(4.0, add(CYAN, 0.05)));
                p.line_segment([g.center, eff[i]], Stroke::new(1.2, alpha(CYAN, 0.5)));
            }
        }

        // --- nodes
        let mut clicked: Option<String> = None;
        let mut new_over: Vec<(String, (f32, f32))> = vec![];
        let mut drag_end = false;
        let mut hover: Option<usize> = None;
        let view = rect.expand(70.0);
        for (ni, n) in nodes.iter().enumerate() {
            if hidden[ni] || !view.contains(eff[ni]) {
                continue;
            }
            let pos = eff[ni];
            let d = &self.devs[n.i];
            let col = if n.dim { shade(kind_col(d.kind), 0.5) } else { kind_col(d.kind) };
            let diff = (sw - n.ang).rem_euclid(TAU);
            let hot = if diff < 0.9 { 1.0 - diff / 0.9 } else { 0.0 };
            let sel = self.sel.as_deref() == Some(d.id.as_str());
            let rad = if d.known { 17.0 } else { 4.0 + ((d.dbm.unwrap_or(-85.0) + 100.0) / 50.0).clamp(0.0, 1.0) * 8.0 };
            let sense = if in_open[ni] { Sense::click() } else { Sense::click_and_drag() };
            let resp = ui.interact(Rect::from_center_size(pos, Vec2::splat(rad * 2.0 + 12.0)), ui.scope_id().with(("node", &d.id)), sense);
            if resp.hovered() {
                hover = Some(ni);
            }
            if resp.dragged() && !in_open[ni] {
                if let Some(pp) = resp.interact_pointer_pos() {
                    let v = pp - c;
                    new_over.push((d.id.clone(), (v.y.atan2(v.x), dmap(v.length(), big).clamp(0.6, DMAX))));
                }
            }
            if resp.drag_stopped() {
                drag_end = true;
            }
            if resp.clicked() {
                clicked = Some(d.id.clone());
            }
            let boost = hot * 0.8 + if sel || resp.hovered() { 0.9 } else { 0.0 };
            if d.connected {
                let pulse = (t * 2.6).sin() * 0.5 + 0.5;
                p.circle_stroke(pos, rad + 5.0 + pulse * 5.0, Stroke::new(1.5, add(col, 0.35 * (1.0 - pulse))));
            }
            if d.known {
                orb(&p, pos, rad, col, boost);
                icon(&p, d.icon, pos, rad * 0.5, shade(col, 0.12));
            } else {
                dot(&p, pos, rad, col, boost);
            }
            if sel {
                for k in 0..4 {
                    let a = t * 1.4 + k as f32 * TAU / 4.0;
                    arc(&p, pos, rad + 10.0, a, a + 0.9, Stroke::new(2.0, Color32::WHITE));
                }
            }
            if (d.known && !n.dim && allow.contains(&ni)) || sel || resp.hovered() {
                let tp = pos + vec2(0.0, rad + 13.0);
                p.text(tp + vec2(1.0, 1.0), Align2::CENTER_CENTER, &d.name, FontId::proportional(11.5), Color32::from_black_alpha(200));
                p.text(tp, Align2::CENTER_CENTER, &d.name, FontId::proportional(11.5), alpha(TEXT, 0.9));
            }
        }

        // --- group icons (collapsed) and collapse buttons (open)
        let mut toggle: Vec<(String, bool)> = vec![]; // (key, pin?)
        let mut group_hover: Option<usize> = None;
        for (gi, g) in groups.iter().enumerate() {
            if !view.contains(g.center) {
                continue;
            }
            let kinds: Vec<Kind> = g.members.iter().map(|&i| self.devs[nodes[i].i].kind).collect();
            let same = kinds.iter().all(|k| *k == kinds[0]);
            let col = if same { kind_col(kinds[0]) } else { Color32::from_rgb(190, 212, 240) };
            if g.open {
                let resp = ui.interact(Rect::from_center_size(g.center, Vec2::splat(40.0)), ui.scope_id().with(("gc", &g.key)), Sense::click());
                let hot = if resp.hovered() { 0.9 } else { 0.2 };
                orb(&p, g.center, 11.0, col, hot);
                p.line_segment([g.center - vec2(5.0, 0.0), g.center + vec2(5.0, 0.0)], Stroke::new(2.2, shade(col, 0.15)));
                if resp.hovered() {
                    group_hover = Some(gi);
                }
                if resp.clicked() {
                    toggle.push((g.key.clone(), !g.pinned)); // pinned -> collapse, peeked -> pin
                }
            } else {
                let resp = ui.interact(Rect::from_center_size(g.center, Vec2::splat(46.0)), ui.scope_id().with(("gi", &g.key)), Sense::click());
                let hovered = resp.hovered();
                orb(&p, g.center, 19.0, col, if hovered { 0.9 } else { 0.25 });
                // small dots on a ring: one per member, coloured by kind
                let nn = g.members.len().min(10);
                for k in 0..nn {
                    let a = t * 0.35 + k as f32 * TAU / nn as f32;
                    let kc = kind_col(self.devs[nodes[g.members[k]].i].kind);
                    p.circle_filled(g.center + vec2(a.cos(), a.sin()) * 27.0, 2.6, kc);
                }
                p.text(g.center, Align2::CENTER_CENTER, g.members.len().to_string(), FontId::monospace(15.0), Color32::from_rgb(8, 16, 30));
                if hovered {
                    group_hover = Some(gi);
                }
                if resp.clicked() {
                    toggle.push((g.key.clone(), true));
                }
            }
        }

        // --- this PC
        let pc_r = 25.0;
        let pulse = (t * 1.3).sin() * 0.5 + 0.5;
        p.circle_stroke(c, pc_r + 8.0 + pulse * 8.0, Stroke::new(1.5, add(CYAN, 0.3 * (1.0 - pulse))));
        orb(&p, c, pc_r, Color32::from_rgb(120, 220, 255), 0.35);
        icon(&p, Icon::Computer, c, pc_r * 0.46, Color32::from_rgb(8, 24, 40));
        lcd(&p, c + vec2(0.0, pc_r + 16.0), Align2::CENTER_CENTER, &self.host.to_uppercase(), 12.0, CYAN);

        // --- hover pills
        if let Some(ni) = hover {
            let d = &self.devs[nodes[ni].i];
            let sub = match d.dbm {
                Some(x) => format!("{} · {:.0} dBm", d.vendor, x),
                None => d.vendor.clone(),
            };
            pill(&p, eff[ni] + vec2(0.0, -24.0), &d.name, &sub, kind_col(d.kind));
        } else if let Some(gi) = group_hover {
            let g = &groups[gi];
            let names: Vec<String> = g.members.iter().take(4).map(|&i| self.devs[nodes[i].i].name.clone()).collect();
            let more = if g.members.len() > 4 { format!(" +{}", g.members.len() - 4) } else { String::new() };
            let head = format!("{} devices close together", g.members.len());
            let sub = if g.open { "click the centre dot to collapse".to_string() } else { format!("{}{} · click to pin open", names.join(", "), more) };
            pill(&p, g.center + vec2(0.0, -34.0), &head, &sub, CYAN);
        }

        // --- apply events
        for (id, v) in new_over {
            self.over.insert(id, v);
        }
        if drag_end {
            self.save_over();
        }
        for (key, pin) in toggle {
            if pin {
                self.open.insert(key);
            } else {
                self.open.remove(&key);
                self.no_preview = Some(key);
                self.prev_key = None;
            }
        }
        if let Some(id) = clicked {
            self.sel = if self.sel.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        }
        if let Some(id) = self.sel.clone() {
            if let Some(ni) = nodes.iter().enumerate().find(|(_, n)| self.devs[n.i].id == id).map(|(ni, _)| ni) {
                let d = self.devs[nodes[ni].i].clone();
                let mut close = false;
                popup(ui.ctx(), &d, eff[ni], rect, &mut close, kind_col(d.kind));
                if close {
                    self.sel = None;
                }
            }
        }

        // --- zoom HUD
        let hud = Rect::from_min_size(pos2(rect.right() - 318.0, rect.bottom() - 48.0), vec2(300.0, 34.0));
        panel(&p, hud, 17.0, CYAN);
        lcd(&p, pos2(hud.left() + 18.0, hud.center().y), Align2::LEFT_CENTER, &format!("ZOOM {:.1}x", self.zoom), 12.0, CYAN);
        p.text(pos2(hud.left() + 116.0, hud.center().y), Align2::LEFT_CENTER, "scroll · drag · double-click", FontId::proportional(11.0), DIM);
        let rb = Rect::from_center_size(pos2(hud.right() - 28.0, hud.center().y), vec2(42.0, 22.0));
        let rr = ui.interact(rb, ui.scope_id().with("reset_view"), Sense::click());
        p.rect_filled(rb, 9.0, alpha(CYAN, if rr.hovered() { 0.35 } else { 0.15 }));
        p.text(rb.center(), Align2::CENTER_CENTER, "1:1", FontId::monospace(11.0), Color32::WHITE);
        if rr.clicked() {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
        }
    }

    // ---------- Bluetooth channel strip ----------

    fn strip(&self, ui: &mut egui::Ui, t: f32) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, shade(BG, 1.35));
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, alpha(CYAN, 0.15)));
        lcd(&p, pos2(rect.left() + 22.0, rect.top() + 20.0), Align2::LEFT_CENTER, "BLUETOOTH CHANNEL MAP", 13.0, CYAN);
        p.text(
            pos2(rect.left() + 250.0, rect.top() + 20.0),
            Align2::LEFT_CENTER,
            "lit = channels the controller hops on · red = avoided · amber bands = Wi-Fi 2.4 GHz networks",
            FontId::proportional(11.5),
            DIM,
        );
        let x0 = rect.left() + 200.0;
        let x1 = rect.right() - 84.0;
        let cw = (x1 - x0) / 79.0;
        // Wi-Fi footprints
        let yw = rect.top() + 36.0;
        p.text(pos2(x0 - 12.0, yw + 12.0), Align2::RIGHT_CENTER, "Wi-Fi 2.4 GHz", FontId::proportional(12.0), AMBER);
        let mut bands: Vec<(i32, i32, f32, &Dev)> = vec![];
        for d in self.devs.iter().filter(|d| d.kind == Kind::Wifi) {
            if let (Some(f), Some(dbm)) = (d.freq, d.dbm) {
                if f < 3000 {
                    bands.push((f as i32 - 10 - 2402, f as i32 + 10 - 2402, ((dbm + 100.0) / 50.0).clamp(0.05, 1.0), d));
                }
            }
        }
        for (lo, hi, s, _) in &bands {
            let r = Rect::from_min_max(pos2(x0 + (*lo).max(0) as f32 * cw, yw), pos2(x0 + (*hi).min(78) as f32 * cw + cw, yw + 24.0));
            p.rect_filled(r, 6.0, add(AMBER, 0.05 + 0.16 * s));
            p.rect_stroke(r, 6.0, Stroke::new(1.0, alpha(AMBER, 0.15 + 0.4 * s)), StrokeKind::Inside);
        }
        let mut top: Vec<&(i32, i32, f32, &Dev)> = bands.iter().filter(|b| b.3.known).collect();
        top.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
        let mut seen: Vec<&str> = vec![];
        let mut spans: Vec<(f32, f32)> = vec![];
        for b in top.iter().take(8) {
            if seen.contains(&b.3.cluster.as_str()) {
                continue;
            }
            seen.push(&b.3.cluster);
            let cx = x0 + ((b.0 + b.1) as f32 / 2.0 + 0.5) * cw;
            let g = p.layout_no_wrap(b.3.name.clone(), FontId::proportional(11.0), Color32::WHITE);
            let span = (cx - g.size().x / 2.0 - 6.0, cx + g.size().x / 2.0 + 6.0);
            if spans.iter().any(|q: &(f32, f32)| span.0 < q.1 && span.1 > q.0) {
                continue;
            }
            spans.push(span);
            p.galley(pos2(cx - g.size().x / 2.0, yw + 12.0 - g.size().y / 2.0), g, alpha(Color32::WHITE, 0.9));
        }
        // link rows
        let mut y = yw + 34.0;
        if self.links.is_empty() {
            p.text(pos2(x0, y + 10.0), Align2::LEFT_CENTER, "No Bluetooth link is connected right now.", FontId::proportional(12.0), DIM);
        }
        for l in &self.links {
            p.text(pos2(x0 - 12.0, y + 9.0), Align2::RIGHT_CENTER, if l.name.is_empty() { &l.mac } else { &l.name }, FontId::proportional(12.0), TEXT);
            for i in 0..79usize {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * cw, y), vec2(cw - 1.5, 18.0));
                let used = l.afh.as_ref().map(|m| m[i]).unwrap_or(true);
                let col = if l.afh.is_none() { MAGENTA } else { CYAN };
                // is a strong Wi-Fi network sitting on this channel?
                let under = bands.iter().any(|(lo, hi, s, _)| *s > 0.55 && (i as i32) >= *lo && (i as i32) <= *hi);
                if used {
                    p.rect_filled(r.expand(1.5), 2.0, add(col, if under { 0.06 } else { 0.10 }));
                    p.rect_filled(r, 2.0, if under { mix(col, RED, 0.45) } else { col });
                    p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 5.0)), 2.0, add(Color32::WHITE, 0.2));
                } else {
                    p.rect_filled(r, 2.0, alpha(RED, 0.20));
                }
            }
            let txt = match &l.afh {
                Some(m) => format!("{}/79", m.iter().filter(|x| **x).count()),
                None => "no AFH".to_string(),
            };
            p.text(pos2(x1 + 4.0, y + 9.0), Align2::LEFT_CENTER, txt, FontId::monospace(10.0), DIM);
            y += 26.0;
        }
        let _ = t;
    }

    // ---------- devices ----------

    fn devices(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, BG);
        let mut order: Vec<usize> = (0..self.devs.len()).collect();
        order.sort_by_key(|&i| (self.devs[i].kind as u8, !self.devs[i].connected as u8, std::cmp::Reverse(self.devs[i].dbm.map(|x| x as i32).unwrap_or(-200))));
        let mut pick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width() - 30.0;
            ui.add_space(14.0);
            for &i in &order {
                let d = &self.devs[i];
                let (r, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
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
        });
        if let Some(id) = pick {
            self.sel = Some(id);
            self.screen = Screen::Radar;
        }
    }

    fn placeholder(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, BG);
        let r = Rect::from_center_size(rect.center(), vec2(520.0, 150.0));
        panel(&p, r, 16.0, MAGENTA);
        lcd(&p, r.center() - vec2(0.0, 24.0), Align2::CENTER_CENTER, "SPECTRUM · NEXT DROP", 20.0, MAGENTA);
        p.text(r.center() + vec2(0.0, 18.0), Align2::CENTER_CENTER, "Full 2.4 GHz view and ranked interference suspects.\nThe Bluetooth channel map on the Radar screen is the preview.", FontId::proportional(14.0), DIM);
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
        let resp = ui.interact(cr, ui.scope_id().with("close"), Sense::click());
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
        if self.screen == Screen::Radar {
            let h = 92.0 + self.links.len().max(1) as f32 * 26.0;
            egui::Panel::bottom("strip").exact_size(h).resizable(false).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| self.strip(ui, t));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| match self.screen {
            Screen::Radar => self.radar(ui, t),
            Screen::Spectrum => self.placeholder(ui),
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
