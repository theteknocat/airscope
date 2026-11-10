//! Date and time picker: a field that shows a moment ("Nov 9, 2026  18:30") and opens a calendar with hour and
//! minute steppers. Drawn with the app's own panel/lcd styling; local time via libc (no date library).

use crate::fx::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn local(unix: i64) -> libc::tm {
    let secs = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    tm
}

/// Local date and time to unix seconds (months 0-11; out-of-range days roll over like mktime).
pub fn make_unix(y: i32, m0: i32, d: i32, h: i32, mi: i32) -> i64 {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = y - 1900;
    tm.tm_mon = m0;
    tm.tm_mday = d;
    tm.tm_hour = h;
    tm.tm_min = mi;
    tm.tm_isdst = -1;
    unsafe { libc::mktime(&mut tm) as i64 }
}

pub fn days_in(y: i32, m0: i32) -> i32 {
    let (ny, nm) = if m0 == 11 { (y + 1, 0) } else { (y, m0 + 1) };
    ((make_unix(ny, nm, 1, 12, 0) - make_unix(y, m0, 1, 12, 0)) as f64 / 86400.0).round() as i32
}

/// Weekday of a date, Monday = 0.
fn weekday(y: i32, m0: i32, d: i32) -> i32 {
    (local(make_unix(y, m0, d, 12, 0)).tm_wday + 6) % 7
}

/// "Nov 9, 2026  18:30" (the app's date format, 24-hour time).
pub fn show(unix: i64) -> String {
    let tm = local(unix);
    format!("{}  {:02}:{:02}", crate::snoop::date(unix as f64), tm.tm_hour, tm.tm_min)
}

/// Width a field needs for the widest date and time it can show (and the placeholder): measured once, with the widest
/// month name and digits, so a field never has more room than it needs and never clips.
pub fn field_width(ui: &egui::Ui, placeholder: &str) -> f32 {
    let key = egui::Id::new(("dt_field_w", placeholder));
    if let Some(w) = ui.ctx().data(|d| d.get_temp::<f32>(key)) {
        return w;
    }
    const SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let font = FontId::proportional(14.0);
    let measure = |s: String| ui.painter().layout_no_wrap(s, font.clone(), Color32::WHITE).size().x;
    let mut widest = measure(placeholder.to_string());
    for m in SHORT {
        for d in '0'..='9' {
            widest = widest.max(measure(format!("{m} {d}{d}, 2{d}{d}{d}  {d}{d}:{d}{d}")));
        }
    }
    let w = (widest + 14.0 + 34.0).ceil(); // left padding, and room for the arrow on the right
    ui.ctx().data_mut(|dt| dt.insert_temp(key, w));
    w
}

/// A field that opens the calendar. `value` is None until something is picked (`placeholder` shows meanwhile).
/// `bad` outlines it in red. Returns true when the value changed.
pub fn field(ui: &mut egui::Ui, id: egui::Id, value: &mut Option<i64>, placeholder: &str, bad: bool, now: i64) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(field_width(ui, placeholder), 36.0), Sense::click());
    let open_id = id.with("open");
    let ctx = ui.ctx().clone();
    let mut open: bool = ctx.data(|d| d.get_temp(open_id)).unwrap_or(false);
    if resp.clicked() {
        open = !open;
        if open {
            let tm = local(value.unwrap_or(now));
            ctx.data_mut(|d| d.insert_temp(id.with("month"), (tm.tm_year + 1900, tm.tm_mon)));
        }
    }
    let p = ui.painter();
    let accent = if bad { RED } else if open { CYAN } else { BLUE };
    if resp.hovered() || open {
        glow_rect_r(p, r, 10.0, 8.0, accent, 0.25);
    }
    panel(p, r, 10.0, if resp.hovered() && !bad { CYAN } else { accent });
    let (txt, col) = match value {
        Some(v) => (show(*v), Color32::WHITE),
        None => (placeholder.to_string(), DIM),
    };
    p.text(pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, txt, FontId::proportional(14.0), col);
    let c = pos2(r.right() - 18.0, r.center().y);
    p.line_segment([c + vec2(-4.5, -2.0), c + vec2(0.0, 2.5)], Stroke::new(1.5, DIM)); // a small down arrow
    p.line_segment([c + vec2(0.0, 2.5), c + vec2(4.5, -2.0)], Stroke::new(1.5, DIM));
    let _ = ui.interact(r, id.with("hand"), Sense::hover()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut changed = false;
    if open {
        let (mut y, mut m0): (i32, i32) = ctx.data(|d| d.get_temp(id.with("month"))).unwrap_or_else(|| {
            let tm = local(value.unwrap_or(now));
            (tm.tm_year + 1900, tm.tm_mon)
        });
        let area = egui::Area::new(id.with("pop")).order(egui::Order::Foreground).fixed_pos(pos2(r.left(), r.bottom() + 6.0)).show(&ctx, |ui| {
            let (card, _) = ui.allocate_exact_size(vec2(288.0, 332.0), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(card, 14.0, BG);
            panel(&p, card, 14.0, CYAN);
            // month header with arrows
            let head = Rect::from_min_size(card.min + vec2(12.0, 10.0), vec2(card.width() - 24.0, 30.0));
            for (dx, glyph, step) in [(0.0, "<", -1), (head.width() - 30.0, ">", 1)] {
                let br = Rect::from_min_size(head.min + vec2(dx, 0.0), vec2(30.0, 30.0));
                let rs = crate::hit(ui, br, id.with(("step", step)), Sense::click());
                p.rect_filled(br, 8.0, alpha(CYAN, if rs.hovered() { 0.2 } else { 0.08 }));
                p.text(br.center(), Align2::CENTER_CENTER, glyph, FontId::monospace(14.0), CYAN);
                if rs.clicked() {
                    let n = y * 12 + m0 + step;
                    (y, m0) = (n.div_euclid(12), n.rem_euclid(12));
                }
            }
            p.text(head.center(), Align2::CENTER_CENTER, format!("{} {}", MONTHS[m0 as usize], y), FontId::proportional(15.0), Color32::WHITE);
            // weekday letters and the day grid (Monday first)
            let gx = card.left() + 12.0;
            let cw = (card.width() - 24.0) / 7.0;
            for (i, l) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                p.text(pos2(gx + (i as f32 + 0.5) * cw, card.top() + 54.0), Align2::CENTER_CENTER, l, FontId::proportional(12.0), DIM);
            }
            let first = weekday(y, m0, 1);
            let today = local(now);
            let cur = value.map(|v| local(v));
            let (h, mi) = cur.as_ref().map_or((today.tm_hour, today.tm_min), |t| (t.tm_hour, t.tm_min));
            for day in 1..=days_in(y, m0) {
                let cell = (first + day - 1) as usize;
                let cr = Rect::from_min_size(pos2(gx + (cell % 7) as f32 * cw, card.top() + 66.0 + (cell / 7) as f32 * 32.0), vec2(cw, 32.0)).shrink(2.0);
                let rs = crate::hit(ui, cr, id.with(("day", day)), Sense::click());
                let sel = cur.as_ref().map_or(false, |t| t.tm_year + 1900 == y && t.tm_mon == m0 && t.tm_mday == day);
                let is_today = today.tm_year + 1900 == y && today.tm_mon == m0 && today.tm_mday == day;
                if sel {
                    p.rect_filled(cr, 8.0, CYAN);
                } else if rs.hovered() {
                    p.rect_filled(cr, 8.0, alpha(CYAN, 0.18));
                }
                if is_today && !sel {
                    p.rect_stroke(cr, 8.0, Stroke::new(1.2, alpha(CYAN, 0.7)), egui::StrokeKind::Inside);
                }
                p.text(cr.center(), Align2::CENTER_CENTER, day.to_string(), FontId::proportional(13.5), if sel { Color32::from_rgb(8, 16, 30) } else { TEXT });
                if rs.clicked() {
                    *value = Some(make_unix(y, m0, day, h, mi));
                    changed = true;
                }
            }
            // time: hour and minute steppers, and a Now button
            let ty = card.bottom() - 52.0;
            p.line_segment([pos2(card.left() + 14.0, ty - 12.0), pos2(card.right() - 14.0, ty - 12.0)], Stroke::new(1.0, alpha(CYAN, 0.25)));
            let base = value.unwrap_or(now);
            let t0 = local(base);
            p.text(pos2(card.left() + 116.0, ty + 15.0), Align2::CENTER_CENTER, ":", FontId::monospace(20.0), DIM);
            for (k, (cx, dh, dm)) in [(card.left() + 62.0, 1, 0), (card.left() + 170.0, 0, 5)].into_iter().enumerate() {
                let val = if k == 0 { t0.tm_hour } else { t0.tm_min };
                lcd(&p, pos2(cx, ty + 15.0), Align2::CENTER_CENTER, &format!("{:02}", val), 20.0, CYAN);
                for (j, (g, sign)) in [("-", -1), ("+", 1)].into_iter().enumerate() {
                    let br = Rect::from_center_size(pos2(cx - 30.0 + j as f32 * 60.0, ty + 15.0), vec2(22.0, 26.0));
                    let rs = crate::hit(ui, br, id.with(("t", k, j)), Sense::click());
                    p.rect_filled(br, 7.0, alpha(CYAN, if rs.hovered() { 0.2 } else { 0.08 }));
                    p.text(br.center(), Align2::CENTER_CENTER, g, FontId::monospace(14.0), CYAN);
                    if rs.clicked() {
                        let (nh, nm) = ((t0.tm_hour + sign * dh).rem_euclid(24), (t0.tm_min + sign * dm).rem_euclid(60));
                        *value = Some(make_unix(t0.tm_year + 1900, t0.tm_mon, t0.tm_mday, nh, nm));
                        changed = true;
                    }
                }
            }
            let nr = Rect::from_min_size(pos2(card.right() - 62.0, ty + 2.0), vec2(48.0, 26.0));
            let rs = crate::hit(ui, nr, id.with("now"), Sense::click());
            p.rect_filled(nr, 8.0, alpha(CYAN, if rs.hovered() { 0.25 } else { 0.12 }));
            p.text(nr.center(), Align2::CENTER_CENTER, "Now", FontId::proportional(13.0), CYAN);
            if rs.clicked() {
                *value = Some(now / 60 * 60);
                changed = true;
            }
        });
        // keep the month the user navigated to; close on a click outside, or Escape
        ctx.data_mut(|d| d.insert_temp(id.with("month"), (y, m0)));
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().map_or(false, |q| !area.response.rect.contains(q) && !r.contains(q)));
        if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(open_id, open));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_lengths_include_leap_years() {
        assert_eq!(days_in(2026, 1), 28);
        assert_eq!(days_in(2028, 1), 29);
        assert_eq!(days_in(2026, 10), 30);
        assert_eq!(days_in(2026, 11), 31);
    }

    #[test]
    fn weekday_counts_from_monday() {
        assert_eq!(weekday(2026, 10, 9), 0); // Monday, Nov 9, 2026
        assert_eq!(weekday(2026, 10, 15), 6); // Sunday
    }

    #[test]
    fn a_picked_moment_reads_back_the_same() {
        let u = make_unix(2026, 10, 9, 18, 30);
        let t = local(u);
        assert_eq!((t.tm_year + 1900, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min), (2026, 10, 9, 18, 30));
        assert!(show(u).ends_with("18:30"));
    }
}
