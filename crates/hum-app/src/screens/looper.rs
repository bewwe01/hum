//! Loop screen (spec section 3.4).

use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Rect, Shape, Stroke};
use hum_engine::engine::{Cmd, RecState, LAYERS};
use hum_engine::presets::PRESETS;
use hum_engine::transport::LOOP_STEPS;

use crate::app::{HumApp, Sheet};
use crate::icons;
use crate::theme::{self, inter, GAP};
use crate::widgets;

/// Colour of each layer's notes and list dot.
pub fn layer_color(layer: usize) -> Color32 {
    match layer {
        0 => theme::VIOLET,
        1 => theme::alpha(theme::VIOLET_LIGHT, 0.6),
        2 => theme::alpha(theme::AMBER_LIGHT, 0.75),
        _ => theme::alpha(theme::MUTED, 0.9),
    }
}

impl HumApp {
    fn layer_filled(&self, layer: usize) -> bool {
        self.status.layers >> layer & 1 == 1
    }

    fn recording_layer(&self) -> Option<usize> {
        (self.status.rec != RecState::Idle).then_some(self.status.rec_layer)
    }

    fn clear_layer(&mut self, layer: usize) {
        self.send(Cmd::ClearLayer(layer as u8));
        self.layers[layer].notes.clear();
        self.rec_order.retain(|l| *l != layer);
    }

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

        // ---- vertical budget ------------------------------------------------
        let list_h = LAYERS as f32 * 52.0 + (LAYERS - 1) as f32 * 8.0;
        let fixed = GAP + list_h + GAP + 72.0 + GAP + 96.0;
        let timeline_h = (rect.bottom() - y - fixed).clamp(110.0, 176.0);

        self.timeline(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), timeline_h)),
        );
        y += timeline_h + GAP;

        for layer in 0..LAYERS {
            let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 52.0));
            self.layer_row(&p, row, layer);
            y += 52.0 + 8.0;
        }
        y += GAP - 8.0;

        self.loop_transport(
            &p,
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 72.0)),
        );
        y += 72.0 + GAP;

        // ---- compact keyboard ------------------------------------------------
        let keys = Rect::from_min_max(pos2(rect.left(), y), rect.max);
        if keys.height() >= 40.0 {
            self.keyboard(&p, keys, false);
        }
    }

    fn timeline(&mut self, p: &egui::Painter, rect: Rect) {
        widgets::surface(p, rect, 26.0, theme::SURFACE, theme::LINE);
        let inner = rect.shrink(14.0);
        let status = self.status;
        let pos = status.pos;
        let layers_any = status.layers != 0;

        // Status line.
        let (dot, text) = match status.rec {
            RecState::CountIn => (theme::AMBER, "Count-in: get ready".to_owned()),
            RecState::Recording => (
                theme::AMBER,
                format!("Recording layer {}", status.rec_layer + 1),
            ),
            RecState::Idle if status.running && status.loop_on && layers_any => {
                (theme::VIOLET, "Playing".to_owned())
            }
            RecState::Idle if status.layers == 0b1111 => {
                (theme::MUTED, "All 4 layers are full".to_owned())
            }
            RecState::Idle if layers_any => (theme::MUTED, "Stopped".to_owned()),
            RecState::Idle => (theme::MUTED, "Tap record to start".to_owned()),
        };
        let line_y = inner.top() + 10.0;
        p.circle_filled(pos2(inner.left() + 5.0, line_y), 5.0, dot);
        theme::text(
            p,
            pos2(inner.left() + 18.0, line_y),
            Align2::LEFT_CENTER,
            &text,
            inter(14.0, 600),
            theme::TEXT,
        );
        let where_text = if !status.running {
            "4 bars".to_owned()
        } else if pos < 0.0 {
            format!(
                "Count-in · Beat {}",
                ((pos + 16.0) / 4.0).floor().clamp(0.0, 3.0) as i32 + 1
            )
        } else {
            format!(
                "Bar {} · Beat {}",
                (pos / 16.0).floor() as i32 % 4 + 1,
                ((pos % 16.0) / 4.0).floor() as i32 + 1
            )
        };
        theme::text(
            p,
            pos2(inner.right(), line_y),
            Align2::RIGHT_CENTER,
            &where_text,
            inter(12.0, 400),
            theme::MUTED,
        );

        // Bar labels.
        let labels_y = inner.top() + 20.0 + 8.0;
        let area = Rect::from_min_max(pos2(inner.left(), labels_y + 16.0 + 6.0), inner.max);
        let bar_w = area.width() / 4.0;
        let current_bar = if status.running && pos >= 0.0 {
            Some((pos / 16.0).floor() as usize % 4)
        } else {
            None
        };
        for bar in 0..4 {
            let (style, color) = if current_bar == Some(bar) {
                (inter(12.0, 600), theme::AMBER)
            } else {
                (inter(12.0, 400), theme::MUTED)
            };
            theme::text(
                p,
                pos2(area.left() + bar as f32 * bar_w, labels_y),
                Align2::LEFT_TOP,
                &format!("Bar {}", bar + 1),
                style,
                color,
            );
        }

        // Grid: beat lines and bar lines.
        for beat in 0..=16 {
            let x = area.left() + area.width() * beat as f32 / 16.0;
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

        // Notes. Pitch decides the row; all layers share one pitch range.
        let (mut lo, mut hi) = (127u8, 0u8);
        for layer in &self.layers {
            for n in &layer.notes {
                lo = lo.min(n.note);
                hi = hi.max(n.note);
            }
        }
        let span = f32::from(hi.saturating_sub(lo).max(7));
        let lane = area.shrink2(vec2(0.0, 4.0));
        let x_of = |t: f32| lane.left() + lane.width() * (t / LOOP_STEPS as f32).clamp(0.0, 1.0);
        let clip = p.with_clip_rect(area);
        for (index, layer) in self.layers.iter().enumerate() {
            let recording = self.recording_layer() == Some(index);
            if !self.layer_filled(index) && !recording {
                continue;
            }
            let mut color = layer_color(index);
            if status.muted >> index & 1 == 1 {
                color = theme::alpha(theme::MUTED, 0.3);
            }
            for n in &layer.notes {
                let y = lane.bottom()
                    - 9.0
                    - (lane.height() - 9.0) * f32::from(n.note.saturating_sub(lo)) / span;
                // A note still being held grows up to the playhead.
                let end = n.end.unwrap_or(if recording {
                    pos.rem_euclid(LOOP_STEPS as f32)
                } else {
                    n.start + 0.5
                });
                let mut segments = [(n.start, end), (0.0, 0.0)];
                if end < n.start {
                    // The note crosses the loop point.
                    segments = [(n.start, LOOP_STEPS as f32), (0.0, end)];
                }
                for (a, b) in segments {
                    if b <= a && !(a == n.start && b == end) {
                        continue;
                    }
                    let x0 = x_of(a);
                    let x1 = x_of(b).max(x0 + 4.0);
                    clip.rect_filled(
                        Rect::from_min_max(pos2(x0, y), pos2(x1, y + 9.0)),
                        4.5,
                        color,
                    );
                }
            }
        }

        // Playhead.
        if status.running {
            let x = x_of(pos.max(0.0));
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

    fn layer_row(&mut self, p: &egui::Painter, row: Rect, layer: usize) {
        let armed = self.recording_layer() == Some(layer);
        let filled = self.layer_filled(layer);
        let dot = pos2(row.left() + 16.0 + 5.0, row.center().y);
        let text_x = dot.x + 5.0 + 12.0;
        if armed {
            widgets::surface(p, row, 18.0, theme::SURFACE_3, theme::AMBER);
            p.circle_filled(dot, 5.0, theme::AMBER);
            let sound = PRESETS[self.layers[layer].sound.min(PRESETS.len() - 1)].name;
            let text = if self.status.rec == RecState::CountIn {
                format!("Get ready to record with {sound}")
            } else {
                format!("Play now to record with {sound}")
            };
            let text = theme::fit(p, &text, inter(14.0, 600), row.right() - 14.0 - text_x);
            theme::text(
                p,
                pos2(text_x, row.center().y),
                Align2::LEFT_CENTER,
                &text,
                inter(14.0, 600),
                theme::TEXT,
            );
        } else if filled {
            let muted = self.status.muted >> layer & 1 == 1;
            widgets::surface(p, row, 18.0, theme::SURFACE, theme::LINE);
            p.circle_filled(dot, 5.0, layer_color(layer));
            let name_color = if muted { theme::MUTED } else { theme::TEXT };
            theme::text(
                p,
                pos2(text_x, row.center().y - 1.0),
                Align2::LEFT_BOTTOM,
                &format!("Layer {}", layer + 1),
                inter(14.0, 600),
                name_color,
            );
            let sound = PRESETS[self.layers[layer].sound.min(PRESETS.len() - 1)].name;
            let caption = if muted {
                format!("{sound} · muted")
            } else {
                sound.to_owned()
            };
            theme::text(
                p,
                pos2(text_x, row.center().y + 2.0),
                Align2::LEFT_TOP,
                &caption,
                inter(12.0, 400),
                theme::MUTED,
            );

            let clear = Rect::from_min_size(
                pos2(row.right() - 4.0 - 44.0, row.top() + 4.0),
                vec2(44.0, 44.0),
            );
            let mute =
                Rect::from_min_size(pos2(clear.left() - 44.0, row.top() + 4.0), vec2(44.0, 44.0));
            let tap = self.touches.button(Id::unique(("layer-mute", layer)), mute);
            if tap.down {
                p.circle_filled(mute.center(), 20.0, theme::SURFACE_2);
            }
            icons::speaker(
                p,
                mute.center(),
                24.0,
                if muted { theme::AMBER } else { theme::TEXT },
                muted,
            );
            if tap.clicked {
                self.send(Cmd::MuteLayer {
                    layer: layer as u8,
                    muted: !muted,
                });
            }
            let tap = self
                .touches
                .button(Id::unique(("layer-clear", layer)), clear);
            if tap.down {
                p.circle_filled(clear.center(), 20.0, theme::SURFACE_2);
            }
            icons::trash(p, clear.center(), 24.0, theme::MUTED);
            if tap.clicked {
                self.clear_layer(layer);
            }
        } else {
            widgets::dashed_round_rect(p, row, 18.0, Stroke::new(1.0, theme::DASHED_EMPTY));
            p.circle_stroke(dot, 4.5, Stroke::new(1.0, theme::DASHED_EMPTY));
            theme::text(
                p,
                pos2(text_x, row.center().y),
                Align2::LEFT_CENTER,
                "Empty",
                inter(14.0, 500),
                theme::MUTED,
            );
        }
    }

    fn loop_transport(&mut self, p: &egui::Painter, rect: Rect) {
        let cy = rect.center().y;
        let total = 56.0 + 16.0 + 72.0 + 16.0 + 56.0 + 16.0 + 56.0;
        let mut x = rect.center().x - total * 0.5;
        let recording = self.status.rec != RecState::Idle;
        let any_layers = self.status.layers != 0;
        let full = self.status.layers == 0b1111;

        // Undo: removes the last recorded layer (or cancels the current take).
        let c = pos2(x + 28.0, cy);
        let can_undo = recording || self.rec_order.iter().any(|l| self.layer_filled(*l));
        let tap = self.touches.button(
            Id::unique("loop-undo"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down && can_undo),
            theme::LINE,
        );
        icons::undo(
            p,
            c,
            24.0,
            if can_undo {
                theme::TEXT
            } else {
                theme::alpha(theme::MUTED, 0.45)
            },
        );
        if tap.clicked {
            if let Some(layer) = self.recording_layer() {
                self.clear_layer(layer);
            } else {
                while let Some(layer) = self.rec_order.pop() {
                    if self.layer_filled(layer) {
                        self.clear_layer(layer);
                        break;
                    }
                }
            }
        }
        x += 56.0 + 16.0;

        // Record.
        let c = pos2(x + 36.0, cy);
        let tap = self.touches.button(
            Id::unique("loop-record"),
            Rect::from_center_size(c, vec2(72.0, 72.0)),
        );
        let enabled = recording || !full;
        let fill = if enabled {
            theme::AMBER
        } else {
            theme::alpha(theme::AMBER, 0.35)
        };
        if enabled {
            widgets::glow_circle(
                p,
                c,
                36.0,
                theme::alpha(theme::AMBER, if recording { 0.45 } else { 0.35 }),
                28.0,
            );
        }
        p.circle_filled(c, 36.0, theme::AMBER_LIGHT);
        p.circle_filled(c, 32.0, widgets::pressed(fill, tap.down));
        if recording {
            icons::stop(p, c, 40.0, theme::ON_AMBER);
        } else {
            p.circle_filled(c, 9.0, theme::ON_AMBER);
        }
        if tap.clicked {
            if recording {
                self.send(Cmd::RecordStop);
            } else if let Some(slot) = (0..LAYERS).find(|l| !self.layer_filled(*l)) {
                self.layers[slot].notes.clear();
                self.layers[slot].sound = self.st.sound_index();
                self.send(Cmd::RecordStart);
            }
        }
        x += 72.0 + 16.0;

        // Play / stop the loop.
        let c = pos2(x + 28.0, cy);
        let playing = self.status.loop_on && self.status.running;
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
            icons::stop(p, c, 28.0, theme::TEXT);
        } else {
            icons::play(p, c + vec2(1.5, 0.0), 26.0, theme::TEXT);
        }
        if tap.clicked {
            self.send(Cmd::LoopPlay(!playing));
        }
        x += 56.0 + 16.0;

        // Clear all (asks first).
        let c = pos2(x + 28.0, cy);
        let can_clear = any_layers || recording;
        let tap = self.touches.button(
            Id::unique("loop-clear-all"),
            Rect::from_center_size(c, vec2(56.0, 56.0)),
        );
        widgets::circle(
            p,
            c,
            28.0,
            widgets::pressed(theme::SURFACE, tap.down && can_clear),
            theme::LINE,
        );
        icons::trash(
            p,
            c,
            24.0,
            if can_clear {
                theme::TEXT
            } else {
                theme::alpha(theme::MUTED, 0.45)
            },
        );
        if tap.clicked && can_clear {
            self.sheet = Some(Sheet::ConfirmClearAll);
        }
    }
}
