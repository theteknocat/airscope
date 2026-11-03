//! Visual toolkit: glow, bevel, shadow, LED bars, LCD text, orbs, icons.
//! Everything is drawn with egui's painter; "glow" is additive (premultiplied alpha 0).
use crate::data::Icon;
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

/// Raised glass panel: drop shadow, vertical gradient, top highlight, bevelled border.
pub fn panel(p: &Painter, rect: Rect, rad: f32, accent: Color32) {
    shadow(p, rect, rad, vec2(0.0, 6.0), 18.0, 1.0);
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
    // inner light along the top edge, inner dark along the bottom
    p.line_segment(
        [pos2(rect.left() + rad, rect.top() + 1.5), pos2(rect.right() - rad, rect.top() + 1.5)],
        Stroke::new(1.0, add(Color32::WHITE, 0.16)),
    );
    p.line_segment(
        [pos2(rect.left() + rad, rect.bottom() - 1.5), pos2(rect.right() - rad, rect.bottom() - 1.5)],
        Stroke::new(1.0, Color32::from_black_alpha(110)),
    );
    p.rect_stroke(rect, rad, Stroke::new(1.0, alpha(accent, 0.55)), StrokeKind::Inside);
    p.rect_stroke(rect.shrink(1.5), (rad - 1.5).max(0.0), Stroke::new(1.0, add(accent, 0.06)), StrokeKind::Inside);
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
    let f = FontId::monospace(size);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        p.text(pos + vec2(dx, dy) * 1.6, anchor, text, f.clone(), add(col, 0.22));
    }
    p.text(pos, anchor, text, f, col);
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
    let s = segs as u32;
    for i in 0..s {
        let (a0, a1) = (1 + i, 1 + (i + 1) % s);
        let (b0, b1) = (1 + s + i, 1 + s + (i + 1) % s);
        m.add_triangle(0, a0, a1);
        m.add_triangle(a0, b0, b1);
        m.add_triangle(a0, b1, a1);
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

/// Shiny bevelled sphere with halo. `hot` (0..1) boosts the glow (sweep pings, selection).
pub fn orb(p: &Painter, c: Pos2, r: f32, col: Color32, hot: f32) {
    glow(p, c, r, col, 0.55 + hot * 1.1);
    p.circle_filled(c + vec2(0.0, 2.5), r + 1.5, Color32::from_black_alpha(110));
    sphere(p, c, r, col, 48);
    // anti-aliased rim (the mesh edge itself is not smoothed) and bevel
    p.circle_stroke(c, r - 0.5, Stroke::new(1.2, shade(col, 0.22)));
    arc(p, c, r - 0.8, PI * 0.95, PI * 1.85, Stroke::new(1.6, add(Color32::WHITE, 0.5)));
    arc(p, c, r - 0.8, -PI * 0.05, PI * 0.85, Stroke::new(1.6, Color32::from_black_alpha(130)));
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
pub fn glow_rect(p: &Painter, r: Rect, g: f32, col: Color32, k: f32) {
    let inner = add(col, k);
    let o = r.expand(g);
    let ins = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    let outs = [o.left_top(), o.right_top(), o.right_bottom(), o.left_bottom()];
    let mut m = Mesh::default();
    for q in ins {
        m.colored_vertex(q, inner);
    }
    for q in outs {
        m.colored_vertex(q, Color32::TRANSPARENT);
    }
    for i in 0..4u32 {
        let j = (i + 1) % 4;
        m.add_triangle(i, j, 4 + i);
        m.add_triangle(j, 4 + j, 4 + i);
    }
    p.add(Shape::mesh(m));
}
