use std::path::PathBuf;

use egui::{FontId, Rect, Ui, Vec2, pos2, vec2};
use zen_ui::ide::IdePalette;
use zen_ui::offscreen::{self, Offscreen};
use zen_ui::theme::{self, family};

const SIZE: Vec2 = vec2(1100.0, 170.0);
const ROW: f32 = 24.0;

const CODE: &[&str] = &[
    "//! Zen Toolbox: the entry point to the Zen IDE family. It installs, updates and launches Zen IDEs",
    "//! (ADR-0008, ADR-0015, ADR-0018).",
    "Stack: Go 1.26, Fiber v3, GORM + pgxpool, Postgres, goose-миграции.",
    "    // course.LectorEnsurer <= lector.Service",
    "    var _ course.LectorEnsurer = (lector.Service)(nil)",
    "#![allow(clippy::print_stderr, reason = \"start-up errors are reported on the terminal\")]",
];

const TREE: &[&str] = &[
    "CHANGELOG.md",
    "CLAUDE.md",
    "container_services.go",
    "routes.go",
    "routes_test.go",
    "ai_scoring_instructions.md",
];

fn sample(ui: &mut Ui, p: &IdePalette) {
    let painter = ui.painter();
    let tree = Rect::from_min_size(pos2(0.0, 0.0), vec2(220.0, SIZE.y));
    let editor = Rect::from_min_max(pos2(tree.right() + 6.0, 0.0), pos2(SIZE.x, SIZE.y));
    painter.rect_filled(ui.max_rect(), 0.0, p.frame);
    painter.rect_filled(tree, 0.0, p.island);
    painter.rect_filled(editor, 0.0, p.island);
    for (row, name) in TREE.iter().enumerate() {
        let center = 10.0 + ROW * (row as f32 + 0.5);
        painter.text(pos2(40.5, center), egui::Align2::LEFT_CENTER, *name, theme::regular(13.0), p.text);
    }
    let gutter = FontId::new(12.5, egui::FontFamily::Monospace);
    for (row, line) in CODE.iter().enumerate() {
        let center = 10.0 + ROW * (row as f32 + 0.5);
        let number = format!("{}", row + 1);
        painter.text(
            pos2(editor.left() + 36.0, center),
            egui::Align2::RIGHT_CENTER,
            number,
            gutter.clone(),
            p.gutter_text,
        );
        let (font, color) = if line.starts_with("//!") {
            (FontId::new(13.0, family::mono_italic()), p.doc_comment)
        } else {
            (FontId::monospace(13.0), p.code)
        };
        painter.text(pos2(editor.left() + 64.0, center), egui::Align2::LEFT_CENTER, *line, font, color);
    }
}

fn main() -> Result<(), String> {
    let folder = std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: text_rendering <folder>")?;
    let p = IdePalette::zen_dark();
    for scale in [1, 2] {
        let mut offscreen = Offscreen::new(SIZE, scale as f32);
        offscreen.set_background(p.frame);
        let image = offscreen.render(|ui| sample(ui, &p));
        offscreen::write_png(&folder.join(format!("text@{scale}x.png")), &image)?;
    }
    Ok(())
}
