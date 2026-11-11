//! Scales and the mapping from the seven on-screen keys to MIDI notes.

/// The scales a beginner can lock the keyboard to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scale {
    #[default]
    MinorPentatonic,
    MajorPentatonic,
    Major,
    NaturalMinor,
    Blues,
    Dorian,
    HarmonicMinor,
}

impl Scale {
    pub const ALL: [Scale; 7] = [
        Scale::MinorPentatonic,
        Scale::MajorPentatonic,
        Scale::Major,
        Scale::NaturalMinor,
        Scale::Blues,
        Scale::Dorian,
        Scale::HarmonicMinor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scale::MinorPentatonic => "Minor pentatonic",
            Scale::MajorPentatonic => "Major pentatonic",
            Scale::Major => "Major",
            Scale::NaturalMinor => "Natural minor",
            Scale::Blues => "Blues",
            Scale::Dorian => "Dorian",
            Scale::HarmonicMinor => "Harmonic minor",
        }
    }

    /// Semitone offsets of the scale degrees within one octave.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        }
    }

    pub fn index(self) -> usize {
        Scale::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Scale {
        Scale::ALL[i % Scale::ALL.len()]
    }
}

/// Note letters for the 12 pitch classes; `true` means "flat" (the UI draws
/// the flat sign itself, e.g. E♭ and B♭).
pub const NOTE_NAMES: [(&str, bool); 12] = [
    ("C", false),
    ("D", true),
    ("D", false),
    ("E", true),
    ("E", false),
    ("F", false),
    ("G", true),
    ("G", false),
    ("A", true),
    ("A", false),
    ("B", true),
    ("B", false),
];

/// Letter and flat flag for a MIDI note or pitch class.
pub fn note_name(note: u8) -> (&'static str, bool) {
    NOTE_NAMES[(note % 12) as usize]
}

/// Plain-text note name such as `Eb`.
pub fn note_name_ascii(note: u8) -> String {
    let (letter, flat) = note_name(note);
    if flat {
        format!("{letter}b")
    } else {
        letter.to_owned()
    }
}

pub const MIN_OCTAVE: i8 = 1;
pub const MAX_OCTAVE: i8 = 6;
pub const DEFAULT_OCTAVE: i8 = 3;

/// How on-screen keys map to notes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyMap {
    /// Root pitch class, 0 = C .. 11 = B.
    pub root: u8,
    pub scale: Scale,
    /// When locked, keys play scale degrees; otherwise consecutive semitones.
    pub locked: bool,
    /// Octave of the lowest key, 1..=6 (octave 3 starts at C3 = MIDI 48).
    pub octave: i8,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self {
            root: 0,
            scale: Scale::MinorPentatonic,
            locked: true,
            octave: DEFAULT_OCTAVE,
        }
    }
}

impl KeyMap {
    /// MIDI note for key index `key` (0 is the lowest on-screen key; indices
    /// past the last key keep climbing, which the arpeggiator uses).
    pub fn note(&self, key: i32) -> u8 {
        let base = 12 * (i32::from(self.octave.clamp(MIN_OCTAVE, MAX_OCTAVE)) + 1)
            + i32::from(self.root % 12);
        let semis = if self.locked {
            let iv = self.scale.intervals();
            let n = iv.len() as i32;
            12 * key.div_euclid(n) + i32::from(iv[key.rem_euclid(n) as usize])
        } else {
            key
        };
        (base + semis).clamp(0, 127) as u8
    }

    /// True when the key plays the root note (in any octave).
    pub fn is_root(&self, key: i32) -> bool {
        self.note(key) % 12 == self.root % 12
    }
}

/// Parse a note name such as `C4`, `Eb3`, `F#2` or a MIDI number.
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u8>() {
        return (n <= 127).then_some(n);
    }
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut pc: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let rest = if let Some(r) = rest.strip_prefix('#') {
        pc += 1;
        r.to_owned()
    } else if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
        pc -= 1;
        r.to_owned()
    } else {
        rest
    };
    let octave: i32 = rest.parse().ok()?;
    let note = 12 * (octave + 1) + pc;
    (0..=127).contains(&note).then_some(note as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_have_correct_semitones() {
        assert_eq!(Scale::MinorPentatonic.intervals(), &[0, 3, 5, 7, 10]);
        assert_eq!(Scale::MajorPentatonic.intervals(), &[0, 2, 4, 7, 9]);
        assert_eq!(Scale::Major.intervals(), &[0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(Scale::NaturalMinor.intervals(), &[0, 2, 3, 5, 7, 8, 10]);
        assert_eq!(Scale::Blues.intervals(), &[0, 3, 5, 6, 7, 10]);
        assert_eq!(Scale::Dorian.intervals(), &[0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(Scale::HarmonicMinor.intervals(), &[0, 2, 3, 5, 7, 8, 11]);
    }

    #[test]
    fn default_keys_are_c_minor_pentatonic_from_c3() {
        let km = KeyMap::default();
        let notes: Vec<u8> = (0..7).map(|k| km.note(k)).collect();
        // C3 Eb3 F3 G3 Bb3 C4 Eb4
        assert_eq!(notes, vec![48, 51, 53, 55, 58, 60, 63]);
        let roots: Vec<bool> = (0..7).map(|k| km.is_root(k)).collect();
        assert_eq!(roots, vec![true, false, false, false, false, true, false]);
    }

    #[test]
    fn every_locked_key_is_in_the_scale_for_all_roots_and_scales() {
        for scale in Scale::ALL {
            for root in 0..12u8 {
                for octave in MIN_OCTAVE..=MAX_OCTAVE {
                    let km = KeyMap {
                        root,
                        scale,
                        locked: true,
                        octave,
                    };
                    let mut prev = None;
                    for key in 0..14 {
                        let note = km.note(key);
                        let rel = (note + 120 - root) % 12;
                        assert!(
                            scale.intervals().contains(&rel),
                            "{scale:?} root {root} key {key}"
                        );
                        if let Some(p) = prev {
                            assert!(note > p, "keys must ascend");
                        }
                        prev = Some(note);
                    }
                    assert_eq!(km.note(0), 12 * (octave as u8 + 1) + root);
                }
            }
        }
    }

    #[test]
    fn unlocked_keys_are_consecutive_semitones() {
        let km = KeyMap {
            root: 2,
            scale: Scale::Major,
            locked: false,
            octave: 4,
        };
        let notes: Vec<u8> = (0..7).map(|k| km.note(k)).collect();
        assert_eq!(notes, vec![62, 63, 64, 65, 66, 67, 68]);
    }

    #[test]
    fn harmonic_minor_in_a() {
        let km = KeyMap {
            root: 9,
            scale: Scale::HarmonicMinor,
            locked: true,
            octave: 3,
        };
        let notes: Vec<u8> = (0..8).map(|k| km.note(k)).collect();
        // A3 B3 C4 D4 E4 F4 G#4 A4
        assert_eq!(notes, vec![57, 59, 60, 62, 64, 65, 68, 69]);
    }

    #[test]
    fn parses_note_names() {
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("A4"), Some(69));
        assert_eq!(parse_note("Eb3"), Some(51));
        assert_eq!(parse_note("F#2"), Some(42));
        assert_eq!(parse_note("60"), Some(60));
        assert_eq!(parse_note("H2"), None);
    }
}
