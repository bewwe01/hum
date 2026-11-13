//! The egui application: state, routing between screens, and the pieces of
//! UI shared by several screens (tab bar, keyboard, BPM pill, sheets).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use eframe::egui::{self, pos2, vec2, Align2, Id, Painter, Rect};
use hum_engine::engine::{CaptureError, Cmd, EngineEvent, Status};
use hum_engine::presets::PRESETS;
use hum_engine::scale::{Scale, MAX_OCTAVE, MIN_OCTAVE, NOTE_NAMES};
use hum_engine::transport::{MAX_BPM, MIN_BPM};
use hum_engine::Category;

use crate::audio::AudioHost;
use crate::icons;
use crate::input::Touches;
use crate::platform::Platform;
use crate::state::{LoopLayer, Looper, Saved, Store};
use crate::theme::{self, grotesk, inter};
use crate::watchdog::Stage;
use crate::widgets;

/// State shared with the winit lifecycle wrapper in `lib.rs`.
#[derive(Default)]
pub struct Hooks {
    pub audio: RefCell<Option<AudioHost>>,
    /// Set when the app went to the background since the last frame.
    pub was_suspended: Cell<bool>,
    pub watchdog: crate::watchdog::Watchdog,
    /// Settings changed but not yet written; flushed on suspend and exit.
    pub unsaved: RefCell<Option<(Store, Saved)>>,
}

impl Hooks {
    /// Write any pending state now (the process may be killed in the
    /// background without further notice).
    pub fn flush_save(&self) {
        if let Some((store, saved)) = self.unsaved.borrow_mut().take() {
            store.save(&saved);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Play,
    Sounds,
    Loop,
    Beats,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Play, Tab::Sounds, Tab::Loop, Tab::Beats];

    fn label(self) -> &'static str {
        match self {
            Tab::Play => "Play",
            Tab::Sounds => "Sounds",
            Tab::Loop => "Loop",
            Tab::Beats => "Beats",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sheet {
    Key,
    Scale,
    ConfirmClearAll,
}

pub struct HumApp {
    pub(crate) platform: Platform,
    pub(crate) hooks: Rc<Hooks>,
    store: Store,
    pub(crate) st: Saved,
    saved: Saved,
    last_change: f64,
    save_pending: bool,

    pub(crate) touches: Touches,
    pub(crate) tab: Tab,
    pub(crate) settings_open: bool,
    pub(crate) sheet: Option<Sheet>,
    pub(crate) status: Status,
    pub(crate) now: f64,
    insets: [f32; 4],
    next_inset_poll: f64,
    next_reopen: f64,
    screen_on_applied: Option<bool>,
    focus_applied: bool,

    // Keyboard.
    key_of_touch: Vec<(u64, u8)>,
    pub(crate) key_down: [u8; 7],
    keyboard_seen: bool,

    // Play screen.
    pub(crate) trail: Vec<(f32, f32, f64)>,
    pub(crate) knob_last_tap: [f64; 4],

    // Sounds screen.
    pub(crate) search: String,
    pub(crate) category: Option<Category>,

    // Loop screen.
    /// Layers removed by undo or delete, newest last (slot, layer, steps, bpm).
    pub(crate) redo: Vec<(usize, LoopLayer, u16, f32)>,
    /// The Sounds screen is choosing a sound for this loop layer.
    pub(crate) assign_layer: Option<usize>,
    /// A short message shown above the tab bar until the given time.
    pub(crate) toast: Option<(String, f64)>,

    // Beats screen.
    pub(crate) drum: usize,
    pub(crate) rng: u32,

    bpm_repeat_at: f64,

    #[cfg(not(target_os = "android"))]
    pub(crate) shots: Option<crate::devshots::Shots>,
}

impl HumApp {
    pub fn new(cc: &eframe::CreationContext<'_>, platform: Platform, hooks: Rc<Hooks>) -> Self {
        theme::install(&cc.egui_ctx);
        hooks.watchdog.start();
        let store = Store::new(platform.data_dir());
        let st = store.load();
        log::info!(
            "Hum {} starting; state file: {}",
            crate::VERSION,
            store
                .path()
                .map_or("none".to_owned(), |p| p.display().to_string())
        );

        let mut audio = AudioHost::new(cc.egui_ctx.clone());
        audio.set_mode(st.buffer);
        audio.start();
        *hooks.audio.borrow_mut() = Some(audio);

        let status = hooks.audio.borrow().as_ref().map(|a| a.shared.status());
        let mut app = Self {
            platform,
            hooks,
            store,
            saved: st.clone(),
            st,
            last_change: 0.0,
            save_pending: false,
            touches: Touches::default(),
            tab: Tab::Play,
            settings_open: false,
            sheet: None,
            status: status.expect("audio host was just created"),
            now: 0.0,
            insets: [0.0; 4],
            next_inset_poll: 0.0,
            next_reopen: 0.0,
            screen_on_applied: None,
            focus_applied: false,
            key_of_touch: Vec::new(),
            key_down: [0; 7],
            keyboard_seen: false,
            trail: Vec::new(),
            knob_last_tap: [-10.0; 4],
            search: String::new(),
            category: None,
            redo: Vec::new(),
            assign_layer: None,
            toast: None,
            drum: 0,
            rng: 0x2545_F491,
            bpm_repeat_at: 0.0,
            #[cfg(not(target_os = "android"))]
            shots: crate::devshots::Shots::from_env(),
        };
        app.sync_engine();
        app
    }

    // ------------------------------------------------------------------
    // Engine access

    pub(crate) fn send(&self, cmd: Cmd) {
        if let Some(audio) = self.hooks.audio.borrow_mut().as_mut() {
            audio.send(cmd);
        }
    }

    pub(crate) fn with_audio<R>(&self, f: impl FnOnce(&mut AudioHost) -> R) -> Option<R> {
        self.hooks.audio.borrow_mut().as_mut().map(f)
    }

    /// Send the complete persistent state to the engine (at startup and
    /// whenever a fresh engine was created).
    fn sync_engine(&mut self) {
        self.push_params();
        self.send(Cmd::SetPreset(self.st.sound_index() as u8));
        self.send(Cmd::SetKeyMap(self.st.keymap()));
        self.send(Cmd::SetBpm(self.st.bpm));
        self.send(Cmd::Metronome(self.st.metronome));
        for (drum, bits) in self.st.patterns.iter().enumerate() {
            self.send(Cmd::SetPattern {
                drum: drum as u8,
                bits: *bits,
            });
        }
        self.send(Cmd::ClearAll);
        if let Some(looper) = &self.st.looper {
            self.send(Cmd::SetLoopSteps(looper.steps));
            for slot in 0..looper.layers.len() {
                self.send_layer(slot);
            }
        }
    }

    /// Mirror the pad and knob values into the engine's atomics.
    fn push_params(&self) {
        if let Some(audio) = self.hooks.audio.borrow().as_ref() {
            let s = &audio.shared;
            s.pad_x.set(self.st.pad_x);
            s.pad_y.set(self.st.pad_y);
            s.attack.set(self.st.attack);
            s.release.set(self.st.release);
            s.reverb.set(self.st.reverb);
            s.volume.set(self.st.volume);
        }
    }

    pub(crate) fn preset(&self) -> &'static hum_engine::Preset {
        &PRESETS[self.st.sound_index()]
    }

    /// Select a sound and reset the pad and knobs to its defaults.
    pub(crate) fn select_sound(&mut self, index: usize) {
        let index = index.min(PRESETS.len() - 1);
        let p = &PRESETS[index];
        self.release_all_keys();
        self.st.sound = p.name.to_owned();
        self.st.attack = hum_engine::engine::ATTACK_DEFAULT;
        self.st.release = hum_engine::engine::RELEASE_DEFAULT;
        self.st.reverb = p.reverb;
        self.st.pad_x = hum_engine::engine::PAD_X_DEFAULT;
        self.st.pad_y = hum_engine::engine::PAD_Y_DEFAULT;
        self.trail.clear();
        self.push_params();
        self.send(Cmd::SetPreset(index as u8));
    }

    /// Play a short preview of the current sound.
    pub(crate) fn preview(&self) {
        let note = if self.preset().category == Category::Bass {
            48
        } else {
            60
        };
        self.send(Cmd::Preview { note });
    }

    pub(crate) fn set_keymap_changed(&mut self) {
        self.send(Cmd::SetKeyMap(self.st.keymap()));
    }

    pub(crate) fn set_bpm(&mut self, bpm: f32) {
        let bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        if bpm != self.st.bpm {
            self.st.bpm = bpm;
            self.send(Cmd::SetBpm(bpm));
        }
    }

    pub(crate) fn haptic(&self) {
        if self.st.haptics {
            self.platform.haptic_tick();
        }
    }

    // ------------------------------------------------------------------
    // Per-frame housekeeping

    fn pump_audio(&mut self, ctx: &egui::Context) {
        if self.hooks.was_suspended.replace(false) {
            // Fingers that were down when the app left the foreground never
            // get their "up" event.
            self.touches.cancel_all();
            self.release_all_keys();
        }
        let mut reset = false;
        let mut events: Vec<EngineEvent> = Vec::new();
        if let Some(audio) = self.hooks.audio.borrow_mut().as_mut() {
            if audio.needs_reopen() && self.now >= self.next_reopen {
                // Headphones plugged or unplugged, or the device went away.
                self.next_reopen = self.now + 1.0;
                audio.reopen();
            }
            if !audio.is_running() && audio.last_error().is_some() && self.now >= self.next_reopen {
                self.next_reopen = self.now + 3.0;
                audio.start();
            }
            audio.log_stream_once();
            reset = audio.take_engine_reset();
            while let Some(e) = audio.poll_event() {
                events.push(e);
            }
            self.status = audio.shared.status();
        }
        if reset {
            self.sync_engine();
        }
        for e in events {
            self.on_engine_event(e);
        }
        self.push_params();

        // Hold audio focus exactly while the output stream is open.
        let running = self
            .with_audio(|a| a.is_running() && !a.is_silent())
            .unwrap_or(false);
        if running != self.focus_applied {
            self.platform.set_audio_focus(running);
            self.focus_applied = running;
        }

        let keep_on = self.st.keep_screen_on;
        if self.screen_on_applied != Some(keep_on) {
            self.platform.set_keep_screen_on(keep_on);
            self.screen_on_applied = Some(keep_on);
        }
        let _ = ctx;
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::LoopStarted { steps, bpm } => {
                self.st.bpm = bpm;
                self.st.looper = Some(Looper::new(steps));
            }
            EngineEvent::LayerCaptured { layer, preset, .. } => {
                let looper = self.st.looper.get_or_insert_with(|| Looper::new(64));
                let slot = usize::from(layer);
                if slot < looper.layers.len() {
                    looper.layers[slot] = Some(LoopLayer {
                        sound: PRESETS[usize::from(preset).min(PRESETS.len() - 1)]
                            .name
                            .to_owned(),
                        ..LoopLayer::default()
                    });
                    looper.order.retain(|&l| l != layer);
                    looper.order.push(layer);
                }
                self.redo.clear();
                self.haptic();
            }
            EngineEvent::LayerNote {
                layer,
                note,
                velocity,
                on,
                off,
            } => {
                if let Some(Some(l)) = self
                    .st
                    .looper
                    .as_mut()
                    .and_then(|lp| lp.layers.get_mut(usize::from(layer)))
                {
                    l.notes.push(crate::state::LoopNote {
                        note,
                        velocity,
                        on,
                        off,
                    });
                }
            }
            EngineEvent::CaptureFailed(why) => {
                let text = match why {
                    CaptureError::Nothing => "Play something first, then tap Loop",
                    CaptureError::TooShort => "Play a little longer (at least 1.5 s)",
                    CaptureError::Full => "All 8 layers are full: delete one first",
                };
                self.show_toast(text);
            }
        }
    }

    pub(crate) fn show_toast(&mut self, text: &str) {
        self.toast = Some((text.to_owned(), self.now + 2.8));
    }

    fn poll_insets(&mut self, ctx: &egui::Context) {
        if self.now < self.next_inset_poll {
            return;
        }
        // Insets change rarely (rotation is locked); poll gently.
        self.next_inset_poll = self.now + 0.5;
        let ppp = ctx.pixels_per_point().max(0.1);
        let i = self.platform.insets();
        let new = [i.left / ppp, i.top / ppp, i.right / ppp, i.bottom / ppp];
        if new != self.insets {
            self.insets = new;
            ctx.request_repaint();
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if self.st != self.saved {
            self.saved = self.st.clone();
            self.last_change = self.now;
            self.save_pending = true;
            *self.hooks.unsaved.borrow_mut() = Some((self.store.clone(), self.st.clone()));
        }
        if self.save_pending {
            if self.now - self.last_change >= 0.5 {
                self.hooks.flush_save();
                self.save_pending = false;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }

    // ------------------------------------------------------------------
    // Frame

    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);
        let watchdog = self.hooks.watchdog.clone();
        watchdog.enter(Stage::Input);
        self.touches.begin_frame(&ctx);
        watchdog.enter(Stage::Audio);
        self.pump_audio(&ctx);
        watchdog.enter(Stage::Insets);
        self.poll_insets(&ctx);
        watchdog.enter(Stage::Draw);
        self.keyboard_seen = false;
        self.handle_back(&ctx);

        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, theme::BG);
        let safe = Rect::from_min_max(
            full.min + vec2(self.insets[0], self.insets[1]),
            full.max - vec2(self.insets[2], self.insets[3]),
        );
        let content = Rect::from_min_max(
            safe.min + vec2(theme::PAD, theme::PAD),
            safe.max - vec2(theme::PAD, theme::PAD_BOTTOM),
        );

        // A modal sheet blocks the screen underneath.
        self.touches.set_blocked(self.sheet.is_some());
        if self.settings_open {
            self.settings_screen(ui, content);
        } else {
            let bar =
                Rect::from_min_max(pos2(content.left(), content.bottom() - 64.0), content.max);
            let body =
                Rect::from_min_max(content.min, pos2(content.right(), bar.top() - theme::GAP));
            match self.tab {
                Tab::Play => self.play_screen(ui, body),
                Tab::Sounds => self.sounds_screen(ui, body),
                Tab::Loop => self.loop_screen(ui, body),
                Tab::Beats => self.beats_screen(ui, body),
            }
            self.tab_bar(ui.painter(), bar);
        }
        self.touches.set_blocked(false);
        if let Some(sheet) = self.sheet {
            self.sheet_ui(ui.painter(), full, safe, sheet);
        }

        if !self.keyboard_seen {
            self.release_all_keys();
        }
        watchdog.enter(Stage::Save);
        self.autosave(&ctx);
        watchdog.enter(Stage::Outside);

        // Repaint only while something moves or is being touched.
        let transport_visible = self.status.running
            && !self.settings_open
            && matches!(self.tab, Tab::Loop | Tab::Beats);
        if self.touches.any_active() || !self.trail.is_empty() || transport_visible {
            ctx.request_repaint();
        }
        if self.status.running && self.st.looper.is_some() && self.tab == Tab::Play {
            // The Loop bar shows the playhead.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some((text, until)) = self.toast.clone() {
            if self.now < until {
                self.draw_toast(ui.painter(), safe, &text);
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            } else {
                self.toast = None;
            }
        }

        #[cfg(not(target_os = "android"))]
        crate::devshots::drive(self, &ctx);
    }

    /// Android back (and Escape on desktop): close the sheet, leave
    /// Settings, return to Play, and finally send the app to the background.
    fn handle_back(&mut self, ctx: &egui::Context) {
        let back = ctx
            .input(|i| i.key_pressed(egui::Key::BrowserBack) || i.key_pressed(egui::Key::Escape));
        if !back {
            return;
        }
        if self.sheet.is_some() {
            self.sheet = None;
        } else if self.settings_open {
            self.settings_open = false;
        } else if self.tab != Tab::Play {
            self.tab = Tab::Play;
        } else {
            self.release_all_keys();
            self.platform.leave();
        }
    }

    // ------------------------------------------------------------------
    // Shared widgets

    fn tab_bar(&mut self, p: &Painter, rect: Rect) {
        widgets::surface(p, rect, 24.0, theme::SURFACE, theme::LINE);
        let inner = rect.shrink(6.0);
        let gap = 4.0;
        let w = (inner.width() - gap * 3.0) / 4.0;
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(inner.left() + i as f32 * (w + gap), inner.top()),
                vec2(w, inner.height()),
            );
            let tap = self.touches.button(Id::unique(("tab", i)), r);
            if tap.clicked && self.tab != *tab {
                self.tab = *tab;
                self.assign_layer = None;
            }
            let active = self.tab == *tab;
            if active {
                p.rect_filled(r, 18.0, theme::SURFACE_3);
            } else if tap.down {
                p.rect_filled(r, 18.0, theme::SURFACE_2);
            }
            let color = if active { theme::AMBER } else { theme::MUTED };
            let icon_c = pos2(r.center().x, r.top() + 18.0);
            match tab {
                Tab::Play => icons::keys(p, icon_c, 24.0, color),
                Tab::Sounds => icons::bars(p, icon_c, 24.0, color),
                Tab::Loop => icons::loop_arrows(p, icon_c, 24.0, color),
                Tab::Beats => icons::grid(p, icon_c, 24.0, color),
            }
            let style = inter(12.0, if active { 600 } else { 500 });
            theme::text(
                p,
                pos2(r.center().x, r.bottom() - 6.0),
                Align2::CENTER_BOTTOM,
                tab.label(),
                style,
                color,
            );
        }
    }

    /// The BPM pill used on the Loop and Beats screens. `right` is its right
    /// edge; returns the rect it occupies.
    pub(crate) fn bpm_pill(&mut self, p: &Painter, right: f32, center_y: f32) -> Rect {
        let number = format!("{}", self.st.bpm.round() as i32);
        let num_w = theme::galley(p, "160", grotesk(15.0, 600), theme::TEXT)
            .size()
            .x;
        let unit_w = theme::galley(p, "BPM", inter(12.0, 400), theme::MUTED)
            .size()
            .x;
        let mid_w = num_w + 5.0 + unit_w;
        let width = 44.0 + mid_w + 44.0;
        let rect = Rect::from_min_size(pos2(right - width, center_y - 22.0), vec2(width, 44.0));
        widgets::surface(p, rect, 22.0, theme::SURFACE, theme::LINE);
        let minus = Rect::from_min_size(rect.min, vec2(44.0, 44.0));
        let plus = Rect::from_min_size(pos2(rect.right() - 44.0, rect.top()), vec2(44.0, 44.0));
        for (i, (r, delta)) in [(minus, -1.0f32), (plus, 1.0)].into_iter().enumerate() {
            let id = Id::unique(("bpm", i));
            let tap = self.touches.button(id, r);
            if tap.down {
                p.circle_filled(r.center(), 18.0, theme::SURFACE_2);
            }
            if tap.pressed {
                self.set_bpm(self.st.bpm + delta);
                self.bpm_repeat_at = self.now + 0.4;
            } else if tap.down && self.now >= self.bpm_repeat_at {
                // Holding the button keeps stepping.
                self.set_bpm(self.st.bpm + delta);
                self.bpm_repeat_at = self.now + 0.06;
            }
            if i == 0 {
                icons::minus(p, r.center(), 24.0, theme::TEXT);
            } else {
                icons::plus(p, r.center(), 24.0, theme::TEXT);
            }
        }
        let x = rect.left() + 44.0;
        theme::text(
            p,
            pos2(x + num_w, center_y),
            Align2::RIGHT_CENTER,
            &number,
            grotesk(15.0, 600),
            theme::TEXT,
        );
        theme::text(
            p,
            pos2(x + num_w + 5.0, center_y + 1.0),
            Align2::LEFT_CENTER,
            "BPM",
            inter(12.0, 400),
            theme::MUTED,
        );
        rect
    }

    /// The seven-key keyboard (Play and Loop screens). Multi-touch: every
    /// finger plays the key under it, and sliding plays each new key.
    pub(crate) fn keyboard(&mut self, p: &Painter, rect: Rect, root_dots: bool) {
        self.keyboard_seen = true;
        let id = Id::unique("keyboard");
        self.touches.claim(id, rect);
        let gap = 6.0;
        let key_w = (rect.width() - gap * 6.0) / 7.0;
        let key_at = |x: f32| -> u8 {
            (((x - rect.left() + gap * 0.5) / (key_w + gap)).floor() as i32).clamp(0, 6) as u8
        };

        // Work out which key each finger is on now.
        let touches: Vec<(u64, f32, bool)> = self
            .touches
            .owned(id)
            .map(|t| (t.id, t.pos.x, t.ended))
            .collect();
        for (touch, x, ended) in touches {
            let current = self.key_of_touch.iter().position(|(t, _)| *t == touch);
            if ended {
                if let Some(i) = current {
                    let (_, key) = self.key_of_touch.swap_remove(i);
                    self.key_release(key);
                }
                continue;
            }
            let key = key_at(x);
            match current {
                Some(i) if self.key_of_touch[i].1 == key => {}
                Some(i) => {
                    let old = self.key_of_touch[i].1;
                    self.key_of_touch[i].1 = key;
                    self.key_press(key);
                    self.key_release(old);
                }
                None => {
                    self.key_of_touch.push((touch, key));
                    self.key_press(key);
                }
            }
        }

        let map = self.st.keymap();
        for key in 0..7u8 {
            let r = Rect::from_min_size(
                pos2(rect.left() + f32::from(key) * (key_w + gap), rect.top()),
                vec2(key_w, rect.height()),
            );
            let down = self.key_down[key as usize] > 0;
            let note = map.note(i32::from(key));
            if down {
                widgets::glow(p, r, 16.0, theme::alpha(theme::VIOLET, 0.55), 26.0);
                widgets::surface(p, r, 16.0, theme::VIOLET, theme::VIOLET_LIGHT);
            } else {
                widgets::surface(p, r, 16.0, theme::SURFACE_2, theme::LINE);
            }
            if self.st.show_note_names && r.height() > 44.0 {
                let (style, color) = if down {
                    (grotesk(15.0, 700), theme::ON_VIOLET)
                } else {
                    (grotesk(15.0, 600), theme::TEXT)
                };
                widgets::note_label(
                    p,
                    pos2(r.center().x, r.bottom() - 12.0),
                    Align2::CENTER_BOTTOM,
                    note,
                    style,
                    color,
                );
            }
            if root_dots && map.is_root(i32::from(key)) {
                let color = if down { theme::ON_VIOLET } else { theme::AMBER };
                p.circle_filled(pos2(r.center().x, r.top() + 14.0), 4.0, color);
            }
        }
    }

    fn key_press(&mut self, key: u8) {
        let count = &mut self.key_down[key as usize];
        *count += 1;
        if *count == 1 {
            self.send(Cmd::KeyDown { key, velocity: 100 });
            self.haptic();
        }
    }

    fn key_release(&mut self, key: u8) {
        let count = &mut self.key_down[key as usize];
        if *count > 0 {
            *count -= 1;
            if *count == 0 {
                self.send(Cmd::KeyUp { key });
            }
        }
    }

    pub(crate) fn release_all_keys(&mut self) {
        self.key_of_touch.clear();
        for key in 0..7u8 {
            if self.key_down[key as usize] > 0 {
                self.key_down[key as usize] = 0;
                self.send(Cmd::KeyUp { key });
            }
        }
    }

    /// The octave stepper: minus, numeral, plus. `right` is its right edge.
    pub(crate) fn octave_stepper(&mut self, p: &Painter, right: f32, center_y: f32) -> Rect {
        let num_w = 26.0;
        let width = 44.0 + num_w + 44.0;
        let rect = Rect::from_min_size(pos2(right - width, center_y - 22.0), vec2(width, 44.0));
        let minus = Rect::from_min_size(rect.min, vec2(44.0, 44.0));
        let plus = Rect::from_min_size(pos2(rect.right() - 44.0, rect.top()), vec2(44.0, 44.0));
        for (i, (r, delta)) in [(minus, -1i8), (plus, 1)].into_iter().enumerate() {
            let enabled = (MIN_OCTAVE..=MAX_OCTAVE).contains(&(self.st.octave + delta));
            let tap = self.touches.button(Id::unique(("octave", i)), r);
            widgets::surface(
                p,
                r,
                22.0,
                widgets::pressed(theme::SURFACE, tap.down),
                theme::LINE,
            );
            if tap.clicked && enabled {
                self.st.octave += delta;
                self.set_keymap_changed();
            }
            let color = if enabled {
                theme::TEXT
            } else {
                theme::alpha(theme::MUTED, 0.45)
            };
            if i == 0 {
                icons::minus(p, r.center(), 24.0, color);
            } else {
                icons::plus(p, r.center(), 24.0, color);
            }
        }
        theme::text(
            p,
            pos2(rect.center().x, center_y),
            Align2::CENTER_CENTER,
            &format!("{}", self.st.octave),
            grotesk(15.0, 600),
            theme::TEXT,
        );
        rect
    }

    // ------------------------------------------------------------------
    // Sheets (key picker, scale picker, clear-all confirmation)

    fn sheet_ui(&mut self, p: &Painter, full: Rect, safe: Rect, sheet: Sheet) {
        p.rect_filled(full, 0.0, theme::alpha(egui::Color32::BLACK, 0.6));
        let width = safe.width() - 2.0 * theme::PAD;
        let height = match sheet {
            Sheet::Key => 20.0 + 28.0 + 14.0 + 3.0 * 48.0 + 2.0 * 8.0 + 20.0,
            Sheet::Scale => 20.0 + 28.0 + 14.0 + 7.0 * 48.0 + 6.0 * 6.0 + 20.0,
            Sheet::ConfirmClearAll => 20.0 + 28.0 + 8.0 + 40.0 + 16.0 + 48.0 + 20.0,
        };
        let rect = Rect::from_min_size(
            pos2(
                safe.left() + theme::PAD,
                safe.bottom() - theme::PAD_BOTTOM - height,
            ),
            vec2(width, height),
        );
        widgets::surface(p, rect, 26.0, theme::SURFACE, theme::LINE);
        let inner = rect.shrink(20.0);
        let title = match sheet {
            Sheet::Key => "Key",
            Sheet::Scale => "Scale",
            Sheet::ConfirmClearAll => "Clear all layers?",
        };
        theme::text(
            p,
            inner.left_top(),
            Align2::LEFT_TOP,
            title,
            grotesk(22.0, 700),
            theme::TEXT,
        );
        let top = inner.top() + 28.0 + 14.0;
        let mut close = false;

        match sheet {
            Sheet::Key => {
                let gap = 8.0;
                let w = (inner.width() - gap * 3.0) / 4.0;
                for (i, _) in NOTE_NAMES.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(
                            inner.left() + (i % 4) as f32 * (w + gap),
                            top + (i / 4) as f32 * (48.0 + gap),
                        ),
                        vec2(w, 48.0),
                    );
                    let selected = usize::from(self.st.key) == i;
                    let tap = self.touches.button(Id::unique(("sheet-key", i)), r);
                    let (fill, border, color) = if selected {
                        (theme::VIOLET, theme::VIOLET_LIGHT, theme::ON_VIOLET)
                    } else {
                        (
                            widgets::pressed(theme::SURFACE_2, tap.down),
                            theme::LINE,
                            theme::TEXT,
                        )
                    };
                    widgets::surface(p, r, 22.0, fill, border);
                    widgets::note_label(
                        p,
                        r.center(),
                        Align2::CENTER_CENTER,
                        i as u8,
                        grotesk(16.0, 600),
                        color,
                    );
                    if tap.clicked {
                        self.st.key = i as u8;
                        self.set_keymap_changed();
                        close = true;
                    }
                }
            }
            Sheet::Scale => {
                for (i, scale) in Scale::ALL.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(inner.left(), top + i as f32 * 54.0),
                        vec2(inner.width(), 48.0),
                    );
                    let selected = self.st.scale() == *scale;
                    let tap = self.touches.button(Id::unique(("sheet-scale", i)), r);
                    let (fill, border, color) = if selected {
                        (theme::VIOLET, theme::VIOLET_LIGHT, theme::ON_VIOLET)
                    } else {
                        (
                            widgets::pressed(theme::SURFACE_2, tap.down),
                            theme::LINE,
                            theme::TEXT,
                        )
                    };
                    widgets::surface(p, r, 22.0, fill, border);
                    theme::text(
                        p,
                        pos2(r.left() + 18.0, r.center().y),
                        Align2::LEFT_CENTER,
                        scale.name(),
                        inter(15.0, if selected { 600 } else { 500 }),
                        color,
                    );
                    let count = format!("{} notes", scale.intervals().len());
                    let c2 = if selected {
                        theme::ON_VIOLET
                    } else {
                        theme::MUTED
                    };
                    theme::text(
                        p,
                        pos2(r.right() - 18.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        &count,
                        inter(12.0, 400),
                        c2,
                    );
                    if tap.clicked {
                        self.st.scale = scale.name().to_owned();
                        self.set_keymap_changed();
                        close = true;
                    }
                }
            }
            Sheet::ConfirmClearAll => {
                widgets::wrapped(
                    p,
                    pos2(inner.left(), inner.top() + 36.0),
                    inner.width(),
                    "This removes every recorded layer. It cannot be undone.",
                    inter(14.0, 400),
                    theme::MUTED,
                );
                let gap = 10.0;
                let w = (inner.width() - gap) / 2.0;
                let y = inner.bottom() - 48.0;
                let cancel = Rect::from_min_size(pos2(inner.left(), y), vec2(w, 48.0));
                let confirm = Rect::from_min_size(pos2(inner.left() + w + gap, y), vec2(w, 48.0));
                let t1 = self.touches.button(Id::unique("sheet-cancel"), cancel);
                let t2 = self.touches.button(Id::unique("sheet-confirm"), confirm);
                widgets::surface(
                    p,
                    cancel,
                    24.0,
                    widgets::pressed(theme::SURFACE_2, t1.down),
                    theme::LINE,
                );
                theme::text(
                    p,
                    cancel.center(),
                    Align2::CENTER_CENTER,
                    "Cancel",
                    inter(15.0, 600),
                    theme::TEXT,
                );
                widgets::surface(
                    p,
                    confirm,
                    24.0,
                    widgets::pressed(theme::AMBER, t2.down),
                    theme::AMBER_LIGHT,
                );
                theme::text(
                    p,
                    confirm.center(),
                    Align2::CENTER_CENTER,
                    "Clear all",
                    inter(15.0, 600),
                    theme::ON_AMBER,
                );
                if t1.clicked {
                    close = true;
                }
                if t2.clicked {
                    self.send(Cmd::ClearAll);
                    self.st.looper = None;
                    self.redo.clear();
                    close = true;
                }
            }
        }

        // Tapping outside the sheet dismisses it.
        let outside = Id::unique("sheet-outside");
        self.touches.claim(outside, full);
        if self
            .touches
            .owned(outside)
            .any(|t| t.is_tap() && !rect.contains(t.pos))
        {
            close = true;
        }
        if close {
            self.sheet = None;
        }
    }
}

impl eframe::App for HumApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}
