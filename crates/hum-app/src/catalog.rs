//! Every playable sound in one list: the 56 synth presets plus the
//! instruments and drum kits of the bundled SoundFont (GeneralUser GS).
//!
//! Sounds are saved by a stable key: synth sounds by their name (as in
//! earlier versions), SoundFont sounds as `gm:<bank>:<program>`.

use hum_engine::engine::{sf_sound, SoundId, SF_BASE};
use hum_engine::presets::{self, PRESETS};
use hum_engine::{Category, Wave};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Piano,
    Keys,
    Guitar,
    Bass,
    Strings,
    Wind,
    Synth,
    Drums,
    World,
    Fx,
}

impl Group {
    pub const ALL: [Group; 10] = [
        Group::Piano,
        Group::Keys,
        Group::Guitar,
        Group::Bass,
        Group::Strings,
        Group::Wind,
        Group::Synth,
        Group::Drums,
        Group::World,
        Group::Fx,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Piano => "Piano",
            Group::Keys => "Keys",
            Group::Guitar => "Guitar",
            Group::Bass => "Bass",
            Group::Strings => "Strings",
            Group::Wind => "Wind",
            Group::Synth => "Synth",
            Group::Drums => "Drums",
            Group::World => "World",
            Group::Fx => "FX",
        }
    }

    /// Waveform glyph shown for SoundFont instruments of this group.
    fn glyph(self) -> Wave {
        match self {
            Group::Piano | Group::Keys => Wave::Triangle,
            Group::Guitar | Group::Strings | Group::Synth => Wave::Saw,
            Group::Bass | Group::Wind => Wave::Sine,
            Group::World => Wave::Square,
            Group::Drums | Group::Fx => Wave::Noise,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Instrument {
    /// Saved identifier.
    pub key: String,
    pub name: String,
    pub group: Group,
    pub sound: SoundId,
    pub wave: Wave,
    /// Default REVERB knob position.
    pub reverb: f32,
    /// Preview with a low note.
    pub low: bool,
    pub drums: bool,
}

pub struct Catalog {
    pub items: Vec<Instrument>,
    pub soundfont_loaded: bool,
}

fn synth_group(c: Category) -> Group {
    match c {
        Category::Bass => Group::Bass,
        Category::Keys => Group::Keys,
        Category::Drums => Group::Drums,
        Category::Fx => Group::Fx,
        Category::Leads | Category::Pads | Category::Arps | Category::Retro => Group::Synth,
    }
}

/// GM program families (General MIDI level 1).
fn gm_group(bank: u16, program: u16) -> Group {
    if bank == 128 {
        return Group::Drums;
    }
    match program {
        0..=7 => Group::Piano,
        8..=23 => Group::Keys,
        24..=31 => Group::Guitar,
        32..=39 => Group::Bass,
        40..=55 => Group::Strings,
        56..=79 => Group::Wind,
        80..=95 => Group::Synth,
        104..=119 => Group::World,
        _ => Group::Fx,
    }
}

pub fn gm_key(bank: u16, program: u16) -> String {
    format!("gm:{bank}:{program}")
}

fn parse_gm(key: &str) -> Option<(u16, u16)> {
    let rest = key.strip_prefix("gm:")?;
    let (b, p) = rest.split_once(':')?;
    let (b, p): (u16, u16) = (b.parse().ok()?, p.parse().ok()?);
    (b <= 128 && p <= 127).then_some((b, p))
}

/// The engine sound for a saved key (works before the SoundFont loads).
pub fn sound_for_key(key: &str) -> Option<SoundId> {
    if let Some((b, p)) = parse_gm(key) {
        return Some(sf_sound(b, p));
    }
    presets::find(key).map(|i| i as SoundId)
}

pub fn is_valid_key(key: &str) -> bool {
    sound_for_key(key).is_some()
}

/// The saved key for an engine sound.
pub fn key_for_sound(sound: SoundId) -> String {
    if sound >= SF_BASE {
        let rel = sound - SF_BASE;
        gm_key(rel / 128, rel % 128)
    } else {
        PRESETS[usize::from(sound).min(PRESETS.len() - 1)]
            .name
            .to_owned()
    }
}

impl Catalog {
    /// The synth sounds only (until the SoundFont has loaded).
    pub fn new() -> Self {
        let items = PRESETS
            .iter()
            .enumerate()
            .map(|(i, p)| Instrument {
                key: p.name.to_owned(),
                name: p.name.to_owned(),
                group: synth_group(p.category),
                sound: i as SoundId,
                wave: p.main_wave(),
                reverb: p.reverb,
                low: p.category == Category::Bass,
                drums: p.category == Category::Drums,
            })
            .collect();
        let mut catalog = Self {
            items,
            soundfont_loaded: false,
        };
        catalog.sort();
        catalog
    }

    /// Order by group (Piano first); within a group, SoundFont instruments
    /// before synth sounds, otherwise keeping their original order.
    fn sort(&mut self) {
        let rank = |g: Group| Group::ALL.iter().position(|x| *x == g).unwrap_or(0);
        self.items
            .sort_by_key(|i| (rank(i.group), i.sound < SF_BASE));
    }

    /// Add the SoundFont's GM instruments (bank 0) and drum kits (bank 128),
    /// in program order, after the synth sounds.
    pub fn add_soundfont(&mut self, presets: &[(u16, u16, String)]) {
        if self.soundfont_loaded {
            return;
        }
        let mut list: Vec<&(u16, u16, String)> = presets
            .iter()
            .filter(|(bank, _, _)| *bank == 0 || *bank == 128)
            .collect();
        list.sort_by_key(|(bank, program, _)| (*bank, *program));
        for (bank, program, name) in list {
            let group = gm_group(*bank, *program);
            self.items.push(Instrument {
                key: gm_key(*bank, *program),
                name: name.trim().to_owned(),
                group,
                sound: sf_sound(*bank, *program),
                wave: group.glyph(),
                reverb: 0.3,
                low: group == Group::Bass,
                drums: group == Group::Drums,
            });
        }
        self.soundfont_loaded = true;
        self.sort();
    }

    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|i| i.key == key)
    }

    /// The instrument for a saved key; a placeholder if it is a SoundFont
    /// sound that has not loaded yet.
    pub fn get(&self, key: &str) -> Instrument {
        if let Some(i) = self.index_of(key) {
            return self.items[i].clone();
        }
        let (sound, group) = match parse_gm(key) {
            Some((b, p)) => (sf_sound(b, p), gm_group(b, p)),
            None => (presets::DEFAULT_PRESET as SoundId, Group::Synth),
        };
        Instrument {
            key: key.to_owned(),
            name: "Loading…".to_owned(),
            group,
            sound,
            wave: group.glyph(),
            reverb: 0.3,
            low: group == Group::Bass,
            drums: group == Group::Drums,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        assert_eq!(sound_for_key("Warm Cloud"), Some(7));
        assert_eq!(key_for_sound(7), "Warm Cloud");
        assert_eq!(sound_for_key("gm:0:24"), Some(sf_sound(0, 24)));
        assert_eq!(key_for_sound(sf_sound(128, 0)), "gm:128:0");
        assert!(!is_valid_key("gm:3:200") && !is_valid_key("Nope"));
    }

    #[test]
    fn groups_follow_general_midi() {
        assert_eq!(gm_group(0, 0), Group::Piano);
        assert_eq!(gm_group(0, 25), Group::Guitar);
        assert_eq!(gm_group(0, 41), Group::Strings);
        assert_eq!(gm_group(0, 73), Group::Wind);
        assert_eq!(gm_group(128, 0), Group::Drums);
        let mut c = Catalog::new();
        assert_eq!(c.items.len(), 56);
        c.add_soundfont(&[
            (0, 0, "Grand Piano".into()),
            (128, 0, "Standard".into()),
            (8, 4, "Variant".into()),
        ]);
        assert_eq!(c.items.len(), 58, "only banks 0 and 128 are listed");
        assert_eq!(c.get("gm:0:0").name, "Grand Piano");
        assert_eq!(c.get("gm:0:5").name, "Loading…");
    }
}
