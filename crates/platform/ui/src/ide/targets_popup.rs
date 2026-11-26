use std::path::{Path, PathBuf};

use egui::text::{LayoutJob, TextFormat};
use egui::{Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use crate::theme;

const ROW: f32 = 22.0;
const VISIBLE: usize = 12;

#[derive(Clone, Debug, PartialEq)]
struct Target {
    path: PathBuf,
    offset: usize,
    shown_path: String,
    line: u64,
    preview: String,
    highlight: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
struct Shown {
    title: String,
    targets: Vec<Target>,
    selected: usize,
    at: PopupAnchor,
    reveal: bool,
    create: bool,
}

#[derive(Default)]
pub(super) struct TargetsPopup {
    shown: Option<Shown>,
}

fn title(state: &Value, count: usize) -> String {
    let name = state["symbol"].as_str().unwrap_or_default().to_owned();
    match (state["kind"].as_str(), name.is_empty()) {
        (Some("implementation"), false) => t!("ide-choose-implementation-of", name = name, count = count),
        (Some("implementation"), true) => t!("ide-choose-implementation"),
        (Some("superMethod"), false) => t!("ide-choose-super-method-of", name = name, count = count),
        (Some("superMethod"), true) => t!("ide-choose-super-method"),
        (Some("typeDeclaration"), _) => t!("ide-choose-type-declaration"),
        (Some("test"), false) => t!("ide-choose-test-of", name = name, count = count),
        (Some("test"), true) => t!("ide-choose-test"),
        (Some("testSubject"), false) => t!("ide-choose-test-subject-of", name = name, count = count),
        (Some("testSubject"), true) => t!("ide-choose-test-subject"),
        (Some("usages"), _) => t!("ide-choose-usages-of", name = name, count = count),
        _ => t!("ide-choose-declaration"),
    }
}

impl TargetsPopup {
    pub(super) fn open(&mut self, state: &Value, at: PopupAnchor, root: Option<&Path>) {
        let targets: Vec<Target> = state["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|target| {
                let path = PathBuf::from(target["path"].as_str()?);
                let shown_path =
                    root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(&path).to_string_lossy().into_owned();
                let highlight = target["highlight"]
                    .as_array()
                    .and_then(|range| Some((range.first()?.as_u64()? as usize, range.get(1)?.as_u64()? as usize)));
                Some(Target {
                    offset: target["start"].as_u64()? as usize,
                    line: target["line"].as_u64().unwrap_or(1),
                    preview: target["preview"].as_str().unwrap_or_default().to_owned(),
                    shown_path,
                    highlight,
                    path,
                })
            })
            .collect();
        let create = state["createTest"] == true;
        self.shown = (!targets.is_empty() || create).then(|| Shown {
            title: title(state, targets.len()),
            targets,
            selected: 0,
            at,
            reveal: true,
            create,
        });
    }

    fn chosen(shown: &Shown, index: usize) -> Value {
        match index.checked_sub(usize::from(shown.create)) {
            Some(index) => {
                let target = &shown.targets[index];
                json!({ "path": target.path, "offset": target.offset })
            }
            None => json!({ "createTest": true }),
        }
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<Value> {
        let shown = self.shown.as_mut()?;
        let count = shown.targets.len() + usize::from(shown.create);
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            shown.selected = shown.selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            shown.selected = (shown.selected + 1) % count;
        }
        shown.reveal |= up || down;
        if enter {
            let chosen = Self::chosen(shown, shown.selected);
            self.shown = None;
            return Some(chosen);
        }
        if escape {
            self.shown = None;
        }
        None
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: CaretPlace) -> Option<Value> {
        let shown = self.shown.as_mut()?;
        let area = egui::Area::new(ui.scope_id().with("targets-popup")).order(egui::Order::Foreground);
        let area = shown.at.area(ui.ctx(), area, caret)?;
        let width = 560.0_f32.min(ui.ctx().content_rect().width() - 40.0).max(260.0);
        let mut chosen = None;
        let area = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(&shown.title).font(theme::semibold(12.5)).color(palette.text));
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                    ui,
                    |ui| {
                        if shown.create {
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            if shown.selected == 0 {
                                ui.painter().rect_filled(rect, 4, palette.selection);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 4, palette.popup_hover);
                            }
                            let label = t!("ide-create-new-test");
                            crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || label.clone());
                            crate::accessibility::edit(&response, |node| node.set_selected(shown.selected == 0));
                            ui.painter().text(
                                pos2(rect.left() + 28.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                label,
                                theme::regular(13.0),
                                palette.text,
                            );
                            if response.clicked() {
                                chosen = Some(0);
                            }
                        }
                        let skip = usize::from(shown.create);
                        for (index, target) in shown.targets.iter().enumerate() {
                            let index = index + skip;
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            if index == shown.selected {
                                ui.painter().rect_filled(rect, 4, palette.selection);
                                if shown.reveal {
                                    response.scroll_to_me(None);
                                }
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 4, palette.popup_hover);
                            }
                            crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || {
                                format!("{} {}: {}", target.shown_path, target.line, target.preview.trim())
                            });
                            crate::accessibility::edit(&response, |node| node.set_selected(index == shown.selected));
                            row(ui, rect, target, palette);
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                    },
                );
            });
        });
        shown.reveal = false;
        if let Some(index) = chosen {
            let chosen = Self::chosen(shown, index);
            self.shown = None;
            return Some(chosen);
        }
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if pressed_outside {
            self.shown = None;
        }
        None
    }
}

fn row(ui: &Ui, rect: Rect, target: &Target, palette: &IdePalette) {
    let painter = ui.painter();
    let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(16.0));
    crate::file_icons::file_icon_at(painter, icon, &target.path);
    let place = format!("{}:{}", target.shown_path, target.line);
    let place = painter.layout_no_wrap(place, theme::regular(12.0), palette.text_secondary);
    let place_left = rect.right() - 8.0 - place.size().x;
    let mut job = LayoutJob::default();
    let (from, to) = target.highlight.unwrap_or((0, 0));
    for (index, c) in target.preview.chars().enumerate() {
        let strong = (from..to).contains(&index);
        let font = if strong { theme::semibold(13.0) } else { theme::regular(13.0) };
        job.append(&c.to_string(), 0.0, TextFormat::simple(font, palette.text));
    }
    let text = painter.layout_job(job);
    let text_left = rect.left() + 28.0;
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2((place_left - 12.0).max(text_left), rect.bottom()));
    painter.with_clip_rect(clip).galley(pos2(text_left, rect.center().y - text.size().y / 2.0), text, palette.text);
    painter.galley(pos2(place_left, rect.center().y - place.size().y / 2.0), place, palette.text_secondary);
}

#[cfg(test)]
mod tests {
    use egui::Pos2;

    use super::*;

    fn state() -> Value {
        json!({
            "pending": false,
            "navigated": false,
            "kind": "implementation",
            "symbol": "Read",
            "targets": [
                { "path": "/p/a/r.go", "start": 40, "end": 44, "line": 3, "column": 6, "preview": "func (r R) Read() {}", "highlight": [11, 15] },
                { "path": "/p/b.go", "start": 7, "end": 11, "line": 9, "column": 1, "preview": "func Read() {}", "highlight": [5, 9] }
            ]
        })
    }

    #[test]
    fn titles_follow_the_kind_and_paths_the_project() {
        let mut popup = TargetsPopup::default();
        popup.open(&state(), PopupAnchor::At(Pos2::ZERO), Some(Path::new("/p")));
        let shown = popup.shown.clone().unwrap();
        assert_eq!(shown.title, t!("ide-choose-implementation-of", name = "Read", count = 2));
        assert_eq!(shown.targets[0].shown_path, Path::new("a").join("r.go").to_string_lossy());
        assert_eq!(shown.targets[1].highlight, Some((5, 9)));
        assert_eq!(title(&json!({ "kind": "declaration", "symbol": "x" }), 2), t!("ide-choose-declaration"));
        assert_eq!(title(&json!({ "kind": "typeDeclaration", "symbol": "x" }), 2), t!("ide-choose-type-declaration"));
        assert_eq!(
            title(&json!({ "kind": "superMethod", "symbol": "Read" }), 2),
            t!("ide-choose-super-method-of", name = "Read", count = 2)
        );
        popup.open(&json!({ "targets": [] }), PopupAnchor::At(Pos2::ZERO), None);
        assert!(popup.shown.is_none());
    }

    #[test]
    fn arrows_choose_and_enter_goes_to_the_target() {
        let ctx = egui::Context::default();
        let mut popup = TargetsPopup::default();
        popup.open(&state(), PopupAnchor::At(Pos2::ZERO), None);
        let key = |key: Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>, popup: &mut TargetsPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui);
                let _ = popup.show(ui, &IdePalette::zen_dark(), CaretPlace::default());
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![key(Key::ArrowUp)], &mut popup), None);
        assert_eq!(popup.shown.as_ref().unwrap().selected, 1);
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), Some(json!({ "path": "/p/b.go", "offset": 7 })));
        assert!(popup.shown.is_none());
        popup.open(&state(), PopupAnchor::At(Pos2::ZERO), None);
        frame(vec![key(Key::Escape)], &mut popup);
        assert!(popup.shown.is_none());
        let none = json!({ "kind": "test", "symbol": "Total", "targets": [], "createTest": true });
        popup.open(&none, PopupAnchor::At(Pos2::ZERO), None);
        assert_eq!(popup.shown.as_ref().unwrap().title, t!("ide-choose-test-of", name = "Total", count = 0));
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), Some(json!({ "createTest": true })), "Create New Test…");
    }
}
