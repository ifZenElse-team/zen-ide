use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub window: [Color32; 4],
    pub window_edge: Color32,
    pub panel: Color32,
    pub panel_edge: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: [Color32; 2],
    pub accent_hover: [Color32; 2],
    pub on_accent: Color32,
    pub link: Color32,
    pub running: Color32,
    pub attention: Color32,
    pub popup: Color32,
}

impl Palette {
    pub const DARK: Palette = Palette {
        dark: true,
        window: [
            Color32::from_rgb(0x24, 0x20, 0x3A),
            Color32::from_rgb(0x17, 0x18, 0x24),
            Color32::from_rgb(0x12, 0x13, 0x1B),
            Color32::from_rgb(0x0E, 0x0F, 0x15),
        ],
        window_edge: Color32::from_rgba_premultiplied(0x2A, 0x2A, 0x33, 0x80),
        panel: Color32::from_rgba_premultiplied(0x0B, 0x0B, 0x0D, 0x0D),
        panel_edge: Color32::from_rgba_premultiplied(0x10, 0x10, 0x12, 0x12),
        hover: Color32::from_rgba_premultiplied(0x0F, 0x0F, 0x12, 0x10),
        text: Color32::from_rgb(0xEE, 0xEF, 0xF4),
        text_secondary: Color32::from_rgb(0x9A, 0x9E, 0xB0),
        text_muted: Color32::from_rgb(0x6B, 0x6F, 0x80),
        accent: [Color32::from_rgb(0x7C, 0x7F, 0xF7), Color32::from_rgb(0x5B, 0x5E, 0xE8)],
        accent_hover: [Color32::from_rgb(0x8E, 0x91, 0xFA), Color32::from_rgb(0x6A, 0x6D, 0xF0)],
        on_accent: Color32::WHITE,
        link: Color32::from_rgb(0xA9, 0xAC, 0xFF),
        running: Color32::from_rgb(0x4A, 0xDE, 0x80),
        attention: Color32::from_rgb(0xF5, 0x9E, 0x0B),
        popup: Color32::from_rgb(0x23, 0x24, 0x31),
    };

    pub const LIGHT: Palette = Palette {
        dark: false,
        window: [
            Color32::from_rgb(0xEE, 0xEC, 0xFB),
            Color32::from_rgb(0xF5, 0xF6, 0xFA),
            Color32::from_rgb(0xF3, 0xF4, 0xF8),
            Color32::from_rgb(0xEB, 0xED, 0xF3),
        ],
        window_edge: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x1A),
        panel: Color32::from_rgba_premultiplied(0xE6, 0xE6, 0xE6, 0xE6),
        panel_edge: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x0F),
        hover: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x0A),
        text: Color32::from_rgb(0x1A, 0x1C, 0x24),
        text_secondary: Color32::from_rgb(0x5D, 0x63, 0x73),
        text_muted: Color32::from_rgb(0x95, 0x9A, 0xA8),
        accent: [Color32::from_rgb(0x63, 0x66, 0xF1), Color32::from_rgb(0x4F, 0x46, 0xE5)],
        accent_hover: [Color32::from_rgb(0x73, 0x76, 0xF4), Color32::from_rgb(0x5B, 0x53, 0xEA)],
        on_accent: Color32::WHITE,
        link: Color32::from_rgb(0x4F, 0x46, 0xE5),
        running: Color32::from_rgb(0x16, 0xA3, 0x4A),
        attention: Color32::from_rgb(0xD9, 0x77, 0x06),
        popup: Color32::WHITE,
    };

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        visuals.window_fill = self.popup;
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_corner_radius = egui::CornerRadius::same(10);
        visuals.menu_corner_radius = egui::CornerRadius::same(10);
        visuals.window_stroke = egui::Stroke::new(1.0, self.panel_edge);
        visuals.selection.bg_fill = self.accent[1];
        visuals.hyperlink_color = self.link;
        visuals.override_text_color = Some(self.text);
        visuals.widgets.hovered.weak_bg_fill = self.hover;
        visuals.widgets.hovered.bg_fill = self.hover;
        visuals.widgets.active.weak_bg_fill = self.hover;
        visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
            widget.corner_radius = egui::CornerRadius::same(6);
        }
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_visuals(visuals);
        ctx.all_styles_mut(|style| {
            style.spacing.scroll = egui::style::ScrollStyle::floating();
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
            style.spacing.menu_margin = egui::Margin::same(6);
            style.text_styles.insert(egui::TextStyle::Button, regular(13.5));
            style.text_styles.insert(egui::TextStyle::Body, regular(13.5));
        });
    }
}

pub mod family {
    use egui::FontFamily;

    pub fn regular() -> FontFamily {
        FontFamily::Proportional
    }

    pub fn semibold() -> FontFamily {
        FontFamily::Name("zen-semibold".into())
    }

    pub fn bold() -> FontFamily {
        FontFamily::Name("zen-bold".into())
    }

    pub fn mono_bold() -> FontFamily {
        FontFamily::Name("zen-mono-bold".into())
    }

    pub fn mono_italic() -> FontFamily {
        FontFamily::Name("zen-mono-italic".into())
    }

    pub fn editor() -> FontFamily {
        FontFamily::Name("zen-editor".into())
    }

    pub fn editor_bold() -> FontFamily {
        FontFamily::Name("zen-editor-bold".into())
    }

    pub fn editor_italic() -> FontFamily {
        FontFamily::Name("zen-editor-italic".into())
    }

    pub fn editor_preview() -> FontFamily {
        FontFamily::Name("zen-editor-preview".into())
    }

    pub fn editor_preview_bold() -> FontFamily {
        FontFamily::Name("zen-editor-preview-bold".into())
    }

    pub fn editor_preview_italic() -> FontFamily {
        FontFamily::Name("zen-editor-preview-italic".into())
    }

    pub fn bold_of(family: &FontFamily) -> FontFamily {
        if *family == editor() {
            editor_bold()
        } else if *family == editor_preview() {
            editor_preview_bold()
        } else {
            mono_bold()
        }
    }
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, family::regular())
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, family::semibold())
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, family::bold())
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    egui_extras::install_image_loaders(ctx);
}

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions(&EditorFaces::default(), &EditorFaces::default()));
}

#[derive(Clone)]
pub struct Face {
    pub bytes: Arc<Vec<u8>>,
    pub index: u32,
}

#[derive(Clone, Default)]
pub struct EditorFaces {
    pub regular: Option<Face>,
    pub bold: Option<Face>,
    pub italic: Option<Face>,
}

pub fn font_definitions(editor: &EditorFaces, preview: &EditorFaces) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        ("inter-regular", crate::fonts::INTER_REGULAR),
        ("inter-semibold", include_bytes!("../../../../assets/fonts/Inter-SemiBold.ttf")),
        ("inter-bold", crate::fonts::INTER_BOLD),
    ];
    let mono_fallbacks = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mono_faces: [(&str, &'static [u8]); 3] = [
        (MONO, include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf")),
        (MONO_BOLD, include_bytes!("../../../../assets/fonts/JetBrainsMono-Bold.ttf")),
        (MONO_ITALIC, include_bytes!("../../../../assets/fonts/JetBrainsMono-Italic.ttf")),
    ];
    for (name, bytes) in mono_faces.into_iter().chain(faces) {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let chain = |primary: Vec<String>, fallbacks: &[String]| -> Vec<String> {
        primary.into_iter().chain(fallbacks.iter().cloned()).collect()
    };
    fonts.families.insert(FontFamily::Monospace, chain(vec![MONO.to_owned()], &mono_fallbacks));
    fonts.families.insert(family::mono_bold(), chain(vec![MONO_BOLD.to_owned()], &mono_fallbacks));
    fonts.families.insert(family::mono_italic(), chain(vec![MONO_ITALIC.to_owned()], &mono_fallbacks));
    for (prefix, chosen, families) in [
        ("zen-editor", editor, [family::editor(), family::editor_bold(), family::editor_italic()]),
        (
            "zen-editor-preview",
            preview,
            [family::editor_preview(), family::editor_preview_bold(), family::editor_preview_italic()],
        ),
    ] {
        let [regular, bold, italic] = families;
        for (suffix, face, mono, family) in [
            ("regular", &chosen.regular, MONO, regular),
            ("bold", &chosen.bold, MONO_BOLD, bold),
            ("italic", &chosen.italic, MONO_ITALIC, italic),
        ] {
            let mut primary = Vec::new();
            if let Some(face) = face {
                let name = format!("{prefix}-{suffix}");
                let mut data = FontData::from_owned(face.bytes.as_ref().clone());
                data.index = face.index;
                fonts.font_data.insert(name.clone(), Arc::new(data));
                primary.push(name);
            }
            primary.push(mono.to_owned());
            fonts.families.insert(family, chain(primary, &mono_fallbacks));
        }
    }
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.insert(FontFamily::Proportional, chain(vec!["inter-regular".to_owned()], &fallbacks));
    fonts.families.insert(family::semibold(), chain(vec!["inter-semibold".to_owned()], &fallbacks));
    fonts.families.insert(family::bold(), chain(vec!["inter-bold".to_owned()], &fallbacks));
    fonts
}

const MONO: &str = "bundled-mono";
const MONO_BOLD: &str = "bundled-mono-bold";
const MONO_ITALIC: &str = "bundled-mono-italic";

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_mono() -> Face {
        Face {
            bytes: Arc::new(include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf").to_vec()),
            index: 0,
        }
    }

    #[test]
    fn the_editor_takes_the_bundled_font_without_a_chosen_font() {
        let fonts = font_definitions(&EditorFaces::default(), &EditorFaces::default());
        for family in [family::editor(), family::editor_preview(), FontFamily::Monospace] {
            assert_eq!(fonts.families[&family][0], MONO, "{family:?}");
        }
        assert_eq!(fonts.families[&family::editor_bold()][0], MONO_BOLD);
        assert_eq!(fonts.families[&family::editor_italic()][0], MONO_ITALIC);
        assert!(!fonts.font_data.contains_key("zen-editor-regular"));
    }

    #[test]
    fn a_chosen_font_comes_first_with_the_bundled_font_behind_it() {
        let chosen = EditorFaces { regular: Some(Face { index: 2, ..bundled_mono() }), ..EditorFaces::default() };
        let fonts = font_definitions(&chosen, &EditorFaces::default());
        let editor = &fonts.families[&family::editor()];
        assert_eq!(editor[..2], ["zen-editor-regular".to_owned(), MONO.to_owned()]);
        assert_eq!(fonts.font_data["zen-editor-regular"].index, 2);
        assert_eq!(
            fonts.families[&family::editor_bold()][0],
            MONO_BOLD,
            "a missing bold face takes the bundled font's"
        );
        assert_eq!(fonts.families[&family::editor_preview()][0], MONO, "the preview has its own faces");
        assert_eq!(fonts.families[&FontFamily::Monospace][0], MONO, "the rest of the UI keeps the bundled font");
    }
}
