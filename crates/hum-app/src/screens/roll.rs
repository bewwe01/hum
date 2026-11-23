//! Piano roll: draw and edit the notes of one loop layer, in any sound.
//!
//! Touch: tap empty space to add a note, tap a note to delete it, drag a
//! note to move it, drag its right edge to resize it, drag empty space to
//! scroll, long-press and drag to select, two fingers to zoom. The Select,
//! Slice and Paint tools change what a one-finger drag does.

use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Painter, Pos2, Rect, Stroke};
use hum_engine::engine::Cmd;
use hum_engine::scale::NOTE_NAMES;

use crate::app::{HumApp, Sheet, Tab};
use crate::icons;
use crate::input::TAP_SLOP;
use crate::roll::{self, Chord, RNote, MIN_LEN, SNAPS};
use crate::state::{LoopLayer, Looper};
use crate::theme::{self, grotesk, inter};
use crate::widgets;

/// "C4", "Eb4"... (the fonts have no flat sign, so a plain "b").
fn pitch_name(pitch: u8) -> String {
    let (name, flat) = NOTE_NAMES[usize::from(pitch % 12)];
    format!(
        "{name}{}{}",
        if flat { "b" } else { "" },
        i32::from(pitch / 12) - 1
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Edit,
    Select,
    Slice,
    Paint,
}

impl Tool {
    const ALL: [Tool; 4] = [Tool::Edit, Tool::Select, Tool::Slice, Tool::Paint];

    fn label(self) -> &'static str {
        match self {
            Tool::Edit => "Edit",
            Tool::Select => "Select",
            Tool::Slice => "Slice",
            Tool::Paint => "Paint",
        }
    }
}

#[derive(Clone, Debug)]
enum Gesture {
    None,
    /// A finger is down; what it does is decided once it moves or lifts.
    Pending {
        id: u64,
        at: Pos2,
        note: Option<usize>,
        edge: bool,
    },
    Pan {
        id: u64,
    },
    Move {
        id: u64,
        at: Pos2,
        anchor: usize,
        orig: Vec<(usize, f32, usize)>,
        row: i32,
    },
    Resize {
        id: u64,
        at: Pos2,
        idx: usize,
        end: f32,
    },
    Boxing {
        id: u64,
        at: Pos2,
        to: Pos2,
    },
    Paint {
        id: u64,
        pitch: u8,
        last: f32,
    },
    Slice {
        id: u64,
        at: Pos2,
        to: Pos2,
    },
    Pinch {
        a: u64,
        b: u64,
        last: [Pos2; 2],
    },
}

pub struct RollUi {
    pub slot: usize,
    notes: Vec<RNote>,
    /// Pitches shown, top row first.
    rows: Vec<u8>,
    pub all_rows: bool,
    drums: bool,
    pub snap: usize,
    pub tool: Tool,
    pub chord: Chord,
    pub full_strength: bool,
    last_len: f32,
    last_vel: u8,
    // View: first visible step, points per step, first visible row, row height.
    x0: f32,
    pps: f32,
    r0: f32,
    row_h: f32,
    fitted: bool,
    gesture: Gesture,
    touch_at: f64,
    vel_drag: Option<u64>,
    undo: Vec<Looper>,
    redo: Vec<Looper>,
}

impl RollUi {
    fn grid(&self) -> f32 {
        SNAPS[self.snap].1
    }

    fn row_of(&self, pitch: u8) -> Option<usize> {
        self.rows.iter().position(|&p| p == pitch)
    }

    fn selected(&self) -> usize {
        self.notes.iter().filter(|n| n.sel).count()
    }
}

/// Layout of the roll screen.
struct Areas {
    header: Rect,
    tools: Rect,
    ruler: Rect,
    keys: Rect,
    grid: Rect,
    vel: Rect,
}

impl HumApp {
    // ------------------------------------------------------------------
    // Opening, rows, committing

    /// Open the piano roll on loop layer `slot`.
    pub(crate) fn open_roll(&mut self, slot: usize) {
        let Some(looper) = &self.st.looper else {
            return;
        };
        let steps = f32::from(looper.steps);
        let Some(Some(layer)) = looper.layers.get(slot) else {
            return;
        };
        let mut layer = layer.clone();
        if layer.tidy {
            // The roll shows what plays: bake "tidy timing" into the notes.
            for n in &mut layer.notes {
                let len = (n.off - n.on).rem_euclid(steps).max(0.25);
                n.on = n.on.round().rem_euclid(steps);
                n.off = (n.on + len).rem_euclid(steps);
            }
            layer.tidy = false;
        }
        let notes = roll::from_layer(&layer.notes, steps);
        let drums = layer.sound.starts_with("gm:128:");
        let scale = roll::scale_pitches(&self.st.keymap());
        let all_rows = notes.iter().any(|n| !scale.contains(&n.pitch));
        if let Some(Some(l)) = self
            .st
            .looper
            .as_mut()
            .and_then(|lp| lp.layers.get_mut(slot))
        {
            *l = layer;
        }
        self.send_layer(slot);
        let mut r = RollUi {
            slot,
            notes,
            rows: Vec::new(),
            all_rows,
            drums,
            snap: 0,
            tool: Tool::Edit,
            chord: Chord::Off,
            full_strength: true,
            last_len: 1.0,
            last_vel: 100,
            x0: 0.0,
            pps: 20.0,
            r0: 0.0,
            row_h: 22.0,
            fitted: false,
            gesture: Gesture::None,
            touch_at: 0.0,
            vel_drag: None,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        self.roll_rows(&mut r);
        self.roll = Some(r);
        self.release_all_keys();
    }

    /// Start a new, empty layer (making a loop of `bars` if there is none)
    /// and open it in the piano roll.
    pub(crate) fn new_roll_layer(&mut self, bars: u16) {
        if self.st.looper.is_none() {
            let steps = bars.clamp(1, 8) * 16;
            self.st.looper = Some(Looper::new(steps));
            self.send(Cmd::SetLoopSteps(steps));
            self.send(Cmd::SetBpm(self.st.bpm));
        }
        let sound = self.st.sound.clone();
        let reverb = self.st.reverb;
        let Some(looper) = self.st.looper.as_mut() else {
            return;
        };
        let Some(slot) = looper.layers.iter().position(Option::is_none) else {
            self.show_toast("All 8 layers are full: delete one first");
            return;
        };
        looper.layers[slot] = Some(LoopLayer {
            sound,
            reverb,
            ..LoopLayer::default()
        });
        looper.order.push(slot as u8);
        self.redo.clear();
        self.open_roll(slot);
    }

    /// Close the roll; an empty new layer is removed again.
    pub(crate) fn close_roll(&mut self) {
        if let Some(r) = self.roll.take() {
            if self.layer(r.slot).is_some_and(|l| l.notes.is_empty()) {
                if let Some(looper) = self.st.looper.as_mut() {
                    looper.layers[r.slot] = None;
                    looper.order.retain(|&l| usize::from(l) != r.slot);
                    if looper.count() == 0 {
                        self.st.looper = None;
                        self.send(Cmd::ClearAll);
                    }
                }
                self.send(Cmd::ClearLayer(r.slot as u8));
            }
        }
    }

    fn roll_rows(&self, r: &mut RollUi) {
        let scale = roll::scale_pitches(&self.st.keymap());
        r.rows = if r.drums {
            (35..=81).rev().collect()
        } else if r.all_rows {
            (roll::LOW..=roll::HIGH).rev().collect()
        } else {
            scale.into_iter().rev().collect()
        };
    }

    fn layer_sound_id(&self, slot: usize) -> hum_engine::engine::SoundId {
        let key = self
            .layer(slot)
            .map(|l| l.sound.clone())
            .unwrap_or_default();
        self.catalog.get(&key).sound
    }

    fn roll_preview(&self, r: &RollUi, pitch: u8, vel: u8) {
        self.send(Cmd::PreviewNote {
            sound: self.layer_sound_id(r.slot),
            note: pitch,
            velocity: vel,
        });
    }

    fn loop_steps(&self) -> f32 {
        self.st.looper.as_ref().map_or(64.0, |l| f32::from(l.steps))
    }

    /// Remember the loop for undo (call before changing it).
    fn roll_snapshot(&self, r: &mut RollUi) {
        if let Some(looper) = &self.st.looper {
            r.undo.push(looper.clone());
            if r.undo.len() > 60 {
                r.undo.remove(0);
            }
            r.redo.clear();
        }
    }

    /// Write the roll's notes into the layer and reload it in the engine.
    fn roll_commit(&mut self, r: &RollUi) {
        let steps = self.loop_steps();
        let notes = roll::to_layer(
            &r.notes
                .iter()
                .copied()
                .filter(|n| n.start < steps)
                .collect::<Vec<_>>(),
            steps,
        );
        if let Some(Some(l)) = self
            .st
            .looper
            .as_mut()
            .and_then(|lp| lp.layers.get_mut(r.slot))
        {
            l.notes = notes;
        }
        self.send_layer(r.slot);
    }

    /// Restore a loop from the undo/redo stacks.
    fn roll_restore(&mut self, r: &mut RollUi, looper: Looper) {
        let steps_changed = self.st.looper.as_ref().map(|l| l.steps) != Some(looper.steps);
        let steps = looper.steps;
        self.st.looper = Some(looper);
        if steps_changed {
            self.send(Cmd::SetLoopSteps(steps));
            for slot in 0..hum_engine::engine::LAYERS {
                self.send_layer(slot);
            }
        } else {
            self.send_layer(r.slot);
        }
        let notes = self
            .layer(r.slot)
            .map(|l| l.notes.clone())
            .unwrap_or_default();
        r.notes = roll::from_layer(&notes, f32::from(steps));
    }

    /// Change the loop's length, repeating or cutting every layer.
    fn roll_set_length(&mut self, r: &mut RollUi, steps: u16, keep_roll_notes: bool) {
        let Some(looper) = self.st.looper.as_mut() else {
            return;
        };
        let from = looper.steps;
        if from == steps {
            return;
        }
        for (slot, layer) in looper.layers.iter_mut().enumerate() {
            if let Some(l) = layer {
                if slot != r.slot || !keep_roll_notes {
                    l.notes = roll::resize_loop(&l.notes, from, steps);
                }
            }
        }
        looper.steps = steps;
        self.send(Cmd::SetLoopSteps(steps));
        if keep_roll_notes {
            self.roll_commit(r);
        } else {
            let notes = self
                .layer(r.slot)
                .map(|l| l.notes.clone())
                .unwrap_or_default();
            r.notes = roll::from_layer(&notes, f32::from(steps));
        }
        for slot in 0..hum_engine::engine::LAYERS {
            self.send_layer(slot);
        }
    }

    // ------------------------------------------------------------------
    // Screen

    pub(crate) fn roll_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(mut r) = self.roll.take() else {
            return;
        };
        if self.layer(r.slot).is_none() {
            return;
        }
        // The layer's sound may have changed on the Sounds screen.
        let drums = self
            .layer(r.slot)
            .is_some_and(|l| l.sound.starts_with("gm:128:"));
        if drums != r.drums {
            r.drums = drums;
            self.roll_rows(&mut r);
            r.fitted = false;
        }
        let p = ui.painter().clone();
        let a = self.roll_areas(rect, r.drums);
        let steps = self.loop_steps();
        self.roll_fit(&mut r, &a, steps);
        if self.roll_header(&p, &a, &mut r) {
            // Closed (or went to choose a sound).
            if self.assign_layer.is_some() {
                self.roll = Some(r);
            }
            return;
        }
        self.roll_tool_row(&p, &a, &mut r);
        self.roll_input(ui, &a, &mut r, steps);
        self.roll_velocity(&p, &a, &mut r);
        self.roll_draw(&p, &a, &r, steps);
        self.roll = Some(r);
        if self.status.running {
            self.repaint.request_repaint();
        }
    }

    fn roll_areas(&self, rect: Rect, drums: bool) -> Areas {
        let wide = self.landscape;
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 44.0));
        let tools = if wide {
            Rect::from_min_size(rect.min, vec2(0.0, 44.0))
        } else {
            Rect::from_min_size(
                pos2(rect.left(), header.bottom() + 8.0),
                vec2(rect.width(), 40.0),
            )
        };
        let top = if wide {
            header.bottom() + 8.0
        } else {
            tools.bottom() + 8.0
        };
        let vel_h = if wide { 48.0 } else { 64.0 };
        let keys_w = if drums { 104.0 } else { 52.0 };
        let ruler = Rect::from_min_max(
            pos2(rect.left() + keys_w, top),
            pos2(rect.right(), top + 18.0),
        );
        let vel = Rect::from_min_max(pos2(rect.left() + keys_w, rect.bottom() - vel_h), rect.max);
        let keys = Rect::from_min_max(
            pos2(rect.left(), ruler.bottom()),
            pos2(rect.left() + keys_w, vel.top() - 6.0),
        );
        let grid = Rect::from_min_max(
            pos2(keys.right(), ruler.bottom()),
            pos2(rect.right(), vel.top() - 6.0),
        );
        Areas {
            header,
            tools,
            ruler,
            keys,
            grid,
            vel,
        }
    }

    /// First view: up to two bars wide, centred on the notes.
    fn roll_fit(&self, r: &mut RollUi, a: &Areas, steps: f32) {
        if !r.fitted {
            r.fitted = true;
            r.pps = a.grid.width() / steps.min(32.0);
            r.row_h = if self.landscape { 22.0 } else { 26.0 };
            r.x0 = 0.0;
            let centre = if r.notes.is_empty() {
                if r.drums {
                    42
                } else {
                    60 + self.st.key
                }
            } else {
                let sum: u32 = r.notes.iter().map(|n| u32::from(n.pitch)).sum();
                (sum / r.notes.len() as u32) as u8
            };
            let row = r
                .rows
                .iter()
                .position(|&p| p <= centre)
                .unwrap_or(r.rows.len() / 2) as f32;
            r.r0 = row - a.grid.height() / r.row_h / 2.0;
        }
        self.roll_clamp(r, a, steps);
    }

    fn roll_clamp(&self, r: &mut RollUi, a: &Areas, steps: f32) {
        let min_pps = a.grid.width() / steps;
        r.pps = r.pps.clamp(min_pps, 140.0);
        r.row_h = r.row_h.clamp(12.0, 52.0);
        let vis_steps = a.grid.width() / r.pps;
        r.x0 = r.x0.clamp(0.0, (steps - vis_steps).max(0.0));
        let vis_rows = a.grid.height() / r.row_h;
        r.r0 = r.r0.clamp(0.0, (r.rows.len() as f32 - vis_rows).max(0.0));
    }

    /// Returns true when the roll closed (or handed over to Sounds).
    fn roll_header(&mut self, p: &Painter, a: &Areas, r: &mut RollUi) -> bool {
        let h = a.header;
        let back = Rect::from_min_size(h.min, vec2(44.0, 44.0));
        if self.roll_icon_button(
            p,
            back,
            "roll-back",
            |p, c, col| icons::chevron_left(p, c, 24.0, col),
            true,
        ) {
            self.close_roll();
            return true;
        }
        // Right side: play, redo, undo (and in landscape the tools).
        let mut right = h.right();
        let play = Rect::from_min_size(pos2(right - 44.0, h.top()), vec2(44.0, 44.0));
        right = play.left() - 6.0;
        let playing = self.status.loop_on && self.status.running;
        let tap = self.touches.button(Id::unique("roll-play"), play);
        widgets::circle(
            p,
            play.center(),
            22.0,
            widgets::pressed(if playing { theme::VIOLET } else { theme::AMBER }, tap.down),
            if playing {
                theme::VIOLET_LIGHT
            } else {
                theme::AMBER_LIGHT
            },
        );
        if playing {
            icons::pause(p, play.center(), 20.0, theme::ON_VIOLET);
        } else {
            icons::play(p, play.center() + vec2(1.0, 0.0), 20.0, theme::ON_AMBER);
        }
        if tap.clicked {
            self.send(Cmd::LoopPlay(!playing));
        }
        let redo = Rect::from_min_size(pos2(right - 44.0, h.top()), vec2(44.0, 44.0));
        let undo = Rect::from_min_size(pos2(redo.left() - 50.0, h.top()), vec2(44.0, 44.0));
        right = undo.left() - 6.0;
        let can_redo = !r.redo.is_empty();
        let can_undo = !r.undo.is_empty();
        if self.roll_icon_button(
            p,
            redo,
            "roll-redo",
            |p, c, col| icons::redo(p, c, 22.0, col),
            can_redo,
        ) {
            if let Some(next) = r.redo.pop() {
                if let Some(cur) = self.st.looper.clone() {
                    r.undo.push(cur);
                }
                self.roll_restore(r, next);
            }
        }
        if self.roll_icon_button(
            p,
            undo,
            "roll-undo",
            |p, c, col| icons::undo(p, c, 22.0, col),
            can_undo,
        ) {
            if let Some(prev) = r.undo.pop() {
                if let Some(cur) = self.st.looper.clone() {
                    r.redo.push(cur);
                }
                self.roll_restore(r, prev);
            }
        }
        if self.landscape {
            let tools = Rect::from_min_max(
                pos2((right - 420.0).max(back.right() + 150.0), h.top()),
                pos2(right, h.bottom()),
            );
            right = tools.left() - 8.0;
            self.roll_tools(p, tools, r);
        }
        // Title: layer and sound; tapping picks another sound.
        let title = Rect::from_min_max(pos2(back.right() + 8.0, h.top()), pos2(right, h.bottom()));
        let tap = self.touches.button(Id::unique("roll-sound"), title);
        widgets::surface(
            p,
            title,
            18.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let color = crate::screens::layer_color(r.slot);
        p.circle_filled(pos2(title.left() + 16.0, title.center().y), 5.0, color);
        let sound = self
            .layer(r.slot)
            .map(|l| self.catalog.get(&l.sound).name)
            .unwrap_or_default();
        let x = title.left() + 30.0;
        let w = title.right() - 12.0 - x;
        theme::text(
            p,
            pos2(x, title.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            &format!("Layer {}", r.slot + 1),
            inter(13.0, 600),
            theme::TEXT,
        );
        let sound = theme::fit(p, &format!("{sound} ›"), inter(12.0, 400), w);
        theme::text(
            p,
            pos2(x, title.center().y + 1.0),
            Align2::LEFT_TOP,
            &sound,
            inter(12.0, 400),
            theme::MUTED,
        );
        if tap.clicked {
            self.assign_layer = Some(r.slot);
            self.tab = Tab::Sounds;
            return true;
        }
        false
    }

    fn roll_icon_button(
        &mut self,
        p: &Painter,
        rect: Rect,
        id: &'static str,
        icon: impl FnOnce(&Painter, Pos2, Color32),
        enabled: bool,
    ) -> bool {
        let tap = self.touches.button(Id::unique(id), rect);
        widgets::circle(
            p,
            rect.center(),
            22.0,
            widgets::pressed(theme::SURFACE, tap.down && enabled),
            theme::LINE,
        );
        icon(
            p,
            rect.center(),
            if enabled {
                theme::TEXT
            } else {
                theme::alpha(theme::MUTED, 0.45)
            },
        );
        tap.clicked && enabled
    }

    fn roll_tool_row(&mut self, p: &Painter, a: &Areas, r: &mut RollUi) {
        if !self.landscape {
            self.roll_tools(p, a.tools, r);
        }
    }

    /// Tool switch, snap and the Tools sheet button.
    fn roll_tools(&mut self, p: &Painter, rect: Rect, r: &mut RollUi) {
        let more = Rect::from_min_size(
            pos2(rect.right() - 70.0, rect.top()),
            vec2(70.0, rect.height()),
        );
        let snap = Rect::from_min_size(
            pos2(more.left() - 70.0, rect.top()),
            vec2(64.0, rect.height()),
        );
        let seg = Rect::from_min_max(rect.min, pos2(snap.left() - 6.0, rect.bottom()));
        p.rect_filled(seg, seg.height() / 2.0, theme::SURFACE_2);
        let w = (seg.width() - 6.0) / 4.0;
        for (i, tool) in Tool::ALL.iter().enumerate() {
            let c = Rect::from_min_size(
                pos2(seg.left() + 3.0 + i as f32 * w, seg.top() + 3.0),
                vec2(w, seg.height() - 6.0),
            );
            let tap = self.touches.button(Id::unique(("roll-tool", i)), c);
            let on = r.tool == *tool;
            if on {
                p.rect_filled(c, c.height() / 2.0, theme::VIOLET);
            }
            let style = inter(13.0, if on { 600 } else { 500 });
            let label = theme::fit(p, tool.label(), style, c.width() - 4.0);
            theme::text(
                p,
                c.center(),
                Align2::CENTER_CENTER,
                &label,
                style,
                if on { theme::ON_VIOLET } else { theme::TEXT },
            );
            if tap.clicked {
                r.tool = *tool;
            }
        }
        let tap = self.touches.button(Id::unique("roll-snap"), snap);
        widgets::surface(
            p,
            snap,
            snap.height() / 2.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let c = snap.center();
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_BOTTOM,
            "SNAP",
            inter(9.0, 600).tracked(0.06),
            theme::MUTED,
        );
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_TOP,
            SNAPS[r.snap].0,
            grotesk(13.0, 600),
            theme::TEXT,
        );
        if tap.clicked {
            r.snap = (r.snap + 1) % SNAPS.len();
        }
        let tap = self.touches.button(Id::unique("roll-more"), more);
        let lit = r.chord != Chord::Off || r.selected() > 0;
        widgets::surface(
            p,
            more,
            more.height() / 2.0,
            widgets::pressed(
                if lit {
                    theme::SURFACE_3
                } else {
                    theme::SURFACE
                },
                tap.down,
            ),
            if lit { theme::VIOLET } else { theme::LINE },
        );
        theme::text(
            p,
            more.center(),
            Align2::CENTER_CENTER,
            "Tools",
            inter(13.0, 600),
            theme::TEXT,
        );
        if tap.clicked {
            self.sheet = Some(Sheet::RollTools);
        }
    }

    // ------------------------------------------------------------------
    // Coordinates

    fn x_of(r: &RollUi, a: &Areas, t: f32) -> f32 {
        a.grid.left() + (t - r.x0) * r.pps
    }

    fn t_of(r: &RollUi, a: &Areas, x: f32) -> f32 {
        r.x0 + (x - a.grid.left()) / r.pps
    }

    fn y_of(r: &RollUi, a: &Areas, row: usize) -> f32 {
        a.grid.top() + (row as f32 - r.r0) * r.row_h
    }

    fn row_at(r: &RollUi, a: &Areas, y: f32) -> Option<usize> {
        let row = ((y - a.grid.top()) / r.row_h + r.r0).floor();
        (row >= 0.0 && (row as usize) < r.rows.len()).then_some(row as usize)
    }

    fn note_rect(r: &RollUi, a: &Areas, n: &RNote) -> Option<Rect> {
        let row = r.row_of(n.pitch)?;
        let y = Self::y_of(r, a, row);
        Some(Rect::from_min_max(
            pos2(Self::x_of(r, a, n.start), y + 1.0),
            pos2(Self::x_of(r, a, n.end()), y + r.row_h - 1.0),
        ))
    }

    // ------------------------------------------------------------------
    // Input

    fn roll_input(&mut self, ui: &egui::Ui, a: &Areas, r: &mut RollUi, steps: f32) {
        // Keys: tap to hear a row.
        let keys_id = Id::unique("roll-keys");
        self.touches.claim(keys_id, a.keys);
        let taps: Vec<Pos2> = self
            .touches
            .owned(keys_id)
            .filter(|t| t.started)
            .map(|t| t.pos)
            .collect();
        for pos in taps {
            if let Some(row) = Self::row_at(r, a, pos.y) {
                self.roll_preview(r, r.rows[row], 100);
            }
        }
        // Ruler: drag to scroll sideways.
        let ruler_id = Id::unique("roll-ruler");
        self.touches.claim(ruler_id, a.ruler);
        let dx: f32 = self
            .touches
            .owned(ruler_id)
            .filter(|t| !t.ended)
            .map(|t| t.delta().x)
            .sum();
        r.x0 -= dx / r.pps;

        // Mouse wheel / trackpad (desktop).
        if ui.rect_contains_pointer(a.grid) {
            let (scroll, zoom, shift) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
            if zoom != 1.0 {
                r.pps *= zoom;
            } else if shift {
                r.x0 -= (scroll.y + scroll.x) / r.pps;
            } else {
                r.x0 -= scroll.x / r.pps;
                r.r0 -= scroll.y / r.row_h;
            }
        }

        let id = Id::unique("roll-grid");
        self.touches.claim(id, a.grid);
        let touches: Vec<(u64, Pos2, Pos2, bool, bool, f32)> = self
            .touches
            .owned(id)
            .map(|t| (t.id, t.pos, t.start, t.started, t.ended, t.travel))
            .collect();
        let live: Vec<&(u64, Pos2, Pos2, bool, bool, f32)> =
            touches.iter().filter(|t| !t.4).collect();

        // Two fingers: zoom and scroll.
        if live.len() >= 2 {
            let (ta, tb) = (live[0], live[1]);
            match &r.gesture {
                Gesture::Pinch { a: ia, b: ib, last } if *ia == ta.0 && *ib == tb.0 => {
                    let (l0, l1) = (last[0], last[1]);
                    let (d0, d1) = (l1 - l0, tb.1 - ta.1);
                    let mid0 = pos2((l0.x + l1.x) / 2.0, (l0.y + l1.y) / 2.0);
                    let mid1 = pos2((ta.1.x + tb.1.x) / 2.0, (ta.1.y + tb.1.y) / 2.0);
                    let anchor_t = Self::t_of(r, a, mid0.x);
                    let anchor_r = (mid0.y - a.grid.top()) / r.row_h + r.r0;
                    if d0.x.abs() > 30.0 && d1.x.abs() > 30.0 {
                        r.pps *= d1.x.abs() / d0.x.abs();
                    }
                    if d0.y.abs() > 30.0 && d1.y.abs() > 30.0 {
                        r.row_h *= d1.y.abs() / d0.y.abs();
                    }
                    self.roll_clamp(r, a, steps);
                    r.x0 = anchor_t - (mid1.x - a.grid.left()) / r.pps;
                    r.r0 = anchor_r - (mid1.y - a.grid.top()) / r.row_h;
                }
                _ => {
                    // A second finger cancels a one-finger edit in progress.
                    self.roll_cancel(r, steps);
                }
            }
            r.gesture = Gesture::Pinch {
                a: ta.0,
                b: tb.0,
                last: [ta.1, tb.1],
            };
            self.roll_clamp(r, a, steps);
            return;
        }
        if matches!(r.gesture, Gesture::Pinch { .. }) && live.is_empty() {
            r.gesture = Gesture::None;
        }

        for &(tid, pos, start, started, ended, travel) in &touches {
            if started && matches!(r.gesture, Gesture::None) {
                let t = Self::t_of(r, a, pos.x);
                let note =
                    Self::row_at(r, a, pos.y).and_then(|row| roll::hit(&r.notes, t, r.rows[row]));
                let edge = note.is_some_and(|i| {
                    Self::note_rect(r, a, &r.notes[i])
                        .is_some_and(|nr| pos.x > nr.right() - (nr.width() * 0.3).clamp(8.0, 18.0))
                });
                if let Some(i) = note {
                    self.roll_preview(r, r.notes[i].pitch, r.notes[i].vel);
                }
                r.gesture = Gesture::Pending {
                    id: tid,
                    at: start,
                    note,
                    edge,
                };
                r.touch_at = self.now;
            }
            self.roll_step(r, a, steps, tid, pos, ended, travel);
        }
        self.roll_clamp(r, a, steps);
    }

    /// Undo a drag that a second finger interrupted.
    fn roll_cancel(&mut self, r: &mut RollUi, _steps: f32) {
        match std::mem::replace(&mut r.gesture, Gesture::None) {
            Gesture::Move { .. } | Gesture::Resize { .. } | Gesture::Paint { .. } => {
                if let Some(prev) = r.undo.pop() {
                    self.roll_restore(r, prev);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn roll_step(
        &mut self,
        r: &mut RollUi,
        a: &Areas,
        steps: f32,
        tid: u64,
        pos: Pos2,
        ended: bool,
        travel: f32,
    ) {
        let grid = r.grid();
        match r.gesture.clone() {
            Gesture::Pending { id, at, note, edge } if id == tid => {
                let held = self.now - r.touch_at > 0.45;
                if ended {
                    r.gesture = Gesture::None;
                    if travel < TAP_SLOP {
                        self.roll_tap(r, a, steps, at, note);
                    }
                } else if travel >= TAP_SLOP {
                    r.gesture = match (note, r.tool) {
                        (Some(i), Tool::Edit | Tool::Select | Tool::Paint) => {
                            self.roll_snapshot(r);
                            if edge && !r.notes[i].sel {
                                Gesture::Resize {
                                    id,
                                    at,
                                    idx: i,
                                    end: r.notes[i].end(),
                                }
                            } else {
                                let moving: Vec<usize> = if r.notes[i].sel {
                                    (0..r.notes.len()).filter(|&k| r.notes[k].sel).collect()
                                } else {
                                    vec![i]
                                };
                                let orig = moving
                                    .iter()
                                    .map(|&k| {
                                        (
                                            k,
                                            r.notes[k].start,
                                            r.row_of(r.notes[k].pitch).unwrap_or(0),
                                        )
                                    })
                                    .collect();
                                Gesture::Move {
                                    id,
                                    at,
                                    anchor: i,
                                    orig,
                                    row: 0,
                                }
                            }
                        }
                        (_, Tool::Slice) => Gesture::Slice { id, at, to: pos },
                        (None, Tool::Select) => Gesture::Boxing { id, at, to: pos },
                        (None, Tool::Paint) => match Self::row_at(r, a, at.y) {
                            Some(row) => {
                                self.roll_snapshot(r);
                                Gesture::Paint {
                                    id,
                                    pitch: r.rows[row],
                                    last: -1.0,
                                }
                            }
                            None => Gesture::None,
                        },
                        _ => Gesture::Pan { id },
                    };
                    self.roll_step(r, a, steps, tid, pos, ended, travel);
                } else if held && note.is_none() && r.tool == Tool::Edit {
                    // Long press: select with a box.
                    r.gesture = Gesture::Boxing { id, at, to: pos };
                    self.haptic();
                }
            }
            Gesture::Pan { id } if id == tid => {
                if ended {
                    r.gesture = Gesture::None;
                } else if let Some(t) = self.touches_pos_delta(tid) {
                    r.x0 -= t.x / r.pps;
                    r.r0 -= t.y / r.row_h;
                }
            }
            Gesture::Move {
                id,
                at,
                anchor,
                orig,
                row,
            } if id == tid => {
                let a_orig = orig.iter().find(|o| o.0 == anchor).map_or(0.0, |o| o.1);
                let raw = (pos.x - at.x) / r.pps;
                let mut dt = roll::snap(a_orig + raw, grid) - a_orig;
                let min_start = orig.iter().map(|o| o.1).fold(f32::MAX, f32::min);
                let max_end = orig
                    .iter()
                    .map(|o| r.notes.get(o.0).map_or(0.0, |n| o.1 + n.len))
                    .fold(0.0f32, f32::max);
                dt = dt.clamp(-min_start, (steps - max_end).max(-min_start));
                let drow = ((pos.y - at.y) / r.row_h).round() as i32;
                let last = r.rows.len() as i32 - 1;
                let drow = orig
                    .iter()
                    .fold(drow, |d, o| d.clamp(-(o.2 as i32), last - o.2 as i32));
                for &(k, start, rw) in &orig {
                    if let Some(n) = r.notes.get_mut(k) {
                        n.start = start + dt;
                        n.pitch = r.rows[(rw as i32 + drow) as usize];
                    }
                }
                if drow != row {
                    if let Some(n) = r.notes.get(anchor) {
                        self.roll_preview(r, n.pitch, n.vel);
                    }
                    r.gesture = Gesture::Move {
                        id,
                        at,
                        anchor,
                        orig: orig.clone(),
                        row: drow,
                    };
                }
                if ended {
                    r.gesture = Gesture::None;
                    roll::sort(&mut r.notes);
                    self.roll_commit(r);
                }
            }
            Gesture::Resize { id, at, idx, end } if id == tid => {
                if let Some(n) = r.notes.get_mut(idx) {
                    let min = if grid > 0.0 { grid } else { MIN_LEN };
                    let new_end = roll::snap(end + (pos.x - at.x) / r.pps, grid);
                    n.len = (new_end - n.start).clamp(min, steps - n.start);
                    if ended {
                        r.last_len = n.len;
                    }
                }
                if ended {
                    r.gesture = Gesture::None;
                    self.roll_commit(r);
                }
            }
            Gesture::Boxing { id, at, .. } if id == tid => {
                if ended {
                    r.gesture = Gesture::None;
                    let sel = Rect::from_two_pos(at, pos);
                    for k in 0..r.notes.len() {
                        let hit =
                            Self::note_rect(r, a, &r.notes[k]).is_some_and(|nr| nr.intersects(sel));
                        r.notes[k].sel = hit;
                    }
                } else {
                    r.gesture = Gesture::Boxing { id, at, to: pos };
                }
            }
            Gesture::Paint { id, pitch, last } if id == tid => {
                let len = if grid > 0.0 { grid } else { r.last_len };
                let t = roll::snap_floor(Self::t_of(r, a, pos.x), len).clamp(0.0, steps - len);
                if (t - last).abs() > 1.0e-3 && roll::hit(&r.notes, t + 0.01, pitch).is_none() {
                    roll::add(&mut r.notes, RNote::new(pitch, t, len, r.last_vel));
                    self.roll_preview(r, pitch, r.last_vel);
                    r.gesture = Gesture::Paint { id, pitch, last: t };
                }
                if ended {
                    r.gesture = Gesture::None;
                    self.roll_commit(r);
                }
            }
            Gesture::Slice { id, at, .. } if id == tid => {
                if ended {
                    r.gesture = Gesture::None;
                    let t = roll::snap(Self::t_of(r, a, at.x), grid);
                    let (r1, r2) = (Self::row_at(r, a, at.y), Self::row_at(r, a, pos.y));
                    if let (Some(r1), Some(r2)) = (r1.or(r2), r2.or(r1)) {
                        let pitches: Vec<u8> = r.rows[r1.min(r2)..=r1.max(r2)].to_vec();
                        let mut copy = r.notes.clone();
                        if roll::slice(&mut copy, t, &pitches) > 0 {
                            self.roll_snapshot(r);
                            r.notes = copy;
                            self.roll_commit(r);
                        }
                    }
                } else {
                    r.gesture = Gesture::Slice { id, at, to: pos };
                }
            }
            _ => {}
        }
    }

    fn touches_pos_delta(&self, tid: u64) -> Option<egui::Vec2> {
        self.touches
            .owned(Id::unique("roll-grid"))
            .find(|t| t.id == tid)
            .map(|t| t.delta())
    }

    /// A tap on the grid at `at` (on `note`, if any).
    fn roll_tap(&mut self, r: &mut RollUi, a: &Areas, steps: f32, at: Pos2, note: Option<usize>) {
        let grid = r.grid();
        match (note, r.tool) {
            (Some(i), Tool::Edit | Tool::Paint) => {
                self.roll_snapshot(r);
                r.notes.remove(i);
                self.roll_commit(r);
            }
            (Some(i), Tool::Select) => r.notes[i].sel = !r.notes[i].sel,
            (Some(i), Tool::Slice) => {
                let t = roll::snap(Self::t_of(r, a, at.x), grid);
                let mut copy = r.notes.clone();
                if roll::slice(&mut copy, t, &[r.notes[i].pitch]) > 0 {
                    self.roll_snapshot(r);
                    r.notes = copy;
                    self.roll_commit(r);
                }
            }
            (None, Tool::Edit | Tool::Paint) => {
                let Some(row) = Self::row_at(r, a, at.y) else {
                    return;
                };
                if r.selected() > 0 {
                    // The first tap off the notes clears the selection.
                    for n in &mut r.notes {
                        n.sel = false;
                    }
                    return;
                }
                let len = r.last_len.min(steps);
                let t = roll::snap_floor(Self::t_of(r, a, at.x), grid).clamp(0.0, steps - MIN_LEN);
                let scale = roll::scale_pitches(&self.st.keymap());
                let pitches = r.chord.pitches(r.rows[row], &scale);
                self.roll_snapshot(r);
                for &pitch in &pitches {
                    roll::add(&mut r.notes, RNote::new(pitch, t, len, r.last_vel));
                }
                if pitches.iter().any(|p| r.row_of(*p).is_none()) && !r.drums {
                    // A chord tone outside the scale: show every note.
                    r.all_rows = true;
                    self.roll_rows(r);
                    self.show_toast("Showing all notes: the chord leaves the scale");
                }
                self.roll_preview(r, pitches[0], r.last_vel);
                self.roll_commit(r);
            }
            (None, Tool::Select) => {
                for n in &mut r.notes {
                    n.sel = false;
                }
            }
            (None, Tool::Slice) => {}
        }
    }

    /// The velocity lane: drag a bar to change how loud its note plays.
    fn roll_velocity(&mut self, p: &Painter, a: &Areas, r: &mut RollUi) {
        let id = Id::unique("roll-vel");
        self.touches.claim(id, a.vel);
        let touches: Vec<(u64, Pos2, bool, bool)> = self
            .touches
            .owned(id)
            .map(|t| (t.id, t.pos, t.started, t.ended))
            .collect();
        for (tid, pos, started, ended) in touches {
            if started {
                self.roll_snapshot(r);
                r.vel_drag = Some(tid);
            }
            if r.vel_drag != Some(tid) {
                continue;
            }
            let v = (1.0 - (pos.y - a.vel.top() - 6.0) / (a.vel.height() - 12.0)).clamp(0.0, 1.0);
            let vel = (1.0 + v * 126.0).round() as u8;
            let any_sel = r.selected() > 0;
            for k in 0..r.notes.len() {
                let x = Self::x_of(r, a, r.notes[k].start);
                if (x - pos.x).abs() < 12.0 && (!any_sel || r.notes[k].sel) {
                    r.notes[k].vel = vel;
                    r.last_vel = vel;
                }
            }
            if ended {
                r.vel_drag = None;
                self.roll_commit(r);
            }
        }
        let _ = p;
    }

    // ------------------------------------------------------------------
    // Drawing

    fn roll_draw(&self, p: &Painter, a: &Areas, r: &RollUi, steps: f32) {
        let grid_p = p.with_clip_rect(a.grid);
        widgets::surface(p, a.grid.expand(1.0), 4.0, theme::BG, theme::LINE);
        let scale = roll::scale_pitches(&self.st.keymap());
        let root = self.st.key;
        // Rows.
        let first = r.r0.floor().max(0.0) as usize;
        let last = ((r.r0 + a.grid.height() / r.row_h).ceil() as usize + 1).min(r.rows.len());
        for row in first..last {
            let pitch = r.rows[row];
            let y = Self::y_of(r, a, row);
            let band =
                Rect::from_min_max(pos2(a.grid.left(), y), pos2(a.grid.right(), y + r.row_h));
            let fill = if r.drums {
                if row % 2 == 0 {
                    theme::SURFACE
                } else {
                    theme::BG
                }
            } else if pitch % 12 == root {
                theme::alpha(theme::AMBER, 0.08)
            } else if scale.contains(&pitch) {
                theme::SURFACE
            } else {
                theme::BG
            };
            grid_p.rect_filled(band, 0.0, fill);
            grid_p.line_segment(
                [band.left_bottom(), band.right_bottom()],
                Stroke::new(1.0, theme::LINE_FAINT),
            );
        }
        // Columns: grid lines, beats, bars.
        let grid = r.grid();
        let fine = if grid > 0.0 && grid * r.pps >= 8.0 {
            grid
        } else {
            1.0
        };
        let mut t = (r.x0 / fine).floor() * fine;
        let t_end = r.x0 + a.grid.width() / r.pps;
        while t <= t_end + fine {
            let x = Self::x_of(r, a, t);
            let bar = (t / 16.0).fract().abs() < 1.0e-3;
            let beat = (t / 4.0).fract().abs() < 1.0e-3;
            let (w, c) = if bar {
                (1.5, theme::BAR_LINE)
            } else if beat {
                (1.0, theme::LINE)
            } else {
                (1.0, theme::LINE_FAINT)
            };
            if fine * r.pps >= 6.0 || beat {
                grid_p.line_segment(
                    [pos2(x, a.grid.top()), pos2(x, a.grid.bottom())],
                    Stroke::new(w, c),
                );
            }
            t += fine;
        }
        // End of the loop.
        let xe = Self::x_of(r, a, steps);
        grid_p.rect_filled(
            Rect::from_min_max(pos2(xe, a.grid.top()), a.grid.max),
            0.0,
            theme::alpha(Color32::BLACK, 0.4),
        );

        // Ghost notes of the other layers.
        if let Some(looper) = &self.st.looper {
            for (slot, layer) in looper.layers.iter().enumerate() {
                let Some(layer) = layer else { continue };
                if slot == r.slot {
                    continue;
                }
                let color = theme::alpha(crate::screens::layer_color(slot), 0.22);
                for n in roll::from_layer(&layer.notes, steps) {
                    if let Some(nr) = Self::note_rect(r, a, &n) {
                        grid_p.rect_filled(nr, 4.0, color);
                    }
                }
            }
        }

        // This layer's notes.
        let color = crate::screens::layer_color(r.slot);
        for n in &r.notes {
            let Some(nr) = Self::note_rect(r, a, n) else {
                continue;
            };
            if nr.right() < a.grid.left() || nr.left() > a.grid.right() {
                continue;
            }
            let fill = if n.sel { theme::TEXT } else { color };
            widgets::glow(&grid_p, nr, 5.0, theme::alpha(color, 0.35), 10.0);
            grid_p.rect_filled(nr, 5.0, fill);
            // Loudness shows as the note's left edge strip.
            let strip = Rect::from_min_size(nr.min, vec2(3.0, nr.height()));
            grid_p.rect_filled(
                strip,
                2.0,
                theme::alpha(theme::ON_VIOLET, 0.25 + 0.6 * f32::from(n.vel) / 127.0),
            );
            if nr.width() > 30.0 && r.row_h >= 16.0 && !r.drums {
                let label = pitch_name(n.pitch);
                theme::text(
                    &grid_p,
                    pos2(nr.left() + 6.0, nr.center().y),
                    Align2::LEFT_CENTER,
                    &label,
                    inter(10.0, 600),
                    theme::ON_VIOLET,
                );
            }
        }

        // Selection box and slice line.
        match &r.gesture {
            Gesture::Boxing { at, to, .. } => {
                let b = Rect::from_two_pos(*at, *to);
                grid_p.rect_filled(b, 4.0, theme::alpha(theme::VIOLET, 0.15));
                grid_p.rect_stroke(
                    b,
                    4.0,
                    Stroke::new(1.5, theme::VIOLET_LIGHT),
                    egui::StrokeKind::Inside,
                );
            }
            Gesture::Slice { at, to, .. } => {
                let x = Self::x_of(r, a, roll::snap(Self::t_of(r, a, at.x), grid));
                grid_p.line_segment(
                    [pos2(x, at.y.min(to.y) - 8.0), pos2(x, at.y.max(to.y) + 8.0)],
                    Stroke::new(2.0, theme::AMBER),
                );
            }
            _ => {}
        }

        // Playhead.
        if self.status.running && self.status.loop_on {
            let x = Self::x_of(r, a, self.status.pos);
            grid_p.line_segment(
                [pos2(x, a.grid.top()), pos2(x, a.grid.bottom())],
                Stroke::new(2.0, theme::AMBER),
            );
        }

        // Ruler: bar numbers.
        let ruler_p = p.with_clip_rect(a.ruler);
        let mut bar = (r.x0 / 16.0).floor();
        while bar * 16.0 <= t_end {
            let x = Self::x_of(r, a, bar * 16.0);
            if bar * 16.0 < steps {
                theme::text(
                    &ruler_p,
                    pos2(x + 4.0, a.ruler.center().y),
                    Align2::LEFT_CENTER,
                    &format!("{}", bar as i32 + 1),
                    grotesk(12.0, 600),
                    theme::MUTED,
                );
            }
            bar += 1.0;
        }

        // Keys column.
        let keys_p = p.with_clip_rect(a.keys);
        for row in first..last {
            let pitch = r.rows[row];
            let y = Self::y_of(r, a, row);
            let k = Rect::from_min_max(
                pos2(a.keys.left(), y + 0.5),
                pos2(a.keys.right() - 4.0, y + r.row_h - 0.5),
            );
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let (fill, text) = if r.drums {
                (theme::SURFACE_2, theme::TEXT)
            } else if black {
                (theme::BG, theme::MUTED)
            } else {
                (theme::SURFACE_3, theme::TEXT)
            };
            keys_p.rect_filled(k, 4.0, fill);
            let label = if r.drums {
                roll::drum_name(pitch).unwrap_or("").to_owned()
            } else if pitch.is_multiple_of(12) || r.row_h >= 20.0 {
                pitch_name(pitch)
            } else {
                String::new()
            };
            if !label.is_empty() && r.row_h >= 12.0 {
                let style = inter(if r.row_h >= 20.0 { 11.0 } else { 9.0 }, 600);
                let label = theme::fit(&keys_p, &label, style, k.width() - 8.0);
                let color = if pitch % 12 == root && !r.drums {
                    theme::AMBER
                } else {
                    text
                };
                theme::text(
                    &keys_p,
                    pos2(k.right() - 5.0, k.center().y),
                    Align2::RIGHT_CENTER,
                    &label,
                    style,
                    color,
                );
            }
        }

        // Velocity lane.
        widgets::surface(p, a.vel, 10.0, theme::SURFACE, theme::LINE);
        theme::text(
            p,
            pos2(a.vel.left() - 6.0, a.vel.center().y),
            Align2::RIGHT_CENTER,
            "VEL",
            inter(10.0, 600).tracked(0.06),
            theme::MUTED,
        );
        let vel_p = p.with_clip_rect(a.vel.shrink(1.0));
        let any_sel = r.selected() > 0;
        for n in &r.notes {
            let x = Self::x_of(r, a, n.start);
            if x < a.vel.left() - 4.0 || x > a.vel.right() + 4.0 {
                continue;
            }
            let h = (a.vel.height() - 12.0) * f32::from(n.vel) / 127.0;
            let top = a.vel.bottom() - 6.0 - h;
            let c = if any_sel && !n.sel {
                theme::alpha(color, 0.35)
            } else if n.sel {
                theme::TEXT
            } else {
                color
            };
            vel_p.line_segment(
                [pos2(x + 2.0, a.vel.bottom() - 6.0), pos2(x + 2.0, top)],
                Stroke::new(3.0, c),
            );
            vel_p.circle_filled(pos2(x + 2.0, top), 4.0, c);
        }
    }

    // ------------------------------------------------------------------
    // Tools sheet

    pub(crate) fn roll_tools_height(&self) -> f32 {
        if self.landscape {
            20.0 + 28.0 + 14.0 + 166.0 + 12.0 + 62.0 + 20.0
        } else {
            20.0 + 28.0 + 14.0 + 166.0 + 12.0 + 3.0 * 74.0 + 8.0
        }
    }

    /// The Tools sheet. Returns true when it should close.
    pub(crate) fn roll_tools_sheet(&mut self, p: &Painter, inner: Rect, top: f32) -> bool {
        let Some(mut r) = self.roll.take() else {
            return true;
        };
        let close = self.roll_tools_body(p, inner, top, &mut r);
        self.roll = Some(r);
        close
    }

    fn roll_tools_body(&mut self, p: &Painter, inner: Rect, top: f32, r: &mut RollUi) -> bool {
        let wide = self.landscape;
        let gap = 14.0;
        let col_w = if wide {
            (inner.width() - gap) / 2.0
        } else {
            inner.width()
        };
        let left = Rect::from_min_size(pos2(inner.left(), top), vec2(col_w, inner.bottom() - top));
        let right = if wide {
            Rect::from_min_size(
                pos2(inner.left() + col_w + gap, top),
                vec2(col_w, inner.bottom() - top),
            )
        } else {
            left
        };
        let steps = self.loop_steps();
        let n_sel = r.selected();
        let label = if n_sel > 0 {
            format!(
                "EDIT {n_sel} SELECTED NOTE{}",
                if n_sel == 1 { "" } else { "S" }
            )
        } else {
            "EDIT ALL NOTES".to_owned()
        };
        let mut y = left.top();
        theme::text(
            p,
            pos2(left.left(), y + 8.0),
            Align2::LEFT_CENTER,
            &label,
            theme::SECTION,
            theme::MUTED,
        );
        y += 18.0;
        let rows: [&[&str]; 3] = [
            &["Quantize", "Humanize", "Duplicate"],
            &["Step −", "Step +", "Oct −", "Oct +"],
            &["Select all", "Deselect", "Delete"],
        ];
        let mut action = None;
        for (ri, labels) in rows.iter().enumerate() {
            let w = (col_w - 8.0 * (labels.len() as f32 - 1.0)) / labels.len() as f32;
            for (ci, l) in labels.iter().enumerate() {
                let b = Rect::from_min_size(
                    pos2(left.left() + ci as f32 * (w + 8.0), y),
                    vec2(w, 44.0),
                );
                let tap = self.touches.button(Id::unique(("roll-act", ri, ci)), b);
                widgets::surface(
                    p,
                    b,
                    22.0,
                    widgets::pressed(theme::SURFACE_2, tap.down),
                    theme::LINE,
                );
                let text = theme::fit(p, l, inter(13.0, 600), w - 8.0);
                theme::text(
                    p,
                    b.center(),
                    Align2::CENTER_CENTER,
                    &text,
                    inter(13.0, 600),
                    theme::TEXT,
                );
                if tap.clicked {
                    action = Some((ri, ci));
                }
            }
            y += 44.0 + 8.0;
        }
        y += 4.0;
        // Chord stamp.
        theme::text(
            p,
            pos2(left.left(), y + 8.0),
            Align2::LEFT_CENTER,
            "CHORD STAMP (TAP TO PLACE A CHORD)",
            theme::SECTION,
            theme::MUTED,
        );
        y += 18.0;
        let labels: Vec<&str> = Chord::ALL.iter().map(|c| c.label()).collect();
        let cur = Chord::ALL.iter().position(|c| *c == r.chord).unwrap_or(0);
        if let Some(i) = self.roll_segmented(
            p,
            Rect::from_min_size(pos2(left.left(), y), vec2(col_w, 44.0)),
            "roll-chord",
            &labels,
            cur,
        ) {
            r.chord = Chord::ALL[i];
        }
        let mut y = if wide { right.top() } else { y + 44.0 + 12.0 };
        // Rows and quantize strength.
        theme::text(
            p,
            pos2(right.left(), y + 8.0),
            Align2::LEFT_CENTER,
            "ROWS",
            theme::SECTION,
            theme::MUTED,
        );
        theme::text(
            p,
            pos2(right.left() + col_w / 2.0 + 4.0, y + 8.0),
            Align2::LEFT_CENTER,
            "QUANTIZE",
            theme::SECTION,
            theme::MUTED,
        );
        y += 18.0;
        let half = (col_w - 8.0) / 2.0;
        if !r.drums {
            if let Some(i) = self.roll_segmented(
                p,
                Rect::from_min_size(pos2(right.left(), y), vec2(half, 44.0)),
                "roll-rows",
                &["Scale", "All"],
                usize::from(r.all_rows),
            ) {
                r.all_rows = i == 1;
                self.roll_rows(r);
                r.fitted = false;
            }
        } else {
            theme::text(
                p,
                pos2(right.left() + 4.0, y + 22.0),
                Align2::LEFT_CENTER,
                "Drum names",
                inter(13.0, 500),
                theme::MUTED,
            );
        }
        if let Some(i) = self.roll_segmented(
            p,
            Rect::from_min_size(pos2(right.left() + half + 8.0, y), vec2(half, 44.0)),
            "roll-strength",
            &["100%", "50%"],
            usize::from(!r.full_strength),
        ) {
            r.full_strength = i == 0;
        }
        y += 44.0 + 12.0;
        // Loop length.
        theme::text(
            p,
            pos2(right.left(), y + 8.0),
            Align2::LEFT_CENTER,
            "LOOP LENGTH (ALL LAYERS)",
            theme::SECTION,
            theme::MUTED,
        );
        y += 18.0;
        let lengths = [16u16, 32, 64, 128];
        let cur = lengths
            .iter()
            .position(|&s| f32::from(s) == steps)
            .unwrap_or(1);
        if let Some(i) = self.roll_segmented(
            p,
            Rect::from_min_size(pos2(right.left(), y), vec2(col_w, 44.0)),
            "roll-length",
            &["1 bar", "2 bars", "4 bars", "8 bars"],
            cur,
        ) {
            self.roll_snapshot(r);
            self.roll_set_length(r, lengths[i], false);
            r.fitted = false;
        }

        let scale = roll::scale_pitches(&self.st.keymap());
        match action {
            Some((0, 0)) => {
                self.roll_snapshot(r);
                let strength = if r.full_strength { 1.0 } else { 0.5 };
                let grid = r.grid();
                roll::quantize(&mut r.notes, grid, strength, steps);
                self.roll_commit(r);
            }
            Some((0, 1)) => {
                self.roll_snapshot(r);
                let mut rng = self.rng;
                roll::humanize(&mut r.notes, 0.12, steps, || {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    (rng as f32 / u32::MAX as f32) * 2.0 - 1.0
                });
                self.rng = rng;
                self.roll_commit(r);
            }
            Some((0, 2)) => {
                self.roll_snapshot(r);
                let needed = roll::duplicate(&mut r.notes, steps);
                if needed > steps {
                    self.roll_set_length(r, needed as u16, true);
                    self.show_toast(&format!("The loop is now {} bars long", needed as u16 / 16));
                    r.fitted = false;
                } else {
                    self.roll_commit(r);
                }
            }
            Some((1, c)) => {
                self.roll_snapshot(r);
                let (deg, oct) = [(-1, 0), (1, 0), (0, -1), (0, 1)][c];
                roll::transpose(&mut r.notes, deg, oct, &scale);
                if !r.drums && r.notes.iter().any(|n| r.row_of(n.pitch).is_none()) {
                    r.all_rows = true;
                    self.roll_rows(r);
                }
                if let Some(n) = r.notes.iter().find(|n| n.sel).or(r.notes.first()) {
                    self.roll_preview(r, n.pitch, n.vel);
                }
                self.roll_commit(r);
            }
            Some((2, 0)) => r.notes.iter_mut().for_each(|n| n.sel = true),
            Some((2, 1)) => r.notes.iter_mut().for_each(|n| n.sel = false),
            Some((2, 2)) => {
                if n_sel > 0 {
                    self.roll_snapshot(r);
                    r.notes.retain(|n| !n.sel);
                    self.roll_commit(r);
                } else {
                    self.show_toast("Select notes first (long-press and drag)");
                }
            }
            _ => {}
        }
        false
    }

    fn roll_segmented(
        &mut self,
        p: &Painter,
        track: Rect,
        id: &'static str,
        labels: &[&str],
        active: usize,
    ) -> Option<usize> {
        p.rect_filled(track, track.height() / 2.0, theme::SURFACE_2);
        let w = (track.width() - 6.0) / labels.len() as f32;
        let mut picked = None;
        for (i, label) in labels.iter().enumerate() {
            let c = Rect::from_min_size(
                pos2(track.left() + 3.0 + i as f32 * w, track.top() + 3.0),
                vec2(w, track.height() - 6.0),
            );
            let tap = self.touches.button(Id::unique((id, i)), c);
            let on = i == active;
            if on {
                p.rect_filled(c, c.height() / 2.0, theme::VIOLET);
            }
            let style = inter(
                if labels.len() > 4 { 11.0 } else { 13.0 },
                if on { 600 } else { 500 },
            );
            let text = theme::fit(p, label, style, c.width() - 2.0);
            theme::text(
                p,
                c.center(),
                Align2::CENTER_CENTER,
                &text,
                style,
                if on { theme::ON_VIOLET } else { theme::TEXT },
            );
            if tap.clicked && !on {
                picked = Some(i);
            }
        }
        picked
    }
}
