//! The egui application.

use crate::theme;
use eframe::egui::{self, Align2};

pub struct HumApp {}

impl HumApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        Self {}
    }
}

impl eframe::App for HumApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, theme::BG);
        theme::text(
            painter,
            rect.center(),
            Align2::CENTER_CENTER,
            "Hum",
            theme::TITLE,
            theme::TEXT,
        );
    }
}
