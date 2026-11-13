//! Loop screen: the layer stack of the capture looper.

use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Painter, Rect, Shape, Stroke};
use hum_engine::engine::{Cmd, LAYERS};

use crate::app::{HumApp, Sheet, Tab};
use crate::icons;
use crate::theme::{self, grotesk, inter, GAP};
use crate::widgets;

const ROW_H: f32 = 56.0;
const ROW_GAP: f32 = 6.0;

/// Colour of each layer's notes, dot and switch.
pub fn layer_color(layer: usize) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(0xA7, 0x8B, 0xFA),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0x5E, 0xD3, 0xB3),
        Color32::from_rgb(0xF4, 0x72, 0xB6),
        Color32::from_rgb(0x60, 0xA5, 0xFA),
        Color32::from_rgb(0xC4, 0xB5, 0xFD),
        Color32::from_rgb(0xFF, 0xD9, 0xA0),
        Color32::from_rgb(0xA3, 0xE6, 0x35),
    ];
    COLORS[layer % COLORS.len()]
}

impl HumApp {
    pub(crate) fn loop_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let mut y = rect.top();

        // ---- header --------------------------------------------------------
        theme::text(
            &p,
            pos2(rect.left(), y + 22.0),
            Align2::LEFT_CENTER,
            "Loop",
            theme::TITLE,
            theme::TEXT,
        );
        let metro_c = pos2(rect.right() - 22.0, y + 22.0);
        let tap = self.touches.button(
            Id::unique("metronome"),
            Rect::from_center_size(metro_c, vec2(44.0, 44.0)),
        );
        if self.st.metronome {
            widgets::circle(&p, metro_c, 22.0, theme::SURFACE_3, theme::VIOLET);
        } else {
            widgets::circle(
                &p,
                metro_c,
                22.0,
                widgets::pressed(theme::SURFACE, tap.down),
                theme::LINE,
            );
        }
        icons::metronome(
            &p,
            metro_c,
            24.0,
            if self.st.metronome {
                theme::TEXT
            } else {
                theme::MUTED
            },
        );
        if tap.clicked {
            self.st.metronome = !self.st.metronome;
            self.send(Cmd::Metronome(self.st.metronome));
        }
        self.bpm_pill(&p, rect.right() - 44.0 - 8.0, y + 22.0);
        y += 44.0 + GAP;

        // ---- layout ----------------------------------------------------------
        let keys_h = (rect.height() * 0.15).clamp(80.0, 120.0);
        let transport_h = 72.0;
        let timeline_h = 128.0;
        let keys = Rect::from_min_max(pos2(rect.left(), rect.bottom() - keys_h), rect.max);
        let transport = Rect::from_min_size(
            pos2(rect.left(), keys.top() - GAP - transport_h),
            vec2(rect.width(), transport_h),
        );
        let timeline = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), timeline_h));
        let list = Rect::from_min_max(
            pos2(rect.left(), timeline.bottom() + GAP),
            pos2(rect.right(), transport.top() - GAP),
        );

        self.timeline(&p, timeline);
        self.layer_list(ui, &p, list);
        self.loop_transport(&p, transport);
        self.keyboard(&p, keys, false);
    }

    fn timeline(&mut self, p: &Painter, rect: Rect) {
        widgets::surface(p, rect, 24.0, theme::SURFACE, theme::LINE);
        let inner = rect.shrink(14.0);
        let status = self.status;
        let steps = self.st.looper.as_ref().map_or(64, |l| l.steps);
        let count = self.st.looper.as_ref().map_or(0, |l| l.count());
        let playing = status.running && status.loop_on && count > 0;

        let (dot, text) = if count == 0 {
            (theme::MUTED, "Play something, then tap Loop".to_owned())
        } else if playing {
            (
                theme::AMBER,
                format!(
                    "Playing · {count} layer{}",
                    if count == 1 { "" } else { "s" }
                ),
            )
        } else {
            (theme::MUTED, "Paused".to_owned())
        };
        let line_y = inner.top() + 8.0;
        p.circle_filled(pos2(inner.left() + 5.0, line_y), 5.0, dot);
        theme::text(
            p,
            pos2(inner.left() + 18.0, line_y),
            Align2::LEFT_CENTER,
            &text,
            inter(14.0, 600),
            theme::TEXT,
        );
        if count > 0 {
            let bars = usize::from(steps) / 16;
            let info = format!(
                "{bars} bar{} · {} BPM",
                if bars == 1 { "" } else { "s" },
                self.st.bpm.round() as i32
            );
            theme::text(
                p,
                pos2(inner.right(), line_y),
                Align2::RIGHT_CENTER,
                &info,
                inter(12.0, 400),
                theme::MUTED,
            );
        }

        let area = Rect::from_min_max(pos2(inner.left(), inner.top() + 24.0), inner.max);
        for beat in 0..=(usize::from(steps) / 4) {
            let x = area.left() + area.width() * (beat * 4) as f32 / f32::from(steps);
            let color = if beat % 4 == 0 {
                theme::BAR_LINE
            } else {
                theme::LINE_FAINT
            };
            p.line_segment(
                [pos2(x, area.top()), pos2(x, area.bottom())],
                Stroke::new(1.0, color),
            );
        }
        let Some(looper) = &self.st.looper else {
            return;
        };
        let (mut lo, mut hi) = (127u8, 0u8);
        for n in looper.layers.iter().flatten().flat_map(|l| l.notes.iter()) {
            lo = lo.min(n.note);
            hi = hi.max(n.note);
        }
        let span = f32::from(hi.saturating_sub(lo).max(7));
        let lane = area.shrink2(vec2(0.0, 3.0));
        let x_of = |t: f32| lane.left() + lane.width() * (t / f32::from(steps)).clamp(0.0, 1.0);
        let clip = p.with_clip_rect(area);
        for (slot, layer) in looper.layers.iter().enumerate() {
            let Some(layer) = layer else { continue };
            let color = if layer.muted {
                theme::alpha(theme::MUTED, 0.25)
            } else {
                layer_color(slot)
            };
            for n in &layer.notes {
                let y = lane.bottom()
                    - 6.0
                    - (lane.height() - 6.0) * f32::from(n.note.saturating_sub(lo)) / span;
                let segments: [(f32, f32); 2] = if n.off >= n.on {
                    [(n.on, n.off), (0.0, 0.0)]
                } else {
                    [(n.on, f32::from(steps)), (0.0, n.off)]
                };
                for (a, b) in segments {
                    if b > a {
                        let x0 = x_of(a);
                        clip.rect_filled(
                            Rect::from_min_max(pos2(x0, y), pos2(x_of(b).max(x0 + 3.0), y + 6.0)),
                            3.0,
                            color,
                        );
                    }
                }
            }
        }
        if status.running {
            let x = x_of(status.pos.max(0.0));
            p.line_segment(
                [pos2(x, area.top() - 2.0), pos2(x, area.bottom())],
                Stroke::new(2.0, theme::AMBER),
            );
            let tip = pos2(x, area.top() + 3.0);
            p.add(Shape::convex_polygon(
                vec![tip + vec2(-5.0, -7.0), tip + vec2(5.0, -7.0), tip],
                theme::AMBER,
                Stroke::NONE,
            ));
        }
    }

    fn layer_list(&mut self, ui: &egui::Ui, p: &Painter, rect: Rect) {
        let slots: Vec<usize> = (0..LAYERS).filter(|&s| self.layer(s).is_some()).collect();
        if slots.is_empty() {
            widgets::dashed_round_rect(
                p,
                Rect::from_min_size(rect.min, vec2(rect.width(), ROW_H)),
                18.0,
                Stroke::new(1.0, theme::DASHED_EMPTY),
            );
            let hint = "Your layers appear here. Each Loop tap adds one.";
            theme::text(
                p,
                pos2(rect.left() + 16.0, rect.top() + ROW_H / 2.0),
                Align2::LEFT_CENTER,
                hint,
                inter(13.0, 500),
                theme::MUTED,
            );
            return;
        }
        // Vertical scrolling when the rows do not fit.
        let id = Id::unique("layer-list");
        self.touches.claim(id, rect);
        let content = slots.len() as f32 * (ROW_H + ROW_GAP) - ROW_GAP;
        let max_scroll = (content - rect.height()).max(0.0);
        let scroll_id = Id::unique("layer-scroll");
        let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
        for t in self.touches.owned(id) {
            if !t.ended && t.travel > crate::input::TAP_SLOP {
                scroll -= t.delta().y;
            }
        }
        if ui.rect_contains_pointer(rect) {
            scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        scroll = scroll.clamp(0.0, max_scroll);
        ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
        let clip = p.with_clip_rect(rect);
        for (i, &slot) in slots.iter().enumerate() {
            let row = Rect::from_min_size(
                pos2(
                    rect.left(),
                    rect.top() - scroll + i as f32 * (ROW_H + ROW_GAP),
                ),
                vec2(rect.width(), ROW_H),
            );
            if row.bottom() < rect.top() || row.top() > rect.bottom() {
                continue;
            }
            self.layer_row(&clip, row, rect, slot);
        }
    }

    fn layer_row(&mut self, p: &Painter, row: Rect, visible: Rect, slot: usize) {
        let Some(layer) = self.layer(slot).cloned() else {
            return;
        };
        let color = layer_color(slot);
        widgets::surface(p, row, 18.0, theme::SURFACE, theme::LINE);
        let hit = |r: Rect| r.intersect(visible);

        // On/off switch, tinted with the layer colour.
        let switch = Rect::from_min_size(pos2(row.left() + 6.0, row.top() + 6.0), vec2(56.0, 44.0));
        let tap = self
            .touches
            .button(Id::unique(("layer-on", slot)), hit(switch));
        let on = !layer.muted;
        let track = Rect::from_center_size(switch.center(), vec2(44.0, 26.0));
        p.rect_filled(track, 13.0, if on { color } else { theme::LINE });
        let knob_x = if on {
            track.right() - 13.0
        } else {
            track.left() + 13.0
        };
        p.circle_filled(
            pos2(knob_x, track.center().y),
            10.0,
            if on { theme::TEXT } else { theme::MUTED },
        );
        if tap.clicked {
            self.toggle_layer(slot);
        }

        // Buttons on the right: tidy timing, delete.
        let trash = Rect::from_min_size(
            pos2(row.right() - 4.0 - 40.0, row.top() + 6.0),
            vec2(40.0, 44.0),
        );
        let tidy =
            Rect::from_min_size(pos2(trash.left() - 40.0, row.top() + 6.0), vec2(40.0, 44.0));
        let tap = self
            .touches
            .button(Id::unique(("layer-tidy", slot)), hit(tidy));
        if layer.tidy {
            p.rect_filled(
                Rect::from_center_size(tidy.center(), vec2(34.0, 30.0)),
                10.0,
                theme::SURFACE_3,
            );
        }
        icons::grid(
            p,
            tidy.center(),
            20.0,
            if layer.tidy {
                theme::VIOLET
            } else {
                theme::MUTED
            },
        );
        if tap.clicked {
            self.toggle_tidy(slot);
            self.show_toast(if layer.tidy {
                "Tidy timing off"
            } else {
                "Tidy timing on: notes snap to the beat"
            });
        }
        let tap = self
            .touches
            .button(Id::unique(("layer-delete", slot)), hit(trash));
        icons::trash(
            p,
            trash.center(),
            22.0,
            if tap.down { theme::TEXT } else { theme::MUTED },
        );
        if tap.clicked {
            self.delete_layer(slot);
            self.show_toast("Layer deleted (Redo brings it back)");
        }

        // Volume slider.
        let slider = Rect::from_min_max(
            pos2(tidy.left() - 76.0, row.top()),
            pos2(tidy.left() - 6.0, row.bottom()),
        );
        let id = Id::unique(("layer-volume", slot));
        self.touches.claim(id, hit(slider));
        let drag: Option<f32> = self
            .touches
            .owned(id)
            .filter(|t| !t.ended)
            .last()
            .map(|t| t.pos.x);
        if let Some(x) = drag {
            self.set_layer_volume(slot, (x - slider.left() - 6.0) / (slider.width() - 12.0));
        }
        let volume = self.layer(slot).map_or(1.0, |l| l.volume);
        let (x0, x1, cy) = (
            slider.left() + 6.0,
            slider.right() - 6.0,
            slider.center().y + 9.0,
        );
        p.line_segment([pos2(x0, cy), pos2(x1, cy)], Stroke::new(4.0, theme::LINE));
        let xv = x0 + (x1 - x0) * volume;
        p.line_segment(
            [pos2(x0, cy), pos2(xv, cy)],
            Stroke::new(4.0, if on { color } else { theme::MUTED }),
        );
        p.circle_filled(pos2(xv, cy), 7.0, theme::TEXT);
        theme::text(
            p,
            pos2(slider.center().x, row.top() + 15.0),
            Align2::CENTER_CENTER,
            "Volume",
            inter(11.0, 500),
            theme::MUTED,
        );

        // Name and sound; tapping chooses another sound for this layer.
        let text = Rect::from_min_max(
            pos2(switch.right() + 4.0, row.top()),
            pos2(slider.left() - 4.0, row.bottom()),
        );
        let tap = self
            .touches
            .button(Id::unique(("layer-sound", slot)), hit(text));
        if tap.down {
            p.rect_filled(text.shrink2(vec2(0.0, 4.0)), 10.0, theme::SURFACE_2);
        }
        let name_color = if on { theme::TEXT } else { theme::MUTED };
        theme::text(
            p,
            pos2(text.left() + 6.0, row.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            &format!("Layer {}", slot + 1),
            inter(14.0, 600),
            name_color,
        );
        let sound = theme::fit(
            p,
            &format!("{} ›", layer.sound),
            inter(12.0, 400),
            text.width() - 8.0,
        );
        theme::text(
            p,
            pos2(text.left() + 6.0, row.center().y + 2.0),
            Align2::LEFT_TOP,
            &sound,
            inter(12.0, 400),
            theme::MUTED,
        );
        if tap.clicked {
            self.assign_layer = Some(slot);
            self.tab = Tab::Sounds;
        }
    }

    fn loop_transport(&mut self, p: &Painter, rect: Rect) {
        let cy = rect.center().y;
        let gap = 10.0;
        let total = 56.0 * 4.0 + 72.0 + gap * 4.0;
        let mut x = rect.center().x - total * 0.5;
        let playing = self.status.loop_on && self.status.running;
        let has_layers = self.can_undo();

        // Undo.
        let c = pos2(x + 28.0, cy);
        let tap = self.touches.button(
            Id::unique("loop-undo"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down && has_layers),
            theme::LINE,
        );
        icons::undo(
            p,
            c,
            24.0,
            if has_layers {
                theme::TEXT
            } else {
                theme::alpha(theme::MUTED, 0.45)
            },
        );
        if tap.clicked && has_layers {
            self.undo();
        }
        x += 56.0 + gap;

        // Loop (capture).
        let c = pos2(x + 36.0, cy);
        self.loop_button(p, c, 36.0, "loop-capture");
        x += 72.0 + gap;

        // Redo.
        let c = pos2(x + 28.0, cy);
        let can_redo = !self.redo.is_empty();
        let tap = self.touches.button(
            Id::unique("loop-redo"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down && can_redo),
            theme::LINE,
        );
        let dim = theme::alpha(theme::MUTED, 0.45);
        icons::redo(p, c, 24.0, if can_redo { theme::TEXT } else { dim });
        if tap.clicked && can_redo {
            self.redo();
        }
        x += 56.0 + gap;

        // Play / pause.
        let c = pos2(x + 28.0, cy);
        let tap = self.touches.button(
            Id::unique("loop-play"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down),
            if playing { theme::VIOLET } else { theme::LINE },
        );
        if playing {
            icons::pause(p, c, 26.0, theme::TEXT);
        } else {
            icons::play(
                p,
                c + vec2(1.5, 0.0),
                26.0,
                if has_layers { theme::TEXT } else { dim },
            );
        }
        if tap.clicked && has_layers {
            self.send(Cmd::LoopPlay(!playing));
        }
        x += 56.0 + gap;

        // Clear all (asks first).
        let c = pos2(x + 28.0, cy);
        let tap = self.touches.button(
            Id::unique("loop-clear-all"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down && has_layers),
            theme::LINE,
        );
        icons::trash(p, c, 24.0, if has_layers { theme::TEXT } else { dim });
        if tap.clicked && has_layers {
            self.sheet = Some(Sheet::ConfirmClearAll);
        }
    }

    /// The big amber Loop button (used on the Play and Loop screens).
    pub(crate) fn loop_button(&mut self, p: &Painter, c: egui::Pos2, r: f32, id: &'static str) {
        let tap = self.touches.button(
            Id::unique(id),
            Rect::from_center_size(c, vec2(r * 2.0, r * 2.0)),
        );
        widgets::glow_circle(p, c, r, theme::alpha(theme::AMBER, 0.35), 24.0);
        p.circle_filled(c, r, theme::AMBER_LIGHT);
        p.circle_filled(c, r - 3.5, widgets::pressed(theme::AMBER, tap.down));
        icons::loop_arrows(p, c - vec2(0.0, r * 0.18), r * 0.62, theme::ON_AMBER);
        theme::text(
            p,
            c + vec2(0.0, r * 0.45),
            Align2::CENTER_CENTER,
            "Loop",
            grotesk(r * 0.36, 700),
            theme::ON_AMBER,
        );
        if tap.pressed {
            self.capture();
        }
    }
}
