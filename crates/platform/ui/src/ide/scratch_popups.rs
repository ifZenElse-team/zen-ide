use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Event, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::scratch::ScratchLanguage;

use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 26.0;
const ROWS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LanguageTarget {
    New,
    Change(PathBuf),
}

struct LanguageList {
    target: LanguageTarget,
    filter: String,
    selected: usize,
    languages: Vec<ScratchLanguage>,
}

struct FileRow {
    path: PathBuf,
    name: String,
    buffer: bool,
}

struct FileList {
    filter: String,
    selected: usize,
    files: Vec<FileRow>,
}

struct MoveDialog {
    path: PathBuf,
    directory: String,
    focused: bool,
}

#[derive(Default)]
pub(super) struct ScratchPopups {
    languages: Option<LanguageList>,
    files: Option<FileList>,
    clear: Option<PathBuf>,
    moving: Option<MoveDialog>,
}

type Row = (String, String, String);

fn rows_json(rows: &[Row]) -> Value {
    Value::Array(rows.iter().map(|(_, text, detail)| json!({ "name": text, "detail": detail })).collect())
}

impl LanguageList {
    fn shown(&self) -> Vec<&ScratchLanguage> {
        let filter = self.filter.to_lowercase();
        self.languages
            .iter()
            .filter(|language| {
                filter.is_empty()
                    || language.name.to_lowercase().contains(&filter)
                    || language.extension.to_lowercase().contains(&filter)
            })
            .collect()
    }

    fn rows(&self) -> Vec<Row> {
        self.shown()
            .into_iter()
            .map(|language| (format!("x.{}", language.extension), language.name.clone(), String::new()))
            .collect()
    }
}

impl FileList {
    fn shown(&self) -> Vec<&FileRow> {
        let filter = self.filter.to_lowercase();
        self.files.iter().filter(|file| filter.is_empty() || file.name.to_lowercase().contains(&filter)).collect()
    }

    fn rows(&self) -> Vec<Row> {
        self.shown()
            .into_iter()
            .map(|file| {
                let detail = if file.buffer { t!("ide-scratch-buffers") } else { t!("ide-scratch-scratches") };
                (file.name.clone(), file.name.clone(), detail)
            })
            .collect()
    }
}

impl ScratchPopups {
    pub(super) fn open_languages(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        target: LanguageTarget,
    ) -> Result<Value, String> {
        let answer =
            registry.dispatch(session, "file.scratch.languages", json!({})).map_err(|error| error.to_string())?;
        let languages: Vec<ScratchLanguage> = serde_json::from_value(answer).unwrap_or_default();
        *self = Self::default();
        let list = LanguageList { target, filter: String::new(), selected: 0, languages };
        let rows = rows_json(&list.rows());
        self.languages = Some(list);
        Ok(rows)
    }

    pub(super) fn open_files(&mut self, registry: &Registry, session: &mut Session) -> Result<Value, String> {
        let answer = registry.dispatch(session, "file.scratch.list", json!({})).map_err(|error| error.to_string())?;
        let row = |entry: &Value, buffer: bool| {
            Some(FileRow {
                path: PathBuf::from(entry["path"].as_str()?),
                name: entry["name"].as_str()?.to_owned(),
                buffer,
            })
        };
        let mut files: Vec<FileRow> =
            answer["scratches"].as_array().into_iter().flatten().filter_map(|entry| row(entry, false)).collect();
        files.extend(answer["buffers"].as_array().into_iter().flatten().filter_map(|entry| row(entry, true)));
        *self = Self::default();
        let list = FileList { filter: String::new(), selected: 0, files };
        let rows = rows_json(&list.rows());
        self.files = Some(list);
        Ok(rows)
    }

    pub(super) fn ask_clear(&mut self, path: PathBuf) {
        *self = Self { clear: Some(path), ..Self::default() };
    }

    pub(super) fn open_move(&mut self, path: PathBuf, directory: &Path) {
        let directory = directory.display().to_string();
        *self = Self { moving: Some(MoveDialog { path, directory, focused: false }), ..Self::default() };
    }

    pub(super) fn is_open(&self) -> bool {
        !self.open_names().is_empty()
    }

    pub(super) fn open_names(&self) -> Vec<&'static str> {
        [
            (self.languages.is_some(), "scratchLanguages"),
            (self.files.is_some(), "scratchFiles"),
            (self.clear.is_some(), "clearBuffer"),
            (self.moving.is_some(), "moveScratch"),
        ]
        .into_iter()
        .filter(|(open, _)| *open)
        .map(|(_, name)| name)
        .collect()
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> Vec<zen_core::DispatchError> {
        let mut notices = Vec::new();
        let mut run = |session: &mut Session, command: &str, params: Value| {
            if let Err(error) = registry.dispatch(session, command, params) {
                notices.push(error);
            }
        };
        if let Some(list) = &mut self.languages {
            let title = match &list.target {
                LanguageTarget::New => t!("ide-scratch-new-title"),
                LanguageTarget::Change(_) => t!("ide-scratch-change-title"),
            };
            let rows = list.rows();
            match list_popup(ui, palette, "scratch-languages", &title, &mut list.filter, &mut list.selected, &rows) {
                Choice::Pick(index) => {
                    let id = list.shown().get(index).map(|language| language.id.clone());
                    let target = list.target.clone();
                    self.languages = None;
                    match (id, target) {
                        (Some(id), LanguageTarget::New) => run(session, "file.scratch.new", json!({ "language": id })),
                        (Some(id), LanguageTarget::Change(path)) => {
                            run(session, "file.scratch.changeLanguage", json!({ "path": path, "language": id }))
                        }
                        (None, _) => {}
                    }
                }
                Choice::Close => self.languages = None,
                Choice::None => {}
            }
        }
        if let Some(list) = &mut self.files {
            let rows = list.rows();
            let title = t!("ide-scratch-files-title");
            match list_popup(ui, palette, "scratch-files", &title, &mut list.filter, &mut list.selected, &rows) {
                Choice::Pick(index) => {
                    let path = list.shown().get(index).map(|file| file.path.clone());
                    self.files = None;
                    if let Some(path) = path {
                        run(session, "file.document.open", json!({ "path": path }));
                    }
                }
                Choice::Close => self.files = None,
                Choice::None => {}
            }
        }
        if let Some(path) = &self.clear {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(clear) = clear_dialog(ui, palette, &name) {
                self.clear = None;
                if let Some(clear) = clear {
                    run(session, "file.scratch.newBuffer", json!({ "clear": clear }));
                }
            }
        }
        if let Some(dialog) = &mut self.moving {
            match move_dialog(ui, palette, dialog) {
                Some(true) => {
                    let params = json!({ "path": dialog.path, "directory": dialog.directory.trim() });
                    self.moving = None;
                    run(session, "file.scratch.move", params);
                }
                Some(false) => self.moving = None,
                None => {}
            }
        }
        notices
    }
}

enum Choice {
    None,
    Pick(usize),
    Close,
}

fn list_popup(
    ui: &mut Ui,
    palette: &IdePalette,
    id: &str,
    title: &str,
    filter: &mut String,
    selected: &mut usize,
    rows: &[Row],
) -> Choice {
    let mut choice = Choice::None;
    *selected = (*selected).min(rows.len().saturating_sub(1));
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            Event::Key { key, pressed, .. } => {
                if *pressed {
                    match key {
                        Key::ArrowDown => *selected = (*selected + 1).min(rows.len().saturating_sub(1)),
                        Key::ArrowUp => *selected = selected.saturating_sub(1),
                        Key::Enter if !rows.is_empty() => choice = Choice::Pick(*selected),
                        Key::Escape => choice = Choice::Close,
                        Key::Backspace => {
                            filter.pop();
                            *selected = 0;
                        }
                        _ => {}
                    }
                }
                false
            }
            Event::Text(text) => {
                filter.push_str(text);
                *selected = 0;
                false
            }
            Event::Paste(_) | Event::Copy | Event::Cut | Event::Ime(_) => false,
            _ => true,
        });
    });
    let screen = ui.ctx().content_rect();
    let width = 340.0;
    let area = egui::Area::new(ui.scope_id().with(id))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.18));
    let response = area.show(ui.ctx(), |ui| {
        dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let heading = if filter.is_empty() { title.to_owned() } else { format!("{title}: {filter}") };
            ui.label(egui::RichText::new(heading).font(theme::semibold(13.0)).color(palette.text));
            ui.add_space(8.0);
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new(t!("ide-goto-file-nothing"))
                        .font(theme::regular(13.0))
                        .color(palette.text_secondary),
                );
                return;
            }
            super::filter_popups::list(ui, id, ROW * ROWS as f32, |ui| {
                for (index, (icon, text, detail)) in rows.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let chosen = index == *selected;
                    accessibility::item(&response, egui::Role::ListBoxOption, chosen, text);
                    if chosen {
                        ui.painter().rect_filled(rect, 5, palette.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 5, palette.popup_hover);
                    }
                    let icon_rect = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(16.0, 16.0));
                    crate::file_icons::file_icon(ui.painter(), icon_rect, icon);
                    let label = ui.painter().text(
                        pos2(rect.left() + 32.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        theme::regular(13.0),
                        palette.text,
                    );
                    if !detail.is_empty() {
                        ui.painter().text(
                            pos2(label.right() + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            detail,
                            theme::regular(12.0),
                            palette.text_secondary,
                        );
                    }
                    if response.clicked() {
                        *selected = index;
                        choice = Choice::Pick(index);
                    }
                }
            });
        });
    });
    super::filter_popups::take_typing(ui);
    if ui.input(|input| input.pointer.any_pressed())
        && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
        && !response.response.rect.contains(pointer)
        && matches!(choice, Choice::None)
    {
        choice = Choice::Close;
    }
    choice
}

fn button(palette: &IdePalette, text: String, primary: bool) -> egui::Button<'static> {
    let color = if primary { palette.accent_text } else { palette.text };
    let button = egui::Button::new(egui::RichText::new(text).font(theme::regular(13.0)).color(color))
        .min_size(vec2(84.0, 30.0))
        .corner_radius(6);
    if primary {
        button.fill(palette.accent)
    } else {
        button.fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, palette.button_border))
    }
}

fn clear_dialog(ui: &mut Ui, palette: &IdePalette, name: &str) -> Option<Option<bool>> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("scratch-clear")).frame(dialog_frame(palette)).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                egui::RichText::new(t!("ide-scratch-clear-title")).font(theme::semibold(15.0)).color(palette.text),
            );
            ui.add_space(12.0);
            let question = t!("ide-scratch-clear-question", name = name);
            ui.add(
                egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(palette.text)).wrap(),
            );
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.add(button(palette, t!("ide-scratch-clear"), true)).clicked() {
                    choice = Some(Some(true));
                }
                if ui.add(button(palette, t!("ide-scratch-keep"), false)).clicked() {
                    choice = Some(Some(false));
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Some(true));
            }
        });
    choice.or_else(|| modal.should_close().then_some(None))
}

fn move_dialog(ui: &mut Ui, palette: &IdePalette, dialog: &mut MoveDialog) -> Option<bool> {
    let mut choice = None;
    let name = dialog.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let modal =
        egui::Modal::new(ui.scope_id().with("scratch-move")).frame(dialog_frame(palette)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("ide-scratch-move-title")).font(theme::semibold(15.0)).color(palette.text));
            ui.add_space(12.0);
            let what = t!("ide-scratch-move-file", name = name.as_str());
            ui.label(egui::RichText::new(what).font(theme::regular(13.5)).color(palette.text));
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(t!("ide-scratch-move-to")).font(theme::regular(12.5)).color(palette.text_secondary),
            );
            ui.add_space(6.0);
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(true);
            }
            let id = ui.scope_id().with("scratch-move-directory");
            let field = egui::Frame::new()
                .fill(palette.text_field)
                .stroke(Stroke::new(1.5, palette.accent))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.directory)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(14.0))
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if !dialog.focused {
                field.request_focus();
                dialog.focused = true;
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.add(button(palette, t!("ide-scratch-move-ok"), true)).clicked() {
                    choice = Some(true);
                }
                if ui.add(button(palette, t!("settings-cancel"), false)).clicked() {
                    choice = Some(false);
                }
            });
        });
    choice.or_else(|| modal.should_close().then_some(false))
}
