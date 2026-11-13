//! Sounds screen (spec section 3.3).

use crate::catalog::Group;
use eframe::egui::{self, pos2, vec2, Align2, Id, Rect};
use hum_engine::engine::Cmd;

use crate::app::{HumApp, Tab};
use crate::icons;
use crate::theme::{self, grotesk, inter, GAP, PAD};
use crate::widgets;

const CARD_H: f32 = 84.0;
const CARD_GAP: f32 = 10.0;

impl HumApp {
    /// Indices of the sounds matching the search text and category chip.
    pub(crate) fn filtered_sounds(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        self.catalog
            .items
            .iter()
            .enumerate()
            .filter(|(_, p)| self.category.is_none_or(|c| p.group == c))
            .filter(|(_, p)| needle.is_empty() || p.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    pub(crate) fn sounds_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let shown = self.filtered_sounds();
        let mut y = rect.top();

        // ---- header --------------------------------------------------------
        theme::text(
            &p,
            pos2(rect.left(), y + 18.0),
            Align2::LEFT_CENTER,
            "Sounds",
            theme::TITLE,
            theme::TEXT,
        );
        let (count, color) = match self.assign_layer {
            Some(slot) => (format!("For Layer {}", slot + 1), theme::AMBER),
            None => (
                format!("{} of {}", shown.len(), self.catalog.items.len()),
                theme::MUTED,
            ),
        };
        theme::text(
            &p,
            pos2(rect.right(), y + 22.0),
            Align2::RIGHT_CENTER,
            &count,
            inter(
                13.0,
                if self.assign_layer.is_some() {
                    600
                } else {
                    400
                },
            ),
            color,
        );
        y += 36.0 + GAP;

        // ---- search field --------------------------------------------------
        let field = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 48.0));
        widgets::surface(&p, field, 24.0, theme::SURFACE, theme::LINE);
        icons::magnifier(
            &p,
            pos2(field.left() + 26.0, field.center().y),
            24.0,
            theme::MUTED,
        );
        let clear = Rect::from_min_size(
            pos2(field.right() - 46.0, field.top() + 2.0),
            vec2(44.0, 44.0),
        );
        let edit_rect = Rect::from_min_max(
            pos2(field.left() + 48.0, field.center().y - 11.0),
            pos2(clear.left(), field.center().y + 11.0),
        );
        let edit = egui::TextEdit::singleline(&mut self.search)
            .hint_text(
                egui::RichText::new(format!("Search {} sounds", self.catalog.items.len()))
                    .color(theme::MUTED),
            )
            .font(egui::FontId::proportional(15.0))
            .text_color(theme::TEXT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(edit_rect.width());
        ui.put(edit_rect, edit);
        if !self.search.is_empty() {
            let tap = self.touches.button(Id::unique("search-clear"), clear);
            let c = clear.center();
            let d = 5.0;
            let color = if tap.down { theme::TEXT } else { theme::MUTED };
            icons::polyline(&p, &[c + vec2(-d, -d), c + vec2(d, d)], 2.0, color);
            icons::polyline(&p, &[c + vec2(-d, d), c + vec2(d, -d)], 2.0, color);
            if tap.clicked {
                self.search.clear();
            }
        }
        y += 48.0 + GAP;

        // ---- category chips (scroll sideways, bleed to the screen edges) ---
        let chips = Rect::from_min_max(
            pos2(rect.left() - PAD, y),
            pos2(rect.right() + PAD, y + 44.0),
        );
        self.category_chips(&p, chips);
        y += 44.0 + GAP;

        // ---- mini player (docked at the bottom) -----------------------------
        let player = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 68.0), rect.max);
        let grid = Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), player.top() - GAP));
        self.sound_grid(ui, &p, grid, &shown);
        self.mini_player(&p, player);
    }

    fn category_chips(&mut self, p: &egui::Painter, rect: Rect) {
        let id = Id::unique("chips");
        self.touches.claim(id, rect);
        let style_on = inter(14.0, 600);
        let style_off = inter(14.0, 500);
        let labels: Vec<(Option<Group>, &str)> = std::iter::once((None, "All"))
            .chain(Group::ALL.iter().map(|c| (Some(*c), c.name())))
            .collect();
        let widths: Vec<f32> = labels
            .iter()
            .map(|(_, l)| theme::galley(p, l, style_on, theme::TEXT).size().x + 36.0)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 8.0 * (widths.len() - 1) as f32 + 2.0 * PAD;
        let max_scroll = (total - rect.width()).max(0.0);

        let scroll_id = Id::unique("chips-scroll");
        let mut scroll: f32 = p.ctx().data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
        let mut tap_at = None;
        for t in self.touches.owned(id) {
            if t.is_tap() {
                tap_at = Some(t.pos);
            } else if !t.ended {
                scroll -= t.delta().x;
            }
        }
        scroll = scroll.clamp(0.0, max_scroll);
        p.ctx().data_mut(|d| d.insert_temp(scroll_id, scroll));

        let clip = p.with_clip_rect(rect);
        let mut x = rect.left() + PAD - scroll;
        for ((cat, label), w) in labels.iter().zip(&widths) {
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, 44.0));
            let selected = self.category == *cat;
            if selected {
                widgets::surface(&clip, r, 22.0, theme::VIOLET, theme::VIOLET_LIGHT);
                theme::text(
                    &clip,
                    r.center(),
                    Align2::CENTER_CENTER,
                    label,
                    style_on,
                    theme::ON_VIOLET,
                );
            } else {
                widgets::surface(&clip, r, 22.0, theme::SURFACE, theme::LINE);
                theme::text(
                    &clip,
                    r.center(),
                    Align2::CENTER_CENTER,
                    label,
                    style_off,
                    theme::TEXT,
                );
            }
            if tap_at.is_some_and(|pos| r.contains(pos)) {
                self.category = *cat;
                p.ctx()
                    .data_mut(|d| d.insert_temp(Id::unique("grid-scroll"), 0.0f32));
            }
            x += w + 8.0;
        }
    }

    fn sound_grid(&mut self, ui: &egui::Ui, p: &egui::Painter, rect: Rect, shown: &[usize]) {
        let id = Id::unique("grid");
        self.touches.claim(id, rect);
        let rows = shown.len().div_ceil(2);
        let content = rows as f32 * CARD_H + rows.saturating_sub(1) as f32 * CARD_GAP;
        let max_scroll = (content - rect.height()).max(0.0);

        let scroll_id = Id::unique("grid-scroll");
        let vel_id = Id::unique("grid-velocity");
        let (mut scroll, mut velocity): (f32, f32) = p.ctx().data(|d| {
            (
                d.get_temp(scroll_id).unwrap_or(0.0),
                d.get_temp(vel_id).unwrap_or(0.0),
            )
        });
        let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
        let mut tap_at = None;
        let mut dragging = false;
        for t in self.touches.owned(id) {
            if t.is_tap() {
                tap_at = Some(t.pos);
            } else if !t.ended {
                dragging = true;
                scroll -= t.delta().y;
                velocity = 0.8 * velocity + 0.2 * (-t.delta().y / dt);
            }
        }
        if !dragging {
            // Coast after a flick, and follow the mouse wheel on desktop.
            if velocity.abs() > 8.0 {
                scroll += velocity * dt;
                velocity *= (-4.0 * dt).exp();
                p.ctx().request_repaint();
            } else {
                velocity = 0.0;
            }
            if ui.rect_contains_pointer(rect) {
                scroll -= ui.input(|i| i.smooth_scroll_delta.y);
            }
        }
        if scroll < 0.0 || scroll > max_scroll {
            velocity = 0.0;
        }
        scroll = scroll.clamp(0.0, max_scroll);
        p.ctx().data_mut(|d| {
            d.insert_temp(scroll_id, scroll);
            d.insert_temp(vel_id, velocity);
        });

        let clip = p.with_clip_rect(rect);
        let w = (rect.width() - CARD_GAP) / 2.0;
        let current = self
            .assigned_sound()
            .or_else(|| self.catalog.index_of(&self.st.sound))
            .unwrap_or(usize::MAX);
        let mut picked = None;
        for (n, &index) in shown.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(
                    rect.left() + (n % 2) as f32 * (w + CARD_GAP),
                    rect.top() - scroll + (n / 2) as f32 * (CARD_H + CARD_GAP),
                ),
                vec2(w, CARD_H),
            );
            if r.bottom() < rect.top() || r.top() > rect.bottom() {
                continue;
            }
            let preset = self.catalog.items[index].clone();
            let selected = index == current;
            if selected {
                widgets::surface_w(&clip, r, 20.0, theme::SURFACE_3, theme::AMBER, 2.0);
            } else {
                widgets::surface(&clip, r, 20.0, theme::SURFACE, theme::LINE);
            }
            let name = theme::fit(&clip, &preset.name, grotesk(16.0, 600), r.width() - 28.0);
            theme::text(
                &clip,
                r.left_top() + vec2(14.0, 12.0),
                Align2::LEFT_TOP,
                &name,
                grotesk(16.0, 600),
                theme::TEXT,
            );
            let glyph = Rect::from_min_size(
                pos2(r.left() + 14.0, r.bottom() - 12.0 - 16.0),
                vec2(28.0, 16.0),
            );
            icons::wave(
                &clip,
                glyph,
                preset.wave,
                if selected {
                    theme::AMBER
                } else {
                    theme::VIOLET
                },
            );
            let (caption, style, color) = if selected {
                ("Selected", inter(12.0, 600), theme::AMBER)
            } else {
                (preset.group.name(), inter(12.0, 400), theme::MUTED)
            };
            theme::text(
                &clip,
                pos2(glyph.right() + 8.0, glyph.center().y),
                Align2::LEFT_CENTER,
                caption,
                style,
                color,
            );
            if tap_at.is_some_and(|pos| r.contains(pos) && rect.contains(pos)) {
                picked = Some(index);
            }
        }
        if shown.is_empty() {
            theme::text(
                p,
                rect.center_top() + vec2(0.0, 40.0),
                Align2::CENTER_CENTER,
                "No sounds match",
                inter(15.0, 500),
                theme::MUTED,
            );
        }
        if let Some(index) = picked {
            if let Some(slot) = self.assign_layer {
                // Choosing a sound for a loop layer: hear it in the loop.
                self.set_layer_sound(slot, index);
                if !self.status.loop_on || !self.status.running {
                    self.send(Cmd::LoopPlay(true));
                }
            } else {
                // Tapping a card selects it and plays a short preview note.
                self.select_sound(index);
                self.preview();
            }
        }
    }

    fn mini_player(&mut self, p: &egui::Painter, rect: Rect) {
        widgets::surface(p, rect, 24.0, theme::SURFACE, theme::LINE);
        let assigning = self.assign_layer.is_some();
        let preset = match self.assigned_sound() {
            Some(index) => self.catalog.items[index].clone(),
            None => self.instrument(),
        };
        let play_c = pos2(rect.left() + 10.0 + 24.0, rect.center().y);
        let play = Rect::from_center_size(play_c, vec2(48.0, 48.0));
        let tap = self.touches.button(Id::unique("mini-play"), play);
        widgets::glow_circle(p, play_c, 24.0, theme::alpha(theme::AMBER, 0.3), 18.0);
        p.circle_filled(play_c, 24.0, widgets::pressed(theme::AMBER, tap.down));
        icons::play(p, play_c + vec2(1.0, 0.0), 24.0, theme::ON_AMBER);
        if tap.pressed {
            if assigning {
                self.send(Cmd::LoopPlay(true));
            } else {
                self.preview();
            }
        }

        let use_w = 72.0;
        let use_rect = Rect::from_min_size(
            pos2(rect.right() - 12.0 - use_w, rect.center().y - 22.0),
            vec2(use_w, 44.0),
        );
        let tap = self.touches.button(Id::unique("mini-use"), use_rect);
        widgets::surface(
            p,
            use_rect,
            22.0,
            widgets::pressed(theme::SURFACE_3, tap.down),
            theme::VIOLET,
        );
        theme::text(
            p,
            use_rect.center(),
            Align2::CENTER_CENTER,
            if assigning { "Done" } else { "Use" },
            inter(15.0, 600),
            theme::TEXT,
        );
        if tap.clicked {
            if assigning {
                self.assign_layer = None;
                self.tab = Tab::Loop;
            } else {
                self.tab = Tab::Play;
            }
        }

        let x = play.right() + 12.0;
        let w = use_rect.left() - 10.0 - x;
        let name = theme::fit(p, &preset.name, grotesk(16.0, 600), w);
        theme::text(
            p,
            pos2(x, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            &name,
            grotesk(16.0, 600),
            theme::TEXT,
        );
        theme::text(
            p,
            pos2(x, rect.center().y + 3.0),
            Align2::LEFT_TOP,
            preset.group.name(),
            inter(12.0, 400),
            theme::MUTED,
        );
    }

    /// The sound of the loop layer being edited, if any.
    fn assigned_sound(&self) -> Option<usize> {
        let layer = self.layer(self.assign_layer?)?;
        self.catalog.index_of(&layer.sound)
    }
}
