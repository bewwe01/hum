//! Haze screen (effects on the whole mix) and the performance buttons on
//! the Play screen: Haze, Drag, Stutter and Tape stop.

use eframe::egui::{self, pos2, vec2, Align2, Id, Painter, Rect, Stroke};
use hum_engine::engine::Cmd;
use hum_engine::haze::HazeAmounts;

use crate::app::HumApp;
use crate::icons;
use crate::state::{DragDepth, Haze};
use crate::theme::{self, inter, GAP};
use crate::widgets;

const EFFECTS: [(&str, &str); 5] = [
    ("Distortion", "From warm to wrecked"),
    ("Crush", "Lo-fi 8-bit grit"),
    ("Tape", "Wobble, hiss and crackle"),
    ("Shimmer", "A huge, glassy reverb that climbs an octave"),
    ("Pump", "The mix ducks on every beat"),
];

impl HumApp {
    pub(crate) fn haze_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let back = Rect::from_min_size(rect.min, vec2(48.0, 48.0));
        let tap = self.touches.button(Id::unique("haze-back"), back);
        widgets::surface(
            &p,
            back,
            16.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::chevron_left(&p, back.center(), 24.0, theme::TEXT);
        if tap.clicked {
            self.haze_open = false;
        }
        theme::text(
            &p,
            pos2(back.right() + 14.0, back.center().y),
            Align2::LEFT_CENTER,
            "Haze",
            theme::TITLE,
            theme::TEXT,
        );
        let haunted = Rect::from_min_size(
            pos2(rect.right() - 108.0, rect.top() + 2.0),
            vec2(108.0, 44.0),
        );
        let off = Rect::from_min_size(
            pos2(haunted.left() - 72.0, rect.top() + 2.0),
            vec2(64.0, 44.0),
        );
        if self.pill_button(&p, haunted, "haze-haunted", "Haunted", true) {
            self.st.haze = Haze::from_amounts(HazeAmounts::HAUNTED);
        }
        if self.pill_button(&p, off, "haze-off", "Off", false) {
            self.st.haze = Haze::default();
        }

        let body = Rect::from_min_max(pos2(rect.left(), back.bottom() + GAP), rect.max);
        let (sliders, options) = if self.landscape {
            let split = body.left() + body.width() * 0.58;
            (
                Rect::from_min_max(body.min, pos2(split, body.bottom())),
                Rect::from_min_max(pos2(split + GAP, body.top()), body.max),
            )
        } else {
            let options_h = 2.0 * (24.0 + 48.0) + 12.0;
            (
                Rect::from_min_max(
                    body.min,
                    pos2(body.right(), body.bottom() - options_h - GAP),
                ),
                Rect::from_min_max(pos2(body.left(), body.bottom() - options_h), body.max),
            )
        };

        // ---- the five effect sliders ----------------------------------------
        let row_h = (sliders.height() / 5.0).clamp(50.0, 72.0);
        for (i, (name, helper)) in EFFECTS.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(sliders.left(), sliders.top() + i as f32 * row_h),
                vec2(sliders.width(), row_h),
            );
            let track = Rect::from_min_max(
                pos2(r.left(), r.center().y + 2.0),
                pos2(r.right(), r.bottom() - 2.0),
            );
            let id = Id::unique(("haze-slider", i));
            self.touches
                .claim(id, Rect::from_min_max(pos2(r.left(), r.top() + 4.0), r.max));
            let drag = self
                .touches
                .owned(id)
                .filter(|t| !t.ended)
                .last()
                .map(|t| t.pos.x);
            if let Some(x) = drag {
                let v = ((x - track.left() - 10.0) / (track.width() - 20.0)).clamp(0.0, 1.0);
                // Snap to off near the left end.
                *self.st.haze.values_mut()[i] = if v < 0.03 { 0.0 } else { v };
            }
            let value = *self.st.haze.values_mut()[i];
            let label_y = r.top() + (r.height() * 0.3).min(16.0);
            theme::text(
                &p,
                pos2(r.left(), label_y),
                Align2::LEFT_CENTER,
                name,
                inter(15.0, 600),
                theme::TEXT,
            );
            let shown = if value <= 0.0 {
                "Off".to_owned()
            } else {
                format!("{}%", (value * 100.0).round() as i32)
            };
            let name_w = theme::galley(&p, name, inter(15.0, 600), theme::TEXT)
                .size()
                .x;
            let shown_w = theme::galley(&p, &shown, inter(13.0, 400), theme::MUTED)
                .size()
                .x;
            let help = theme::fit(
                &p,
                helper,
                inter(12.0, 400),
                r.width() - name_w - shown_w - 28.0,
            );
            theme::text(
                &p,
                pos2(r.left() + name_w + 10.0, label_y + 1.0),
                Align2::LEFT_CENTER,
                &help,
                inter(12.0, 400),
                theme::MUTED,
            );
            theme::text(
                &p,
                pos2(r.right(), label_y),
                Align2::RIGHT_CENTER,
                &shown,
                inter(13.0, 400),
                theme::MUTED,
            );
            let (x0, x1, cy) = (track.left() + 10.0, track.right() - 10.0, track.center().y);
            p.line_segment([pos2(x0, cy), pos2(x1, cy)], Stroke::new(4.0, theme::LINE));
            let xv = x0 + (x1 - x0) * value;
            p.line_segment(
                [pos2(x0, cy), pos2(xv, cy)],
                Stroke::new(4.0, theme::VIOLET),
            );
            p.circle_filled(
                pos2(xv, cy),
                9.0,
                if value > 0.0 {
                    theme::TEXT
                } else {
                    theme::MUTED
                },
            );
        }

        // ---- Drag depth and stutter length -------------------------------------
        let mut y = options.top();
        theme::text(
            &p,
            pos2(options.left(), y + 10.0),
            Align2::LEFT_CENTER,
            "DRAG",
            theme::SECTION,
            theme::MUTED,
        );
        y += 24.0;
        let choice = self.segmented(
            &p,
            Rect::from_min_size(pos2(options.left(), y), vec2(options.width(), 48.0)),
            "haze-drag",
            &["Drag  −3", "Deep  −5"],
            usize::from(self.st.drag == DragDepth::Deep),
        );
        if let Some(i) = choice {
            self.st.drag = if i == 0 {
                DragDepth::Drag
            } else {
                DragDepth::Deep
            };
            if self.drag_on {
                self.send_drag();
            }
        }
        y += 48.0 + 12.0;
        theme::text(
            &p,
            pos2(options.left(), y + 10.0),
            Align2::LEFT_CENTER,
            "STUTTER",
            theme::SECTION,
            theme::MUTED,
        );
        y += 24.0;
        let choice = self.segmented(
            &p,
            Rect::from_min_size(pos2(options.left(), y), vec2(options.width(), 48.0)),
            "haze-stutter",
            &["1/8 note", "1/16 note"],
            usize::from(self.st.stutter == 16),
        );
        if let Some(i) = choice {
            self.st.stutter = if i == 0 { 8 } else { 16 };
        }
    }

    fn pill_button(
        &mut self,
        p: &Painter,
        r: Rect,
        id: &'static str,
        label: &str,
        primary: bool,
    ) -> bool {
        let tap = self.touches.button(Id::unique(id), r);
        let (fill, border, color) = if primary {
            (theme::AMBER, theme::AMBER_LIGHT, theme::ON_AMBER)
        } else {
            (theme::SURFACE, theme::LINE, theme::TEXT)
        };
        widgets::surface(
            p,
            r,
            r.height() / 2.0,
            widgets::pressed(fill, tap.down),
            border,
        );
        theme::text(
            p,
            r.center(),
            Align2::CENTER_CENTER,
            label,
            inter(14.0, 600),
            color,
        );
        tap.clicked
    }

    /// A segmented control; returns the newly picked index.
    fn segmented(
        &mut self,
        p: &Painter,
        track: Rect,
        id: &'static str,
        labels: &[&str],
        active: usize,
    ) -> Option<usize> {
        p.rect_filled(track, track.height() / 2.0, theme::SURFACE_2);
        let w = (track.width() - 8.0) / labels.len() as f32;
        let mut picked = None;
        for (i, label) in labels.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(track.left() + 4.0 + i as f32 * w, track.top() + 4.0),
                vec2(w, track.height() - 8.0),
            );
            let tap = self.touches.button(Id::unique((id, i)), r);
            let on = i == active;
            if on {
                p.rect_filled(r, r.height() / 2.0, theme::VIOLET);
            }
            let (style, color) = if on {
                (inter(14.0, 600), theme::ON_VIOLET)
            } else {
                (inter(14.0, 500), theme::TEXT)
            };
            theme::text(p, r.center(), Align2::CENTER_CENTER, label, style, color);
            if tap.clicked && !on {
                picked = Some(i);
            }
        }
        picked
    }

    /// Haze, Drag, Stutter (hold) and Tape stop (hold), in a row or a
    /// column.
    pub(crate) fn fx_buttons(&mut self, p: &Painter, rect: Rect) {
        self.fx_seen = true;
        let vertical = rect.height() > rect.width() * 1.5;
        let gap = 8.0;
        let n = 4.0;
        let cell = |i: usize| {
            if vertical {
                let h = (rect.height() - gap * (n - 1.0)) / n;
                Rect::from_min_size(
                    pos2(rect.left(), rect.top() + i as f32 * (h + gap)),
                    vec2(rect.width(), h),
                )
            } else {
                let w = (rect.width() - gap * (n - 1.0)) / n;
                Rect::from_min_size(
                    pos2(rect.left() + i as f32 * (w + gap), rect.top()),
                    vec2(w, rect.height()),
                )
            }
        };
        let labels = ["Haze", "Drag", "Stutter", "Tape stop"];
        for (i, label) in labels.iter().enumerate() {
            let r = cell(i);
            let tap = self.touches.button(Id::unique(("fx", i)), r);
            let lit = match i {
                0 => !self.st.haze.is_off(),
                1 => self.drag_on,
                2 => self.stutter_held,
                _ => self.tape_held,
            };
            match i {
                0 if tap.clicked => self.haze_open = true,
                1 if tap.clicked => {
                    self.drag_on = !self.drag_on;
                    self.send_drag();
                    self.haptic();
                }
                2 => {
                    if tap.pressed && !self.stutter_held {
                        self.stutter_held = true;
                        self.send(Cmd::Stutter(self.st.stutter));
                        self.haptic();
                    } else if self.stutter_held && !tap.down {
                        self.stutter_held = false;
                        self.send(Cmd::Stutter(0));
                    }
                }
                3 => {
                    if tap.pressed && !self.tape_held {
                        self.tape_held = true;
                        self.send(Cmd::TapeStop(true));
                        self.haptic();
                    } else if self.tape_held && !tap.down {
                        self.tape_held = false;
                        self.send(Cmd::TapeStop(false));
                    }
                }
                _ => {}
            }
            let lit = lit || tap.down;
            let (fill, border, color) = if lit {
                (theme::VIOLET, theme::VIOLET_LIGHT, theme::ON_VIOLET)
            } else {
                (theme::SURFACE, theme::LINE, theme::TEXT)
            };
            if lit {
                widgets::glow(p, r, 14.0, theme::alpha(theme::VIOLET, 0.45), 16.0);
            }
            widgets::surface(p, r, 14.0, fill, border);
            let style = inter(if vertical { 12.0 } else { 13.0 }, 600);
            let text = theme::fit(p, label, style, r.width() - 8.0);
            theme::text(p, r.center(), Align2::CENTER_CENTER, &text, style, color);
        }
    }

    /// Let go of Stutter and Tape stop (their button left the screen).
    pub(crate) fn release_fx(&mut self) {
        if self.stutter_held {
            self.stutter_held = false;
            self.send(Cmd::Stutter(0));
        }
        if self.tape_held {
            self.tape_held = false;
            self.send(Cmd::TapeStop(false));
        }
    }
}
