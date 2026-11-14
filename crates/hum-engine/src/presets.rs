//! The 56 built-in sounds: seven per category, in the order the app shows
//! them. This table is the single source of truth for every sound.

use crate::preset::{
    adsr, arp, ArpPattern, Category, Filter, Lfo, LfoDest, LfoShape, Osc, Preset, Wave,
};

pub const COUNT: usize = 57;

const fn sine(level: f32) -> Osc {
    Osc::new(Wave::Sine, level)
}
const fn tri(level: f32) -> Osc {
    Osc::new(Wave::Triangle, level)
}
const fn saw(level: f32) -> Osc {
    Osc::new(Wave::Saw, level)
}
const fn sq(level: f32) -> Osc {
    Osc::new(Wave::Square, level)
}
const fn noise(level: f32) -> Osc {
    Osc::new(Wave::Noise, level)
}
const fn lfo(shape: LfoShape, rate: f32, dest: LfoDest, depth: f32, wobble: f32) -> Lfo {
    Lfo::new(shape, rate, dest, depth, wobble)
}

const OFF: Osc = Osc::OFF;
const B: Preset = Preset::BASE;

use LfoDest::{Amp as ToAmp, Filter as ToFilter, Pitch as ToPitch};
use LfoShape::{Random, Sine, Square, Wander};

const LEAD: Preset = Preset {
    category: Category::Leads,
    ..B
};
const PAD: Preset = Preset {
    category: Category::Pads,
    reverb: 0.45,
    ..B
};
const BASS: Preset = Preset {
    category: Category::Bass,
    reverb: 0.08,
    ..B
};
const KEYS: Preset = Preset {
    category: Category::Keys,
    reverb: 0.3,
    ..B
};
const ARP: Preset = Preset {
    category: Category::Arps,
    reverb: 0.3,
    ..B
};
const FX: Preset = Preset {
    category: Category::Fx,
    reverb: 0.45,
    ..B
};
const RETRO: Preset = Preset {
    category: Category::Retro,
    reverb: 0.12,
    ..B
};
const DRUM: Preset = Preset {
    category: Category::Drums,
    one_shot: true,
    reverb: 0.1,
    lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.0),
    ..B
};

pub static PRESETS: [Preset; COUNT] = [
    // ------------------------------------------------------------- Leads
    Preset {
        name: "Laser Lead",
        osc: [saw(1.0), OFF, OFF],
        pitch_env: 12.0,
        pitch_time: 0.07,
        filter: Filter::lp(7000.0, 0.2).track(0.3),
        amp: adsr(0.003, 0.2, 0.8, 0.15),
        lfo: lfo(Sine, 5.5, ToPitch, 0.08, 0.6),
        delay: 0.15,
        delay_beats: 0.5,
        gain: 0.47,
        ..LEAD
    },
    Preset {
        name: "Sunbeam",
        osc: [saw(0.8).det(-8.0), saw(0.8).det(8.0), OFF],
        filter: Filter::lp(9000.0, 0.1),
        amp: adsr(0.01, 0.3, 0.9, 0.25),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.5),
        chorus: 0.5,
        gain: 0.31,
        ..LEAD
    },
    Preset {
        name: "Glass Saw",
        osc: [saw(1.0), OFF, OFF],
        filter: Filter::lp(2200.0, 0.82).track(0.6).env(0.6),
        fenv: adsr(0.002, 0.3, 0.2, 0.3),
        amp: adsr(0.004, 0.3, 0.8, 0.2),
        lfo: lfo(Sine, 4.0, ToFilter, 0.0, 1.2),
        delay: 0.3,
        delay_beats: 0.5,
        gain: 0.34,
        ..LEAD
    },
    Preset {
        name: "Pixel Pulse",
        osc: [sq(1.0).width(0.25), OFF, OFF],
        filter: Filter::lp(8000.0, 0.1),
        amp: adsr(0.001, 0.12, 0.5, 0.08),
        lfo: lfo(Sine, 6.0, ToPitch, 0.0, 0.5),
        gain: 0.45,
        ..LEAD
    },
    Preset {
        name: "Neon Square",
        osc: [sq(1.0), OFF, OFF],
        unison: 2,
        spread: 18.0,
        width: 0.7,
        filter: Filter::lp(1800.0, 0.35).env(2.5).track(0.3),
        fenv: adsr(0.005, 0.25, 0.3, 0.25),
        amp: adsr(0.004, 0.3, 0.85, 0.2),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.5),
        delay: 0.28,
        gain: 0.32,
        ..LEAD
    },
    Preset {
        name: "Soft Sine",
        osc: [sine(1.0), tri(0.15), OFF],
        filter: Filter::lp(5000.0, 0.0),
        amp: adsr(0.02, 0.3, 0.9, 0.3),
        lfo: lfo(Sine, 4.5, ToPitch, 0.1, 0.5),
        gain: 0.37,
        ..LEAD
    },
    Preset {
        name: "Hyper Saw",
        osc: [saw(1.0), OFF, OFF],
        unison: 5,
        spread: 56.0,
        width: 1.0,
        filter: Filter::lp(8000.0, 0.1),
        amp: adsr(0.01, 0.3, 0.9, 0.3),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.5),
        chorus: 0.2,
        gain: 0.41,
        ..LEAD
    },
    // -------------------------------------------------------------- Pads
    Preset {
        name: "Warm Cloud",
        osc: [saw(0.8).det(-7.0), saw(0.8).det(7.0), OFF],
        sub: 0.4,
        filter: Filter::lp(1400.0, 0.15).track(0.3),
        amp: adsr(0.6, 1.0, 0.85, 1.5),
        lfo: lfo(Sine, 0.12, ToFilter, 0.15, 1.0),
        chorus: 0.3,
        reverb: 0.45,
        gain: 0.33,
        ..PAD
    },
    Preset {
        name: "Dusk Choir",
        osc: [saw(0.8).det(-10.0), saw(0.8).det(10.0), OFF],
        filter: Filter::formant(700.0, 1.6, 0.75),
        amp: adsr(0.5, 1.0, 0.9, 1.2),
        lfo: lfo(Sine, 0.25, ToFilter, 0.12, 0.5),
        chorus: 0.4,
        reverb: 0.5,
        gain: 0.16,
        ..PAD
    },
    Preset {
        name: "Slow Tide",
        osc: [sine(1.0), tri(0.6), OFF],
        filter: Filter::lp(900.0, 0.3).track(0.5),
        amp: adsr(1.2, 1.0, 0.9, 2.0),
        lfo: lfo(Sine, 0.1, ToFilter, 1.0, 1.5),
        reverb: 0.5,
        gain: 0.60,
        ..PAD
    },
    Preset {
        name: "Velvet Air",
        osc: [tri(1.0), noise(0.1), OFF],
        filter: Filter::lp(3000.0, 0.1).track(0.3),
        amp: adsr(0.35, 0.8, 0.85, 1.0),
        lfo: lfo(Sine, 4.0, ToPitch, 0.0, 0.3),
        chorus: 0.2,
        reverb: 0.4,
        gain: 0.43,
        ..PAD
    },
    Preset {
        name: "Aurora",
        osc: [saw(1.0), OFF, sine(0.08).oct(2)],
        unison: 5,
        spread: 44.0,
        width: 1.0,
        filter: Filter::lp(500.0, 0.3).env(3.5),
        fenv: adsr(2.5, 3.0, 0.7, 2.0),
        amp: adsr(0.8, 1.0, 0.9, 2.0),
        lfo: lfo(Sine, 0.2, ToFilter, 0.0, 0.8),
        chorus: 0.3,
        reverb: 0.55,
        gain: 0.35,
        ..PAD
    },
    Preset {
        name: "Deep Ocean",
        osc: [saw(0.8).oct(-1).det(-6.0), saw(0.8).oct(-1).det(6.0), OFF],
        sub: 0.3,
        filter: Filter::lp(420.0, 0.2).track(0.2),
        amp: adsr(1.5, 1.0, 0.9, 2.5),
        lfo: lfo(Sine, 0.07, ToFilter, 0.3, 1.2),
        reverb: 0.7,
        gain: 0.39,
        ..PAD
    },
    Preset {
        name: "Frosted Glass",
        osc: [
            tri(1.0).oct(1),
            sine(0.25).ratio(2.76),
            sine(0.12).ratio(5.4),
        ],
        filter: Filter::lp(7000.0, 0.1),
        amp: adsr(0.25, 2.0, 0.6, 1.8),
        lfo: lfo(Sine, 3.0, ToAmp, 0.0, 0.5),
        chorus: 0.2,
        reverb: 0.5,
        gain: 0.42,
        ..PAD
    },
    // -------------------------------------------------------------- Bass
    Preset {
        name: "Sub Thump",
        osc: [sine(1.0).oct(-1), OFF, OFF],
        pitch_env: 12.0,
        pitch_time: 0.05,
        filter: Filter::lp(800.0, 0.0),
        amp: adsr(0.001, 0.4, 0.25, 0.1),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        drive: 0.2,
        gain: 1.01,
        ..BASS
    },
    Preset {
        name: "Rubber Bass",
        osc: [tri(1.0).oct(-1), sq(0.5).oct(-1), OFF],
        filter: Filter::lp(300.0, 0.6).env(3.2),
        fenv: adsr(0.001, 0.14, 0.1, 0.1),
        amp: adsr(0.002, 0.3, 0.4, 0.1),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        gain: 0.45,
        ..BASS
    },
    Preset {
        name: "Wobble Wub",
        osc: [saw(1.0).oct(-1), saw(0.5).oct(-1).det(10.0), OFF],
        filter: Filter::lp(500.0, 0.7),
        amp: adsr(0.005, 0.2, 0.9, 0.12),
        lfo: lfo(Sine, 3.67, ToFilter, 0.8, 2.2).sync(0.5),
        drive: 0.3,
        gain: 0.45,
        ..BASS
    },
    Preset {
        name: "Acid Squelch",
        osc: [saw(1.0).oct(-1), OFF, OFF],
        filter: Filter::lp(350.0, 0.9).env(4.0),
        fenv: adsr(0.001, 0.16, 0.0, 0.1),
        amp: adsr(0.002, 0.3, 0.5, 0.08),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 1.0),
        glide: 0.06,
        drive: 0.35,
        gain: 0.55,
        ..BASS
    },
    Preset {
        name: "Fat Saw Bass",
        osc: [
            saw(0.7).oct(-1).det(-12.0),
            saw(0.7).oct(-1),
            saw(0.7).oct(-1).det(12.0),
        ],
        sub: 0.5,
        filter: Filter::lp(1100.0, 0.2),
        amp: adsr(0.004, 0.3, 0.8, 0.12),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        gain: 0.43,
        ..BASS
    },
    Preset {
        name: "Moog Low",
        osc: [sq(0.7).oct(-1), saw(0.7).oct(-1), OFF],
        filter: Filter::lp(500.0, 0.35).env(1.8).four_pole(),
        fenv: adsr(0.002, 0.3, 0.3, 0.15),
        amp: adsr(0.003, 0.3, 0.8, 0.12),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        drive: 0.15,
        gain: 0.41,
        ..BASS
    },
    Preset {
        name: "Growl",
        osc: [saw(1.0).oct(-1), OFF, OFF],
        fm_ratio: 0.5,
        fm_index: 0.35,
        filter: Filter::lp(700.0, 0.5),
        amp: adsr(0.004, 0.3, 0.85, 0.12),
        lfo: lfo(Sine, 6.0, ToFilter, 0.6, 1.8),
        drive: 0.7,
        gain: 0.67,
        ..BASS
    },
    // -------------------------------------------------------------- Keys
    Preset {
        name: "Soft Bell",
        osc: [sine(1.0), OFF, OFF],
        fm_ratio: 3.5,
        fm_index: 0.35,
        fm_decay: 1.2,
        amp: adsr(0.002, 2.2, 0.0, 1.2),
        lfo: lfo(Sine, 4.0, ToPitch, 0.0, 0.3),
        reverb: 0.35,
        gain: 0.56,
        ..KEYS
    },
    Preset {
        name: "E-Piano",
        osc: [sine(1.0), sine(0.1).ratio(14.0).decay(0.08), OFF],
        fm_ratio: 1.0,
        fm_index: 0.22,
        fm_decay: 0.5,
        amp: adsr(0.002, 1.6, 0.0, 0.4),
        lfo: lfo(Sine, 4.5, ToAmp, 0.0, 0.6),
        chorus: 0.25,
        reverb: 0.2,
        gain: 0.69,
        ..KEYS
    },
    Preset {
        name: "Pluck Harp",
        osc: [saw(1.0), OFF, OFF],
        filter: Filter::lp(500.0, 0.15).env(4.5).track(0.5),
        fenv: adsr(0.001, 0.22, 0.0, 0.2),
        amp: adsr(0.001, 1.4, 0.0, 0.6),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        reverb: 0.3,
        gain: 0.65,
        ..KEYS
    },
    Preset {
        name: "Marimba",
        osc: [sine(1.0), sine(0.35).ratio(4.0).decay(0.07), OFF],
        click: 0.2,
        amp: adsr(0.001, 0.55, 0.0, 0.3),
        lfo: lfo(Sine, 5.0, ToAmp, 0.0, 0.6),
        reverb: 0.25,
        gain: 0.56,
        ..KEYS
    },
    Preset {
        name: "Music Box",
        osc: [sine(1.0).oct(1), OFF, OFF],
        fm_ratio: 5.0,
        fm_index: 0.12,
        fm_decay: 0.3,
        amp: adsr(0.001, 1.1, 0.0, 0.8),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        delay: 0.2,
        delay_beats: 0.25,
        delay_feedback: 0.2,
        reverb: 0.3,
        gain: 0.66,
        ..KEYS
    },
    Preset {
        name: "Toy Piano",
        osc: [sq(0.6).width(0.35), sine(0.35).ratio(6.3).decay(0.05), OFF],
        click: 0.25,
        filter: Filter::hp(500.0, 0.1),
        amp: adsr(0.001, 0.7, 0.0, 0.25),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        reverb: 0.2,
        gain: 0.56,
        ..KEYS
    },
    Preset {
        name: "Chime Keys",
        osc: [
            sine(1.0),
            sine(0.5).ratio(2.76).decay(1.5),
            sine(0.3).ratio(5.4).decay(0.8),
        ],
        amp: adsr(0.002, 3.5, 0.0, 2.5),
        lfo: lfo(Sine, 3.0, ToAmp, 0.0, 0.5),
        reverb: 0.45,
        gain: 0.41,
        ..KEYS
    },
    // -------------------------------------------------------------- Arps
    Preset {
        name: "Rain Arp",
        osc: [tri(1.0), OFF, OFF],
        filter: Filter::lp(6000.0, 0.1),
        amp: adsr(0.001, 0.25, 0.0, 0.2),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        delay: 0.35,
        delay_beats: 0.75,
        arp: arp(ArpPattern::Up, 4, 2, 0.5),
        gain: 0.54,
        ..ARP
    },
    Preset {
        name: "Stairway",
        osc: [saw(1.0), OFF, OFF],
        filter: Filter::lp(2500.0, 0.2).env(1.5),
        fenv: adsr(0.002, 0.2, 0.1, 0.2),
        amp: adsr(0.003, 0.3, 0.6, 0.15),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        delay: 0.15,
        arp: arp(ArpPattern::UpDown, 2, 2, 0.7),
        gain: 0.53,
        ..ARP
    },
    Preset {
        name: "Twinkle",
        osc: [sine(1.0).oct(2), OFF, OFF],
        amp: adsr(0.001, 0.3, 0.0, 0.3),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        delay: 0.3,
        delay_beats: 0.75,
        reverb: 0.4,
        arp: arp(ArpPattern::Random, 4, 2, 0.5),
        gain: 0.48,
        ..ARP
    },
    Preset {
        name: "Bounce",
        osc: [sq(1.0), OFF, OFF],
        filter: Filter::lp(4000.0, 0.15),
        amp: adsr(0.001, 0.1, 0.3, 0.05),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        arp: arp(ArpPattern::UpOctave, 4, 1, 0.35),
        gain: 0.50,
        ..ARP
    },
    Preset {
        name: "Cascade",
        osc: [saw(1.0), OFF, OFF],
        filter: Filter::lp(3000.0, 0.15).env(1.0),
        fenv: adsr(0.002, 0.3, 0.0, 0.5),
        amp: adsr(0.002, 0.3, 0.5, 1.2),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 0.8),
        reverb: 0.55,
        arp: arp(ArpPattern::Down, 4, 2, 0.5),
        gain: 0.33,
        ..ARP
    },
    Preset {
        name: "Spiral",
        osc: [tri(1.0), OFF, OFF],
        filter: Filter::lp(6000.0, 0.1),
        amp: adsr(0.002, 0.25, 0.3, 0.2),
        lfo: lfo(Sine, 5.0, ToPitch, 0.0, 0.3),
        delay: 0.2,
        delay_beats: 0.5,
        arp: arp(ArpPattern::InOut, 4, 3, 0.6),
        gain: 0.53,
        ..ARP
    },
    Preset {
        name: "Pulse Runner",
        osc: [sq(1.0).width(0.3), OFF, OFF],
        filter: Filter::lp(1800.0, 0.3).env(2.0),
        fenv: adsr(0.001, 0.09, 0.0, 0.08),
        amp: adsr(0.001, 0.12, 0.4, 0.05),
        lfo: lfo(Sine, 5.0, ToFilter, 0.0, 1.0),
        drive: 0.2,
        arp: arp(ArpPattern::Repeat, 4, 1, 0.5),
        gain: 0.68,
        ..ARP
    },
    // ---------------------------------------------------------------- FX
    Preset {
        name: "Wind Sweep",
        osc: [noise(1.0), OFF, OFF],
        filter: Filter::bp(800.0, 0.8),
        amp: adsr(0.8, 1.0, 1.0, 1.5),
        lfo: lfo(Sine, 0.15, ToFilter, 1.6, 1.5),
        reverb: 0.4,
        gain: 0.45,
        ..FX
    },
    Preset {
        name: "Riser",
        osc: [saw(0.8), noise(0.35), OFF],
        pitch_env: -24.0,
        pitch_time: 3.0,
        filter: Filter::lp(400.0, 0.3).env(4.5),
        fenv: adsr(3.0, 0.1, 1.0, 0.6),
        amp: adsr(0.3, 0.5, 1.0, 0.6),
        lfo: lfo(Sine, 6.0, ToPitch, 0.0, 0.6),
        reverb: 0.4,
        gain: 0.38,
        ..FX
    },
    Preset {
        name: "Drone",
        osc: [
            saw(0.7).oct(-1).det(-14.0),
            saw(0.7).oct(-1),
            saw(0.7).oct(-1).det(11.0),
        ],
        filter: Filter::lp(900.0, 0.2),
        amp: adsr(1.5, 1.0, 1.0, 3.0),
        lfo: lfo(Wander, 0.15, ToPitch, 0.15, 0.6),
        reverb: 0.5,
        gain: 0.31,
        ..FX
    },
    Preset {
        name: "Alien Voice",
        osc: [saw(1.0), OFF, OFF],
        filter: Filter::formant(500.0, 2.6, 0.85).env(1.6),
        fenv: adsr(1.2, 1.5, 0.4, 0.5),
        amp: adsr(0.03, 0.3, 0.9, 0.4),
        lfo: lfo(Random, 11.0, ToPitch, 0.5, 2.0),
        gain: 0.36,
        ..FX
    },
    Preset {
        name: "Rumble",
        osc: [noise(0.9), sine(0.6).oct(-2), OFF],
        filter: Filter::lp(110.0, 0.3),
        amp: adsr(0.5, 1.0, 1.0, 2.0),
        lfo: lfo(Wander, 0.5, ToFilter, 0.4, 1.0),
        drive: 0.3,
        gain: 1.02,
        ..FX
    },
    Preset {
        name: "Shimmer",
        osc: [
            sine(0.5).oct(1),
            sine(0.35).oct(2),
            sine(0.25).oct(1).ratio(3.0),
        ],
        amp: adsr(1.5, 1.0, 1.0, 2.5),
        lfo: lfo(Sine, 3.0, ToAmp, 0.15, 0.5),
        chorus: 0.4,
        reverb: 0.7,
        gain: 0.38,
        ..FX
    },
    Preset {
        name: "Zap",
        osc: [sine(0.9), noise(0.5), OFF],
        pitch_env: 36.0,
        pitch_time: 0.25,
        filter: Filter::bp(300.0, 0.5).env(4.5),
        fenv: adsr(0.001, 0.22, 0.0, 0.1),
        amp: adsr(0.001, 0.3, 0.0, 0.1),
        lfo: lfo(Sine, 9.0, ToPitch, 0.0, 1.0),
        delay: 0.2,
        delay_beats: 0.25,
        gain: 0.46,
        ..FX
    },
    // ------------------------------------------------------------- Retro
    Preset {
        name: "8-Bit Hero",
        osc: [sq(1.0), OFF, OFF],
        amp: adsr(0.001, 0.1, 0.8, 0.06),
        lfo: lfo(Sine, 6.0, ToPitch, 0.06, 0.5),
        crush_bits: 5.0,
        crush_rate: 14_000.0,
        gain: 0.26,
        ..RETRO
    },
    Preset {
        name: "Chip Bass",
        osc: [sq(1.0).width(0.25).oct(-1), OFF, OFF],
        amp: adsr(0.001, 0.18, 0.3, 0.05),
        lfo: lfo(Sine, 6.0, ToPitch, 0.0, 0.4),
        crush_bits: 6.0,
        crush_rate: 12_000.0,
        gain: 0.46,
        ..RETRO
    },
    Preset {
        name: "Arcade",
        osc: [sq(1.0), OFF, OFF],
        amp: adsr(0.001, 0.1, 0.8, 0.06),
        lfo: lfo(Square, 14.0, ToPitch, 7.0, 5.0).unipolar(),
        crush_bits: 6.0,
        crush_rate: 16_000.0,
        gain: 0.28,
        ..RETRO
    },
    Preset {
        name: "Handheld",
        osc: [sq(1.0).width(0.125), OFF, OFF],
        filter: Filter::hp(300.0, 0.0),
        amp: adsr(0.001, 0.1, 0.7, 0.05),
        lfo: lfo(Sine, 7.0, ToPitch, 0.0, 0.5),
        crush_bits: 4.0,
        crush_rate: 16_000.0,
        gain: 0.39,
        ..RETRO
    },
    Preset {
        name: "Buzz Lead",
        osc: [saw(0.8), sq(0.4).ratio(1.5), OFF],
        ring: 0.6,
        filter: Filter::lp(5000.0, 0.2),
        amp: adsr(0.003, 0.2, 0.85, 0.12),
        lfo: lfo(Sine, 5.5, ToPitch, 0.0, 0.5),
        drive: 0.4,
        gain: 0.53,
        ..RETRO
    },
    Preset {
        name: "Tape Lead",
        osc: [saw(0.8), tri(0.6), OFF],
        filter: Filter::lp(2200.0, 0.2).track(0.3),
        amp: adsr(0.01, 0.3, 0.85, 0.25),
        lfo: lfo(Wander, 2.2, ToPitch, 0.12, 0.5),
        drive: 0.45,
        chorus: 0.15,
        gain: 0.83,
        ..RETRO
    },
    Preset {
        name: "Cassette Pad",
        osc: [tri(1.0), noise(0.05), OFF],
        filter: Filter::lp(1600.0, 0.15).track(0.3),
        amp: adsr(0.4, 0.8, 0.85, 1.2),
        lfo: lfo(Wander, 0.8, ToPitch, 0.15, 0.5),
        drive: 0.2,
        crush_rate: 12_000.0,
        chorus: 0.3,
        reverb: 0.35,
        gain: 0.47,
        ..RETRO
    },
    // ------------------------------------------------------------- Drums
    Preset {
        name: "Synth Kick",
        osc: [sine(1.0), OFF, OFF],
        // 45 Hz body, starting near 150 Hz.
        fixed_note: 29.5,
        fixed_track: 0.08,
        pitch_env: 20.8,
        pitch_time: 0.11,
        click: 0.35,
        amp: adsr(0.001, 0.34, 0.0, 0.2),
        drive: 0.3,
        reverb: 0.0,
        gain: 1.02,
        ..DRUM
    },
    Preset {
        name: "Snappy Snare",
        osc: [tri(0.8).decay(0.12), noise(0.8), OFF],
        // ~200 Hz body.
        fixed_note: 55.35,
        fixed_track: 0.1,
        pitch_env: 7.0,
        pitch_time: 0.03,
        filter: Filter::hp(170.0, 0.1),
        amp: adsr(0.001, 0.17, 0.0, 0.1),
        reverb: 0.12,
        gain: 0.59,
        ..DRUM
    },
    Preset {
        name: "Hat Tick",
        osc: [noise(1.0), OFF, OFF],
        fixed_note: 60.0,
        fixed_track: 0.1,
        filter: Filter::hp(7000.0, 0.2).track(1.0),
        amp: adsr(0.0005, 0.04, 0.0, 0.03),
        reverb: 0.05,
        gain: 0.71,
        ..DRUM
    },
    Preset {
        name: "Clap",
        osc: [noise(1.0), OFF, OFF],
        fixed_note: 60.0,
        fixed_track: 0.1,
        filter: Filter::bp(1200.0, 0.55).track(1.0),
        amp: adsr(0.0005, 0.16, 0.0, 0.1),
        bursts: 3,
        burst_gap: 0.011,
        reverb: 0.3,
        gain: 0.99,
        ..DRUM
    },
    Preset {
        name: "Tom Drop",
        osc: [sine(1.0).oct(-1), OFF, OFF],
        pitch_env: 9.0,
        pitch_time: 0.14,
        click: 0.12,
        amp: adsr(0.001, 0.38, 0.0, 0.2),
        reverb: 0.15,
        gain: 0.65,
        ..DRUM
    },
    Preset {
        name: "Zap Perc",
        osc: [sq(1.0), OFF, OFF],
        fixed_note: 55.0,
        fixed_track: 0.3,
        pitch_env: 30.0,
        pitch_time: 0.07,
        filter: Filter::lp(9000.0, 0.1),
        amp: adsr(0.0005, 0.11, 0.0, 0.05),
        gain: 0.65,
        ..DRUM
    },
    Preset {
        name: "Rim Click",
        osc: [tri(0.6).decay(0.03), OFF, OFF],
        // ~1.7 kHz.
        fixed_note: 92.4,
        fixed_track: 0.1,
        click: 0.9,
        filter: Filter::bp(1800.0, 0.4).track(1.0),
        amp: adsr(0.0003, 0.04, 0.0, 0.03),
        gain: 0.97,
        ..DRUM
    },
    // -------------------------------------------------------------- Wind
    Preset {
        name: "Ocarina",
        category: Category::Wind,
        // A pure, hollow tone with a breath of noise and a little scoop
        // into each note; pad Y adds vibrato.
        osc: [sine(1.0), tri(0.1).oct(1), noise(0.035)],
        filter: Filter::lp(3200.0, 0.1).track(0.6),
        amp: adsr(0.045, 0.3, 0.92, 0.14),
        pitch_env: -0.35,
        pitch_time: 0.06,
        lfo: lfo(Sine, 5.3, ToPitch, 0.04, 0.45),
        reverb: 0.35,
        gain: 0.41,
        ..B
    },
];

/// Index of the sound selected on first launch.
pub const DEFAULT_PRESET: usize = 7; // Warm Cloud

/// The drum voices used by the beat maker, in track order
/// (Kick, Snare, Hat, Clap).
pub const BEAT_DRUMS: [&str; 4] = ["Synth Kick", "Snappy Snare", "Hat Tick", "Clap"];

pub fn find(name: &str) -> Option<usize> {
    PRESETS
        .iter()
        .position(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::Category;

    #[test]
    fn there_are_exactly_57_uniquely_named_sounds() {
        assert_eq!(PRESETS.len(), 57);
        for (i, a) in PRESETS.iter().enumerate() {
            assert!(!a.name.is_empty());
            for b in &PRESETS[i + 1..] {
                assert_ne!(a.name, b.name);
            }
        }
    }

    #[test]
    fn seven_sounds_per_category_in_order() {
        // The original eight categories hold seven sounds each; the Wind
        // category (the ocarina) follows them.
        for (c, cat) in Category::ALL.iter().take(8).enumerate() {
            for i in 0..7 {
                assert_eq!(
                    PRESETS[c * 7 + i].category,
                    *cat,
                    "{}",
                    PRESETS[c * 7 + i].name
                );
            }
        }
    }

    #[test]
    fn defaults_and_beat_drums_exist() {
        assert_eq!(PRESETS[DEFAULT_PRESET].name, "Warm Cloud");
        for name in BEAT_DRUMS {
            let i = find(name).expect(name);
            assert!(PRESETS[i].one_shot);
        }
        for p in PRESETS.iter() {
            assert_eq!(p.category == Category::Arps, p.arp.is_some(), "{}", p.name);
            assert!(p.category != Category::Wind || p.name == "Ocarina");
            assert_eq!(p.category == Category::Drums, p.one_shot, "{}", p.name);
            assert!((1..=7).contains(&p.unison), "{}", p.name);
        }
    }
}
