use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use zen_i18n::t;

use super::WelcomeView;
use crate::{accessibility, theme};

const SITE: &str = "https://ide.ifzenelse.space/docs/";
const REFERENCE: &str = "keyboard-shortcuts";
const ROW: f32 = 30.0;

const CHAPTERS: &[&str] = &[
    "getting-started",
    "editor",
    "navigation",
    "find-and-replace",
    "project",
    "devops",
    "go",
    "java",
    "kotlin",
    "rust",
    "web",
    "terminal",
    "images",
    "settings",
    "plugins",
    "ai-agents",
    "updates",
    "toolbox",
    "version-control",
    "run-and-test",
    "debugging",
    "language-servers",
    "local-history",
    "notifications",
    "http-client",
    "appearance",
    "tool-windows",
    "accessibility",
    "caffeine",
    "scratches",
    "maven-and-gradle",
    "power-save-and-diagnostics",
    "database",
    "ant",
    "properties",
    "spring-boot",
    "docker",
    "shell-scripts",
    "kubernetes",
];

fn title(chapter: &str) -> String {
    t!(&format!("ide-welcome-learn-{chapter}"))
}

pub(in crate::ide) fn guide_url(params: &serde_json::Value) -> Result<String, String> {
    let Some(chapter) = params.get("chapter").and_then(serde_json::Value::as_str) else { return Ok(SITE.to_owned()) };
    if chapter == REFERENCE || CHAPTERS.contains(&chapter) {
        return Ok(format!("{SITE}{chapter}.html"));
    }
    Err(format!("no chapter `{chapter}`; the chapters are {} and {REFERENCE}", CHAPTERS.join(", ")))
}

impl WelcomeView {
    pub(super) fn learn(&mut self, ui: &mut Ui, main: Rect) {
        let p = self.palette;
        let pane = main.shrink2(vec2(20.0, 16.0));
        ui.painter().text(
            pane.left_top(),
            Align2::LEFT_TOP,
            t!("ide-welcome-learn-guide"),
            theme::semibold(15.0),
            p.text,
        );
        let mut list = ui.new_child(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(pane.left(), pane.top() + 30.0), pane.max)),
        );
        egui::ScrollArea::vertical().id_salt("welcome-learn").auto_shrink([false, false]).show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for chapter in CHAPTERS.iter().copied().chain([REFERENCE]) {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(row, 6, p.hover);
                }
                let name = title(chapter);
                ui.painter().text(
                    pos2(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    theme::regular(13.5),
                    p.text,
                );
                accessibility::name(&response, egui::Role::Link, &name);
                if response.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("{SITE}{chapter}.html")));
                }
            }
            ui.allocate_exact_size(Vec2::splat(ROW / 2.0), Sense::hover());
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_page_of_the_guide_is_a_chapter_in_order() {
        let guide = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/guide");
        let mut files: Vec<String> = std::fs::read_dir(guide)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.as_bytes()[0].is_ascii_digit())
            .collect();
        files.sort();
        let pages: Vec<&str> =
            files.iter().filter_map(|file| file.strip_suffix(".md")?.split_once('-')).map(|(_, slug)| slug).collect();
        assert_eq!(pages, CHAPTERS);
    }

    #[test]
    fn every_chapter_has_a_title_in_every_language() {
        zen_i18n::register(crate::toolbox::LOCALES);
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                for chapter in CHAPTERS.iter().copied().chain([REFERENCE]) {
                    assert!(!title(chapter).starts_with("ide-welcome-learn"), "{language}: {chapter}");
                }
            });
        }
    }

    #[test]
    fn a_chapter_is_a_page_of_the_website_and_an_unknown_one_is_refused() {
        assert_eq!(guide_url(&json!({})).unwrap(), SITE);
        assert_eq!(
            guide_url(&json!({ "chapter": "database" })).unwrap(),
            "https://ide.ifzenelse.space/docs/database.html"
        );
        assert!(guide_url(&json!({ "chapter": "nothing" })).unwrap_err().contains("getting-started"));
    }
}
