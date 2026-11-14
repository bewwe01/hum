//! Chord sheets: reading the "chords over lyrics" text people copy from tab
//! websites (and ChordPro's inline `[G]lyrics`), and the saved song.
//!
//! Nothing is downloaded: the user copies a sheet in their browser and
//! pastes it here.

use hum_engine::chords::Chord;
use serde::{Deserialize, Serialize};

/// A saved song.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    pub title: String,
    /// The sheet exactly as pasted or typed.
    pub text: String,
    /// Semitones added to every chord (sound and names).
    pub transpose: i8,
    /// Capo fret: the chords are shapes, they sound this much higher.
    pub capo: i8,
    pub bpm: f32,
    /// Beats each chord lasts in auto mode.
    pub beats: u8,
    /// Auto mode pattern (index into the engine's patterns).
    pub pattern: u8,
    /// Play as piano (voice-led) instead of guitar.
    pub piano: bool,
}

impl Default for Song {
    fn default() -> Self {
        Self {
            title: String::new(),
            text: String::new(),
            transpose: 0,
            capo: 0,
            bpm: 90.0,
            beats: 4,
            pattern: 1,
            piano: false,
        }
    }
}

impl Song {
    /// A new song from pasted text: the title and capo come from the sheet.
    pub fn from_text(text: &str) -> Song {
        let text = clean(text);
        let sheet = Sheet::parse(&text);
        Song {
            title: sheet
                .title
                .clone()
                .unwrap_or_else(|| "Untitled song".to_owned()),
            capo: sheet.capo,
            text,
            ..Song::default()
        }
    }

    pub fn sanitize(&mut self) {
        self.transpose = self.transpose.clamp(-11, 11);
        self.capo = self.capo.clamp(0, 11);
        if !(40.0..=200.0).contains(&self.bpm) {
            self.bpm = 90.0;
        }
        if ![1, 2, 4, 8].contains(&self.beats) {
            self.beats = 4;
        }
        self.pattern = self
            .pattern
            .min(hum_engine::engine::ACC_PATTERNS.len() as u8 - 1);
    }
}

/// Strip markup some sites put around chords and tabs, and tabs/CRs.
fn clean(text: &str) -> String {
    let mut s = text.replace("\r\n", "\n").replace('\r', "\n");
    for tag in ["[ch]", "[/ch]", "[tab]", "[/tab]"] {
        s = s.replace(tag, "");
    }
    s = s.replace('\t', "    ");
    s.trim_matches('\n').to_owned()
}

/// One line of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Blank,
    /// `[Verse 1]`, `Chorus:` and similar.
    Section(String),
    /// Chords at their columns, with the lyric line under them (if any).
    Chords {
        chords: Vec<(usize, Chord, String)>,
        lyric: Option<String>,
    },
    Lyric(String),
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub lines: Vec<Line>,
    pub title: Option<String>,
    pub capo: i8,
}

/// Tokens that may sit on a chord line without being chords.
fn is_filler(token: &str) -> bool {
    let t = token.trim_matches(|c| matches!(c, '(' | ')' | '|' | '-' | '/' | '*' | '.' | ':'));
    t.is_empty()
        || t.eq_ignore_ascii_case("n.c")
        || t.eq_ignore_ascii_case("nc")
        || ((t.starts_with('x') || t.starts_with('X'))
            && t.len() <= 3
            && t[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Strip brackets around a chord token, e.g. `(Am)`.
fn chord_token(token: &str) -> Option<Chord> {
    let t = token.trim_matches(|c| matches!(c, '(' | ')' | '|' | '*' | ','));
    Chord::parse(t)
}

/// The chords of a line, if every token is a chord (or filler) and there
/// is at least one chord.
fn chord_line(line: &str) -> Option<Vec<(usize, Chord, String)>> {
    let mut out = Vec::new();
    let mut col = 0;
    for (i, ch) in line.char_indices() {
        let _ = ch;
        if i < col {
            continue;
        }
        if line[i..].starts_with(char::is_whitespace) {
            continue;
        }
        let end = line[i..]
            .find(char::is_whitespace)
            .map_or(line.len(), |e| i + e);
        let token = &line[i..end];
        col = end;
        let column = line[..i].chars().count();
        if let Some(chord) = chord_token(token) {
            out.push((column, chord, token.to_owned()));
        } else if !is_filler(token) {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn section(line: &str) -> Option<String> {
    let t = line.trim();
    if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
        let inner = &t[1..t.len() - 1];
        if Chord::parse(inner).is_none() {
            return Some(inner.trim().to_owned());
        }
    }
    const NAMES: [&str; 10] = [
        "intro",
        "verse",
        "chorus",
        "bridge",
        "outro",
        "pre-chorus",
        "interlude",
        "solo",
        "refrain",
        "instrumental",
    ];
    let lower = t.trim_end_matches(':').to_ascii_lowercase();
    let word = lower.split_whitespace().next().unwrap_or("");
    if t.len() <= 24 && NAMES.contains(&word) && lower.split_whitespace().count() <= 3 {
        return Some(t.trim_end_matches(':').to_owned());
    }
    None
}

fn capo_of(line: &str) -> Option<i8> {
    let lower = line.to_ascii_lowercase();
    let at = lower.find("capo")?;
    let rest = &lower[at + 4..];
    if rest.contains("no capo") || rest.trim_start().starts_with("none") {
        return Some(0);
    }
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n: i8 = digits.parse().ok()?;
    (0..=11).contains(&n).then_some(n)
}

/// ChordPro-style inline chords: `[G]Hello [C]there`.
fn inline_chords(line: &str) -> Option<Line> {
    if !line.contains('[') {
        return None;
    }
    let mut chords = Vec::new();
    let mut lyric = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        lyric.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find(']')?;
        let token = &after[..close];
        let chord = Chord::parse(token)?;
        chords.push((lyric.chars().count(), chord, token.to_owned()));
        rest = &after[close + 1..];
    }
    lyric.push_str(rest);
    let lyric = lyric.trim_end().to_owned();
    Some(Line::Chords {
        chords,
        lyric: (!lyric.trim().is_empty()).then_some(lyric),
    })
}

impl Sheet {
    pub fn parse(text: &str) -> Sheet {
        let raw: Vec<&str> = text.lines().map(str::trim_end).collect();
        let mut sheet = Sheet::default();
        let mut i = 0;
        while i < raw.len() {
            let line = raw[i];
            i += 1;
            if line.trim().is_empty() {
                if sheet.lines.last() != Some(&Line::Blank) && !sheet.lines.is_empty() {
                    sheet.lines.push(Line::Blank);
                }
                continue;
            }
            if let Some(capo) = capo_of(line) {
                if chord_line(line).is_none() {
                    sheet.capo = capo;
                    continue;
                }
            }
            if let Some(chords) = chord_line(line) {
                // The next line is its lyric unless it is chords, a section
                // or blank.
                let lyric = raw.get(i).and_then(|next| {
                    let next_is_text = !next.trim().is_empty()
                        && chord_line(next).is_none()
                        && section(next).is_none();
                    next_is_text.then(|| next.to_string())
                });
                if lyric.is_some() {
                    i += 1;
                }
                sheet.lines.push(Line::Chords { chords, lyric });
                continue;
            }
            if let Some(name) = section(line) {
                sheet.lines.push(Line::Section(name));
                continue;
            }
            if let Some(l) = inline_chords(line) {
                sheet.lines.push(l);
                continue;
            }
            if sheet.title.is_none() && sheet.lines.is_empty() && line.trim().chars().count() <= 60
            {
                sheet.title = Some(line.trim().to_owned());
                continue;
            }
            sheet.lines.push(Line::Lyric(line.to_owned()));
        }
        while sheet.lines.last() == Some(&Line::Blank) {
            sheet.lines.pop();
        }
        sheet
    }

    /// Every chord in playing order: (line index, chord index, chord).
    pub fn sequence(&self) -> Vec<(usize, usize, Chord)> {
        let mut out = Vec::new();
        for (li, line) in self.lines.iter().enumerate() {
            if let Line::Chords { chords, .. } = line {
                for (ci, (_, chord, _)) in chords.iter().enumerate() {
                    out.push((li, ci, *chord));
                }
            }
        }
        out
    }

    /// The distinct chords, in order of first appearance.
    pub fn distinct(&self) -> Vec<Chord> {
        let mut out: Vec<Chord> = Vec::new();
        for (_, _, c) in self.sequence() {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// Whether the sheet spells its chords with flats.
    pub fn uses_flats(&self) -> bool {
        self.sequence().iter().any(|(_, _, c)| c.flat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = "Favourite Chords Demo\r\n\
Capo 2\n\
\n\
[Intro]\n\
G  D  Em  C\n\
\n\
[Verse 1]\n\
G              D\n\
Walking down the road\n\
Em         C/G     x2\n\
Singing all along\n\
\n\
Chorus:\n\
[Am]Inline [F]chords here\n";

    #[test]
    fn reads_chords_over_lyrics() {
        let sheet = Sheet::parse(&clean(SHEET));
        assert_eq!(sheet.title.as_deref(), Some("Favourite Chords Demo"));
        assert_eq!(sheet.capo, 2);
        let names: Vec<String> = sheet
            .sequence()
            .iter()
            .map(|(_, _, c)| c.name(false))
            .collect();
        assert_eq!(
            names,
            ["G", "D", "Em", "C", "G", "D", "Em", "C/G", "Am", "F"]
        );
        assert!(sheet.lines.contains(&Line::Section("Verse 1".into())));
        assert!(sheet.lines.contains(&Line::Section("Chorus".into())));
        let lyric = sheet.lines.iter().find_map(|l| match l {
            Line::Chords {
                chords,
                lyric: Some(t),
            } if chords.len() == 2 => Some((chords[1].0, t.clone())),
            _ => None,
        });
        assert_eq!(lyric, Some((15, "Walking down the road".to_owned())));
        assert_eq!(sheet.distinct().len(), 7);
    }

    #[test]
    fn prose_is_not_mistaken_for_chords() {
        let sheet = Sheet::parse("A day in the life\nI read the news today\nAm I wrong\nA E");
        assert_eq!(sheet.sequence().len(), 2, "only the last line is chords");
    }

    #[test]
    fn song_from_text_takes_title_and_capo() {
        let song = Song::from_text(SHEET);
        assert_eq!(song.title, "Favourite Chords Demo");
        assert_eq!(song.capo, 2);
        assert!(!song.text.contains('\r'));
    }
}
