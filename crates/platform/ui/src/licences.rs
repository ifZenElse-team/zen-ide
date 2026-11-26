use std::path::{Path, PathBuf};

use egui::{RichText, Ui, vec2};
use zen_i18n::t;

use crate::theme;

#[derive(Debug)]
pub struct LicencesView {
    folder: Option<PathBuf>,
    files: Vec<String>,
    chosen: usize,
    lines: Vec<String>,
}

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

impl LicencesView {
    pub fn new(folder: Option<PathBuf>) -> Self {
        let files = folder.as_deref().map(zen_update::licences::files).unwrap_or_default();
        let mut view = Self { folder, files, chosen: 0, lines: Vec::new() };
        view.choose(0);
        view
    }

    pub fn bundled() -> Self {
        Self::new(zen_update::licences::bundled())
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn chosen(&self) -> Option<&str> {
        self.files.get(self.chosen).map(String::as_str)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn choose(&mut self, index: usize) {
        self.chosen = index.min(self.files.len().saturating_sub(1));
        let text = match (&self.folder, self.files.get(self.chosen)) {
            (Some(folder), Some(file)) => zen_update::licences::read(folder, file).unwrap_or_default(),
            _ => String::new(),
        };
        let text = if self.chosen().is_some_and(|file| file.ends_with(".html")) { plain(&text) } else { text };
        self.lines = text.lines().map(str::to_owned).collect();
    }

    pub fn show(&mut self, ui: &mut Ui, frame: egui::Frame, product: &str) -> bool {
        let mut close = false;
        let mut picked = None;
        let modal = egui::Modal::new(ui.scope_id().with("open-source-licences")).frame(frame).show(ui.ctx(), |ui| {
            let size = ui.ctx().content_rect().size();
            let width = (size.x - 80.0).clamp(320.0, 860.0);
            let height = (size.y - 200.0).clamp(200.0, 520.0);
            ui.set_width(width);
            ui.label(RichText::new(t!("licences-title")).font(theme::semibold(15.0)).color(ui.visuals().text_color()));
            ui.add_space(6.0);
            let hint = if self.files.is_empty() {
                t!("licences-missing", product = product)
            } else {
                t!("licences-hint", product = product)
            };
            ui.add(
                egui::Label::new(RichText::new(hint).font(theme::regular(12.5)).color(ui.visuals().weak_text_color()))
                    .wrap(),
            );
            ui.add_space(10.0);
            if !self.files.is_empty() {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui(vec2((width * 0.3).clamp(110.0, 220.0), height), |ui| {
                        egui::ScrollArea::vertical().id_salt("licence-files").auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                for (index, file) in self.files.iter().enumerate() {
                                    let label = RichText::new(file.as_str()).font(theme::regular(12.5));
                                    if ui.selectable_label(index == self.chosen, label).clicked() {
                                        picked = Some(index);
                                    }
                                }
                            },
                        );
                    });
                    ui.separator();
                    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                        let font = egui::FontId::monospace(12.0);
                        let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
                        egui::ScrollArea::both()
                            .id_salt(("licence-text", self.chosen))
                            .auto_shrink([false, false])
                            .show_rows(ui, row, self.lines.len(), |ui, rows| {
                                for line in &self.lines[rows] {
                                    ui.add(egui::Label::new(RichText::new(line.as_str()).font(font.clone())).extend());
                                }
                            });
                    });
                });
                ui.add_space(12.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t!("licences-close")).clicked() {
                    close = true;
                }
            });
        });
        if let Some(index) = picked {
            self.choose(index);
        }
        close || modal.should_close()
    }
}

#[cfg(test)]
mod tests {
    use super::{LicencesView, plain};

    #[test]
    fn the_viewer_lists_the_notices_first_and_shows_the_chosen_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("LICENSE-MIT"), "MIT License\nline two").unwrap();
        std::fs::write(dir.path().join("THIRD-PARTY-NOTICES"), "Third-party notices for GoGarden").unwrap();
        let mut view = LicencesView::new(Some(dir.path().to_owned()));
        assert_eq!(view.files(), ["THIRD-PARTY-NOTICES", "LICENSE-MIT"]);
        assert_eq!(view.lines(), ["Third-party notices for GoGarden"]);
        view.choose(1);
        assert_eq!((view.chosen(), view.lines().len()), (Some("LICENSE-MIT"), 2));
        assert!(LicencesView::new(None).files().is_empty());
        assert_eq!(plain("<p>A &amp; B</p>"), "A & B");
    }

    #[test]
    fn the_viewer_draws_a_long_notices_file_only_as_far_as_it_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let notices: String = (0..20_000).map(|line| format!("crate-{line} 1.0.0, MIT\n")).collect();
        std::fs::write(dir.path().join("THIRD-PARTY-NOTICES"), notices).unwrap();
        std::fs::write(dir.path().join("LICENSE-MIT"), "MIT License").unwrap();
        let mut view = LicencesView::new(Some(dir.path().to_owned()));
        let mut offscreen = crate::offscreen::Offscreen::new(egui::vec2(1000.0, 800.0), 1.0);
        let frame = egui::Frame::popup(&egui::Style::default());
        let _ = offscreen.render(|ui| {
            let _ = view.show(ui, frame, "GoGarden");
        });
        let tree = offscreen.accessibility_tree(|ui| {
            let _ = view.show(ui, frame, "GoGarden");
        });
        let shown =
            |text: &str| tree.nodes.iter().any(|(_, node)| node.label() == Some(text) || node.value() == Some(text));
        assert!(shown("crate-0 1.0.0, MIT") && shown("LICENSE-MIT"));
        assert!(!shown("crate-19999 1.0.0, MIT"), "rows are drawn as they are seen");
    }
}
