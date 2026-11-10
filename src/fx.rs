//! Visual toolkit: glow, bevel, shadow, LED bars, LCD text, orbs, icons.
//! Everything is drawn with egui's painter; "glow" is additive (premultiplied alpha 0).
use crate::Hint;
use crate::data::Icon;
use eframe::egui::epaint::PathStroke;
use eframe::egui::{pos2, vec2, Align2, Color32, CursorIcon, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
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
        3 => {
            let pts = [(-1.0, 0.1), (-0.5, 0.1), (-0.25, -0.9), (0.1, 0.9), (0.4, -0.1), (1.0, -0.1)];
            for w in pts.windows(2) {
                p.line_segment([c + vec2(w[0].0, w[0].1) * s, c + vec2(w[1].0, w[1].1) * s], st);
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
    glow_rect_r(p, r, 2.0, g, col, k);
}

/// `glow_rect` for a shape with rounded corners of radius `rad`, so the halo follows its outline.
pub fn glow_rect_r(p: &Painter, r: Rect, rad: f32, g: f32, col: Color32, k: f32) {
    // Rings of rounded-rectangle outlines growing out from the shape, fading with the same quadratic falloff
    // as `glow`, so the halo follows the corners instead of forming a hard-edged rectangle.
    const RINGS: usize = 4;
    const ARC: usize = 4; // segments per corner
    let rad0 = rad.min(r.width().min(r.height()) / 2.0);
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

// ---------- toolbar widgets shared by the Devices and Diagnostics screens ----------

/// Action button in the app's glass style; dimmed and inert when `on` is false. `flash` (0..1) fades a state halo in and out (accent is then
/// the state colour); `min_w` keeps the width steady when the label changes. Returns the response (use `.clicked()`).
pub fn action_button(ui: &mut Ui, icon: Option<Glyph>, label: &str, accent: Color32, flash: f32, on: bool, min_w: f32) -> eframe::egui::Response {
    action_button_h(ui, icon, label, accent, flash, on, min_w, 36.0)
}

/// `action_button` at another height (28 for the smaller controls of the page header).
pub fn action_button_h(ui: &mut Ui, icon: Option<Glyph>, label: &str, accent: Color32, flash: f32, on: bool, min_w: f32, h: f32) -> eframe::egui::Response {
    let small = h < 32.0;
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(if small { 13.0 } else { 14.0 }), if on { Color32::WHITE } else { DIM });
    let extra = if icon.is_some() && !label.is_empty() { 26.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2((g.size().x + if small { 30.0 } else { 40.0 } + extra).max(min_w), h), if on { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if on {
        if flash > 0.0 {
            glow_rect_r(p, r, 10.0, 12.0, accent, 0.5 * flash); // halo with the button's own corner radius
        }
        panel(p, r, 10.0, if resp.hovered() && flash == 0.0 { CYAN } else { accent });
    } else {
        p.rect_filled(r, 10.0, alpha(Color32::WHITE, 0.04));
    }
    // icon and text are centred together as one block
    let x0 = r.center().x - (g.size().x + extra) / 2.0;
    if let Some(ic) = icon {
        glyph(p, ic, if label.is_empty() { r.center() } else { pos2(x0 + 8.0, r.center().y) }, if small { 6.5 } else { 7.5 }, if on { Color32::WHITE } else { DIM });
    }
    p.galley(pos2(x0 + extra, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
    if on { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Small line icons for buttons.
#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    Record,
    Stop,
    Open,
    Copy,
    Save,
    Check,
    Cross,
    Refresh,
    Undo,
    Back,
    Help,
    List,
    Trash,
}

/// Outline of the question mark, in units of its 512-wide source box centred on 0 (see `Glyph::Help`).
const QM: [(f32, f32); 118] = [(-0.3367, -0.3543), (-0.3259, -0.3795), (-0.3123, -0.4029), (-0.2961, -0.4242), (-0.2774, -0.4432), (-0.2567, -0.4598), (-0.2342, -0.4738), (-0.2100, -0.4850), (-0.1845, -0.4932), (-0.1579, -0.4983), (-0.1305, -0.5000), (0.0973, -0.5000), (0.1373, -0.4968), (0.1753, -0.4874), (0.2106, -0.4725), (0.2429, -0.4524), (0.2716, -0.4277), (0.2963, -0.3990), (0.3163, -0.3667), (0.3312, -0.3313), (0.3405, -0.2934), (0.3438, -0.2535), (0.3424, -0.2273), (0.3382, -0.2016), (0.3315, -0.1767), (0.3222, -0.1527), (0.3105, -0.1299), (0.2966, -0.1084), (0.2804, -0.0885), (0.2622, -0.0702), (0.2420, -0.0538), (0.2199, -0.0395), (0.0938, 0.0328), (0.0923, 0.0477), (0.0886, 0.0619), (0.0828, 0.0751), (0.0752, 0.0872), (0.0658, 0.0979), (0.0549, 0.1072), (0.0427, 0.1147), (0.0293, 0.1203), (0.0150, 0.1238), (0.0000, 0.1250), (-0.0152, 0.1238), (-0.0297, 0.1202), (-0.0432, 0.1146), (-0.0554, 0.1070), (-0.0664, 0.0976), (-0.0757, 0.0867), (-0.0833, 0.0744), (-0.0890, 0.0610), (-0.0925, 0.0465), (-0.0938, 0.0312), (-0.0938, -0.0215), (-0.0932, -0.0315), (-0.0916, -0.0412), (-0.0891, -0.0507), (-0.0855, -0.0598), (-0.0811, -0.0684), (-0.0758, -0.0766), (-0.0696, -0.0841), (-0.0626, -0.0911), (-0.0549, -0.0973), (-0.0465, -0.1027), (0.1266, -0.2020), (0.1319, -0.2054), (0.1367, -0.2093), (0.1411, -0.2137), (0.1449, -0.2184), (0.1483, -0.2236), (0.1511, -0.2290), (0.1533, -0.2348), (0.1549, -0.2407), (0.1559, -0.2469), (0.1562, -0.2531), (0.1555, -0.2627), (0.1532, -0.2719), (0.1496, -0.2803), (0.1448, -0.2881), (0.1389, -0.2949), (0.1320, -0.3008), (0.1243, -0.3056), (0.1159, -0.3091), (0.1068, -0.3113), (0.0973, -0.3121), (-0.1305, -0.3121), (-0.1344, -0.3119), (-0.1382, -0.3112), (-0.1418, -0.3100), (-0.1453, -0.3084), (-0.1485, -0.3064), (-0.1514, -0.3041), (-0.1541, -0.3014), (-0.1563, -0.2984), (-0.1583, -0.2950), (-0.1598, -0.2914), (-0.1613, -0.2867), (-0.1675, -0.2728), (-0.1757, -0.2604), (-0.1856, -0.2496), (-0.1969, -0.2406), (-0.2094, -0.2334), (-0.2228, -0.2283), (-0.2369, -0.2252), (-0.2515, -0.2243), (-0.2662, -0.2258), (-0.2809, -0.2297), (-0.2947, -0.2359), (-0.3070, -0.2441), (-0.3178, -0.2539), (-0.3268, -0.2652), (-0.3340, -0.2777), (-0.3392, -0.2912), (-0.3423, -0.3053), (-0.3432, -0.3198), (-0.3418, -0.3346), (-0.3379, -0.3492), (-0.3363, -0.3539)];
const QM_TRI: [u16; 348] = [117, 0, 1, 117, 1, 2, 117, 2, 3, 117, 3, 4, 117, 4, 5, 117, 5, 6, 117, 6, 7, 117, 7, 8, 117, 8, 9, 117, 9, 10, 117, 10, 11, 117, 11, 12, 117, 12, 13, 117, 13, 14, 117, 14, 15, 117, 15, 16, 117, 16, 17, 117, 17, 18, 117, 18, 19, 117, 19, 20, 20, 21, 22, 20, 22, 23, 20, 23, 24, 20, 24, 25, 20, 25, 26, 20, 26, 27, 20, 27, 28, 20, 28, 29, 20, 29, 30, 20, 30, 31, 20, 31, 32, 32, 33, 34, 32, 34, 35, 32, 35, 36, 32, 36, 37, 32, 37, 38, 32, 38, 39, 32, 39, 40, 32, 40, 41, 20, 32, 41, 20, 41, 42, 20, 42, 43, 20, 43, 44, 20, 44, 45, 20, 45, 46, 20, 46, 47, 20, 47, 48, 20, 48, 49, 20, 49, 50, 20, 50, 51, 20, 51, 52, 20, 52, 53, 20, 53, 54, 20, 54, 55, 20, 55, 56, 20, 56, 57, 20, 57, 58, 20, 58, 59, 20, 59, 60, 20, 60, 61, 20, 61, 62, 20, 62, 63, 20, 63, 64, 20, 64, 65, 20, 65, 66, 20, 66, 67, 20, 67, 68, 20, 68, 69, 20, 69, 70, 20, 70, 71, 20, 71, 72, 20, 72, 73, 20, 73, 74, 20, 74, 75, 20, 75, 76, 20, 76, 77, 20, 77, 78, 20, 78, 79, 20, 79, 80, 20, 80, 81, 20, 81, 82, 20, 82, 83, 117, 20, 83, 117, 83, 84, 117, 84, 85, 117, 85, 86, 117, 86, 87, 117, 87, 88, 117, 88, 89, 117, 89, 90, 117, 90, 91, 117, 91, 92, 117, 92, 93, 117, 93, 94, 117, 94, 95, 117, 95, 96, 117, 96, 97, 117, 97, 98, 117, 98, 99, 117, 99, 100, 117, 100, 101, 117, 101, 102, 117, 102, 103, 117, 103, 104, 117, 104, 105, 117, 105, 106, 117, 106, 107, 117, 107, 108, 117, 108, 109, 117, 109, 110, 117, 110, 111, 117, 111, 112, 117, 112, 113, 117, 113, 114, 117, 114, 115, 115, 116, 117];

/// Draws `g` centred on `c`, about `2 * s` across.
pub fn glyph(p: &Painter, g: Glyph, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new(1.7, col);
    let pt = |x: f32, y: f32| c + vec2(x, y) * s;
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|q| pt(q.0, q.1)).collect(), st));
    };
    match g {
        Glyph::Record => {
            p.circle_filled(c, s * 0.85, if col == Color32::WHITE { RED } else { col }); // the red record dot
        }
        Glyph::Stop => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 1.5)), 2.0, col);
        }
        Glyph::Open => {
            // a folder with its tab
            p.add(Shape::closed_line(vec![pt(-1.0, -0.65), pt(-0.3, -0.65), pt(0.05, -0.3), pt(1.0, -0.3), pt(1.0, 0.8), pt(-1.0, 0.8)], st));
        }
        Glyph::Copy => {
            p.rect_stroke(Rect::from_min_max(pt(-0.95, -0.95), pt(0.35, 0.35)), 1.5, st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(-0.35, -0.35), pt(0.95, 0.95)), 1.5, st, StrokeKind::Middle);
        }
        Glyph::Save => {
            // an arrow going down into a tray
            line(&[(0.0, -0.95), (0.0, 0.3)]);
            line(&[(-0.55, -0.25), (0.0, 0.35), (0.55, -0.25)]);
            line(&[(-0.95, 0.15), (-0.95, 0.9), (0.95, 0.9), (0.95, 0.15)]);
        }
        Glyph::Refresh => {
            // an almost-closed circle with an arrowhead at its end
            let (r, a0, a1) = (0.8f32, 40.0f32.to_radians(), 320.0f32.to_radians());
            let pts: Vec<Pos2> = (0..=20).map(|i| { let a = a0 + (a1 - a0) * i as f32 / 20.0; pt(r * a.cos(), r * a.sin()) }).collect();
            p.add(Shape::line(pts, st));
            let e = vec2(r * a1.cos(), r * a1.sin());
            let t = vec2(-a1.sin(), a1.cos()); // direction of travel at the end
            let n = vec2(a1.cos(), a1.sin());
            line(&[((e - t * 0.55 + n * 0.45).x, (e - t * 0.55 + n * 0.45).y), (e.x, e.y), ((e - t * 0.55 - n * 0.45).x, (e - t * 0.55 - n * 0.45).y)]);
        }
        Glyph::Undo => {
            let pt = |x: f32, y: f32| c + vec2(-x, y) * s;
            let line = |pts: &[(f32, f32)]| { p.add(Shape::line(pts.iter().map(|q| pt(q.0, q.1)).collect(), st)); };
            // an almost-closed circle with an arrowhead at its end
            let (r, a0, a1) = (0.8f32, 40.0f32.to_radians(), 320.0f32.to_radians());
            let pts: Vec<Pos2> = (0..=20).map(|i| { let a = a0 + (a1 - a0) * i as f32 / 20.0; pt(r * a.cos(), r * a.sin()) }).collect();
            p.add(Shape::line(pts, st));
            let e = vec2(r * a1.cos(), r * a1.sin());
            let t = vec2(-a1.sin(), a1.cos()); // direction of travel at the end
            let n = vec2(a1.cos(), a1.sin());
            line(&[((e - t * 0.55 + n * 0.45).x, (e - t * 0.55 + n * 0.45).y), (e.x, e.y), ((e - t * 0.55 - n * 0.45).x, (e - t * 0.55 - n * 0.45).y)]);
        }
        Glyph::Back => {
            line(&[(0.95, 0.0), (-0.95, 0.0)]);
            line(&[(-0.25, -0.7), (-0.95, 0.0), (-0.25, 0.7)]);
        }
        Glyph::Help => {
            // the question mark of Font Awesome's circle-question (free set), without the circle: outline as triangles, plus the dot
            let k = s * 1.9;
            let mut m = eframe::egui::Mesh::default();
            for q in QM {
                m.colored_vertex(c + vec2(q.0, q.1) * k, col);
            }
            for t in QM_TRI.chunks(3) {
                m.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
            }
            p.add(Shape::mesh(m));
            p.circle_filled(c + vec2(0.0, 0.375) * k, 0.125 * k, col);
        }
        Glyph::List => {
            for y in [-0.65f32, 0.0, 0.65] {
                p.circle_filled(pt(-0.85, y), 1.3, col);
                line(&[(-0.4, y), (0.95, y)]);
            }
        }
        Glyph::Trash => {
            line(&[(-0.95, -0.5), (0.95, -0.5)]); // lid
            line(&[(-0.35, -0.5), (-0.35, -0.95), (0.35, -0.95), (0.35, -0.5)]); // handle
            p.add(Shape::closed_line(vec![pt(-0.7, -0.5), pt(-0.58, 1.0), pt(0.58, 1.0), pt(0.7, -0.5)], st)); // bin
            line(&[(-0.2, -0.1), (-0.2, 0.7)]);
            line(&[(0.2, -0.1), (0.2, 0.7)]);
        }
        Glyph::Check => line(&[(-0.85, 0.05), (-0.3, 0.65), (0.9, -0.65)]),
        Glyph::Cross => {
            line(&[(-0.7, -0.7), (0.7, 0.7)]);
            line(&[(0.7, -0.7), (-0.7, 0.7)]);
        }
    }
}

/// A view tab: plain text with a glowing underline on the current one.
pub fn view_tab(ui: &mut Ui, label: &str, on: bool, badge: usize) -> bool {
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(16.0), Color32::WHITE);
    // count badge: a circle for one digit, growing into a pill as the number gets longer
    let bg = (badge > 0).then(|| ui.painter().layout_no_wrap(badge.to_string(), FontId::monospace(11.0), Color32::from_rgb(8, 16, 30)));
    let bw = bg.as_ref().map_or(0.0, |b| (b.size().x + 12.0).max(18.0));
    let extra = if bg.is_some() { bw + 6.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 32.0 + extra, 34.0), Sense::click());
    let p = ui.painter();
    let col = if on { Color32::WHITE } else if resp.hovered() { TEXT } else { DIM };
    p.text(pos2(r.left() + 16.0 + g.size().x / 2.0, r.center().y - 2.0), Align2::CENTER_CENTER, label, FontId::proportional(16.0), col);
    if let Some(b) = bg {
        let c = pos2(r.left() + 16.0 + g.size().x + 6.0 + bw / 2.0, r.center().y - 2.0);
        p.rect_filled(Rect::from_center_size(c, vec2(bw, 18.0)), 9.0, CYAN); // flat, no glow
        p.galley(c - b.size() / 2.0, b, Color32::PLACEHOLDER);
    }
    if on {
        let u = Rect::from_min_size(pos2(r.left() + 10.0, r.bottom() - 4.0), vec2(r.width() - 20.0, 3.0));
        glow_rect(p, u, 8.0, CYAN, 0.5);
        p.rect_filled(u, 1.5, CYAN);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// A small on/off toggle for what the view shows: rounded outline with a lamp, nothing like the action buttons.
pub fn toggle_chip(ui: &mut Ui, label: &str, on: bool, col: Color32, tip: &str) -> bool {
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), Color32::WHITE);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 44.0, 28.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 14.0, if on { alpha(col, if resp.hovered() { 0.28 } else { 0.18 }) } else { alpha(Color32::WHITE, if resp.hovered() { 0.08 } else { 0.03 }) });
    p.rect_stroke(r, 14.0, Stroke::new(1.2, if on { alpha(col, 0.85) } else { alpha(Color32::WHITE, 0.18) }), StrokeKind::Inside);
    let c = pos2(r.left() + 16.0, r.center().y);
    if on {
        dot(p, c, 4.0, col, 0.2);
    } else {
        p.circle_stroke(c, 4.0, Stroke::new(1.2, DIM));
    }
    p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), if on { Color32::WHITE } else { DIM });
    resp.on_hover_cursor(CursorIcon::PointingHand).hint(tip).clicked()
}

/// Draws `icon` and `label` as one block centred in `r` (for buttons that are drawn by hand).
pub fn icon_label(p: &Painter, r: Rect, icon: Glyph, label: &str, size: f32) {
    let g = p.layout_no_wrap(label.to_string(), FontId::proportional(size), Color32::WHITE);
    if label.is_empty() {
        return glyph(p, icon, r.center(), 7.5, Color32::WHITE);
    }
    let x0 = r.center().x - (g.size().x + 24.0) / 2.0;
    glyph(p, icon, pos2(x0 + 8.0, r.center().y), 7.5, Color32::WHITE);
    p.galley(pos2(x0 + 24.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
}
