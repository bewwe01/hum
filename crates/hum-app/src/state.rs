//! Everything the app remembers between launches, stored as JSON in the
//! app's private files directory.

use std::path::{Path, PathBuf};

use hum_engine::engine::{
    ATTACK_DEFAULT, PAD_X_DEFAULT, PAD_Y_DEFAULT, RELEASE_DEFAULT, VOLUME_DEFAULT,
};
use hum_engine::presets::{self, PRESETS};
use hum_engine::scale::{KeyMap, Scale, DEFAULT_OCTAVE, MAX_OCTAVE, MIN_OCTAVE};
use hum_engine::transport::{DEFAULT_BPM, DEFAULT_PATTERNS, MAX_BPM, MIN_BPM};
use serde::{Deserialize, Serialize};

use crate::audio::BufferMode;

const FILE_NAME: &str = "hum-state.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub version: u32,
    /// Sound name (names are stable; indices might not be).
    pub sound: String,
    /// Root note, 0 = C .. 11 = B.
    pub key: u8,
    pub scale: String,
    pub locked: bool,
    pub octave: i8,
    pub attack: f32,
    pub release: f32,
    pub reverb: f32,
    pub volume: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub bpm: f32,
    pub metronome: bool,
    /// Beat patterns (Kick, Snare, Hat, Clap); bit i = step i + 1.
    pub patterns: [u16; 4],
    pub buffer: BufferMode,
    pub show_note_names: bool,
    pub haptics: bool,
    pub keep_screen_on: bool,
}

impl Default for Saved {
    fn default() -> Self {
        let preset = &PRESETS[presets::DEFAULT_PRESET];
        Self {
            version: 1,
            sound: preset.name.to_owned(),
            key: 0,
            scale: Scale::MinorPentatonic.name().to_owned(),
            locked: true,
            octave: DEFAULT_OCTAVE,
            attack: ATTACK_DEFAULT,
            release: RELEASE_DEFAULT,
            reverb: preset.reverb,
            volume: VOLUME_DEFAULT,
            pad_x: PAD_X_DEFAULT,
            pad_y: PAD_Y_DEFAULT,
            bpm: DEFAULT_BPM,
            metronome: true,
            patterns: DEFAULT_PATTERNS,
            buffer: BufferMode::Auto,
            show_note_names: true,
            haptics: true,
            keep_screen_on: false,
        }
    }
}

fn unit(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

impl Saved {
    /// Clamp everything into its valid range (the file may be hand-edited
    /// or come from an older version).
    pub fn sanitized(mut self) -> Self {
        let d = Saved::default();
        if presets::find(&self.sound).is_none() {
            self.sound = d.sound.clone();
        }
        if !Scale::ALL.iter().any(|s| s.name() == self.scale) {
            self.scale = d.scale.clone();
        }
        self.key %= 12;
        self.octave = self.octave.clamp(MIN_OCTAVE, MAX_OCTAVE);
        self.attack = unit(self.attack, d.attack);
        self.release = unit(self.release, d.release);
        self.reverb = unit(self.reverb, d.reverb);
        self.volume = unit(self.volume, d.volume);
        self.pad_x = unit(self.pad_x, d.pad_x);
        self.pad_y = unit(self.pad_y, d.pad_y);
        self.bpm = if self.bpm.is_finite() {
            self.bpm.round().clamp(MIN_BPM, MAX_BPM)
        } else {
            d.bpm
        };
        self
    }

    pub fn sound_index(&self) -> usize {
        presets::find(&self.sound).unwrap_or(presets::DEFAULT_PRESET)
    }

    pub fn scale(&self) -> Scale {
        Scale::ALL
            .iter()
            .copied()
            .find(|s| s.name() == self.scale)
            .unwrap_or_default()
    }

    pub fn keymap(&self) -> KeyMap {
        KeyMap {
            root: self.key % 12,
            scale: self.scale(),
            locked: self.locked,
            octave: self.octave,
        }
    }

    pub fn from_json(text: &str) -> Self {
        serde_json::from_str::<Saved>(text)
            .unwrap_or_default()
            .sanitized()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

/// Loads and saves [`Saved`].
#[derive(Clone)]
pub struct Store {
    path: Option<PathBuf>,
}

impl Store {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            path: dir.map(|d| d.join(FILE_NAME)),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn load(&self) -> Saved {
        let Some(path) = &self.path else {
            return Saved::default();
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Saved::from_json(&text),
            Err(_) => Saved::default(),
        }
    }

    pub fn save(&self, saved: &Saved) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write to a temporary file first so a crash cannot corrupt the state.
        let tmp = path.with_extension("json.tmp");
        let result =
            std::fs::write(&tmp, saved.to_json()).and_then(|()| std::fs::rename(&tmp, path));
        if let Err(err) = result {
            log::warn!("could not save state to {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_launch_defaults_match_the_spec() {
        let s = Saved::default();
        assert_eq!(s.sound, "Warm Cloud");
        assert_eq!(s.key, 0);
        assert_eq!(s.scale(), Scale::MinorPentatonic);
        assert!(s.locked);
        assert_eq!(s.octave, 3);
        assert_eq!(s.bpm, 110.0);
        assert_eq!(
            (s.attack, s.release, s.reverb, s.volume),
            (0.2, 0.6, 0.45, 0.75)
        );
        assert!(s.show_note_names && s.haptics && !s.keep_screen_on);
        assert_eq!(s.buffer, BufferMode::Auto);
        assert_eq!(s.patterns, DEFAULT_PATTERNS);
    }

    #[test]
    fn round_trips_through_json() {
        let s = Saved {
            sound: "Acid Squelch".to_owned(),
            key: 3,
            scale: Scale::Dorian.name().to_owned(),
            locked: false,
            patterns: [1, 2, 3, 0xFFFF],
            buffer: BufferMode::Safe,
            bpm: 142.0,
            ..Saved::default()
        };
        assert_eq!(Saved::from_json(&s.to_json()), s);
    }

    #[test]
    fn bad_or_partial_files_fall_back_safely() {
        assert_eq!(Saved::from_json("not json"), Saved::default());
        let s = Saved::from_json(
            r#"{"sound":"Nope","key":40,"octave":99,"bpm":9000,"volume":-3,"scale":"Whatever"}"#,
        );
        assert_eq!(s.sound, "Warm Cloud");
        assert_eq!(s.key, 4);
        assert_eq!(s.octave, 6);
        assert_eq!(s.bpm, 160.0);
        assert_eq!(s.volume, 0.0);
        assert_eq!(s.scale(), Scale::MinorPentatonic);
        // Missing fields take their defaults.
        assert!(s.locked && s.haptics);
    }

    #[test]
    fn store_saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("hum-test-{}", std::process::id()));
        let store = Store::new(Some(dir.clone()));
        assert_eq!(store.load(), Saved::default());
        let s = Saved {
            sound: "Zap".to_owned(),
            ..Saved::default()
        };
        store.save(&s);
        assert_eq!(store.load(), s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
