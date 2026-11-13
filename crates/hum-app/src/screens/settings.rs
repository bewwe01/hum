//! Settings screen (spec section 3.6). Pushed over Play; no tab bar.

use std::sync::atomic::Ordering;

use eframe::egui::{self, pos2, vec2, Align2, Id, Rect, Stroke};
use hum_engine::presets::PRESETS;

use crate::app::HumApp;
use crate::audio::BufferMode;
use crate::icons;
use crate::theme::{self, inter, GAP};
use crate::widgets;

const CARD_PAD: f32 = 16.0;

impl HumApp {
    pub(crate) fn settings_screen(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();

        // ---- header (fixed) -------------------------------------------------
        let back = Rect::from_min_size(rect.min, vec2(48.0, 48.0));
        let tap = self.touches.button(Id::unique("settings-back"), back);
        widgets::surface(
            &p,
            back,
            16.0,
            widgets::pressed(theme::SURFACE, tap.down),
            theme::LINE,
        );
        icons::chevron_left(&p, back.center(), 24.0, theme::TEXT);
        if tap.clicked {
            self.settings_open = false;
        }
        theme::text(
            &p,
            pos2(back.right() + 14.0, back.center().y),
            Align2::LEFT_CENTER,
            "Settings",
            theme::TITLE,
            theme::TEXT,
        );

        // ---- scrolling body ---------------------------------------------------
        let view = Rect::from_min_max(pos2(rect.left(), back.bottom() + GAP), rect.max);
        let scroll_id = Id::unique("settings-scroll");
        let height_id = Id::unique("settings-height");
        let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
        let content_h: f32 = ui.data(|d| d.get_temp(height_id)).unwrap_or(0.0);
        let clip = p.with_clip_rect(view);
        let mut y = view.top() - scroll;
        let w = view.width();
        let x = view.left();

        // AUDIO
        y = section_label(&clip, x, y, "AUDIO");
        let helper = "Lower is more responsive. If you hear crackling, pick Safe.";
        let helper_h = measure(&clip, helper, w - 2.0 * CARD_PAD);
        let card_h = CARD_PAD + 20.0 + 6.0 + helper_h + 12.0 + 48.0 + 12.0 + 1.0 + 52.0;
        let card = Rect::from_min_size(pos2(x, y), vec2(w, card_h));
        widgets::surface(&clip, card, 22.0, theme::SURFACE, theme::LINE);
        let mut cy = card.top() + CARD_PAD;
        theme::text(
            &clip,
            pos2(card.left() + CARD_PAD, cy),
            Align2::LEFT_TOP,
            "Audio buffer",
            inter(16.0, 600),
            theme::TEXT,
        );
        cy += 20.0 + 6.0;
        widgets::wrapped(
            &clip,
            pos2(card.left() + CARD_PAD, cy),
            w - 2.0 * CARD_PAD,
            helper,
            inter(12.0, 400),
            theme::MUTED,
        );
        cy += helper_h + 12.0;
        // Segmented control.
        let track = Rect::from_min_size(
            pos2(card.left() + CARD_PAD, cy),
            vec2(w - 2.0 * CARD_PAD, 48.0),
        );
        clip.rect_filled(track, 24.0, theme::SURFACE_2);
        let seg_w = (track.width() - 8.0) / 3.0;
        for (i, mode) in BufferMode::ALL.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(track.left() + 4.0 + i as f32 * seg_w, track.top() + 4.0),
                vec2(seg_w, 40.0),
            );
            let tap = self
                .touches
                .button(Id::unique(("buffer", i)), r.intersect(view));
            let active = self.st.buffer == *mode;
            if active {
                clip.rect_filled(r, 20.0, theme::VIOLET);
            }
            let (style, color) = if active {
                (inter(14.0, 600), theme::ON_VIOLET)
            } else {
                (inter(14.0, 500), theme::TEXT)
            };
            theme::text(
                &clip,
                r.center(),
                Align2::CENTER_CENTER,
                mode.label(),
                style,
                color,
            );
            if tap.clicked && !active {
                self.st.buffer = *mode;
                let mode = *mode;
                self.with_audio(|a| a.set_mode(mode));
            }
        }
        cy += 48.0 + 12.0;
        clip.line_segment(
            [
                pos2(card.left() + CARD_PAD, cy),
                pos2(card.right() - CARD_PAD, cy),
            ],
            Stroke::new(1.0, theme::LINE),
        );
        cy += 1.0;
        let (latency, detail) = self
            .with_audio(|a| {
                let rate = a.info.sample_rate.load(Ordering::Relaxed);
                let frames = a.info.callback_frames.load(Ordering::Relaxed);
                if a.is_silent() {
                    (
                        "Silent".to_owned(),
                        "No audio device found; the engine runs without sound".to_owned(),
                    )
                } else if !a.is_running() {
                    (
                        "No audio output".to_owned(),
                        a.last_error()
                            .unwrap_or("The audio stream is closed")
                            .to_owned(),
                    )
                } else {
                    let latency = a
                        .latency_ms()
                        .map_or("Measuring…".to_owned(), |ms| format!("{ms:.0} ms"));
                    let detail = if frames > 0 {
                        format!("{rate} Hz · {frames} frames per callback")
                    } else {
                        format!("{rate} Hz")
                    };
                    (latency, detail)
                }
            })
            .unwrap_or_default();
        let row_c = cy + 26.0;
        theme::text(
            &clip,
            pos2(card.left() + CARD_PAD, row_c - 1.0),
            Align2::LEFT_BOTTOM,
            "Measured latency",
            inter(15.0, 500),
            theme::TEXT,
        );
        let latency_w = theme::galley(&clip, &latency, theme::grotesk(15.0, 600), theme::TEXT)
            .size()
            .x;
        let detail = theme::fit(
            &clip,
            &detail,
            inter(12.0, 400),
            w - 2.0 * CARD_PAD - latency_w - 12.0,
        );
        theme::text(
            &clip,
            pos2(card.left() + CARD_PAD, row_c + 2.0),
            Align2::LEFT_TOP,
            &detail,
            inter(12.0, 400),
            theme::MUTED,
        );
        theme::text(
            &clip,
            pos2(card.right() - CARD_PAD, row_c),
            Align2::RIGHT_CENTER,
            &latency,
            theme::grotesk(15.0, 600),
            theme::TEXT,
        );
        y = card.bottom() + GAP;

        // PLAYING
        y = section_label(&clip, x, y, "PLAYING");
        let card = Rect::from_min_size(pos2(x, y), vec2(w, 3.0 * 60.0 + 2.0));
        widgets::surface(&clip, card, 22.0, theme::SURFACE, theme::LINE);
        let rows = [
            ("Show note names", "Labels on the keys"),
            ("Haptic feedback", "A small buzz on every key"),
            ("Keep screen on", "While you are playing"),
        ];
        for (i, (label, helper)) in rows.iter().enumerate() {
            let row = Rect::from_min_size(
                pos2(card.left(), card.top() + i as f32 * 61.0),
                vec2(w, 60.0),
            );
            if i > 0 {
                clip.line_segment(
                    [
                        pos2(row.left() + CARD_PAD, row.top() - 0.5),
                        pos2(row.right() - CARD_PAD, row.top() - 0.5),
                    ],
                    Stroke::new(1.0, theme::LINE),
                );
            }
            theme::text(
                &clip,
                pos2(row.left() + CARD_PAD, row.center().y - 1.0),
                Align2::LEFT_BOTTOM,
                label,
                inter(15.0, 500),
                theme::TEXT,
            );
            theme::text(
                &clip,
                pos2(row.left() + CARD_PAD, row.center().y + 2.0),
                Align2::LEFT_TOP,
                helper,
                inter(12.0, 400),
                theme::MUTED,
            );
            let touch = Rect::from_center_size(
                pos2(row.right() - CARD_PAD - 26.0, row.center().y),
                vec2(60.0, 44.0),
            );
            let tap = self
                .touches
                .button(Id::unique(("switch", i)), touch.intersect(view));
            let value = match i {
                0 => &mut self.st.show_note_names,
                1 => &mut self.st.haptics,
                _ => &mut self.st.keep_screen_on,
            };
            if tap.clicked {
                *value = !*value;
            }
            let on = *value;
            widgets::switch(&clip, touch, on);
            if tap.clicked && i == 1 && on {
                self.haptic();
            }
        }
        y = card.bottom() + GAP;

        // ABOUT
        y = section_label(&clip, x, y, "ABOUT");
        let promise = "No permissions, no network, no tracking. Everything stays on your phone.";
        let promise_w = w - 2.0 * CARD_PAD - 24.0 - 12.0;
        let promise_h = measure(&clip, promise, promise_w).max(24.0);
        let card_h = 48.0 + 1.0 + 48.0 + 1.0 + 14.0 + promise_h + 16.0;
        let card = Rect::from_min_size(pos2(x, y), vec2(w, card_h));
        widgets::surface(&clip, card, 22.0, theme::SURFACE, theme::LINE);
        let mut cy = card.top();
        for (label, value) in [
            ("Version", crate::VERSION.to_owned()),
            ("Sounds", PRESETS.len().to_string()),
        ] {
            theme::text(
                &clip,
                pos2(card.left() + CARD_PAD, cy + 24.0),
                Align2::LEFT_CENTER,
                label,
                inter(15.0, 500),
                theme::TEXT,
            );
            theme::text(
                &clip,
                pos2(card.right() - CARD_PAD, cy + 24.0),
                Align2::RIGHT_CENTER,
                &value,
                theme::grotesk(15.0, 600),
                theme::MUTED,
            );
            cy += 48.0;
            clip.line_segment(
                [
                    pos2(card.left() + CARD_PAD, cy),
                    pos2(card.right() - CARD_PAD, cy),
                ],
                Stroke::new(1.0, theme::LINE),
            );
            cy += 1.0;
        }
        cy += 14.0;
        icons::shield(
            &clip,
            pos2(card.left() + CARD_PAD + 12.0, cy + 12.0),
            24.0,
            theme::VIOLET,
        );
        widgets::wrapped(
            &clip,
            pos2(
                card.left() + CARD_PAD + 36.0,
                cy + (24.0 - promise_h.min(24.0)) * 0.5,
            ),
            promise_w,
            promise,
            inter(13.0, 400),
            theme::MUTED,
        );
        y = card.bottom();

        // ---- scrolling (only when the content is taller than the view) --------
        let total = y + scroll - view.top();
        ui.data_mut(|d| d.insert_temp(height_id, total));
        let max_scroll = (content_h.max(total) - view.height()).max(0.0);
        let id = Id::unique("settings-body");
        self.touches.claim(id, view);
        for t in self.touches.owned(id) {
            if !t.ended {
                scroll -= t.delta().y;
            }
        }
        if ui.rect_contains_pointer(view) {
            scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        ui.data_mut(|d| d.insert_temp(scroll_id, scroll.clamp(0.0, max_scroll)));
    }
}

fn section_label(p: &egui::Painter, x: f32, y: f32, text: &str) -> f32 {
    theme::text(
        p,
        pos2(x + 4.0, y),
        Align2::LEFT_TOP,
        text,
        theme::SECTION,
        theme::MUTED,
    );
    y + 16.0 + 8.0
}

fn measure(p: &egui::Painter, text: &str, width: f32) -> f32 {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        inter(12.0, 400).format(theme::MUTED),
    );
    job.wrap.max_width = width;
    p.layout_job(job).size().y
}
