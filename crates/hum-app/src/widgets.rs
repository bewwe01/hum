//! Custom-painted building blocks shared by the screens.

use eframe::egui::{
    self, epaint::Shadow, pos2, vec2, Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape,
    Stroke, StrokeKind,
};
use hum_engine::scale::note_name;

use crate::icons;
use crate::theme::{self, Style};

/// A filled rounded rectangle with a 1-point inner border.
pub fn surface(p: &Painter, rect: Rect, radius: f32, fill: Color32, border: Color32) {
    surface_w(p, rect, radius, fill, border, 1.0);
}

pub fn surface_w(p: &Painter, rect: Rect, radius: f32, fill: Color32, border: Color32, width: f32) {
    p.rect(
        rect,
        radius,
        fill,
        Stroke::new(width, border),
        StrokeKind::Inside,
    );
}

/// A soft glow behind a rounded rectangle.
pub fn glow(p: &Painter, rect: Rect, radius: f32, color: Color32, blur: f32) {
    let shadow = Shadow {
        offset: [0, 0],
        blur: blur.clamp(0.0, 255.0) as u8,
        spread: 0,
        color,
    };
    p.add(shadow.as_shape(rect, CornerRadius::from(radius)));
}

/// A soft glow behind a circle.
pub fn glow_circle(p: &Painter, c: Pos2, r: f32, color: Color32, blur: f32) {
    glow(
        p,
        Rect::from_center_size(c, vec2(r * 2.0, r * 2.0)),
        r,
        color,
        blur,
    );
}

/// Dashed outline of a rounded rectangle.
pub fn dashed_round_rect(p: &Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius.min(rect.height() * 0.5).min(rect.width() * 0.5);
    let rect = rect.shrink(stroke.width * 0.5);
    let mut pts: Vec<Pos2> = Vec::with_capacity(48);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    for (c, from) in corners {
        pts.extend(icons::arc_points(c, r, from, from + 90.0));
    }
    pts.push(pts[0]);
    p.extend(Shape::dashed_line(&pts, stroke, 5.0, 4.0));
}

/// Width of a note label such as "E♭" in the given style.
pub fn note_label_width(p: &Painter, note: u8, style: Style) -> f32 {
    let (letter, is_flat) = note_name(note);
    let w = theme::galley(p, letter, style, theme::TEXT).size().x;
    if is_flat {
        w + style.size * 0.42 + 1.0
    } else {
        w
    }
}

/// Paint a note name (letter plus a drawn flat sign where needed).
/// `anchor` positions the whole label relative to `pos`.
pub fn note_label(
    p: &Painter,
    pos: Pos2,
    anchor: Align2,
    note: u8,
    style: Style,
    color: Color32,
) -> Rect {
    let (letter, is_flat) = note_name(note);
    let g = theme::galley(p, letter, style, color);
    let flat_w = if is_flat {
        style.size * 0.42 + 1.0
    } else {
        0.0
    };
    let size = vec2(g.size().x + flat_w, g.size().y);
    let rect = anchor.anchor_size(pos, size);
    let letter_w = g.size().x;
    p.galley(rect.min, g, color);
    if is_flat {
        // Sit the flat on the text baseline (about 78% down the line box).
        let baseline = rect.top() + size.y * 0.80;
        icons::flat(
            p,
            pos2(rect.left() + letter_w + 1.0, baseline),
            style.size * 0.78,
            color,
        );
    }
    rect
}

/// The 64x64 knob from the Play screen. `value` is 0..1.
pub fn knob(p: &Painter, c: Pos2, value: f32, accent: Color32) {
    const START: f32 = 135.0;
    const SWEEP: f32 = 270.0;
    let value = value.clamp(0.0, 1.0);
    icons::arc(p, c, 26.0, START, START + SWEEP, 6.0, theme::LINE);
    if value > 0.004 {
        icons::arc(p, c, 26.0, START, START + SWEEP * value, 6.0, accent);
    }
    p.circle_filled(c, 17.0, theme::SURFACE_2);
    let a = (START + SWEEP * value).to_radians();
    let dir = vec2(a.cos(), a.sin());
    icons::polyline(p, &[c + dir * 6.0, c + dir * 14.0], 3.0, theme::TEXT);
}

/// The 52x32 switch from Settings, centred in `rect`.
pub fn switch(p: &Painter, rect: Rect, on: bool) {
    let track = Rect::from_center_size(rect.center(), vec2(52.0, 32.0));
    p.rect_filled(track, 16.0, if on { theme::VIOLET } else { theme::LINE });
    let x = if on {
        track.right() - 16.0
    } else {
        track.left() + 16.0
    };
    p.circle_filled(
        pos2(x, track.center().y),
        13.0,
        if on { theme::TEXT } else { theme::MUTED },
    );
}

/// A round icon button background.
pub fn circle(p: &Painter, c: Pos2, r: f32, fill: Color32, border: Color32) {
    p.circle(c, r, fill, Stroke::new(1.0, border));
}

/// Darken a fill slightly while a finger is on it.
pub fn pressed(color: Color32, down: bool) -> Color32 {
    if down {
        let f = |c: u8| (f32::from(c) * 1.18 + 8.0).min(255.0) as u8;
        Color32::from_rgb(f(color.r()), f(color.g()), f(color.b()))
    } else {
        color
    }
}

/// Paint text that wraps inside `width`, top-left at `pos`. Returns its size.
pub fn wrapped(
    p: &Painter,
    pos: Pos2,
    width: f32,
    s: &str,
    style: Style,
    color: Color32,
) -> egui::Vec2 {
    let mut job = egui::text::LayoutJob::single_section(s.to_owned(), style.format(color));
    job.wrap.max_width = width;
    let g = p.layout_job(job);
    let size = g.size();
    p.galley(pos, g, color);
    size
}
