//! Chord symbols (as written on tab sites) and how to voice them on a
//! guitar or a piano.

/// A parsed chord symbol such as `F#m7`, `Cadd9` or `G/B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    /// Root pitch class, 0 = C .. 11 = B.
    pub root: u8,
    /// Bit `i` set = the chord contains the interval of `i` semitones above
    /// the root (0..=23, so 9ths, 11ths and 13ths stay distinct).
    pub tones: u32,
    /// Slash bass pitch class (`G/B` → B).
    pub bass: Option<u8>,
    /// The spelling after the root, without the slash bass (`m7`, `sus4`).
    suffix: [u8; 12],
    suffix_len: u8,
    /// The root was written with a flat.
    pub flat: bool,
}

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

pub fn pitch_name(pc: u8, flats: bool) -> &'static str {
    if flats {
        FLAT_NAMES[usize::from(pc % 12)]
    } else {
        SHARP_NAMES[usize::from(pc % 12)]
    }
}

/// Parse a note name at the start of `s` (`C`, `F#`, `Bb`, `E♭`); returns
/// the pitch class, whether it was flat, and the bytes consumed.
fn parse_note(s: &str) -> Option<(u8, bool, usize)> {
    let mut chars = s.char_indices();
    let (_, letter) = chars.next()?;
    let pc: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut used = letter.len_utf8();
    let rest = &s[used..];
    let (pc, flat) = if let Some(c) = rest.chars().next().filter(|c| ['#', '♯'].contains(c)) {
        used += c.len_utf8();
        (pc + 1, false)
    } else if let Some(c) = rest.chars().next().filter(|c| ['b', '♭'].contains(c)) {
        used += c.len_utf8();
        (pc - 1, true)
    } else {
        (pc, false)
    };
    Some((pc.rem_euclid(12) as u8, flat, used))
}

impl Chord {
    /// Parse a chord symbol. Returns `None` for anything that is not one
    /// (so lyric words are rejected).
    pub fn parse(token: &str) -> Option<Chord> {
        let token = token.trim();
        let (root, flat, used) = parse_note(token)?;
        let rest = &token[used..];
        let (suffix, bass) = match rest.split_once('/') {
            Some((s, b)) => {
                let (bpc, _, bused) = parse_note(b)?;
                if bused != b.len() {
                    return None;
                }
                (s, Some(bpc))
            }
            None => (rest, None),
        };
        let tones = suffix_tones(suffix)?;
        let bytes = suffix.as_bytes();
        if bytes.len() > 12 {
            return None;
        }
        let mut buf = [0u8; 12];
        buf[..bytes.len()].copy_from_slice(bytes);
        Some(Chord {
            root,
            tones,
            bass,
            suffix: buf,
            suffix_len: bytes.len() as u8,
            flat,
        })
    }

    pub fn suffix(&self) -> &str {
        std::str::from_utf8(&self.suffix[..usize::from(self.suffix_len)]).unwrap_or("")
    }

    /// The chord moved by `semitones`.
    pub fn transposed(&self, semitones: i32) -> Chord {
        let shift = |pc: u8| (i32::from(pc) + semitones).rem_euclid(12) as u8;
        Chord {
            root: shift(self.root),
            bass: self.bass.map(shift),
            ..*self
        }
    }

    /// Display name, spelling accidentals with flats or sharps.
    pub fn name(&self, flats: bool) -> String {
        let mut s = String::from(pitch_name(self.root, flats));
        s.push_str(self.suffix());
        if let Some(b) = self.bass {
            s.push('/');
            s.push_str(pitch_name(b, flats));
        }
        s
    }

    /// Distinct pitch classes of the chord (root first), at most 6.
    pub fn pitch_classes(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(6);
        for i in 0..24u8 {
            if self.tones >> i & 1 == 1 {
                let pc = (self.root + i) % 12;
                if !out.contains(&pc) {
                    out.push(pc);
                }
            }
        }
        out.truncate(6);
        out
    }

    fn has(&self, interval: u8) -> bool {
        self.tones >> interval & 1 == 1
    }
}

/// Interval set for a chord suffix, or `None` if it is not a chord.
fn suffix_tones(s: &str) -> Option<u32> {
    let mut third: Option<u8> = Some(4);
    let mut fifth: Option<u8> = Some(7);
    let mut seventh: Option<u8> = None;
    let mut extra: u32 = 0;
    let mut major_seventh = false;
    let mut minor = false;
    let mut rest = s;

    // Tokens are matched longest first.
    const TOKENS: [&str; 52] = [
        "maj13", "maj11", "maj9", "maj7", "mmaj7", "mMaj7", "mM7", "min7", "min9", "min", "dim7",
        "dim", "aug", "sus2", "sus4", "sus", "add9", "add2", "add11", "add4", "add13", "add6",
        "m7b5", "ø7", "ø", "°7", "°", "Δ7", "Δ", "M7", "M9", "maj", "M", "m", "-", "+", "b5", "#5",
        "b9", "#9", "#11", "b13", "13", "11", "9", "7", "6", "5", "4", "2", "(", ")",
    ];
    while !rest.is_empty() {
        let Some(tok) = TOKENS.iter().find(|t| rest.starts_with(**t)) else {
            // Separators sometimes found inside chord symbols.
            if rest.starts_with(',') || rest.starts_with(' ') {
                rest = &rest[1..];
                continue;
            }
            return None;
        };
        rest = &rest[tok.len()..];
        match *tok {
            "maj" | "M" | "Δ" => major_seventh = true,
            "maj7" | "M7" | "Δ7" => {
                seventh = Some(11);
            }
            "maj9" | "M9" => {
                seventh = Some(11);
                extra |= 1 << 14;
            }
            "maj11" => {
                seventh = Some(11);
                extra |= (1 << 14) | (1 << 17);
            }
            "maj13" => {
                seventh = Some(11);
                extra |= (1 << 14) | (1 << 21);
            }
            "m" | "min" | "-" => {
                minor = true;
                third = Some(3);
            }
            "min7" => {
                minor = true;
                third = Some(3);
                seventh = Some(10);
            }
            "min9" => {
                minor = true;
                third = Some(3);
                seventh = Some(10);
                extra |= 1 << 14;
            }
            "mmaj7" | "mMaj7" | "mM7" => {
                minor = true;
                third = Some(3);
                seventh = Some(11);
            }
            "dim" | "°" => {
                third = Some(3);
                fifth = Some(6);
            }
            "dim7" | "°7" => {
                third = Some(3);
                fifth = Some(6);
                seventh = Some(9);
            }
            "m7b5" | "ø" | "ø7" => {
                third = Some(3);
                fifth = Some(6);
                seventh = Some(10);
            }
            "aug" | "+" | "#5" => fifth = Some(8),
            "b5" => fifth = Some(6),
            "sus2" | "2" if seventh.is_none() && extra == 0 => third = Some(2),
            "sus4" | "sus" | "4" => third = Some(5),
            "2" => extra |= 1 << 2,
            "5" => third = None,
            "6" | "add6" => extra |= 1 << 9,
            "7" => seventh = Some(if major_seventh { 11 } else { 10 }),
            "9" => {
                seventh.get_or_insert(if major_seventh { 11 } else { 10 });
                extra |= 1 << 14;
            }
            "11" => {
                seventh.get_or_insert(if major_seventh { 11 } else { 10 });
                extra |= (1 << 14) | (1 << 17);
            }
            "13" => {
                seventh.get_or_insert(if major_seventh { 11 } else { 10 });
                extra |= (1 << 14) | (1 << 21);
            }
            "add9" | "add2" => extra |= 1 << 14,
            "add11" | "add4" => extra |= 1 << 17,
            "add13" => extra |= 1 << 21,
            "b9" => extra |= 1 << 13,
            "#9" => extra |= 1 << 15,
            "#11" => extra |= 1 << 18,
            "b13" => extra |= 1 << 20,
            _ => {} // brackets
        }
    }
    // "M" alone (major triad) or "maj" alone: a plain major chord.
    let _ = minor;
    let mut tones = 1u32;
    if let Some(t) = third {
        tones |= 1 << t;
    }
    if let Some(f) = fifth {
        tones |= 1 << f;
    }
    if let Some(s7) = seventh {
        tones |= 1 << s7;
    }
    Some(tones | extra)
}

// ---------------------------------------------------------------------------
// Guitar

/// Open strings of a standard-tuned guitar, low E to high E.
pub const GUITAR_STRINGS: [u8; 6] = [40, 45, 50, 55, 59, 64];

/// Frets per string (-1 = muted), low E to high E.
type Shape = [i8; 6];

const fn m(intervals: &[u8]) -> u32 {
    let mut mask = 0u32;
    let mut i = 0;
    while i < intervals.len() {
        mask |= 1 << intervals[i];
        i += 1;
    }
    mask
}

const MAJ: u32 = m(&[0, 4, 7]);
const MIN: u32 = m(&[0, 3, 7]);
const DOM7: u32 = m(&[0, 4, 7, 10]);
const MIN7: u32 = m(&[0, 3, 7, 10]);
const MAJ7: u32 = m(&[0, 4, 7, 11]);
const SUS2: u32 = m(&[0, 2, 7]);
const SUS4: u32 = m(&[0, 5, 7]);
const ADD9: u32 = m(&[0, 4, 7, 14]);
const DIM: u32 = m(&[0, 3, 6]);
const M7B5: u32 = m(&[0, 3, 6, 10]);

/// Well-known open chord shapes.
const OPEN: [(u8, u32, Shape); 33] = [
    (0, MAJ, [-1, 3, 2, 0, 1, 0]),
    (0, DOM7, [-1, 3, 2, 3, 1, 0]),
    (0, MAJ7, [-1, 3, 2, 0, 0, 0]),
    (0, ADD9, [-1, 3, 2, 0, 3, 0]),
    (2, MAJ, [-1, -1, 0, 2, 3, 2]),
    (2, MIN, [-1, -1, 0, 2, 3, 1]),
    (2, DOM7, [-1, -1, 0, 2, 1, 2]),
    (2, MAJ7, [-1, -1, 0, 2, 2, 2]),
    (2, MIN7, [-1, -1, 0, 2, 1, 1]),
    (2, SUS2, [-1, -1, 0, 2, 3, 0]),
    (2, SUS4, [-1, -1, 0, 2, 3, 3]),
    (4, MAJ, [0, 2, 2, 1, 0, 0]),
    (4, MIN, [0, 2, 2, 0, 0, 0]),
    (4, DOM7, [0, 2, 0, 1, 0, 0]),
    (4, MIN7, [0, 2, 0, 0, 0, 0]),
    (4, MAJ7, [0, 2, 1, 1, 0, 0]),
    (4, SUS4, [0, 2, 2, 2, 0, 0]),
    (5, MAJ, [1, 3, 3, 2, 1, 1]),
    (5, MAJ7, [-1, -1, 3, 2, 1, 0]),
    (5, MIN, [1, 3, 3, 1, 1, 1]),
    (7, MAJ, [3, 2, 0, 0, 0, 3]),
    (7, DOM7, [3, 2, 0, 0, 0, 1]),
    (7, MAJ7, [3, 2, 0, 0, 0, 2]),
    (7, SUS4, [3, 3, 0, 0, 1, 3]),
    (9, MAJ, [-1, 0, 2, 2, 2, 0]),
    (9, MIN, [-1, 0, 2, 2, 1, 0]),
    (9, DOM7, [-1, 0, 2, 0, 2, 0]),
    (9, MIN7, [-1, 0, 2, 0, 1, 0]),
    (9, MAJ7, [-1, 0, 2, 1, 2, 0]),
    (9, SUS2, [-1, 0, 2, 2, 0, 0]),
    (9, SUS4, [-1, 0, 2, 2, 3, 0]),
    (11, DOM7, [-1, 2, 1, 2, 0, 2]),
    (0, SUS4, [-1, 3, 3, 0, 1, 1]),
];

/// Movable barre shapes: (chord type, root on low E (true) or A, offsets
/// from the barre fret; -1 = muted).
const BARRE: [(u32, bool, Shape); 17] = [
    (MAJ, true, [0, 2, 2, 1, 0, 0]),
    (MIN, true, [0, 2, 2, 0, 0, 0]),
    (DOM7, true, [0, 2, 0, 1, 0, 0]),
    (MIN7, true, [0, 2, 0, 0, 0, 0]),
    (MAJ7, true, [0, -1, 1, 1, 0, -1]),
    (SUS4, true, [0, 2, 2, 2, 0, 0]),
    (MAJ, false, [-1, 0, 2, 2, 2, 0]),
    (MIN, false, [-1, 0, 2, 2, 1, 0]),
    (DOM7, false, [-1, 0, 2, 0, 2, 0]),
    (MIN7, false, [-1, 0, 2, 0, 1, 0]),
    (MAJ7, false, [-1, 0, 2, 1, 2, 0]),
    (SUS2, false, [-1, 0, 2, 2, 0, 0]),
    (SUS4, false, [-1, 0, 2, 2, 3, 0]),
    (DIM, false, [-1, 0, 1, 2, 1, -1]),
    (M7B5, false, [-1, 0, 1, 0, 1, -1]),
    (ADD9, false, [-1, 0, 2, 2, 0, 0]),
    (ADD9, true, [0, 2, 2, 1, 0, 2]),
];

fn shape_notes(shape: &Shape, barre: i8) -> Vec<u8> {
    shape
        .iter()
        .zip(GUITAR_STRINGS)
        .filter(|(f, _)| **f >= 0)
        .map(|(f, open)| (i32::from(open) + i32::from(*f + barre)) as u8)
        .collect()
}

/// MIDI notes of a guitar voicing, low to high: an open shape if one
/// exists, else a barre shape, else a spread voicing of the chord tones.
pub fn guitar_voicing(chord: &Chord) -> Vec<u8> {
    let tones = chord.tones;
    let mut notes = if let Some((_, _, shape)) = OPEN
        .iter()
        .find(|(r, t, _)| *r == chord.root && *t == tones)
    {
        shape_notes(shape, 0)
    } else {
        let mut best: Option<(i8, Vec<u8>)> = None;
        for (t, on_e, shape) in BARRE.iter() {
            if *t != tones {
                continue;
            }
            let open = if *on_e { 4 } else { 9 };
            let mut fret = (i32::from(chord.root) - open).rem_euclid(12) as i8;
            if fret == 0 {
                fret = 12;
            }
            if fret <= 10 && best.as_ref().is_none_or(|(f, _)| fret < *f) {
                best = Some((fret, shape_notes(shape, fret)));
            }
        }
        match best {
            Some((_, notes)) => notes,
            None => spread_voicing(chord),
        }
    };
    if let Some(bass) = chord.bass {
        // Put the slash bass under the shape (dropping the old lowest note).
        if notes.first().is_some_and(|n| n % 12 != bass) {
            notes.remove(0);
            let top = notes.first().copied().unwrap_or(52);
            let mut b = i32::from(top) - 1;
            while b.rem_euclid(12) as u8 != bass {
                b -= 1;
            }
            notes.insert(0, b.max(40) as u8);
        }
    }
    notes
}

/// Chord tones spread over a guitar's range: bass root, then the other
/// tones going up, at least a minor third apart.
fn spread_voicing(chord: &Chord) -> Vec<u8> {
    // The lowest root a guitar can play (E2..D#3).
    let bass = 40 + (i32::from(chord.root) - 4).rem_euclid(12);
    let mut notes = vec![bass as u8];
    let mut last = bass;
    let pcs = chord.pitch_classes();
    let mut wanted: Vec<u8> = pcs.iter().skip(1).copied().collect();
    wanted.push(chord.root);
    for pc in wanted.iter().cycle().take(8) {
        let mut n = last + 3;
        while n.rem_euclid(12) as u8 != *pc {
            n += 1;
        }
        if n > 76 || notes.len() >= 6 {
            break;
        }
        notes.push(n as u8);
        last = n;
    }
    notes
}

// ---------------------------------------------------------------------------
// Piano

/// A piano voicing: a bass note (root or slash bass) and a close chord in
/// the middle of the keyboard, choosing the inversion that moves least from
/// `previous` (the right hand of the last chord).
pub fn piano_voicing(chord: &Chord, previous: Option<&[u8]>) -> Vec<u8> {
    let bass_pc = chord.bass.unwrap_or(chord.root);
    let bass = 36 + (i32::from(bass_pc)).rem_euclid(12);
    let bass = if bass < 40 { bass + 12 } else { bass } as u8;

    let mut pcs = chord.pitch_classes();
    // Four notes at most in the right hand: drop the fifth, then the root.
    while pcs.len() > 4 {
        let fifth = (chord.root + 7) % 12;
        if let Some(i) = pcs.iter().position(|&p| p == fifth && chord.has(7)) {
            pcs.remove(i);
        } else {
            pcs.remove(0);
        }
    }
    let mut best: Option<(i32, Vec<u8>)> = None;
    for start in 0..pcs.len() {
        // Inversion `start`, stacked upwards from the first note >= 53.
        let mut notes = Vec::with_capacity(pcs.len());
        let mut n = 53 + (i32::from(pcs[start]) - 53).rem_euclid(12);
        notes.push(n);
        for k in 1..pcs.len() {
            let pc = pcs[(start + k) % pcs.len()];
            n += 1;
            while n.rem_euclid(12) as u8 != pc {
                n += 1;
            }
            notes.push(n);
        }
        let cost = match previous {
            Some(prev) if !prev.is_empty() => {
                let centre = |v: &[i32]| v.iter().sum::<i32>() as f32 / v.len() as f32;
                let p: Vec<i32> = prev.iter().map(|&x| i32::from(x)).collect();
                (centre(&notes) - centre(&p)).abs() as i32 * 4
                    + notes
                        .iter()
                        .map(|a| p.iter().map(|b| (a - b).abs()).min().unwrap_or(0))
                        .sum::<i32>()
            }
            _ => (notes[0] - 60).abs(),
        };
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, notes.iter().map(|&x| x as u8).collect()));
        }
    }
    let mut out = vec![bass];
    out.extend(best.map(|(_, v)| v).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcs(name: &str) -> Vec<u8> {
        let mut v = Chord::parse(name)
            .unwrap_or_else(|| panic!("{name}"))
            .pitch_classes();
        v.sort_unstable();
        v
    }

    #[test]
    fn parses_common_chord_symbols() {
        assert_eq!(pcs("C"), vec![0, 4, 7]);
        assert_eq!(pcs("Am"), vec![0, 4, 9]);
        assert_eq!(pcs("F#m7"), vec![1, 4, 6, 9]);
        assert_eq!(pcs("Bb"), vec![2, 5, 10]);
        assert_eq!(pcs("Cmaj7"), vec![0, 4, 7, 11]);
        assert_eq!(pcs("CM7"), vec![0, 4, 7, 11]);
        assert_eq!(pcs("G7"), vec![2, 5, 7, 11]);
        assert_eq!(pcs("Dsus4"), vec![2, 7, 9]);
        assert_eq!(pcs("Dsus2"), vec![2, 4, 9]);
        assert_eq!(pcs("Cadd9"), vec![0, 2, 4, 7]);
        assert_eq!(pcs("Bdim"), vec![2, 5, 11]);
        assert_eq!(pcs("Bm7b5"), vec![2, 5, 9, 11]);
        assert_eq!(pcs("Caug"), vec![0, 4, 8]);
        assert_eq!(pcs("E5"), vec![4, 11]);
        assert_eq!(pcs("A7sus4"), vec![2, 4, 7, 9]);
        assert_eq!(pcs("Em9"), vec![2, 4, 6, 7, 11]);
        let gb = Chord::parse("G/B").unwrap();
        assert_eq!((gb.root, gb.bass), (7, Some(11)));
        assert_eq!(Chord::parse("E♭m").unwrap().root, 3);
    }

    #[test]
    fn rejects_words() {
        for w in [
            "Amazing", "And", "Every", "Chorus", "Bridge", "Do", "x2", "Capo", "Ab3c", "C/X",
        ] {
            assert!(Chord::parse(w).is_none(), "{w} is not a chord");
        }
    }

    #[test]
    fn transposes_and_names() {
        let c = Chord::parse("F#m7").unwrap();
        assert_eq!(c.transposed(2).name(false), "G#m7");
        assert_eq!(c.transposed(2).name(true), "Abm7");
        let s = Chord::parse("D/F#").unwrap();
        assert_eq!(s.transposed(-2).name(false), "C/E");
    }

    #[test]
    fn guitar_shapes_are_the_familiar_ones() {
        // G = 320003: G2 B2 D3 G3 B3 G4.
        assert_eq!(
            guitar_voicing(&Chord::parse("G").unwrap()),
            vec![43, 47, 50, 55, 59, 67]
        );
        // Am = x02210.
        assert_eq!(
            guitar_voicing(&Chord::parse("Am").unwrap()),
            vec![45, 52, 57, 60, 64]
        );
        // Bm: A-shape barre at fret 2 (x24432).
        assert_eq!(
            guitar_voicing(&Chord::parse("Bm").unwrap()),
            vec![47, 54, 59, 62, 66]
        );
        // Every voicing holds only chord tones, within a guitar's range.
        for name in [
            "C#m", "Ebmaj7", "F#7", "G#m7", "Bb", "Cm7b5", "E7b9", "Dm9", "A/C#",
        ] {
            let c = Chord::parse(name).unwrap();
            let v = guitar_voicing(&c);
            assert!(v.len() >= 3, "{name}: {v:?}");
            let allowed: Vec<u8> = c.pitch_classes().into_iter().chain(c.bass).collect();
            for n in &v {
                assert!((40..=88).contains(n), "{name}: {v:?}");
                assert!(allowed.contains(&(n % 12)), "{name}: {n} not in chord");
            }
            assert!(
                v.windows(2).all(|w| w[0] < w[1]),
                "{name}: {v:?} must ascend"
            );
        }
        // Slash bass ends up at the bottom.
        assert_eq!(guitar_voicing(&Chord::parse("A/C#").unwrap())[0] % 12, 1);
    }

    #[test]
    fn piano_voicings_move_smoothly() {
        let progression = ["C", "Am", "F", "G", "C"];
        let mut prev: Option<Vec<u8>> = None;
        for name in progression {
            let v = piano_voicing(&Chord::parse(name).unwrap(), prev.as_deref());
            let (bass, right) = (v[0], &v[1..]);
            assert!((40..52).contains(&bass), "{name} bass {bass}");
            assert!(
                right.iter().all(|n| (53..=80).contains(n)),
                "{name}: {right:?}"
            );
            if let Some(p) = &prev {
                let move_sum: i32 = right
                    .iter()
                    .map(|a| {
                        p.iter()
                            .map(|b| (i32::from(*a) - i32::from(*b)).abs())
                            .min()
                            .unwrap()
                    })
                    .sum();
                assert!(move_sum <= 6, "{name}: hands jumped ({move_sum})");
            }
            prev = Some(right.to_vec());
        }
        let slash = piano_voicing(&Chord::parse("C/E").unwrap(), None);
        assert_eq!(slash[0] % 12, 4);
    }
}
