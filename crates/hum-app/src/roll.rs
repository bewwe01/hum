//! The piano roll's note model and editing operations, kept free of UI
//! code so they can be unit-tested. Times are in sixteenth steps within
//! the loop; a loop layer stores the same notes as `LoopNote`s (whose end
//! may wrap past the loop point).

use hum_engine::scale::KeyMap;

use crate::state::LoopNote;

/// Lowest and highest pitch shown for pitched sounds (C1..C7).
pub const LOW: u8 = 24;
pub const HIGH: u8 = 96;
/// The shortest note (a 1/64 note).
pub const MIN_LEN: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RNote {
    pub pitch: u8,
    pub start: f32,
    pub len: f32,
    pub vel: u8,
    pub sel: bool,
}

impl RNote {
    pub fn new(pitch: u8, start: f32, len: f32, vel: u8) -> Self {
        Self {
            pitch,
            start,
            len,
            vel,
            sel: false,
        }
    }

    pub fn end(&self) -> f32 {
        self.start + self.len
    }
}

/// Snap choices: label and grid size in steps (0 = off).
pub const SNAPS: [(&str, f32); 7] = [
    ("1/16", 1.0),
    ("1/8", 2.0),
    ("1/4", 4.0),
    ("1/32", 0.5),
    ("1/8T", 4.0 / 3.0),
    ("1/16T", 2.0 / 3.0),
    ("Off", 0.0),
];

pub fn snap(t: f32, grid: f32) -> f32 {
    if grid > 0.0 {
        (t / grid).round() * grid
    } else {
        t
    }
}

pub fn snap_floor(t: f32, grid: f32) -> f32 {
    if grid > 0.0 {
        (t / grid + 1.0e-4).floor() * grid
    } else {
        t
    }
}

pub fn from_layer(notes: &[LoopNote], steps: f32) -> Vec<RNote> {
    let mut out: Vec<RNote> = notes
        .iter()
        .map(|n| {
            let start = n.on.rem_euclid(steps);
            let mut len = (n.off - n.on).rem_euclid(steps);
            if len < 1.0e-3 {
                len = if (n.off - n.on).abs() < 1.0e-3 {
                    MIN_LEN
                } else {
                    steps
                };
            }
            RNote::new(n.note, start, len.max(MIN_LEN), n.velocity.max(1))
        })
        .collect();
    sort(&mut out);
    out
}

pub fn to_layer(notes: &[RNote], steps: f32) -> Vec<LoopNote> {
    notes
        .iter()
        .map(|n| {
            let on = n.start.rem_euclid(steps);
            // A note as long as the loop would end where it starts.
            let len = n.len.clamp(MIN_LEN, steps - 0.01);
            LoopNote {
                note: n.pitch,
                velocity: n.vel,
                on,
                off: (on + len).rem_euclid(steps),
            }
        })
        .collect()
}

pub fn sort(notes: &mut [RNote]) {
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(b.pitch.cmp(&a.pitch)));
}

/// The note under (`t`, `pitch`), topmost (last drawn) first.
pub fn hit(notes: &[RNote], t: f32, pitch: u8) -> Option<usize> {
    notes
        .iter()
        .rposition(|n| n.pitch == pitch && t >= n.start && t < n.end())
}

/// Add a note, replacing any note of the same pitch starting at the same
/// time. Returns its index.
pub fn add(notes: &mut Vec<RNote>, note: RNote) -> usize {
    notes.retain(|n| !(n.pitch == note.pitch && (n.start - note.start).abs() < 1.0e-3));
    notes.push(note);
    sort(notes);
    notes
        .iter()
        .position(|n| n.pitch == note.pitch && (n.start - note.start).abs() < 1.0e-3)
        .unwrap_or(0)
}

/// Cut every note on one of `pitches` that crosses `t` in two.
pub fn slice(notes: &mut Vec<RNote>, t: f32, pitches: &[u8]) -> usize {
    let mut added = Vec::new();
    for n in notes.iter_mut() {
        if pitches.contains(&n.pitch) && t > n.start + MIN_LEN * 0.5 && t < n.end() - MIN_LEN * 0.5
        {
            let tail = RNote::new(n.pitch, t, n.end() - t, n.vel);
            n.len = t - n.start;
            added.push(tail);
        }
    }
    let count = added.len();
    notes.extend(added);
    sort(notes);
    count
}

/// Indices the helpers act on: the selection, or every note.
pub fn targets(notes: &[RNote]) -> Vec<usize> {
    let sel: Vec<usize> = (0..notes.len()).filter(|&i| notes[i].sel).collect();
    if sel.is_empty() {
        (0..notes.len()).collect()
    } else {
        sel
    }
}

/// Move note starts towards the grid by `strength` (0..1).
pub fn quantize(notes: &mut [RNote], grid: f32, strength: f32, steps: f32) {
    let grid = if grid > 0.0 { grid } else { 1.0 };
    for i in targets(notes) {
        let n = &mut notes[i];
        let target = snap(n.start, grid);
        n.start = (n.start + (target - n.start) * strength).rem_euclid(steps);
    }
    sort(notes);
}

/// Nudge timing (up to ±`amount` steps) and loudness (±12) at random;
/// `rand` returns values in -1..1.
pub fn humanize(notes: &mut [RNote], amount: f32, steps: f32, mut rand: impl FnMut() -> f32) {
    for i in targets(notes) {
        let n = &mut notes[i];
        n.start = (n.start + rand() * amount).clamp(0.0, steps - MIN_LEN);
        n.vel = (f32::from(n.vel) + rand() * 12.0)
            .round()
            .clamp(20.0, 127.0) as u8;
    }
    sort(notes);
}

/// Copy the selection (or everything) right after itself, by whole bars.
/// Returns the loop length needed (larger than `steps` if it must grow).
pub fn duplicate(notes: &mut Vec<RNote>, steps: f32) -> f32 {
    let idx = targets(notes);
    if idx.is_empty() {
        return steps;
    }
    let all = !notes.iter().any(|n| n.sel);
    let (first, last) = idx.iter().fold((f32::MAX, 0.0f32), |(a, b), &i| {
        (a.min(notes[i].start), b.max(notes[i].end()))
    });
    let span = if all {
        steps
    } else {
        // Whole bars from the bar the selection starts in.
        let bar0 = (first / 16.0).floor() * 16.0;
        ((last - bar0) / 16.0).ceil().max(1.0) * 16.0
    };
    let copies: Vec<RNote> = idx
        .iter()
        .map(|&i| {
            let mut c = notes[i];
            c.start += span;
            c.sel = true;
            c
        })
        .collect();
    for n in notes.iter_mut() {
        n.sel = false;
    }
    // Copies that start past the loop's end need a longer loop.
    let raw = copies.iter().fold(steps, |m, c| m.max(c.start + MIN_LEN));
    notes.extend(copies);
    sort(notes);
    // Loop lengths are 1, 2, 4 or 8 bars.
    [16.0, 32.0, 64.0, 128.0]
        .into_iter()
        .find(|&s| s >= raw)
        .unwrap_or(128.0)
        .max(steps)
}

/// Scale pitches (ascending) between LOW and HIGH.
pub fn scale_pitches(map: &KeyMap) -> Vec<u8> {
    let intervals = map.scale.intervals();
    (LOW..=HIGH)
        .filter(|p| intervals.contains(&((p + 120 - map.root) % 12)))
        .collect()
}

/// Move a pitch by `steps` scale degrees.
pub fn scale_step(pitch: u8, steps: i32, scale: &[u8]) -> u8 {
    let pos = scale.iter().rposition(|&p| p <= pitch).unwrap_or(0) as i32;
    let i = (pos + steps).clamp(0, scale.len() as i32 - 1) as usize;
    scale[i]
}

/// Transpose the targets by scale degrees and octaves.
pub fn transpose(notes: &mut [RNote], degrees: i32, octaves: i32, scale: &[u8]) {
    for i in targets(notes) {
        let n = &mut notes[i];
        let p = if degrees != 0 {
            scale_step(n.pitch, degrees, scale)
        } else {
            n.pitch
        };
        n.pitch = (i32::from(p) + 12 * octaves).clamp(0, 127) as u8;
    }
}

/// Chord shapes for the stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    Off,
    /// The triad that fits the key, built from scale degrees.
    InKey,
    Major,
    Minor,
    Seventh,
    Sus,
    Power,
}

impl Chord {
    pub const ALL: [Chord; 7] = [
        Chord::Off,
        Chord::InKey,
        Chord::Major,
        Chord::Minor,
        Chord::Seventh,
        Chord::Sus,
        Chord::Power,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Chord::Off => "Off",
            Chord::InKey => "In key",
            Chord::Major => "Major",
            Chord::Minor => "Minor",
            Chord::Seventh => "7th",
            Chord::Sus => "Sus",
            Chord::Power => "Power",
        }
    }

    /// The chord's pitches with `root` at the bottom.
    pub fn pitches(self, root: u8, scale: &[u8]) -> Vec<u8> {
        let fixed: &[i32] = match self {
            Chord::Off => &[0],
            Chord::InKey => {
                return [0, 2, 4]
                    .iter()
                    .map(|&d| scale_step(root, d, scale))
                    .collect();
            }
            Chord::Major => &[0, 4, 7],
            Chord::Minor => &[0, 3, 7],
            Chord::Seventh => &[0, 4, 7, 10],
            Chord::Sus => &[0, 5, 7],
            Chord::Power => &[0, 7, 12],
        };
        fixed
            .iter()
            .map(|&i| (i32::from(root) + i).clamp(0, 127) as u8)
            .collect()
    }
}

/// Fit a layer's notes to a new loop length: repeat them to fill a longer
/// loop, drop what falls outside a shorter one.
pub fn resize_loop(notes: &[LoopNote], from: u16, to: u16) -> Vec<LoopNote> {
    let (from_f, to_f) = (f32::from(from), f32::from(to));
    let mut out = Vec::new();
    if to > from {
        let copies = to / from.max(1);
        for k in 0..copies {
            let shift = f32::from(k) * from_f;
            for n in notes {
                let len = (n.off - n.on).rem_euclid(from_f);
                let on = n.on.rem_euclid(from_f) + shift;
                out.push(LoopNote {
                    on,
                    off: (on + len).rem_euclid(to_f),
                    ..*n
                });
            }
        }
    } else {
        for n in notes {
            if n.on < to_f {
                let len = (n.off - n.on).rem_euclid(from_f).min(to_f - n.on);
                out.push(LoopNote {
                    off: (n.on + len.max(MIN_LEN)).rem_euclid(to_f),
                    ..*n
                });
            }
        }
    }
    out
}

/// General MIDI drum names (notes 35..=81), for SoundFont drum kits.
pub fn drum_name(note: u8) -> Option<&'static str> {
    const NAMES: [&str; 47] = [
        "Kick 2",
        "Kick",
        "Side stick",
        "Snare",
        "Clap",
        "Snare 2",
        "Low floor tom",
        "Closed hat",
        "High floor tom",
        "Pedal hat",
        "Low tom",
        "Open hat",
        "Low-mid tom",
        "High-mid tom",
        "Crash",
        "High tom",
        "Ride",
        "China",
        "Ride bell",
        "Tambourine",
        "Splash",
        "Cowbell",
        "Crash 2",
        "Vibraslap",
        "Ride 2",
        "High bongo",
        "Low bongo",
        "Mute conga",
        "Open conga",
        "Low conga",
        "High timbale",
        "Low timbale",
        "High agogo",
        "Low agogo",
        "Cabasa",
        "Maracas",
        "Short whistle",
        "Long whistle",
        "Short guiro",
        "Long guiro",
        "Claves",
        "High wood block",
        "Low wood block",
        "Mute cuica",
        "Open cuica",
        "Mute triangle",
        "Open triangle",
    ];
    note.checked_sub(35)
        .and_then(|i| NAMES.get(usize::from(i)).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hum_engine::scale::Scale;

    fn map() -> KeyMap {
        KeyMap {
            root: 0,
            scale: Scale::Major,
            octave: 4,
            locked: true,
        }
    }

    #[test]
    fn layer_notes_round_trip_including_wrapping_ones() {
        let notes = vec![
            LoopNote {
                note: 60,
                velocity: 90,
                on: 2.0,
                off: 6.0,
            },
            LoopNote {
                note: 64,
                velocity: 80,
                on: 30.0,
                off: 2.0,
            },
        ];
        let r = from_layer(&notes, 32.0);
        assert_eq!(r[0].pitch, 60);
        assert_eq!(r[1].len, 4.0, "the wrapping note is 4 steps long");
        let back = to_layer(&r, 32.0);
        assert_eq!(back[1].off, 2.0);
        assert_eq!(back[0].on, 2.0);
    }

    #[test]
    fn add_hit_slice_and_snap() {
        let mut n = Vec::new();
        add(&mut n, RNote::new(60, 4.0, 4.0, 100));
        add(&mut n, RNote::new(60, 4.0, 2.0, 100));
        assert_eq!(n.len(), 1, "same pitch and start replaces");
        assert_eq!(hit(&n, 5.0, 60), Some(0));
        assert_eq!(hit(&n, 6.5, 60), None);
        n[0].len = 4.0;
        assert_eq!(slice(&mut n, 6.0, &[60]), 1);
        assert_eq!((n[0].len, n[1].start, n[1].len), (2.0, 6.0, 2.0));
        assert_eq!(snap(5.4, 4.0 / 3.0), 16.0 / 3.0);
        assert_eq!(snap_floor(1.99, 1.0), 1.0);
    }

    #[test]
    fn quantize_humanize_and_transpose_act_on_the_selection() {
        let mut n = vec![RNote::new(60, 1.3, 1.0, 100), RNote::new(62, 2.6, 1.0, 100)];
        n[1].sel = true;
        quantize(&mut n, 1.0, 1.0, 16.0);
        assert_eq!((n[0].start, n[1].start), (1.3, 3.0));
        let scale = scale_pitches(&map());
        transpose(&mut n, 1, 0, &scale);
        assert_eq!(n[1].pitch, 64, "D up one degree of C major is E");
        transpose(&mut n, 0, -1, &scale);
        assert_eq!(n[1].pitch, 52);
        let mut x = 0.0f32;
        humanize(&mut n, 0.1, 16.0, || {
            x = -x + 0.5;
            x
        });
        assert!(n.iter().all(|q| (20..=127).contains(&q.vel)));
    }

    #[test]
    fn duplicate_copies_by_bars_and_reports_growth() {
        let mut n = vec![RNote::new(60, 0.0, 2.0, 100), RNote::new(62, 4.0, 2.0, 100)];
        let needed = duplicate(&mut n, 32.0);
        assert_eq!(n.len(), 4);
        assert_eq!(needed, 64.0, "copying everything doubles the loop");
        let mut m = vec![RNote::new(60, 0.0, 2.0, 100)];
        m[0].sel = true;
        assert_eq!(duplicate(&mut m, 32.0), 32.0);
        assert_eq!(m[1].start, 16.0, "one bar later");
        assert!(m[1].sel && !m[0].sel);
    }

    #[test]
    fn chords_and_loop_resizing() {
        let scale = scale_pitches(&map());
        assert_eq!(
            Chord::InKey.pitches(62, &scale),
            [62, 65, 69],
            "D minor in C major"
        );
        assert_eq!(Chord::Major.pitches(62, &scale), [62, 66, 69]);
        let notes = vec![LoopNote {
            note: 60,
            velocity: 90,
            on: 14.0,
            off: 2.0,
        }];
        let longer = resize_loop(&notes, 16, 32);
        assert_eq!(longer.len(), 2);
        assert_eq!((longer[1].on, longer[1].off), (30.0, 2.0));
        let shorter = resize_loop(
            &[LoopNote {
                note: 60,
                velocity: 90,
                on: 20.0,
                off: 22.0,
            }],
            32,
            16,
        );
        assert!(shorter.is_empty());
        assert_eq!(drum_name(36), Some("Kick"));
        assert_eq!(drum_name(81), Some("Open triangle"));
        assert_eq!(drum_name(82), None);
    }
}
