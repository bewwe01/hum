//! Beats screen (spec section 3.5).

use eframe::egui::{self, pos2, vec2, Align2, Id, Rect};
use hum_engine::engine::Cmd;

use crate::app::HumApp;
use crate::icons;
use crate::theme::{self, grotesk, inter, GAP};
use crate::widgets;

pub const DRUM_NAMES: [&str; 4] = ["Kick", "Snare", "Hat", "Clap"];

impl HumApp {
    fn set_pattern(&mut self, drum: usize, bits: u16) {
        self.st.patterns[drum] = bits;
        self.send(Cmd::SetPattern {
            drum: drum as u8,
            bits,
        });
    }

    fn set_rolls(&mut self, drum: usize, bits: u16) {
        self.st.rolls[drum] = bits;
        self.send(Cmd::SetRolls {
            drum: drum as u8,
            bits,
        });
    }

    fn apply_style(&mut self, style: &hum_engine::transport::BeatStyle) {
        for drum in 0..4 {
            self.set_pattern(drum, style.patterns[drum]);
            self.set_rolls(drum, style.rolls[drum]);
        }
        self.st.kit = style.kit;
        self.send(Cmd::SetKit(style.kit));
        self.st.half_time = style.half_time;
        self.send(Cmd::HalfTime(style.half_time));
        self.st.swing = style.swing;
        self.send(Cmd::Swing(style.swing));
        // A loop keeps its own tempo.
        if self.st.looper.is_none() {
            self.set_bpm(style.bpm);
        }
    }

    /// Starter beats and the swing slider.
    pub(crate) fn beat_style_sheet(&mut self, p: &egui::Painter, inner: Rect, top: f32) -> bool {
        let mut close = false;
        let mut y = top;
        for (i, style) in hum_engine::transport::BEAT_STYLES.iter().enumerate() {
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 56.0));
            let tap = self.touches.button(Id::unique(("beat-style", i)), r);
            widgets::surface(
                p,
                r,
                20.0,
                widgets::pressed(theme::SURFACE_2, tap.down),
                theme::LINE,
            );
            theme::text(
                p,
                pos2(r.left() + 18.0, r.center().y - 1.0),
                Align2::LEFT_BOTTOM,
                style.name,
                inter(15.0, 600),
                theme::TEXT,
            );
            let about = theme::fit(p, style.about, inter(12.0, 400), r.width() - 36.0);
            theme::text(
                p,
                pos2(r.left() + 18.0, r.center().y + 2.0),
                Align2::LEFT_TOP,
                &about,
                inter(12.0, 400),
                theme::MUTED,
            );
            if tap.clicked {
                self.apply_style(style);
                self.show_toast(&format!("{} beat loaded", style.name));
                close = true;
            }
            y += 64.0;
        }
        y += 4.0;
        let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 56.0));
        let track = Rect::from_min_max(pos2(r.left(), r.top() + 22.0), r.max);
        let id = Id::unique("beat-swing");
        self.touches.claim(id, track);
        let drag = self
            .touches
            .owned(id)
            .filter(|t| !t.ended)
            .last()
            .map(|t| t.pos.x);
        if let Some(x) = drag {
            let v = ((x - track.left() - 10.0) / (track.width() - 20.0)).clamp(0.0, 1.0);
            let v = if v < 0.04 { 0.0 } else { v };
            if v != self.st.swing {
                self.st.swing = v;
                self.send(Cmd::Swing(v));
            }
        }
        theme::text(
            p,
            pos2(r.left(), r.top() + 10.0),
            Align2::LEFT_CENTER,
            "Swing",
            inter(14.0, 600),
            theme::TEXT,
        );
        let shown = format!("{}%", (self.st.swing * 100.0).round() as i32);
        theme::text(
            p,
            pos2(r.right(), r.top() + 10.0),
            Align2::RIGHT_CENTER,
            &shown,
            inter(13.0, 400),
            theme::MUTED,
        );
        let (x0, x1, cy) = (track.left() + 10.0, track.right() - 10.0, track.center().y);
        p.line_segment(
            [pos2(x0, cy), pos2(x1, cy)],
            egui::Stroke::new(4.0, theme::LINE),
        );
        let xv = x0 + (x1 - x0) * self.st.swing;
        p.line_segment(
            [pos2(x0, cy), pos2(xv, cy)],
            egui::Stroke::new(4.0, theme::VIOLET),
        );
        p.circle_filled(pos2(xv, cy), 9.0, theme::TEXT);
        close
    }

    /// Kit, half-time, roll editing and the Styles sheet.
    fn beat_options(&mut self, p: &egui::Painter, rect: Rect) {
        let gap = 8.0;
        let w = (rect.width() - gap * 3.0) / 4.0;
        let kit = if self.st.kit == 1 {
            "808 kit"
        } else {
            "Classic"
        };
        let labels = [kit, "Half-time", "Rolls", "Styles"];
        for (i, label) in labels.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(rect.left() + i as f32 * (w + gap), rect.top()),
                vec2(w, rect.height()),
            );
            let tap = self.touches.button(Id::unique(("beat-option", i)), r);
            let on = match i {
                0 => self.st.kit == 1,
                1 => self.st.half_time,
                2 => self.roll_mode,
                _ => false,
            };
            let (fill, border, color) = if on {
                (theme::SURFACE_3, theme::VIOLET, theme::TEXT)
            } else {
                (theme::SURFACE, theme::LINE, theme::TEXT)
            };
            widgets::surface(p, r, 22.0, widgets::pressed(fill, tap.down), border);
            let style = inter(13.0, if on { 600 } else { 500 });
            let text = theme::fit(p, label, style, r.width() - 10.0);
            theme::text(p, r.center(), Align2::CENTER_CENTER, &text, style, color);
            if !tap.clicked {
                continue;
            }
            match i {
                0 => {
                    self.st.kit = 1 - self.st.kit.min(1);
                    self.send(Cmd::SetKit(self.st.kit));
                }
                1 => {
                    self.st.half_time = !self.st.half_time;
                    self.send(Cmd::HalfTime(self.st.half_time));
                }
                2 => {
                    self.roll_mode = !self.roll_mode;
                    if self.roll_mode {
                        self.show_toast("Tap a step to give it a triplet roll");
                    }
                }
                _ => self.sheet = Some(crate::app::Sheet::BeatStyle),
            }
        }
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    pub(crate) fn beats_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let mut y = rect.top();

        // ---- header --------------------------------------------------------
        theme::text(
            &p,
            pos2(rect.left(), y + 22.0),
            Align2::LEFT_CENTER,
            "Beats",
            theme::TITLE,
            theme::TEXT,
        );
        self.bpm_pill(&p, rect.right(), y + 22.0);
        y += 44.0 + GAP;

        let playing = self.status.beat_on && self.status.running;
        let step = if playing && self.status.step >= 0 {
            let s = if self.st.half_time {
                self.status.step / 2
            } else {
                self.status.step
            };
            Some((s % 16) as usize)
        } else {
            None
        };

        // Landscape is short: the step grid alone shows the pattern.
        if !self.landscape {
            // ---- overview card -------------------------------------------------
            let row_h = 12.0;
            let row_gap = 8.0;
            let card_h = 28.0 + 4.0 * row_h + 3.0 * row_gap;
            let card = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), card_h));
            widgets::surface(&p, card, 22.0, theme::SURFACE, theme::LINE);
            let cells_left = card.left() + 14.0 + 48.0;
            let cell_w = (card.right() - 14.0 - cells_left - 15.0 * 3.0) / 16.0;
            for (drum, name) in DRUM_NAMES.iter().enumerate() {
                let cy = card.top() + 14.0 + drum as f32 * (row_h + row_gap) + row_h * 0.5;
                let color = if drum == self.drum {
                    theme::AMBER
                } else {
                    theme::MUTED
                };
                theme::text(
                    &p,
                    pos2(card.left() + 14.0, cy),
                    Align2::LEFT_CENTER,
                    name,
                    inter(12.0, 600),
                    color,
                );
                for s in 0..16 {
                    let r = Rect::from_min_size(
                        pos2(cells_left + s as f32 * (cell_w + 3.0), cy - row_h * 0.5),
                        vec2(cell_w, row_h),
                    );
                    let on = self.st.patterns[drum] >> s & 1 == 1;
                    let mut color = if on { theme::VIOLET } else { theme::LINE };
                    if on && self.st.rolls[self.drum] >> s & 1 == 1 {
                        // A roll: three small dots.
                        for k in 0..3 {
                            p.circle_filled(
                                r.left_top() + vec2(14.0 + k as f32 * 8.0, 14.0),
                                2.8,
                                theme::ON_VIOLET,
                            );
                        }
                    }
                    if step == Some(s) {
                        color = if on { theme::AMBER } else { theme::BAR_LINE };
                    }
                    p.rect_filled(r, 4.0, color);
                }
            }
            y += card_h + GAP;
        }

        // ---- drum chips ----------------------------------------------------
        let gap = 8.0;
        let chip_w = (rect.width() - gap * 3.0) / 4.0;
        for (drum, name) in DRUM_NAMES.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(rect.left() + drum as f32 * (chip_w + gap), y),
                vec2(chip_w, 44.0),
            );
            let tap = self.touches.button(Id::unique(("drum-chip", drum)), r);
            if tap.pressed && self.drum != drum {
                self.drum = drum;
            }
            if self.drum == drum {
                widgets::surface(&p, r, 22.0, theme::VIOLET, theme::VIOLET_LIGHT);
                theme::text(
                    &p,
                    r.center(),
                    Align2::CENTER_CENTER,
                    name,
                    inter(14.0, 600),
                    theme::ON_VIOLET,
                );
            } else {
                widgets::surface(
                    &p,
                    r,
                    22.0,
                    widgets::pressed(theme::SURFACE, tap.down),
                    theme::LINE,
                );
                theme::text(
                    &p,
                    r.center(),
                    Align2::CENTER_CENTER,
                    name,
                    inter(14.0, 500),
                    theme::TEXT,
                );
            }
        }
        y += 44.0 + GAP;

        // ---- kit, half-time, rolls, styles -------------------------------------
        self.beat_options(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 44.0)),
        );
        y += 44.0 + GAP;

        // ---- transport row (bottom) ----------------------------------------
        let transport = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 64.0), rect.max);
        let play_c = pos2(transport.left() + 32.0, transport.center().y);
        let tap = self.touches.button(
            Id::unique("beat-play"),
            Rect::from_center_size(play_c, vec2(64.0, 64.0)),
        );
        widgets::glow_circle(&p, play_c, 32.0, theme::alpha(theme::AMBER, 0.4), 26.0);
        p.circle_filled(play_c, 32.0, widgets::pressed(theme::AMBER, tap.down));
        if self.status.beat_on {
            icons::stop(&p, play_c, 32.0, theme::ON_AMBER);
        } else {
            icons::play(&p, play_c + vec2(2.0, 0.0), 32.0, theme::ON_AMBER);
        }
        if tap.clicked {
            self.send(Cmd::BeatPlay(!self.status.beat_on));
        }

        let pills_left = play_c.x + 32.0 + 12.0;
        let pills_w = transport.right() - pills_left;
        let clear_w = (pills_w - 8.0) * 0.4;
        let clear = Rect::from_min_size(
            pos2(pills_left, transport.center().y - 24.0),
            vec2(clear_w, 48.0),
        );
        let surprise = Rect::from_min_max(
            pos2(clear.right() + 8.0, clear.top()),
            pos2(transport.right(), clear.bottom()),
        );
        let tap = self.touches.button(Id::unique("beat-clear"), clear);
        widgets::surface(
            &p,
            clear,
            24.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        pill_content(&p, clear, "Clear", |p, c| {
            icons::trash(p, c, 20.0, theme::TEXT)
        });
        if tap.clicked {
            self.set_pattern(self.drum, 0);
        }
        let tap = self.touches.button(Id::unique("beat-surprise"), surprise);
        widgets::surface(
            &p,
            surprise,
            24.0,
            widgets::pressed(theme::SURFACE_3, tap.down),
            theme::VIOLET,
        );
        pill_content(&p, surprise, "Surprise me", |p, c| {
            icons::dice(p, c, 20.0, theme::TEXT)
        });
        if tap.clicked {
            // Roughly 35% of the steps, and never an empty pattern.
            let mut bits = 0u16;
            for s in 0..16 {
                if self.random() % 100 < 35 {
                    bits |= 1 << s;
                }
            }
            if bits == 0 {
                bits = 1 << (self.random() % 16);
            }
            self.set_pattern(self.drum, bits);
        }

        // ---- step grid (fills the space in between) --------------------------
        let grid = Rect::from_min_max(
            pos2(rect.left(), y),
            pos2(rect.right(), transport.top() - GAP),
        );
        let (cols, rows) = if self.landscape { (8, 2) } else { (4, 4) };
        let pad_w = (grid.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let pad_h = ((grid.height() - gap * (rows as f32 - 1.0)) / rows as f32).max(24.0);
        for s in 0..16usize {
            let r = Rect::from_min_size(
                pos2(
                    grid.left() + (s % cols) as f32 * (pad_w + gap),
                    grid.top() + (s / cols) as f32 * (pad_h + gap),
                ),
                vec2(pad_w, pad_h),
            );
            let tap = self.touches.button(Id::unique(("step", s)), r);
            if tap.pressed {
                // Toggle on touch-down so tapping feels immediate.
                let bit = 1u16 << s;
                let (pattern, rolls) = (self.st.patterns[self.drum], self.st.rolls[self.drum]);
                if self.roll_mode {
                    self.set_rolls(self.drum, rolls ^ bit);
                    if rolls & bit == 0 && pattern & bit == 0 {
                        self.set_pattern(self.drum, pattern | bit);
                    }
                } else {
                    self.set_pattern(self.drum, pattern ^ bit);
                    if pattern & bit != 0 && rolls & bit != 0 {
                        self.set_rolls(self.drum, rolls & !bit);
                    }
                }
                self.haptic();
            }
            let on = self.st.patterns[self.drum] >> s & 1 == 1;
            let number = format!("{}", s + 1);
            if on {
                widgets::surface(&p, r, 16.0, theme::VIOLET, theme::VIOLET_LIGHT);
                theme::text(
                    &p,
                    r.left_bottom() + vec2(12.0, -10.0),
                    Align2::LEFT_BOTTOM,
                    &number,
                    grotesk(14.0, 600),
                    theme::ON_VIOLET,
                );
            } else {
                widgets::surface(
                    &p,
                    r,
                    16.0,
                    widgets::pressed(theme::SURFACE_2, tap.down),
                    theme::LINE,
                );
                theme::text(
                    &p,
                    r.left_bottom() + vec2(12.0, -10.0),
                    Align2::LEFT_BOTTOM,
                    &number,
                    grotesk(14.0, 600),
                    theme::MUTED,
                );
            }
            if step == Some(s) {
                let c = r.right_top() + vec2(-15.0, 15.0);
                if on {
                    p.circle_filled(c, 7.0, theme::ON_VIOLET);
                }
                p.circle_filled(c, 5.0, theme::AMBER);
            }
        }
    }
}

/// Icon plus label, centred together in a pill.
fn pill_content(
    p: &egui::Painter,
    rect: Rect,
    label: &str,
    icon: impl FnOnce(&egui::Painter, egui::Pos2),
) {
    let style = inter(15.0, 600);
    let text_w = theme::galley(p, label, style, theme::TEXT).size().x;
    let total = 20.0 + 8.0 + text_w;
    let x = rect.center().x - total * 0.5;
    icon(p, pos2(x + 10.0, rect.center().y));
    theme::text(
        p,
        pos2(x + 28.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        style,
        theme::TEXT,
    );
}
