//! Preset data model. A sound is plain data: the engine has no per-sound
//! code paths. The 56 built-in sounds live in [`crate::presets`].

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Leads,
    Pads,
    Bass,
    Keys,
    Arps,
    Fx,
    Retro,
    Drums,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Leads,
        Category::Pads,
        Category::Bass,
        Category::Keys,
        Category::Arps,
        Category::Fx,
        Category::Retro,
        Category::Drums,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Leads => "Leads",
            Category::Pads => "Pads",
            Category::Bass => "Bass",
            Category::Keys => "Keys",
            Category::Arps => "Arps",
            Category::Fx => "FX",
            Category::Retro => "Retro",
            Category::Drums => "Drums",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Sine,
    Triangle,
    Saw,
    /// Square / pulse; the duty cycle is [`Osc::width`].
    Square,
    Noise,
}

/// One oscillator layer.
#[derive(Clone, Copy, Debug)]
pub struct Osc {
    pub wave: Wave,
    /// Linear level; 0 switches the layer off.
    pub level: f32,
    /// Octave offset.
    pub octave: i8,
    /// Detune in cents.
    pub detune: f32,
    /// Extra frequency ratio (for inharmonic partials); 1 = none.
    pub ratio: f32,
    /// Pulse width for [`Wave::Square`], 0..1.
    pub width: f32,
    /// If > 0, the layer's level decays by 60 dB over this many seconds.
    pub decay: f32,
}

impl Osc {
    pub const OFF: Osc = Osc::new(Wave::Sine, 0.0);

    pub const fn new(wave: Wave, level: f32) -> Osc {
        Osc {
            wave,
            level,
            octave: 0,
            detune: 0.0,
            ratio: 1.0,
            width: 0.5,
            decay: 0.0,
        }
    }
    pub const fn oct(mut self, octave: i8) -> Osc {
        self.octave = octave;
        self
    }
    pub const fn det(mut self, cents: f32) -> Osc {
        self.detune = cents;
        self
    }
    pub const fn ratio(mut self, ratio: f32) -> Osc {
        self.ratio = ratio;
        self
    }
    pub const fn width(mut self, width: f32) -> Osc {
        self.width = width;
        self
    }
    pub const fn decay(mut self, seconds: f32) -> Osc {
        self.decay = seconds;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterMode {
    LowPass,
    BandPass,
    HighPass,
    /// Two parallel band-passes (a vowel-like formant pair).
    Formant,
}

#[derive(Clone, Copy, Debug)]
pub struct Filter {
    pub mode: FilterMode,
    /// Cutoff in Hz at pad X = centre.
    pub cutoff: f32,
    /// 0..1.
    pub resonance: f32,
    /// 0 = fixed cutoff, 1 = cutoff follows the key (relative to C4).
    pub key_track: f32,
    /// Filter-envelope amount in octaves (may be negative).
    pub env: f32,
    /// 24 dB/octave cascade instead of 12 dB.
    pub four_pole: bool,
    /// Second formant frequency as a ratio of the cutoff (Formant mode).
    pub formant_ratio: f32,
}

impl Filter {
    pub const fn new(mode: FilterMode, cutoff: f32, resonance: f32) -> Filter {
        Filter {
            mode,
            cutoff,
            resonance,
            key_track: 0.0,
            env: 0.0,
            four_pole: false,
            formant_ratio: 1.5,
        }
    }
    pub const fn lp(cutoff: f32, resonance: f32) -> Filter {
        Filter::new(FilterMode::LowPass, cutoff, resonance)
    }
    pub const fn bp(cutoff: f32, resonance: f32) -> Filter {
        Filter::new(FilterMode::BandPass, cutoff, resonance)
    }
    pub const fn hp(cutoff: f32, resonance: f32) -> Filter {
        Filter::new(FilterMode::HighPass, cutoff, resonance)
    }
    pub const fn formant(cutoff: f32, ratio: f32, resonance: f32) -> Filter {
        let mut f = Filter::new(FilterMode::Formant, cutoff, resonance);
        f.formant_ratio = ratio;
        f
    }
    pub const fn track(mut self, amount: f32) -> Filter {
        self.key_track = amount;
        self
    }
    pub const fn env(mut self, octaves: f32) -> Filter {
        self.env = octaves;
        self
    }
    pub const fn four_pole(mut self) -> Filter {
        self.four_pole = true;
        self
    }
}

/// Attack, decay, sustain, release (seconds, seconds, level, seconds).
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

pub const fn adsr(a: f32, d: f32, s: f32, r: f32) -> Adsr {
    Adsr { a, d, s, r }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    /// Sample and hold.
    Random,
    /// Smoothly interpolated random values (slow drift).
    Wander,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LfoDest {
    /// Depth in semitones.
    Pitch,
    /// Depth in octaves of cutoff.
    Filter,
    /// Depth 0..1 (tremolo).
    Amp,
}

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    pub shape: LfoShape,
    /// Rate in Hz (used when `sync_beats` is 0).
    pub rate: f32,
    /// If > 0, one LFO cycle lasts this many beats (tempo-synced).
    pub sync_beats: f32,
    /// Depth that is always present.
    pub depth: f32,
    /// Extra depth added at pad Y = top ("wobblier").
    pub wobble: f32,
    pub dest: LfoDest,
    /// Output 0..1 instead of -1..1 (e.g. two-note pitch alternation).
    pub unipolar: bool,
}

impl Lfo {
    pub const fn new(shape: LfoShape, rate: f32, dest: LfoDest, depth: f32, wobble: f32) -> Lfo {
        Lfo {
            shape,
            rate,
            sync_beats: 0.0,
            depth,
            wobble,
            dest,
            unipolar: false,
        }
    }
    pub const fn sync(mut self, beats: f32) -> Lfo {
        self.sync_beats = beats;
        self
    }
    pub const fn unipolar(mut self) -> Lfo {
        self.unipolar = true;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArpPattern {
    Up,
    Down,
    UpDown,
    Random,
    /// Each note followed by the same note an octave up.
    UpOctave,
    /// Alternates from the outside in: lowest, highest, 2nd lowest, ...
    InOut,
    /// Re-triggers the held notes without building a chord.
    Repeat,
}

#[derive(Clone, Copy, Debug)]
pub struct Arp {
    pub pattern: ArpPattern,
    /// 2 = eighth notes, 4 = sixteenth notes.
    pub steps_per_beat: u8,
    /// Octave range, 1..=3.
    pub octaves: u8,
    /// Note length as a fraction of the step.
    pub gate: f32,
}

pub const fn arp(pattern: ArpPattern, steps_per_beat: u8, octaves: u8, gate: f32) -> Option<Arp> {
    Some(Arp {
        pattern,
        steps_per_beat,
        octaves,
        gate,
    })
}

/// A complete sound.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub category: Category,

    /// Up to three oscillator layers. Layer 0 is the "main" oscillator: it is
    /// the one that gets unison, FM and the waveform glyph in the UI.
    pub osc: [Osc; 3],
    /// Unison voices for layer 0, 1..=7.
    pub unison: u8,
    /// Total unison detune spread in cents.
    pub spread: f32,
    /// Stereo width of the unison voices, 0..1.
    pub width: f32,
    /// Sine sub-oscillator level, one octave below the note.
    pub sub: f32,
    /// 2-op FM: modulator frequency ratio (0 = FM off).
    pub fm_ratio: f32,
    /// FM index (modulation depth in cycles) at note-on.
    pub fm_index: f32,
    /// If > 0, the FM index decays by 60 dB over this many seconds.
    pub fm_decay: f32,
    /// Ring modulation between layers 0 and 1.
    pub ring: f32,
    /// Level of a ~2 ms noise click at note-on.
    pub click: f32,

    pub filter: Filter,
    pub amp: Adsr,
    pub fenv: Adsr,
    /// Pitch envelope start offset in semitones; glides to 0 in `pitch_time`.
    pub pitch_env: f32,
    pub pitch_time: f32,
    pub lfo: Lfo,
    /// Glide (portamento) time in seconds.
    pub glide: f32,
    /// Saturation amount, 0..1.
    pub drive: f32,

    /// Bit-crusher: bit depth (0 = off) and sample-and-hold rate in Hz (0 = off).
    pub crush_bits: f32,
    pub crush_rate: f32,
    /// Effect sends, 0..1.
    pub chorus: f32,
    pub delay: f32,
    /// Delay time in beats (tempo-aware) and feedback.
    pub delay_beats: f32,
    pub delay_feedback: f32,
    /// Default reverb send (the REVERB knob's default for this sound).
    pub reverb: f32,

    /// Output trim so that all sounds sit at a similar loudness.
    pub gain: f32,

    /// One-shot (drum) behaviour: the note always plays to the end.
    pub one_shot: bool,
    /// If > 0, the pitch is this MIDI note instead of the key...
    pub fixed_note: f32,
    /// ...plus this fraction of the key's distance from C4.
    pub fixed_track: f32,
    /// Extra fast noise bursts before the main decay (hand clap).
    pub bursts: u8,
    pub burst_gap: f32,

    pub arp: Option<Arp>,
}

impl Preset {
    /// A neutral starting point; the preset table overrides what it needs.
    pub const BASE: Preset = Preset {
        name: "",
        category: Category::Leads,
        osc: [Osc::new(Wave::Saw, 1.0), Osc::OFF, Osc::OFF],
        unison: 1,
        spread: 0.0,
        width: 0.0,
        sub: 0.0,
        fm_ratio: 0.0,
        fm_index: 0.0,
        fm_decay: 0.0,
        ring: 0.0,
        click: 0.0,
        filter: Filter::lp(18_000.0, 0.0),
        amp: adsr(0.005, 0.2, 0.8, 0.2),
        fenv: adsr(0.005, 0.2, 0.0, 0.2),
        pitch_env: 0.0,
        pitch_time: 0.0,
        lfo: Lfo::new(LfoShape::Sine, 5.0, LfoDest::Pitch, 0.0, 0.4),
        glide: 0.0,
        drive: 0.0,
        crush_bits: 0.0,
        crush_rate: 0.0,
        chorus: 0.0,
        delay: 0.0,
        delay_beats: 0.75,
        delay_feedback: 0.35,
        reverb: 0.2,
        gain: 1.0,
        one_shot: false,
        fixed_note: 0.0,
        fixed_track: 0.0,
        bursts: 0,
        burst_gap: 0.0,
        arp: None,
    };

    /// The waveform shown for this sound in the UI.
    pub fn main_wave(&self) -> Wave {
        self.osc[0].wave
    }
}
