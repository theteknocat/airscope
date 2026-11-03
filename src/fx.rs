//! Visual toolkit: glow, bevel, shadow, LED bars, LCD text, orbs, icons.
//! Everything is drawn with egui's painter; "glow" is additive (premultiplied alpha 0).
use crate::data::Icon;
use eframe::egui::epaint::PathStroke;
use eframe::egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use std::f32::consts::{PI, TAU};

pub const BG: Color32 = Color32::from_rgb(5, 9, 18);
pub const PANEL_TOP: Color32 = Color32::from_rgb(24, 38, 64);
pub const PANEL_BOT: Color32 = Color32::from_rgb(9, 15, 29);
pub const CYAN: Color32 = Color32::from_rgb(53, 208, 255);
pub const GREEN: Color32 = Color32::from_rgb(56, 242, 138);
pub const AMBER: Color32 = Color32::from_rgb(255, 179, 71);
pub const MAGENTA: Color32 = Color32::from_rgb(211, 107, 255);
pub const RED: Color32 = Color32::from_rgb(255, 77, 109);
pub const BLUE: Color32 = Color32::from_rgb(86, 148, 255);
pub const TEXT: Color32 = Color32::from_rgb(214, 226, 245);
pub const DIM: Color32 = Color32::from_rgb(120, 140, 175);

/// Additive colour (adds light instead of covering).
pub fn add(c: Color32, a: f32) -> Color32 {
    let f = a.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied((c.r() as f32 * f) as u8, (c.g() as f32 * f) as u8, (c.b() as f32 * f) as u8, 0)
}
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}
pub fn shade(c: Color32, k: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * k).min(255.0) as u8, (c.g() as f32 * k).min(255.0) as u8, (c.b() as f32 * k).min(255.0) as u8)
}

/// Soft halo made of stacked additive circles.
pub fn glow(p: &Painter, c: Pos2, r: f32, col: Color32, k: f32) {
    // additive radial mesh: smooth quadratic falloff from r out to 3.4r
    const RINGS: usize = 7;
    const SEGS: usize = 40;
    let mut m = Mesh::default();
    let v = |u: f32| add(col, k * 0.33 * (1.0 - u) * (1.0 - u));
    m.colored_vertex(c, v(0.0));
    for j in 0..RINGS {
        let u = j as f32 / (RINGS - 1) as f32;
        let rr = r * (1.0 + u * 2.4);
        for i in 0..SEGS {
            let a = i as f32 / SEGS as f32 * TAU;
            m.colored_vertex(c + vec2(a.cos(), a.sin()) * rr, v(u));
        }
    }
    for i in 0..SEGS {
        m.add_triangle(0, 1 + i as u32, 1 + ((i + 1) % SEGS) as u32);
    }
    for j in 0..RINGS - 1 {
        for i in 0..SEGS {
            let (a0, a1) = ((1 + j * SEGS + i) as u32, (1 + j * SEGS + (i + 1) % SEGS) as u32);
            let (b0, b1) = (a0 + SEGS as u32, a1 + SEGS as u32);
            m.add_triangle(a0, b0, a1);
            m.add_triangle(a1, b0, b1);
        }
    }
    p.add(Shape::mesh(m));
}

pub fn shadow(p: &Painter, rect: Rect, rad: f32, off: Vec2, spread: f32, k: f32) {
    const N: usize = 8;
    for i in 0..N {
        let t = i as f32 / N as f32;
        let g = spread * t;
        p.rect_filled(rect.translate(off).expand(g), rad + g, Color32::from_black_alpha((k * 16.0 * (1.0 - t)) as u8));
    }
}

/// A smooth vertical gradient band across a rounded rectangle (one mesh, so no seams between strips), between
/// y0 and y1, following the rounded corners. `f` gives the vertex colour at a given y.
fn glass_band(p: &Painter, rect: Rect, rad: f32, y0: f32, y1: f32, f: impl Fn(f32) -> Color32) {
    let rad = rad.min(rect.width().min(rect.height()) / 2.0).max(0.0);
    let inset = |y: f32| -> f32 {
        let d_top = (y - rect.top()).clamp(0.0, rad);
        let d_bot = (rect.bottom() - y).clamp(0.0, rad);
        let d = d_top.min(d_bot);
        rad - (rad * rad - (rad - d) * (rad - d)).max(0.0).sqrt()
    };
    let mut ys: Vec<f32> = (0..=10).map(|i| y0 + (y1 - y0) * i as f32 / 10.0).collect();
    for k in 0..=6 {
        let d = rad * (1.0 - (k as f32 / 6.0 * std::f32::consts::FRAC_PI_2).cos());
        for y in [rect.top() + d, rect.bottom() - d] {
            if y > y0 && y < y1 {
                ys.push(y);
            }
        }
    }
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    let mut m = Mesh::default();
    for &y in &ys {
        let (l, r) = (rect.left() + inset(y), rect.right() - inset(y));
        let c = f(y);
        m.colored_vertex(pos2(l, y), c);
        m.colored_vertex(pos2(r, y), c);
    }
    for i in 0..ys.len().saturating_sub(1) as u32 {
        let a = i * 2;
        m.add_triangle(a, a + 1, a + 2);
        m.add_triangle(a + 1, a + 3, a + 2);
    }
    p.add(Shape::mesh(m));
}

/// Raised glass panel: drop shadow, vertical gradient, soft specular sheen over the top, shaded bottom, and a rim
/// that is bright along the top-left and fades to the accent colour towards the bottom-right.
pub fn panel(p: &Painter, rect: Rect, rad: f32, accent: Color32) {
    shadow(p, rect, rad, vec2(0.0, 6.0), 18.0, 1.0);
    // body
    const N: usize = 24;
    for i in 0..N {
        let t0 = i as f32 / N as f32;
        let t1 = (i + 1) as f32 / N as f32;
        let strip = Rect::from_min_max(
            pos2(rect.left() - 1.0, rect.top() + rect.height() * t0),
            pos2(rect.right() + 1.0, rect.top() + rect.height() * t1 + 1.0),
        );
        p.with_clip_rect(strip).rect_filled(rect, rad, mix(PANEL_TOP, PANEL_BOT, t0));
    }
    let inner = rect.shrink(1.0);
    let irad = (rad - 1.0).max(0.0);
    // glossy sheen: light fading out over the top of the glass (follows the rounded corners)
    let sh = (rect.height() * 0.5).min(64.0);
    glass_band(p, inner, irad, inner.top(), inner.top() + sh, |y| add(Color32::WHITE, 0.085 * (1.0 - (y - inner.top()) / sh).max(0.0).powf(1.7)));
    // shade gathering along the bottom edge
    let bh = (rect.height() * 0.3).min(40.0);
    glass_band(p, inner, irad, inner.bottom() - bh, inner.bottom(), |y| {
        let t = ((y - (inner.bottom() - bh)) / bh).clamp(0.0, 1.0);
        Color32::from_black_alpha((70.0 * t * t) as u8)
    });
    // inner highlight just inside the rim: bright at the top, gone by a third of the way down
    let h = rect.height().max(1.0);
    let top = rect.top();
    p.add(Shape::closed_line(
        rounded_outline(rect.shrink(1.6), (rad - 1.6).max(0.0), 5),
        PathStroke::new_uv(1.0, move |_, q| add(Color32::WHITE, 0.30 * (1.0 - ((q.y - top) / (h * 0.35)).clamp(0.0, 1.0)))),
    ));
    // rim: lit top-left edge, accent-coloured and dimmer towards the bottom-right
    let (lit, dim) = (alpha(mix(accent, Color32::WHITE, 0.6), 0.9), alpha(accent, 0.38));
    let (l, w) = (rect.left(), rect.width().max(1.0));
    p.add(Shape::closed_line(
        rounded_outline(rect.shrink(0.5), (rad - 0.5).max(0.0), 5),
        PathStroke::new_uv(1.2, move |_, q| {
            let t = (((q.x - l) / w) * 0.35 + ((q.y - top) / h) * 0.65).clamp(0.0, 1.0);
            mix(lit, dim, t.sqrt())
        }),
    ));
}

pub fn led_bar(p: &Painter, rect: Rect, frac: f32, col: Color32, segs: usize) {
    let gap = 2.0;
    let w = (rect.width() - gap * (segs as f32 - 1.0)) / segs as f32;
    for i in 0..segs {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * (w + gap), rect.top()), vec2(w, rect.height()));
        let lit = (i as f32 + 0.5) / segs as f32 <= frac;
        if lit {
            glow_rect(p, r, 5.0, col, 0.28);
            p.rect_filled(r, 1.5, col);
            p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), r.height() * 0.4)), 1.5, add(Color32::WHITE, 0.22));
        } else {
            p.rect_filled(r, 1.5, alpha(col, 0.10));
            p.rect_stroke(r, 1.5, Stroke::new(1.0, Color32::from_black_alpha(90)), StrokeKind::Inside);
        }
    }
}

/// Glowing monospace "LCD" readout.
pub fn lcd(p: &Painter, pos: Pos2, anchor: Align2, text: &str, size: f32, col: Color32) {
    // Halo (kept well below the text colour so the letters stay crisp and clearly the brightest thing): the same laid-out text drawn additively on four rings around its position, fading outwards, so the
    // light spreads evenly in every direction (a cheap stand-in for a blur) instead of smearing along the axes.
    let g = p.layout_no_wrap(text.to_string(), FontId::monospace(size), Color32::PLACEHOLDER);
    let tl = anchor.anchor_size(pos, g.size()).min;
    let reach = (size * 0.15).clamp(2.0, 5.5);
    for (ring, (rf, k)) in [(0.28, 0.040), (0.52, 0.028), (0.76, 0.018), (1.0, 0.010)].into_iter().enumerate() {
        let n = 8 + ring * 3;
        for i in 0..n {
            let a = (i as f32 + 0.5 * ring as f32) / n as f32 * TAU;
            p.galley(tl + vec2(a.cos(), a.sin()) * reach * rf, g.clone(), add(col, k));
        }
    }
    p.galley(tl, g, col);
}

pub fn arc(p: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) {
    let n = 28;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

/// Smooth shaded sphere: a mesh with a bright off-centre vertex, a mid ring and a dark rim.
/// The GPU interpolates the vertex colours, so there is no banding.
pub fn sphere(p: &Painter, c: Pos2, r: f32, col: Color32, segs: usize) {
    let hi = mix(col, Color32::WHITE, 0.50);
    let rim = shade(col, 0.22);
    let f = vec2(-0.30, -0.36) * r; // highlight position
    let mut m = Mesh::default();
    m.colored_vertex(c + f, hi); // 0
    for i in 0..segs {
        let a = i as f32 / segs as f32 * TAU;
        m.colored_vertex(c + f * 0.35 + vec2(a.cos(), a.sin()) * r * 0.62, col); // ring A
    }
    for i in 0..segs {
        let a = i as f32 / segs as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * r, rim); // ring B (rim)
    }
    for i in 0..segs {
        let a = i as f32 / segs as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * (r + 1.0), Color32::TRANSPARENT); // ring C: 1 px feather, hides the jagged edge
    }
    let s = segs as u32;
    for i in 0..s {
        let (a0, a1) = (1 + i, 1 + (i + 1) % s);
        let (b0, b1) = (1 + s + i, 1 + s + (i + 1) % s);
        let (c0, c1) = (1 + 2 * s + i, 1 + 2 * s + (i + 1) % s);
        m.add_triangle(0, a0, a1);
        m.add_triangle(a0, b0, b1);
        m.add_triangle(a0, b1, a1);
        m.add_triangle(b0, c0, c1);
        m.add_triangle(b0, c1, b1);
    }
    p.add(Shape::mesh(m));
}

/// Soft white highlight: opaque in the middle, fully transparent at the edge (no hard outline needed).
fn blob(p: &Painter, c: Pos2, r: f32, a: f32) {
    let segs = 20u32;
    let mut m = Mesh::default();
    let k = (a.clamp(0.0, 1.0) * 255.0) as u8;
    m.colored_vertex(c, Color32::from_rgba_premultiplied(k, k, k, k));
    for i in 0..segs {
        let t = i as f32 / segs as f32 * TAU;
        m.colored_vertex(c + vec2(t.cos(), t.sin()) * r, Color32::TRANSPARENT);
    }
    for i in 0..segs {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % segs);
    }
    p.add(Shape::mesh(m));
}

/// Soft bevel just inside a sphere's edge: light spilling in from the top-left, shade gathering at the bottom-right.
/// Both fade out towards the middle, so there is no hard line.
fn bevel_ring(p: &Painter, c: Pos2, r: f32, w: f32) {
    const SEGS: usize = 40;
    let light = vec2(-0.6, -0.8);
    let mut m = Mesh::default();
    for i in 0..SEGS {
        let a = i as f32 / SEGS as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * (r - w), Color32::TRANSPARENT);
    }
    for i in 0..SEGS {
        let a = i as f32 / SEGS as f32 * TAU;
        let n = vec2(a.cos(), a.sin());
        let d = n.x * light.x + n.y * light.y;
        let col = if d > 0.0 { add(Color32::WHITE, 0.42 * d.powf(1.4)) } else { Color32::from_black_alpha((120.0 * (-d).powf(1.4)) as u8) };
        m.colored_vertex(c + n * r, col);
    }
    let n = SEGS as u32;
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_triangle(i, n + i, j);
        m.add_triangle(j, n + i, n + j);
    }
    p.add(Shape::mesh(m));
}

/// Shiny bevelled sphere with halo. `hot` (0..1) boosts the glow (sweep pings, selection).
pub fn orb(p: &Painter, c: Pos2, r: f32, col: Color32, hot: f32) {
    glow(p, c, r, col, 0.55 + hot * 1.1);
    sphere(p, c, r, col, 48);
    // anti-aliased rim (the mesh edge itself is not smoothed) and bevel
    p.circle_stroke(c, r - 0.5, Stroke::new(1.2, shade(col, 0.22)));
    bevel_ring(p, c, r - 0.5, (r * 0.24).clamp(2.5, 5.0));
    blob(p, c + vec2(-0.34, -0.42) * r, r * 0.34, 0.85);
}

/// Small shaded dot for unidentified signals.
pub fn dot(p: &Painter, c: Pos2, r: f32, col: Color32, hot: f32) {
    glow(p, c, r, col, 0.45 + hot);
    sphere(p, c, r, col, 28);
    p.circle_stroke(c, r - 0.4, Stroke::new(1.0, shade(col, 0.3)));
    blob(p, c + vec2(-0.3, -0.35) * r, r * 0.5, 0.8);
}

pub fn icon(p: &Painter, ic: Icon, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new((s * 0.17).max(1.4), col);
    let line = |a: Vec2, b: Vec2| p.line_segment([c + a * s, c + b * s], st);
    match ic {
        Icon::Wifi => {
            for k in 1..=3 {
                arc(p, c + vec2(0.0, 0.55 * s), s * 0.42 * k as f32, PI * 1.22, PI * 1.78, st);
            }
            p.circle_filled(c + vec2(0.0, 0.58 * s), s * 0.12, col);
        }
        Icon::Router => {
            p.rect_stroke(Rect::from_center_size(c + vec2(0.0, 0.3 * s), vec2(1.9 * s, 0.85 * s)), 2.0, st, StrokeKind::Middle);
            line(vec2(-0.6, -0.1), vec2(-0.85, -0.85));
            line(vec2(0.6, -0.1), vec2(0.85, -0.85));
            p.circle_filled(c + vec2(-0.5, 0.3) * s, s * 0.1, col);
            p.circle_filled(c + vec2(-0.2, 0.3) * s, s * 0.1, col);
        }
        Icon::Keyboard => {
            p.rect_stroke(Rect::from_center_size(c, vec2(2.0 * s, 1.15 * s)), 2.0, st, StrokeKind::Middle);
            for row in 0..3 {
                for k in 0..4 {
                    let x = (-0.62 + k as f32 * 0.41) * s + if row == 1 { 0.1 * s } else { 0.0 };
                    p.rect_filled(Rect::from_center_size(c + vec2(x, (-0.28 + row as f32 * 0.28) * s), vec2(0.22 * s, 0.14 * s)), 1.0, col);
                }
            }
        }
        Icon::Headphones => {
            arc(p, c + vec2(0.0, 0.1 * s), 0.85 * s, PI, TAU, st);
            p.rect_filled(Rect::from_center_size(c + vec2(-0.85, 0.35) * s, vec2(0.38 * s, 0.75 * s)), 2.0, col);
            p.rect_filled(Rect::from_center_size(c + vec2(0.85, 0.35) * s, vec2(0.38 * s, 0.75 * s)), 2.0, col);
        }
        Icon::Phone => {
            p.rect_stroke(Rect::from_center_size(c, vec2(1.0 * s, 1.8 * s)), 3.0, st, StrokeKind::Middle);
            p.circle_filled(c + vec2(0.0, 0.7 * s), s * 0.09, col);
        }
        Icon::Gamepad => {
            p.rect_stroke(Rect::from_center_size(c, vec2(2.0 * s, 1.1 * s)), s * 0.5, st, StrokeKind::Middle);
            line(vec2(-0.7, 0.0), vec2(-0.3, 0.0));
            line(vec2(-0.5, -0.2), vec2(-0.5, 0.2));
            p.circle_filled(c + vec2(0.4, 0.1) * s, s * 0.1, col);
            p.circle_filled(c + vec2(0.7, -0.1) * s, s * 0.1, col);
        }
        Icon::Mouse => {
            p.rect_stroke(Rect::from_center_size(c, vec2(1.15 * s, 1.8 * s)), s * 0.55, st, StrokeKind::Middle);
            line(vec2(0.0, -0.9), vec2(0.0, -0.2));
        }
        Icon::Bluetooth => {
            let pts = [(-0.5, -0.4), (0.5, 0.4), (0.0, 0.9), (0.0, -0.9), (0.5, -0.4), (-0.5, 0.4)];
            p.add(Shape::line(pts.iter().map(|&(x, y)| c + vec2(x, y) * s).collect(), st));
        }
        Icon::Computer => {
            p.rect_stroke(Rect::from_center_size(c, vec2(1.2 * s, 1.9 * s)), 2.0, st, StrokeKind::Middle);
            line(vec2(-0.4, -0.55), vec2(0.4, -0.55));
            p.circle_filled(c + vec2(0.0, 0.6) * s, s * 0.1, col);
        }
        Icon::Tv => {
            p.rect_stroke(Rect::from_center_size(c + vec2(0.0, -0.1 * s), vec2(2.0 * s, 1.3 * s)), 2.0, st, StrokeKind::Middle);
            line(vec2(-0.5, 0.85), vec2(0.5, 0.85));
        }
        Icon::Server => {
            for k in 0..3 {
                let y = (-0.62 + k as f32 * 0.62) * s;
                p.rect_stroke(Rect::from_center_size(c + vec2(0.0, y), vec2(1.9 * s, 0.5 * s)), 2.0, st, StrokeKind::Middle);
                p.circle_filled(c + vec2(0.62 * s, y), s * 0.07, col);
            }
        }
        Icon::Printer => {
            p.rect_stroke(Rect::from_center_size(c + vec2(0.0, 0.15 * s), vec2(1.9 * s, 0.95 * s)), 2.0, st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_center_size(c + vec2(0.0, -0.6 * s), vec2(1.1 * s, 0.55 * s)), 1.0, st, StrokeKind::Middle);
        }
        Icon::Speaker => {
            p.rect_stroke(Rect::from_center_size(c, vec2(1.3 * s, 1.9 * s)), 3.0, st, StrokeKind::Middle);
            p.circle_stroke(c + vec2(0.0, 0.35 * s), s * 0.38, st);
            p.circle_filled(c + vec2(0.0, -0.55 * s), s * 0.12, col);
        }
        Icon::Camera => {
            p.rect_stroke(Rect::from_center_size(c, vec2(1.9 * s, 1.3 * s)), 3.0, st, StrokeKind::Middle);
            p.circle_stroke(c, s * 0.38, st);
        }
        Icon::Unknown => {
            p.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(s * 1.8), col);
        }
    }
}

pub fn nav_icon(p: &Painter, which: u8, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new(1.6, col);
    match which {
        0 => {
            p.circle_stroke(c, s, st);
            p.circle_stroke(c, s * 0.5, st);
            p.line_segment([c, c + vec2(0.7, -0.7) * s], st);
        }
        1 => {
            for (i, h) in [0.5, 1.0, 0.7, 1.4, 0.9].iter().enumerate() {
                let x = (-0.9 + i as f32 * 0.45) * s;
                p.rect_filled(Rect::from_min_max(c + vec2(x - 0.12 * s, s - h * s), c + vec2(x + 0.12 * s, s)), 1.0, col);
            }
        }
        _ => {
            for k in 0..3 {
                let y = (-0.7 + k as f32 * 0.7) * s;
                p.circle_filled(c + vec2(-0.8 * s, y), 1.8, col);
                p.line_segment([c + vec2(-0.4 * s, y), c + vec2(0.9 * s, y)], st);
            }
        }
    }
}

/// Subtle glow around a rectangle: an additive gradient from `k` at the edge to nothing `g` px out.
/// Points around a rounded rectangle (clockwise from the top-right arc), `arc` segments per corner.
pub fn rounded_outline(rr: Rect, rad: f32, arc: usize) -> Vec<Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let rad = rad.min(rr.width().min(rr.height()) / 2.0).max(0.0);
    let corners = [(rr.right() - rad, rr.top() + rad, -FRAC_PI_2), (rr.right() - rad, rr.bottom() - rad, 0.0), (rr.left() + rad, rr.bottom() - rad, FRAC_PI_2), (rr.left() + rad, rr.top() + rad, PI)];
    corners.iter().flat_map(|&(cx, cy, a0)| (0..=arc).map(move |i| {
        let a = a0 + i as f32 / arc as f32 * FRAC_PI_2;
        pos2(cx + rad * a.cos(), cy + rad * a.sin())
    })).collect()
}

pub fn glow_rect(p: &Painter, r: Rect, g: f32, col: Color32, k: f32) {
    // Rings of rounded-rectangle outlines growing out from the shape, fading with the same quadratic falloff
    // as `glow`, so the halo follows the corners instead of forming a hard-edged rectangle.
    const RINGS: usize = 4;
    const ARC: usize = 4; // segments per corner
    let rad0 = 2.0f32.min(r.width().min(r.height()) / 2.0);
    let outline = |e: f32| rounded_outline(r.expand(e), rad0 + e, ARC);
    let n = 4 * (ARC + 1);
    let mut m = Mesh::default();
    for j in 0..=RINGS {
        let u = j as f32 / RINGS as f32;
        let c = add(col, k * (1.0 - u) * (1.0 - u));
        for q in outline(g * u) {
            m.colored_vertex(q, c);
        }
    }
    for j in 0..RINGS {
        for i in 0..n {
            let (a0, a1) = ((j * n + i) as u32, (j * n + (i + 1) % n) as u32);
            let (b0, b1) = (a0 + n as u32, a1 + n as u32);
            m.add_triangle(a0, b0, a1);
            m.add_triangle(a1, b0, b1);
        }
    }
    p.add(Shape::mesh(m));
}
