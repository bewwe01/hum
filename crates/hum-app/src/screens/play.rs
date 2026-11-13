//! Play screen (spec section 3.2).

use eframe::egui::{self, pos2, vec2, Align2, Id, Rect, Stroke};
use hum_engine::engine::{ATTACK_DEFAULT, RELEASE_DEFAULT, VOLUME_DEFAULT};
use hum_engine::presets::PRESETS;

use crate::app::{HumApp, Sheet, Tab};
use crate::icons;
use crate::theme::{self, grotesk, inter, GAP};
use crate::widgets;

/// Seconds the finger trail on the pad stays visible.
const TRAIL_SECONDS: f64 = 0.7;
const DOUBLE_TAP_SECONDS: f64 = 0.35;

impl HumApp {
    pub(crate) fn play_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let mut y = rect.top();

        // ---- header: sound picker + settings button ----------------------
        let settings = Rect::from_min_size(pos2(rect.right() - 56.0, y), vec2(56.0, 56.0));
        let picker =
            Rect::from_min_max(pos2(rect.left(), y), pos2(settings.left() - 10.0, y + 56.0));
        let tap = self.touches.button(Id::unique("picker"), picker);
        widgets::surface(
            &p,
            picker,
            18.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let tile = Rect::from_min_size(
            pos2(picker.left() + 10.0, picker.center().y - 18.0),
            vec2(36.0, 36.0),
        );
        p.rect_filled(tile, 12.0, theme::SURFACE_3);
        let preset = self.preset();
        icons::wave(
            &p,
            Rect::from_center_size(tile.center(), vec2(22.0, 12.0)),
            preset.main_wave(),
            theme::VIOLET,
        );
        let text_x = tile.right() + 12.0;
        let text_w = picker.right() - 40.0 - text_x;
        let name = theme::fit(&p, preset.name, grotesk(18.0, 600), text_w);
        theme::text(
            &p,
            pos2(text_x, picker.top() + 8.0),
            Align2::LEFT_TOP,
            &name,
            grotesk(18.0, 600),
            theme::TEXT,
        );
        let subtitle = format!(
            "{} · {} of {}",
            preset.category.name(),
            self.st.sound_index() + 1,
            PRESETS.len()
        );
        theme::text(
            &p,
            pos2(text_x, picker.bottom() - 9.0),
            Align2::LEFT_BOTTOM,
            &subtitle,
            inter(12.0, 400),
            theme::MUTED,
        );
        icons::chevron_down(
            &p,
            pos2(picker.right() - 22.0, picker.center().y),
            24.0,
            theme::MUTED,
        );
        if tap.clicked {
            self.tab = Tab::Sounds;
        }

        let tap = self.touches.button(Id::unique("settings"), settings);
        widgets::surface(
            &p,
            settings,
            18.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::sliders(&p, settings.center(), 24.0, theme::TEXT);
        if tap.clicked {
            self.settings_open = true;
        }
        y += 56.0 + GAP;

        // ---- scale row -----------------------------------------------------
        self.scale_row(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 44.0)),
        );
        y += 44.0 + GAP;

        // ---- vertical budget ----------------------------------------------
        let knobs_h = 64.0 + 8.0 + 14.0;
        let keys_min = 44.0 + 8.0 + 140.0;
        let pad_h = (rect.bottom() - y - GAP - knobs_h - GAP - keys_min).clamp(120.0, 232.0);

        // ---- XY pad --------------------------------------------------------
        self.xy_pad(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), pad_h)),
        );
        y += pad_h + GAP;

        // ---- knobs ---------------------------------------------------------
        self.knob_row(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), knobs_h)),
        );
        y += knobs_h + GAP;

        // ---- keyboard block ------------------------------------------------
        let stepper = self.octave_stepper(&p, rect.right(), y + 22.0);
        let helper = if self.st.locked {
            "Every key fits the scale, so nothing sounds wrong"
        } else {
            "Lock is off: the keys play every note in a row"
        };
        let helper_w = stepper.left() - 12.0 - rect.left();
        let size = {
            let mut job = egui::text::LayoutJob::single_section(
                helper.to_owned(),
                inter(12.0, 400).format(theme::MUTED),
            );
            job.wrap.max_width = helper_w;
            p.layout_job(job).size()
        };
        widgets::wrapped(
            &p,
            pos2(rect.left(), y + 22.0 - size.y * 0.5),
            helper_w,
            helper,
            inter(12.0, 400),
            theme::MUTED,
        );
        y += 44.0 + 8.0;
        let keys = Rect::from_min_max(pos2(rect.left(), y), rect.max);
        self.keyboard(&p, keys, true);
    }

    fn scale_row(&mut self, p: &egui::Painter, rect: Rect) {
        let gap = 8.0;
        let label = inter(14.0, 400);
        let value = inter(14.0, 600);

        // "Key C"
        let key_label_w = theme::galley(p, "Key", label, theme::MUTED).size().x;
        let key_value_w = widgets::note_label_width(p, self.st.key, value);
        let key_w = 16.0 + key_label_w + 6.0 + key_value_w + 16.0;
        let key_rect = Rect::from_min_size(rect.min, vec2(key_w, 44.0));

        // "Locked"
        let lock_text = if self.st.locked { "Locked" } else { "Unlocked" };
        let lock_style = inter(14.0, if self.st.locked { 600 } else { 500 });
        let lock_w = 16.0
            + 8.0
            + 8.0
            + theme::galley(p, lock_text, lock_style, theme::TEXT)
                .size()
                .x
            + 16.0;
        let lock_rect =
            Rect::from_min_size(pos2(rect.right() - lock_w, rect.top()), vec2(lock_w, 44.0));

        let scale_rect = Rect::from_min_max(
            pos2(key_rect.right() + gap, rect.top()),
            pos2(lock_rect.left() - gap, rect.bottom()),
        );

        let tap = self.touches.button(Id::unique("pill-key"), key_rect);
        widgets::surface(
            p,
            key_rect,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let x = key_rect.left() + 16.0;
        theme::text(
            p,
            pos2(x, key_rect.center().y),
            Align2::LEFT_CENTER,
            "Key",
            label,
            theme::MUTED,
        );
        widgets::note_label(
            p,
            pos2(x + key_label_w + 6.0, key_rect.center().y),
            Align2::LEFT_CENTER,
            self.st.key,
            value,
            theme::TEXT,
        );
        if tap.clicked {
            self.sheet = Some(Sheet::Key);
        }

        let tap = self.touches.button(Id::unique("pill-scale"), scale_rect);
        widgets::surface(
            p,
            scale_rect,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let x = scale_rect.left() + 16.0;
        let scale_name = self.st.scale().name();
        let label_w = theme::galley(p, "Scale", label, theme::MUTED).size().x + 6.0;
        let name_full_w = theme::galley(p, scale_name, value, theme::TEXT).size().x;
        let room = scale_rect.right() - 14.0 - x;
        // On narrow phones the "Scale" label gives way to the scale's name.
        let label_w = if label_w + name_full_w <= room {
            label_w
        } else {
            0.0
        };
        if label_w > 0.0 {
            theme::text(
                p,
                pos2(x, scale_rect.center().y),
                Align2::LEFT_CENTER,
                "Scale",
                label,
                theme::MUTED,
            );
        }
        let name = theme::fit(p, scale_name, value, room - label_w);
        theme::text(
            p,
            pos2(x + label_w, scale_rect.center().y),
            Align2::LEFT_CENTER,
            &name,
            value,
            theme::TEXT,
        );
        if tap.clicked {
            self.sheet = Some(Sheet::Scale);
        }

        let tap = self.touches.button(Id::unique("pill-lock"), lock_rect);
        if self.st.locked {
            widgets::surface(p, lock_rect, 22.0, theme::SURFACE_3, theme::VIOLET);
        } else {
            widgets::surface(p, lock_rect, 22.0, theme::SURFACE, theme::LINE);
        }
        let dot = pos2(lock_rect.left() + 16.0 + 4.0, lock_rect.center().y);
        p.circle_filled(
            dot,
            4.0,
            if self.st.locked {
                theme::AMBER
            } else {
                theme::MUTED
            },
        );
        let color = if self.st.locked {
            theme::TEXT
        } else {
            theme::MUTED
        };
        theme::text(
            p,
            pos2(dot.x + 4.0 + 8.0, lock_rect.center().y),
            Align2::LEFT_CENTER,
            lock_text,
            lock_style,
            color,
        );
        if tap.clicked {
            self.st.locked = !self.st.locked;
            self.set_keymap_changed();
        }
    }

    fn xy_pad(&mut self, p: &egui::Painter, rect: Rect) {
        let id = Id::unique("xy-pad");
        self.touches.claim(id, rect);
        let touch = self
            .touches
            .owned(id)
            .filter(|t| !t.ended)
            .last()
            .map(|t| t.pos);
        if let Some(pos) = touch {
            let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let y = (1.0 - (pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            self.st.pad_x = x;
            self.st.pad_y = y;
            let far_enough = self.trail.last().is_none_or(|(tx, ty, _)| {
                ((tx - x) * rect.width()).hypot((ty - y) * rect.height()) > 7.0
            });
            if far_enough {
                self.trail.push((x, y, self.now));
            }
        }
        let now = self.now;
        self.trail.retain(|(_, _, t)| now - t < TRAIL_SECONDS);

        widgets::surface(p, rect, 26.0, theme::SURFACE, theme::LINE);
        let clip = p.with_clip_rect(rect.shrink(1.0));
        for i in 1..3 {
            let x = rect.left() + rect.width() * i as f32 / 3.0;
            let y = rect.top() + rect.height() * i as f32 / 3.0;
            clip.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, theme::LINE_FAINT),
            );
            clip.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0, theme::LINE_FAINT),
            );
        }
        theme::text(
            p,
            rect.left_top() + vec2(16.0, 14.0),
            Align2::LEFT_TOP,
            "Slide your finger",
            inter(14.0, 600),
            theme::TEXT,
        );
        theme::text(
            p,
            rect.left_top() + vec2(16.0, 34.0),
            Align2::LEFT_TOP,
            "Up = wobblier",
            inter(12.0, 400),
            theme::MUTED,
        );
        theme::text(
            p,
            rect.left_bottom() + vec2(16.0, -12.0),
            Align2::LEFT_BOTTOM,
            "Dull",
            inter(12.0, 400),
            theme::MUTED,
        );
        theme::text(
            p,
            rect.right_bottom() + vec2(-16.0, -12.0),
            Align2::RIGHT_BOTTOM,
            "Bright",
            inter(12.0, 400),
            theme::MUTED,
        );

        let at = |x: f32, y: f32| {
            pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        };
        // Dotted trail that fades out.
        for (x, y, t) in &self.trail {
            let age = ((now - t) / TRAIL_SECONDS) as f32;
            clip.circle_filled(
                at(*x, *y),
                2.5,
                theme::alpha(theme::VIOLET, 0.9 * (1.0 - age)),
            );
        }
        let c = at(self.st.pad_x, self.st.pad_y);
        clip.circle_filled(c, 40.0, theme::alpha(theme::AMBER, 0.16));
        clip.circle_filled(c, 24.0, theme::AMBER);
        clip.circle_filled(c, 8.0, theme::BG);
    }

    fn knob_row(&mut self, p: &egui::Painter, rect: Rect) {
        let gap = 8.0;
        let w = (rect.width() - gap * 3.0) / 4.0;
        let labels = ["ATTACK", "RELEASE", "REVERB", "VOLUME"];
        let defaults = [
            ATTACK_DEFAULT,
            RELEASE_DEFAULT,
            self.preset().reverb,
            VOLUME_DEFAULT,
        ];
        for i in 0..4 {
            let col = Rect::from_min_size(
                pos2(rect.left() + i as f32 * (w + gap), rect.top()),
                vec2(w, rect.height()),
            );
            let id = Id::unique(("knob", i));
            self.touches.claim(id, col);
            let mut delta = 0.0;
            let mut tapped = false;
            for t in self.touches.owned(id) {
                if t.is_tap() {
                    tapped = true;
                } else if !t.ended {
                    delta -= t.delta().y / 160.0;
                }
            }
            let value = match i {
                0 => &mut self.st.attack,
                1 => &mut self.st.release,
                2 => &mut self.st.reverb,
                _ => &mut self.st.volume,
            };
            if delta != 0.0 {
                *value = (*value + delta).clamp(0.0, 1.0);
            }
            if tapped {
                // Double tap resets to the sound's default.
                if self.now - self.knob_last_tap[i] < DOUBLE_TAP_SECONDS {
                    *value = defaults[i];
                    self.knob_last_tap[i] = -10.0;
                } else {
                    self.knob_last_tap[i] = self.now;
                }
            }
            let value = *value;
            let c = pos2(col.center().x, col.top() + 32.0);
            let accent = if i == 3 { theme::AMBER } else { theme::VIOLET };
            widgets::knob(p, c, value, accent);
            theme::text(
                p,
                pos2(col.center().x, col.bottom()),
                Align2::CENTER_BOTTOM,
                labels[i],
                inter(12.0, 600).tracked(0.04),
                theme::MUTED,
            );
        }
    }
}
