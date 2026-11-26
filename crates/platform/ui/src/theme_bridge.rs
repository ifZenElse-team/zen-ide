use egui::text::TextFormat;
use egui::{Color32, FontId, Stroke};
use zen_syntax::HighlightKind;
use zen_theme::{Rgba, Style, SyntaxKey, Underline};

pub fn color(rgba: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba.r(), rgba.g(), rgba.b(), rgba.a())
}

pub fn syntax_key(kind: HighlightKind) -> SyntaxKey {
    match kind {
        HighlightKind::Keyword => SyntaxKey::Keyword,
        HighlightKind::String => SyntaxKey::String,
        HighlightKind::Number => SyntaxKey::Number,
        HighlightKind::Comment => SyntaxKey::Comment,
        HighlightKind::DocComment => SyntaxKey::DocComment,
        HighlightKind::Macro => SyntaxKey::Macro,
        HighlightKind::Type => SyntaxKey::Type,
        HighlightKind::Function => SyntaxKey::Function,
        HighlightKind::Lifetime => SyntaxKey::Lifetime,
        HighlightKind::Constant => SyntaxKey::Constant,
        HighlightKind::Attribute => SyntaxKey::Attribute,
        HighlightKind::Punctuation => SyntaxKey::Punctuation,
        HighlightKind::Heading => SyntaxKey::Heading,
        HighlightKind::Strong => SyntaxKey::Strong,
        HighlightKind::Emphasis => SyntaxKey::Emphasis,
        HighlightKind::Link => SyntaxKey::Link,
        HighlightKind::Code => SyntaxKey::Code,
        HighlightKind::Property => SyntaxKey::Property,
        HighlightKind::GlobalVariable => SyntaxKey::GlobalVariable,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CodeStyle {
    pub color: Color32,
    pub background: Option<Color32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<Underline>,
    pub strikethrough: bool,
}

impl CodeStyle {
    pub fn from_style(style: Style) -> CodeStyle {
        CodeStyle {
            color: color(style.color),
            background: style.background.map(color),
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strikethrough: style.strikethrough,
        }
    }

    pub fn plain(color: Color32) -> CodeStyle {
        CodeStyle { color, background: None, bold: false, italic: false, underline: None, strikethrough: false }
    }

    pub fn format(&self, font: FontId) -> TextFormat {
        let line = |on: bool| if on { Stroke::new(1.0, self.color) } else { Stroke::NONE };
        let font_id =
            if self.bold { FontId::new(font.size, crate::theme::family::bold_of(&font.family)) } else { font };
        TextFormat {
            font_id,
            color: self.color,
            background: self.background.unwrap_or(Color32::TRANSPARENT),
            italics: self.italic,
            underline: line(self.underline.is_some()),
            strikethrough: line(self.strikethrough),
            ..TextFormat::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_keep_their_alpha() {
        assert_eq!(color(Rgba::rgb(0x25, 0x28, 0x2C)), Color32::from_rgb(0x25, 0x28, 0x2C));
        assert_eq!(color(Rgba::new(0, 0, 0, 110)), Color32::from_black_alpha(110));
        assert_eq!(color(Rgba::new(0x80, 0x80, 0x80, 0x30)), Color32::from_rgba_unmultiplied(0x80, 0x80, 0x80, 0x30));
    }

    #[test]
    fn styles_become_text_formats() {
        let font = FontId::monospace(13.0);
        let plain = CodeStyle::plain(Color32::RED).format(font.clone());
        assert_eq!(plain, TextFormat { font_id: font.clone(), color: Color32::RED, ..TextFormat::default() });

        let style = Style {
            background: Some(Rgba::rgb(0, 0, 0xFF)),
            bold: true,
            underline: Some(Underline::Wave),
            strikethrough: true,
            ..Style::color(Rgba::rgb(0xFF, 0, 0)).italic()
        };
        let format = CodeStyle::from_style(style).format(font);
        assert_eq!(format.font_id.family, crate::theme::family::mono_bold());
        let editor = CodeStyle::from_style(style).format(FontId::new(13.0, crate::theme::family::editor()));
        assert_eq!(
            editor.font_id.family,
            crate::theme::family::editor_bold(),
            "the editor's text is bold in its own font"
        );
        assert_eq!(format.background, Color32::BLUE);
        assert!(format.italics);
        assert_eq!(format.underline, Stroke::new(1.0, Color32::RED));
        assert_eq!(format.strikethrough, Stroke::new(1.0, Color32::RED));
    }

    #[test]
    fn every_highlight_kind_has_its_own_key() {
        let kinds = [
            HighlightKind::Keyword,
            HighlightKind::String,
            HighlightKind::Number,
            HighlightKind::Comment,
            HighlightKind::DocComment,
            HighlightKind::Macro,
            HighlightKind::Type,
            HighlightKind::Function,
            HighlightKind::Lifetime,
            HighlightKind::Constant,
            HighlightKind::Attribute,
            HighlightKind::Punctuation,
            HighlightKind::Heading,
            HighlightKind::Strong,
            HighlightKind::Emphasis,
            HighlightKind::Link,
            HighlightKind::Code,
        ];
        let keys: std::collections::BTreeSet<SyntaxKey> = kinds.into_iter().map(syntax_key).collect();
        assert_eq!(keys.len(), kinds.len());
    }
}
