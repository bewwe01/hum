//! Design tokens (section 3.1 of the spec), fonts and text helpers.

use eframe::egui::{
    self, epaint::text::VariationCoords, text::LayoutJob, Align2, Color32, FontData,
    FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, TextFormat,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x0E, 0x0B, 0x1A);
pub const SURFACE: Color32 = Color32::from_rgb(0x1A, 0x15, 0x30);
pub const SURFACE_2: Color32 = Color32::from_rgb(0x24, 0x1D, 0x42);
pub const SURFACE_3: Color32 = Color32::from_rgb(0x2E, 0x24, 0x57);
pub const LINE: Color32 = Color32::from_rgb(0x33, 0x2B, 0x58);
pub const LINE_FAINT: Color32 = Color32::from_rgb(0x2A, 0x22, 0x48);
pub const BAR_LINE: Color32 = Color32::from_rgb(0x3E, 0x34, 0x68);
pub const DASHED_EMPTY: Color32 = Color32::from_rgb(0x4A, 0x3F, 0x7A);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xEE, 0xFF);
pub const MUTED: Color32 = Color32::from_rgb(0xA7, 0x9F, 0xC4);
pub const VIOLET: Color32 = Color32::from_rgb(0xA7, 0x8B, 0xFA);
pub const VIOLET_LIGHT: Color32 = Color32::from_rgb(0xC4, 0xB5, 0xFD);
pub const AMBER: Color32 = Color32::from_rgb(0xFF, 0xB5, 0x47);
pub const AMBER_LIGHT: Color32 = Color32::from_rgb(0xFF, 0xD9, 0xA0);
pub const ON_VIOLET: Color32 = Color32::from_rgb(0x14, 0x0D, 0x2B);
pub const ON_AMBER: Color32 = Color32::from_rgb(0x1A, 0x11, 0x00);

/// Screen padding and block gap.
pub const PAD: f32 = 20.0;
pub const PAD_BOTTOM: f32 = 16.0;
pub const GAP: f32 = 14.0;

/// Multiply a colour's alpha (colours here are opaque, so this is exact).
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        c.r(),
        c.g(),
        c.b(),
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// Font face. `Grotesk` is Space Grotesk (titles, names, numerals); `Inter`
/// is everything else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Grotesk,
    Inter,
}

/// A text style: face, size in points, weight (400..700) and tracking in em.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub face: Face,
    pub size: f32,
    pub weight: u16,
    pub tracking: f32,
}

pub const fn grotesk(size: f32, weight: u16) -> Style {
    Style {
        face: Face::Grotesk,
        size,
        weight,
        tracking: 0.0,
    }
}

pub const fn inter(size: f32, weight: u16) -> Style {
    Style {
        face: Face::Inter,
        size,
        weight,
        tracking: 0.0,
    }
}

impl Style {
    pub const fn tracked(mut self, em: f32) -> Self {
        self.tracking = em;
        self
    }

    fn family(&self) -> FontFamily {
        match self.face {
            Face::Grotesk => FontFamily::Name("grotesk".into()),
            Face::Inter => {
                if self.weight >= 600 {
                    FontFamily::Name("inter-600".into())
                } else if self.weight >= 500 {
                    FontFamily::Name("inter-500".into())
                } else {
                    FontFamily::Proportional
                }
            }
        }
    }

    pub fn format(&self, color: Color32) -> TextFormat {
        let mut f = TextFormat {
            font_id: FontId::new(self.size, self.family()),
            color,
            extra_letter_spacing: self.tracking * self.size,
            ..Default::default()
        };
        if self.face == Face::Grotesk {
            // Space Grotesk ships as one variable font; pick the weight here.
            f.coords = VariationCoords::new([(b"wght", f32::from(self.weight))]);
        }
        f
    }
}

pub const TITLE: Style = grotesk(30.0, 700);
pub const CAPTION: Style = inter(12.0, 400);
pub const SECTION: Style = inter(12.0, 600).tracked(0.06);

/// Lay out a single line of text.
pub fn galley(painter: &Painter, s: &str, style: Style, color: Color32) -> Arc<egui::Galley> {
    let job = LayoutJob::single_section(s.to_owned(), style.format(color));
    painter.layout_job(job)
}

/// Paint one line of text anchored at `pos`. Returns the painted rect.
pub fn text(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    s: &str,
    style: Style,
    color: Color32,
) -> Rect {
    let g = galley(painter, s, style, color);
    let rect = anchor.anchor_size(pos, g.size());
    painter.galley(rect.min, g, color);
    rect
}

/// Truncate `s` with an ellipsis so it fits in `max_w` points.
pub fn fit(painter: &Painter, s: &str, style: Style, max_w: f32) -> String {
    if galley(painter, s, style, TEXT).size().x <= max_w {
        return s.to_owned();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while chars.len() > 1 {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_owned() + "…";
        if galley(painter, &candidate, style, TEXT).size().x <= max_w {
            return candidate;
        }
    }
    s.to_owned()
}

/// Install the bundled fonts and the dark visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        "inter-400",
        include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        "inter-500",
        include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        "inter-600",
        include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        "grotesk",
        include_bytes!("../../../assets/fonts/SpaceGrotesk-Variable.ttf"),
    );

    let fam = |primary: &str| vec![primary.to_owned(), "inter-400".to_owned()];
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["inter-400".to_owned()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["inter-400".to_owned()]);
    fonts
        .families
        .insert(FontFamily::Name("inter-500".into()), fam("inter-500"));
    fonts
        .families
        .insert(FontFamily::Name("inter-600".into()), fam("inter-600"));
    fonts
        .families
        .insert(FontFamily::Name("grotesk".into()), fam("grotesk"));
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = alpha(VIOLET, 0.45);
    visuals.selection.stroke = egui::Stroke::new(1.0, VIOLET_LIGHT);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|s| {
        s.interaction.selectable_labels = false;
        s.spacing.item_spacing = egui::vec2(0.0, 0.0);
    });
}
