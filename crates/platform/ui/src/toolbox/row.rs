use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, Galley, Rect, Ui, Vec2, pos2, vec2};

use super::metrics;
use crate::{components, theme};

const GAP: f32 = 10.0;
const VERSION_LINES: usize = 3;
const SEPARATOR: &str = " · ";
const TEXT_X: f32 = 18.0 + metrics::MARK + 14.0;
const NAME_Y: f32 = 19.0;
const VERSION_Y: f32 = 39.0;

pub(super) struct RowContent<'a> {
    pub name: &'a str,
    pub name_color: Color32,
    pub version: &'a str,
    pub version_color: Color32,
    pub link: Option<(&'a str, Color32)>,
    pub progress: bool,
    pub button: Option<&'a str>,
    pub more: bool,
}

pub(super) struct RowLayout {
    pub height: f32,
    pub mark: Rect,
    pub name: Rect,
    pub name_galley: Arc<Galley>,
    pub version: Rect,
    pub version_galley: Arc<Galley>,
    pub link: Option<(Rect, Arc<Galley>)>,
    pub bar: Option<Rect>,
    pub button: Option<Rect>,
    pub more: Rect,
    pub elided: bool,
}

impl RowLayout {
    pub(super) fn new(ui: &Ui, width: f32, content: &RowContent) -> Self {
        let painter = ui.painter();
        let more = Rect::from_center_size(pos2(width - 16.0, 21.0), vec2(22.0, 26.0));
        let button = content.button.map(|label| {
            let size = components::primary_button_size(ui, label);
            let right = more.left() - 8.0;
            Rect::from_min_max(
                pos2(right - size.x, more.center().y - size.y / 2.0),
                pos2(right, more.center().y + size.y / 2.0),
            )
        });
        let column_right = match (button, content.more) {
            (Some(button), _) => button.left() - GAP,
            (None, true) => more.left() - GAP,
            (None, false) => width - 16.0,
        };
        let column = (column_right - TEXT_X).max(1.0);

        let mut name_job =
            LayoutJob::simple_singleline(content.name.to_owned(), theme::semibold(15.0), content.name_color);
        name_job.wrap =
            TextWrapping { max_width: column, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let name_galley = painter.layout_job(name_job);
        let name = Rect::from_min_size(pos2(TEXT_X, NAME_Y - name_galley.size().y / 2.0), name_galley.size());

        let version_font = theme::regular(13.0);
        let line_height = painter.layout_no_wrap("0".to_owned(), version_font.clone(), content.version_color).size().y;
        let text = break_at_separators(content.version, column, |text| {
            painter.layout_no_wrap(text.to_owned(), version_font.clone(), content.version_color).size().x
        });
        let mut version_job = LayoutJob::simple(text, version_font, content.version_color, column);
        version_job.wrap = TextWrapping {
            max_width: column,
            max_rows: VERSION_LINES,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        let version_galley = painter.layout_job(version_job);
        let version = Rect::from_min_size(pos2(TEXT_X, VERSION_Y - line_height / 2.0), version_galley.size());
        let extra = (version_galley.size().y - line_height).max(0.0);

        let link = content.link.filter(|_| !content.progress).map(|(text, color)| {
            let galley = painter.layout_no_wrap(text.to_owned(), theme::regular(13.0), color);
            let rect = Rect::from_min_size(pos2(TEXT_X, 58.0 + extra - galley.size().y / 2.0), galley.size());
            (rect, galley)
        });
        let bar =
            content.progress.then(|| Rect::from_min_max(pos2(TEXT_X, 56.0 + extra), pos2(width - 16.0, 59.0 + extra)));
        let base = if link.is_some() || bar.is_some() { metrics::ROW } else { metrics::ROW_COMPACT };

        Self {
            height: base + extra,
            mark: Rect::from_min_size(pos2(18.0, 9.0), Vec2::splat(metrics::MARK)),
            name,
            elided: name_galley.elided || version_galley.elided,
            name_galley,
            version,
            version_galley,
            link,
            bar,
            button,
            more,
        }
    }

    pub(super) fn at(mut self, origin: egui::Pos2) -> Self {
        let by = origin.to_vec2();
        self.mark = self.mark.translate(by);
        self.name = self.name.translate(by);
        self.version = self.version.translate(by);
        if let Some((rect, _)) = &mut self.link {
            *rect = rect.translate(by);
        }
        self.bar = self.bar.map(|bar| bar.translate(by));
        self.button = self.button.map(|button| button.translate(by));
        self.more = self.more.translate(by);
        self
    }
}

fn break_at_separators(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    let mut lines: Vec<String> = Vec::new();
    for part in text.split(SEPARATOR) {
        match lines.last_mut() {
            Some(line) if measure(&format!("{line}{SEPARATOR}{part}")) <= width => {
                line.push_str(SEPARATOR);
                line.push_str(part);
            }
            _ => lines.push(part.to_owned()),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_ui(check: impl FnOnce(&Ui)) {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut check = Some(check);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(check) = check.take() {
                check(ui);
            }
        })
        .textures_delta
        .clear();
    }

    #[test]
    fn a_version_line_wraps_at_its_separators() {
        let measure = |text: &str| text.chars().count() as f32;
        assert_eq!(
            break_at_separators("0.38.27 · Update to 0.38.31 · 32 MB", 100.0, measure),
            "0.38.27 · Update to 0.38.31 · 32 MB"
        );
        assert_eq!(
            break_at_separators("0.38.27 · Update to 0.38.31 · 32 MB", 27.0, measure),
            "0.38.27 · Update to 0.38.31\n32 MB"
        );
        assert_eq!(
            break_at_separators("0.38.27 · Update to 0.38.31 · 32 MB", 10.0, measure),
            "0.38.27\nUpdate to 0.38.31\n32 MB"
        );
        assert_eq!(break_at_separators("0.2.6", 1.0, measure), "0.2.6");
    }

    #[test]
    fn the_buttons_never_cover_the_text_of_a_row() {
        let rows: &[(&str, &str, &str, &str, Option<&str>)] = &[
            ("GoGarden", "0.38.27 · Update to 0.38.31 · 32 MB", "What's new", "Update", Some("menu")),
            ("GoGarden", "0.38.27 · Обновить до 0.38.31 · 32 МБ", "Что нового", "Обновить", Some("menu")),
            ("RustRonin", "0.2.6 · Update to 0.12.104 · 1024 MB", "What's new", "Update", Some("menu")),
            ("RustRonin", "0.2.6 · Обновить до 0.12.104 · 1024 МБ", "Что нового", "Обновить", Some("menu")),
            ("WebWave", "0.1.0 · 37 MB", "What's new", "Install", None),
            ("WebWave", "0.1.0 · 37 МБ", "Что нового", "Установить", None),
            ("SharpShuriken", "0.38.27", "", "", Some("menu")),
            ("SharpShuriken", "Скоро", "", "", None),
        ];
        with_ui(|ui| {
            for &(name, version, link, button, more) in rows {
                for with_link in [true, false] {
                    for width in [396.0, 360.0, 320.0, 280.0] {
                        let content = RowContent {
                            name,
                            name_color: Color32::WHITE,
                            version,
                            version_color: Color32::GRAY,
                            link: (with_link && !link.is_empty()).then_some((link, Color32::LIGHT_BLUE)),
                            progress: false,
                            button: (!button.is_empty()).then_some(button),
                            more: more.is_some(),
                        };
                        let layout = RowLayout::new(ui, width, &content).at(pos2(20.0, 300.0));
                        let row = Rect::from_min_size(pos2(20.0, 300.0), vec2(width, layout.height));
                        let mut texts = vec![("name", layout.name), ("version", layout.version)];
                        texts.extend(layout.link.as_ref().map(|(rect, _)| ("link", *rect)));
                        let mut buttons: Vec<(&str, Rect)> = more.map(|menu| (menu, layout.more)).into_iter().collect();
                        buttons.extend(layout.button.map(|rect| ("button", rect)));
                        for (text, text_rect) in &texts {
                            assert!(
                                row.contains_rect(*text_rect),
                                "{name} `{version}` at {width}: the {text} is outside the row"
                            );
                            for (control, control_rect) in &buttons {
                                assert!(
                                    !text_rect.intersects(*control_rect),
                                    "{name} `{version}` at {width}: the {control} {control_rect:?} covers the {text} {text_rect:?}"
                                );
                            }
                        }
                        if width >= 396.0 {
                            assert!(!layout.elided, "{name} `{version}`: the full text fits in the window");
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn a_wrapped_version_line_makes_the_row_taller_and_moves_the_link_down() {
        with_ui(|ui| {
            let content = |version| RowContent {
                name: "GoGarden",
                name_color: Color32::WHITE,
                version,
                version_color: Color32::GRAY,
                link: Some(("What's new", Color32::LIGHT_BLUE)),
                progress: false,
                button: Some("Update"),
                more: true,
            };
            let short = RowLayout::new(ui, 396.0, &content("0.38.27"));
            assert_eq!(short.height, metrics::ROW);
            let long = RowLayout::new(ui, 396.0, &content("0.38.27 · Update to 0.38.31 · 32 MB"));
            assert!(long.height > short.height);
            let (link, _) = long.link.as_ref().unwrap();
            assert!(link.top() >= long.version.bottom(), "the link stays under the version line");
            assert_eq!(long.version_galley.rows.len(), 2);
        });
    }
}
