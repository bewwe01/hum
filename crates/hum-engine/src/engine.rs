//! The engine: voices, effects, transport, beat sequencer, looper and
//! arpeggiator behind one real-time-safe `process` call.
//!
//! Threading model: the audio thread owns the [`Engine`]. The UI talks to it
//! with [`Cmd`] values (delivered through a lock-free queue by the host),
//! continuous controls in [`Shared`] (atomics), and reads back status from
//! [`Shared`] plus a small stream of [`EngineEvent`]s. `process` and
//! `handle` never allocate, lock or block.

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;

use rustysynth::Synthesizer;

use crate::dsp::{smooth_coef, Rng, TAU};
use crate::fx::{Chorus, Delay, Limiter, Reverb};
use crate::preset::{ArpPattern, Preset};
use crate::presets::{self, PRESETS};
use crate::scale::KeyMap;
use crate::transport::{
    Transport, DEFAULT_PATTERNS, DRUMS, LOOP_STEPS, MAX_BPM, MIN_BPM, STEPS_PER_BAR,
};
use crate::voice::{BlockCtx, Buses, Macros, Voice, MAX_BLOCK};

pub const VOICES: usize = 24;

/// Identifies a sound: values below [`SF_BASE`] index the synth preset
/// table, values from [`SF_BASE`] select a SoundFont preset
/// (`SF_BASE + bank * 128 + program`).
pub type SoundId = u16;
pub const SF_BASE: SoundId = 1024;

pub const fn sf_sound(bank: u16, program: u16) -> SoundId {
    SF_BASE + bank * 128 + program
}

/// SoundFont output level relative to the synth voices.
const SF_GAIN: f32 = 7.0;
/// MIDI channels used for SoundFont parts: the live part and 8 layers.
const SF_CHANNELS: usize = 9;
pub const LAYERS: usize = 8;
/// Maximum number of events (note-ons plus note-offs) in one loop layer.
pub const LAYER_CAPACITY: usize = 1024;
/// Live note events remembered for capturing (on and off).
const HISTORY: usize = 4096;
/// A first loop must last between these many seconds.
pub const MIN_LOOP_SECONDS: f64 = 1.5;
pub const MAX_LOOP_SECONDS: f64 = 32.0;
/// A pause longer than this ends the phrase that the first capture takes.
const PHRASE_GAP_SECONDS: f64 = 3.0;
/// Inferred tempo aims for this BPM.
const TARGET_BPM: f64 = 110.0;
const NO_LOOP_STEPS: u32 = LOOP_STEPS as u32;
/// On-screen keys, plus one virtual key used for previews.
pub const KEYS: usize = 7;
const PREVIEW_KEY: usize = KEYS;
const KEY_SLOTS: usize = KEYS + 1;

const PART_LIVE: u8 = 0;
const PART_LAYER0: u8 = 1;
const PART_DRUMS: u8 = PART_LAYER0 + LAYERS as u8;
const PARTS: usize = PART_DRUMS as usize + 1;

/// Pad X sweeps the cutoff over this many octaves in total.
pub const BRIGHT_OCTAVES: f32 = 6.0;
/// Neutral knob positions (the envelope multiplier is 1 here).
pub const ATTACK_DEFAULT: f32 = 0.2;
pub const RELEASE_DEFAULT: f32 = 0.6;
pub const VOLUME_DEFAULT: f32 = 0.75;
pub const PAD_X_DEFAULT: f32 = 0.5;
pub const PAD_Y_DEFAULT: f32 = 0.3;

/// Wet level of the reverb return.
const REVERB_RETURN: f32 = 1.6;
const CHORUS_RETURN: f32 = 0.8;

/// ATTACK knob (0..1) to envelope-time multiplier: 1x at 20%, up to 24x.
pub fn attack_multiplier(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v >= ATTACK_DEFAULT {
        24.0f32.powf((v - ATTACK_DEFAULT) / (1.0 - ATTACK_DEFAULT))
    } else {
        0.25f32.powf((ATTACK_DEFAULT - v) / ATTACK_DEFAULT)
    }
}

/// RELEASE knob (0..1) to envelope-time multiplier: 1x at 60%, up to 6x.
pub fn release_multiplier(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v >= RELEASE_DEFAULT {
        6.0f32.powf((v - RELEASE_DEFAULT) / (1.0 - RELEASE_DEFAULT))
    } else {
        (1.0f32 / 12.0).powf((RELEASE_DEFAULT - v) / RELEASE_DEFAULT)
    }
}

/// VOLUME knob (0..1) to linear master gain.
pub fn volume_gain(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    1.4 * v * v
}

/// Move `value` towards `target`, landing exactly once it is close.
#[inline]
fn glide(value: &mut f32, target: f32, coef: f32) {
    *value += (target - *value) * coef;
    if (target - *value).abs() < 1.0e-5 {
        *value = target;
    }
}

// ---------------------------------------------------------------------------
// Shared state (atomics)

/// An `f32` stored in an atomic.
#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// A snapshot of the transport / looper state for the UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub running: bool,
    pub beat_on: bool,
    pub loop_on: bool,
    pub metronome: bool,
    /// A loop length has been set (by the first capture or a load).
    pub has_loop: bool,
    /// Bit i = layer i holds notes.
    pub layers: u8,
    /// Bit i = layer i is muted.
    pub muted: u8,
    /// Loop length in sixteenth steps (64 when there is no loop).
    pub loop_steps: u32,
    /// Playhead in sixteenth steps, 0..loop_steps.
    pub pos: f32,
    /// Current step, or -1 when stopped.
    pub step: i32,
    pub active_voices: u32,
}

/// Lock-free state shared between the UI and the audio thread.
#[derive(Debug)]
pub struct Shared {
    // UI -> audio: continuous controls, all 0..1.
    pub pad_x: AtomicF32,
    pub pad_y: AtomicF32,
    pub attack: AtomicF32,
    pub release: AtomicF32,
    pub reverb: AtomicF32,
    pub volume: AtomicF32,
    // audio -> UI: status.
    flags: AtomicU32,
    loop_steps: AtomicU32,
    pos: AtomicF32,
    step: AtomicI32,
    voices: AtomicU32,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            pad_x: AtomicF32::new(PAD_X_DEFAULT),
            pad_y: AtomicF32::new(PAD_Y_DEFAULT),
            attack: AtomicF32::new(ATTACK_DEFAULT),
            release: AtomicF32::new(RELEASE_DEFAULT),
            reverb: AtomicF32::new(PRESETS[presets::DEFAULT_PRESET].reverb),
            volume: AtomicF32::new(VOLUME_DEFAULT),
            flags: AtomicU32::new(0),
            loop_steps: AtomicU32::new(NO_LOOP_STEPS),
            pos: AtomicF32::new(0.0),
            step: AtomicI32::new(-1),
            voices: AtomicU32::new(0),
        }
    }
}

impl Shared {
    pub fn status(&self) -> Status {
        let f = self.flags.load(Ordering::Relaxed);
        Status {
            running: f & 1 != 0,
            beat_on: f & 2 != 0,
            loop_on: f & 4 != 0,
            metronome: f & 8 != 0,
            has_loop: f & 16 != 0,
            layers: ((f >> 8) & 0xFF) as u8,
            muted: ((f >> 16) & 0xFF) as u8,
            loop_steps: self.loop_steps.load(Ordering::Relaxed),
            pos: self.pos.get(),
            step: self.step.load(Ordering::Relaxed),
            active_voices: self.voices.load(Ordering::Relaxed),
        }
    }
}

// ---------------------------------------------------------------------------
// Commands and events

/// Discrete messages from the UI to the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    /// An on-screen key (0..7) went down / up. Keys go through the scale
    /// lock and, for Arps sounds, the arpeggiator.
    KeyDown {
        key: u8,
        velocity: u8,
    },
    KeyUp {
        key: u8,
    },
    /// Play a raw MIDI note with the live sound (bypasses the arpeggiator).
    NoteOn {
        note: u8,
        velocity: u8,
    },
    NoteOff {
        note: u8,
    },
    /// Release everything that is sounding (fast fade).
    AllNotesOff,
    /// Select the live sound.
    SetSound(SoundId),
    /// Play a short preview of the live sound at the given MIDI note.
    Preview {
        note: u8,
    },
    SetKeyMap(KeyMap),
    SetBpm(f32),
    /// Replace one drum track's 16-step pattern (bit i = step i + 1).
    SetPattern {
        drum: u8,
        bits: u16,
    },
    BeatPlay(bool),
    LoopPlay(bool),
    Metronome(bool),
    /// Turn what was just played into a loop layer. With no loop yet, the
    /// phrase played so far sets the loop length and tempo; afterwards the
    /// last pass of the loop becomes a new layer.
    Capture,
    ClearLayer(u8),
    LayerVolume {
        layer: u8,
        volume: f32,
    },
    LayerSound {
        layer: u8,
        sound: SoundId,
    },
    /// Set the loop length in steps (0 = no loop). Used when restoring.
    SetLoopSteps(u16),
    /// Replace a layer's notes: BeginLayer, then LoadNote for each note,
    /// then EndLayer. Positions are in steps within the loop.
    BeginLayer {
        layer: u8,
        sound: SoundId,
        muted: bool,
        volume: f32,
    },
    LoadNote {
        layer: u8,
        note: u8,
        velocity: u8,
        on: f32,
        off: f32,
    },
    EndLayer(u8),
    MuteLayer {
        layer: u8,
        muted: bool,
    },
    ClearAll,
}

/// Why a capture did not produce a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// Nothing was played.
    Nothing,
    /// The first phrase was shorter than [`MIN_LOOP_SECONDS`].
    TooShort,
    /// All layers hold notes.
    Full,
}

/// Messages from the engine to the UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineEvent {
    /// The first capture set the loop length and tempo.
    LoopStarted {
        steps: u16,
        bpm: f32,
    },
    /// A layer was captured; `notes` LayerNote events follow.
    LayerCaptured {
        layer: u8,
        sound: SoundId,
        notes: u16,
    },
    /// One captured note; positions in steps within the loop. `off` may be
    /// smaller than `on` when the note crosses the loop point.
    LayerNote {
        layer: u8,
        note: u8,
        velocity: u8,
        on: f32,
        off: f32,
    },
    CaptureFailed(CaptureError),
}

const EVENT_QUEUE: usize = 4096;

#[derive(Clone, Copy, Debug)]
struct LoopEvent {
    t: f32,
    note: u8,
    /// 0 = note-off.
    velocity: u8,
}

struct Layer {
    events: Box<[LoopEvent]>,
    len: usize,
    cursor: usize,
    active: bool,
    muted: bool,
    sound: SoundId,
    volume: f32,
}

/// A live note event remembered for capturing.
#[derive(Clone, Copy, Debug)]
struct HistEvent {
    /// Engine sample clock.
    sample: u64,
    /// Unwrapped transport position in steps (valid within `epoch`).
    steps: f64,
    epoch: u32,
    note: u8,
    /// 0 = note-off.
    velocity: u8,
    sound: SoundId,
}

/// A note assembled during a capture.
#[derive(Clone, Copy, Debug)]
struct CapNote {
    on: f64,
    off: f64,
    note: u8,
    velocity: u8,
    sound: SoundId,
    open: bool,
}

const CAP_NOTES: usize = LAYER_CAPACITY / 2;

impl Layer {
    fn new() -> Self {
        Self {
            events: vec![
                LoopEvent {
                    t: 0.0,
                    note: 0,
                    velocity: 0
                };
                LAYER_CAPACITY
            ]
            .into_boxed_slice(),
            len: 0,
            cursor: 0,
            active: false,
            muted: false,
            sound: 0,
            volume: 1.0,
        }
    }

    fn sort(&mut self) {
        // Note-offs before note-ons at the same position (re-triggers).
        self.events[..self.len].sort_unstable_by(|a, b| {
            a.t.total_cmp(&b.t)
                .then((a.velocity > 0).cmp(&(b.velocity > 0)))
        });
    }
}

const NEVER: u64 = u64::MAX;

struct ArpState {
    held: [bool; KEY_SLOTS],
    velocity: u8,
    index: u32,
    /// Samples until the next step / until the sounding note is released.
    next: u64,
    off: u64,
    note: Option<u8>,
    last: Option<u8>,
}

impl ArpState {
    fn new() -> Self {
        Self {
            held: [false; KEY_SLOTS],
            velocity: 100,
            index: 0,
            next: NEVER,
            off: NEVER,
            note: None,
            last: None,
        }
    }

    fn any_held(&self) -> bool {
        self.held.iter().any(|h| *h)
    }
}

pub struct Engine {
    sample_rate: f32,
    shared: Arc<Shared>,
    voices: Box<[Voice]>,
    age: u64,
    buses: Buses,
    chorus: Chorus,
    delay: Delay,
    reverb: Reverb,
    limiter: Limiter,
    volume: f32,
    rng: Rng,

    live_sound: SoundId,
    keymap: KeyMap,
    /// Note currently sounding for each key slot.
    keys: [Option<u8>; KEY_SLOTS],
    macros: [Macros; PARTS],
    last_pitch: [f32; PARTS],
    arp: ArpState,
    preview_off: u64,

    transport: Transport,
    next_step: i64,
    beat_on: bool,
    loop_on: bool,
    metronome: bool,
    patterns: [u16; DRUMS],
    drum_presets: [u8; DRUMS],
    layers: [Layer; LAYERS],
    has_loop: bool,
    loop_steps: u32,
    /// Loop passes since the transport (re)started, for unwrapped positions.
    wraps: i64,
    /// Bumped whenever the transport restarts; history positions in steps
    /// are only comparable within one epoch.
    epoch: u32,
    /// Engine sample clock.
    now: u64,
    history: Box<[HistEvent]>,
    hist_head: usize,
    hist_len: usize,
    cap: Box<[CapNote]>,

    click_phase: f32,
    click_inc: f32,
    click_env: f32,

    events: [EngineEvent; EVENT_QUEUE],
    events_len: usize,

    /// SoundFont player (handed over once the bank has loaded).
    sf: Option<Box<Synthesizer>>,
    /// Sound and controller values last sent to each SoundFont channel.
    sf_sound: [SoundId; SF_CHANNELS],
    sf_cc: [[i32; 3]; SF_CHANNELS],
    sf_left: [f32; MAX_BLOCK],
    sf_right: [f32; MAX_BLOCK],
}

impl Engine {
    /// Allocates all buffers. Call from a non-real-time thread.
    pub fn new(sample_rate: f32, shared: Arc<Shared>) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 8000.0 {
            sample_rate
        } else {
            48_000.0
        };
        let voices: Vec<Voice> = (0..VOICES)
            .map(|i| Voice::new(0x1234_5678 ^ (i as u32 * 7919 + 1)))
            .collect();
        let drum_presets = presets::BEAT_DRUMS.map(|name| presets::find(name).unwrap_or(0) as u8);
        let live_sound = presets::DEFAULT_PRESET as SoundId;
        let mut engine = Self {
            sample_rate,
            shared,
            voices: voices.into_boxed_slice(),
            age: 0,
            buses: Buses::new(),
            chorus: Chorus::new(sample_rate),
            delay: Delay::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            limiter: Limiter::new(sample_rate),
            volume: 0.0,
            rng: Rng::new(0xC0FF_EE11),
            live_sound,
            keymap: KeyMap::default(),
            keys: [None; KEY_SLOTS],
            macros: [Macros::NEUTRAL; PARTS],
            last_pitch: [60.0; PARTS],
            arp: ArpState::new(),
            preview_off: NEVER,
            transport: Transport::new(sample_rate),
            next_step: 0,
            beat_on: false,
            loop_on: false,
            metronome: false,
            patterns: DEFAULT_PATTERNS,
            drum_presets,
            layers: std::array::from_fn(|_| Layer::new()),
            has_loop: false,
            loop_steps: NO_LOOP_STEPS,
            wraps: 0,
            epoch: 0,
            now: 0,
            history: vec![
                HistEvent {
                    sample: 0,
                    steps: 0.0,
                    epoch: 0,
                    note: 0,
                    velocity: 0,
                    sound: 0
                };
                HISTORY
            ]
            .into_boxed_slice(),
            hist_head: 0,
            hist_len: 0,
            cap: vec![
                CapNote {
                    on: 0.0,
                    off: 0.0,
                    note: 0,
                    velocity: 0,
                    sound: 0,
                    open: false
                };
                CAP_NOTES
            ]
            .into_boxed_slice(),
            click_phase: 0.0,
            click_inc: 0.0,
            click_env: 0.0,
            events: [EngineEvent::CaptureFailed(CaptureError::Nothing); EVENT_QUEUE],
            events_len: 0,
            sf: None,
            sf_sound: [SoundId::MAX; SF_CHANNELS],
            sf_cc: [[-1; 3]; SF_CHANNELS],
            sf_left: [0.0; MAX_BLOCK],
            sf_right: [0.0; MAX_BLOCK],
        };
        engine.volume = volume_gain(engine.shared.volume.get());
        engine.macros[PART_LIVE as usize] = engine.live_targets();
        engine.update_delay();
        engine.publish();
        engine
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// The live sound's synth preset (None for a SoundFont sound).
    pub fn live_synth(&self) -> Option<&'static Preset> {
        PRESETS.get(usize::from(self.live_sound))
    }

    /// Install the SoundFont player; returns the previous one, if any.
    pub fn set_synth(&mut self, synth: Box<Synthesizer>) -> Option<Box<Synthesizer>> {
        self.sf_sound = [SoundId::MAX; SF_CHANNELS];
        self.sf_cc = [[-1; 3]; SF_CHANNELS];
        self.sf.replace(synth)
    }

    pub fn has_soundfont(&self) -> bool {
        self.sf.is_some()
    }

    pub fn bpm(&self) -> f32 {
        self.transport.bpm()
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Hand all queued [`EngineEvent`]s to `f` and clear the queue.
    pub fn drain_events(&mut self, mut f: impl FnMut(EngineEvent)) {
        for e in &self.events[..self.events_len] {
            f(*e);
        }
        self.events_len = 0;
    }

    fn emit(&mut self, e: EngineEvent) {
        if self.events_len < EVENT_QUEUE {
            self.events[self.events_len] = e;
            self.events_len += 1;
        }
    }

    // ------------------------------------------------------------------
    // Commands

    /// Apply a command. Real-time safe.
    pub fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::KeyDown { key, velocity } => self.key_down(key as usize, velocity, None),
            Cmd::KeyUp { key } => self.key_up(key as usize),
            Cmd::NoteOn { note, velocity } => self.live_on(note.min(127), velocity.clamp(1, 127)),
            Cmd::NoteOff { note } => self.live_off(note),
            Cmd::AllNotesOff => {
                self.release_live();
                for v in self.voices.iter_mut() {
                    v.kill();
                }
                if let Some(sf) = self.sf.as_mut() {
                    sf.note_off_all(false);
                }
            }
            Cmd::SetSound(sound) => {
                if sound != self.live_sound {
                    self.release_live();
                    self.live_sound = sound;
                    self.update_delay();
                }
            }
            Cmd::Preview { note } => {
                self.key_up(PREVIEW_KEY);
                self.key_down(PREVIEW_KEY, 100, Some(note.min(127)));
                let arp = self.live_synth().is_some_and(|p| p.arp.is_some());
                let seconds = if arp { 1.6 } else { 0.6 };
                self.preview_off = (seconds * self.sample_rate) as u64;
            }
            Cmd::SetKeyMap(map) => self.keymap = map,
            Cmd::SetBpm(bpm) => {
                self.transport.set_bpm(bpm);
                self.update_delay();
            }
            Cmd::SetPattern { drum, bits } => {
                if let Some(p) = self.patterns.get_mut(drum as usize) {
                    *p = bits;
                }
            }
            Cmd::BeatPlay(on) => {
                self.beat_on = on;
                if on {
                    self.ensure_running();
                } else {
                    self.maybe_stop();
                }
            }
            Cmd::LoopPlay(on) => {
                if on {
                    self.loop_on = true;
                    self.ensure_running();
                } else {
                    self.loop_on = false;
                    for i in 0..LAYERS {
                        self.release_part(PART_LAYER0 + i as u8);
                    }
                    self.maybe_stop();
                }
            }
            Cmd::Metronome(on) => self.metronome = on,
            Cmd::Capture => self.capture(),
            Cmd::ClearLayer(layer) => {
                self.clear_layer(layer as usize);
                if !self.layers.iter().any(|l| l.active) {
                    self.reset_loop();
                }
                self.maybe_stop();
            }
            Cmd::LayerVolume { layer, volume } => {
                if let Some(l) = self.layers.get_mut(layer as usize) {
                    l.volume = volume.clamp(0.0, 1.5);
                    self.macros[PART_LAYER0 as usize + layer as usize].gain = l.volume;
                }
            }
            Cmd::LayerSound { layer, sound } => {
                if (layer as usize) < LAYERS {
                    self.layers[layer as usize].sound = sound;
                    self.release_part(PART_LAYER0 + layer);
                }
            }
            Cmd::SetLoopSteps(steps) => {
                if steps == 0 {
                    self.reset_loop();
                } else {
                    self.has_loop = true;
                    self.loop_steps = u32::from(steps);
                }
            }
            Cmd::BeginLayer {
                layer,
                sound,
                muted,
                volume,
            } => {
                if (layer as usize) < LAYERS {
                    self.release_part(PART_LAYER0 + layer);
                    let l = &mut self.layers[layer as usize];
                    l.active = false;
                    l.len = 0;
                    l.cursor = 0;
                    l.sound = sound;
                    l.muted = muted;
                    l.volume = volume.clamp(0.0, 1.5);
                    self.macros[PART_LAYER0 as usize + layer as usize] = Macros {
                        gain: l.volume,
                        ..Macros::NEUTRAL
                    };
                }
            }
            Cmd::LoadNote {
                layer,
                note,
                velocity,
                on,
                off,
            } => {
                let steps = self.loop_steps as f32;
                if let Some(l) = self.layers.get_mut(layer as usize) {
                    if l.len + 2 <= LAYER_CAPACITY && on.is_finite() && off.is_finite() {
                        let on = on.rem_euclid(steps);
                        let off = off.rem_euclid(steps);
                        l.events[l.len] = LoopEvent {
                            t: on,
                            note,
                            velocity: velocity.max(1),
                        };
                        l.events[l.len + 1] = LoopEvent {
                            t: off,
                            note,
                            velocity: 0,
                        };
                        l.len += 2;
                    }
                }
            }
            Cmd::EndLayer(layer) => {
                let pos = self.transport.pos() as f32;
                if let Some(l) = self.layers.get_mut(layer as usize) {
                    l.sort();
                    l.active = l.len > 0;
                    l.cursor = l.events[..l.len]
                        .iter()
                        .position(|e| e.t >= pos)
                        .unwrap_or(l.len);
                }
            }
            Cmd::MuteLayer { layer, muted } => {
                if let Some(l) = self.layers.get_mut(layer as usize) {
                    l.muted = muted;
                    if muted {
                        self.release_part(PART_LAYER0 + layer);
                    }
                }
            }
            Cmd::ClearAll => {
                for i in 0..LAYERS {
                    self.clear_layer(i);
                }
                self.reset_loop();
                self.maybe_stop();
            }
        }
        self.publish();
    }

    fn update_delay(&mut self) {
        let (beats, feedback) = self
            .live_synth()
            .map_or((0.75, 0.3), |p| (p.delay_beats, p.delay_feedback));
        self.delay.set(beats, self.transport.bpm(), feedback);
    }

    fn ensure_running(&mut self) {
        if !self.transport.running {
            self.restart_transport();
        }
    }

    fn restart_transport(&mut self) {
        self.transport.start(0.0);
        self.next_step = 0;
        self.wraps = 0;
        self.epoch = self.epoch.wrapping_add(1);
        for l in self.layers.iter_mut() {
            l.cursor = 0;
        }
    }

    /// Forget the loop length (no layers left).
    fn reset_loop(&mut self) {
        self.has_loop = false;
        self.loop_steps = NO_LOOP_STEPS;
        self.loop_on = false;
    }

    /// Stop the clock when nothing needs it any more.
    fn maybe_stop(&mut self) {
        if !self.layers.iter().any(|l| l.active) {
            self.loop_on = false;
        }
        if !self.beat_on && !self.loop_on && self.transport.running {
            self.transport.stop();
            self.next_step = 0;
            for i in 0..LAYERS {
                self.layers[i].cursor = 0;
                self.release_part(PART_LAYER0 + i as u8);
            }
        }
    }

    // ------------------------------------------------------------------
    // Keys, live notes, arpeggiator

    fn key_down(&mut self, key: usize, velocity: u8, fixed_note: Option<u8>) {
        if key >= KEY_SLOTS || self.keys[key].is_some() {
            return;
        }
        let velocity = velocity.clamp(1, 127);
        let note = fixed_note.unwrap_or_else(|| self.keymap.note(key as i32));
        self.keys[key] = Some(note);
        if self.live_synth().is_some_and(|p| p.arp.is_some()) {
            let was_idle = !self.arp.any_held();
            self.arp.held[key] = true;
            self.arp.velocity = velocity;
            if was_idle {
                self.arp.index = 0;
                self.arp.last = None;
                self.arp.next = 0;
            }
        } else {
            self.live_on(note, velocity);
        }
    }

    fn key_up(&mut self, key: usize) {
        if key >= KEY_SLOTS {
            return;
        }
        let Some(note) = self.keys[key].take() else {
            return;
        };
        if key == PREVIEW_KEY {
            self.preview_off = NEVER;
        }
        if self.arp.held[key] {
            self.arp.held[key] = false;
            if !self.arp.any_held() {
                self.arp.next = NEVER;
            }
        } else {
            self.live_off(note);
        }
    }

    /// Release all keys and anything the live part is playing.
    fn release_live(&mut self) {
        for key in 0..KEY_SLOTS {
            self.key_up(key);
        }
        self.arp.held = [false; KEY_SLOTS];
        self.arp.next = NEVER;
        self.arp.off = NEVER;
        if let Some(note) = self.arp.note.take() {
            self.live_off(note);
        }
        self.release_part(PART_LIVE);
        self.preview_off = NEVER;
    }

    fn live_on(&mut self, note: u8, velocity: u8) {
        self.note_on(PART_LIVE, self.live_sound, note, velocity);
        self.remember(note, velocity);
    }

    fn live_off(&mut self, note: u8) {
        self.note_off(PART_LIVE, note);
        self.remember(note, 0);
    }

    /// Unwrapped transport position in steps.
    fn unwrapped_pos(&self) -> f64 {
        self.wraps as f64 * f64::from(self.loop_steps) + self.transport.pos()
    }

    /// Add a live note event to the capture history (a ring buffer).
    fn remember(&mut self, note: u8, velocity: u8) {
        let event = HistEvent {
            sample: self.now,
            steps: if self.transport.running {
                self.unwrapped_pos()
            } else {
                f64::NAN
            },
            epoch: self.epoch,
            note,
            velocity,
            sound: self.live_sound,
        };
        self.history[self.hist_head] = event;
        self.hist_head = (self.hist_head + 1) % HISTORY;
        self.hist_len = (self.hist_len + 1).min(HISTORY);
    }

    /// History event `i` counted from the oldest.
    fn hist(&self, i: usize) -> HistEvent {
        let oldest = (self.hist_head + HISTORY - self.hist_len) % HISTORY;
        self.history[(oldest + i) % HISTORY]
    }

    fn arp_step(&mut self) {
        let Some(arp) = self.live_synth().and_then(|p| p.arp) else {
            self.arp.next = NEVER;
            return;
        };
        // Build the note pool from the held keys.
        let mut pool = [0u8; 40];
        let mut len = 0usize;
        let held = self.arp.held.iter().filter(|h| **h).count();
        if held == 0 {
            self.arp.next = NEVER;
            return;
        }
        let mut base = [0u8; KEY_SLOTS + 2];
        let mut base_len = 0usize;
        if held == 1 && arp.pattern != ArpPattern::Repeat {
            // One finger plays a chord built from the scale.
            let key = self.arp.held.iter().position(|h| *h).unwrap_or(0);
            let root = self.keys[key].unwrap_or(60);
            if key < KEYS && self.keymap.locked {
                for step in [0, 2, 4] {
                    base[base_len] = self.keymap.note(key as i32 + step);
                    base_len += 1;
                }
            } else if self.keymap.locked {
                // Preview key: a chord on the scale's first degrees, moved to
                // the preview note.
                let origin = self.keymap.note(0);
                for step in [0, 2, 4] {
                    let offset = self.keymap.note(step).saturating_sub(origin);
                    base[base_len] = root.saturating_add(offset).min(127);
                    base_len += 1;
                }
            } else {
                for offset in [0u8, 4, 7] {
                    base[base_len] = root.saturating_add(offset).min(127);
                    base_len += 1;
                }
            }
        } else {
            for key in 0..KEY_SLOTS {
                if self.arp.held[key] {
                    base[base_len] = self.keys[key].unwrap_or(60);
                    base_len += 1;
                }
            }
            base[..base_len].sort_unstable();
        }
        for octave in 0..arp.octaves.clamp(1, 3) {
            for b in &base[..base_len] {
                let n = u16::from(*b) + 12 * u16::from(octave);
                if n <= 127 && len < pool.len() {
                    pool[len] = n as u8;
                    len += 1;
                }
            }
        }
        if len == 0 {
            self.arp.next = NEVER;
            return;
        }

        let i = self.arp.index as usize;
        let note = match arp.pattern {
            ArpPattern::Up | ArpPattern::Repeat => pool[i % len],
            ArpPattern::Down => pool[len - 1 - i % len],
            ArpPattern::UpDown => {
                let period = (2 * len).saturating_sub(2).max(1);
                let j = i % period;
                pool[if j < len { j } else { period - j }]
            }
            ArpPattern::Random => {
                let mut pick = pool[self.rng.next_u32() as usize % len];
                if len > 1 && Some(pick) == self.arp.last {
                    pick = pool[(self.rng.next_u32() as usize) % len];
                }
                pick
            }
            ArpPattern::UpOctave => {
                let j = i % (2 * len);
                (u16::from(pool[j / 2]) + 12 * (j % 2) as u16).min(127) as u8
            }
            ArpPattern::InOut => {
                let j = i % len;
                if j.is_multiple_of(2) {
                    pool[j / 2]
                } else {
                    pool[len - 1 - j / 2]
                }
            }
        };
        self.arp.index = self.arp.index.wrapping_add(1);

        if let Some(prev) = self.arp.note.take() {
            self.live_off(prev);
        }
        let velocity = self.arp.velocity;
        self.live_on(note, velocity);
        self.arp.note = Some(note);
        self.arp.last = Some(note);

        // Schedule the note-off and the next step.
        let step = f64::from(self.sample_rate) * 60.0
            / (f64::from(self.transport.bpm()) * f64::from(arp.steps_per_beat.max(1)));
        self.arp.off = ((step * f64::from(arp.gate.clamp(0.05, 1.0))) as u64).max(1);
        let mut next = step.round().max(1.0) as u64;
        if self.transport.running {
            // Lock to the transport grid so arps stay in time with the beat.
            let grid = 4.0 / f64::from(arp.steps_per_beat.max(1));
            let pos = self.transport.pos();
            let mut boundary = ((pos / grid + 1.0e-6).floor() + 1.0) * grid;
            let mut wait = self.transport.samples_until(boundary);
            if (wait as f64) < step * 0.25 {
                boundary += grid;
                wait = self.transport.samples_until(boundary);
            }
            next = wait.max(1);
        }
        self.arp.next = next;
    }

    // ------------------------------------------------------------------
    // Voices

    fn note_on(&mut self, part: u8, sound: SoundId, note: u8, velocity: u8) {
        if sound >= SF_BASE {
            self.sf_note_on(part, sound, note, velocity);
            return;
        }
        let preset = &PRESETS[usize::from(sound).min(PRESETS.len() - 1)];
        // Prefer a free voice, then the oldest released one, then the oldest.
        let mut best = 0usize;
        let mut best_rank = (3u8, u64::MAX);
        for (i, v) in self.voices.iter().enumerate() {
            let rank = if !v.active {
                (0, 0)
            } else if v.is_releasing() {
                (1, v.age)
            } else {
                (2, v.age)
            };
            if rank < best_rank {
                best_rank = rank;
                best = i;
            }
        }
        self.age += 1;
        let from = self.last_pitch[part as usize];
        let gain = f32::from(velocity) / 127.0;
        self.voices[best].note_on(preset, part, note, 0.35 + 0.65 * gain, from, self.age);
        self.last_pitch[part as usize] = f32::from(note);
    }

    fn note_off(&mut self, part: u8, note: u8) {
        for v in self.voices.iter_mut() {
            if v.active && v.gate && v.part == part && v.note == note {
                v.note_off();
            }
        }
        if let Some(sf) = self.sf.as_mut() {
            if usize::from(part) < SF_CHANNELS {
                sf.note_off(i32::from(part), i32::from(note));
            }
        }
    }

    fn release_part(&mut self, part: u8) {
        for v in self.voices.iter_mut() {
            if v.active && v.gate && v.part == part {
                v.note_off();
            }
        }
        if let Some(sf) = self.sf.as_mut() {
            if usize::from(part) < SF_CHANNELS {
                sf.note_off_all_channel(i32::from(part), false);
            }
        }
    }

    /// Play a SoundFont note on the part's channel, switching the channel's
    /// bank and program first if needed.
    fn sf_note_on(&mut self, part: u8, sound: SoundId, note: u8, velocity: u8) {
        let ch = usize::from(part);
        let Some(sf) = self.sf.as_mut() else { return };
        if ch >= SF_CHANNELS {
            return;
        }
        if self.sf_sound[ch] != sound {
            let rel = i32::from(sound - SF_BASE);
            sf.note_off_all_channel(ch as i32, false);
            sf.process_midi_message(ch as i32, 0xB0, 0x00, rel / 128);
            sf.process_midi_message(ch as i32, 0xC0, rel % 128, 0);
            self.sf_sound[ch] = sound;
        }
        sf.note_on(ch as i32, i32::from(note), i32::from(velocity));
    }

    /// Mirror volume, reverb and modulation into the SoundFont channels.
    fn sf_controls(&mut self) {
        let Some(sf) = self.sf.as_mut() else { return };
        for ch in 0..SF_CHANNELS {
            let m = &self.macros[ch];
            let volume = if ch == 0 {
                1.0
            } else {
                self.layers[ch - 1].volume
            };
            let reverb = if m.reverb >= 0.0 { m.reverb } else { 0.3 };
            let wobble = if ch == 0 { m.wobble } else { 0.0 };
            let values = [
                (volume * 100.0).round().clamp(0.0, 127.0) as i32,
                (reverb * 127.0).round().clamp(0.0, 127.0) as i32,
                (wobble * 100.0).round().clamp(0.0, 127.0) as i32,
            ];
            for (k, cc) in [0x07, 0x5B, 0x01].into_iter().enumerate() {
                if self.sf_cc[ch][k] != values[k] {
                    sf.process_midi_message(ch as i32, 0xB0, cc, values[k]);
                    self.sf_cc[ch][k] = values[k];
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Looper

    /// Pair up remembered note events whose note-on lies in `[from, to]`
    /// (measured with `at`) into `self.cap`. Notes still held at `to` end
    /// there. Returns the number of notes.
    fn collect(&mut self, from: f64, to: f64, at: impl Fn(&HistEvent) -> Option<f64>) -> usize {
        let mut count = 0usize;
        for i in 0..self.hist_len {
            let e = self.hist(i);
            let Some(x) = at(&e) else { continue };
            if e.velocity > 0 {
                if x >= from && x <= to && count < CAP_NOTES {
                    self.cap[count] = CapNote {
                        on: x,
                        off: to,
                        note: e.note,
                        velocity: e.velocity,
                        sound: e.sound,
                        open: true,
                    };
                    count += 1;
                }
            } else if let Some(n) = self.cap[..count]
                .iter_mut()
                .rev()
                .find(|n| n.open && n.note == e.note)
            {
                n.off = x.min(to);
                n.open = false;
            }
        }
        count
    }

    fn capture(&mut self) {
        let Some(slot) = self.layers.iter().position(|l| !l.active) else {
            self.emit(EngineEvent::CaptureFailed(CaptureError::Full));
            return;
        };
        let sr = f64::from(self.sample_rate);
        let (count, from, scale) = if !self.has_loop {
            // The phrase: walk back from now over notes with no long pause.
            let now = self.now;
            // Silence before the tap belongs to the loop; only a pause
            // between notes ends the phrase.
            let mut start = None;
            let mut last: Option<u64> = None;
            for i in (0..self.hist_len).rev() {
                let e = self.hist(i);
                let gap = last.map_or(0, |l| l.saturating_sub(e.sample));
                if gap as f64 > PHRASE_GAP_SECONDS * sr
                    || now.saturating_sub(e.sample) as f64 > MAX_LOOP_SECONDS * sr
                {
                    break;
                }
                last = Some(e.sample);
                if e.velocity > 0 {
                    start = Some(e.sample);
                }
            }
            let Some(start) = start else {
                self.emit(EngineEvent::CaptureFailed(CaptureError::Nothing));
                return;
            };
            let seconds = (now - start) as f64 / sr;
            if seconds < MIN_LOOP_SECONDS {
                self.emit(EngineEvent::CaptureFailed(CaptureError::TooShort));
                return;
            }
            // Infer the tempo: the bar count whose BPM is nearest the target.
            let mut best = (4u32, f64::INFINITY);
            for bars in [1u32, 2, 4, 8] {
                let bpm = 60.0 * 4.0 * f64::from(bars) / seconds;
                let in_range = bpm >= f64::from(MIN_BPM) - 1e-9 && bpm <= f64::from(MAX_BPM) + 1e-9;
                if in_range && (bpm - TARGET_BPM).abs() < best.1 {
                    best = (bars, (bpm - TARGET_BPM).abs());
                }
            }
            let steps = best.0 * STEPS_PER_BAR as u32;
            let count = self.collect(start as f64, now as f64, |e| Some(e.sample as f64));
            if count == 0 {
                self.emit(EngineEvent::CaptureFailed(CaptureError::Nothing));
                return;
            }
            let bpm = (60.0 * 4.0 * f64::from(best.0) / seconds) as f32;
            self.has_loop = true;
            self.loop_steps = steps;
            self.transport.set_bpm(bpm);
            self.update_delay();
            self.restart_transport();
            self.emit(EngineEvent::LoopStarted {
                steps: steps as u16,
                bpm: self.transport.bpm(),
            });
            (count, start as f64, f64::from(steps) / (now - start) as f64)
        } else {
            if !self.transport.running {
                self.emit(EngineEvent::CaptureFailed(CaptureError::Nothing));
                return;
            }
            // The last pass of the loop.
            let to = self.unwrapped_pos();
            let from = to - f64::from(self.loop_steps);
            let epoch = self.epoch;
            let count = self.collect(from, to, |e| {
                (e.epoch == epoch && e.steps.is_finite()).then_some(e.steps)
            });
            if count == 0 {
                self.emit(EngineEvent::CaptureFailed(CaptureError::Nothing));
                return;
            }
            (count, 0.0, 1.0)
        };

        let steps = f64::from(self.loop_steps);
        let first_sound = self.cap[0].sound;
        let live_macros = self.macros[PART_LIVE as usize];
        let layer = &mut self.layers[slot];
        layer.len = 0;
        layer.sound = first_sound;
        layer.muted = false;
        layer.volume = 1.0;
        for n in &self.cap[..count] {
            let on = ((n.on - from) * scale).rem_euclid(steps);
            let length = ((n.off - n.on) * scale).clamp(0.05, steps - 0.05);
            let off = (on + length).rem_euclid(steps);
            layer.events[layer.len] = LoopEvent {
                t: on as f32,
                note: n.note,
                velocity: n.velocity,
            };
            layer.events[layer.len + 1] = LoopEvent {
                t: off as f32,
                note: n.note,
                velocity: 0,
            };
            layer.len += 2;
        }
        layer.sort();
        let pos = self.transport.pos() as f32;
        layer.cursor = layer.events[..layer.len]
            .iter()
            .position(|e| e.t >= pos)
            .unwrap_or(layer.len);
        layer.active = true;
        self.macros[PART_LAYER0 as usize + slot] = Macros {
            gain: 1.0,
            ..live_macros
        };
        self.loop_on = true;
        self.emit(EngineEvent::LayerCaptured {
            layer: slot as u8,
            sound: first_sound,
            notes: count as u16,
        });
        for i in 0..count {
            let n = self.cap[i];
            let on = ((n.on - from) * scale).rem_euclid(steps);
            let length = ((n.off - n.on) * scale).clamp(0.05, steps - 0.05);
            self.emit(EngineEvent::LayerNote {
                layer: slot as u8,
                note: n.note,
                velocity: n.velocity,
                on: on as f32,
                off: (on + length).rem_euclid(steps) as f32,
            });
        }
    }

    fn clear_layer(&mut self, index: usize) {
        if index >= LAYERS {
            return;
        }
        let layer = &mut self.layers[index];
        layer.active = false;
        layer.len = 0;
        layer.cursor = 0;
        layer.muted = false;
        self.release_part(PART_LAYER0 + index as u8);
    }

    // ------------------------------------------------------------------
    // Sequencing

    fn on_step(&mut self, step: i64) {
        self.shared.step.store(step as i32, Ordering::Relaxed);
        let in_bar = step.rem_euclid(STEPS_PER_BAR as i64) as usize;
        if step >= 0 && self.beat_on {
            for drum in 0..DRUMS {
                if self.patterns[drum] >> in_bar & 1 == 1 {
                    let velocity = if in_bar.is_multiple_of(4) { 112 } else { 96 };
                    self.note_on(
                        PART_DRUMS,
                        SoundId::from(self.drum_presets[drum]),
                        60,
                        velocity,
                    );
                }
            }
        }
        if self.metronome && in_bar.is_multiple_of(4) {
            let accent = in_bar == 0;
            self.click_inc = if accent { 1568.0 } else { 1046.5 } / self.sample_rate;
            self.click_phase = 0.0;
            self.click_env = if accent { 0.32 } else { 0.22 };
        }
    }

    /// Fire everything that is due at the current sample.
    fn run_due(&mut self) {
        if self.preview_off == 0 {
            self.key_up(PREVIEW_KEY);
        }
        if self.transport.running {
            loop {
                if self.transport.samples_until(f64::from(self.loop_steps)) == 0 {
                    self.transport.wrap(self.loop_steps);
                    self.wraps += 1;
                    self.next_step -= i64::from(self.loop_steps);
                    for l in self.layers.iter_mut() {
                        l.cursor = 0;
                    }
                    continue;
                }
                if self.transport.samples_until(self.next_step as f64) == 0 {
                    let step = self.next_step;
                    self.next_step += 1;
                    self.on_step(step);
                    continue;
                }
                break;
            }
            for i in 0..LAYERS {
                if !self.layers[i].active {
                    continue;
                }
                while self.layers[i].cursor < self.layers[i].len {
                    let e = self.layers[i].events[self.layers[i].cursor];
                    if self.transport.samples_until(f64::from(e.t)) != 0 {
                        break;
                    }
                    self.layers[i].cursor += 1;
                    let part = PART_LAYER0 + i as u8;
                    if e.velocity == 0 {
                        self.note_off(part, e.note);
                    } else if self.loop_on && !self.layers[i].muted {
                        self.note_on(part, self.layers[i].sound, e.note, e.velocity);
                    }
                }
            }
        }
        if self.arp.off == 0 {
            self.arp.off = NEVER;
            if let Some(note) = self.arp.note.take() {
                self.live_off(note);
            }
        }
        if self.arp.next == 0 {
            self.arp_step();
        }
    }

    /// Samples until the next scheduled event (at least 1).
    fn next_event_distance(&self) -> u64 {
        let mut d = self.preview_off.min(self.arp.next).min(self.arp.off);
        if self.transport.running {
            d = d.min(self.transport.samples_until(f64::from(self.loop_steps)));
            d = d.min(self.transport.samples_until(self.next_step as f64));
            for l in self.layers.iter() {
                if l.active && l.cursor < l.len {
                    d = d.min(
                        self.transport
                            .samples_until(f64::from(l.events[l.cursor].t)),
                    );
                }
            }
        }
        d.max(1)
    }

    fn advance(&mut self, n: usize) {
        let n64 = n as u64;
        for t in [&mut self.preview_off, &mut self.arp.next, &mut self.arp.off] {
            if *t != NEVER {
                *t = t.saturating_sub(n64);
            }
        }
        self.now += n64;
        if self.transport.running {
            self.transport.advance(n);
        }
    }

    // ------------------------------------------------------------------
    // Audio

    fn live_targets(&self) -> Macros {
        let s = &self.shared;
        Macros {
            bright: (s.pad_x.get().clamp(0.0, 1.0) - 0.5) * BRIGHT_OCTAVES,
            wobble: s.pad_y.get().clamp(0.0, 1.0),
            attack: attack_multiplier(s.attack.get()),
            release: release_multiplier(s.release.get()),
            reverb: s.reverb.get().clamp(0.0, 1.0),
            gain: 1.0,
        }
    }

    /// Render interleaved stereo audio into `out` (length = 2 * frames).
    /// Real-time safe: no allocation, locks or I/O.
    pub fn process(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let targets = self.live_targets();
        self.sf_controls();
        let volume_target = volume_gain(self.shared.volume.get());
        let volume_coef = smooth_coef(0.02, 1.0 / self.sample_rate);
        let click_decay = (-1.0 / (0.018 * self.sample_rate)).exp();

        let mut done = 0usize;
        while done < frames {
            self.run_due();
            let n = (frames - done)
                .min(MAX_BLOCK)
                .min(self.next_event_distance() as usize);

            // Smooth the live controls (pad and knobs) once per block.
            let coef = smooth_coef(0.015, n as f32 / self.sample_rate);
            let m = &mut self.macros[PART_LIVE as usize];
            glide(&mut m.bright, targets.bright, coef);
            glide(&mut m.wobble, targets.wobble, coef);
            glide(&mut m.attack, targets.attack, coef);
            glide(&mut m.release, targets.release, coef);
            glide(&mut m.reverb, targets.reverb, coef);

            for frame in self.buses.dry[..n]
                .iter_mut()
                .chain(self.buses.chorus[..n].iter_mut())
                .chain(self.buses.delay[..n].iter_mut())
                .chain(self.buses.reverb[..n].iter_mut())
            {
                *frame = [0.0; 2];
            }
            let mut ctx = BlockCtx {
                sample_rate: self.sample_rate,
                bpm: self.transport.bpm(),
                macros: Macros::NEUTRAL,
            };
            for v in self.voices.iter_mut() {
                if v.active {
                    ctx.macros = self.macros[(v.part as usize).min(PARTS - 1)];
                    v.render(n, &ctx, &mut self.buses, 0);
                }
            }
            if let Some(sf) = self.sf.as_mut() {
                sf.render(&mut self.sf_left[..n], &mut self.sf_right[..n]);
                for i in 0..n {
                    self.buses.dry[i][0] += self.sf_left[i] * SF_GAIN;
                    self.buses.dry[i][1] += self.sf_right[i] * SF_GAIN;
                }
            }

            for i in 0..n {
                let dry = self.buses.dry[i];
                let ch = self.chorus.process(self.buses.chorus[i]);
                let dl = self.delay.process(self.buses.delay[i]);
                let rv = self.reverb.process(self.buses.reverb[i]);
                let mut l = dry[0] + ch[0] * CHORUS_RETURN + dl[0] + rv[0] * REVERB_RETURN;
                let mut r = dry[1] + ch[1] * CHORUS_RETURN + dl[1] + rv[1] * REVERB_RETURN;
                if self.click_env > 1.0e-4 {
                    let s = (TAU * self.click_phase).sin() * self.click_env;
                    self.click_phase += self.click_inc;
                    if self.click_phase >= 1.0 {
                        self.click_phase -= 1.0;
                    }
                    self.click_env *= click_decay;
                    l += s;
                    r += s;
                }
                self.volume += (volume_target - self.volume) * volume_coef;
                let y = self.limiter.process([l * self.volume, r * self.volume]);
                out[(done + i) * 2] = y[0];
                out[(done + i) * 2 + 1] = y[1];
            }

            self.advance(n);
            done += n;
        }
        self.publish();
    }

    fn publish(&self) {
        let mut flags = 0u32;
        flags |= u32::from(self.transport.running);
        flags |= u32::from(self.beat_on) << 1;
        flags |= u32::from(self.loop_on) << 2;
        flags |= u32::from(self.metronome) << 3;
        flags |= u32::from(self.has_loop) << 4;
        for (i, l) in self.layers.iter().enumerate() {
            flags |= u32::from(l.active) << (8 + i);
            flags |= u32::from(l.muted) << (16 + i);
        }
        let s = &self.shared;
        s.flags.store(flags, Ordering::Relaxed);
        s.loop_steps.store(self.loop_steps, Ordering::Relaxed);
        s.pos.set(self.transport.pos() as f32);
        if !self.transport.running {
            s.step.store(-1, Ordering::Relaxed);
        }
        s.voices
            .store(self.active_voices() as u32, Ordering::Relaxed);
    }
}
