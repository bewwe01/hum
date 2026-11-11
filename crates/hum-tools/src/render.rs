//! Offline rendering of single sounds through the real engine.

use std::sync::Arc;

use hum_engine::engine::{Cmd, Engine, Shared};
use hum_engine::preset::Category;
use hum_engine::scale::{KeyMap, Scale};
use hum_engine::PRESETS;

use crate::analysis;

pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug)]
pub struct RenderSpec {
    /// Index into [`PRESETS`].
    pub preset: usize,
    pub note: u8,
    /// Seconds the key is held.
    pub hold: f32,
    /// Maximum seconds rendered after the key is released.
    pub max_tail: f32,
    /// XY pad position, 0..1 each.
    pub pad_x: f32,
    pub pad_y: f32,
}

impl RenderSpec {
    /// Sensible defaults for auditioning a sound.
    pub fn new(preset: usize) -> Self {
        let p = &PRESETS[preset];
        let note = match p.category {
            Category::Bass => 48, // C3
            _ => 60,              // C4
        };
        let hold = if p.one_shot {
            0.05
        } else if p.arp.is_some() {
            3.0
        } else {
            (p.amp.a * 2.0 + 1.0).max(p.pitch_time + 1.0).max(1.5)
        };
        Self {
            preset,
            note,
            hold,
            max_tail: 12.0,
            pad_x: 0.5,
            pad_y: 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub peak: f32,
    /// RMS over the audible part of the render.
    pub rms: f32,
    /// Spectral centroid in Hz of the held part.
    pub centroid: f32,
    /// Seconds from key release until the output stays below -60 dBFS.
    pub tail: f32,
    pub seconds: f32,
}

pub struct Rendered {
    /// Interleaved stereo.
    pub samples: Vec<f32>,
    pub hold_frames: usize,
    pub stats: Stats,
}

/// -60 dBFS.
const AUDIBLE: f32 = 1.0e-3;

pub fn render(spec: &RenderSpec) -> Rendered {
    let sr = SAMPLE_RATE as f32;
    let p = &PRESETS[spec.preset];
    let shared = Arc::new(Shared::default());
    shared.pad_x.set(spec.pad_x);
    shared.pad_y.set(spec.pad_y);
    shared.reverb.set(p.reverb);
    let mut engine = Engine::new(sr, shared);
    engine.handle(Cmd::SetPreset(spec.preset as u8));
    // Put the requested note on the first key so arpeggios start from it.
    engine.handle(Cmd::SetKeyMap(KeyMap {
        root: spec.note % 12,
        scale: Scale::MinorPentatonic,
        locked: true,
        octave: (spec.note / 12) as i8 - 1,
    }));

    let hold_frames = (spec.hold * sr) as usize;
    let mut samples = vec![0.0f32; hold_frames * 2];
    if p.arp.is_some() {
        engine.handle(Cmd::KeyDown {
            key: 0,
            velocity: 100,
        });
    } else {
        engine.handle(Cmd::NoteOn {
            note: spec.note,
            velocity: 100,
        });
    }
    for chunk in samples.chunks_mut(512) {
        engine.process(chunk);
    }
    if p.arp.is_some() {
        engine.handle(Cmd::KeyUp { key: 0 });
    } else {
        engine.handle(Cmd::NoteOff { note: spec.note });
    }

    // Render the tail until it has been quiet for 300 ms.
    let quiet_needed = (0.3 * sr) as usize;
    let max_tail = (spec.max_tail * sr) as usize;
    let mut quiet = 0usize;
    let mut tail_frames = 0usize;
    let mut block = [0.0f32; 512];
    while tail_frames < max_tail && quiet < quiet_needed {
        engine.process(&mut block);
        if analysis::peak(&block) < AUDIBLE * 0.25 {
            quiet += block.len() / 2;
        } else {
            quiet = 0;
        }
        samples.extend_from_slice(&block);
        tail_frames += block.len() / 2;
    }

    let end = analysis::last_above(&samples, AUDIBLE);
    let mono = analysis::to_mono(&samples);
    let held = &mono[..hold_frames.max(4096).min(mono.len())];
    // Skip the attack so slow pads are measured at full level.
    let skip = ((p.amp.a * sr) as usize).min(held.len() / 2);
    let stats = Stats {
        peak: analysis::peak(&samples),
        rms: analysis::rms(&samples[..end * 2]),
        centroid: analysis::spectral_centroid(&held[skip..], sr),
        tail: end.saturating_sub(hold_frames) as f32 / sr,
        seconds: samples.len() as f32 / 2.0 / sr,
    };
    Rendered {
        samples,
        hold_frames,
        stats,
    }
}

/// File-system friendly name, e.g. "08-warm-cloud".
pub fn file_stem(preset: usize) -> String {
    let slug: String = PRESETS[preset]
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("{:02}-{}", preset + 1, slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_warm_cloud() {
        let index = hum_engine::presets::find("Warm Cloud").unwrap();
        let r = render(&RenderSpec::new(index));
        assert!(r.stats.peak > 0.05 && r.stats.peak <= 1.0);
        assert!(r.stats.rms > 0.01);
        assert!(r.stats.tail > 0.5, "a pad with reverb must ring out");
        assert!(r.stats.centroid > 50.0 && r.stats.centroid < 5000.0);
        assert_eq!(file_stem(index), "08-warm-cloud");
    }
}
