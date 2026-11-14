//! The playing surface: four modes sharing one multi-touch engine.
//!
//! * Scale: seven keys (fourteen on wide screens) locked to the scale (they go through the engine's
//!   key map, so arpeggiator sounds arpeggiate).
//! * Piano: as many keys as fit (an octave and one note on a phone held
//!   upright, two octaves or more in landscape), with black keys.
//! * Chords: one triad per scale degree.
//! * Drums: eight pads on the General MIDI drum notes.
//!
//! Every finger plays the zone under it and sliding plays each new zone.
//! The notes a zone started are remembered, so changing mode, octave or
//! key while fingers are down never leaves a note hanging.

use eframe::egui::{pos2, vec2, Align2, Id, Painter, Rect};
use hum_engine::engine::Cmd;
use hum_engine::scale::{KeyMap, Scale};
use serde::{Deserialize, Serialize};

use crate::app::HumApp;
use crate::theme::{self, grotesk, inter};
use crate::widgets;

pub const MAX_ZONES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum PlayMode {
    #[default]
    Scale,
    Piano,
    Chords,
    Drums,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [
        PlayMode::Scale,
        PlayMode::Piano,
        PlayMode::Chords,
        PlayMode::Drums,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Scale => "Scale",
            PlayMode::Piano => "Piano",
            PlayMode::Chords => "Chords",
            PlayMode::Drums => "Drums",
        }
    }
}

/// What a pressed zone is playing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Held {
    /// Scale mode: the on-screen key sent through the engine's key map.
    key: Option<u8>,
    notes: [u8; 3],
    count: usize,
}

#[derive(Clone, Copy, Debug)]
enum Label {
    Note(u8),
    Chord(u8, &'static str),
    Text(&'static str),
}

#[derive(Clone, Copy, Debug)]
struct Zone {
    rect: Rect,
    held: Held,
    label: Label,
    black: bool,
    root: bool,
    in_scale: bool,
}

/// General MIDI drum notes for the eight pads.
const DRUM_PADS: [(u8, &str); 8] = [
    (36, "Kick"),
    (38, "Snare"),
    (39, "Clap"),
    (37, "Rim"),
    (42, "Hat"),
    (46, "Open hat"),
    (47, "Tom"),
    (49, "Crash"),
];

/// The seven-note scale chords are built from (pentatonic and blues
/// scales borrow the chords of their parent scale).
fn chord_scale(s: Scale) -> Scale {
    match s {
        Scale::MinorPentatonic | Scale::Blues => Scale::NaturalMinor,
        Scale::MajorPentatonic => Scale::Major,
        other => other,
    }
}

fn chord_suffix(third: u8, fifth: u8) -> &'static str {
    match (third, fifth) {
        (4, 7) => "",
        (3, 7) => "m",
        (3, 6) => "°",
        (4, 8) => "+",
        _ => "",
    }
}

fn zones(mode: PlayMode, map: KeyMap, rect: Rect) -> Vec<Zone> {
    let scale_notes = |note: u8| {
        let rel = (note + 120 - map.root) % 12;
        (map.scale.intervals().contains(&rel), rel == 0)
    };
    let mut out = Vec::with_capacity(MAX_ZONES);
    match mode {
        PlayMode::Scale | PlayMode::Chords => {
            let gap = 6.0;
            let n: u8 = if mode == PlayMode::Scale && rect.width() >= 600.0 {
                14
            } else {
                7
            };
            let w = (rect.width() - gap * f32::from(n - 1)) / f32::from(n);
            let chord_map = KeyMap {
                scale: chord_scale(map.scale),
                locked: true,
                ..map
            };
            for k in 0..n {
                let r = Rect::from_min_size(
                    pos2(rect.left() + f32::from(k) * (w + gap), rect.top()),
                    vec2(w, rect.height()),
                );
                if mode == PlayMode::Scale {
                    let note = map.note(i32::from(k));
                    out.push(Zone {
                        rect: r,
                        held: Held {
                            key: Some(k),
                            notes: [note, 0, 0],
                            count: 1,
                        },
                        label: Label::Note(note),
                        black: false,
                        root: map.is_root(i32::from(k)),
                        in_scale: true,
                    });
                } else {
                    let d = i32::from(k);
                    let n = [
                        chord_map.note(d),
                        chord_map.note(d + 2),
                        chord_map.note(d + 4),
                    ];
                    out.push(Zone {
                        rect: r,
                        held: Held {
                            key: None,
                            notes: n,
                            count: 3,
                        },
                        label: Label::Chord(n[0], chord_suffix(n[1] - n[0], n[2] - n[0])),
                        black: false,
                        root: n[0] % 12 == map.root % 12,
                        in_scale: true,
                    });
                }
            }
        }
        PlayMode::Piano => {
            const STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
            // White keys at least ~44 points wide: 8 on a phone held upright,
            // 15 or more (two octaves) in landscape.
            let whites = ((rect.width() / 44.0) as usize).clamp(8, 36);
            let lower = if whites >= 15 { 1 } else { 0 };
            let octave = (map.octave.clamp(1, 6) - lower).max(0) as u8;
            let base = 12 * (octave + 1);
            let white_note = |i: usize| (base + 12 * (i / 7) as u8 + STEPS[i % 7]).min(127);
            let gap = 3.0;
            let w = (rect.width() - gap * (whites as f32 - 1.0)) / whites as f32;
            // Black keys first: they sit on top and win the hit test.
            for after in 0..whites - 1 {
                if !matches!(after % 7, 0 | 1 | 3 | 4 | 5) {
                    continue;
                }
                let note = white_note(after) + 1;
                let x = rect.left() + (after as f32 + 1.0) * (w + gap) - gap / 2.0;
                let (in_scale, root) = scale_notes(note);
                out.push(Zone {
                    rect: Rect::from_center_size(
                        pos2(x, rect.top() + rect.height() * 0.3),
                        vec2(w * 0.62, rect.height() * 0.6),
                    ),
                    held: Held {
                        key: None,
                        notes: [note, 0, 0],
                        count: 1,
                    },
                    label: Label::Note(note),
                    black: true,
                    root,
                    in_scale,
                });
            }
            for i in 0..whites {
                let note = white_note(i);
                let (in_scale, root) = scale_notes(note);
                out.push(Zone {
                    rect: Rect::from_min_size(
                        pos2(rect.left() + i as f32 * (w + gap), rect.top()),
                        vec2(w, rect.height()),
                    ),
                    held: Held {
                        key: None,
                        notes: [note, 0, 0],
                        count: 1,
                    },
                    label: Label::Note(note),
                    black: false,
                    root,
                    in_scale,
                });
            }
        }
        PlayMode::Drums => {
            let gap = 8.0;
            let w = (rect.width() - gap * 3.0) / 4.0;
            let h = (rect.height() - gap) / 2.0;
            for (i, (note, name)) in DRUM_PADS.iter().enumerate() {
                out.push(Zone {
                    rect: Rect::from_min_size(
                        pos2(
                            rect.left() + (i % 4) as f32 * (w + gap),
                            rect.top() + (i / 4) as f32 * (h + gap),
                        ),
                        vec2(w, h),
                    ),
                    held: Held {
                        key: None,
                        notes: [*note, 0, 0],
                        count: 1,
                    },
                    label: Label::Text(name),
                    black: false,
                    root: false,
                    in_scale: true,
                });
            }
        }
    }
    out
}

impl HumApp {
    /// Draw the playing surface for the current mode and play it.
    pub(crate) fn keyboard(&mut self, p: &Painter, rect: Rect, root_dots: bool) {
        self.keyboard_seen = true;
        let mode = self.st.play_mode;
        let zones = zones(mode, self.st.keymap(), rect);
        let id = Id::unique("keyboard");
        self.touches.claim(id, rect);

        let touches: Vec<(u64, eframe::egui::Pos2, bool)> = self
            .touches
            .owned(id)
            .map(|t| (t.id, t.pos, t.ended))
            .collect();
        for (touch, pos, ended) in touches {
            let current = self.zone_of_touch.iter().position(|(t, _)| *t == touch);
            if ended {
                if let Some(i) = current {
                    let (_, zone) = self.zone_of_touch.swap_remove(i);
                    self.zone_release(zone);
                }
                continue;
            }
            let Some(z) = zones.iter().position(|z| z.rect.contains(pos)) else {
                continue;
            };
            // Loudness follows where the key is touched: soft at the top.
            let r = zones[z].rect;
            let velocity = if mode == PlayMode::Drums {
                110
            } else {
                (55.0 + 72.0 * ((pos.y - r.top()) / r.height()).clamp(0.0, 1.0)) as u8
            };
            match current {
                Some(i) if self.zone_of_touch[i].1 == z => {}
                Some(i) => {
                    let old = self.zone_of_touch[i].1;
                    self.zone_of_touch[i].1 = z;
                    self.zone_press(z, zones[z].held, velocity);
                    self.zone_release(old);
                }
                None => {
                    self.zone_of_touch.push((touch, z));
                    self.zone_press(z, zones[z].held, velocity);
                }
            }
        }

        // White keys and pads first, black keys on top.
        let mut order: Vec<usize> = (0..zones.len()).filter(|&i| !zones[i].black).collect();
        order.extend((0..zones.len()).filter(|&i| zones[i].black));
        for i in order {
            let z = zones[i];
            let down = self.zone_down[i] > 0;
            let r = z.rect;
            let radius = if z.black { 10.0 } else { 16.0 };
            if down {
                widgets::glow(p, r, radius, theme::alpha(theme::VIOLET, 0.55), 26.0);
                widgets::surface(p, r, radius, theme::VIOLET, theme::VIOLET_LIGHT);
            } else if z.black {
                widgets::surface(p, r, radius, theme::BG, theme::LINE);
            } else {
                widgets::surface(p, r, radius, theme::SURFACE_2, theme::LINE);
            }
            let text_color = if down { theme::ON_VIOLET } else { theme::TEXT };
            match z.label {
                Label::Note(note) if self.st.show_note_names && r.height() > 44.0 && !z.black => {
                    let style = grotesk(
                        if mode == PlayMode::Piano { 13.0 } else { 15.0 },
                        if down { 700 } else { 600 },
                    );
                    widgets::note_label(
                        p,
                        pos2(r.center().x, r.bottom() - 12.0),
                        Align2::CENTER_BOTTOM,
                        note,
                        style,
                        text_color,
                    );
                }
                Label::Chord(note, suffix) if r.height() > 44.0 => {
                    let style = grotesk(15.0, if down { 700 } else { 600 });
                    let at = pos2(r.center().x, r.bottom() - 12.0);
                    let w = widgets::note_label_width(p, note, style);
                    let sw = theme::galley(p, suffix, style, text_color).size().x;
                    let left = at.x - (w + sw) / 2.0;
                    widgets::note_label(
                        p,
                        pos2(left, at.y),
                        Align2::LEFT_BOTTOM,
                        note,
                        style,
                        text_color,
                    );
                    theme::text(
                        p,
                        pos2(left + w, at.y),
                        Align2::LEFT_BOTTOM,
                        suffix,
                        style,
                        text_color,
                    );
                }
                Label::Text(name) => {
                    theme::text(
                        p,
                        pos2(r.left() + 12.0, r.bottom() - 10.0),
                        Align2::LEFT_BOTTOM,
                        name,
                        inter(13.0, 600),
                        text_color,
                    );
                }
                _ => {}
            }
            if root_dots || mode == PlayMode::Piano {
                let dot = pos2(
                    r.center().x,
                    if z.black {
                        r.bottom() - 10.0
                    } else {
                        r.top() + 14.0
                    },
                );
                if z.root && mode != PlayMode::Drums {
                    p.circle_filled(dot, 4.0, if down { theme::ON_VIOLET } else { theme::AMBER });
                } else if mode == PlayMode::Piano && z.in_scale && self.st.locked {
                    p.circle_filled(
                        dot,
                        3.0,
                        if down {
                            theme::ON_VIOLET
                        } else {
                            theme::alpha(theme::VIOLET, 0.8)
                        },
                    );
                }
            }
        }
    }

    fn zone_press(&mut self, zone: usize, held: Held, velocity: u8) {
        if zone >= MAX_ZONES {
            return;
        }
        self.zone_down[zone] += 1;
        if self.zone_down[zone] > 1 {
            return;
        }
        self.zone_held[zone] = held;
        match held.key {
            Some(key) => self.send(Cmd::KeyDown { key, velocity }),
            None => {
                for &note in &held.notes[..held.count] {
                    self.send(Cmd::NoteOn { note, velocity });
                }
            }
        }
        self.haptic();
    }

    fn zone_release(&mut self, zone: usize) {
        if zone >= MAX_ZONES || self.zone_down[zone] == 0 {
            return;
        }
        self.zone_down[zone] -= 1;
        if self.zone_down[zone] == 0 {
            let held = self.zone_held[zone];
            match held.key {
                Some(key) => self.send(Cmd::KeyUp { key }),
                None => {
                    for &note in &held.notes[..held.count] {
                        self.send(Cmd::NoteOff { note });
                    }
                }
            }
        }
    }

    /// Lift every finger (mode, octave or key changes; leaving the screen).
    pub(crate) fn release_all_keys(&mut self) {
        self.zone_of_touch.clear();
        for zone in 0..MAX_ZONES {
            if self.zone_down[zone] > 0 {
                self.zone_down[zone] = 1;
                self.zone_release(zone);
            }
        }
    }

    pub(crate) fn set_play_mode(&mut self, mode: PlayMode) {
        if self.st.play_mode != mode {
            self.release_all_keys();
            self.st.play_mode = mode;
        }
    }

    /// The mode switch above the keyboard.
    pub(crate) fn mode_switch(&mut self, p: &Painter, rect: Rect) {
        p.rect_filled(rect, rect.height() / 2.0, theme::SURFACE_2);
        let w = (rect.width() - 6.0) / 4.0;
        for (i, mode) in PlayMode::ALL.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(rect.left() + 3.0 + i as f32 * w, rect.top() + 3.0),
                vec2(w, rect.height() - 6.0),
            );
            let tap = self.touches.button(Id::unique(("play-mode", i)), r);
            let active = self.st.play_mode == *mode;
            if active {
                p.rect_filled(r, r.height() / 2.0, theme::VIOLET);
            }
            let (style, color) = if active {
                (inter(13.0, 600), theme::ON_VIOLET)
            } else {
                (inter(13.0, 500), theme::TEXT)
            };
            theme::text(
                p,
                r.center(),
                Align2::CENTER_CENTER,
                mode.label(),
                style,
                color,
            );
            if tap.clicked {
                self.set_play_mode(*mode);
            }
        }
    }
}
