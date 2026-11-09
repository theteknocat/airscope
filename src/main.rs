mod data;
mod demo;
mod bluez;
mod capture;
mod diag;
mod fx;
mod hci;
mod net;
mod nl80211;
mod nm;
mod picker;
mod snoop;

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
    gdrag: Option<Vec<String>>, // member ids of a group icon being dragged
}

struct Burst {
    w: Vec2, // position relative to the PC in units of the radar radius, so it follows zoom/pan
    col: Color32,
    t0: f32,
    rad: f32,
    known: bool,
    ghost: bool, // fade the device out in place (not for a member of a collapsed group: only the ring)
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

const PC_R: f32 = 25.0; // radius of the PC orb at the centre, in screen px

/// Nearest a device's centre may sit to the PC's centre: close is fine, but its edge never touches the PC orb.
fn min_px(d: &Dev) -> f32 {
    PC_R + 6.0 + node_rad(d)
}

#[allow(clippy::too_many_arguments)]
/// Visual radius of a device's dot/orb.
fn node_rad(d: &Dev) -> f32 {
    if d.known {
        17.0
    } else {
        4.0 + data::signal_frac(d.dbm.unwrap_or(-85.0)) * 8.0
    }
}

#[allow(clippy::too_many_arguments)]
/// Pulsing halo for a device with a problem: red for a real problem, amber for an early warning.
fn trouble_halo(p: &egui::Painter, pos: Pos2, rad: f32, sev: f32, t: f32, col: Color32) {
    let pulse = (t * 4.0).sin() * 0.5 + 0.5;
    glow(p, pos, rad + 16.0 + pulse * 6.0, col, 0.35 + 0.4 * sev);
    p.circle_stroke(pos, rad + 7.0 + pulse * 3.0, Stroke::new(1.5 + sev, alpha(col, 0.5 + 0.4 * pulse)));
}

fn node_widget(ui: &egui::Ui, p: &egui::Painter, d: &Dev, n: &N, ni: usize, pos: Pos2, t: f32, sw: f32, sel: bool, label: f32, interactive: bool, can_drag: bool, scale: f32, c: Pos2, big: f32, ev: &mut Ev) {
    let col = if n.dim { shade(kind_col(d.kind), 0.5) } else { kind_col(d.kind) };
    let diff = (sw - n.ang).rem_euclid(TAU);
    let hot = if diff < 0.9 { 1.0 - diff / 0.9 } else { 0.0 };
    let rad = scale * node_rad(d);
    let mut hovered = false;
    if interactive {
        let sense = if can_drag { Sense::click_and_drag() } else { Sense::click() };
        let resp = hit(ui, Rect::from_center_size(pos, Vec2::splat(rad * 2.0 + 12.0)), ui.scope_id().with(("node", &d.id)), sense);
        hovered = resp.hovered();
        if hovered {
            ev.hover = Some(ni);
        }
        if can_drag && resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            ev.dragging = Some(d.id.clone());
            if let Some(pp) = resp.interact_pointer_pos() {
                let v = pp - c;
                ev.over.push((d.id.clone(), (v.y.atan2(v.x), dmap(v.length().max(min_px(d)), big).clamp(0.6, DMAX))));
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
    // the hovered device is painted on a higher layer: always above neighbouring orbs and labels
    let fp;
    let p: &egui::Painter = if hovered {
        fp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("hover_node"))).with_clip_rect(p.clip_rect());
        &fp
    } else {
        p
    };
    if d.trouble > 0.0 {
        trouble_halo(p, pos, rad, d.trouble, t, RED);
    } else if d.caution > 0.0 {
        trouble_halo(p, pos, rad, d.caution, t, AMBER);
    }
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
    // offline (dimmed) devices keep their label too, dimmed like the orb; hover brightens it
    let a = if sel || hovered { 1.0 } else if d.known { label } else { 0.0 };
    let dimf = if n.dim && !(sel || hovered) { 0.5 } else { 1.0 };
    if a > 0.01 {
        let tp = pos + vec2(0.0, rad + 13.0);
        if hovered {
            hover_label(p, tp, &d.name, FontId::proportional(11.5));
        } else {
            let name = fit_middle(p, &d.name, FontId::proportional(11.5), LABEL_W);
            p.text(tp + vec2(1.0, 1.0), Align2::CENTER_CENTER, &name, FontId::proportional(11.5), Color32::from_black_alpha((200.0 * a * dimf) as u8));
            p.text(tp, Align2::CENTER_CENTER, &name, FontId::proportional(11.5), alpha(TEXT, 0.9 * a * dimf));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Radar,
    Spectrum,
    Devices,
    Diagnostics,
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
    vendor_open: bool,
    popup_rects: Vec<Rect>, // where info boxes were drawn last frame: the wheel scrolls them instead of zooming the radar
    wheel: f32,             // wheel movement taken for an info box this frame
    shot_sent: bool, // AIRSCOPE_SHOT: screenshot already requested
    vendor_path: String,
    vendor_msg: Option<(bool, String)>, // (ok, text) of the last import
    vendor_copied: Option<(usize, Instant)>, // which link was just opened
    vendor_pick: Option<picker::Pick>, // file dialog for the downloaded list, while open
    screen: Screen,
    diag: diag::Diag,
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
    label_a: HashMap<String, f32>, // 0..1 fade of each device's permanent label
    last_world: HashMap<String, (Vec2, bool)>, // where each device was last shown (PC-relative, zoom-independent), and whether it sat inside a collapsed group icon
    dragging: Option<String>,
    gdrag: Option<Vec<String>>,    // a whole group is being dragged: its members move together and don't merge until dropped
    prev_groups: Vec<(String, Vec<String>)>,
    formed: HashSet<String>,       // groups whose icon is already showing
    gshift: HashMap<String, Vec2>, // eased push-away shift per group
    mine: Vec<String>,             // optional extra SSID prefixes counted as yours (~/.config/airscope/mine.txt)
    q: String,                     // device list filters
    show: [bool; 3],
    scope: u8,
    hide_unnamed: bool,
    collapsed: HashSet<usize>,
    anchor: Pos2,                  // where the info box opens when picked on a non-radar screen
    tip_want: Option<TipC>,        // tooltip requested this frame (drawn, faded, at the end of ui())
    tip_last: Option<TipC>,
    tip_a: f32,
    pops: Vec<Pop>,                // info boxes: the selected one fades in, others fade out
    pop_uid: u64,
    radar_anchor: Option<(String, Pos2)>,
}

fn cfg_path() -> Option<std::path::PathBuf> {
    if demo::on() {
        return None; // demo mode neither reads nor writes the real saved positions
    }
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
/// Devices on the radar are the ones with a wireless signal: Wi-Fi access points and connected Bluetooth devices.
/// Everything else (network devices, Bluetooth that is not connected) sits in the tray beside it.
fn on_radar(d: &Dev) -> bool {
    d.dbm.is_some() || (d.kind == Kind::Bt && d.connected)
}

/// Tray heading for a device: its kind of device, so a Bluetooth speaker and a network speaker sit together.
fn tray_group(d: &Dev) -> &'static str {
    match d.icon {
        Icon::Router | Icon::Wifi => "Routers",
        Icon::Keyboard => "Keyboards",
        Icon::Headphones => "Headphones",
        Icon::Phone => "Phones",
        Icon::Gamepad => "Game controllers",
        Icon::Mouse => "Mice",
        Icon::Bluetooth => "Other Bluetooth",
        Icon::Computer => "Computers",
        Icon::Tv => "TVs",
        Icon::Server => "Servers and storage",
        Icon::Printer => "Printers",
        Icon::Speaker => "Speakers",
        Icon::Camera => "Cameras",
        Icon::Unknown => "Unidentified",
    }
}

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
        let host = if demo::on() { "studio-pc".to_string() } else { std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().to_string() };
        App {
            h: data::spawn(),
            ver: u64::MAX,
            devs: vec![],
            links: vec![],
            wifi_at: None,
            bt_at: None,
            lan_at: None,
            errs: vec![],
            shot_sent: false,
            popup_rects: vec![],
            wheel: 0.0,
            vendor_open: std::env::var("AIRSCOPE_VENDOR").is_ok(), // debug aid
            vendor_path: String::new(),
            vendor_msg: None,
            vendor_copied: None,
            vendor_pick: None,
            screen: match std::env::var("AIRSCOPE_SCREEN").as_deref() { Ok("spectrum") => Screen::Spectrum, Ok("devices") => Screen::Devices, Ok("diag") => Screen::Diagnostics, _ => Screen::Radar }, // debug aid
            diag: diag::Diag::new(),
            sel: std::env::var("AIRSCOPE_SEL").ok(), // debug aid
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
            label_a: HashMap::new(),
            last_world: HashMap::new(),
            dragging: None,
            gdrag: None,
            prev_groups: vec![],
            formed: HashSet::new(),
            gshift: HashMap::new(),
            mine: std::env::var("HOME").ok().filter(|_| !demo::on()).and_then(|h| std::fs::read_to_string(format!("{}/.config/airscope/mine.txt", h)).ok()).map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default(),
            q: String::new(),
            show: [true; 3],
            scope: 0,
            hide_unnamed: false,
            collapsed: HashSet::from([0, 1, 2, 3, 4]), // every group starts closed
            anchor: pos2(700.0, 400.0),
            tip_want: None,
            tip_last: None,
            tip_a: 0.0,
            pops: vec![],
            pop_uid: 0,
            radar_anchor: None,
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
        // a router's Wi-Fi BSSID can equal its LAN MAC: keep every device id unique (Wi-Fi keeps the original)
        let mut seen = HashSet::new();
        for d in self.devs.iter_mut() {
            if !seen.insert(d.id.clone()) {
                d.id = format!("{}~{}", d.id, match d.kind { Kind::Wifi => "wifi", Kind::Bt => "bt", Kind::Lan => "lan" });
                seen.insert(d.id.clone());
            }
        }
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
        let items = [(Screen::Radar, "Radar", 0u8), (Screen::Spectrum, "Spectrum", 1), (Screen::Devices, "Devices", 2), (Screen::Diagnostics, "BT Diagnostics", 3)];
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
        for (c, l) in [(AMBER, "Wi-Fi"), (BLUE, "Bluetooth"), (GREEN, "LAN")] {
            dot(&p, pos2(rect.left() + 28.0, y), 5.0, c, 0.0);
            p.text(pos2(rect.left() + 46.0, y), Align2::LEFT_CENTER, l, FontId::proportional(13.0), DIM);
            y += 22.0;
        }
    }

    /// Dialog for importing a newer vendor list, drawn like the rest of the app: centred, dimmed backdrop.
    fn vendor_window(&mut self, ctx: &egui::Context) {
        if !self.vendor_open {
            return;
        }
        let Some(oui) = data::oui() else { return };
        if let Some(res) = self.vendor_pick.as_ref().and_then(|p| p.poll()) {
            self.vendor_pick = None;
            match res {
                Ok(Some(path)) => {
                    self.vendor_path = path.display().to_string();
                    self.import_vendors(&oui);
                }
                Ok(None) => {}
                Err(e) => self.vendor_msg = Some((false, e)),
            }
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).find(|s| !s.is_empty()));
        if let Some(path) = dropped {
            self.vendor_path = path;
            self.import_vendors(&oui);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.vendor_open = false;
        }
        let screen = ctx.content_rect();
        let (w, h) = (620.0f32, 520.0f32);
        let r = Rect::from_center_size(screen.center(), vec2(w.min(screen.width() - 20.0), h.min(screen.height() - 20.0)));
        // backdrop: dims the app and closes the dialog when clicked
        let mut close = false;
        egui::Area::new(egui::Id::new("vendor_scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (sr, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(sr, 0.0, alpha(BG, 0.72));
            if resp.clicked() {
                close = true;
            }
        });
        let (total, c) = oui.stats();
        let col = CYAN;
        egui::Area::new(egui::Id::new("vendor_dialog")).order(egui::Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
            let (_, _) = ui.allocate_exact_size(r.size(), Sense::click()); // swallow clicks so the backdrop doesn't see them
            let p = ui.painter_at(r.expand(40.0));
            panel(&p, r, 16.0, col);
            let (l, rr) = (r.left() + 30.0, r.right() - 30.0);
            let inner = rr - l;
            // title
            lcd(&p, pos2(l, r.top() + 38.0), Align2::LEFT_CENTER, "VENDOR LIST", 24.0, col);
            p.text(pos2(l, r.top() + 66.0), Align2::LEFT_CENTER, "Turns the first half of a MAC address into a manufacturer name.", FontId::proportional(13.0), DIM);
            let cr = Rect::from_center_size(pos2(r.right() - 26.0, r.top() + 26.0), Vec2::splat(26.0));
            let resp = hit(ui, cr, ui.id().with("vclose"), Sense::click());
            let cc = if resp.hovered() { Color32::WHITE } else { DIM };
            p.line_segment([cr.center() + vec2(-5.0, -5.0), cr.center() + vec2(5.0, 5.0)], Stroke::new(2.0, cc));
            p.line_segment([cr.center() + vec2(5.0, -5.0), cr.center() + vec2(-5.0, 5.0)], Stroke::new(2.0, cc));
            if resp.clicked() {
                close = true;
            }
            // counts
            let tiles = [("KNOWN", total.to_string(), GREEN), ("BUILT IN", c[0].to_string(), CYAN), ("+ SYSTEM LIST", format!("+{}", c[1]), BLUE), ("+ YOUR FILE", format!("+{}", c[2]), AMBER)];
            let tw = (inner - 3.0 * 12.0) / 4.0;
            for (i, (label, num, tc)) in tiles.iter().enumerate() {
                let t = Rect::from_min_size(pos2(l + i as f32 * (tw + 12.0), r.top() + 92.0), vec2(tw, 62.0));
                p.rect_filled(t, 10.0, alpha(BG, 0.55));
                p.rect_stroke(t, 10.0, Stroke::new(1.0, alpha(*tc, 0.35)), StrokeKind::Inside);
                lcd(&p, pos2(t.left() + 14.0, t.top() + 24.0), Align2::LEFT_CENTER, num, 22.0, *tc);
                p.text(pos2(t.left() + 14.0, t.top() + 46.0), Align2::LEFT_CENTER, *label, FontId::monospace(10.5), alpha(*tc, 0.8));
            }
            let step = |y: f32, n: &str, title: &str| {
                let c = pos2(l + 13.0, y);
                p.circle_stroke(c, 13.0, Stroke::new(1.5, col));
                p.text(c, Align2::CENTER_CENTER, n, FontId::monospace(13.0), col);
                p.text(pos2(l + 38.0, y), Align2::LEFT_CENTER, title, FontId::proportional(15.0), Color32::WHITE);
            };
            let pill = |ui: &egui::Ui, rect: Rect, id: &str, label: &str, accent: Color32| -> bool {
                let resp = hit(ui, rect, ui.id().with(id), Sense::click());
                panel(&p, rect, 9.0, if resp.hovered() { CYAN } else { accent });
                p.text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.5), Color32::WHITE);
                resp.clicked()
            };
            // step 1
            let mut y = r.top() + 190.0;
            p.line_segment([pos2(l, y - 18.0), pos2(rr, y - 18.0)], Stroke::new(1.0, alpha(col, 0.15)));
            step(y, "1", "Download a newer list in your browser");
            y += 30.0;
            let links = [("IEEE text file", "oui.txt", "https://standards-oui.ieee.org/oui/oui.txt"), ("IEEE spreadsheet", "oui.csv", "https://standards-oui.ieee.org/oui/oui.csv"), ("Wireshark list", "manuf", "https://www.wireshark.org/download/automated/data/manuf")];
            for (i, (name, file, url)) in links.iter().enumerate() {
                p.text(pos2(l + 38.0, y + 12.0), Align2::LEFT_CENTER, format!("{}  ·  {}", name, file), FontId::proportional(13.5), TEXT);
                p.text(pos2(l + 38.0, y + 32.0), Align2::LEFT_CENTER, *url, FontId::monospace(11.5), DIM);
                let copied = self.vendor_copied.map_or(false, |(j, t)| j == i && t.elapsed().as_secs_f32() < 1.6);
                let b = Rect::from_min_size(pos2(rr - 112.0, y + 8.0), vec2(112.0, 32.0));
                if pill(ui, b, &format!("copy{}", i), if copied { "Opened" } else { "Open link" }, if copied { GREEN } else { BLUE }) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(*url));
                    self.vendor_copied = Some((i, Instant::now()));
                }
                y += 54.0;
            }
            // step 2
            y += 8.0;
            step(y, "2", "Give the downloaded file to Airscope");
            p.text(pos2(l + 38.0, y + 26.0), Align2::LEFT_CENTER, "Choose it with Open, or drag it onto this window.", FontId::proportional(12.5), DIM);
            let field = Rect::from_min_size(pos2(l + 38.0, y + 46.0), vec2(inner - 38.0 - 124.0, 32.0));
            p.rect_filled(field, 8.0, alpha(BG, 0.55));
            p.rect_stroke(field, 8.0, Stroke::new(1.0, alpha(CYAN, 0.30)), StrokeKind::Inside);
            let shown = self.vendor_path.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("No file chosen yet");
            p.text(pos2(field.left() + 12.0, field.center().y), Align2::LEFT_CENTER, shown, FontId::proportional(13.5), if self.vendor_path.is_empty() { DIM } else { TEXT });
            if pill(ui, Rect::from_min_size(pos2(rr - 112.0, y + 46.0), vec2(112.0, 32.0)), "open", if self.vendor_pick.is_some() { "Choosing..." } else { "Open" }, BLUE) && self.vendor_pick.is_none() {
                self.vendor_pick = Some(picker::Pick::start("Choose the downloaded vendor list"));
            }
            // result and footer
            if let Some((ok, msg)) = &self.vendor_msg {
                let mc = if *ok { GREEN } else { RED };
                dot(&p, pos2(l + 46.0, y + 98.0), 4.0, mc, 0.2);
                let g = p.layout(msg.clone(), FontId::proportional(13.0), mc, inner - 70.0);
                p.galley(pos2(l + 60.0, y + 90.0), g, mc);
            }
            if let Some(path) = data::user_oui_path() {
                let g = p.layout(format!("Your imported list is saved to {}. Delete that file to go back to the built-in list.", path.display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1)), FontId::proportional(11.5), DIM, inner);
                p.galley(pos2(l, r.bottom() - 18.0 - g.size().y), g, DIM);
            }
        });
        if close {
            self.vendor_open = false;
        }
    }

    /// AIRSCOPE_SHOT=<file.ppm>: grab the app's own frame after a few seconds and quit (README screenshots;
    /// nothing outside the app window is ever captured).
    fn debug_shot(&mut self, ctx: &egui::Context, t: f32) {
        let Ok(path) = std::env::var("AIRSCOPE_SHOT") else { return };
        if t > 7.0 && !self.shot_sent {
            self.shot_sent = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                for c in &img.pixels {
                    out.extend_from_slice(&[c.r(), c.g(), c.b()]);
                }
                let _ = std::fs::write(&path, out);
                std::process::exit(0);
            })));
        }
    }

    fn import_vendors(&mut self, oui: &data::Oui) {
        self.vendor_msg = Some(match oui.import(&self.vendor_path) {
            Ok(n) => (true, format!("Imported {} manufacturers. Names update within a few seconds.", n)),
            Err(e) => (false, e),
        });
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
            Screen::Diagnostics => "BT DIAGNOSTICS",
        };
        lcd(&p, pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, title, 28.0, CYAN);

        let age = |t: Option<Instant>| t.map(|t| format!("{}s", t.elapsed().as_secs())).unwrap_or("--".into());
        let wifi_n = self.devs.iter().filter(|d| d.kind == Kind::Wifi).count();
        let bt_n = self.links.len();
        let lan_n = self.devs.iter().filter(|d| d.kind == Kind::Lan).count();
        // each chip has three text levels (full, count only, name only); the full text is always in the tooltip
        let chips = [
            (AMBER, ["WI-FI".to_string(), format!("WI-FI {wifi_n}"), format!("WI-FI {wifi_n} APs · {}", age(self.wifi_at))], format!("Wi-Fi: {wifi_n} access points, last scan {} ago", age(self.wifi_at))),
            (BLUE, ["BT".to_string(), format!("BT {bt_n}"), format!("BT {bt_n} linked · {}", age(self.bt_at))], format!("Bluetooth: {bt_n} linked, last update {} ago", age(self.bt_at))),
            (GREEN, ["LAN".to_string(), format!("LAN {lan_n}"), format!("LAN {lan_n} · {}", age(self.lan_at))], format!("LAN: {lan_n} devices, last update {} ago", age(self.lan_at))),
        ];
        let mut x = rect.right() - 20.0;
        let left = rect.left() + 26.0 + title.len() as f32 * 17.0 + 16.0;
        let width = |lvl: usize| -> f32 { chips.iter().map(|(_, t, _)| p.layout_no_wrap(t[lvl].clone(), FontId::monospace(12.0), TEXT).size().x + 50.0).sum() };
        // chips shrink first (full text, count only, name only); if even names do not fit beside the full buttons, the buttons become icons
        let fit = |btn: f32| (0..3).rev().find(|&l| width(l) <= x - btn - left);
        let (lvl, compact) = match fit(314.0) {
            Some(l) => (l, false),
            None => (fit(92.0).unwrap_or(0), true),
        };
        let (rw, vw) = if compact { (38.0, 38.0) } else { (150.0, 132.0) };
        // rescan button
        {
            let r = Rect::from_min_size(pos2(x - rw, rect.center().y - 17.0), vec2(rw, 34.0));
            let resp = hit(ui, r, ui.scope_id().with("rescan"), Sense::click());
            panel(&p, r, 9.0, if resp.hovered() { CYAN } else { BLUE });
            icon_label(&p, r, Glyph::Refresh, if compact { "" } else { "Rescan Wi-Fi" }, 14.0);
            if resp.clicked() {
                self.h.lock().unwrap().rescan = true;
            }
            resp.hint("Rescan Wi-Fi. Active scan. It briefly disturbs Bluetooth on the shared Wi-Fi/BT chip, so it only runs when you ask.");
            x -= rw + 16.0;
        }
        // vendor database button
        {
            let r = Rect::from_min_size(pos2(x - vw, rect.center().y - 17.0), vec2(vw, 34.0));
            let resp = hit(ui, r, ui.scope_id().with("vendors"), Sense::click());
            panel(&p, r, 9.0, if resp.hovered() { CYAN } else { alpha(BLUE, 0.6) });
            icon_label(&p, r, Glyph::List, if compact { "" } else { "Vendor list" }, 14.0);
            if resp.clicked() {
                self.vendor_open = !self.vendor_open;
            }
            resp.hint("Vendor list. Update the list that turns MAC addresses into manufacturer names.");
            x -= vw + 16.0;
        }
        for (i, (c, txt, tip)) in chips.iter().enumerate().rev() {
            let g = p.layout_no_wrap(txt[lvl].clone(), FontId::monospace(12.0), TEXT);
            let w = g.size().x + 40.0;
            let r = Rect::from_min_size(pos2(x - w, rect.center().y - 15.0), vec2(w, 30.0));
            panel(&p, r, 15.0, *c);
            dot(&p, pos2(r.left() + 16.0, r.center().y), 3.5, *c, 0.2);
            p.galley(pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, TEXT);
            ui.interact(r, ui.scope_id().with(("chip", i)), Sense::hover()).hint(tip.clone()); // status only: no hand cursor
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
        // Existing devices are frozen while a group is open or a drag is active, so nothing drifts into it.
        // Devices that genuinely appear or disappear still do (removals only wait for a drag to finish).
        let drag = self.dragging.is_some() || self.gdrag.is_some();
        let freeze = !self.open.is_empty() || drag;
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
        if !drag {
            let live: HashSet<&str> = self.devs.iter().map(|d| d.id.as_str()).collect();
            let mut gone: Vec<(String, Kind, bool, Option<f32>)> = vec![];
            let over = &self.over;
            self.latched.retain(|d| {
                if !live.contains(d.id.as_str()) && crossed(angle(d, over)) {
                    gone.push((d.id.clone(), d.kind, d.known, d.dbm));
                    false
                } else {
                    true
                }
            });
            for (id, kind, known, dbm) in gone {
                if let Some(&(w, in_icon)) = self.last_world.get(&id) {
                    let rad = if known { 17.0 } else { 4.0 + data::signal_frac(dbm.unwrap_or(-85.0)) * 8.0 };
                    self.bursts.push(Burst { w, col: kind_col(kind), t0: t, rad, known, ghost: !in_icon });
                }
            }
        }
    }

    fn radar(&mut self, ui: &mut egui::Ui, t: f32) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, BG);
        // the tray takes the right-hand side; the radar gets the rest
        let tray_area = Rect::from_min_max(pos2(full.right() - 200.0, full.top() + 8.0), pos2(full.right() - 12.0, full.bottom() - 12.0));
        let rect = Rect::from_min_max(full.min, pos2(full.right() - 212.0, full.bottom()));
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);

        // --- view: zoom + pan. The background is registered first (and has no hand cursor).
        let base = (rect.width().min(rect.height()) * 0.5 - 28.0).max(120.0);
        let bg = ui.interact(rect, ui.scope_id().with("radar_bg"), Sense::click_and_drag());
        // the radar's grab area: inside the outermost ring plus a margin (the rest of the screen is just background)
        let (c_now, reach) = (rect.center() + self.pan, base * self.zoom + 28.0);
        let in_reach = |q: Pos2| (q - c_now).length() <= reach;
        let ok_id = egui::Id::new("radar_pan_ok");
        if bg.drag_started() {
            let ok = bg.interact_pointer_pos().map_or(false, in_reach);
            ui.ctx().data_mut(|d| d.insert_temp(ok_id, ok));
        }
        let pan_ok = ui.ctx().data(|d| d.get_temp::<bool>(ok_id)).unwrap_or(false);
        if bg.dragged() && pan_ok {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing); // closed hand only while dragging
        } else if bg.hovered() && bg.hover_pos().map_or(false, in_reach) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab); // open hand over the radar (an orb under the pointer shows the pointing hand)
        }
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let dt = ui.input(|i| i.stable_dt).min(0.05) * std::env::var("AIRSCOPE_SLOW").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0); // debug aid
        let k = 1.0 - (-dt * 18.0).exp(); // ~150 ms ease for node transitions
        let k_slow = 1.0 - (-dt * 5.0).exp(); // ~0.5 s ease for signal-driven moves
        if bg.dragged() && pan_ok {
            self.pan += bg.drag_delta();
            self.reset_view = false; // taking over cancels a running reset
        }
        if bg.double_clicked() && bg.interact_pointer_pos().map_or(false, in_reach) {
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
            self.gdrag = None;
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
            .filter(|(_, d)| on_radar(d))
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
                N { i, pos: c + vec2(a.cos(), a.sin()) * rmap(*sd, big).max(min_px(d)), ang: a.rem_euclid(TAU), dist: *sd, dim }
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
                if let Some(gd) = &self.gdrag {
                    // a dragged group stays together but does not merge with anything else until dropped
                    let (a, b) = (gd.contains(&self.latched[nodes[i].i].id), gd.contains(&self.latched[nodes[j].i].id));
                    if a != b {
                        continue;
                    }
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
            let center0 = { let v = center0 - c; let need = PC_R + 6.0 + 22.0; if v.length() < need { c + if v.length() > 1.0 { v / v.length() } else { vec2(0.0, -1.0) } * need } else { center0 } }; // the group icon (radius 22) clears the PC orb too
            let key = key_of[ri].clone().unwrap();
            if let Some(r) = &reveal {
                if mem.iter().any(|&i| &self.latched[nodes[i].i].id == r) {
                    self.open.insert(key.clone()); // a device picked from the list must be visible
                }
            }
            if std::env::var_os("AIRSCOPE_AUTO").is_some() && mem.iter().any(|&i| self.latched[nodes[i].i].kind == Kind::Bt && self.latched[nodes[i].i].connected) {
                // debug aid: open for 7 s, closed for 7 s
                if t > 4.0 && (t - 4.0).rem_euclid(14.0) < 7.0 {
                    self.open.insert(key.clone());
                } else {
                    self.open.remove(&key);
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

        // remember where each device is shown (a member of a collapsed group counts as sitting on its icon)
        for ni in 0..cnt {
            let (q, in_icon) = match group_of[ni] {
                Some(gi) if iconshown[gi] => (groups[gi].center, true),
                _ => (disp[ni], false),
            };
            self.last_world.insert(self.latched[nodes[ni].i].id.clone(), ((q - c) / big, in_icon));
        }

        // anything sitting under an open group's circle is obscured and not clickable
        let masked: Vec<bool> = (0..cnt).map(|i| !in_open[i] && groups.iter().filter(|g| g.open).any(|g| (disp[i] - g.center).length() < g.halo_r)).collect();

        // --- which labels fit (connected and strongest first)
        let mut allow: HashSet<usize> = Default::default();
        {
            // every dot that is actually drawn blocks labels, including members of a fanned-out group
            let mut placed: Vec<Rect> = (0..cnt).filter(|&i| vis1[i] || in_open[i]).map(|i| Rect::from_center_size(disp[i], Vec2::splat(2.0 * (node_rad(&self.latched[nodes[i].i]) * fscale[i] + 3.0)))).collect();
            placed.extend(groups.iter().enumerate().filter(|(gi, _)| iconshown[*gi]).map(|(_, g)| Rect::from_center_size(g.center, Vec2::splat(44.0))));
            placed.push(Rect::from_center_size(c, Vec2::splat(120.0)));
            let mut order: Vec<usize> = (0..cnt).filter(|&i| !hidden[i] && !masked[i] && self.latched[nodes[i].i].known).collect();
            order.sort_by(|&a, &b| {
                let (da, db) = (&self.latched[nodes[a].i], &self.latched[nodes[b].i]);
                db.connected.cmp(&da.connected).then(db.dbm.unwrap_or(-120.0).partial_cmp(&da.dbm.unwrap_or(-120.0)).unwrap())
            });
            for i in order {
                let g = p.layout_no_wrap(fit_middle(&p, &self.latched[nodes[i].i].name, FontId::proportional(11.5), LABEL_W), FontId::proportional(11.5), TEXT);
                let r = Rect::from_center_size(disp[i] + vec2(0.0, 30.0), g.size() + vec2(8.0, 4.0));
                if placed.iter().all(|q| !q.intersects(r)) {
                    placed.push(r);
                    allow.insert(i);
                }
            }
        }

        // labels fade in and out instead of popping as the layout changes
        let mut lab = vec![0.0f32; cnt];
        for i in 0..cnt {
            let e = self.label_a.entry(self.latched[nodes[i].i].id.clone()).or_insert(0.0);
            *e += ((if allow.contains(&i) { 1.0 } else { 0.0 }) - *e) * k;
            lab[i] = *e;
        }

        let view = rect.expand(70.0);
        let mut ev = Ev { clicked: None, over: vec![], drag_end: false, hover: None, dragging: None, gdrag: None };
        let sel_id = self.sel.clone();

        // --- links from the PC to connected Bluetooth devices, with travelling pulses
        for (ni, n) in nodes.iter().enumerate() {
            let d = &self.latched[n.i];
            if d.kind == Kind::Bt && d.connected && vis1[ni] && group_of[ni].is_none() {
                draw_link(&p, c, disp[ni], kind_col(d.kind), t, 1.0);
            }
        }
        // a group holding a connected Bluetooth device gets the same link
        for (gi, g) in groups.iter().enumerate() {
            let cm: Vec<Pos2> = g.members.iter().filter(|&&i| self.latched[nodes[i].i].kind == Kind::Bt && self.latched[nodes[i].i].connected).map(|&i| disp[i]).collect();
            if cm.is_empty() {
                continue;
            }
            if iconshown[gi] {
                draw_link(&p, c, g.center, kind_col(Kind::Bt), t, 1.0);
            } else if !g.open && !closing[gi] {
                group_member_links(&p, c, g.center, &cm, 0.0, t); // members still flying in
            }
        }

        // --- layer 1: everything that is not part of an open group
        for (ni, n) in nodes.iter().enumerate() {
            if !vis1[ni] || group_of[ni].map_or(false, |gi| closing[gi]) || !view.contains(disp[ni]) {
                continue;
            }
            let d = &self.latched[n.i];
            let interactive = !hidden[ni] && (!masked[ni] || Some(ni) == drag_ni);
            let sel = sel_id.as_deref() == Some(d.id.as_str());
            let sc = fscale[ni] * self.appear.get(&d.id).copied().unwrap_or(1.0);
            node_widget(ui, &p, d, n, ni, disp[ni], t, sw, sel, lab[ni], interactive, true, sc, c, big, &mut ev);
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
                let resp = hit(ui, Rect::from_center_size(g.center, Vec2::splat(46.0)), ui.scope_id().with(("gi", &g.key)), Sense::click_and_drag());
                hovered = resp.hovered();
                if resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    // move every member by the pointer movement (saved as their positions, like dragging one device)
                    let dl = resp.drag_delta();
                    for &i in &g.members {
                        let v = nodes[i].pos + dl - c;
                        ev.over.push((self.latched[nodes[i].i].id.clone(), (v.y.atan2(v.x), dmap(v.length().max(min_px(&self.latched[nodes[i].i])), big).clamp(0.6, DMAX))));
                    }
                    ev.gdrag = Some(g.members.iter().map(|&i| self.latched[nodes[i].i].id.clone()).collect());
                }
                if resp.drag_stopped() {
                    ev.drag_end = true;
                }
                if resp.clicked() {
                    toggle.push((g.key.clone(), true));
                }
            }
            let sc = self.gscale.entry(g.key.clone()).or_insert(0.0);
            *sc += (1.0 - *sc) * k;
            let sc = *sc;
            let r = 19.0 * (0.55 + 0.45 * sc);
            let worst = g.members.iter().map(|&i| self.latched[nodes[i].i].trouble).fold(0.0f32, f32::max);
            let warn = g.members.iter().map(|&i| self.latched[nodes[i].i].caution).fold(0.0f32, f32::max);
            if worst > 0.0 {
                trouble_halo(&p, g.center, r, worst, t, RED);
            } else if warn > 0.0 {
                trouble_halo(&p, g.center, r, warn, t, AMBER);
            }
            orb(&p, g.center, r, col, 0.2 + sweep_hot(c, g.center, sw) * 0.8 + if hovered { 0.7 } else { 0.0 });
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
        let pc_r = PC_R;
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
            if g.open || closing[gi] {
                let cm: Vec<Pos2> = g.members.iter().filter(|&&i| self.latched[nodes[i].i].kind == Kind::Bt && self.latched[nodes[i].i].connected).map(|&i| disp[i]).collect();
                group_member_links(&p, c, g.center, &cm, 27.0, t);
            }
            if !g.open {
                if closing[gi] {
                    // reverse of the fan-out: members glide back toward the icon, circle fading
                    for &i in &g.members {
                        p.line_segment([g.center, disp[i]], Stroke::new(1.2, alpha(CYAN, 0.5 * amt)));
                    }
                    for &i in &g.members {
                        let d = &self.latched[nodes[i].i];
                        let sel = sel_id.as_deref() == Some(d.id.as_str());
                        node_widget(ui, &p, d, &nodes[i], i, disp[i], t, sw, sel, 0.0, false, false, fscale[i], c, big, &mut ev);
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
                node_widget(ui, &p, d, &nodes[i], i, disp[i], t, sw, sel, lab[i], true, true, 1.0, c, big, &mut ev);
            }
            let kinds: Vec<Kind> = g.members.iter().map(|&i| self.latched[nodes[i].i].kind).collect();
            let col = if kinds.iter().all(|x| *x == kinds[0]) { kind_col(kinds[0]) } else { Color32::from_rgb(190, 212, 240) };
            let resp = hit(ui, Rect::from_center_size(g.center, Vec2::splat(40.0)), ui.scope_id().with(("gc", &g.key)), Sense::click());
            orb(&p, g.center, 11.0, col, 0.2 + sweep_hot(c, g.center, sw) * 0.8 + if resp.hovered() { 0.7 } else { 0.0 });
            p.line_segment([g.center - vec2(5.0, 0.0), g.center + vec2(5.0, 0.0)], Stroke::new(2.2, shade(col, 0.15)));
            if resp.clicked() {
                toggle.push((g.key.clone(), false));
            }
        }

        // --- bursts (a device left the air)
        // the device fades out as the ring expands from where it was
        self.bursts.retain(|b| t - b.t0 < 0.6);
        for b in &self.bursts {
            let age = (t - b.t0) / 0.6;
            let pos = c + b.w * big;
            let fade = (1.0 - age / 0.5).clamp(0.0, 1.0);
            if fade > 0.0 && b.ghost {
                let mut pg = p.clone();
                pg.set_opacity(fade);
                if b.known {
                    orb(&pg, pos, b.rad, b.col, 0.0);
                } else {
                    dot(&pg, pos, b.rad, b.col, 0.0);
                }
            }
            p.circle_stroke(pos, b.rad * 0.5 + age * 46.0, Stroke::new(2.0, add(b.col, 0.6 * (1.0 - age))));
        }

        // --- hover pills (always on top)
        if let Some(ni) = ev.hover {
            let d = &self.latched[nodes[ni].i];
            let sub = match d.dbm {
                Some(x) => format!("{} · {:.0} dBm", d.vendor, x),
                None => d.vendor.clone(),
            };
            let tc = TipC { anchor: disp[ni] + vec2(0.0, -10.0), col: kind_col(d.kind), title: d.name.clone(), sub: vec![sub], hint: Some("Click for details · Drag to move".into()), side: None };
            self.tip_want = Some(tc);
        } else if let Some(gi) = group_hover {
            let g = &groups[gi];
            let names: Vec<String> = g.members.iter().take(4).map(|&i| self.latched[nodes[i].i].name.clone()).collect();
            let more = if g.members.len() > 4 { format!(" +{}", g.members.len() - 4) } else { String::new() };
            let tc = TipC { anchor: g.center + vec2(0.0, -20.0), col: CYAN, title: format!("{} devices close together", g.members.len()), sub: vec![format!("{}{}", names.join(", "), more)], hint: Some("Click to expand · Drag to move".into()), side: None };
            self.tip_want = Some(tc);
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
        if let Some(g) = ev.gdrag {
            self.gdrag = Some(g);
        }
        if ev.drag_end {
            self.dragging = None;
            self.gdrag = None;
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
                    self.radar_anchor = Some((id, disp[ni])); // the shared info box follows the dot
                }
            }
        }

        // --- zoom HUD
        let hud = Rect::from_min_size(pos2(rect.left() + 18.0, rect.bottom() - 48.0), vec2(170.0, 34.0));
        panel(&p, hud, 17.0, CYAN);
        lcd(&p, pos2(hud.left() + 18.0, hud.center().y), Align2::LEFT_CENTER, &format!("ZOOM {:.1}x", self.zoom), 12.0, CYAN);
        let rb = Rect::from_center_size(pos2(hud.right() - 28.0, hud.center().y), vec2(42.0, 22.0));
        let rr = hit(ui, rb, ui.scope_id().with("reset_view"), Sense::click());
        p.rect_filled(rb, 9.0, alpha(CYAN, if rr.hovered() { 0.35 } else { 0.15 }));
        glyph(&p, Glyph::Refresh, rb.center(), 6.5, Color32::WHITE);
        let rr = rr.hint("Reset zoom and position");
        if rr.clicked() {
            self.reset_view = true;
        }
        self.tray(ui, tray_area);
    }

    /// Side tray: devices that are not on the radar (not connected, or no wireless signal), grouped by kind of
    /// device and sorted by name, two to a row with the name under the orb. Click one for its info box; they
    /// cannot be dragged or reordered.
    fn tray(&mut self, ui: &mut egui::Ui, area: Rect) {
        const CELL_H: f32 = 72.0;
        const HEAD_H: f32 = 28.0;
        let p = ui.painter_at(area.expand(40.0));
        panel(&p, area, 14.0, CYAN);
        lcd(&p, pos2(area.left() + 16.0, area.top() + 24.0), Align2::LEFT_CENTER, "NOT CONNECTED/NO SIGNAL", 10.5, CYAN);
        let mut items: Vec<(&'static str, String, usize)> = self.latched.iter().enumerate().filter(|(_, d)| !on_radar(d)).map(|(i, d)| (tray_group(d), d.name.to_lowercase(), i)).collect();
        items.sort();
        let view = Rect::from_min_max(pos2(area.left() + 8.0, area.top() + 44.0), pos2(area.right() - 8.0, area.bottom() - 10.0));
        if items.is_empty() {
            p.text(view.center_top() + vec2(0.0, 30.0), Align2::CENTER_CENTER, "Everything is on the radar", FontId::proportional(12.0), DIM);
            return;
        }
        // groups in order: (heading, indexes into `items`)
        let mut groups: Vec<(&'static str, Vec<usize>)> = vec![];
        for (k, (g, _, _)) in items.iter().enumerate() {
            match groups.last_mut() {
                Some((lg, v)) if lg == g => v.push(k),
                _ => groups.push((g, vec![k])),
            }
        }
        let content: f32 = groups.iter().map(|(_, v)| HEAD_H + v.len().div_ceil(2) as f32 * CELL_H).sum();
        let max = (content - view.height()).max(0.0);
        let sid = egui::Id::new("tray_scroll");
        let mut scroll: f32 = ui.ctx().data(|d| d.get_temp(sid)).unwrap_or(0.0);
        if max > 0.0 && ui.input(|i| i.pointer.hover_pos()).map_or(false, |q| view.contains(q)) {
            scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        scroll = scroll.clamp(0.0, max);
        ui.ctx().data_mut(|d| d.insert_temp(sid, scroll));
        let clip = ui.painter_at(view);
        let inner_w = view.width() - if max > 0.0 { 9.0 } else { 0.0 };
        let cell_w = inner_w / 2.0;
        let mut y = view.top() - scroll;
        let (mut click, mut anchor, mut hover) = (None, None, None::<(Pos2, String)>);
        for (g, members) in &groups {
            clip.text(pos2(view.left() + 8.0, y + 14.0), Align2::LEFT_CENTER, format!("{} ({})", g.to_uppercase(), members.len()), FontId::monospace(10.5), alpha(CYAN, 0.8));
            y += HEAD_H;
            for (n, k) in members.iter().enumerate() {
                let r = Rect::from_min_size(pos2(view.left() + (n % 2) as f32 * cell_w, y + (n / 2) as f32 * CELL_H), vec2(cell_w, CELL_H - 2.0));
                if r.bottom() < view.top() || r.top() > view.bottom() {
                    continue;
                }
                let d = &self.latched[items[*k].2];
                let resp = hit(ui, r.intersect(view), ui.scope_id().with(("tray", &d.id)), Sense::click());
                let sel = self.sel.as_deref() == Some(d.id.as_str());
                let col = if d.online { kind_col(d.kind) } else { shade(kind_col(d.kind), 0.55) };
                if resp.hovered() || sel {
                    clip.rect_filled(r.shrink(2.0), 8.0, alpha(col, if sel { 0.22 } else { 0.12 }));
                    if sel {
                        clip.rect_stroke(r.shrink(2.0), 8.0, Stroke::new(1.0, alpha(col, 0.7)), StrokeKind::Inside);
                    }
                }
                let ic = pos2(r.center().x, r.top() + 24.0);
                if d.known {
                    orb(&clip, ic, 17.0, col, 0.0);
                    icon(&clip, d.icon, ic, 8.5, shade(col, 0.12));
                } else {
                    dot(&clip, ic, 8.0, col, 0.0);
                }
                let label = fit_middle(&clip, &d.name, FontId::proportional(11.0), cell_w - 8.0);
                clip.text(pos2(r.center().x, r.top() + 56.0), Align2::CENTER_CENTER, label, FontId::proportional(11.0), alpha(TEXT, if d.online { 0.95 } else { 0.55 }));
                if resp.hovered() {
                    hover = Some((pos2(r.center().x, r.top() + 56.0), d.name.clone()));
                }
                if resp.clicked() {
                    click = Some(d.id.clone());
                }
                if sel {
                    anchor = Some((d.id.clone(), pos2(r.left(), r.center().y)));
                }
            }
            y += members.len().div_ceil(2) as f32 * CELL_H;
        }
        if let Some((at, name)) = hover {
            let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tray_hover")));
            hover_label(&top, at, &name, FontId::proportional(11.0));
        }
        if max > 0.0 {
            scrollbar(&p, Rect::from_min_max(pos2(view.right() - 5.0, view.top()), pos2(view.right(), view.bottom())), scroll, max, view.height(), content, CYAN);
        }
        if let Some(id) = click {
            self.sel = if self.sel.as_deref() == Some(id.as_str()) { None } else { Some(id) };
        }
        if let Some(a) = anchor {
            self.radar_anchor = Some(a); // the shared info box opens beside the tray cell
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
            let e = m.entry((name.clone(), ch)).or_insert(Net { id: d.id.clone(), name, chan: ch, f: f as f32, dbm, aps: 0, mine, hidden });
            e.aps += 1;
            e.mine |= mine;
            if dbm > e.dbm {
                e.dbm = dbm;
                e.id = d.id.clone();
            }
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
    /// Returns (y after the rows, hovered row, clicked row). Every row has the same full-width hit area.
    #[allow(clippy::too_many_arguments)]
    fn bt_rows(&self, ui: &egui::Ui, p: &egui::Painter, label_x: f32, xl: &dyn Fn(usize) -> f32, cw: f32, mut y: f32, row_x: (f32, f32), crowd: &dyn Fn(usize) -> f32) -> (f32, Option<usize>, Option<usize>) {
        let (mut hov, mut clk) = (None, None);
        for (li, l) in self.links.iter().enumerate() {
            let row = Rect::from_min_max(pos2(row_x.0, y - 3.0), pos2(row_x.1, y + 21.0));
            let resp = hit(ui, row, ui.scope_id().with(("btrow", &l.mac)), Sense::click());
            if resp.hovered() {
                hov = Some(li);
                p.rect_filled(row, 8.0, alpha(CYAN, 0.08));
                p.rect_stroke(row, 8.0, Stroke::new(1.0, alpha(CYAN, 0.5)), StrokeKind::Inside);
            }
            if resp.clicked() {
                clk = Some(li);
            }
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
            y += 26.0;
        }
        (y, hov, clk)
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

        let legend_w = (rect.width() * 0.22).clamp(150.0, 300.0); // legend shrinks first on narrow windows so the graph keeps its width
        let n_links = self.links.len().max(1) as f32;
        let has_bt = !self.links.is_empty();
        let tail = 26.0 + if has_bt { 34.0 + 26.0 + n_links * 26.0 + 22.0 } else { 26.0 } + 14.0; // content below the axis, plus bottom padding
        let gh = (rect.height() - 74.0 - 40.0 - tail - 150.0).clamp(150.0, 330.0);
        let gx0 = l + 130.0;
        let gx1 = rect.right() - legend_w - 60.0;
        let gw = gx1 - gx0;
        let base = rect.top() + 74.0 + gh;
        let fx = |f: f32| gx0 + (f - 2399.0) / 87.0 * gw;
        let frame = Rect::from_min_max(pos2(l, rect.top() + 14.0), pos2(gx1 + 24.0, base + tail));
        panel(&p, frame, 14.0, CYAN);
        let n24 = nets.iter().filter(|n| !n.hidden || n.aps > 0).map(|n| n.aps.max(1) as usize).sum::<usize>();
        lcd(&p, pos2((gx0 + gx1) / 2.0, rect.top() + 36.0), Align2::CENTER_CENTER, &format!("2.4 GHZ WI-FI NETWORKS WITHIN RANGE: {}", n24), 12.0, AMBER);
        // gridlines + axes
        for (dbm, lbl) in [(-80.0, "-80"), (-60.0, "-60"), (-40.0, "-40"), (-20.0, "-20")] {
            let y = base - data::signal_frac(dbm) * (gh - 20.0);
            p.line_segment([pos2(gx0, y), pos2(gx1, y)], Stroke::new(1.0, alpha(CYAN, 0.07)));
            p.text(pos2(gx0 - 6.0, y), Align2::RIGHT_CENTER, lbl, FontId::monospace(9.5), DIM);
        }
        p.text(pos2(gx0 - 6.0, base - (gh - 20.0) - 12.0), Align2::RIGHT_CENTER, "dBm", FontId::monospace(9.5), CYAN); // the unit, at the top of the number column
        for n in 1..=13 {
            let f = 2407.0 + 5.0 * n as f32;
            p.line_segment([pos2(fx(f), rect.top() + 72.0), pos2(fx(f), base)], Stroke::new(1.0, alpha(CYAN, if [1, 6, 11].contains(&n) { 0.16 } else { 0.06 })));
            p.text(pos2(fx(f), base + 9.0), Align2::CENTER_CENTER, n.to_string(), FontId::monospace(11.0), if [1, 6, 11].contains(&n) { CYAN } else { DIM });
        }
        p.text(pos2(gx0 - 6.0, base + 9.0), Align2::RIGHT_CENTER, "Wi-Fi ch", FontId::monospace(9.5), CYAN);
        p.line_segment([pos2(gx0, base), pos2(gx1, base)], Stroke::new(1.5, alpha(CYAN, 0.35)));

        // hover target: from the legend, else from the graph
        let ptr = ui.input(|i| i.pointer.hover_pos());
        let mut pick: Option<(String, Pos2)> = None;
        let hh = |n: &Net, dx: f32| n.strength() * (gh - 20.0) * hill(dx);
        // peak labels (strongest first, shifted up when they would collide, cut in the middle so they stay inside the graph)
        let mut labels: Vec<(usize, Rect, String, bool)> = vec![];
        {
            let mut placed: Vec<Rect> = vec![];
            for (i, n) in nets.iter().enumerate().filter(|(_, n)| !n.hidden) {
                let full = if n.mine { format!("{} · yours", n.name) } else { n.name.clone() };
                let cx = fx(n.f);
                let max_w = (((cx - gx0).min(gx1 - cx)) * 2.0 - 12.0).max(60.0);
                let shown = fit_middle(&p, &full, FontId::proportional(11.5), max_w);
                let sz = p.layout_no_wrap(shown.clone(), FontId::proportional(11.5), Color32::WHITE).size() + vec2(10.0, 4.0);
                let mut y = base - hh(n, 0.0) - 12.0;
                let mut r = Rect::from_center_size(pos2(cx, y), sz);
                let mut tries = 0;
                while placed.iter().any(|q| q.intersects(r)) && tries < 6 {
                    y -= 15.0;
                    r = Rect::from_center_size(pos2(cx, y), sz);
                    tries += 1;
                }
                if r.top() < rect.top() + 66.0 {
                    continue;
                }
                if tries < 6 {
                    placed.push(r);
                }
                labels.push((i, r, shown, tries >= 6));
            }
        }
        let tile_id = egui::Id::new("spectrum_tile_hover");
        let mut hover: Option<usize> = ui.ctx().data(|d| d.get_temp::<Option<usize>>(tile_id)).flatten(); // a dot in a tile (seen last frame) lights its hill like the list does
        let lx = gx1 + 44.0;
        let card = Rect::from_min_max(pos2(lx - 12.0, frame.top()), pos2(rect.right() - 18.0, frame.bottom()));
        panel(&p, card, 14.0, CYAN);
        let ly0 = card.top() + 12.0;
        let max_rows = (((card.height() - 24.0) / 24.0) as usize).max(1);
        // legend: every network, scrollable when there are more than fit
        let lview = Rect::from_min_max(pos2(lx, ly0), pos2(card.right() - 14.0, ly0 + max_rows as f32 * 24.0));
        let lmax = (nets.len() as f32 - max_rows as f32).max(0.0) * 24.0;
        let lid = egui::Id::new("spectrum_legend_scroll");
        let mut loff: f32 = ui.ctx().data(|d| d.get_temp(lid)).unwrap_or(0.0);
        if lmax > 0.0 && ptr.map_or(false, |q| lview.contains(q)) {
            loff -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        loff = loff.clamp(0.0, lmax);
        ui.ctx().data_mut(|d| d.insert_temp(lid, loff));
        let mut legend: Vec<(Rect, usize)> = vec![];
        for i in 0..nets.len() {
            let r = Rect::from_min_size(pos2(lx, ly0 + i as f32 * 24.0 - loff), vec2(lview.width(), 22.0));
            if r.bottom() > lview.top() && r.top() < lview.bottom() {
                legend.push((r, i));
            }
        }
        let mut from_legend = false;
        for (r, i) in &legend {
            if hit(ui, r.intersect(lview), ui.scope_id().with(("lg", *i)), Sense::click()).hovered() {
                hover = Some(*i);
                from_legend = true;
            }
        }
        // hills under the cursor, nearest outline first (ties: the stronger network), so a hill hidden
        // inside a bigger one is still reachable by pointing near its own edge
        if hover.is_none() {
            if let Some(q) = ptr {
                hover = labels.iter().find(|l| !l.3 && l.1.contains(q)).map(|l| l.0);
            }
        }
        let mut under: Vec<usize> = vec![];
        if hover.is_none() {
            if let Some(q) = ptr.filter(|q| q.x > gx0 && q.x < gx1 && q.y < base && q.y > rect.top() + 72.0) {
                let f = 2399.0 + (q.x - gx0) / gw * 87.0;
                let mut c: Vec<(usize, f32)> = nets.iter().enumerate().filter_map(|(i, n)| {
                    let depth = hh(n, f - n.f) - (base - q.y);
                    (depth >= 0.0).then_some((i, depth))
                }).collect();
                c.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
                under = c.into_iter().map(|x| x.0).collect();
                hover = under.first().copied();
            }
        }
        if let Some(h) = hover {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if let Some(q) = ptr.filter(|q| ui.input(|i| i.pointer.primary_clicked()) && ui.ctx().layer_id_at(*q) == Some(ui.layer_id())) {
                pick = Some((nets[h].id.clone(), q));
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
        // labels above the peaks (placed before hover detection; drawn here)
        for (i, r, shown, crowded) in &labels {
            if *crowded && hover != Some(*i) {
                continue;
            }
            let n = &nets[*i];
            let col = col_of(n);
            let g = p.layout_no_wrap(shown.clone(), FontId::proportional(11.5), Color32::WHITE);
            p.rect_filled(*r, 6.0, Color32::from_rgba_unmultiplied(6, 12, 26, 190));
            p.rect_stroke(*r, 6.0, Stroke::new(1.0, alpha(col, if hover.map_or(true, |h| h == *i) { 0.9 } else { 0.3 })), StrokeKind::Inside);
            p.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
        }
        // Bluetooth channels aligned under the same frequency axis
        let bcw = gw / 87.0;
        let crowd = |i: usize| (Self::load_at(&nets, 2402.0 + i as f32) / 2.2).clamp(0.0, 1.0);
        let mut tip_box: Option<(Pos2, String, Vec<String>)> = None;
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
            yc += 34.0;
        }
        let title = if has_bt { format!("CONNECTED BLUETOOTH DEVICES: {}", self.links.len()) } else { "NO BLUETOOTH DEVICES".to_string() };
        let mid = (gx0 + gx1) / 2.0;
        lcd(&p, pos2(mid, yc + 8.0), Align2::CENTER_CENTER, &title, 12.0, BLUE);
        if has_bt {
            let (yend, bt_hover, bt_click) = self.bt_rows(ui, &p, gx0 - 12.0, &|i| fx(2401.5 + i as f32), bcw, yc + 26.0, (l + 12.0, gx1 + 12.0), &crowd);
            if let (Some(i), Some(q)) = (bt_click, ptr) {
                pick = Some((self.links[i].mac.clone(), q));
            }
            if let (Some(i), Some(q)) = (bt_hover, ptr) {
                let l = &self.links[i];
                let n = l.afh.as_ref().map_or(79, |m| m.iter().filter(|x| **x).count());
                let hot = (0..79).filter(|&c| l.afh.as_ref().map_or(true, |m| m[c]) && crowd(c) > 0.5).count();
                let mut lines = vec![match &l.afh {
                    Some(_) => format!("Adaptive frequency hopping: uses {} of the 79 channels and steers around the {} it found noisy or blocked.", n, 79 - n),
                    None => "No AFH (adaptive frequency hopping): this device can't avoid busy channels, so it hops across all 79, including every one Wi-Fi is using. It is the most exposed to Wi-Fi interference.".to_string(),
                }];
                lines.push(format!("Estimate: {} of its {} channels sit under busy Wi-Fi right now.", hot, n));
                if let Some(lq) = l.lq {
                    lines.push(format!("Link quality {}% · {}", data::lq_pct(lq), if l.mfr.is_empty() { l.mac.clone() } else { format!("{} · {}", l.mfr, l.mac) }));
                }
                tip_box = Some((q, if l.name.is_empty() { l.mac.clone() } else { l.name.clone() }, lines));
            }
            let _ = yend;
        }

        // legend
        let pl = p.with_clip_rect(lview.expand2(vec2(4.0, 0.0)));
        for (r, i) in &legend {
            let n = &nets[*i];
            let col = col_of(n);
            if hover == Some(*i) {
                pl.rect_filled(*r, 6.0, alpha(col, 0.16));
            }
            dot(&pl, pos2(r.left() + 10.0, r.center().y), 5.0, col, 0.0);
            let nm = if n.mine { format!("{} · yours", n.name) } else if n.aps > 1 && n.hidden { format!("{} ×{}", n.name, n.aps) } else { n.name.clone() };
            let info = pl.layout_no_wrap(format!("ch{} {:.0}", n.chan, n.dbm), FontId::monospace(10.5), DIM);
            let nm = fit_middle(&pl, &nm, FontId::proportional(12.0), r.width() - 24.0 - 12.0 - info.size().x - 8.0); // full name shows in the hover box
            pl.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, nm, FontId::proportional(12.0), if n.hidden { DIM } else { TEXT });
            pl.galley(pos2(r.right() - 12.0 - info.size().x, r.center().y - info.size().y / 2.0), info, DIM);
        }
        if lmax > 0.0 {
            scrollbar(&p, Rect::from_min_max(pos2(lview.right() - 5.0, lview.top()), pos2(lview.right(), lview.bottom())), loff, lmax, lview.height(), lview.height() + lmax, CYAN);
        }
        // hover tooltip
        if let (Some(h), Some(q)) = (hover, ptr) {
            let n = &nets[h];
            let ov: Vec<&Net> = nets.iter().enumerate().filter(|(j, o)| *j != h && (o.f - n.f).abs() < 22.0 && o.dbm > -85.0).map(|(_, o)| o).collect();
            let same = ov.iter().filter(|o| (o.f - n.f).abs() < 1.0).count();
            let sub = format!("ch {} · {:.0} dBm · {} on same channel, {} partly overlapping", n.chan, n.dbm, same, ov.len() - same);
            let mut subs = vec![sub];
            let stacked = under.len() > 1 && under.first() == Some(&h);
            if stacked {
                // other hills stacked under the same spot (point nearer a hill's edge to pick it)
                let names: Vec<String> = under.iter().skip(1).take(4).map(|j| if nets[*j].name.is_empty() { "(hidden)".to_string() } else { nets[*j].name.clone() }).collect();
                let more = under.len() - 1 - names.len();
                subs.push(format!("Also here: {}{}", names.join(", "), if more > 0 { format!(" +{}", more) } else { String::new() }));
            }
            let title = format!("{}{}", n.name, if n.aps > 1 { format!(" ({} access points)", n.aps) } else { String::new() });
            self.tip_want = Some(if from_legend {
                // beside the legend, level with the pointer, so the rows above and below stay readable
                TipC { anchor: pos2(0.0, q.y), col: col_of(n), title, sub: subs, hint: None, side: Some(lview.left() - 14.0) }
            } else {
                TipC { anchor: pos2(q.x.clamp(rect.left() + 200.0, rect.right() - 200.0), q.y), col: col_of(n), title, sub: subs, hint: None, side: None }
            });
        }

        // your network, drawn: where it sits and how busy each clean channel is (no explanation text)
        let cy = frame.bottom() + 12.0;
        let pr = Rect::from_min_max(pos2(l, cy), pos2(rect.right() - 24.0, (cy + 112.0).min(rect.bottom() - 8.0)));
        panel(&p, pr, 12.0, GREEN);
        lcd(&p, pos2(pr.left() + 14.0, pr.top() + 16.0), Align2::LEFT_CENTER, "YOUR NETWORK", 11.5, GREEN);
        let mut yn_tip: Option<TipC> = None;
        let mut tile_h: Option<usize> = None;
        let hq = ui.input(|i| i.pointer.hover_pos()).unwrap_or(pr.center());
        let mine = nets.iter().find(|n| n.mine && !n.name.contains("Guest")).or_else(|| nets.iter().find(|n| n.mine));
        match mine {
            None => {
                p.text(pos2(pr.left() + 14.0, pr.top() + 48.0), Align2::LEFT_CENTER, "not on 2.4 GHz", FontId::proportional(13.0), DIM);
            }
            Some(mn) => {
                let nm = fit_middle(&p, &mn.name, FontId::proportional(15.0), 220.0);
                p.text(pos2(pr.left() + 14.0, pr.top() + 46.0), Align2::LEFT_CENTER, nm, FontId::proportional(15.0), Color32::WHITE);
                lcd(&p, pos2(pr.left() + 14.0, pr.top() + 78.0), Align2::LEFT_CENTER, &format!("CH {}", mn.chan), 26.0, AMBER);
            }
        }
        let chans: [u32; 3] = [1, 6, 11]; // the only 2.4 GHz channels that don't overlap each other; any other channel overlaps at least one of them
        let chan_f = |c: u32| 2407.0 + 5.0 * c as f32;
        let loads: Vec<f32> = chans.iter().map(|&c| (Self::load_at(&nets, chan_f(c)) / 2.2).clamp(0.0, 1.0)).collect();
        let (gap, x0) = (12.0, pr.left() + 250.0);
        let tw = ((pr.right() - 14.0 - x0 - gap * (chans.len() as f32 - 1.0)) / chans.len() as f32).max(110.0);
        for (i, &c) in chans.iter().enumerate() {
            let tr = Rect::from_min_size(pos2(x0 + i as f32 * (tw + gap), pr.top() + 12.0), vec2(tw, pr.height() - 24.0));
            let load = loads[i];
            let col = if load < 0.33 { GREEN } else if load < 0.66 { AMBER } else { RED };
            panel(&p, tr, 10.0, col);
            lcd(&p, pos2(tr.left() + 12.0, tr.top() + 18.0), Align2::LEFT_CENTER, &format!("CH {c}"), 13.0, col);
            led_bar(&p, Rect::from_min_size(pos2(tr.left() + 12.0, tr.top() + 38.0), vec2(tr.width() - 24.0, 9.0)), load, col, 14);
            // one dot per other network heard on or overlapping this channel (solid = same channel, ring = partly)
            let f = chan_f(c);
            let mut others: Vec<&Net> = nets.iter().filter(|o| (o.mine || o.dbm > -85.0) && (o.f - f).abs() < 22.0).collect(); // all of your networks (main, guest) are bigger dots in every tile they touch
            others.sort_by(|a, b| b.mine.cmp(&a.mine).then(b.dbm.partial_cmp(&a.dbm).unwrap_or(std::cmp::Ordering::Equal))); // yours first
            let room = (((tr.width() - 24.0) / 17.0) as usize).max(1);
            for (k, o) in others.iter().take(room).enumerate() {
                let cc = pos2(tr.left() + 18.0 + k as f32 * 17.0, tr.bottom() - 20.0);
                let same_ch = (o.f - f).abs() < 1.0;
                let rad = if o.mine { 7.0 } else { 5.0 }; // yours is the bigger dot
                if same_ch {
                    dot(&p, cc, rad, col_of(o), 0.0); // glowing, like the list on the right: on exactly this channel
                } else {
                    p.circle_filled(cc, rad - 0.5, alpha(col_of(o), if o.mine { 0.7 } else { 0.4 })); // dimmer: only partly overlapping
                }
                let nm = if o.name.is_empty() { "(unnamed)".to_string() } else if o.mine { format!("{} (yours)", o.name) } else { o.name.clone() };
                // hit area: the whole 17 px slot, 30 px tall (the dot itself stays small); a faint halo shows which dot is meant
                let slot = Rect::from_center_size(cc, vec2(17.0, 30.0)).intersect(tr);
                if ui.interact(slot, ui.id().with(("chan_dot", c, k)), Sense::hover()).hovered() {
                    p.circle_filled(cc, rad + 4.0, alpha(col_of(o), 0.16));
                    tile_h = nets.iter().position(|n| std::ptr::eq(n, *o));
                    yn_tip = Some(TipC { anchor: hq, col: col_of(o), title: nm.clone(), sub: vec![format!("ch {} · {:.0} dBm · {}", o.chan, o.dbm, if same_ch { "on this channel" } else { "partly overlaps" })], hint: None, side: None });
                }
            }
            if others.len() > room {
                p.text(pos2(tr.right() - 12.0, tr.bottom() - 20.0), Align2::RIGHT_CENTER, format!("+{}", others.len() - room), FontId::proportional(11.0), DIM);
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(tile_id, tile_h));
        if let Some(t) = yn_tip {
            self.tip_want = Some(t);
        }
        if let Some((q, title, lines)) = tip_box {
            self.tip_want = Some(TipC { anchor: q, col: BLUE, title, sub: lines, hint: None, side: None });
        }
        if let Some((id, q)) = pick {
            self.sel = if self.sel.as_deref() == Some(id.as_str()) { None } else { Some(id) };
            self.anchor = q;
        }
    }

    // ---------- devices ----------

    fn devices(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        // --- filter + group
        let q = self.q.to_lowercase();
        let groups: [(&str, &str, Color32); 5] = [
            ("YOURS", "Bluetooth devices", BLUE),
            ("YOURS", "Network devices (LAN)", GREEN),
            ("YOURS", "Wi-Fi networks", AMBER),
            ("NEARBY · not yours", "Wi-Fi networks", AMBER),
            ("NEARBY · not yours", "Unnamed access points", DIM),
        ];
        let mut items: Vec<Vec<usize>> = vec![vec![]; 5];
        for (i, d) in self.devs.iter().enumerate() {
            let mine = self.is_mine(d);
            let ok = self.show[d.kind as usize]
                && !(self.hide_unnamed && !d.known)
                && (q.is_empty() || d.name.to_lowercase().contains(&q) || d.vendor.to_lowercase().contains(&q) || d.id.to_lowercase().contains(&q) || d.details.iter().any(|(_, v)| v.to_lowercase().contains(&q)));
            if !ok {
                continue;
            }
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
        let yours: usize = items[..3].iter().map(|v| v.len()).sum();
        let nearby: usize = items[3..].iter().map(|v| v.len()).sum();
        let total = if self.scope == 0 { yours } else { nearby };
        if self.scope == 0 {
            items[3].clear();
            items[4].clear();
        } else {
            for v in &mut items[..3] {
                v.clear();
            }
        }
        // --- toolbar: search on top; below it the tabs (Yours / Nearby) and the filters, the same layout as Diagnostics
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("Search name, vendor, MAC…").desired_width(320.0).margin(vec2(12.0, 10.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("Showing {} of {} devices", total, self.devs.len())).size(12.0).color(DIM));
                });
            });
            ui.add_space(10.0);
            // filters: right of the tabs
            let (show, hide) = (&mut self.show, &mut self.hide_unnamed);
            let mut chips = |ui: &mut egui::Ui, rev: bool| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let mut all = vec![(0usize, "Wi-Fi", AMBER), (1, "Bluetooth", BLUE), (2, "LAN", GREEN), (3, "Hide unnamed", MAGENTA)];
                if rev {
                    all.reverse();
                }
                for (k, lbl, col) in all {
                    let on = if k == 3 { *hide } else { show[k] };
                    let what = match k { 0 => "Wi-Fi networks", 1 => "Bluetooth devices", 2 => "network (LAN) devices", _ => "devices without a name" };
                    let tip = match (k == 3, on) {
                        (false, true) => format!("Showing {what}. Click to hide them."),
                        (false, false) => format!("Hiding {what}. Click to show them."),
                        (true, true) => format!("Hiding {what}. Click to show them."),
                        (true, false) => format!("Showing {what}. Click to hide them."),
                    };
                    if toggle_chip(ui, lbl, on, col, &tip) {
                        if k == 3 { *hide = !*hide } else { show[k] = !show[k] }
                    }
                }
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (i, l) in [format!("Yours  ·  {yours}"), format!("Nearby (not yours)  ·  {nearby}")].iter().enumerate() {
                    if view_tab(ui, l, self.scope == i as u8) {
                        self.scope = i as u8;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| chips(ui, true));
            });
            ui.separator();
        });
        // --- list
        let mut pick: Option<(String, Pos2)> = None;
        let mut toggle = None;
        let collapsed = self.collapsed.clone();
        let devs = &self.devs;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width() - 34.0;
            for (g, (_section, title, col)) in groups.iter().enumerate() {
                if items[g].is_empty() {
                    continue;
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
                    panel(p, r, 12.0, if resp.hovered() { CYAN } else if d.trouble > 0.0 { RED } else if d.caution > 0.0 { AMBER } else { col });
                    if d.known {
                        orb(p, pos2(r.left() + 34.0, r.center().y), 15.0, col, 0.0);
                        icon(p, d.icon, pos2(r.left() + 34.0, r.center().y), 7.5, shade(col, 0.12));
                    } else {
                        dot(p, pos2(r.left() + 34.0, r.center().y), 6.0, col, 0.0);
                    }
                    p.text(pos2(r.left() + 66.0, r.center().y - 9.0), Align2::LEFT_CENTER, &d.name, FontId::proportional(15.0), Color32::WHITE);
                    let kindname = d.details.first().map(|x| x.1.clone()).unwrap_or_default();
                    p.text(pos2(r.left() + 66.0, r.center().y + 10.0), Align2::LEFT_CENTER, format!("{} · {}", kindname, d.vendor), FontId::proportional(12.0), DIM);
                    // right edge inwards: CONNECTED label, one strength bar (no units: the info box has them), problem chip
                    let mut x = r.right() - 20.0;
                    if d.connected && d.dbm.is_none() {
                        p.text(pos2(x, r.center().y), Align2::RIGHT_CENTER, "CONNECTED", FontId::monospace(12.0), GREEN);
                        x -= 98.0;
                    }
                    let strength = d.dbm.map(data::signal_frac).or_else(|| data::bt_quality(d.lq, d.rssi));
                    if let Some(f) = strength {
                        led_bar(p, Rect::from_min_size(pos2(x - 130.0, r.center().y - 5.0), vec2(130.0, 10.0)), f, col, 13);
                        x -= 146.0;
                    }
                    if d.trouble > 0.0 || d.caution > 0.0 {
                        let (acc, word) = if d.trouble > 0.0 { (RED, "ISSUE") } else { (AMBER, "WARNING") };
                        // clear problem marker; click the row for what the app found out
                        let chip = Rect::from_min_max(pos2(x - 74.0, r.center().y - 11.0), pos2(x, r.center().y + 11.0));
                        p.rect_filled(chip, 11.0, alpha(acc, 0.18));
                        p.rect_stroke(chip, 11.0, Stroke::new(1.2, alpha(acc, 0.8)), StrokeKind::Inside);
                        dot(p, pos2(chip.left() + 11.0, chip.center().y), 3.5, acc, 0.3);
                        p.text(pos2(chip.left() + 21.0, chip.center().y), Align2::LEFT_CENTER, word, FontId::monospace(11.5), acc);
                    }
                    ui.add_space(8.0);
                    if resp.clicked() {
                        pick = Some((d.id.clone(), resp.interact_pointer_pos().unwrap_or(r.center())));
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
        if let Some((id, q)) = pick {
            self.sel = if self.sel.as_deref() == Some(id.as_str()) { None } else { Some(id) };
            self.anchor = q;
        }
    }
}

/// Hover text for any widget, drawn with the shared popup at the pointer (instead of egui's own tooltip).
/// "Title. Details." splits into a title and a wrapped line; one long sentence goes in the wrapped line.
pub trait Hint {
    fn hint(self, text: impl Into<String>) -> egui::Response;
}
impl Hint for egui::Response {
    fn hint(self, text: impl Into<String>) -> egui::Response {
        if self.hovered() {
            let text: String = text.into();
            let (title, rest) = match text.split_once(". ") {
                Some((a, b)) if a.chars().count() <= 36 => (a.to_string(), b.to_string()),
                _ => (text.clone(), String::new()), // one sentence: all title, wrapped when long
            };
            let anchor = self.ctx.input(|i| i.pointer.hover_pos()).unwrap_or(self.rect.center_bottom());
            let c = TipC { anchor, col: CYAN, title, sub: if rest.is_empty() { vec![] } else { vec![rest] }, hint: None, side: None };
            self.ctx.data_mut(|d| d.insert_temp(egui::Id::new("hint_want"), Some(c)));
        }
        self
    }
}

#[derive(Clone)]
struct TipC {
    hint: Option<String>, // how to interact, shown last under a divider (dim)
    anchor: Pos2,
    col: Color32,
    title: String,
    sub: Vec<String>,
    side: Option<f32>, // boxed only: put the box to the left of this x, centred on anchor.y
}

fn draw_tip(p: &egui::Painter, c: &TipC, bounds: Rect) {
    tip(p, c.anchor, bounds, &c.title, &c.sub, c.hint.as_deref(), c.col, c.side);
}

/// How strongly the radar sweep is lighting up a point (1 as it passes, fading over ~0.9 rad behind it).
fn sweep_hot(c: Pos2, q: Pos2, sw: f32) -> f32 {
    let v = q - c;
    let diff = (sw - v.y.atan2(v.x).rem_euclid(TAU)).rem_euclid(TAU);
    if diff < 0.9 {
        1.0 - diff / 0.9
    } else {
        0.0
    }
}

/// Glowing link from the PC to a connected device (or a group holding one), with travelling pulses.
fn draw_link(p: &egui::Painter, a: Pos2, b: Pos2, col: Color32, t: f32, k: f32) {
    p.line_segment([a, b], Stroke::new(5.0, add(col, 0.06 * k)));
    p.line_segment([a, b], Stroke::new(1.5, alpha(col, 0.55 * k)));
    for kk in 0..3 {
        let u = (t * 0.6 + kk as f32 / 3.0).fract();
        let q = a + (b - a) * u;
        glow(p, q, 2.5, col, 0.8 * k);
        p.circle_filled(q, 2.0, Color32::WHITE);
    }
}

/// Links from the PC to the connected Bluetooth members of a group. As members near the group centre their
/// line ends slide onto it and share the brightness, so several lines merge into the single group line
/// (and split out again when the group opens). `start_off` keeps the line off the PC orb.
fn group_member_links(p: &egui::Painter, c: Pos2, centre: Pos2, pts: &[Pos2], start_off: f32, t: f32) {
    let n = pts.len() as f32;
    for &q in pts {
        let x = (1.0 - ((q - centre).length() - 27.0) / 24.0).clamp(0.0, 1.0);
        let m = x * x * (3.0 - 2.0 * x);
        let end = q + (centre - q) * m;
        let v = end - c;
        if v.length() > start_off + 13.0 {
            draw_link(p, if start_off > 0.0 { c + v / v.length() * start_off } else { c }, end, kind_col(Kind::Bt), t, 1.0 / (1.0 + m * (n - 1.0)));
        }
    }
}

fn ease(a: f32) -> f32 {
    a * a * (3.0 - 2.0 * a)
}

struct Pop {
    id: String,
    dev: Dev,
    anchor: Pos2,
    a: f32,
    uid: u64,
}

/// Multi-line hover box above `anchor`.
fn tip(p: &egui::Painter, anchor: Pos2, bounds: Rect, title: &str, lines: &[String], hint: Option<&str>, col: Color32, side: Option<f32>) {
    let g0 = p.layout(title.to_string(), FontId::proportional(14.0), Color32::WHITE, 312.0);
    let gs: Vec<_> = lines.iter().map(|l| p.layout(l.clone(), FontId::proportional(12.0), TEXT, 312.0)).collect();
    let gh = hint.map(|h| p.layout(h.to_string(), FontId::proportional(11.5), DIM, 312.0));
    let w = (g0.size().x.max(gs.iter().map(|g| g.size().x).fold(0.0, f32::max)).max(gh.as_ref().map_or(0.0, |g| g.size().x)) + 28.0).clamp(60.0, 340.0); // fits its text, up to 340 px
    // equal padding (12 px) above and below the text; the gaps between title and lines only when there are lines
    let th = if title.is_empty() { 0.0 } else { g0.size().y + if gs.is_empty() { 0.0 } else { 6.0 } };
    let hint_h = gh.as_ref().map_or(0.0, |g| 9.0 + 1.0 + 9.0 + g.size().y); // gap, divider, gap, hint text
    let h = 12.0 + th + gs.iter().map(|g| g.size().y).sum::<f32>() + 5.0 * gs.len().saturating_sub(1) as f32 + hint_h + 12.0;
    let (x, y) = match side {
        Some(rx) => ((rx - w).max(bounds.left() + 8.0), (anchor.y - h / 2.0).clamp(bounds.top() + 8.0, (bounds.bottom() - h - 8.0).max(bounds.top() + 8.0))),
        None => ((anchor.x - w / 2.0).clamp(bounds.left() + 8.0, bounds.right() - w - 8.0), if anchor.y - h - 14.0 < bounds.top() + 8.0 && anchor.y < bounds.top() + h + 30.0 { (anchor.y + 22.0).min(bounds.bottom() - h - 8.0) } else { (anchor.y - h - 14.0).max(bounds.top() + 8.0) }),
    };
    let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
    panel(p, r, 10.0, col);
    let mut yy = r.top() + 12.0;
    if !title.is_empty() {
        let hh = g0.size().y;
        p.galley(pos2(r.left() + 14.0, yy), g0, Color32::WHITE);
        yy += hh + 6.0;
    }
    let n = gs.len();
    for (i, g) in gs.into_iter().enumerate() {
        let lh = g.size().y;
        p.galley(pos2(r.left() + 14.0, yy), g, TEXT);
        yy += lh + if i + 1 < n { 5.0 } else { 0.0 };
    }
    if let Some(g) = gh {
        yy += 9.0;
        p.line_segment([pos2(r.left() + 14.0, yy), pos2(r.right() - 14.0, yy)], Stroke::new(1.0, alpha(col, 0.25)));
        p.galley(pos2(r.left() + 14.0, yy + 10.0), g, DIM);
    }
}

struct Net {
    id: String, // strongest access point of this network (for the info box)
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
        data::signal_frac(self.dbm).max(0.05)
    }
}

/// Footprint of a 20 MHz Wi-Fi channel: 1 at the centre, 0 at ±11 MHz.
fn mac_bytes(s: &str) -> Option<[u8; 6]> {
    let v: Vec<u8> = s[..s.len().min(17)].split(':').filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
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

#[allow(clippy::too_many_arguments)]
/// Battery icon with the percentage inside. The number has a dark outline so it reads on any fill colour.
fn battery_icon(p: &egui::Painter, r: Rect, pct: i32) {
    let pct = pct.clamp(0, 100);
    let fill_col = if pct > 40 { GREEN } else if pct > 15 { AMBER } else { RED };
    let body = Rect::from_min_max(r.min, pos2(r.right() - 4.0, r.bottom()));
    p.rect_filled(body, 6.0, alpha(BG, 0.7));
    let inner = body.shrink(2.5);
    let fw = (inner.width() * pct as f32 / 100.0).max(if pct > 0 { 3.0 } else { 0.0 });
    p.rect_filled(Rect::from_min_size(inner.min, vec2(fw, inner.height())), 4.0, alpha(fill_col, 0.9));
    p.rect_stroke(body, 6.0, Stroke::new(1.5, alpha(Color32::WHITE, 0.55)), StrokeKind::Inside);
    p.rect_filled(Rect::from_center_size(pos2(r.right() - 1.5, r.center().y), vec2(3.5, 10.0)), 1.5, alpha(Color32::WHITE, 0.55));
    let (c, f) = (body.center(), FontId::monospace(13.0));
    let txt = format!("{}%", pct);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (1.0, 1.0), (-1.0, -1.0)] {
        p.text(c + vec2(dx, dy), Align2::CENTER_CENTER, &txt, f.clone(), Color32::from_black_alpha(230));
    }
    p.text(c, Align2::CENTER_CENTER, &txt, f, Color32::WHITE);
}

/// One labelled indicator row: caption, segmented bar, value.
fn meter(p: &egui::Painter, x0: f32, x1: f32, y: f32, label: &str, frac: f32, col: Color32, value: &str) {
    p.text(pos2(x0, y + 4.5), Align2::LEFT_CENTER, label, FontId::monospace(10.5), alpha(CYAN, 0.8));
    led_bar(p, Rect::from_min_size(pos2(x0 + 64.0, y), vec2(x1 - x0 - 64.0 - 112.0, 9.0)), frac.clamp(0.0, 1.0), col, 20);
    p.text(pos2(x1, y + 4.5), Align2::RIGHT_CENTER, value, FontId::monospace(12.0), TEXT);
}

const LABEL_W: f32 = 104.0; // widest radar label before the middle is cut out

/// Full name on a dark, semi-transparent, round-ended bar so it reads over anything. Kept inside the screen.
fn hover_label(p: &egui::Painter, center: Pos2, text: &str, font: FontId) {
    let g = p.layout_no_wrap(text.to_string(), font, TEXT);
    let mut r = Rect::from_center_size(center, g.size() + vec2(20.0, 8.0));
    let lim = p.ctx().content_rect().shrink(4.0);
    r = r.translate(vec2((lim.left() - r.left()).max(0.0) + (lim.right() - r.right()).min(0.0), 0.0));
    p.rect_filled(r, r.height() / 2.0, Color32::from_black_alpha(215));
    p.galley(r.center() - g.size() / 2.0, g, TEXT);
}

/// `text` shortened to fit `max_w` by cutting out the middle ("Studio-Mac…Book-Air"), so both the start and the
/// end of a name stay readable.
fn fit_middle(p: &egui::Painter, text: &str, font: FontId, max_w: f32) -> String {
    let width = |s: &str| p.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    if width(text) <= max_w {
        return text.to_string();
    }
    let ch: Vec<char> = text.chars().collect();
    for keep in (2..ch.len()).rev() {
        let (head, tail) = ((keep + 1) / 2, keep / 2);
        let s: String = ch[..head].iter().chain(std::iter::once(&'…')).chain(ch[ch.len() - tail..].iter()).collect();
        if width(&s) <= max_w {
            return s;
        }
    }
    "…".to_string()
}

/// Slim scrollbar so a scrollable box looks scrollable.
fn scrollbar(p: &egui::Painter, track: Rect, scroll: f32, max: f32, view: f32, content: f32, col: Color32) {
    if max <= 0.0 {
        return;
    }
    p.rect_filled(track, 3.0, alpha(col, 0.14));
    let th = (view / content * track.height()).clamp(26.0, track.height());
    let ty = track.top() + (scroll / max) * (track.height() - th);
    p.rect_filled(Rect::from_min_size(pos2(track.left(), ty), vec2(track.width(), th)), 3.0, alpha(col, 0.75));
}

/// Info box for one device. Returns the screen rects it covers (the wheel belongs to whatever box the mouse is over).
/// A device with a problem gets an ISSUE button by its name; it opens an inner box with the diagnosis.
fn popup(ctx: &egui::Context, d: &Dev, anchor: Pos2, bounds: Rect, close: &mut bool, col: Color32, a: f32, uid: u64, wheel: f32) -> Vec<Rect> {
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
    // indicator rows under the name: dBm for Wi-Fi; link, signal and channels for a connected Bluetooth device
    let sig_rows = if d.dbm.is_some() { 1 } else if d.lq.is_some() { 2 + d.rssi.is_some() as usize } else { 0 };
    let top = 94.0 + if sig_rows == 0 { 0.0 } else { 2.0 + sig_rows as f32 * 18.0 }; // where the detail rows start
    let h_full = (top - 8.0) + rows_h + 30.0 + why.size().y + 34.0;
    let h = h_full.min((bounds.height() - 20.0).max(240.0)); // taller content scrolls with the mouse wheel
    let mut x = anchor.x + 40.0;
    if x + w > bounds.right() - 10.0 {
        x = anchor.x - 40.0 - w;
    }
    let y = (anchor.y - 50.0).clamp(bounds.top() + 10.0, (bounds.bottom() - h - 10.0).max(bounds.top() + 10.0));
    let r = Rect::from_min_size(pos2(x.max(bounds.left() + 10.0), y), vec2(w, h));

    // inner box with the problem details (only measured when the device has an issue)
    let has_issue = !d.issue.is_empty();
    let acc = if d.trouble > 0.0 { RED } else { AMBER };
    let open_id = egui::Id::new(("issue_open", uid));
    let mut issue_open: bool = has_issue && ctx.data(|dt| dt.get_temp(open_id)).unwrap_or(std::env::var_os("AIRSCOPE_ISSUE").is_some()); // env: debug aid
    let ikw = 96.0;
    let iw = w - 20.0;
    let irows: Vec<_> = d
        .issue
        .iter()
        .map(|(k, v)| (lp.layout_no_wrap(k.to_uppercase(), FontId::monospace(10.5), alpha(acc, 0.9)), lp.layout(v.clone(), FontId::proportional(13.0), TEXT, iw - 52.0 - ikw)))
        .collect();
    let irows_h: f32 = irows.iter().map(|(_, g)| g.size().y.max(14.0) + 8.0).sum();
    let ih_full = 44.0 + irows_h + 14.0;
    let ih = ih_full.min(r.height() - (top - 16.0) - 12.0).max(120.0);
    let ir = Rect::from_min_size(r.min + vec2(10.0, top - 16.0), vec2(iw, ih));
    let chip = Rect::from_min_max(pos2(r.right() - 22.0 - 24.0, r.top() + 42.0), pos2(r.right() - 22.0, r.top() + 66.0));
    let bat_right = if has_issue { chip.left() - 10.0 } else { r.right() - 22.0 };
    let bat = Rect::from_min_max(pos2(bat_right - 62.0, r.top() + 42.0), pos2(bat_right, r.top() + 66.0));

    // the wheel goes to the box under the mouse
    let ptr = ctx.input(|i| i.pointer.hover_pos());
    let main_id = egui::Id::new(("popup_scroll", uid));
    let inner_id = egui::Id::new(("issue_scroll", uid));
    let mut scroll: f32 = ctx.data(|dt| dt.get_temp(main_id)).unwrap_or(0.0);
    let mut iscroll: f32 = ctx.data(|dt| dt.get_temp(inner_id)).unwrap_or(0.0);
    let (max_scroll, imax) = ((h_full - h).max(0.0), (ih_full - ih).max(0.0));
    if let Some(q) = ptr {
        if issue_open && ir.contains(q) {
            iscroll = (iscroll - wheel).clamp(0.0, imax);
        } else if r.contains(q) && !issue_open {
            scroll = (scroll - wheel).clamp(0.0, max_scroll);
        }
    }
    ctx.data_mut(|dt| {
        dt.insert_temp(main_id, scroll);
        dt.insert_temp(inner_id, iscroll);
    });

    let mut chip_clicked = false;
    egui::Area::new(egui::Id::new(("popup", uid))).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min).show(ctx, |ui| {
        ui.set_opacity(a);
        let (_, _) = ui.allocate_exact_size(r.size(), Sense::click());
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
        if has_issue {
            // just a glowing dot in a ring: red = issue, amber = warning (the tooltip says which)
            let resp = hit(ui, chip, ui.id().with("issue_chip"), Sense::click()).hint(if d.trouble > 0.0 { "Issue found: click for details" } else { "Warning: click for details" });
            let pulse = (ui.input(|i| i.time) as f32 * 4.0).sin() * 0.5 + 0.5;
            let c = chip.center();
            glow(&p, c, 17.0 + pulse * 4.0, acc, 0.3 + 0.2 * pulse);
            p.circle_filled(c, 12.0, alpha(acc, if issue_open { 0.45 } else if resp.hovered() { 0.32 } else { 0.18 }));
            p.circle_stroke(c, 12.0, Stroke::new(1.4, alpha(acc, 0.65 + 0.3 * pulse)));
            dot(&p, c, 4.5, acc, 0.4 + 0.5 * pulse);
            chip_clicked = resp.clicked();
        }
        if let (Some(b), true) = (d.battery, d.kind == Kind::Bt) {
            battery_icon(&p, bat, b);
        }
        // indicators: dBm for Wi-Fi; link, signal and channels for a connected Bluetooth device
        let by = r.top() + 84.0;
        let (x0, x1) = (r.left() + 22.0, r.right() - 22.0);
        let has = |k: &str| d.issue.iter().any(|row| row.0 == k);
        if let Some(dbm) = d.dbm {
            led_bar(&p, Rect::from_min_size(pos2(x0, by), vec2(w - 130.0, 9.0)), data::signal_frac(dbm), col, 24);
            p.text(pos2(x1, by + 4.5), Align2::RIGHT_CENTER, format!("{:.0} dBm", dbm), FontId::monospace(13.0), TEXT);
        } else if let Some(q) = d.lq {
            let mut y = by;
            meter(&p, x0, x1, y, "LINK", q as f32 / 255.0, if has("Link health") { RED } else { col }, &format!("{}%", data::lq_pct(q)));
            if let Some(rs) = d.rssi {
                y += 18.0;
                // RSSI is relative to the ideal receive range: 0 or above is fine, -20 or below is very weak
                let v = if rs >= 0 { "in range".to_string() } else { format!("{} dB", rs) };
                meter(&p, x0, x1, y, "SIGNAL", 1.0 + (rs.min(0) as f32) / 20.0, if has("Weak signal") { AMBER } else { col }, &v);
            }
            y += 18.0;
            let (frac, v) = match d.chan_use {
                Some(n) => (n as f32 / 79.0, format!("{} of 79", n)),
                None => (1.0, "all 79 (fixed)".to_string()),
            };
            meter(&p, x0, x1, y, "CHANNELS", frac, if has("Channel map") { AMBER } else { col }, &v);
        }
        // rows
        let mut yy = r.top() + top;
        p.line_segment([pos2(r.left() + 20.0, yy - 6.0), pos2(r.right() - 20.0, yy - 6.0)], Stroke::new(1.0, alpha(CYAN, 0.15)));
        let body = p.with_clip_rect(Rect::from_min_max(pos2(r.left(), r.top() + top - 6.0), pos2(r.right(), r.bottom() - 30.0)));
        yy -= scroll;
        for (k, v) in rows {
            let rh = v.size().y.max(14.0);
            body.galley(pos2(r.left() + 22.0, yy + 2.0), k, CYAN);
            body.galley(pos2(r.left() + 22.0 + kw, yy), v, TEXT);
            yy += rh + 7.0;
        }
        yy += 6.0;
        lcd(&body, pos2(r.left() + 22.0, yy + 6.0), Align2::LEFT_CENTER, "HOW WE KNOW", 11.0, col);
        body.galley(pos2(r.left() + 22.0, yy + 20.0), why, TEXT);
        scrollbar(&p, Rect::from_min_max(pos2(r.right() - 13.0, r.top() + top - 2.0), pos2(r.right() - 8.0, r.bottom() - 34.0)), scroll, max_scroll, h - top - 32.0, h_full - top - 32.0, col);
        let more = max_scroll > 0.0 && scroll < max_scroll - 1.0;
        let foot = if more { "Scroll for more (mouse wheel)" } else if on_radar(d) { "Drag the node to where this device really is." } else { "In the tray: no wireless signal to place it on the radar." };
        p.text(pos2(r.left() + 22.0, r.bottom() - 14.0), Align2::LEFT_CENTER, foot, FontId::proportional(11.0), if more { CYAN } else { DIM });
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

    // inner box: the ISSUE button toggles it; a click anywhere outside it closes it
    if has_issue {
        let outside = ctx.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().map_or(false, |q| !ir.contains(q) && !chip.contains(q)));
        if chip_clicked {
            issue_open = !issue_open;
        } else if outside {
            issue_open = false;
        }
        ctx.data_mut(|dt| dt.insert_temp(open_id, issue_open));
        if issue_open {
            egui::Area::new(egui::Id::new(("popup_issue", uid))).order(egui::Order::Foreground).fade_in(false).fixed_pos(ir.min).show(ctx, |ui| {
                ui.set_opacity(a);
                let (_, _) = ui.allocate_exact_size(ir.size(), Sense::click());
                let p = ui.painter_at(ir.expand(40.0));
                p.rect_filled(ir.expand(2.0), 16.0, alpha(BG, 0.5));
                panel(&p, ir, 14.0, acc);
                lcd(&p, pos2(ir.left() + 22.0, ir.top() + 24.0), Align2::LEFT_CENTER, "WHAT'S HAPPENING", 13.0, acc);
                let body = p.with_clip_rect(Rect::from_min_max(pos2(ir.left(), ir.top() + 42.0), pos2(ir.right(), ir.bottom() - 6.0)));
                let mut yy = ir.top() + 50.0 - iscroll;
                for (k, v) in irows {
                    let rh = v.size().y.max(14.0);
                    body.galley(pos2(ir.left() + 22.0, yy + 2.0), k, acc);
                    body.galley(pos2(ir.left() + 22.0 + ikw, yy), v, TEXT);
                    yy += rh + 8.0;
                }
                scrollbar(&p, Rect::from_min_max(pos2(ir.right() - 13.0, ir.top() + 46.0), pos2(ir.right() - 8.0, ir.bottom() - 10.0)), iscroll, imax, ih - 56.0, ih_full - 56.0, acc);
            });
        }
    }
    let mut rects = vec![r];
    if issue_open {
        rects.push(ir);
    }
    rects
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        self.sync();
        let sel_before = self.sel.clone();
        ui.ctx().request_repaint();
        // the wheel belongs to an info box (or the vendor dialog) when the mouse is over one, not to what is behind it
        self.wheel = 0.0;
        if let Some(q) = ui.input(|i| i.pointer.hover_pos()) {
            if self.vendor_open || self.popup_rects.iter().any(|r| r.contains(q)) {
                self.wheel = ui.input(|i| i.smooth_scroll_delta.y);
                ui.ctx().input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
        }
        self.popup_rects.clear();
        let t = self.t0.elapsed().as_secs_f32();
        egui::Panel::left("nav").exact_size(210.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| self.nav(ui));
        egui::Panel::top("hdr").exact_size(70.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| self.header(ui, t));
        self.radar_anchor = None;
        let central = ui.available_rect_before_wrap();
        egui::CentralPanel::no_frame().show(ui, |ui| match self.screen {
            Screen::Radar => self.radar(ui, t),
            Screen::Spectrum => self.spectrum(ui, t),
            Screen::Devices => self.devices(ui),
            Screen::Diagnostics => {
                let names: diag::Names = self.devs.iter().filter(|d| d.kind == Kind::Bt).map(|d| (d.id.to_uppercase(), (d.name.clone(), d.icon))).collect();
                self.diag.ui(ui, &names)
            }
        });
        if matches!(self.screen, Screen::Diagnostics) {
            if let Some(t) = self.diag.tip.take() {
                self.tip_want = Some(t);
            }
        }
        self.vendor_window(ui.ctx());
        self.debug_shot(ui.ctx(), t);
        // info boxes are shared by every screen: opening one fades it in, switching fades the old one out
        let dt = ui.input(|i| i.stable_dt).min(0.05);
        let want: Option<String> = if self.screen == Screen::Radar { self.sel.clone().filter(|id| self.radar_anchor.as_ref().map_or(false, |(i, _)| i == id)) } else { self.sel.clone() };
        if let Some(id) = &want {
            if !self.pops.iter().any(|p| &p.id == id) {
                if let Some(d) = self.devs.iter().find(|d| &d.id == id).cloned() {
                    let anchor = self.radar_anchor.as_ref().filter(|(i, _)| i == id).map(|(_, q)| *q).unwrap_or(self.anchor);
                    self.pop_uid += 1;
                    self.pops.push(Pop { id: id.clone(), dev: d, anchor, a: 0.0, uid: self.pop_uid });
                }
            }
        }
        for p in self.pops.iter_mut() {
            let target = want.as_deref() == Some(p.id.as_str());
            p.a = (p.a + if target { dt / 0.14 } else { -dt / 0.14 }).clamp(0.0, 1.0);
            if let Some(d) = self.devs.iter().find(|d| d.id == p.id) {
                p.dev = d.clone();
            }
            if target {
                if let Some((_, q)) = self.radar_anchor.as_ref().filter(|(i, _)| *i == p.id) {
                    p.anchor = *q;
                }
            }
        }
        self.pops.retain(|p| p.a > 0.0 || want.as_deref() == Some(p.id.as_str()));
        let mut close = false;
        for p in &self.pops {
            let mut c = false;
            let rs = popup(ui.ctx(), &p.dev, p.anchor, central, &mut c, kind_col(p.dev.kind), ease(p.a), p.uid, self.wheel);
            self.popup_rects.extend(rs);
            if c && want.as_deref() == Some(p.id.as_str()) {
                close = true;
            }
        }
        if close {
            self.sel = None;
        }
        // a click anywhere outside the info boxes closes the open one (a click on a device already switched
        // or toggled the selection above, which is why an unchanged selection means "a click on nothing")
        let outside = ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().map_or(false, |q| !self.popup_rects.iter().any(|r| r.contains(q))));
        if outside && self.sel.is_some() && self.sel == sel_before {
            self.sel = None;
        }
        // hover texts asked for by plain widgets (`.hint(..)`) use the same popup, unless a richer one is already wanted
        let hint = ui.ctx().data_mut(|d| d.remove_temp::<Option<TipC>>(egui::Id::new("hint_want"))).flatten();
        if self.tip_want.is_none() {
            self.tip_want = hint;
        }
        // tooltips fade too
        if let Some(w) = self.tip_want.take() {
            if self.tip_last.as_ref().map_or(true, |l| l.title != w.title) {
                self.tip_a = self.tip_a.min(0.2);
            }
            self.tip_last = Some(w);
            self.tip_a = (self.tip_a + dt / 0.12).min(1.0);
        } else {
            self.tip_a = (self.tip_a - dt / 0.15).max(0.0);
        }
        if self.tip_a > 0.0 {
            if let Some(c) = &self.tip_last {
                let mut p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("airscope_tip")));
                p.set_opacity(ease(self.tip_a));
                draw_tip(&p, c, central);
            }
        }
    }
}

fn window_options(samples: u16) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([1000.0, 640.0]).with_title("Airscope").with_app_id("airscope"),
        multisampling: samples, // smooths the edges of the custom meshes (orbs, bars, glows) that egui does not anti-alias itself
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    // Multisampling is a hard requirement when the graphics configuration is picked: a machine that cannot provide
    // it makes eframe return an error or panic instead of opening a window. So try it, and if that fails restart
    // once without it (jagged edges, but it runs). AIRSCOPE_MSAA=<n> sets the sample count (0 = off, for testing).
    let fallback = std::env::var_os("AIRSCOPE_FALLBACK").is_some();
    let samples: u16 = if fallback { 0 } else { std::env::var("AIRSCOPE_MSAA").ok().and_then(|v| v.parse().ok()).unwrap_or(4) };
    let run = |samples| eframe::run_native("Airscope", window_options(samples), Box::new(|cc| Ok(Box::new(App::new(cc)))));
    if samples == 0 {
        return run(0);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(samples))) {
        Ok(Ok(())) => Ok(()),
        failed => {
            eprintln!("airscope: could not start with {}x multisampling ({}); restarting without it", samples, match &failed { Ok(Err(e)) => e.to_string(), _ => "panic".to_string() });
            use std::os::unix::process::CommandExt;
            // a fresh process: the windowing library only allows one event loop per process
            let err = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "airscope".into())).args(std::env::args_os().skip(1)).env("AIRSCOPE_FALLBACK", "1").exec();
            eprintln!("airscope: could not restart: {}", err);
            match failed {
                Ok(r) => r,
                Err(_) => run(0),
            }
        }
    }
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
