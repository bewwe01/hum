//! Songs screen: a library of chord sheets pasted from the browser, and
//! playing along with them (strum pad, chord pads, Follow and Auto).

use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, pos2, vec2, Align2, FontFamily, FontId, Id, Painter, Rect};
use hum_engine::chords::{guitar_voicing, piano_voicing, Chord};
use hum_engine::engine::{sf_sound, Cmd, SoundId, ACC_CHORDS, ACC_PATTERNS, STRUM_NOTES};

use crate::app::{HumApp, Tab};
use crate::icons;
use crate::songs::{Line, Sheet, Song};
use crate::theme::{self, grotesk, inter, Style, GAP};
use crate::widgets;

/// GM programs used for songs.
const GUITAR: u16 = 25;
const PIANO: u16 = 0;
const BEATS: [u8; 4] = [1, 2, 4, 8];

#[derive(Default)]
pub struct SongsUi {
    /// Index of the open song.
    open: Option<usize>,
    edit: Option<Editor>,
    /// Current chord (index into the sheet's chord sequence).
    pos: usize,
    /// The chord row the sheet last scrolled to.
    scrolled_to: Option<usize>,
    prev_voicing: Option<Vec<u8>>,
    paste: Option<Receiver<Option<String>>>,
    auto: bool,
    delete_armed: f64,
    /// Parsed sheet of the open song (re-parsed when the text changes).
    sheet: Option<(String, Sheet)>,
    /// The finger on the strum pad: (touch, anchor y, last direction).
    strum: Option<(u64, f32, i8)>,
}

struct Editor {
    index: Option<usize>,
    title: String,
    text: String,
}

/// One painted piece of the sheet, positioned relative to the sheet's top.
enum Item {
    Section(f32, String),
    Text(f32, String),
    Chord(Rect, String, usize),
}

impl HumApp {
    /// The song instrument while a song is open on the Songs screen.
    pub(crate) fn song_sound(&self) -> Option<SoundId> {
        if self.tab != Tab::Songs || self.settings_open || self.songs.edit.is_some() {
            return None;
        }
        let song = self.st.songs.get(self.songs.open?)?;
        Some(sf_sound(0, if song.piano { PIANO } else { GUITAR }))
    }

    /// Back action on the Songs screen; false when there is nothing to close.
    pub(crate) fn songs_back(&mut self) -> bool {
        if self.songs.edit.is_some() {
            self.songs.edit = None;
            true
        } else if self.songs.open.is_some() {
            self.close_song();
            true
        } else {
            false
        }
    }

    fn close_song(&mut self) {
        self.stop_auto();
        self.send(Cmd::StrumRelease);
        self.songs.open = None;
        self.songs.sheet = None;
    }

    fn open_song(&mut self, index: usize) {
        self.songs.open = Some(index);
        self.songs.pos = 0;
        self.songs.scrolled_to = None;
        self.songs.prev_voicing = None;
        self.songs.auto = false;
        self.songs.sheet = None;
        if self.st.looper.is_none() {
            self.set_bpm(self.st.songs[index].bpm);
        }
    }

    /// Developer screenshots: open the first song on its third chord.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn open_song_for_shot(&mut self) {
        if !self.st.songs.is_empty() && self.songs.open.is_none() {
            self.open_song(0);
            self.songs.pos = 6;
        }
    }

    /// Clipboard text requested by the Paste button.
    pub(crate) fn poll_paste(&mut self) {
        let Some(rx) = &self.songs.paste else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(text) => text,
            Err(TryRecvError::Empty) => {
                self.repaint
                    .request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => None,
        };
        self.songs.paste = None;
        match result {
            Some(text) if !text.trim().is_empty() => {
                let song = Song::from_text(&text);
                if Sheet::parse(&song.text).sequence().is_empty() {
                    self.show_toast("No chords found in what you copied");
                } else {
                    self.st.songs.insert(0, song);
                    self.open_song(0);
                    self.show_toast("Song added");
                }
            }
            _ => self.show_toast("Nothing to paste: copy a chord sheet first"),
        }
    }

    pub(crate) fn songs_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        if self.songs.open.is_some_and(|i| i >= self.st.songs.len()) {
            self.songs.open = None;
        }
        if self.songs.edit.is_some() {
            self.song_editor(ui, rect);
        } else if let Some(index) = self.songs.open {
            self.song_view(ui, rect, index);
        } else {
            self.song_library(ui, rect);
        }
    }

    // ------------------------------------------------------------------
    // Library

    fn song_library(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let cy = rect.top() + 22.0;
        theme::text(
            &p,
            pos2(rect.left(), cy),
            Align2::LEFT_CENTER,
            "Songs",
            theme::TITLE,
            theme::TEXT,
        );
        let paste = Rect::from_min_size(pos2(rect.right() - 100.0, rect.top()), vec2(100.0, 44.0));
        let typed = Rect::from_min_size(pos2(paste.left() - 88.0, rect.top()), vec2(80.0, 44.0));
        if self.text_button(&p, paste, "song-paste", "Paste", true) && self.songs.paste.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.songs.paste = Some(rx);
            self.platform.request_paste(tx);
        }
        if self.text_button(&p, typed, "song-type", "Type", false) {
            self.songs.edit = Some(Editor {
                index: None,
                title: String::new(),
                text: String::new(),
            });
        }

        let list = Rect::from_min_max(pos2(rect.left(), rect.top() + 44.0 + GAP), rect.max);
        if self.st.songs.is_empty() {
            let card = Rect::from_min_size(list.min, vec2(list.width(), 150.0));
            widgets::surface(&p, card, 20.0, theme::SURFACE, theme::LINE);
            let inner = card.shrink(18.0);
            theme::text(
                &p,
                inner.left_top(),
                Align2::LEFT_TOP,
                "Play along with songs",
                grotesk(17.0, 600),
                theme::TEXT,
            );
            widgets::wrapped(
                &p,
                inner.left_top() + vec2(0.0, 30.0),
                inner.width(),
                "In your browser, select and copy a chord sheet (chords above the \
                 lyrics, as on tab sites), then tap Paste. Or tap Type to write one.",
                inter(14.0, 400),
                theme::MUTED,
            );
            return;
        }

        const ROW_H: f32 = 64.0;
        const ROW_GAP: f32 = 8.0;
        let id = Id::unique("song-list");
        self.touches.claim(id, list);
        let content = self.st.songs.len() as f32 * (ROW_H + ROW_GAP) - ROW_GAP;
        let scroll = self.scroll(ui, id, list, content);
        let mut tapped = None;
        for t in self.touches.owned(id) {
            if t.is_tap() {
                let row = ((t.pos.y - list.top() + scroll) / (ROW_H + ROW_GAP)) as usize;
                tapped = Some(row);
            }
        }
        let clip = p.with_clip_rect(list);
        for (i, song) in self.st.songs.iter().enumerate() {
            let row = Rect::from_min_size(
                pos2(
                    list.left(),
                    list.top() - scroll + i as f32 * (ROW_H + ROW_GAP),
                ),
                vec2(list.width(), ROW_H),
            );
            if row.bottom() < list.top() || row.top() > list.bottom() {
                continue;
            }
            widgets::surface(&clip, row, 18.0, theme::SURFACE, theme::LINE);
            let sheet = Sheet::parse(&song.text);
            let flats = sheet.uses_flats();
            let names: Vec<String> = sheet
                .distinct()
                .iter()
                .take(6)
                .map(|c| c.transposed(i32::from(song.transpose)).name(flats))
                .collect();
            let w = row.width() - 36.0;
            let title = theme::fit(&clip, &song.title, grotesk(17.0, 600), w);
            theme::text(
                &clip,
                pos2(row.left() + 18.0, row.top() + 11.0),
                Align2::LEFT_TOP,
                &title,
                grotesk(17.0, 600),
                theme::TEXT,
            );
            let mut sub = names.join("  ");
            if song.capo > 0 {
                sub = format!("Capo {} · {sub}", song.capo);
            }
            let sub = theme::fit(&clip, &sub, inter(12.0, 400), w);
            theme::text(
                &clip,
                pos2(row.left() + 18.0, row.bottom() - 11.0),
                Align2::LEFT_BOTTOM,
                &sub,
                inter(12.0, 400),
                theme::MUTED,
            );
        }
        if let Some(i) = tapped.filter(|&i| i < self.st.songs.len()) {
            self.open_song(i);
        }
    }

    /// Vertical drag/wheel scrolling for a list claimed by `id`.
    fn scroll(&mut self, ui: &egui::Ui, id: Id, rect: Rect, content: f32) -> f32 {
        let key = id.with("scroll");
        let mut scroll: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
        for t in self.touches.owned(id) {
            if !t.ended && t.travel > crate::input::TAP_SLOP {
                scroll -= t.delta().y;
            }
        }
        if ui.rect_contains_pointer(rect) {
            scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        scroll = scroll.clamp(0.0, (content - rect.height()).max(0.0));
        ui.data_mut(|d| d.insert_temp(key, scroll));
        scroll
    }

    fn text_button(
        &mut self,
        p: &Painter,
        r: Rect,
        id: &'static str,
        label: &str,
        primary: bool,
    ) -> bool {
        let tap = self.touches.button(Id::unique(id), r);
        let (fill, border, color) = if primary {
            (theme::AMBER, theme::AMBER_LIGHT, theme::ON_AMBER)
        } else {
            (theme::SURFACE, theme::LINE, theme::TEXT)
        };
        widgets::surface(
            p,
            r,
            r.height() / 2.0,
            widgets::pressed(fill, tap.down),
            border,
        );
        theme::text(
            p,
            r.center(),
            Align2::CENTER_CENTER,
            label,
            inter(14.0, 600),
            color,
        );
        tap.clicked
    }

    // ------------------------------------------------------------------
    // Editor

    fn song_editor(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let back = Rect::from_min_size(rect.min, vec2(44.0, 44.0));
        let tap = self.touches.button(Id::unique("song-edit-back"), back);
        widgets::surface(
            &p,
            back,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::chevron_left(&p, back.center(), 24.0, theme::TEXT);
        if tap.clicked {
            self.songs.edit = None;
            return;
        }
        let save = Rect::from_min_size(pos2(rect.right() - 96.0, rect.top()), vec2(96.0, 44.0));
        let new = self.songs.edit.as_ref().is_some_and(|e| e.index.is_none());
        theme::text(
            &p,
            pos2(back.right() + 12.0, back.center().y),
            Align2::LEFT_CENTER,
            if new { "New song" } else { "Edit song" },
            grotesk(20.0, 700),
            theme::TEXT,
        );
        let saved = self.text_button(&p, save, "song-save", "Save", true);

        let font = FontId::new(15.0, FontFamily::Proportional);
        let title_rect = Rect::from_min_size(
            pos2(rect.left(), rect.top() + 44.0 + 12.0),
            vec2(rect.width(), 40.0),
        );
        let text_rect = Rect::from_min_max(pos2(rect.left(), title_rect.bottom() + 10.0), rect.max);
        widgets::surface(&p, text_rect, 14.0, theme::SURFACE, theme::LINE);
        if let Some(ed) = self.songs.edit.as_mut() {
            ui.put(
                title_rect,
                egui::TextEdit::singleline(&mut ed.title)
                    .hint_text("Title")
                    .font(font.clone())
                    .margin(vec2(12.0, 10.0)),
            );
            let inner = text_rect.shrink(8.0);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut ed.text)
                            .hint_text("Chords above lyrics, for example:\n\nG          D\nHello, it's me\nEm         C\nI was wondering")
                            .font(font)
                            .frame(egui::Frame::NONE)
                            .desired_width(inner.width())
                            .desired_rows(((inner.height() / 20.0) as usize).max(4)),
                    );
                });
            });
        }
        if saved {
            let Some(ed) = self.songs.edit.take() else {
                return;
            };
            let mut song = match ed.index.and_then(|i| self.st.songs.get(i).cloned()) {
                Some(old) => Song {
                    text: ed.text.clone(),
                    ..old
                },
                None => Song::from_text(&ed.text),
            };
            song.text = ed.text.replace('\r', "");
            if !ed.title.trim().is_empty() {
                song.title = ed.title.trim().to_owned();
            }
            if Sheet::parse(&song.text).sequence().is_empty() {
                self.show_toast("Saved, but no chords were found in it");
            }
            let index = match ed.index {
                Some(i) if i < self.st.songs.len() => {
                    self.st.songs[i] = song;
                    i
                }
                _ => {
                    self.st.songs.insert(0, song);
                    0
                }
            };
            self.open_song(index);
        }
    }

    // ------------------------------------------------------------------
    // Playing a song

    fn voicing(&mut self, chord: &Chord, song: &Song) -> Vec<u8> {
        let c = chord.transposed(i32::from(song.transpose) + i32::from(song.capo));
        let mut notes = if song.piano {
            piano_voicing(&c, self.songs.prev_voicing.as_deref())
        } else {
            guitar_voicing(&c)
        };
        notes.truncate(STRUM_NOTES);
        if song.piano {
            self.songs.prev_voicing = Some(notes.clone());
        }
        notes
    }

    fn strum_chord(&mut self, chord: &Chord, song: &Song, up: bool, velocity: u8) {
        let v = self.voicing(chord, song);
        let mut notes = [0u8; STRUM_NOTES];
        notes[..v.len()].copy_from_slice(&v);
        let spread_ms = match (song.piano, up) {
            (true, _) => 3,
            (false, false) => 14,
            (false, true) => 10,
        };
        self.send(Cmd::Strum {
            notes,
            count: v.len() as u8,
            velocity,
            spread_ms,
            up,
            ring: false,
        });
    }

    fn stop_auto(&mut self) {
        if self.songs.auto {
            self.send(Cmd::AccPlay(false));
            self.songs.auto = false;
        }
    }

    /// (Re)send the song's chords to the accompaniment and play from the
    /// current chord.
    fn start_auto(&mut self, song: &Song, seq: &[(usize, usize, Chord)]) {
        if seq.is_empty() {
            return;
        }
        self.send(Cmd::AccBegin);
        self.songs.prev_voicing = None;
        for (_, _, chord) in seq.iter().take(ACC_CHORDS) {
            let v = self.voicing(chord, song);
            let mut notes = [0u8; STRUM_NOTES];
            notes[..v.len()].copy_from_slice(&v);
            self.send(Cmd::AccChord {
                notes,
                count: v.len() as u8,
                steps: u16::from(song.beats) * 4,
            });
        }
        self.send(Cmd::AccPattern(song.pattern));
        self.send(Cmd::AccJump(self.songs.pos.min(ACC_CHORDS - 1) as u16));
        self.send(Cmd::AccPlay(true));
        self.songs.auto = true;
    }

    fn song_view(&mut self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let p = ui.painter().clone();
        let text = self.st.songs[index].text.clone();
        if self.songs.sheet.as_ref().is_none_or(|(t, _)| *t != text) {
            self.songs.sheet = Some((text.clone(), Sheet::parse(&text)));
        }
        let sheet = self
            .songs
            .sheet
            .as_ref()
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        let seq = sheet.sequence();
        if self.songs.auto && !self.status.running {
            self.songs.auto = false;
        }
        if self.songs.auto && self.status.acc_index >= 0 {
            self.songs.pos = self.status.acc_index as usize;
        }
        if !seq.is_empty() {
            self.songs.pos = self.songs.pos.min(seq.len() - 1);
        }

        // ---- layout --------------------------------------------------------
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 44.0));
        let (controls_a, controls_b, sheet_rect, follow, pads);
        if self.landscape {
            let split = rect.left() + rect.width() * 0.54;
            controls_a = None;
            let right = Rect::from_min_max(pos2(split + 12.0, header.bottom() + 10.0), rect.max);
            controls_b = Rect::from_min_max(
                pos2(rect.left(), rect.bottom() - 44.0),
                pos2(split, rect.bottom()),
            );
            sheet_rect = Rect::from_min_max(
                pos2(rect.left(), header.bottom() + 10.0),
                pos2(split, controls_b.top() - 10.0),
            );
            let follow_h = (right.height() * 0.45).clamp(84.0, 130.0);
            follow = Rect::from_min_size(right.min, vec2(right.width(), follow_h));
            pads = Rect::from_min_max(pos2(right.left(), follow.bottom() + 10.0), right.max);
        } else {
            let a = Rect::from_min_size(
                pos2(rect.left(), header.bottom() + 10.0),
                vec2(rect.width(), 44.0),
            );
            controls_a = Some(a);
            controls_b = Rect::from_min_size(
                pos2(rect.left(), a.bottom() + 10.0),
                vec2(rect.width(), 44.0),
            );
            pads = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 100.0), rect.max);
            follow = Rect::from_min_size(
                pos2(rect.left(), pads.top() - 10.0 - 96.0),
                vec2(rect.width(), 96.0),
            );
            sheet_rect = Rect::from_min_max(
                pos2(rect.left(), controls_b.bottom() + 10.0),
                pos2(rect.right(), follow.top() - 10.0),
            );
        }

        // ---- header ----------------------------------------------------------
        let back = Rect::from_min_size(header.min, vec2(44.0, 44.0));
        let tap = self.touches.button(Id::unique("song-back"), back);
        widgets::surface(
            &p,
            back,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::chevron_left(&p, back.center(), 24.0, theme::TEXT);
        if tap.clicked {
            self.close_song();
            return;
        }
        let mut right = header.right();
        let trash = Rect::from_min_size(pos2(right - 44.0, header.top()), vec2(44.0, 44.0));
        right = trash.left() - 8.0;
        let tap = self.touches.button(Id::unique("song-delete"), trash);
        let armed = self.now < self.songs.delete_armed;
        widgets::surface(
            &p,
            trash,
            22.0,
            widgets::pressed(if armed { theme::AMBER } else { theme::SURFACE }, tap.down),
            theme::LINE,
        );
        icons::trash(
            &p,
            trash.center(),
            22.0,
            if armed { theme::ON_AMBER } else { theme::TEXT },
        );
        if tap.clicked {
            if armed {
                self.close_song();
                self.st.songs.remove(index);
                self.show_toast("Song deleted");
                return;
            }
            self.songs.delete_armed = self.now + 2.5;
            self.show_toast("Tap the bin again to delete this song");
        }
        let edit = Rect::from_min_size(pos2(right - 60.0, header.top()), vec2(60.0, 44.0));
        right = edit.left() - 8.0;
        if self.text_button(&p, edit, "song-edit", "Edit", false) {
            let song = &self.st.songs[index];
            self.songs.edit = Some(Editor {
                index: Some(index),
                title: song.title.clone(),
                text: song.text.clone(),
            });
            self.stop_auto();
            return;
        }
        if controls_a.is_none() {
            let a = Rect::from_min_max(
                pos2(
                    right - 360.0_f32.min(right - back.right() - 120.0),
                    header.top(),
                ),
                pos2(right, header.bottom()),
            );
            right = a.left() - 8.0;
            self.song_controls_a(&p, a, index, &seq);
        }
        let title_w = right - back.right() - 12.0;
        let title = theme::fit(&p, &self.st.songs[index].title, grotesk(18.0, 600), title_w);
        theme::text(
            &p,
            pos2(back.right() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            &title,
            grotesk(18.0, 600),
            theme::TEXT,
        );
        if let Some(a) = controls_a {
            self.song_controls_a(&p, a, index, &seq);
        }
        if index >= self.st.songs.len() {
            return;
        }
        self.song_controls_b(&p, controls_b, index, &seq);

        // ---- sheet, follow pad, chord pads ---------------------------------
        self.song_sheet(ui, &p, sheet_rect, index, &sheet, &seq);
        self.follow_pad(&p, follow, index, &seq);
        self.chord_pads(&p, pads, index, &sheet);
        if self.status.running {
            self.repaint.request_repaint();
        }
    }

    /// Instrument, key (transpose) and capo.
    fn song_controls_a(
        &mut self,
        p: &Painter,
        rect: Rect,
        index: usize,
        seq: &[(usize, usize, Chord)],
    ) {
        let song = self.st.songs[index].clone();
        let chip_w = 84.0;
        let chip = Rect::from_min_size(rect.min, vec2(chip_w, rect.height()));
        let label = if song.piano { "Piano" } else { "Guitar" };
        let mut changed = false;
        if self.text_button(p, chip, "song-inst", label, false) {
            self.st.songs[index].piano = !song.piano;
            self.songs.prev_voicing = None;
            changed = true;
        }
        let w = (rect.right() - chip.right() - 16.0) / 2.0;
        let key = Rect::from_min_size(pos2(chip.right() + 8.0, rect.top()), vec2(w, rect.height()));
        let capo = Rect::from_min_size(pos2(key.right() + 8.0, rect.top()), vec2(w, rect.height()));
        let t = song.transpose;
        let key_text = if t == 0 {
            "0".to_owned()
        } else {
            format!("{t:+}")
        };
        let d = self.stepper(p, key, "song-key", "KEY", &key_text);
        if d != 0 {
            let s = &mut self.st.songs[index];
            s.transpose = (i32::from(s.transpose) + d).clamp(-11, 11) as i8;
            changed = true;
        }
        let d = self.stepper(p, capo, "song-capo", "CAPO", &song.capo.to_string());
        if d != 0 {
            let s = &mut self.st.songs[index];
            s.capo = (i32::from(s.capo) + d).clamp(0, 11) as i8;
            changed = true;
        }
        if changed && self.songs.auto {
            let song = self.st.songs[index].clone();
            self.start_auto(&song, seq);
        }
    }

    /// Minus, small label over a value, plus. Returns -1, 0 or 1.
    fn stepper(
        &mut self,
        p: &Painter,
        rect: Rect,
        id: &'static str,
        label: &str,
        value: &str,
    ) -> i32 {
        widgets::surface(p, rect, rect.height() / 2.0, theme::SURFACE, theme::LINE);
        let bw = 40.0_f32.min(rect.width() / 3.0);
        let minus = Rect::from_min_size(rect.min, vec2(bw, rect.height()));
        let plus =
            Rect::from_min_size(pos2(rect.right() - bw, rect.top()), vec2(bw, rect.height()));
        let mut delta = 0;
        for (i, (r, d)) in [(minus, -1), (plus, 1)].into_iter().enumerate() {
            let tap = self.touches.button(Id::unique((id, i)), r);
            if tap.down {
                p.circle_filled(r.center(), 16.0, theme::SURFACE_2);
            }
            if i == 0 {
                icons::minus(p, r.center(), 20.0, theme::TEXT);
            } else {
                icons::plus(p, r.center(), 20.0, theme::TEXT);
            }
            if tap.clicked {
                delta = d;
            }
        }
        let c = rect.center();
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_BOTTOM,
            label,
            inter(10.0, 600).tracked(0.06),
            theme::MUTED,
        );
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_TOP,
            value,
            grotesk(15.0, 600),
            theme::TEXT,
        );
        delta
    }

    /// Auto play, pattern, beats per chord and tempo.
    fn song_controls_b(
        &mut self,
        p: &Painter,
        rect: Rect,
        index: usize,
        seq: &[(usize, usize, Chord)],
    ) {
        let song = self.st.songs[index].clone();
        let play_c = pos2(rect.left() + 22.0, rect.center().y);
        let tap = self.touches.button(
            Id::unique("song-auto"),
            Rect::from_center_size(play_c, vec2(44.0, 44.0)),
        );
        let on = self.songs.auto;
        widgets::circle(
            p,
            play_c,
            22.0,
            widgets::pressed(if on { theme::VIOLET } else { theme::SURFACE }, tap.down),
            if on { theme::VIOLET_LIGHT } else { theme::LINE },
        );
        if on {
            icons::pause(p, play_c, 22.0, theme::ON_VIOLET);
        } else {
            icons::play(p, play_c + vec2(1.0, 0.0), 22.0, theme::TEXT);
        }
        if tap.clicked {
            if on {
                self.stop_auto();
            } else {
                self.start_auto(&song, seq);
            }
        }
        let bpm = self.bpm_pill(p, rect.right(), rect.center().y);
        if (self.st.songs[index].bpm - self.st.bpm).abs() > 0.1 && self.st.looper.is_none() {
            self.st.songs[index].bpm = self.st.bpm;
        }
        let left = play_c.x + 22.0 + 8.0;
        let room = bpm.left() - 8.0 - left;
        let beats_w = (room * 0.36).min(64.0);
        let pattern = Rect::from_min_size(
            pos2(left, rect.top()),
            vec2(room - beats_w - 8.0, rect.height()),
        );
        let beats = Rect::from_min_size(
            pos2(pattern.right() + 8.0, rect.top()),
            vec2(beats_w, rect.height()),
        );
        let mut changed = false;
        let name = ACC_PATTERNS[usize::from(song.pattern).min(ACC_PATTERNS.len() - 1)];
        let tap = self.touches.button(Id::unique("song-pattern"), pattern);
        widgets::surface(
            p,
            pattern,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let fitted = theme::fit(p, name, inter(13.0, 600), pattern.width() - 16.0);
        theme::text(
            p,
            pattern.center(),
            Align2::CENTER_CENTER,
            &fitted,
            inter(13.0, 600),
            theme::TEXT,
        );
        if tap.clicked {
            self.st.songs[index].pattern = (song.pattern + 1) % ACC_PATTERNS.len() as u8;
            changed = true;
        }
        let tap = self.touches.button(Id::unique("song-beats"), beats);
        widgets::surface(
            p,
            beats,
            22.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        let c = beats.center();
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_BOTTOM,
            "BEATS",
            inter(10.0, 600).tracked(0.06),
            theme::MUTED,
        );
        theme::text(
            p,
            pos2(c.x, c.y - 1.0),
            Align2::CENTER_TOP,
            &song.beats.to_string(),
            grotesk(15.0, 600),
            theme::TEXT,
        );
        if tap.clicked {
            let i = BEATS.iter().position(|&b| b == song.beats).unwrap_or(2);
            self.st.songs[index].beats = BEATS[(i + 1) % BEATS.len()];
            changed = true;
        }
        if changed {
            if self.songs.auto {
                let song = self.st.songs[index].clone();
                self.start_auto(&song, seq);
            } else {
                let what = if self.st.songs[index].beats != song.beats {
                    format!(
                        "Each chord lasts {} beats in Auto",
                        self.st.songs[index].beats
                    )
                } else {
                    format!(
                        "Auto plays: {}",
                        ACC_PATTERNS[usize::from(self.st.songs[index].pattern)]
                    )
                };
                self.show_toast(&what);
            }
        }
    }

    fn sheet_items(
        p: &Painter,
        sheet: &Sheet,
        size: f32,
        transpose: i32,
        flats: bool,
    ) -> (Vec<Item>, f32) {
        let lyric_style = inter(size, 400);
        let chord_style = grotesk(size, 700);
        let width = |s: &str, st: Style| theme::galley(p, s, st, theme::TEXT).size().x;
        let char_w = width("abcdefghij klmnopqrs", lyric_style) / 20.0;
        let line_h = size * 1.45;
        let mut items = Vec::new();
        let mut y = 0.0;
        let mut seq = 0;
        for line in &sheet.lines {
            match line {
                Line::Blank => y += line_h * 0.5,
                Line::Section(name) => {
                    y += 4.0;
                    items.push(Item::Section(y, name.clone()));
                    y += line_h + 4.0;
                }
                Line::Lyric(text) => {
                    items.push(Item::Text(y, text.clone()));
                    y += line_h;
                }
                Line::Chords { chords, lyric } => {
                    let mut min_x = 0.0f32;
                    for (col, chord, _) in chords {
                        let x = match lyric {
                            Some(l) => {
                                let len = l.chars().count();
                                let prefix: String = l.chars().take(*col).collect();
                                width(&prefix, lyric_style)
                                    + col.saturating_sub(len) as f32 * char_w
                            }
                            None => *col as f32 * char_w,
                        }
                        .max(min_x);
                        let name = chord.transposed(transpose).name(flats);
                        let w = width(&name, chord_style);
                        let r = Rect::from_min_size(pos2(x, y), vec2(w, line_h));
                        min_x = r.right() + size * 0.7;
                        items.push(Item::Chord(r, name, seq));
                        seq += 1;
                    }
                    y += line_h;
                    if let Some(l) = lyric {
                        items.push(Item::Text(y, l.clone()));
                        y += line_h;
                    }
                }
            }
        }
        (items, y)
    }

    fn song_sheet(
        &mut self,
        ui: &egui::Ui,
        p: &Painter,
        rect: Rect,
        index: usize,
        sheet: &Sheet,
        seq: &[(usize, usize, Chord)],
    ) {
        widgets::surface(p, rect, 18.0, theme::SURFACE, theme::LINE);
        let inner = rect.shrink2(vec2(14.0, 10.0));
        let song = &self.st.songs[index];
        let transpose = i32::from(song.transpose);
        let flats = sheet.uses_flats();
        // Shrink the text until the widest line fits (down to 11 points).
        let mut size = 15.0;
        let widest = |size: f32| {
            let (items, _) = Self::sheet_items(p, sheet, size, transpose, flats);
            items
                .iter()
                .map(|it| match it {
                    Item::Text(_, t) => theme::galley(p, t, inter(size, 400), theme::TEXT).size().x,
                    Item::Chord(r, _, _) => r.right(),
                    Item::Section(..) => 0.0,
                })
                .fold(0.0f32, f32::max)
        };
        let w = widest(size);
        if w > inner.width() {
            size = (size * inner.width() / w).max(11.0);
        }
        let (items, content) = Self::sheet_items(p, sheet, size, transpose, flats);

        let id = Id::unique("song-sheet");
        self.touches.claim(id, rect);
        let key = id.with("scroll");
        // Keep the current chord in view when it changes.
        if self.songs.scrolled_to != Some(self.songs.pos) {
            self.songs.scrolled_to = Some(self.songs.pos);
            if let Some(Item::Chord(r, _, _)) = items
                .iter()
                .find(|it| matches!(it, Item::Chord(_, _, s) if *s == self.songs.pos))
            {
                let current: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
                if r.top() < current || r.bottom() + size * 1.5 > current + inner.height() {
                    let target = (r.top() - inner.height() * 0.3).max(0.0);
                    ui.data_mut(|d| d.insert_temp(key, target));
                }
            }
        }
        let scroll = self.scroll(ui, id, inner, content);
        let origin = pos2(inner.left(), inner.top() - scroll);
        let mut jump = None;
        for t in self.touches.owned(id) {
            if t.is_tap() {
                for it in &items {
                    if let Item::Chord(r, _, s) = it {
                        if r.translate(origin.to_vec2()).expand(6.0).contains(t.pos) {
                            jump = Some(*s);
                        }
                    }
                }
            }
        }
        let clip = p.with_clip_rect(inner.expand2(vec2(6.0, 4.0)));
        let line_h = size * 1.45;
        for it in &items {
            match it {
                Item::Section(y, name) => {
                    let top = origin.y + y;
                    if top > inner.bottom() || top + line_h < inner.top() {
                        continue;
                    }
                    let tw = theme::galley(&clip, name, inter(12.0, 600), theme::AMBER)
                        .size()
                        .x;
                    let pill =
                        Rect::from_min_size(pos2(origin.x - 2.0, top), vec2(tw + 20.0, line_h));
                    clip.rect_filled(pill, line_h / 2.0, theme::alpha(theme::AMBER, 0.14));
                    theme::text(
                        &clip,
                        pill.center(),
                        Align2::CENTER_CENTER,
                        name,
                        inter(12.0, 600),
                        theme::AMBER,
                    );
                }
                Item::Text(y, text) => {
                    let top = origin.y + y;
                    if top > inner.bottom() || top + line_h < inner.top() {
                        continue;
                    }
                    theme::text(
                        &clip,
                        pos2(origin.x, top + line_h / 2.0),
                        Align2::LEFT_CENTER,
                        text,
                        inter(size, 400),
                        theme::TEXT,
                    );
                }
                Item::Chord(r, name, s) => {
                    let r = r.translate(origin.to_vec2());
                    if r.top() > inner.bottom() || r.bottom() < inner.top() {
                        continue;
                    }
                    let current = *s == self.songs.pos;
                    let color = if current {
                        clip.rect_filled(r.expand2(vec2(5.0, -1.0)), 7.0, theme::AMBER);
                        theme::ON_AMBER
                    } else {
                        theme::VIOLET_LIGHT
                    };
                    theme::text(
                        &clip,
                        r.center(),
                        Align2::CENTER_CENTER,
                        name,
                        grotesk(size, 700),
                        color,
                    );
                }
            }
        }
        if seq.is_empty() {
            theme::text(
                p,
                rect.center(),
                Align2::CENTER_CENTER,
                "No chords found: tap Edit",
                inter(14.0, 500),
                theme::MUTED,
            );
        }
        if let Some(s) = jump.filter(|&s| s < seq.len()) {
            self.songs.pos = s;
            self.songs.scrolled_to = Some(s);
            let song = self.st.songs[index].clone();
            self.strum_chord(&seq[s].2, &song, false, 100);
            if self.songs.auto {
                self.send(Cmd::AccJump(s as u16));
            }
        }
    }

    /// The strum pad (current chord) with previous / next buttons.
    fn follow_pad(&mut self, p: &Painter, rect: Rect, index: usize, seq: &[(usize, usize, Chord)]) {
        let song = self.st.songs[index].clone();
        let flats = self
            .songs
            .sheet
            .as_ref()
            .is_some_and(|(_, s)| s.uses_flats());
        let prev = Rect::from_min_size(rect.min, vec2(48.0, rect.height()));
        let next = Rect::from_min_size(
            pos2(rect.right() - 92.0, rect.top()),
            vec2(92.0, rect.height()),
        );
        let pad = Rect::from_min_max(
            pos2(prev.right() + 8.0, rect.top()),
            pos2(next.left() - 8.0, rect.bottom()),
        );
        let mut moved = None;
        let tap = self.touches.button(Id::unique("song-prev"), prev);
        widgets::surface(
            p,
            prev,
            18.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::chevron_left(p, prev.center(), 24.0, theme::TEXT);
        if tap.pressed && !seq.is_empty() {
            moved = Some((self.songs.pos + seq.len() - 1) % seq.len());
        }
        let tap = self.touches.button(Id::unique("song-next"), next);
        widgets::surface(
            p,
            next,
            18.0,
            widgets::pressed(theme::AMBER, tap.down),
            theme::AMBER_LIGHT,
        );
        theme::text(
            p,
            pos2(next.center().x, next.center().y - 2.0),
            Align2::CENTER_BOTTOM,
            "Next",
            inter(15.0, 600),
            theme::ON_AMBER,
        );
        if let Some((_, _, c)) = seq.get((self.songs.pos + 1) % seq.len().max(1)) {
            let name = c.transposed(i32::from(song.transpose)).name(flats);
            theme::text(
                p,
                pos2(next.center().x, next.center().y + 2.0),
                Align2::CENTER_TOP,
                &name,
                grotesk(15.0, 700),
                theme::ON_AMBER,
            );
        }
        if tap.pressed && !seq.is_empty() {
            moved = Some((self.songs.pos + 1) % seq.len());
        }
        if let Some(pos) = moved {
            self.songs.pos = pos;
            self.strum_chord(&seq[pos].2, &song, false, 104);
            if self.songs.auto {
                self.send(Cmd::AccJump(pos as u16));
            }
            self.haptic();
        }

        // The strum pad: touch strums down, then swipe up and down.
        let id = Id::unique("song-strum");
        self.touches.claim(id, pad);
        let mut strums: Vec<(bool, u8)> = Vec::new();
        let touches: Vec<(u64, f32, bool, bool)> = self
            .touches
            .owned(id)
            .map(|t| (t.id, t.pos.y, t.started, t.ended))
            .collect();
        for (tid, y, started, ended) in touches {
            if started {
                self.songs.strum = Some((tid, y, 1));
                strums.push((false, 100));
            } else if let Some((sid, anchor, dir)) = self.songs.strum {
                if sid != tid {
                    continue;
                }
                if ended {
                    self.songs.strum = None;
                    continue;
                }
                let dy = y - anchor;
                let threshold = 26.0;
                if dir > 0 && dy < -threshold {
                    self.songs.strum = Some((tid, y, -1));
                    strums.push((true, 88));
                } else if dir < 0 && dy > threshold {
                    self.songs.strum = Some((tid, y, 1));
                    strums.push((false, 100));
                } else if (dir > 0 && dy > 0.0) || (dir < 0 && dy < 0.0) {
                    self.songs.strum = Some((tid, y, dir));
                }
            }
        }
        let down = self.songs.strum.is_some();
        if down {
            widgets::glow(p, pad, 18.0, theme::alpha(theme::VIOLET, 0.4), 20.0);
        }
        widgets::surface(
            p,
            pad,
            18.0,
            if down {
                theme::SURFACE_3
            } else {
                theme::SURFACE_2
            },
            if down { theme::VIOLET } else { theme::LINE },
        );
        if let Some((_, _, chord)) = seq.get(self.songs.pos) {
            let name = chord.transposed(i32::from(song.transpose)).name(flats);
            theme::text(
                p,
                pos2(pad.center().x, pad.center().y + 4.0),
                Align2::CENTER_BOTTOM,
                &name,
                grotesk(30.0, 700),
                theme::TEXT,
            );
            let hint = if song.piano {
                "Tap to play"
            } else {
                "Tap or swipe to strum"
            };
            let hint = theme::fit(p, hint, inter(12.0, 400), pad.width() - 12.0);
            theme::text(
                p,
                pos2(pad.center().x, pad.center().y + 10.0),
                Align2::CENTER_TOP,
                &hint,
                inter(12.0, 400),
                theme::MUTED,
            );
            let chord = *chord;
            for (up, velocity) in strums {
                self.strum_chord(&chord, &song, up, velocity);
            }
        }
    }

    /// One pad per distinct chord of the song.
    fn chord_pads(&mut self, p: &Painter, rect: Rect, index: usize, sheet: &Sheet) {
        let song = self.st.songs[index].clone();
        let chords = sheet.distinct();
        if chords.is_empty() {
            return;
        }
        let flats = sheet.uses_flats();
        let gap = 8.0;
        let cols = if self.landscape { 3 } else { 4 };
        let rows = (((rect.height() + gap) / (44.0 + gap)) as usize).max(1);
        let rows = rows.min(chords.len().div_ceil(cols));
        let w = (rect.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = ((rect.height() - gap * (rows as f32 - 1.0)) / rows as f32).min(64.0);
        let current = self
            .songs
            .sheet
            .as_ref()
            .and_then(|(_, s)| s.sequence().get(self.songs.pos).map(|x| x.2));
        for (i, chord) in chords.iter().take(cols * rows).enumerate() {
            let r = Rect::from_min_size(
                pos2(
                    rect.left() + (i % cols) as f32 * (w + gap),
                    rect.top() + (i / cols) as f32 * (h + gap),
                ),
                vec2(w, h),
            );
            let tap = self.touches.button(Id::unique(("song-pad", i)), r);
            let is_current = current == Some(*chord);
            let (fill, border, color) = if tap.down {
                (theme::VIOLET, theme::VIOLET_LIGHT, theme::ON_VIOLET)
            } else if is_current {
                (theme::SURFACE_3, theme::AMBER, theme::TEXT)
            } else {
                (theme::SURFACE, theme::LINE, theme::TEXT)
            };
            widgets::surface(p, r, 14.0, fill, border);
            let name = chord.transposed(i32::from(song.transpose)).name(flats);
            let name = theme::fit(p, &name, grotesk(17.0, 700), r.width() - 8.0);
            theme::text(
                p,
                r.center(),
                Align2::CENTER_CENTER,
                &name,
                grotesk(17.0, 700),
                color,
            );
            if tap.pressed {
                self.strum_chord(chord, &song, false, 100);
                self.haptic();
            }
        }
    }
}
