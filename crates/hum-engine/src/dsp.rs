//! Small DSP building blocks: math helpers, random numbers, envelopes,
//! band-limited oscillators and a state-variable filter.
//!
//! Nothing in here allocates.

pub const TAU: f32 = core::f32::consts::TAU;
pub const PI: f32 = core::f32::consts::PI;

/// Values smaller than this are treated as silence in feedback paths so that
/// denormal numbers never reach the CPU's slow path.
pub const DENORMAL: f32 = 1.0e-20;

#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < DENORMAL {
        0.0
    } else {
        x
    }
}

#[inline]
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Cheap, monotonic tanh-like saturator, bounded to [-1, 1].
#[inline]
pub fn soft_sat(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// One-pole smoothing coefficient for a time constant in seconds, applied
/// once every `step` seconds: `y += (target - y) * coef`.
#[inline]
pub fn smooth_coef(time: f32, step: f32) -> f32 {
    if time <= 0.0 {
        1.0
    } else {
        1.0 - (-step / time).exp()
    }
}

/// xorshift32 random number generator.
#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn unipolar(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.unipolar() * 2.0 - 1.0
    }
}

/// PolyBLEP residual for a unit step at phase 0, `t` in [0, 1), `dt` = phase
/// increment per sample.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// Band-limited sawtooth in [-1, 1].
#[inline]
pub fn saw(t: f32, dt: f32) -> f32 {
    2.0 * t - 1.0 - poly_blep(t, dt)
}

/// Band-limited pulse with duty cycle `width`, DC-centred.
#[inline]
pub fn pulse(t: f32, dt: f32, width: f32) -> f32 {
    let mut v = if t < width { 1.0 } else { -1.0 };
    v += poly_blep(t, dt);
    v -= poly_blep(fract(t + 1.0 - width), dt);
    v - (2.0 * width - 1.0)
}

/// Triangle in [-1, 1] (harmonics fall off at 12 dB/octave, so the naive
/// shape aliases very little).
#[inline]
pub fn triangle(t: f32) -> f32 {
    4.0 * (t - 0.5).abs() - 1.0
}

#[inline]
pub fn sine(t: f32) -> f32 {
    (TAU * t).sin()
}

// ---------------------------------------------------------------------------
// Envelope

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-sample ADSR coefficients.
#[derive(Clone, Copy, Debug)]
pub struct EnvCoefs {
    pub attack_inc: f32,
    pub decay_coef: f32,
    pub sustain: f32,
    pub release_coef: f32,
}

/// ln(1000): an exponential segment falls by 60 dB over its nominal time.
const LN_60DB: f32 = 6.907_755;
/// Level below which a release is treated as finished (-80 dB).
const SILENCE: f32 = 1.0e-4;

impl EnvCoefs {
    /// `step` is the time between calls to [`Env::next`], in seconds.
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32, step: f32) -> Self {
        Self {
            attack_inc: (step / attack.max(0.0002)).min(1.0),
            decay_coef: (-LN_60DB * step / decay.max(0.001)).exp(),
            sustain: sustain.clamp(0.0, 1.0),
            release_coef: (-LN_60DB * step / release.max(0.002)).exp(),
        }
    }
}

/// ADSR envelope with a linear attack and exponential decay and release.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub stage: Stage,
    pub level: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
        }
    }
}

impl Env {
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn next(&mut self, c: &EnvCoefs) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += c.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = c.sustain + (self.level - c.sustain) * c.decay_coef;
                if self.level - c.sustain < SILENCE {
                    self.level = c.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = c.sustain;
            }
            Stage::Release => {
                self.level *= c.release_coef;
                if self.level < SILENCE {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

// ---------------------------------------------------------------------------
// State-variable filter (Zavalishin / Simper trapezoidal form)

/// Outputs of one filter step.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfOut {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// `g` = tan(pi * fc / fs), `k` = damping (2 = no resonance, towards 0 =
    /// self-oscillation). Unconditionally stable for g > 0, k > 0.
    #[inline]
    pub fn step(&mut self, x: f32, g: f32, k: f32) -> SvfOut {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            lp: v2,
            bp: v1,
            hp: x - k * v1 - v2,
        }
    }

    /// Call occasionally (control rate) to keep the state out of denormals.
    #[inline]
    pub fn flush(&mut self) {
        self.ic1 = flush(self.ic1);
        self.ic2 = flush(self.ic2);
    }
}

/// Filter coefficient for a cutoff in Hz.
#[inline]
pub fn svf_g(cutoff: f32, sample_rate: f32) -> f32 {
    let fc = cutoff.clamp(16.0, sample_rate * 0.45);
    (PI * fc / sample_rate).tan()
}

/// Map a 0..1 resonance amount to the SVF damping factor.
#[inline]
pub fn svf_k(resonance: f32) -> f32 {
    2.0 - 1.94 * resonance.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_reference_pitches() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(60.0) - 261.6256).abs() < 1e-2);
        assert!((midi_to_hz(57.0) - 220.0).abs() < 1e-3);
    }

    #[test]
    fn oscillators_stay_bounded() {
        for &dt in &[0.001f32, 0.01, 0.1, 0.3] {
            let mut t = 0.0f32;
            for _ in 0..10_000 {
                for v in [
                    saw(t, dt),
                    pulse(t, dt, 0.5),
                    pulse(t, dt, 0.125),
                    triangle(t),
                    sine(t),
                ] {
                    assert!(v.is_finite() && v.abs() <= 2.0, "{v} at t={t} dt={dt}");
                }
                t += dt;
                if t >= 1.0 {
                    t -= 1.0;
                }
            }
        }
    }

    #[test]
    fn envelope_runs_through_all_stages() {
        let step = 1.0 / 48_000.0;
        let c = EnvCoefs::new(0.01, 0.05, 0.5, 0.1, step);
        let mut e = Env::default();
        e.trigger();
        let mut peak = 0.0f32;
        for _ in 0..(0.2 * 48_000.0) as usize {
            peak = peak.max(e.next(&c));
        }
        assert!((peak - 1.0).abs() < 1e-6);
        assert_eq!(e.stage, Stage::Sustain);
        assert!((e.level - 0.5).abs() < 1e-3);
        e.release();
        let mut n = 0;
        while e.stage != Stage::Idle {
            e.next(&c);
            n += 1;
            assert!(n < 48_000, "release must end");
        }
        assert_eq!(e.level, 0.0);
    }

    #[test]
    fn filter_is_stable_under_fast_modulation() {
        let mut f = Svf::default();
        let mut rng = Rng::new(7);
        for i in 0..200_000 {
            let cutoff = 20.0 + 20_000.0 * rng.unipolar();
            let g = svf_g(cutoff, 48_000.0);
            let k = svf_k(if i % 2 == 0 { 1.0 } else { 0.0 });
            let o = f.step(rng.bipolar(), g, k);
            assert!(o.lp.is_finite() && o.bp.is_finite() && o.hp.is_finite());
            assert!(o.lp.abs() < 1.0e4);
        }
    }

    #[test]
    fn soft_sat_is_bounded_and_monotonic() {
        let mut prev = -2.0;
        let mut x = -10.0f32;
        while x < 10.0 {
            let y = soft_sat(x);
            assert!((-1.0..=1.0).contains(&y));
            assert!(y >= prev);
            prev = y;
            x += 0.01;
        }
    }
}
