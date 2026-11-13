//! Shared effect bus: chorus, tempo-aware stereo delay, Freeverb-style
//! reverb and the master soft limiter.
//!
//! All buffers are allocated once in the constructors; processing never
//! allocates.

use crate::dsp::{flush, TAU};

// ---------------------------------------------------------------------------
// Chorus

pub struct Chorus {
    buf: [Vec<f32>; 2],
    pos: usize,
    phase: f32,
    sample_rate: f32,
}

impl Chorus {
    const BASE_SECONDS: f32 = 0.014;
    const DEPTH_SECONDS: f32 = 0.004;
    const RATE_HZ: f32 = 0.63;

    pub fn new(sample_rate: f32) -> Self {
        let len = (sample_rate * 0.05) as usize + 8;
        Self {
            buf: [vec![0.0; len], vec![0.0; len]],
            pos: 0,
            phase: 0.0,
            sample_rate,
        }
    }

    /// Returns the wet signal for one input frame.
    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let len = self.buf[0].len();
        self.buf[0][self.pos] = input[0];
        self.buf[1][self.pos] = input[1];
        self.phase += Self::RATE_HZ / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let mut out = [0.0f32; 2];
        for (ch, o) in out.iter_mut().enumerate() {
            // The two channels are modulated in quadrature for width.
            let lfo = (TAU * (self.phase + 0.25 * ch as f32)).sin();
            let delay = (Self::BASE_SECONDS + Self::DEPTH_SECONDS * lfo) * self.sample_rate;
            let read = self.pos as f32 + len as f32 - delay;
            let i0 = read as usize;
            let frac = read - i0 as f32;
            let a = self.buf[ch][i0 % len];
            let b = self.buf[ch][(i0 + 1) % len];
            *o = a + (b - a) * frac;
        }
        self.pos = (self.pos + 1) % len;
        out
    }
}

// ---------------------------------------------------------------------------
// Stereo (ping-pong) delay

pub struct Delay {
    buf: [Vec<f32>; 2],
    pos: usize,
    /// Current (smoothed) delay length in samples.
    length: f32,
    target: f32,
    feedback: f32,
    damp: [f32; 2],
    sample_rate: f32,
}

impl Delay {
    const MAX_SECONDS: f32 = 1.5;

    pub fn new(sample_rate: f32) -> Self {
        let len = (sample_rate * Self::MAX_SECONDS) as usize + 8;
        let initial = sample_rate * 0.3;
        Self {
            buf: [vec![0.0; len], vec![0.0; len]],
            pos: 0,
            length: initial,
            target: initial,
            feedback: 0.35,
            damp: [0.0; 2],
            sample_rate,
        }
    }

    /// Set the delay time from a note length in beats at the given tempo.
    pub fn set(&mut self, beats: f32, bpm: f32, feedback: f32) {
        let seconds = (beats.max(0.05) * 60.0 / bpm.max(20.0)).min(Self::MAX_SECONDS - 0.01);
        self.target = seconds * self.sample_rate;
        self.feedback = feedback.clamp(0.0, 0.85);
    }

    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let len = self.buf[0].len();
        // Glide to a new delay time instead of jumping (no clicks).
        self.length += (self.target - self.length) * 0.0002;
        let read = self.pos as f32 + len as f32 - self.length;
        let i0 = read as usize;
        let frac = read - i0 as f32;
        let mut out = [0.0f32; 2];
        for (ch, o) in out.iter_mut().enumerate() {
            let a = self.buf[ch][i0 % len];
            let b = self.buf[ch][(i0 + 1) % len];
            *o = a + (b - a) * frac;
        }
        // Feedback is low-passed and crosses channels (ping-pong).
        let mono_in = (input[0] + input[1]) * 0.5;
        self.damp[0] = flush(self.damp[0] + (out[1] - self.damp[0]) * 0.45);
        self.damp[1] = flush(self.damp[1] + (out[0] - self.damp[1]) * 0.45);
        self.buf[0][self.pos] = mono_in + self.damp[0] * self.feedback;
        self.buf[1][self.pos] = self.damp[1] * self.feedback;
        self.pos = (self.pos + 1) % len;
        out
    }
}

// ---------------------------------------------------------------------------
// Reverb (Freeverb topology: 8 parallel combs into 4 series all-passes)

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
            store: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = flush(out * (1.0 - damp) + self.store * damp);
        self.buf[self.pos] = input + self.store * feedback;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buf[self.pos];
        let out = buffered - input;
        self.buf[self.pos] = flush(input + buffered * 0.5);
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    feedback: f32,
    damp: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let make = |spread: usize| {
            (
                COMB_TUNING
                    .iter()
                    .map(|&t| Comb::new(((t + spread) as f32 * scale) as usize))
                    .collect::<Vec<_>>(),
                ALLPASS_TUNING
                    .iter()
                    .map(|&t| Allpass::new(((t + spread) as f32 * scale) as usize))
                    .collect::<Vec<_>>(),
            )
        };
        let (cl, al) = make(0);
        let (cr, ar) = make(STEREO_SPREAD);
        Self {
            combs: [cl, cr],
            allpasses: [al, ar],
            feedback: 0.86,
            damp: 0.3,
        }
    }

    /// Returns the wet signal for one input frame.
    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mono = (input[0] + input[1]) * 0.015;
        let mut out = [0.0f32; 2];
        for (ch, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            for comb in self.combs[ch].iter_mut() {
                acc += comb.process(mono, self.feedback, self.damp);
            }
            for ap in self.allpasses[ch].iter_mut() {
                acc = ap.process(acc);
            }
            *o = acc;
        }
        // A little cross-feed keeps the tail centred but wide.
        [out[0] * 0.85 + out[1] * 0.15, out[1] * 0.85 + out[0] * 0.15]
    }
}

// ---------------------------------------------------------------------------
// Bus compressor

/// A gentle stereo-linked RMS compressor in front of the limiter: as loop
/// layers stack up it evens out the level ("glue") so the limiter rarely
/// has to work hard.
pub struct Compressor {
    env: f32,
    attack: f32,
    release: f32,
}

impl Compressor {
    /// Threshold (linear, about -12 dBFS) and ratio.
    const THRESHOLD: f32 = 0.25;
    const RATIO: f32 = 2.0;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            env: 0.0,
            attack: 1.0 - (-1.0 / (0.010 * sample_rate)).exp(),
            release: 1.0 - (-1.0 / (0.250 * sample_rate)).exp(),
        }
    }

    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let power = 0.5 * (input[0] * input[0] + input[1] * input[1]);
        let power = if power.is_finite() { power } else { 0.0 };
        let coef = if power > self.env {
            self.attack
        } else {
            self.release
        };
        self.env = flush(self.env + (power - self.env) * coef);
        let level = self.env.sqrt();
        if level <= Self::THRESHOLD {
            return input;
        }
        let gain = (Self::THRESHOLD / level).powf(1.0 - 1.0 / Self::RATIO);
        [input[0] * gain, input[1] * gain]
    }
}

// ---------------------------------------------------------------------------
// Master soft limiter

/// Level where the limiter starts to act.
const KNEE: f32 = 0.5;
/// The output can never exceed `KNEE + RANGE` (< 1.0 = 0 dBFS).
const RANGE: f32 = 0.45;

/// Stereo-linked soft limiter. The gain is derived from an instant-attack
/// peak follower, so the output is mathematically bounded: because the
/// follower is never below the current sample's magnitude, `|out| <=
/// curve(env) < KNEE + RANGE = 0.95`.
pub struct Limiter {
    env: f32,
    release: f32,
}

impl Limiter {
    pub const CEILING: f32 = KNEE + RANGE;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            env: 0.0,
            release: (-1.0 / (0.12 * sample_rate)).exp(),
        }
    }

    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let l = if input[0].is_finite() { input[0] } else { 0.0 };
        let r = if input[1].is_finite() { input[1] } else { 0.0 };
        let peak = l.abs().max(r.abs());
        self.env = if peak > self.env {
            peak
        } else {
            flush(peak + (self.env - peak) * self.release)
        };
        if self.env <= KNEE {
            return [l, r];
        }
        let target = KNEE + RANGE * ((self.env - KNEE) / RANGE).tanh();
        let gain = target / self.env;
        // The clamp only guards against floating-point rounding.
        [
            (l * gain).clamp(-Self::CEILING, Self::CEILING),
            (r * gain).clamp(-Self::CEILING, Self::CEILING),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::Rng;

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut lim = Limiter::new(48_000.0);
        let mut rng = Rng::new(3);
        for i in 0..200_000 {
            let gain = match (i / 5_000) % 4 {
                0 => 0.1,
                1 => 1.0,
                2 => 30.0,
                _ => 1.0e6,
            };
            let out = lim.process([rng.bipolar() * gain, rng.bipolar() * gain]);
            assert!(out[0].abs() <= Limiter::CEILING && out[1].abs() <= Limiter::CEILING);
        }
        let out = lim.process([f32::NAN, f32::INFINITY]);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn limiter_is_transparent_for_quiet_signals() {
        let mut lim = Limiter::new(48_000.0);
        for i in 0..1000 {
            let x = (i as f32 * 0.05).sin() * 0.4;
            assert_eq!(lim.process([x, -x]), [x, -x]);
        }
    }

    #[test]
    fn effects_are_stable_and_decay_to_silence() {
        let sr = 48_000.0;
        let mut chorus = Chorus::new(sr);
        let mut delay = Delay::new(sr);
        let mut reverb = Reverb::new(sr);
        delay.set(0.5, 110.0, 0.85);
        let mut rng = Rng::new(11);
        let mut peak = 0.0f32;
        // One second of loud noise, then 20 seconds of silence.
        for i in 0..(21 * 48_000) {
            let x = if i < 48_000 {
                [rng.bipolar(), rng.bipolar()]
            } else {
                [0.0, 0.0]
            };
            let c = chorus.process(x);
            let d = delay.process(x);
            let r = reverb.process(x);
            for v in [c[0], c[1], d[0], d[1], r[0], r[1]] {
                assert!(v.is_finite());
                if i > 20 * 48_000 {
                    peak = peak.max(v.abs());
                }
            }
        }
        assert!(peak < 1.0e-3, "tails must die away, got {peak}");
    }
}
