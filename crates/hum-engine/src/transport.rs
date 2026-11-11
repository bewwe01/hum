//! The shared musical clock used by the beat maker, the looper and the
//! arpeggiator. Positions are measured in sixteenth-note steps; the loop is
//! four bars (64 steps) long.

pub const STEPS_PER_BEAT: usize = 4;
pub const STEPS_PER_BAR: usize = 16;
pub const BARS: usize = 4;
pub const LOOP_STEPS: usize = STEPS_PER_BAR * BARS;

pub const MIN_BPM: f32 = 60.0;
pub const MAX_BPM: f32 = 160.0;
pub const DEFAULT_BPM: f32 = 110.0;

/// Number of drum tracks in the beat maker (Kick, Snare, Hat, Clap).
pub const DRUMS: usize = 4;

/// Default beat patterns; bit `i` is step `i + 1`.
/// Kick 1, 5, 9, 13. Snare 5, 13, 16. Hat every odd step plus 16. Clap 13.
pub const DEFAULT_PATTERNS: [u16; DRUMS] = [
    (1 << 0) | (1 << 4) | (1 << 8) | (1 << 12),
    (1 << 4) | (1 << 12) | (1 << 15),
    0b0101_0101_0101_0101 | (1 << 15),
    1 << 12,
];

/// Guards `ceil` against floating-point dust (far below one sample).
const EPS: f64 = 1.0e-6;

/// Samples per sixteenth-note step.
pub fn samples_per_step(bpm: f32, sample_rate: f32) -> f64 {
    f64::from(sample_rate) * 60.0 / (f64::from(bpm) * STEPS_PER_BEAT as f64)
}

/// The sample index (counted from the start of the transport) at which
/// step `k` begins, for a constant tempo. This is the reference the
/// sequencer is tested against.
pub fn step_sample(k: u64, bpm: f32, sample_rate: f32) -> u64 {
    (k as f64 * samples_per_step(bpm, sample_rate) - EPS)
        .ceil()
        .max(0.0) as u64
}

#[derive(Clone, Debug)]
pub struct Transport {
    sample_rate: f32,
    bpm: f32,
    sps: f64,
    /// Position (in steps) at the anchor sample.
    anchor: f64,
    /// Samples elapsed since the anchor.
    n: u64,
    pub running: bool,
}

impl Transport {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: DEFAULT_BPM,
            sps: samples_per_step(DEFAULT_BPM, sample_rate),
            anchor: 0.0,
            n: 0,
            running: false,
        }
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn samples_per_step(&self) -> f64 {
        self.sps
    }

    /// Change tempo without moving the playhead.
    pub fn set_bpm(&mut self, bpm: f32) {
        let bpm = if bpm.is_finite() {
            bpm.clamp(MIN_BPM, MAX_BPM)
        } else {
            DEFAULT_BPM
        };
        self.anchor = self.pos();
        self.n = 0;
        self.bpm = bpm;
        self.sps = samples_per_step(bpm, self.sample_rate);
    }

    /// Start running from position `at` (in steps; negative = count-in).
    pub fn start(&mut self, at: f64) {
        self.anchor = at;
        self.n = 0;
        self.running = true;
    }

    /// Stop and reset the playhead to the start.
    pub fn stop(&mut self) {
        self.running = false;
        self.anchor = 0.0;
        self.n = 0;
    }

    /// Current position in steps.
    pub fn pos(&self) -> f64 {
        self.anchor + self.n as f64 / self.sps
    }

    /// Samples from now until position `t` is reached (0 if it is due).
    pub fn samples_until(&self, t: f64) -> u64 {
        let target = ((t - self.anchor) * self.sps - EPS).ceil();
        if target <= self.n as f64 {
            0
        } else {
            target as u64 - self.n
        }
    }

    pub fn advance(&mut self, samples: usize) {
        self.n += samples as u64;
    }

    /// Jump back by one loop length (called exactly at the loop end).
    pub fn wrap(&mut self) {
        self.anchor -= LOOP_STEPS as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_patterns_match_the_spec() {
        let steps = |bits: u16| -> Vec<usize> {
            (0..16)
                .filter(|i| bits >> i & 1 == 1)
                .map(|i| i + 1)
                .collect()
        };
        assert_eq!(steps(DEFAULT_PATTERNS[0]), vec![1, 5, 9, 13]);
        assert_eq!(steps(DEFAULT_PATTERNS[1]), vec![5, 13, 16]);
        assert_eq!(
            steps(DEFAULT_PATTERNS[2]),
            vec![1, 3, 5, 7, 9, 11, 13, 15, 16]
        );
        assert_eq!(steps(DEFAULT_PATTERNS[3]), vec![13]);
    }

    #[test]
    fn step_samples_are_exact() {
        // 120 BPM at 48 kHz: a sixteenth is exactly 6000 samples.
        for k in 0..200 {
            assert_eq!(step_sample(k, 120.0, 48_000.0), k * 6000);
        }
        // 160 BPM: 4500 samples.
        assert_eq!(step_sample(7, 160.0, 48_000.0), 31_500);
        // 110 BPM: 6545.4545... samples, rounded up to the next sample.
        assert_eq!(step_sample(1, 110.0, 48_000.0), 6546);
        assert_eq!(step_sample(11, 110.0, 48_000.0), 72_000);
        assert_eq!(step_sample(3, 110.0, 48_000.0), 19_637);
    }

    #[test]
    fn transport_reaches_steps_on_the_reference_samples() {
        for &bpm in &[60.0f32, 97.0, 110.0, 120.0, 133.3, 160.0] {
            let mut t = Transport::new(48_000.0);
            t.set_bpm(bpm);
            t.start(0.0);
            let mut sample = 0u64;
            for k in 0..(LOOP_STEPS as u64) {
                let wait = t.samples_until(k as f64);
                t.advance(wait as usize);
                sample += wait;
                assert_eq!(sample, step_sample(k, bpm, 48_000.0), "bpm {bpm} step {k}");
                assert_eq!(t.samples_until(k as f64), 0);
            }
        }
    }

    #[test]
    fn tempo_changes_keep_the_position() {
        let mut t = Transport::new(48_000.0);
        t.start(0.0);
        t.advance(100_000);
        let before = t.pos();
        t.set_bpm(150.0);
        assert!((t.pos() - before).abs() < 1e-9);
        t.set_bpm(1000.0);
        assert_eq!(t.bpm(), MAX_BPM);
    }
}
