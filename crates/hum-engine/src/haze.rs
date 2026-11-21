//! "Haze": effects on the whole mix. Distortion, a bit-crusher, a worn
//! tape (wobble, hiss and crackle), a shimmer for the reverb (an octave-up
//! pitch shifter in its feedback path) and a beat-synced "pump" (the mix
//! ducks on every beat, like sidechain compression).
//!
//! Every amount is 0..1 and 0 leaves the signal untouched. All buffers are
//! allocated in `new`; `process` is real-time safe.

// Stereo loops index several per-channel arrays at once.
#![allow(clippy::needless_range_loop)]

use crate::dsp::flush;

/// The five Haze amounts, 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HazeAmounts {
    pub drive: f32,
    pub crush: f32,
    pub tape: f32,
    pub shimmer: f32,
    pub pump: f32,
}

impl HazeAmounts {
    /// "Haunted": the preset that sets them all at once.
    pub const HAUNTED: HazeAmounts = HazeAmounts {
        drive: 0.35,
        crush: 0.2,
        tape: 0.5,
        shimmer: 0.6,
        pump: 0.3,
    };

    pub fn is_off(&self) -> bool {
        self.drive <= 0.0
            && self.crush <= 0.0
            && self.tape <= 0.0
            && self.shimmer <= 0.0
            && self.pump <= 0.0
    }
}

/// Base delay of the tape path (seconds) and its wobble depths.
const TAPE_DELAY: f32 = 0.008;
const WOW_DEPTH: f32 = 0.0035;
const WOW_HZ: f32 = 0.55;
const FLUTTER_DEPTH: f32 = 0.00025;
const FLUTTER_HZ: f32 = 6.5;

/// Octave-up pitch shifter window (seconds).
const SHIFT_WINDOW: f32 = 0.045;

pub struct Haze {
    sample_rate: f32,
    rng: u32,

    // Distortion tone filter.
    tone: [f32; 2],

    // Bit-crusher sample-and-hold.
    held: [f32; 2],
    hold_phase: f32,

    // Tape.
    tape_buf: [Vec<f32>; 2],
    tape_pos: usize,
    wow: f32,
    flutter: f32,
    tape_mix: f32,
    tape_lp: [f32; 2],
    hiss_lp: f32,
    crackle: f32,

    // Shimmer pitch shifter.
    shift_buf: [Vec<f32>; 2],
    shift_pos: usize,
    shift_phase: f32,

    // Pump gain (smoothed).
    pump_gain: f32,
}

impl Haze {
    pub fn new(sample_rate: f32) -> Self {
        let tape_len = ((TAPE_DELAY + WOW_DEPTH + FLUTTER_DEPTH) * sample_rate) as usize + 8;
        let shift_len = (SHIFT_WINDOW * sample_rate) as usize + 8;
        Self {
            sample_rate,
            rng: 0x1234_5678,
            tone: [0.0; 2],
            held: [0.0; 2],
            hold_phase: 0.0,
            tape_buf: [vec![0.0; tape_len], vec![0.0; tape_len]],
            tape_pos: 0,
            wow: 0.0,
            flutter: 0.0,
            tape_mix: 0.0,
            tape_lp: [0.0; 2],
            hiss_lp: 0.0,
            crackle: 0.0,
            shift_buf: [vec![0.0; shift_len], vec![0.0; shift_len]],
            shift_pos: 0,
            shift_phase: 0.0,
            pump_gain: 1.0,
        }
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Octave-up copy of the reverb's output, fed back into its input.
    #[inline]
    pub fn shimmer(&mut self, input: [f32; 2], amount: f32) -> [f32; 2] {
        let len = self.shift_buf[0].len();
        for (ch, x) in input.iter().enumerate() {
            self.shift_buf[ch][self.shift_pos] = flush(*x);
        }
        if amount <= 0.0 {
            self.shift_pos = (self.shift_pos + 1) % len;
            return [0.0; 2];
        }
        // Two read heads sweep through the window twice as fast as the
        // write head, crossfaded with a sine window.
        let window = (len - 4) as f32;
        let mut out = [0.0f32; 2];
        for k in 0..2 {
            let phase = (self.shift_phase + 0.5 * k as f32).fract();
            let delay = window * (1.0 - phase);
            let gain = (std::f32::consts::PI * phase).sin();
            let read = self.shift_pos as f32 + len as f32 - delay;
            let i0 = read as usize % len;
            let i1 = (i0 + 1) % len;
            let frac = read.fract();
            for (ch, o) in out.iter_mut().enumerate() {
                let b = &self.shift_buf[ch];
                *o += (b[i0] + (b[i1] - b[i0]) * frac) * gain;
            }
        }
        self.shift_phase = (self.shift_phase + 1.0 / window).fract();
        self.shift_pos = (self.shift_pos + 1) % len;
        [out[0] * 0.7, out[1] * 0.7]
    }

    /// Process one frame of the mix. `beat` is the position within the
    /// current beat (0..1), for the pump.
    #[inline]
    pub fn process(&mut self, input: [f32; 2], a: &HazeAmounts, beat: f32) -> [f32; 2] {
        let mut x = input;

        // ---- distortion ----------------------------------------------------
        if a.drive > 0.0 {
            let pre = 1.0 + a.drive * 24.0;
            let comp = 1.0 / (1.0 + a.drive * 4.0);
            let mix = (a.drive * 4.0).min(1.0);
            // The tone darkens as the drive goes up (tames the fizz).
            let cutoff = 12_000.0 - 7_000.0 * a.drive;
            let k = 1.0 - (-std::f32::consts::TAU * cutoff / self.sample_rate).exp();
            for ch in 0..2 {
                let shaped = (x[ch] * pre).tanh() * comp;
                self.tone[ch] = flush(self.tone[ch] + (shaped - self.tone[ch]) * k);
                x[ch] += (self.tone[ch] - x[ch]) * mix;
            }
        }

        // ---- bit-crusher -----------------------------------------------------
        if a.crush > 0.0 {
            let bits = 16.0 - a.crush * 12.0;
            let steps = 2.0f32.powf(bits - 1.0);
            // Hold each sample for up to ~10 samples (lower sample rate).
            let hold = 1.0 + a.crush * a.crush * 9.0;
            self.hold_phase += 1.0;
            if self.hold_phase >= hold {
                self.hold_phase -= hold;
                for ch in 0..2 {
                    self.held[ch] = (x[ch] * steps).round() / steps;
                }
            }
            let mix = (a.crush * 4.0).min(1.0);
            for ch in 0..2 {
                x[ch] += (self.held[ch] - x[ch]) * mix;
            }
        }

        // ---- tape --------------------------------------------------------------
        let len = self.tape_buf[0].len();
        for ch in 0..2 {
            self.tape_buf[ch][self.tape_pos] = flush(x[ch]);
        }
        let target = if a.tape > 0.0 { 1.0 } else { 0.0 };
        self.tape_mix += (target - self.tape_mix) * (1.0 / (0.03 * self.sample_rate));
        if self.tape_mix > 1.0e-4 {
            let sr = self.sample_rate;
            self.wow = (self.wow + WOW_HZ / sr).fract();
            self.flutter = (self.flutter + FLUTTER_HZ / sr).fract();
            let tau = std::f32::consts::TAU;
            let delay = (TAPE_DELAY
                + a.tape * WOW_DEPTH * (tau * self.wow).sin()
                + a.tape * FLUTTER_DEPTH * (tau * self.flutter).sin())
                * sr;
            let read = self.tape_pos as f32 + len as f32 - delay;
            let i0 = read as usize % len;
            let i1 = (i0 + 1) % len;
            let frac = read.fract();
            // Worn tape loses its top end.
            let cutoff = 16_000.0 - 9_000.0 * a.tape;
            let k = 1.0 - (-tau * cutoff / sr).exp();
            let hiss = {
                let n = self.noise();
                self.hiss_lp += (n - self.hiss_lp) * 0.35;
                (n - self.hiss_lp) * 0.006 * a.tape
            };
            // Sparse crackle: a few random clicks per second.
            if self.noise().abs() > 1.0 - 4.0 * a.tape / sr {
                self.crackle = 0.12 * a.tape * self.noise().signum();
            }
            let pop = self.crackle;
            self.crackle *= 0.6;
            for ch in 0..2 {
                let b = &self.tape_buf[ch];
                let wet = b[i0] + (b[i1] - b[i0]) * frac;
                self.tape_lp[ch] = flush(self.tape_lp[ch] + (wet - self.tape_lp[ch]) * k);
                let tape = self.tape_lp[ch] + hiss + pop;
                x[ch] += (tape - x[ch]) * self.tape_mix;
            }
        }
        self.tape_pos = (self.tape_pos + 1) % len;

        // ---- pump --------------------------------------------------------------
        let duck = 1.0 - beat.clamp(0.0, 1.0);
        let target = 1.0 - a.pump * 0.75 * duck * duck;
        // ~3 ms smoothing: no click when the beat comes round.
        self.pump_gain += (target - self.pump_gain) * (1.0 / (0.003 * self.sample_rate));
        [x[0] * self.pump_gain, x[1] * self.pump_gain]
    }
}

// ---------------------------------------------------------------------------
// Stutter and tape stop

/// Seconds of the mix kept for the tape stop.
const HISTORY: f32 = 2.0;
/// Longest stutter slice (a quarter note at 60 BPM).
const MAX_SLICE: f32 = 1.0;
/// How long the tape takes to wind down.
const STOP_TIME: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Live,
    /// Repeating `len` samples of `slice`.
    Stutter {
        len: usize,
        pos: usize,
    },
    /// Reading the history at a falling speed.
    TapeStop {
        read: f64,
        rate: f64,
    },
}

/// Replaces the mix with a repeating slice (stutter) or a slowing copy of
/// it (tape stop) while the engine keeps time underneath.
pub struct Glitch {
    sample_rate: f32,
    history: [Vec<f32>; 2],
    write: usize,
    slice: [Vec<f32>; 2],
    mode: Mode,
    /// 0 = live, 1 = glitched (smoothed, for click-free changes).
    mix: f32,
    /// Last glitched frame, held while fading back to live.
    last: [f32; 2],
}

impl Glitch {
    pub fn new(sample_rate: f32) -> Self {
        let h = (HISTORY * sample_rate) as usize;
        let s = (MAX_SLICE * sample_rate) as usize;
        Self {
            sample_rate,
            history: [vec![0.0; h], vec![0.0; h]],
            write: 0,
            slice: [vec![0.0; s], vec![0.0; s]],
            mode: Mode::Live,
            mix: 0.0,
            last: [0.0; 2],
        }
    }

    pub fn is_live(&self) -> bool {
        self.mode == Mode::Live && self.mix <= 0.0
    }

    /// Start repeating the last `seconds` of the mix (0 stops).
    pub fn stutter(&mut self, seconds: f32) {
        if seconds <= 0.0 {
            if matches!(self.mode, Mode::Stutter { .. }) {
                self.mode = Mode::Live;
            }
            return;
        }
        let len = ((seconds * self.sample_rate) as usize).clamp(64, self.slice[0].len());
        let h = self.history[0].len();
        for ch in 0..2 {
            for i in 0..len {
                self.slice[ch][i] = self.history[ch][(self.write + h - len + i) % h];
            }
        }
        self.mode = Mode::Stutter { len, pos: 0 };
    }

    pub fn tape_stop(&mut self, on: bool) {
        if on {
            let h = self.history[0].len();
            self.mode = Mode::TapeStop {
                read: ((self.write + h - 1) % h) as f64,
                rate: 1.0,
            };
        } else if matches!(self.mode, Mode::TapeStop { .. }) {
            self.mode = Mode::Live;
        }
    }

    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let h = self.history[0].len();
        self.history[0][self.write] = input[0];
        self.history[1][self.write] = input[1];
        self.write = (self.write + 1) % h;

        let fade = 1.0 / (0.004 * self.sample_rate);
        let wet = match &mut self.mode {
            Mode::Live => None,
            Mode::Stutter { len, pos } => {
                // A 2 ms fade at both ends of the slice keeps it click-free.
                let edge = (0.002 * self.sample_rate).max(1.0);
                let i = *pos;
                let g = (i as f32 / edge).min((*len - i) as f32 / edge).min(1.0);
                let out = [self.slice[0][i] * g, self.slice[1][i] * g];
                *pos = (i + 1) % *len;
                Some(out)
            }
            Mode::TapeStop { read, rate } => {
                let r = *read;
                let i0 = r as usize % h;
                let i1 = (i0 + 1) % h;
                let frac = r.fract() as f32;
                let g = (*rate as f32).sqrt();
                let out = [
                    (self.history[0][i0] + (self.history[0][i1] - self.history[0][i0]) * frac) * g,
                    (self.history[1][i0] + (self.history[1][i1] - self.history[1][i0]) * frac) * g,
                ];
                *read = (r + *rate) % h as f64;
                *rate = (*rate - 1.0 / f64::from(STOP_TIME * self.sample_rate)).max(0.0);
                Some(out)
            }
        };
        match wet {
            Some(w) => {
                self.mix = (self.mix + fade).min(1.0);
                self.last = w;
            }
            None => {
                self.mix = (self.mix - fade).max(0.0);
                self.last = [self.last[0] * 0.999, self.last[1] * 0.999];
            }
        }
        let w = self.last;
        [
            input[0] + (w[0] - input[0]) * self.mix,
            input[1] + (w[1] - input[1]) * self.mix,
        ]
    }
}
