//! Hand-drawn vector icons (no icon font, no bitmaps). Every icon is drawn
//! centred on `c` inside a `size` x `size` box with 2-point strokes.

use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use hum_engine::Wave;
use std::f32::consts::{PI, TAU};

const W: f32 = 2.0;

fn stroke(color: Color32) -> Stroke {
    Stroke::new(W, color)
}

/// A poly-line with rounded ends and joints.
pub fn polyline(p: &Painter, points: &[Pos2], width: f32, color: Color32) {
    if points.len() < 2 {
        return;
    }
    p.add(Shape::line(points.to_vec(), Stroke::new(width, color)));
    for end in [points[0], points[points.len() - 1]] {
        p.circle_filled(end, width * 0.5, color);
    }
}

/// Points along a circular arc. Angles are in degrees, clockwise from the
/// positive x axis (screen coordinates).
pub fn arc_points(c: Pos2, radius: f32, from_deg: f32, to_deg: f32) -> Vec<Pos2> {
    let sweep = to_deg - from_deg;
    let n = ((sweep.abs() / 6.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = (from_deg + sweep * i as f32 / n as f32).to_radians();
            c + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

pub fn arc(
    p: &Painter,
    c: Pos2,
    radius: f32,
    from_deg: f32,
    to_deg: f32,
    width: f32,
    color: Color32,
) {
    polyline(p, &arc_points(c, radius, from_deg, to_deg), width, color);
}

/// A small waveform glyph inside `rect` (used for sound cards and the picker).
pub fn wave(p: &Painter, rect: Rect, wave: Wave, color: Color32) {
    let (l, r, t, b) = (
        rect.left(),
        rect.right(),
        rect.top() + 1.0,
        rect.bottom() - 1.0,
    );
    let mid = rect.center().y;
    let w = rect.width();
    let pts: Vec<Pos2> = match wave {
        Wave::Sine => (0..=24)
            .map(|i| {
                let x = i as f32 / 24.0;
                pos2(l + x * w, mid - (x * TAU).sin() * (b - t) * 0.5)
            })
            .collect(),
        Wave::Saw => vec![
            pos2(l, b),
            pos2(l + w * 0.5, t),
            pos2(l + w * 0.5, b),
            pos2(r, t),
            pos2(r, b),
        ],
        Wave::Square => vec![
            pos2(l, b),
            pos2(l, t),
            pos2(l + w * 0.5, t),
            pos2(l + w * 0.5, b),
            pos2(r, b),
            pos2(r, t),
        ],
        Wave::Triangle => vec![
            pos2(l, mid),
            pos2(l + w * 0.25, t),
            pos2(l + w * 0.75, b),
            pos2(r, mid),
        ],
        Wave::Noise => {
            const Y: [f32; 9] = [0.1, -0.8, 0.5, -0.3, 0.9, -0.6, 0.2, -0.9, 0.4];
            Y.iter()
                .enumerate()
                .map(|(i, y)| pos2(l + w * i as f32 / 8.0, mid - y * (b - t) * 0.5))
                .collect()
        }
    };
    polyline(p, &pts, W, color);
}

pub fn chevron_down(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    polyline(
        p,
        &[
            c + vec2(-s, -s * 0.5),
            c + vec2(0.0, s * 0.5),
            c + vec2(s, -s * 0.5),
        ],
        W,
        color,
    );
}

pub fn chevron_left(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    polyline(
        p,
        &[
            c + vec2(s * 0.5, -s),
            c + vec2(-s * 0.5, 0.0),
            c + vec2(s * 0.5, s),
        ],
        W,
        color,
    );
}

pub fn sliders(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let h = size * 0.36;
    for (i, knob) in [0.3f32, -0.25, 0.1].iter().enumerate() {
        let y = c.y + (i as f32 - 1.0) * size * 0.27;
        polyline(p, &[pos2(c.x - h, y), pos2(c.x + h, y)], W, color);
        let k = pos2(c.x + knob * size, y);
        p.circle_filled(k, 3.5, color);
    }
}

pub fn minus(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    polyline(p, &[c + vec2(-s, 0.0), c + vec2(s, 0.0)], W, color);
}

pub fn plus(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    polyline(p, &[c + vec2(-s, 0.0), c + vec2(s, 0.0)], W, color);
    polyline(p, &[c + vec2(0.0, -s), c + vec2(0.0, s)], W, color);
}

pub fn magnifier(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let r = size * 0.24;
    let centre = c + vec2(-size * 0.06, -size * 0.06);
    p.circle_stroke(centre, r, stroke(color));
    let d = vec2(1.0, 1.0).normalized();
    polyline(
        p,
        &[centre + d * r, centre + d * (r + size * 0.2)],
        W,
        color,
    );
}

/// Filled "play" triangle.
pub fn play(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.3;
    let pts = vec![
        c + vec2(-s * 0.7, -s),
        c + vec2(s, 0.0),
        c + vec2(-s * 0.7, s),
    ];
    p.add(Shape::convex_polygon(pts, color, Stroke::new(1.5, color)));
}

/// Filled, slightly rounded "stop" square.
pub fn stop(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    p.rect_filled(
        Rect::from_center_size(c, vec2(s * 2.0, s * 2.0)),
        3.0,
        color,
    );
}

pub fn metronome(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let h = size * 0.36;
    let top = size * 0.12;
    let bottom = size * 0.3;
    let body = [
        c + vec2(-bottom, h),
        c + vec2(-top, -h),
        c + vec2(top, -h),
        c + vec2(bottom, h),
        c + vec2(-bottom, h),
    ];
    polyline(p, &body, W, color);
    polyline(
        p,
        &[c + vec2(0.0, h * 0.55), c + vec2(size * 0.26, -h * 0.75)],
        W,
        color,
    );
}

/// Speaker; with `muted` it gets a cross, otherwise sound waves.
pub fn speaker(p: &Painter, c: Pos2, size: f32, color: Color32, muted: bool) {
    let s = size * 0.15;
    let x0 = c.x - size * 0.3;
    let body = [
        pos2(x0, c.y - s),
        pos2(x0 + s * 1.1, c.y - s),
        pos2(x0 + s * 2.6, c.y - s * 2.3),
        pos2(x0 + s * 2.6, c.y + s * 2.3),
        pos2(x0 + s * 1.1, c.y + s),
        pos2(x0, c.y + s),
        pos2(x0, c.y - s),
    ];
    polyline(p, &body, W, color);
    let right = pos2(c.x + size * 0.2, c.y);
    if muted {
        let d = size * 0.11;
        polyline(p, &[right + vec2(-d, -d), right + vec2(d, d)], W, color);
        polyline(p, &[right + vec2(-d, d), right + vec2(d, -d)], W, color);
    } else {
        arc(
            p,
            pos2(c.x + size * 0.02, c.y),
            size * 0.2,
            -45.0,
            45.0,
            W,
            color,
        );
        arc(
            p,
            pos2(c.x + size * 0.02, c.y),
            size * 0.34,
            -40.0,
            40.0,
            W,
            color,
        );
    }
}

pub fn trash(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let w = size * 0.24;
    let top = c.y - size * 0.2;
    let bottom = c.y + size * 0.34;
    polyline(
        p,
        &[pos2(c.x - w * 1.35, top), pos2(c.x + w * 1.35, top)],
        W,
        color,
    );
    polyline(
        p,
        &[
            pos2(c.x - w * 0.45, top),
            pos2(c.x - w * 0.45, top - size * 0.12),
            pos2(c.x + w * 0.45, top - size * 0.12),
            pos2(c.x + w * 0.45, top),
        ],
        W,
        color,
    );
    polyline(
        p,
        &[
            pos2(c.x - w, top),
            pos2(c.x - w * 0.8, bottom),
            pos2(c.x + w * 0.8, bottom),
            pos2(c.x + w, top),
        ],
        W,
        color,
    );
    for dx in [-w * 0.3, w * 0.3] {
        polyline(
            p,
            &[
                pos2(c.x + dx, top + size * 0.14),
                pos2(c.x + dx, bottom - size * 0.12),
            ],
            W,
            color,
        );
    }
}

/// Counter-clockwise arrow.
pub fn undo(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let r = size * 0.3;
    let centre = c + vec2(0.0, size * 0.04);
    arc(p, centre, r, -150.0, 110.0, W, color);
    let tip = centre
        + vec2(
            (-150.0f32).to_radians().cos(),
            (-150.0f32).to_radians().sin(),
        ) * r;
    let a = size * 0.2;
    polyline(
        p,
        &[tip + vec2(-a * 0.2, -a), tip, tip + vec2(a, -a * 0.15)],
        W,
        color,
    );
}

/// Two arrows chasing each other in a circle.
pub fn loop_arrows(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let r = size * 0.32;
    for base in [0.0f32, 180.0] {
        arc(p, c, r, base + 20.0, base + 150.0, W, color);
        let a = (base + 150.0).to_radians();
        let tip = c + vec2(a.cos(), a.sin()) * r;
        let tangent = vec2(-a.sin(), a.cos());
        let normal = vec2(a.cos(), a.sin());
        let s = size * 0.15;
        polyline(
            p,
            &[
                tip - tangent * s + normal * s * 0.9,
                tip,
                tip - tangent * s - normal * s * 0.9,
            ],
            W,
            color,
        );
    }
}

/// Piano keys (the Play tab).
pub fn keys(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let w = size * 0.4;
    let h = size * 0.34;
    let rect = Rect::from_center_size(c, vec2(w * 2.0, h * 2.0));
    p.rect_stroke(rect, 4.0, stroke(color), eframe::egui::StrokeKind::Middle);
    for dx in [-w / 3.0, w / 3.0] {
        polyline(
            p,
            &[pos2(c.x + dx, c.y - h * 0.1), pos2(c.x + dx, c.y + h)],
            W,
            color,
        );
        p.rect_filled(
            Rect::from_min_max(
                pos2(c.x + dx - 2.2, c.y - h),
                pos2(c.x + dx + 2.2, c.y + h * 0.05),
            ),
            1.0,
            color,
        );
    }
}

/// Level bars (the Sounds tab).
pub fn bars(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let heights = [0.3f32, 0.7, 0.45, 0.85, 0.35];
    for (i, h) in heights.iter().enumerate() {
        let x = c.x + (i as f32 - 2.0) * size * 0.17;
        let half = size * 0.4 * h;
        polyline(p, &[pos2(x, c.y - half), pos2(x, c.y + half)], W, color);
    }
}

/// A 3x3 grid of step dots (the Beats tab).
pub fn grid(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let on = [true, false, true, false, true, true, true, false, false];
    for (i, lit) in on.iter().enumerate() {
        let x = c.x + ((i % 3) as f32 - 1.0) * size * 0.28;
        let y = c.y + ((i / 3) as f32 - 1.0) * size * 0.28;
        let r = Rect::from_center_size(pos2(x, y), vec2(size * 0.19, size * 0.19));
        if *lit {
            p.rect_filled(r, 1.5, color);
        } else {
            p.rect_stroke(
                r,
                1.5,
                Stroke::new(1.5, color),
                eframe::egui::StrokeKind::Inside,
            );
        }
    }
}

pub fn dice(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 0.34;
    let rect = Rect::from_center_size(c, vec2(s * 2.0, s * 2.0));
    p.rect_stroke(rect, 4.0, stroke(color), eframe::egui::StrokeKind::Middle);
    let d = s * 0.48;
    for (dx, dy) in [(-d, -d), (d, -d), (0.0, 0.0), (-d, d), (d, d)] {
        p.circle_filled(c + vec2(dx, dy), 1.6, color);
    }
}

pub fn shield(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let w = size * 0.32;
    let top = c.y - size * 0.38;
    let mut pts = vec![
        pos2(c.x, top),
        pos2(c.x + w, top + size * 0.12),
        pos2(c.x + w, c.y + size * 0.02),
    ];
    // Curved bottom.
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        let a = t * PI * 0.5;
        pts.push(pos2(
            c.x + w * a.cos(),
            c.y + size * 0.02 + size * 0.38 * a.sin(),
        ));
    }
    let mut left: Vec<Pos2> = pts[1..pts.len() - 1]
        .iter()
        .rev()
        .map(|q| pos2(2.0 * c.x - q.x, q.y))
        .collect();
    pts.append(&mut left);
    pts.push(pos2(c.x, top));
    polyline(p, &pts, W, color);
    // Check mark.
    polyline(
        p,
        &[
            c + vec2(-size * 0.12, size * 0.02),
            c + vec2(-size * 0.02, size * 0.12),
            c + vec2(size * 0.14, -size * 0.08),
        ],
        W,
        color,
    );
}

/// Musical flat sign, `height` tall, with its left edge at `pos.x` and its
/// bottom at `pos.y`. Returns the width used.
pub fn flat(p: &Painter, pos: Pos2, height: f32, color: Color32) -> f32 {
    let w = height * 0.36;
    let stroke_w = (height * 0.1).clamp(1.2, 2.2);
    let x = pos.x + stroke_w * 0.5;
    let top = pos.y - height;
    let bottom = pos.y;
    polyline(p, &[pos2(x, top), pos2(x, bottom)], stroke_w, color);
    // The bowl: from the middle of the stem, out to the right, back to the foot.
    let mid = pos.y - height * 0.48;
    let mut pts = Vec::with_capacity(12);
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        // Quadratic Bezier: (x, mid) -> control (x + 2w, mid - small) -> (x, bottom).
        let a = pos2(x, mid);
        let ctl = pos2(x + w * 2.1, mid - height * 0.12);
        let b = pos2(x, bottom);
        let q = a.to_vec2() * (1.0 - t) * (1.0 - t)
            + ctl.to_vec2() * 2.0 * t * (1.0 - t)
            + b.to_vec2() * t * t;
        pts.push(q.to_pos2());
    }
    polyline(p, &pts, stroke_w, color);
    w + stroke_w * 1.5
}
