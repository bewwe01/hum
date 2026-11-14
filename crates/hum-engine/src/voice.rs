//! One synth voice: oscillators, FM, filter, envelopes, LFO, drive, crusher.
//!
//! A voice renders short blocks (at most [`MAX_BLOCK`] samples). Slow things
//! (LFO, pitch, filter cutoff, envelope coefficients) are evaluated once per
//! block and interpolated, so parameter changes never produce zipper noise.

use crate::dsp::{self, midi_to_hz, soft_sat, svf_g, svf_k, Env, EnvCoefs, Rng, Stage, Svf, TAU};
use crate::preset::{FilterMode, LfoDest, LfoShape, Preset, Wave};

/// Longest block a voice renders in one go (the control period).
pub const MAX_BLOCK: usize = 16;
pub const MAX_UNISON: usize = 7;
/// Phases: unison copies of layer 0, then layers 1 and 2.
const PHASES: usize = MAX_UNISON + 2;
/// Duration of the fade applied to a stolen voice.
const STEAL_FADE_SECONDS: f32 = 0.004;

/// Per-part performance controls (the Play screen's pad and knobs).
#[derive(Clone, Copy, Debug)]
pub struct Macros {
    /// Cutoff offset in octaves (pad X).
    pub bright: f32,
    /// 0..1 extra LFO depth (pad Y).
    pub wobble: f32,
    /// Multiplier for attack times.
    pub attack: f32,
    /// Multiplier for release times.
    pub release: f32,
    /// Reverb send 0..1; negative = use the preset's own default.
    pub reverb: f32,
    /// Part volume (loop layers), 1 = unchanged.
    pub gain: f32,
    /// Part pan, -1 (left) .. 1 (right).
    pub pan: f32,
}

impl Macros {
    pub const NEUTRAL: Macros = Macros {
        bright: 0.0,
        wobble: 0.0,
        attack: 1.0,
        release: 1.0,
        reverb: -1.0,
        gain: 1.0,
        pan: 0.0,
    };
}

/// Everything a voice needs to know about the outside world for one block.
#[derive(Clone, Copy, Debug)]
pub struct BlockCtx {
    pub sample_rate: f32,
    pub bpm: f32,
    pub macros: Macros,
}

/// Stereo scratch buses a voice mixes into.
#[derive(Clone, Copy, Debug)]
pub struct Buses {
    pub dry: [[f32; 2]; MAX_BLOCK],
    pub chorus: [[f32; 2]; MAX_BLOCK],
    pub delay: [[f32; 2]; MAX_BLOCK],
    pub reverb: [[f32; 2]; MAX_BLOCK],
}

impl Buses {
    pub const fn new() -> Self {
        Self {
            dry: [[0.0; 2]; MAX_BLOCK],
            chorus: [[0.0; 2]; MAX_BLOCK],
            delay: [[0.0; 2]; MAX_BLOCK],
            reverb: [[0.0; 2]; MAX_BLOCK],
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

impl Default for Buses {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    preset: &'static Preset,
    note: u8,
    velocity: f32,
    from_pitch: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Voice {
    pub active: bool,
    pub part: u8,
    pub note: u8,
    /// True while the key is held (before note-off).
    pub gate: bool,
    /// Allocation counter value at note-on; smaller = older.
    pub age: u64,

    preset: &'static Preset,
    velocity: f32,
    pitch: f32,
    target_pitch: f32,
    time: f32,

    phase: [f32; PHASES],
    ratio: [f32; PHASES],
    pan: [[f32; 2]; MAX_UNISON],
    unison: usize,
    sub_phase: f32,
    fm_phase: f32,
    rng: Rng,

    filt_a: [Svf; 2],
    filt_b: [Svf; 2],
    g: f32,
    amp: Env,
    fenv: Env,
    amp_gain: f32,

    lfo_phase: f32,
    lfo_from: f32,
    lfo_to: f32,

    click: f32,
    crush_phase: f32,
    crush_hold: [f32; 2],
    bursts_left: u8,
    next_burst: f32,

    /// 1 while playing normally; ramps to 0 when the voice is being stolen.
    fade: f32,
    /// Glide time set by a legato move (0 = use the preset's glide).
    glide_override: f32,
    fading: bool,
    pending: Option<Pending>,
}

impl Voice {
    pub fn new(seed: u32) -> Self {
        Self {
            active: false,
            part: 0,
            note: 0,
            gate: false,
            age: 0,
            preset: &Preset::BASE,
            velocity: 0.0,
            pitch: 60.0,
            target_pitch: 60.0,
            time: 0.0,
            phase: [0.0; PHASES],
            ratio: [1.0; PHASES],
            pan: [[0.707; 2]; MAX_UNISON],
            unison: 1,
            sub_phase: 0.0,
            fm_phase: 0.0,
            rng: Rng::new(seed),
            filt_a: [Svf::default(); 2],
            filt_b: [Svf::default(); 2],
            g: 0.1,
            amp: Env::default(),
            fenv: Env::default(),
            amp_gain: 1.0,
            lfo_phase: 0.0,
            lfo_from: 0.0,
            lfo_to: 0.0,
            click: 0.0,
            crush_phase: 0.0,
            crush_hold: [0.0; 2],
            bursts_left: 0,
            next_burst: 0.0,
            fade: 1.0,
            glide_override: 0.0,
            fading: false,
            pending: None,
        }
    }

    pub fn preset(&self) -> &'static Preset {
        self.preset
    }

    /// True when the voice has been released (or is a finished one-shot tail).
    pub fn is_releasing(&self) -> bool {
        self.active && !self.gate
    }

    /// Current pitch in (fractional) MIDI notes; used as the glide source.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// Start a note. If the voice is still sounding it is faded out quickly
    /// first (click-free stealing) and the new note starts right after.
    pub fn note_on(
        &mut self,
        preset: &'static Preset,
        part: u8,
        note: u8,
        velocity: f32,
        from_pitch: f32,
        age: u64,
    ) {
        let pending = Pending {
            preset,
            note,
            velocity,
            from_pitch,
        };
        // Book-keeping reflects the new note immediately so that a note-off
        // arriving during the fade is not lost.
        self.part = part;
        self.note = note;
        self.gate = true;
        self.age = age;
        if self.active {
            self.fading = true;
            self.pending = Some(pending);
        } else {
            self.start(pending);
        }
    }

    /// Legato: move this sounding voice to `note`, gliding over `glide`
    /// seconds, without restarting its envelopes.
    pub fn legato(&mut self, note: u8, glide: f32) {
        let p = self.preset;
        self.note = note;
        let key = f32::from(note);
        self.target_pitch = if p.fixed_note > 0.0 {
            p.fixed_note + (key - 60.0) * p.fixed_track
        } else {
            key
        };
        self.glide_override = glide;
    }

    pub fn note_off(&mut self) {
        self.gate = false;
        if self.pending.is_some() {
            // Released before the stolen voice finished fading: the new note
            // still starts, and is released right away (see `start`).
            return;
        }
        if !self.preset.one_shot {
            self.amp.release();
            self.fenv.release();
        }
    }

    /// Fade out fast and free the voice (used for "all notes off").
    pub fn kill(&mut self) {
        if self.active {
            self.fading = true;
            self.pending = None;
            self.gate = false;
        }
    }

    fn start(&mut self, p: Pending) {
        let preset = p.preset;
        self.active = true;
        self.preset = preset;
        self.velocity = p.velocity;
        self.time = 0.0;
        self.fade = 1.0;
        self.fading = false;
        self.pending = None;
        self.glide_override = 0.0;

        let key = f32::from(p.note);
        self.target_pitch = if preset.fixed_note > 0.0 {
            preset.fixed_note + (key - 60.0) * preset.fixed_track
        } else {
            key
        };
        self.pitch = if preset.glide > 0.0 {
            p.from_pitch
        } else {
            self.target_pitch
        };

        // Oscillator tuning ratios and start phases.
        let unison = usize::from(preset.unison).clamp(1, MAX_UNISON);
        self.unison = unison;
        let base0 = layer_ratio(preset, 0);
        for u in 0..MAX_UNISON {
            let pos = if unison > 1 {
                u as f32 / (unison - 1) as f32 * 2.0 - 1.0
            } else {
                0.0
            };
            let cents = pos * preset.spread * 0.5;
            self.ratio[u] = base0 * (cents / 1200.0).exp2();
            let pan = pos * preset.width.clamp(0.0, 1.0);
            let angle = (pan + 1.0) * 0.25 * core::f32::consts::PI;
            let norm = 1.0 / (unison as f32).sqrt();
            // Equal-power pan, scaled so a centred voice has unity gain.
            self.pan[u] = [
                angle.cos() * core::f32::consts::SQRT_2 * norm,
                angle.sin() * core::f32::consts::SQRT_2 * norm,
            ];
        }
        self.ratio[MAX_UNISON] = layer_ratio(preset, 1);
        self.ratio[MAX_UNISON + 1] = layer_ratio(preset, 2);
        for i in 0..PHASES {
            let layer = if i < MAX_UNISON {
                0
            } else {
                i - MAX_UNISON + 1
            };
            // Sines and triangles start at a zero crossing (clean attacks,
            // consistent drum hits); bright waves start at a random phase.
            self.phase[i] = match preset.osc[layer].wave {
                Wave::Sine => 0.0,
                Wave::Triangle => 0.25,
                _ => self.rng.unipolar(),
            };
        }
        if unison > 1 {
            for u in 0..unison {
                self.phase[u] = self.rng.unipolar();
            }
        }
        self.sub_phase = 0.0;
        self.fm_phase = 0.0;

        for f in self.filt_a.iter_mut().chain(self.filt_b.iter_mut()) {
            f.reset();
        }
        self.g = -1.0; // "not set yet": the first block jumps to its target
        self.amp.reset();
        self.amp.trigger();
        self.fenv.reset();
        self.fenv.trigger();
        self.amp_gain = -1.0;

        self.lfo_phase = 0.0;
        self.lfo_from = 0.0;
        self.lfo_to = self.rng.bipolar();
        self.click = preset.click;
        self.crush_phase = 1.0;
        self.crush_hold = [0.0; 2];
        self.bursts_left = preset.bursts;
        self.next_burst = preset.burst_gap;

        if !self.gate {
            self.note_off();
        }
    }

    fn lfo_value(&mut self, dt: f32, bpm: f32) -> f32 {
        let lfo = &self.preset.lfo;
        let rate = if lfo.sync_beats > 0.0 {
            bpm / 60.0 / lfo.sync_beats
        } else {
            lfo.rate
        };
        self.lfo_phase += rate * dt;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= self.lfo_phase.floor();
            self.lfo_from = self.lfo_to;
            self.lfo_to = self.rng.bipolar();
        }
        let t = self.lfo_phase;
        let v = match lfo.shape {
            LfoShape::Sine => (TAU * t).sin(),
            LfoShape::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
            LfoShape::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::Random => self.lfo_to,
            LfoShape::Wander => {
                let s = t * t * (3.0 - 2.0 * t);
                self.lfo_from + (self.lfo_to - self.lfo_from) * s
            }
        };
        if lfo.unipolar {
            0.5 - 0.5 * v
        } else {
            v
        }
    }

    /// Render `n` (<= [`MAX_BLOCK`]) samples, adding into `buses[offset..]`.
    pub fn render(&mut self, n: usize, ctx: &BlockCtx, buses: &mut Buses, offset: usize) {
        if !self.active || n == 0 {
            return;
        }
        debug_assert!(offset + n <= MAX_BLOCK);
        let p = self.preset;
        let sr = ctx.sample_rate;
        let inv_sr = 1.0 / sr;
        let dt = n as f32 * inv_sr;
        let m = &ctx.macros;

        // ---- control-rate work -------------------------------------------
        let glide = p.glide.max(self.glide_override);
        if glide > 0.0 {
            self.pitch += (self.target_pitch - self.pitch) * dsp::smooth_coef(glide * 0.33, dt);
        } else {
            self.pitch = self.target_pitch;
        }
        let pitch_env = if p.pitch_time > 0.0 && self.time < p.pitch_time {
            let x = 1.0 - self.time / p.pitch_time;
            p.pitch_env * x * x
        } else {
            0.0
        };

        let lfo = self.lfo_value(dt, ctx.bpm);
        let depth = p.lfo.depth + p.lfo.wobble * m.wobble.clamp(0.0, 1.0);
        let (lfo_pitch, lfo_filter, lfo_amp) = match p.lfo.dest {
            LfoDest::Pitch => (lfo * depth, 0.0, 1.0),
            LfoDest::Filter => (0.0, lfo * depth, 1.0),
            LfoDest::Amp => (0.0, 0.0, 1.0 - depth.clamp(0.0, 1.0) * (0.5 - 0.5 * lfo)),
        };

        let note = self.pitch + pitch_env + lfo_pitch;
        let f0 = midi_to_hz(note).min(sr * 0.45);
        let inc0 = f0 * inv_sr;

        // Layer levels (with optional per-layer decay).
        let mut level = [0.0f32; 3];
        for (l, lvl) in level.iter_mut().enumerate() {
            let o = &p.osc[l];
            *lvl = if o.decay > 0.0 {
                o.level * (-6.9 * self.time / o.decay).exp()
            } else {
                o.level
            };
        }
        let fm_on = p.fm_ratio > 0.0 && p.fm_index > 0.0;
        let fm_index = if !fm_on {
            0.0
        } else if p.fm_decay > 0.0 {
            p.fm_index * (-6.9 * self.time / p.fm_decay).exp()
        } else {
            p.fm_index
        };
        let fm_inc = (inc0 * p.fm_ratio).min(0.45);

        // Filter.
        let attack = m.attack.max(0.01);
        let release = m.release.max(0.01);
        let fc = EnvCoefs::new(
            p.fenv.a * attack,
            p.fenv.d,
            p.fenv.s,
            p.fenv.r * release,
            dt,
        );
        let fenv = self.fenv.next(&fc);
        let key_oct = (self.pitch - 60.0) / 12.0 * p.filter.key_track;
        let cutoff =
            p.filter.cutoff * (m.bright + key_oct + p.filter.env * fenv + lfo_filter).exp2();
        let g_target = svf_g(cutoff, sr);
        if self.g < 0.0 {
            self.g = g_target;
        }
        let dg = (g_target - self.g) / n as f32;
        let k = svf_k(p.filter.resonance);
        let g2_ratio = if p.filter.mode == FilterMode::Formant {
            p.filter.formant_ratio
        } else {
            1.0
        };

        // Amp envelope coefficients. One-shots have no sustain, so the
        // RELEASE knob stretches their decay instead.
        let (decay, rel) = if p.one_shot {
            (p.amp.d * release, p.amp.r)
        } else {
            (p.amp.d, p.amp.r * release)
        };
        let in_bursts = self.bursts_left > 0;
        let ac = EnvCoefs::new(
            p.amp.a * attack,
            if in_bursts { p.burst_gap * 0.9 } else { decay },
            p.amp.s,
            rel,
            inv_sr,
        );
        if in_bursts && self.time >= self.next_burst {
            self.bursts_left -= 1;
            self.next_burst += p.burst_gap;
            self.amp.trigger();
        }

        let amp_target = lfo_amp * self.velocity * p.gain * m.gain.clamp(0.0, 2.0);
        if self.amp_gain < 0.0 {
            self.amp_gain = amp_target;
        }
        let d_amp = (amp_target - self.amp_gain) / n as f32;

        let drive_gain = 1.0 + p.drive * 7.0;
        let drive_comp = 1.0 / (1.0 + p.drive * 2.5);
        let crush_inc = if p.crush_rate > 0.0 {
            (p.crush_rate * inv_sr).min(1.0)
        } else {
            1.0
        };
        let crush_q = if p.crush_bits > 0.0 {
            (p.crush_bits - 1.0).exp2()
        } else {
            0.0
        };
        let fade_step = inv_sr / STEAL_FADE_SECONDS;
        let click_coef = (-inv_sr / 0.0008).exp();

        let reverb_send = if m.reverb >= 0.0 { m.reverb } else { p.reverb }.clamp(0.0, 1.0);
        // Equal-power part pan, unity gain in the centre.
        let pan_angle = (m.pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * core::f32::consts::PI;
        let (pan_l, pan_r) = if m.pan == 0.0 {
            (1.0, 1.0)
        } else {
            (
                pan_angle.cos() * core::f32::consts::SQRT_2,
                pan_angle.sin() * core::f32::consts::SQRT_2,
            )
        };
        let unison = self.unison;
        let w0 = p.osc[0].wave;
        let w1 = p.osc[1].wave;
        let w2 = p.osc[2].wave;
        let width0 = p.osc[0].width;
        let on1 = p.osc[1].level > 0.0 || p.ring > 0.0;
        let on2 = p.osc[2].level > 0.0;

        // ---- audio-rate loop ---------------------------------------------
        for i in 0..n {
            let pm = if fm_on {
                let v = (TAU * self.fm_phase).sin() * fm_index;
                self.fm_phase += fm_inc;
                if self.fm_phase >= 1.0 {
                    self.fm_phase -= 1.0;
                }
                v
            } else {
                0.0
            };

            // Layer 0 (with unison).
            let mut l = 0.0f32;
            let mut r = 0.0f32;
            for u in 0..unison {
                let inc = (inc0 * self.ratio[u]).min(0.45);
                let s = osc(w0, &mut self.phase[u], inc, pm, width0, &mut self.rng);
                l += s * self.pan[u][0];
                r += s * self.pan[u][1];
            }
            let o0 = (l + r) * 0.5;
            l *= level[0];
            r *= level[0];

            // Layers 1 and 2, sub oscillator, ring modulation.
            let mut c = 0.0f32;
            if on1 {
                let inc = (inc0 * self.ratio[MAX_UNISON]).min(0.45);
                let s = osc(
                    w1,
                    &mut self.phase[MAX_UNISON],
                    inc,
                    0.0,
                    p.osc[1].width,
                    &mut self.rng,
                );
                c += s * level[1] + s * o0 * p.ring;
            }
            if on2 {
                let inc = (inc0 * self.ratio[MAX_UNISON + 1]).min(0.45);
                let s = osc(
                    w2,
                    &mut self.phase[MAX_UNISON + 1],
                    inc,
                    0.0,
                    p.osc[2].width,
                    &mut self.rng,
                );
                c += s * level[2];
            }
            if p.sub > 0.0 {
                c += (TAU * self.sub_phase).sin() * p.sub;
                self.sub_phase += inc0 * 0.5;
                if self.sub_phase >= 1.0 {
                    self.sub_phase -= 1.0;
                }
            }
            l += c;
            r += c;

            if p.drive > 0.0 {
                l = soft_sat(l * drive_gain) * drive_comp;
                r = soft_sat(r * drive_gain) * drive_comp;
            }

            // Filter.
            self.g += dg;
            let g = self.g;
            let mut out = [l, r];
            for (ch, x) in out.iter_mut().enumerate() {
                let a = self.filt_a[ch].step(*x, g, k);
                *x = match p.filter.mode {
                    FilterMode::LowPass => {
                        if p.filter.four_pole {
                            self.filt_b[ch].step(a.lp, g, core::f32::consts::SQRT_2).lp
                        } else {
                            a.lp
                        }
                    }
                    FilterMode::HighPass => {
                        if p.filter.four_pole {
                            self.filt_b[ch].step(a.hp, g, core::f32::consts::SQRT_2).hp
                        } else {
                            a.hp
                        }
                    }
                    FilterMode::BandPass => a.bp * k * 2.0,
                    FilterMode::Formant => {
                        let g2 = (g * g2_ratio).min(3.0);
                        let b = self.filt_b[ch].step(*x, g2, k);
                        (a.bp + b.bp * 0.7) * k * 2.0 + a.lp * 0.1
                    }
                };
            }

            if self.click > 1.0e-4 {
                let s = self.rng.bipolar() * self.click;
                out[0] += s;
                out[1] += s;
                self.click *= click_coef;
            }

            // Bit-crusher: sample-and-hold, then quantise.
            if crush_inc < 1.0 {
                self.crush_phase += crush_inc;
                if self.crush_phase >= 1.0 {
                    self.crush_phase -= 1.0;
                    self.crush_hold = out;
                }
                out = self.crush_hold;
            }
            if crush_q > 0.0 {
                out[0] = (out[0] * crush_q).round() / crush_q;
                out[1] = (out[1] * crush_q).round() / crush_q;
            }

            // Amplitude.
            self.amp_gain += d_amp;
            let mut a = self.amp.next(&ac) * self.amp_gain;
            if self.fading {
                self.fade -= fade_step;
                if self.fade <= 0.0 {
                    self.fade = 0.0;
                }
                a *= self.fade;
            }
            let yl = out[0] * a * pan_l;
            let yr = out[1] * a * pan_r;

            let j = offset + i;
            buses.dry[j][0] += yl;
            buses.dry[j][1] += yr;
            buses.chorus[j][0] += yl * p.chorus;
            buses.chorus[j][1] += yr * p.chorus;
            buses.delay[j][0] += yl * p.delay;
            buses.delay[j][1] += yr * p.delay;
            buses.reverb[j][0] += yl * reverb_send;
            buses.reverb[j][1] += yr * reverb_send;
        }

        // ---- end of block -------------------------------------------------
        self.time += dt;
        for f in self.filt_a.iter_mut().chain(self.filt_b.iter_mut()) {
            f.flush();
        }

        if self.fading && self.fade <= 0.0 {
            match self.pending.take() {
                Some(pending) => self.start(pending),
                None => self.free(),
            }
            return;
        }
        let finished = match self.amp.stage {
            Stage::Idle => true,
            // A sustain level of zero means the sound has fully decayed.
            Stage::Sustain => p.amp.s <= 0.0 && self.bursts_left == 0,
            _ => false,
        };
        if finished && self.click <= 1.0e-4 {
            self.free();
        }
    }

    fn free(&mut self) {
        self.active = false;
        self.gate = false;
        self.fading = false;
        self.pending = None;
        self.amp.reset();
        self.fenv.reset();
    }
}

fn layer_ratio(p: &Preset, layer: usize) -> f32 {
    let o = &p.osc[layer];
    (f32::from(o.octave) + o.detune / 1200.0).exp2() * o.ratio
}

/// One oscillator sample; advances `phase` by `inc`. `pm` is phase
/// modulation in cycles.
#[inline]
fn osc(wave: Wave, phase: &mut f32, inc: f32, pm: f32, width: f32, rng: &mut Rng) -> f32 {
    let t = *phase;
    *phase += inc;
    if *phase >= 1.0 {
        *phase -= 1.0;
    }
    if pm != 0.0 {
        let tm = t + pm;
        let tm = tm - tm.floor();
        return match wave {
            Wave::Sine => dsp::sine(tm),
            Wave::Triangle => dsp::triangle(tm),
            Wave::Saw => 2.0 * tm - 1.0,
            Wave::Square => {
                if tm < width {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Noise => rng.bipolar(),
        };
    }
    match wave {
        Wave::Sine => dsp::sine(t),
        Wave::Triangle => dsp::triangle(t),
        Wave::Saw => dsp::saw(t, inc),
        Wave::Square => dsp::pulse(t, inc, width),
        Wave::Noise => rng.bipolar(),
    }
}
