//! End-to-end tests of the engine through its public, real-time API.
//!
//! Nothing here can say whether a sound is *pleasant*; these tests pin down
//! the things that can be measured: bounded finite output, silence after
//! release, click-free voice stealing and sample-exact timing.

use std::sync::Arc;

use hum_engine::engine::{CaptureError, Cmd, Engine, EngineEvent, Shared, LAYERS, VOICES};
use hum_engine::fx::Limiter;
use hum_engine::presets::{self, PRESETS};
use hum_engine::scale::{KeyMap, Scale};
use hum_engine::transport::step_sample;

const SR: f32 = 48_000.0;

fn engine() -> Engine {
    Engine::new(SR, Arc::new(Shared::default()))
}

fn select(e: &mut Engine, name: &str) {
    let index = presets::find(name).unwrap_or_else(|| panic!("no preset {name}"));
    e.handle(Cmd::SetPreset(index as u8));
    e.shared().reverb.set(PRESETS[index].reverb);
}

/// Select a sound with its reverb off, and let the reverb setting settle.
fn select_dry(e: &mut Engine, name: &str) {
    select(e, name);
    e.shared().reverb.set(0.0);
    let mut sink = Vec::new();
    render(e, 24_000, &mut sink);
}

/// First frame at or after `from` louder than `threshold`.
fn first_loud(samples: &[f32], from: usize, threshold: f32) -> Option<usize> {
    samples
        .chunks_exact(2)
        .skip(from)
        .position(|f| f[0].abs().max(f[1].abs()) > threshold)
        .map(|i| i + from)
}

/// Render `frames` frames in awkward, varying chunk sizes (as a real audio
/// callback might) and append them to `out`.
fn render(e: &mut Engine, frames: usize, out: &mut Vec<f32>) {
    let sizes = [64usize, 1, 480, 17, 256, 3, 96, 1024];
    let mut left = frames;
    let mut i = 0;
    let mut block = vec![0.0f32; 2048];
    while left > 0 {
        let n = sizes[i % sizes.len()].min(left);
        i += 1;
        let slice = &mut block[..n * 2];
        e.process(slice);
        out.extend_from_slice(slice);
        left -= n;
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

fn assert_clean(samples: &[f32], what: &str) {
    for (i, s) in samples.iter().enumerate() {
        assert!(s.is_finite(), "{what}: sample {i} is {s}");
        assert!(s.abs() <= 1.0, "{what}: sample {i} = {s} exceeds 0 dBFS");
    }
    assert!(
        peak(samples) <= Limiter::CEILING + 1.0e-6,
        "{what}: above the limiter ceiling"
    );
}

/// Index of the first frame that is not exactly silent.
fn first_sound(samples: &[f32]) -> Option<usize> {
    samples
        .chunks_exact(2)
        .position(|f| f[0] != 0.0 || f[1] != 0.0)
}

fn drain(e: &mut Engine) -> Vec<EngineEvent> {
    let mut events = Vec::new();
    e.drain_events(|ev| events.push(ev));
    events
}

#[test]
fn all_56_presets_render_bounded_non_silent_audio() {
    assert_eq!(PRESETS.len(), 56);
    for (index, p) in PRESETS.iter().enumerate() {
        let mut e = engine();
        e.handle(Cmd::SetPreset(index as u8));
        e.shared().reverb.set(p.reverb);
        let mut out = Vec::new();
        // Play through the keys so Arps sounds arpeggiate.
        e.handle(Cmd::KeyDown {
            key: 2,
            velocity: 110,
        });
        render(&mut e, 72_000, &mut out);
        e.handle(Cmd::KeyUp { key: 2 });
        render(&mut e, 48_000, &mut out);
        assert_clean(&out, p.name);
        assert!(
            peak(&out) > 0.01,
            "{} is silent (peak {})",
            p.name,
            peak(&out)
        );
    }
}

#[test]
fn output_stays_below_0_dbfs_under_abuse() {
    // Every key down on every sound with all controls at their extremes and
    // the beat running: the limiter must hold.
    for (index, p) in PRESETS.iter().enumerate() {
        let mut e = engine();
        e.handle(Cmd::SetPreset(index as u8));
        let s = e.shared().clone();
        s.volume.set(1.0);
        s.reverb.set(1.0);
        s.pad_x.set(1.0);
        s.pad_y.set(1.0);
        s.attack.set(0.0);
        s.release.set(1.0);
        e.handle(Cmd::SetBpm(160.0));
        e.handle(Cmd::BeatPlay(true));
        e.handle(Cmd::Metronome(true));
        for key in 0..7 {
            e.handle(Cmd::KeyDown { key, velocity: 127 });
        }
        for note in [24u8, 36, 48, 60, 72, 84, 96, 108, 120] {
            e.handle(Cmd::NoteOn {
                note,
                velocity: 127,
            });
        }
        let mut out = Vec::new();
        render(&mut e, 36_000, &mut out);
        s.pad_x.set(0.0);
        s.pad_y.set(0.0);
        s.attack.set(1.0);
        s.release.set(0.0);
        render(&mut e, 12_000, &mut out);
        assert_clean(&out, p.name);
    }
}

#[test]
fn note_off_reaches_silence_and_frees_voices() {
    for (index, p) in PRESETS.iter().enumerate() {
        let mut e = engine();
        e.handle(Cmd::SetPreset(index as u8));
        e.shared().reverb.set(p.reverb);
        let mut out = Vec::new();
        e.handle(Cmd::KeyDown {
            key: 0,
            velocity: 100,
        });
        e.handle(Cmd::KeyDown {
            key: 4,
            velocity: 100,
        });
        render(&mut e, 24_000, &mut out);
        if p.amp.s > 0.0 && p.arp.is_none() {
            assert_eq!(
                e.active_voices(),
                2,
                "{}: held notes must keep sounding",
                p.name
            );
        }
        e.handle(Cmd::KeyUp { key: 0 });
        e.handle(Cmd::KeyUp { key: 4 });
        // 25 seconds is far longer than any release, delay or reverb tail.
        out.clear();
        render(&mut e, 25 * 48_000, &mut out);
        assert_eq!(e.active_voices(), 0, "{}: voices still active", p.name);
        let tail = &out[out.len() - 9_600..];
        assert!(
            peak(tail) < 1.0e-4,
            "{}: not silent after release ({})",
            p.name,
            peak(tail)
        );
        assert_clean(&out, p.name);
    }
}

#[test]
fn note_on_starts_immediately_and_note_off_ends_the_note() {
    let mut e = engine();
    select(&mut e, "Pixel Pulse");
    e.shared().reverb.set(0.0);
    let mut out = Vec::new();
    // Give the smoothed controls time to settle on "no reverb".
    render(&mut e, 24_000, &mut out);
    assert_eq!(peak(&out), 0.0, "engine must be silent before any note");
    e.handle(Cmd::NoteOn {
        note: 60,
        velocity: 100,
    });
    out.clear();
    render(&mut e, 4800, &mut out);
    assert_eq!(
        first_sound(&out),
        Some(0),
        "the note must start on the very first sample"
    );
    assert_eq!(e.active_voices(), 1);
    e.handle(Cmd::NoteOff { note: 60 });
    out.clear();
    render(&mut e, 48_000, &mut out);
    assert_eq!(e.active_voices(), 0);
    // With no reverb or delay the output returns to exact silence.
    assert_eq!(peak(&out[out.len() - 4_800..]), 0.0);
}

/// Largest jump between consecutive samples.
fn max_step(samples: &[f32]) -> f32 {
    let mut worst = 0.0f32;
    for ch in 0..2 {
        let mut prev = samples[ch];
        for f in samples.chunks_exact(2) {
            worst = worst.max((f[ch] - prev).abs());
            prev = f[ch];
        }
    }
    worst
}

#[test]
fn voice_stealing_does_not_click() {
    // A pure, low tone makes any discontinuity obvious: a 100 Hz sine moves
    // by about 0.003 per sample at this level, while cutting a voice off
    // mid-cycle would jump by up to its full amplitude (about 0.2).
    let mut e = engine();
    select(&mut e, "Soft Sine");
    e.shared().reverb.set(0.0);
    let mut out = Vec::new();
    let mut note_ons = 0;
    // Hold far more notes than there are voices, never releasing any.
    for round in 0..4 {
        for note in 33u8..57 {
            e.handle(Cmd::NoteOn {
                note: note + (round % 2),
                velocity: 100,
            });
            note_ons += 1;
            render(&mut e, 731, &mut out);
        }
    }
    assert!(note_ons > VOICES * 3, "the test must actually steal voices");
    assert_eq!(e.active_voices(), VOICES);
    assert_clean(&out, "stealing");
    let step = max_step(&out);
    assert!(
        step < 0.05,
        "click detected while stealing voices: step {step}"
    );

    // "All notes off" is a fast fade, not a cut.
    out.clear();
    e.handle(Cmd::AllNotesOff);
    render(&mut e, 4_800, &mut out);
    assert!(
        max_step(&out) < 0.05,
        "click on all-notes-off: {}",
        max_step(&out)
    );
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn sequencer_places_steps_at_exact_sample_positions() {
    let hat = 2u8;
    for &bpm in &[60.0f32, 97.0, 110.0, 120.0, 133.0, 160.0] {
        for &step in &[0u64, 1, 5, 11, 15] {
            let mut e = engine();
            e.handle(Cmd::SetBpm(bpm));
            for drum in 0..4 {
                e.handle(Cmd::SetPattern { drum, bits: 0 });
            }
            e.handle(Cmd::SetPattern {
                drum: hat,
                bits: 1 << step,
            });
            e.handle(Cmd::BeatPlay(true));
            let mut out = Vec::new();
            let expected = step_sample(step, bpm, SR) as usize;
            render(&mut e, expected + 2_000, &mut out);
            assert_eq!(first_sound(&out), Some(expected), "bpm {bpm} step {step}");
        }
    }
}

#[test]
fn beat_reports_steps_and_stopping_resets_the_playhead() {
    let mut e = engine();
    e.handle(Cmd::SetBpm(120.0));
    e.handle(Cmd::BeatPlay(true));
    let mut out = Vec::new();
    render(&mut e, 6_000 * 5 + 100, &mut out);
    let status = e.shared().status();
    assert!(status.running && status.beat_on);
    assert_eq!(status.step, 5);
    assert!((status.pos - 5.0166).abs() < 0.01, "pos {}", status.pos);
    assert!(peak(&out) > 0.05, "the default beat must be audible");
    e.handle(Cmd::BeatPlay(false));
    let status = e.shared().status();
    assert!(!status.running);
    assert_eq!(status.step, -1);
    assert_eq!(status.pos, 0.0);
}

#[test]
fn arpeggiator_free_runs_without_the_transport() {
    // With the transport stopped an arp starts on the key press and steps at
    // the tempo: 120 BPM sixteenths are 6000 samples apart.
    let mut e = engine();
    select(&mut e, "Pulse Runner");
    e.shared().reverb.set(0.0);
    e.handle(Cmd::SetBpm(120.0));
    let mut out = Vec::new();
    e.handle(Cmd::KeyDown {
        key: 3,
        velocity: 100,
    });
    render(&mut e, 30_000, &mut out);
    assert_eq!(first_sound(&out), Some(0));
    e.handle(Cmd::KeyUp { key: 3 });
    render(&mut e, 48_000, &mut out);
    assert_eq!(
        e.active_voices(),
        0,
        "the arp must stop when the key is released"
    );
}

#[test]
fn changing_sound_while_holding_keys_leaves_nothing_stuck() {
    let mut e = engine();
    let mut out = Vec::new();
    for round in 0..PRESETS.len() {
        e.handle(Cmd::SetPreset(round as u8));
        e.handle(Cmd::KeyDown {
            key: (round % 7) as u8,
            velocity: 100,
        });
        e.handle(Cmd::Preview { note: 60 });
        render(&mut e, 2_400, &mut out);
    }
    e.handle(Cmd::SetPreset(0));
    for key in 0..7 {
        e.handle(Cmd::KeyUp { key });
    }
    out.clear();
    render(&mut e, 25 * 48_000, &mut out);
    assert_eq!(e.active_voices(), 0);
    assert!(peak(&out[out.len() - 9_600..]) < 1.0e-4);
}

#[test]
fn preview_plays_a_short_note_by_itself() {
    let mut e = engine();
    select(&mut e, "Soft Bell");
    let mut out = Vec::new();
    e.handle(Cmd::Preview { note: 60 });
    render(&mut e, 4_800, &mut out);
    assert!(peak(&out) > 0.01);
    assert!(e.active_voices() >= 1);
    render(&mut e, 10 * 48_000, &mut out);
    assert_eq!(e.active_voices(), 0, "the preview must release on its own");
}

#[test]
fn works_at_other_sample_rates() {
    for &sr in &[44_100.0f32, 96_000.0] {
        let mut e = Engine::new(sr, Arc::new(Shared::default()));
        e.handle(Cmd::BeatPlay(true));
        e.handle(Cmd::KeyDown {
            key: 0,
            velocity: 100,
        });
        let mut out = Vec::new();
        render(&mut e, sr as usize, &mut out);
        assert_clean(&out, "sample rate");
        assert!(peak(&out) > 0.01);
    }
}

/// Play `note` for `length` samples starting now, then `rest` samples of
/// silence.
fn play(e: &mut Engine, note: u8, length: usize, rest: usize, out: &mut Vec<f32>) {
    e.handle(Cmd::NoteOn {
        note,
        velocity: 100,
    });
    render(e, length, out);
    e.handle(Cmd::NoteOff { note });
    render(e, rest, out);
}

fn captured_notes(events: &[EngineEvent]) -> Vec<(u8, u8, f32, f32)> {
    events
        .iter()
        .filter_map(|ev| match *ev {
            EngineEvent::LayerNote {
                layer,
                note,
                on,
                off,
                ..
            } => Some((layer, note, on, off)),
            _ => None,
        })
        .collect()
}

#[test]
fn first_capture_sets_the_loop_length_and_tempo() {
    let mut e = engine();
    select_dry(&mut e, "Pixel Pulse");
    let mut out = Vec::new();
    // A 4-second phrase: notes at 0 s and 1 s, capture at exactly 4 s.
    play(&mut e, 60, 9_600, 38_400, &mut out);
    play(&mut e, 64, 9_600, 134_400, &mut out);
    e.handle(Cmd::Capture);
    let events = drain(&mut e);
    // 4 s fits 2 bars at 120 BPM best (nearest to 110 within 60..160).
    assert_eq!(
        events[0],
        EngineEvent::LoopStarted {
            steps: 32,
            bpm: 120.0
        }
    );
    assert!(matches!(
        events[1],
        EngineEvent::LayerCaptured {
            layer: 0,
            notes: 2,
            ..
        }
    ));
    let notes = captured_notes(&events);
    assert_eq!(notes.len(), 2);
    // 1 s of a 4 s loop of 32 steps is step 8; 0.2 s long is 1.6 steps.
    assert!((notes[0].2 - 0.0).abs() < 1e-3 && (notes[0].3 - 1.6).abs() < 1e-3);
    assert!((notes[1].2 - 8.0).abs() < 1e-3);
    let status = e.shared().status();
    assert!(status.has_loop && status.running && status.loop_on);
    assert_eq!(status.loop_steps, 32);
    assert_eq!(status.layers, 1);

    // The loop starts at the capture: note 1 at once, note 2 one second
    // later, and the whole loop again four seconds later.
    out.clear();
    render(&mut e, 192_000 + 4_800, &mut out);
    assert_eq!(first_sound(&out), Some(0));
    assert_eq!(
        first_sound(&out[28_800 * 2..]).map(|i| i + 28_800),
        Some(48_000)
    );
    assert_eq!(
        first_sound(&out[110_000 * 2..]).map(|i| i + 110_000),
        Some(192_000)
    );
    assert_clean(&out, "loop");
}

#[test]
fn later_captures_take_the_last_pass() {
    let mut e = engine();
    select_dry(&mut e, "Pixel Pulse");
    let mut out = Vec::new();
    play(&mut e, 60, 9_600, 182_400, &mut out); // 4 s phrase
    e.handle(Cmd::Capture);
    drain(&mut e);
    // Play over the loop: a note 2 s into the pass, then capture after a
    // full pass. The new layer holds it at step 16 of 32.
    select(&mut e, "Soft Sine");
    render(&mut e, 96_000, &mut out);
    play(&mut e, 67, 24_000, 96_000, &mut out);
    e.handle(Cmd::Capture);
    let events = drain(&mut e);
    let soft_sine = presets::find("Soft Sine").unwrap() as u8;
    assert!(matches!(
        events[0],
        EngineEvent::LayerCaptured { layer: 1, notes: 1, preset } if preset == soft_sine
    ));
    let notes = captured_notes(&events);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1, 67);
    assert!(
        (notes[0].2 - 16.0).abs() < 1e-3 && (notes[0].3 - 20.0).abs() < 1e-3,
        "{notes:?}"
    );
    assert_eq!(e.shared().status().layers, 0b11);

    // Muting, volume and clearing.
    e.handle(Cmd::MuteLayer {
        layer: 1,
        muted: true,
    });
    e.handle(Cmd::LayerVolume {
        layer: 0,
        volume: 0.5,
    });
    assert_eq!(e.shared().status().muted, 0b10);
    e.handle(Cmd::ClearLayer(0));
    e.handle(Cmd::ClearLayer(1));
    let status = e.shared().status();
    assert!(!status.has_loop && !status.running && status.layers == 0);
}

#[test]
fn capture_explains_why_nothing_was_kept() {
    let mut e = engine();
    let mut out = Vec::new();
    e.handle(Cmd::Capture);
    assert_eq!(
        drain(&mut e),
        vec![EngineEvent::CaptureFailed(CaptureError::Nothing)]
    );
    play(&mut e, 60, 4_800, 19_200, &mut out); // 0.5 s phrase
    e.handle(Cmd::Capture);
    assert_eq!(
        drain(&mut e),
        vec![EngineEvent::CaptureFailed(CaptureError::TooShort)]
    );
    // A pause longer than 3 s starts a new phrase: the old note is dropped.
    render(&mut e, 4 * 48_000, &mut out);
    play(&mut e, 62, 4_800, 91_200, &mut out);
    e.handle(Cmd::Capture);
    let events = drain(&mut e);
    assert_eq!(captured_notes(&events).len(), 1);
    // Fill every layer, then one more capture has nowhere to go.
    // (The loop is 2 s now, so each new note must fall in the last 2 s.)
    for _ in 1..LAYERS {
        play(&mut e, 64, 4_800, 48_000, &mut out);
        e.handle(Cmd::Capture);
    }
    drain(&mut e);
    assert_eq!(e.shared().status().layers, 0xFF);
    play(&mut e, 64, 4_800, 48_000, &mut out);
    e.handle(Cmd::Capture);
    assert_eq!(
        drain(&mut e),
        vec![EngineEvent::CaptureFailed(CaptureError::Full)]
    );
}

#[test]
fn loaded_layers_replay_at_their_positions() {
    let mut e = engine();
    select(&mut e, "Pixel Pulse");
    e.shared().reverb.set(0.0);
    e.handle(Cmd::SetBpm(120.0)); // a step is 6000 samples
    e.handle(Cmd::SetLoopSteps(16));
    let preset = presets::find("Pixel Pulse").unwrap() as u8;
    e.handle(Cmd::BeginLayer {
        layer: 3,
        preset,
        muted: false,
        volume: 1.0,
    });
    e.handle(Cmd::LoadNote {
        layer: 3,
        note: 60,
        velocity: 100,
        on: 5.0,
        off: 6.0,
    });
    e.handle(Cmd::EndLayer(3));
    e.handle(Cmd::LoopPlay(true));
    let mut out = Vec::new();
    render(&mut e, 96_000 + 40_000, &mut out);
    // Loaded layers use the sound's own reverb, so look for the attack.
    let first = first_loud(&out, 0, 0.05).unwrap();
    let again = first_loud(&out, 60_000, 0.05).unwrap();
    assert!((30_000..30_064).contains(&first), "{first}");
    assert_eq!(
        again - first,
        96_000,
        "one loop (16 steps of 6000 samples) later"
    );
}

#[test]
fn arpeggiator_walks_the_scale_in_time() {
    let mut e = engine();
    select(&mut e, "Rain Arp"); // up, sixteenth notes, two octaves
    e.handle(Cmd::SetBpm(120.0));
    e.handle(Cmd::SetKeyMap(KeyMap {
        root: 0,
        scale: Scale::MinorPentatonic,
        locked: true,
        octave: 3,
    }));
    let mut out = Vec::new();
    // Hold one key for 3 s (24 sixteenths at 120 BPM) and capture it.
    e.handle(Cmd::KeyDown {
        key: 0,
        velocity: 100,
    });
    render(&mut e, 144_000, &mut out);
    e.handle(Cmd::KeyUp { key: 0 });
    e.handle(Cmd::Capture);
    let notes = captured_notes(&drain(&mut e));
    let pitches: Vec<u8> = notes.iter().map(|n| n.1).collect();
    // One held key becomes a chord from the scale (C, F, B flat), climbing
    // through two octaves.
    assert_eq!(&pitches[..8], &[48, 53, 58, 60, 65, 70, 48, 53]);
    assert_eq!(notes.len(), 24);
    // Evenly spaced: 3 s became 1 bar (80 BPM), so a note every 2/3 step.
    for (i, n) in notes.iter().enumerate() {
        assert!(
            (n.2 - i as f32 * 16.0 / 24.0).abs() < 0.01,
            "arp step {i} at {}",
            n.2
        );
    }
    for p in &pitches {
        assert!(
            [0, 3, 5, 7, 10].contains(&(p % 12)),
            "{p} is outside the scale"
        );
    }
    assert_clean(&out, "arp");
}
