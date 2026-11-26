use std::path::{Path, PathBuf};

use egui::{Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use crate::theme;

const ROW: f32 = 24.0;
const VISIBLE: usize = 12;

#[derive(Clone, Debug, PartialEq)]
struct Connection {
    id: String,
    name: String,
}

#[derive(Clone, Debug)]
struct Shown {
    path: PathBuf,
    at: PopupAnchor,
    run: Option<Value>,
    connections: Vec<Connection>,
    current: Option<String>,
    project: bool,
    selected: usize,
    reveal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Chosen {
    pub(super) path: PathBuf,
    pub(super) connection: String,
    pub(super) project: bool,
    pub(super) run: Option<Value>,
}

#[derive(Default)]
pub(super) struct SqlConnectionPopup {
    shown: Option<Shown>,
}

fn connections(registry: &Registry, session: &mut Session) -> Vec<Connection> {
    let listed = registry.dispatch(session, "db.dataSource.list", json!({ "limit": 200 })).unwrap_or_default();
    listed["connections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|connection| {
            Some(Connection {
                id: connection["id"].as_str()?.to_owned(),
                name: connection["name"].as_str()?.to_owned(),
            })
        })
        .collect()
}

impl SqlConnectionPopup {
    pub(super) fn open(&mut self, registry: &Registry, session: &mut Session, path: &Path, run: Option<Value>) {
        let listed = registry.dispatch(session, "db.injection.list", json!({ "path": path, "limit": 0 }));
        let current = listed.ok().and_then(|listed| listed["connection"].as_str().map(str::to_owned));
        let connections = connections(registry, session);
        let selected = current
            .as_ref()
            .and_then(|current| connections.iter().position(|connection| connection.id == *current))
            .unwrap_or_default();
        self.shown = Some(Shown {
            path: path.to_owned(),
            at: PopupAnchor::UNDER_CARET,
            run,
            connections,
            current,
            project: false,
            selected,
            reveal: true,
        });
    }

    #[cfg(test)]
    pub(super) fn highlighted(&self) -> Option<&str> {
        let shown = self.shown.as_ref()?;
        shown.connections.get(shown.selected).map(|connection| connection.id.as_str())
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<Chosen> {
        let shown = self.shown.as_mut()?;
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
                input.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let count = shown.connections.len();
        if count > 0 {
            shown.selected = (shown.selected + down % count + count - up % count) % count;
        }
        shown.reveal |= up > 0 || down > 0;
        if escape {
            self.shown = None;
            return None;
        }
        if enter {
            let index = shown.selected;
            return self.choose(index);
        }
        None
    }

    fn choose(&mut self, index: usize) -> Option<Chosen> {
        let shown = self.shown.take()?;
        let connection = shown.connections.get(index)?;
        Some(Chosen { path: shown.path, connection: connection.id.clone(), project: shown.project, run: shown.run })
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: CaretPlace) -> Option<Chosen> {
        let shown = self.shown.as_mut()?;
        let area = egui::Area::new(ui.scope_id().with("sql-connection-popup")).order(egui::Order::Foreground);
        let area = shown.at.area(ui.ctx(), area, caret)?;
        let mut chosen = None;
        let mut toggled = false;
        let header = if shown.run.is_some() {
            t!("ide-sql-code-choose-to-run")
        } else {
            let name = shown.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            t!("ide-sql-code-connection-for", file = name)
        };
        let area = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(280.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                let text = ui.painter().layout_no_wrap(header.clone(), theme::regular(12.0), palette.text_secondary);
                ui.painter().galley(
                    pos2(rect.left() + 8.0, rect.center().y - text.size().y / 2.0),
                    text,
                    palette.text_secondary,
                );
                if shown.connections.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    let none = t!("ide-database-no-connections");
                    let text = ui.painter().layout_no_wrap(none, theme::regular(13.0), palette.text_secondary);
                    ui.painter().galley(
                        pos2(rect.left() + 8.0, rect.center().y - text.size().y / 2.0),
                        text,
                        palette.text_secondary,
                    );
                }
                egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                    ui,
                    |ui| {
                        for (index, connection) in shown.connections.iter().enumerate() {
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
                            crate::accessibility::item(
                                &response,
                                egui::Role::ListBoxOption,
                                index == shown.selected,
                                &connection.name,
                            );
                            let current = shown.current.as_deref() == Some(connection.id.as_str());
                            row(ui, rect, &connection.name, current, palette);
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                    },
                );
                ui.add_space(2.0);
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, palette.frame));
                ui.add_space(2.0);
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(rect, 4, palette.popup_hover);
                }
                let label = t!("ide-sql-code-for-project");
                crate::accessibility::item(&response, egui::Role::CheckBox, shown.project, &label);
                row(ui, rect, &label, shown.project, palette);
                let box_rect = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(13.0));
                ui.painter().rect_stroke(
                    box_rect,
                    3,
                    egui::Stroke::new(1.0, palette.text_secondary),
                    egui::StrokeKind::Inside,
                );
                toggled = response.clicked();
            });
        });
        shown.reveal = false;
        if toggled {
            shown.project = !shown.project;
        }
        if let Some(index) = chosen {
            return self.choose(index);
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

fn row(ui: &Ui, rect: Rect, label: &str, checked: bool, palette: &IdePalette) {
    let painter = ui.painter();
    if checked {
        let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(12.0));
        super::glyphs::glyph_check(painter, icon, palette.text);
    }
    let text = painter.layout_no_wrap(label.to_owned(), theme::regular(13.0), palette.text);
    let left = rect.left() + 28.0;
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
    painter.with_clip_rect(clip).galley(pos2(left, rect.center().y - text.size().y / 2.0), text, palette.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup(project: bool) -> SqlConnectionPopup {
        SqlConnectionPopup {
            shown: Some(Shown {
                path: PathBuf::from("/p/load.go"),
                at: PopupAnchor::At(egui::Pos2::ZERO),
                run: Some(json!({ "path": "/p/load.go" })),
                connections: vec![
                    Connection { id: "a".into(), name: "Alpha".into() },
                    Connection { id: "b".into(), name: "Beta".into() },
                ],
                current: Some("a".into()),
                project,
                selected: 0,
                reveal: true,
            }),
        }
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn arrows_and_enter_choose_a_connection_and_keep_the_run() {
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>, popup: &mut SqlConnectionPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui);
                let _ = popup.show(ui, &IdePalette::zen_dark(), CaretPlace::default());
            });
            output.textures_delta.clear();
            out
        };
        let mut shown = popup(true);
        assert_eq!(frame(vec![key(Key::ArrowDown)], &mut shown), None);
        let chosen = frame(vec![key(Key::Enter)], &mut shown).unwrap();
        assert_eq!((chosen.connection.as_str(), chosen.project), ("b", true));
        assert_eq!(chosen.run, Some(json!({ "path": "/p/load.go" })));
        assert!(!shown.is_open());
        let mut shown = popup(false);
        assert_eq!(frame(vec![key(Key::Escape)], &mut shown), None);
        assert!(!shown.is_open());
    }
}
