//! End-to-end tests of the engine through its public, real-time API.
//!
//! Nothing here can say whether a sound is *pleasant*; these tests pin down
//! the things that can be measured: bounded finite output, silence after
//! release, click-free voice stealing and sample-exact timing.

use std::sync::Arc;

use hum_engine::engine::{Cmd, Engine, EngineEvent, RecState, Shared, VOICES};
use hum_engine::fx::Limiter;
use hum_engine::presets::{self, PRESETS};
use hum_engine::scale::{KeyMap, Scale};
use hum_engine::transport::{step_sample, LOOP_STEPS};

const SR: f32 = 48_000.0;

fn engine() -> Engine {
    Engine::new(SR, Arc::new(Shared::default()))
}

fn select(e: &mut Engine, name: &str) {
    let index = presets::find(name).unwrap_or_else(|| panic!("no preset {name}"));
    e.handle(Cmd::SetPreset(index as u8));
    e.shared().reverb.set(PRESETS[index].reverb);
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

/// Record one note into the looper and return (engine, absolute sample of
/// the start of the first recorded loop pass).
fn record_one_note(bpm: f32, offset: usize, length: usize) -> (Engine, usize, Vec<EngineEvent>) {
    let mut e = engine();
    select(&mut e, "Pixel Pulse");
    e.shared().reverb.set(0.0);
    e.handle(Cmd::SetBpm(bpm));
    e.handle(Cmd::RecordStart);
    assert_eq!(e.shared().status().rec, RecState::CountIn);
    let mut out = Vec::new();
    // One bar of count-in (metronome off, so it is silent).
    let count_in = step_sample(16, bpm, SR) as usize;
    render(&mut e, count_in, &mut out);
    assert_eq!(peak(&out), 0.0);
    render(&mut e, offset, &mut out);
    assert_eq!(e.shared().status().rec, RecState::Recording);
    e.handle(Cmd::NoteOn {
        note: 60,
        velocity: 100,
    });
    render(&mut e, length, &mut out);
    e.handle(Cmd::NoteOff { note: 60 });
    let events = drain(&mut e);
    (e, count_in, events)
}

#[test]
fn looper_replays_recorded_events_at_the_right_times() {
    // 120 BPM: one step is exactly 6000 samples, the loop 384000.
    let bpm = 120.0;
    let loop_len = step_sample(LOOP_STEPS as u64, bpm, SR) as usize;
    assert_eq!(loop_len, 384_000);
    let (offset, length) = (12_000usize, 6_000usize);
    let (mut e, count_in, events) = record_one_note(bpm, offset, length);
    assert_eq!(
        events,
        vec![
            EngineEvent::LoopNote {
                layer: 0,
                note: 60,
                on: true,
                t: 2.0
            },
            EngineEvent::LoopNote {
                layer: 0,
                note: 60,
                on: false,
                t: 3.0
            },
        ]
    );

    // Finish the first pass: the take ends by itself after four bars.
    let mut out = Vec::new();
    let played = offset + length;
    render(&mut e, loop_len - played, &mut out);
    let mut out = Vec::new();
    render(&mut e, 1, &mut out);
    assert_eq!(
        drain(&mut e),
        vec![EngineEvent::RecordDone {
            layer: 0,
            kept: true
        }]
    );
    let status = e.shared().status();
    assert_eq!(status.rec, RecState::Idle);
    assert_eq!(status.layers, 0b0001);
    assert!(status.loop_on && status.running);

    // Second and third pass: the note comes back exactly one loop later.
    render(&mut e, 2 * loop_len - 1, &mut out);
    let _ = count_in;
    assert_eq!(
        first_sound(&out),
        Some(offset),
        "replay must start on the recorded sample"
    );
    let second_pass = &out[loop_len * 2..];
    assert_eq!(
        first_sound(second_pass),
        Some(offset),
        "and again on the next pass"
    );
    // The replayed note also ends: silence returns before the pass is over.
    assert_eq!(peak(&out[(loop_len - 48_000) * 2..loop_len * 2]), 0.0);
    assert_clean(&out, "loop playback");

    // Muting silences the layer; un-muting brings it back; clearing empties it.
    e.handle(Cmd::MuteLayer {
        layer: 0,
        muted: true,
    });
    out.clear();
    render(&mut e, loop_len, &mut out);
    assert_eq!(peak(&out), 0.0);
    e.handle(Cmd::MuteLayer {
        layer: 0,
        muted: false,
    });
    out.clear();
    render(&mut e, loop_len, &mut out);
    assert_eq!(first_sound(&out), Some(offset));
    e.handle(Cmd::ClearLayer(0));
    let status = e.shared().status();
    assert_eq!(status.layers, 0);
    assert!(
        !status.running,
        "the clock stops when nothing is left to play"
    );
}

#[test]
fn looper_timing_holds_at_other_tempos() {
    for &bpm in &[60.0f32, 110.0, 160.0] {
        let loop_len = step_sample(LOOP_STEPS as u64, bpm, SR) as usize;
        let (offset, length) = (10_007usize, 4_321usize);
        let (mut e, _, _) = record_one_note(bpm, offset, length);
        let mut out = Vec::new();
        render(&mut e, loop_len - offset - length, &mut out);
        out.clear();
        render(&mut e, loop_len, &mut out);
        let start = first_sound(&out).expect("the layer must replay") as i64;
        // Positions are stored as f32 steps, which is finer than a sample
        // but can round across a sample boundary.
        assert!(
            (start - offset as i64).abs() <= 1,
            "bpm {bpm}: {start} vs {offset}"
        );
    }
}

#[test]
fn each_layer_keeps_its_own_sound_and_undo_order() {
    let mut e = engine();
    e.handle(Cmd::SetBpm(160.0));
    let loop_len = step_sample(LOOP_STEPS as u64, 160.0, SR) as usize;
    let mut out = Vec::new();
    for (layer, name) in ["Pixel Pulse", "Soft Sine", "Chip Bass", "Marimba"]
        .iter()
        .enumerate()
    {
        select(&mut e, name);
        e.handle(Cmd::RecordStart);
        let status = e.shared().status();
        assert_eq!(status.rec_layer, layer);
        let wait = if layer == 0 {
            step_sample(16, 160.0, SR) as usize
        } else {
            0
        };
        render(&mut e, wait + 4_500, &mut out);
        e.handle(Cmd::NoteOn {
            note: 60 + layer as u8,
            velocity: 90,
        });
        render(&mut e, 9_000, &mut out);
        e.handle(Cmd::NoteOff {
            note: 60 + layer as u8,
        });
        render(&mut e, loop_len, &mut out);
        assert_eq!(e.shared().status().layers, (1u8 << (layer + 1)) - 1);
    }
    // A fifth take has nowhere to go.
    e.handle(Cmd::RecordStart);
    assert_eq!(e.shared().status().rec, RecState::Idle);
    assert_clean(&out, "four layers");
    e.handle(Cmd::ClearAll);
    assert_eq!(e.shared().status().layers, 0);
}

#[test]
fn empty_take_is_discarded() {
    let mut e = engine();
    e.handle(Cmd::SetBpm(160.0));
    e.handle(Cmd::RecordStart);
    let mut out = Vec::new();
    let total = step_sample(16 + LOOP_STEPS as u64, 160.0, SR) as usize;
    render(&mut e, total + 10, &mut out);
    assert_eq!(
        drain(&mut e),
        vec![EngineEvent::RecordDone {
            layer: 0,
            kept: false
        }]
    );
    let status = e.shared().status();
    assert_eq!(status.layers, 0);
    assert!(!status.running);
}

#[test]
fn count_in_clicks_only_with_the_metronome() {
    let mut e = engine();
    e.handle(Cmd::Metronome(true));
    e.handle(Cmd::RecordStart);
    let mut out = Vec::new();
    render(&mut e, 2_000, &mut out);
    assert_eq!(
        first_sound(&out),
        Some(1),
        "the first click starts with the count-in"
    );
    assert!(peak(&out) > 0.05);
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
    e.handle(Cmd::RecordStart);
    let mut out = Vec::new();
    render(&mut e, 96_000, &mut out); // count-in
    e.handle(Cmd::KeyDown {
        key: 0,
        velocity: 100,
    });
    render(&mut e, 6_000 * 12 + 10, &mut out);
    e.handle(Cmd::KeyUp { key: 0 });
    render(&mut e, 12_000, &mut out);
    let ons: Vec<(u8, f32)> = drain(&mut e)
        .into_iter()
        .filter_map(|ev| match ev {
            EngineEvent::LoopNote {
                note, on: true, t, ..
            } => Some((note, t)),
            _ => None,
        })
        .collect();
    let notes: Vec<u8> = ons.iter().map(|(n, _)| *n).collect();
    // One held key becomes a chord from the scale (degrees 1, 3, 5 of C
    // minor pentatonic = C, F, B flat), climbing through two octaves.
    assert_eq!(&notes[..8], &[48, 53, 58, 60, 65, 70, 48, 53]);
    assert!(notes.len() >= 12);
    for (i, (_, t)) in ons.iter().enumerate() {
        assert!((t - i as f32).abs() < 0.01, "arp step {i} at {t}");
    }
    for note in &notes {
        assert!(
            [0, 3, 5, 7, 10].contains(&(note % 12)),
            "{note} is outside the scale"
        );
    }
    assert_clean(&out, "arp");
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
