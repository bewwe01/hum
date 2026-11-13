//! Capture-looper actions: the UI keeps the master copy of every layer
//! (for saving, tidy timing, undo/redo and changing a layer's sound) and
//! reloads a layer into the engine whenever it changes.

use eframe::egui::{pos2, vec2, Align2, Painter, Rect};
use hum_engine::engine::{Cmd, LAYERS};
use hum_engine::presets::{self, PRESETS};

use crate::app::HumApp;
use crate::state::{LoopLayer, Looper};
use crate::theme::{self, inter};
use crate::widgets;

impl HumApp {
    /// Send layer `slot` (its notes, sound, mute and volume) to the engine,
    /// or clear it there if the slot is empty.
    pub(crate) fn send_layer(&self, slot: usize) {
        let Some(looper) = &self.st.looper else {
            return;
        };
        let Some(Some(layer)) = looper.layers.get(slot) else {
            self.send(Cmd::ClearLayer(slot as u8));
            return;
        };
        let steps = f32::from(looper.steps);
        let preset = presets::find(&layer.sound).unwrap_or(presets::DEFAULT_PRESET) as u8;
        self.send(Cmd::BeginLayer {
            layer: slot as u8,
            preset,
            muted: layer.muted,
            volume: layer.volume,
        });
        for n in &layer.notes {
            let (mut on, mut off) = (n.on, n.off);
            if layer.tidy {
                // Snap the start to the nearest sixteenth, keep the length.
                let length = (n.off - n.on).rem_euclid(steps).max(0.25);
                on = on.round().rem_euclid(steps);
                off = (on + length).rem_euclid(steps);
            }
            self.send(Cmd::LoadNote {
                layer: slot as u8,
                note: n.note,
                velocity: n.velocity,
                on,
                off,
            });
        }
        self.send(Cmd::EndLayer(slot as u8));
    }

    /// Turn what was just played into a layer (the engine replies with
    /// LoopStarted / LayerCaptured / LayerNote or CaptureFailed).
    pub(crate) fn capture(&mut self) {
        self.send(Cmd::Capture);
    }

    pub(crate) fn layer(&self, slot: usize) -> Option<&LoopLayer> {
        self.st.looper.as_ref()?.layers.get(slot)?.as_ref()
    }

    fn layer_mut(&mut self, slot: usize) -> Option<&mut LoopLayer> {
        self.st.looper.as_mut()?.layers.get_mut(slot)?.as_mut()
    }

    /// Remove a layer, keeping it for redo.
    pub(crate) fn delete_layer(&mut self, slot: usize) {
        let bpm = self.st.bpm;
        let Some(looper) = self.st.looper.as_mut() else {
            return;
        };
        let Some(layer) = looper.layers.get_mut(slot).and_then(Option::take) else {
            return;
        };
        looper.order.retain(|&l| usize::from(l) != slot);
        self.redo.push((slot, layer, looper.steps, bpm));
        if looper.count() == 0 {
            self.st.looper = None;
        }
        self.send(Cmd::ClearLayer(slot as u8));
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.st.looper.as_ref().is_some_and(|l| l.count() > 0)
    }

    /// Remove the most recently captured layer.
    pub(crate) fn undo(&mut self) {
        let Some(looper) = &self.st.looper else {
            return;
        };
        let slot = looper
            .order
            .iter()
            .rev()
            .map(|&l| usize::from(l))
            .find(|&l| looper.layers.get(l).is_some_and(Option::is_some))
            .or_else(|| {
                (0..LAYERS)
                    .rev()
                    .find(|&l| looper.layers.get(l).is_some_and(Option::is_some))
            });
        if let Some(slot) = slot {
            self.delete_layer(slot);
        }
    }

    /// Bring back the last removed layer.
    pub(crate) fn redo(&mut self) {
        let Some((slot, layer, steps, bpm)) = self.redo.pop() else {
            return;
        };
        if self.st.looper.is_none() {
            self.st.looper = Some(Looper::new(steps));
            self.st.bpm = bpm;
            self.send(Cmd::SetBpm(bpm));
            self.send(Cmd::SetLoopSteps(steps));
        }
        let Some(looper) = self.st.looper.as_mut() else {
            return;
        };
        // Use the original slot if it is free, otherwise any free slot.
        let slot = if looper.layers[slot].is_none() {
            slot
        } else if let Some(free) = looper.layers.iter().position(Option::is_none) {
            free
        } else {
            self.redo.push((slot, layer, steps, bpm));
            return;
        };
        looper.layers[slot] = Some(layer);
        looper.order.push(slot as u8);
        self.send_layer(slot);
        self.send(Cmd::LoopPlay(true));
    }

    pub(crate) fn toggle_layer(&mut self, slot: usize) {
        if let Some(layer) = self.layer_mut(slot) {
            layer.muted = !layer.muted;
            let muted = layer.muted;
            self.send(Cmd::MuteLayer {
                layer: slot as u8,
                muted,
            });
        }
    }

    pub(crate) fn set_layer_volume(&mut self, slot: usize, volume: f32) {
        if let Some(layer) = self.layer_mut(slot) {
            layer.volume = volume.clamp(0.0, 1.0);
            let volume = layer.volume;
            self.send(Cmd::LayerVolume {
                layer: slot as u8,
                volume,
            });
        }
    }

    pub(crate) fn set_layer_sound(&mut self, slot: usize, index: usize) {
        let index = index.min(PRESETS.len() - 1);
        if let Some(layer) = self.layer_mut(slot) {
            layer.sound = PRESETS[index].name.to_owned();
            self.send(Cmd::LayerPreset {
                layer: slot as u8,
                preset: index as u8,
            });
        }
    }

    pub(crate) fn toggle_tidy(&mut self, slot: usize) {
        if let Some(layer) = self.layer_mut(slot) {
            layer.tidy = !layer.tidy;
            self.send_layer(slot);
        }
    }

    pub(crate) fn draw_toast(&self, p: &Painter, safe: Rect, text: &str) {
        let style = inter(14.0, 500);
        let w =
            (theme::galley(p, text, style, theme::TEXT).size().x + 40.0).min(safe.width() - 32.0);
        let rect = Rect::from_center_size(
            pos2(safe.center().x, safe.bottom() - 16.0 - 64.0 - 14.0 - 24.0),
            vec2(w, 44.0),
        );
        widgets::surface(p, rect, 22.0, theme::SURFACE_3, theme::VIOLET);
        let label = theme::fit(p, text, style, w - 32.0);
        theme::text(
            p,
            rect.center(),
            Align2::CENTER_CENTER,
            &label,
            style,
            theme::TEXT,
        );
    }
}
