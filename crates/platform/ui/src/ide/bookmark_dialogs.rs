use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Event, Key, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::bookmarks::{MNEMONIC_ROWS, POPUP_LINE_ONLY, mnemonic_of};

use super::bookmarks_window::place_params;
use super::vcs::{primary_button, secondary_button};
use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const CELL: f32 = 30.0;
const GAP: f32 = 4.0;
const GRID_WIDTH: f32 = 10.0 * (CELL + GAP) + CELL;

#[derive(Clone, Debug, Default, PartialEq)]
struct Listed {
    taken: Vec<(char, PathBuf, Option<u32>, String)>,
    default_list: String,
    lists: Vec<String>,
    replace_without_asking: bool,
    own: Option<Value>,
}

impl Listed {
    fn read(registry: &Registry, session: &mut Session, place: Option<(&Path, Option<u32>)>) -> Self {
        let answer = registry.dispatch(session, "navigate.bookmark.list", json!({})).unwrap_or_default();
        let bookmarks = answer["bookmarks"].as_array().cloned().unwrap_or_default();
        let taken = bookmarks
            .iter()
            .filter_map(|bookmark| {
                let mnemonic = bookmark["mnemonic"].as_str().and_then(mnemonic_of)?;
                let path = PathBuf::from(bookmark["path"].as_str()?);
                let line = bookmark["line"].as_u64().and_then(|line| u32::try_from(line).ok()?.checked_sub(1));
                Some((mnemonic, path, line, bookmark["description"].as_str().unwrap_or_default().to_owned()))
            })
            .collect();
        let own = place.and_then(|(path, line)| {
            bookmarks
                .iter()
                .find(|bookmark| {
                    bookmark["path"].as_str().map(Path::new) == Some(path)
                        && bookmark["line"].as_u64() == line.map(|line| u64::from(line) + 1)
                })
                .cloned()
        });
        Self {
            taken,
            default_list: answer["defaultList"].as_str().unwrap_or_default().to_owned(),
            lists: answer["lists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|list| list.as_str().map(str::to_owned))
                .collect(),
            replace_without_asking: answer["replaceWithoutAsking"].as_bool().unwrap_or_default(),
            own,
        }
    }

    fn holder(&self, mnemonic: char) -> Option<&(char, PathBuf, Option<u32>, String)> {
        self.taken.iter().find(|(taken, ..)| *taken == mnemonic)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Chooser {
    path: PathBuf,
    line: Option<u32>,
    chosen: Option<char>,
    description: String,
    focus_field: bool,
    changing: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    Mnemonic(Chooser),
    GoTo,
    Replace { chooser: Chooser, other: (PathBuf, Option<u32>), dont_ask: bool },
    Rename { path: PathBuf, line: Option<u32>, text: String, focused: bool },
    MoveToList { path: PathBuf, line: Option<u32>, current: Option<String>, text: String, focused: bool },
    ListName { old: Option<String>, taken: Vec<String>, text: String, focused: bool, tabs: bool },
    DeleteList { name: String, count: usize },
    Show { entries: Vec<Entry>, selected: Option<usize>, reveal: bool },
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Header(String),
    Line { path: PathBuf, line: Option<u32>, mnemonic: Option<char>, description: String, breakpoint: bool },
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Done {
    pub(super) commands: Vec<(&'static str, Value)>,
}

#[derive(Default)]
pub(super) struct BookmarkDialogs {
    dialog: Option<Dialog>,
    listed: Listed,
}

impl BookmarkDialogs {
    pub(super) fn name(&self) -> Option<&'static str> {
        Some(match self.dialog.as_ref()? {
            Dialog::Mnemonic(_) => "bookmarkMnemonic",
            Dialog::GoTo => "goToMnemonic",
            Dialog::Replace { .. } => "bookmarkReplace",
            Dialog::Rename { .. } => "bookmarkRename",
            Dialog::MoveToList { .. } => "bookmarkList",
            Dialog::ListName { tabs: true, .. } => "bookmarkOpenTabs",
            Dialog::ListName { .. } => "bookmarkListName",
            Dialog::DeleteList { .. } => "bookmarkListDelete",
            Dialog::Show { .. } => "lineBookmarks",
        })
    }

    pub(super) fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub(super) fn close(&mut self) {
        self.dialog = None;
    }

    pub(super) fn open_mnemonic(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: PathBuf,
        line: Option<u32>,
    ) {
        self.listed = Listed::read(registry, session, Some((&path, line)));
        let own = self.listed.own.clone().unwrap_or_default();
        self.dialog = Some(Dialog::Mnemonic(Chooser {
            chosen: own["mnemonic"].as_str().and_then(mnemonic_of),
            description: own["description"].as_str().unwrap_or_default().to_owned(),
            changing: !own.is_null(),
            path,
            line,
            focus_field: false,
        }));
    }

    pub(super) fn open_go_to(&mut self, registry: &Registry, session: &mut Session) {
        self.listed = Listed::read(registry, session, None);
        self.dialog = Some(Dialog::GoTo);
    }

    pub(super) fn open_rename(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: PathBuf,
        line: Option<u32>,
    ) -> bool {
        self.listed = Listed::read(registry, session, Some((&path, line)));
        let Some(own) = self.listed.own.clone() else { return false };
        let text = own["description"].as_str().unwrap_or_default().to_owned();
        self.dialog = Some(Dialog::Rename { path, line, text, focused: false });
        true
    }

    pub(super) fn open_move_to_list(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: PathBuf,
        line: Option<u32>,
    ) -> bool {
        self.listed = Listed::read(registry, session, Some((&path, line)));
        let Some(own) = self.listed.own.clone() else { return false };
        let current = own["list"].as_str().map(str::to_owned);
        self.dialog = Some(Dialog::MoveToList { path, line, current, text: String::new(), focused: false });
        true
    }

    pub(super) fn open_list_name(&mut self, registry: &Registry, session: &mut Session, old: Option<String>) {
        self.listed = Listed::read(registry, session, None);
        let mut taken = self.listed.lists.clone();
        taken.push(self.listed.default_list.clone());
        taken.retain(|name| Some(name) != old.as_ref());
        let text = old.clone().unwrap_or_default();
        self.dialog = Some(Dialog::ListName { old, taken, text, focused: false, tabs: false });
    }

    pub(super) fn open_tabs_name(&mut self, registry: &Registry, session: &mut Session) {
        self.listed = Listed::read(registry, session, None);
        let mut taken = self.listed.lists.clone();
        taken.push(self.listed.default_list.clone());
        self.dialog = Some(Dialog::ListName { old: None, taken, text: String::new(), focused: false, tabs: true });
    }

    pub(super) fn open_delete_list(&mut self, name: String, count: usize) {
        self.dialog = Some(Dialog::DeleteList { name, count });
    }

    pub(super) fn open_show(&mut self, registry: &Registry, session: &mut Session) {
        let line_only = session.settings().get_or(POPUP_LINE_ONLY, true);
        let listed = super::bookmarks_window::Listed::read(registry, session);
        let mut entries = Vec::new();
        for group in &listed.groups {
            let shown: Vec<_> = group.bookmarks.iter().filter(|found| !line_only || found.line.is_some()).collect();
            if shown.is_empty() {
                continue;
            }
            entries.push(Entry::Header(group.name.clone()));
            entries.extend(shown.into_iter().map(|found| Entry::Line {
                path: found.path.clone(),
                line: found.line,
                mnemonic: found.mnemonic,
                description: found.description.clone(),
                breakpoint: false,
            }));
        }
        if !line_only && !listed.breakpoints.is_empty() {
            entries.push(Entry::Header(t!("ide-bookmarks-breakpoints")));
            entries.extend(listed.breakpoints.iter().map(|found| Entry::Line {
                path: found.path.clone(),
                line: found.line,
                mnemonic: None,
                description: found.condition.clone().unwrap_or_default(),
                breakpoint: true,
            }));
        }
        let selected = entries.iter().position(|entry| matches!(entry, Entry::Line { .. }));
        self.dialog = Some(Dialog::Show { entries, selected, reveal: true });
    }

    pub(super) fn shown_rows(&self) -> Vec<String> {
        let Some(Dialog::Show { entries, .. }) = &self.dialog else { return Vec::new() };
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Header(name) => format!("[{name}]"),
                Entry::Line { path, line, mnemonic, description, .. } => {
                    let mnemonic = mnemonic.map(|mnemonic| format!("{mnemonic} ")).unwrap_or_default();
                    format!("{mnemonic}{} {description}", place_name(path, *line)).trim_end().to_owned()
                }
            })
            .collect()
    }

    pub(super) fn show(&mut self, ui: &mut Ui, palette: &IdePalette) -> Option<Done> {
        let dialog = self.dialog.take()?;
        let (next, done) = match dialog {
            Dialog::Mnemonic(chooser) => self.chooser(ui, palette, chooser),
            Dialog::GoTo => self.go_to(ui, palette),
            Dialog::Replace { chooser, other, dont_ask } => replace(ui, palette, chooser, other, dont_ask),
            Dialog::Rename { path, line, text, focused } => rename(ui, palette, path, line, text, focused),
            Dialog::MoveToList { path, line, current, text, focused } => {
                self.move_to_list(ui, palette, (path, line), current, text, focused)
            }
            Dialog::ListName { old, taken, text, focused, tabs } => {
                list_name(ui, palette, ListName { old, taken, text, focused, tabs })
            }
            Dialog::DeleteList { name, count } => delete_list(ui, palette, name, count),
            Dialog::Show { entries, selected, reveal } => show_popup(ui, palette, entries, selected, reveal),
        };
        self.dialog = next;
        done
    }

    fn chooser(&mut self, ui: &mut Ui, p: &IdePalette, mut chooser: Chooser) -> (Option<Dialog>, Option<Done>) {
        let mut confirm = false;
        let field_id = ui.make_persistent_id("bookmark-description");
        let field_focused = ui.memory(|memory| memory.has_focus(field_id));
        if !field_focused {
            ui.input_mut(|input| {
                input.events.retain(|event| match event {
                    Event::Text(text) => match mnemonic_of(text) {
                        Some(mnemonic) => {
                            chooser.chosen = Some(mnemonic);
                            chooser.focus_field = true;
                            false
                        }
                        None => true,
                    },
                    Event::Key { key: Key::Enter, pressed: true, .. } => {
                        confirm = true;
                        false
                    }
                    _ => true,
                })
            });
        }
        let title = if chooser.changing { t!("ide-bookmark-change-mnemonic") } else { t!("ide-bookmark-add-mnemonic") };
        let own = (chooser.path.clone(), chooser.line);
        let mut cancel = false;
        let modal =
            egui::Modal::new(ui.scope_id().with("bookmark-mnemonic")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(GRID_WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(12.0);
                if let Some(clicked) = grid(ui, p, &self.listed, chooser.chosen, Some(&own), false) {
                    chooser.chosen = Some(clicked);
                    chooser.focus_field = true;
                }
                ui.add_space(12.0);
                let field = egui::Frame::new()
                    .fill(p.text_field)
                    .stroke(Stroke::new(1.0, p.button_border))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut chooser.description)
                                .id(field_id)
                                .hint_text(t!("ide-bookmark-description"))
                                .frame(egui::Frame::NONE)
                                .font(theme::regular(13.0))
                                .desired_width(f32::INFINITY),
                        )
                    })
                    .inner;
                accessibility::name_with(&field, egui::Role::TextInput, || t!("ide-bookmark-description"));
                if std::mem::take(&mut chooser.focus_field) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    confirm = true;
                }
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui
                        .add_enabled_ui(chooser.chosen.is_some(), |ui| primary_button(ui, p, &t!("settings-ok")))
                        .inner
                        .clicked()
                    {
                        confirm = true;
                    }
                    if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || modal.should_close() {
            return (None, None);
        }
        let Some(mnemonic) = chooser.chosen.filter(|_| confirm) else { return (Some(Dialog::Mnemonic(chooser)), None) };
        let other =
            self.listed.holder(mnemonic).map(|(_, path, line, _)| (path.clone(), *line)).filter(|other| *other != own);
        if let Some(other) = other.filter(|_| !self.listed.replace_without_asking) {
            return (Some(Dialog::Replace { chooser, other, dont_ask: false }), None);
        }
        (None, Some(Done { commands: vec![set_mnemonic(&chooser, mnemonic)] }))
    }

    fn go_to(&mut self, ui: &mut Ui, p: &IdePalette) -> (Option<Dialog>, Option<Done>) {
        let mut chosen = None;
        let taken: Vec<char> = self.listed.taken.iter().map(|(mnemonic, ..)| *mnemonic).collect();
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Text(text) => match mnemonic_of(text) {
                    Some(mnemonic) => {
                        if taken.contains(&mnemonic) {
                            chosen = Some(mnemonic);
                        }
                        false
                    }
                    None => true,
                },
                _ => true,
            })
        });
        let modal =
            egui::Modal::new(ui.scope_id().with("bookmark-go-to")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t!("ide-bookmark-go-to")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(12.0);
                if let Some(clicked) = grid(ui, p, &self.listed, None, None, true) {
                    chosen = Some(clicked);
                }
                if taken.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(t!("ide-bookmark-no-mnemonics"))
                            .font(theme::regular(12.5))
                            .color(p.text_secondary),
                    );
                }
            });
        match chosen {
            Some(mnemonic) => (
                None,
                Some(Done { commands: vec![("navigate.bookmark.goTo", json!({ "mnemonic": mnemonic.to_string() }))] }),
            ),
            None if modal.should_close() => (None, None),
            None => (Some(Dialog::GoTo), None),
        }
    }

    fn move_to_list(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (path, line): (PathBuf, Option<u32>),
        current: Option<String>,
        mut text: String,
        mut focused: bool,
    ) -> (Option<Dialog>, Option<Done>) {
        let mut chosen: Option<Option<String>> = None;
        let mut cancel = false;
        let mut lists: Vec<Option<String>> = vec![None];
        lists.extend(self.listed.lists.iter().cloned().map(Some));
        lists.retain(|list| *list != current);
        let modal = egui::Modal::new(ui.scope_id().with("bookmark-list")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("ide-bookmark-another-list")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            for list in &lists {
                let name = list.clone().unwrap_or_else(|| self.listed.default_list.clone());
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                accessibility::button_with(&response, || name.clone());
                if response.hovered() {
                    ui.painter().rect_filled(rect, 5, p.popup_hover);
                }
                ui.painter().text(
                    pos2(rect.left() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    theme::regular(13.0),
                    p.text,
                );
                if response.clicked() {
                    chosen = Some(list.clone());
                }
            }
            ui.add_space(10.0);
            let label = ui.label(
                egui::RichText::new(t!("ide-bookmark-new-list")).font(theme::regular(12.5)).color(p.text_secondary),
            );
            ui.add_space(6.0);
            let field = egui::Frame::new()
                .fill(p.text_field)
                .stroke(Stroke::new(1.0, p.button_border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(13.0))
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id)
                })
                .inner;
            if !focused {
                field.request_focus();
                focused = true;
            }
            let named = !text.trim().is_empty();
            if field.lost_focus() && named && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                chosen = Some(Some(text.trim().to_owned()));
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.add_enabled_ui(named, |ui| primary_button(ui, p, &t!("settings-ok"))).inner.clicked() {
                    chosen = Some(Some(text.trim().to_owned()));
                }
                if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if let Some(list) = chosen {
            let mut params = place_params(&path, line);
            params["list"] = json!(list);
            return (None, Some(Done { commands: vec![("navigate.bookmark.moveToList", params)] }));
        }
        if cancel || modal.should_close() {
            return (None, None);
        }
        (Some(Dialog::MoveToList { path, line, current, text, focused }), None)
    }
}

fn set_mnemonic(chooser: &Chooser, mnemonic: char) -> (&'static str, Value) {
    let mut params = place_params(&chooser.path, chooser.line);
    params["mnemonic"] = json!(mnemonic.to_string());
    params["description"] = json!(chooser.description);
    params["replace"] = json!(true);
    ("navigate.bookmark.setMnemonic", params)
}

fn place_name(path: &Path, line: Option<u32>) -> String {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    match line {
        Some(line) => format!("{name}:{}", line + 1),
        None => name,
    }
}

fn grid(
    ui: &mut Ui,
    p: &IdePalette,
    listed: &Listed,
    chosen: Option<char>,
    own: Option<&(PathBuf, Option<u32>)>,
    only_taken: bool,
) -> Option<char> {
    let mut clicked = None;
    for (row, keys) in MNEMONIC_ROWS.iter().enumerate() {
        let indent = row.saturating_sub(1) as f32 * CELL / 3.0;
        let (rect, _) = ui.allocate_exact_size(vec2(GRID_WIDTH, CELL + GAP), Sense::hover());
        for (index, mnemonic) in keys.chars().enumerate() {
            let cell = egui::Rect::from_min_size(
                pos2(rect.left() + indent + index as f32 * (CELL + GAP), rect.top()),
                vec2(CELL, CELL),
            );
            let holder = listed.holder(mnemonic);
            let elsewhere = holder.is_some_and(|(_, path, line, _)| own != Some(&(path.clone(), *line)));
            let enabled = !only_taken || holder.is_some();
            let response = ui.interact(
                cell,
                ui.scope_id().with(("mnemonic", mnemonic)),
                if enabled { Sense::click() } else { Sense::hover() },
            );
            accessibility::item(&response, egui::Role::Button, chosen == Some(mnemonic), &mnemonic.to_string());
            let (fill, ink) = if chosen == Some(mnemonic) {
                (p.accent, p.accent_text)
            } else if response.hovered() && enabled {
                (p.popup_hover, p.text)
            } else if elsewhere || (only_taken && holder.is_some()) {
                (p.selection, p.text)
            } else if enabled {
                (p.text_field, p.text)
            } else {
                (Color32::TRANSPARENT, p.text_secondary.gamma_multiply(0.5))
            };
            ui.painter().rect(cell, 5, fill, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            ui.painter().text(cell.center(), Align2::CENTER_CENTER, mnemonic, theme::semibold(13.0), ink);
            if let Some((_, path, line, description)) = holder {
                let place = place_name(path, *line);
                let hint = if description.is_empty() { place } else { format!("{place}  {description}") };
                response.clone().on_hover_text(hint);
            }
            if response.clicked() && enabled {
                clicked = Some(mnemonic);
            }
        }
    }
    clicked
}

fn replace(
    ui: &mut Ui,
    p: &IdePalette,
    chooser: Chooser,
    other: (PathBuf, Option<u32>),
    mut dont_ask: bool,
) -> (Option<Dialog>, Option<Done>) {
    let Some(mnemonic) = chooser.chosen else { return (None, None) };
    let (mut yes, mut no) = (false, false);
    let modal = egui::Modal::new(ui.scope_id().with("bookmark-replace")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let question =
            t!("ide-bookmark-replace-question", mnemonic = mnemonic.to_string(), place = place_name(&other.0, other.1));
        ui.add(egui::Label::new(egui::RichText::new(question).font(theme::semibold(14.0)).color(p.text)).wrap());
        ui.add_space(14.0);
        ui.checkbox(
            &mut dont_ask,
            egui::RichText::new(t!("ide-lsp-dont-ask")).font(theme::regular(13.0)).color(p.text),
        );
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if primary_button(ui, p, &t!("ide-bookmark-replace-move")).clicked() {
                yes = true;
            }
            if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                no = true;
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            yes = true;
        }
    });
    if yes {
        let mut commands = Vec::new();
        if dont_ask {
            commands.push(("navigate.bookmark.options", json!({ "replaceWithoutAsking": true })));
        }
        commands.push(set_mnemonic(&chooser, mnemonic));
        return (None, Some(Done { commands }));
    }
    if no || modal.should_close() {
        return (Some(Dialog::Mnemonic(chooser)), None);
    }
    (Some(Dialog::Replace { chooser, other, dont_ask }), None)
}

fn rename(
    ui: &mut Ui,
    p: &IdePalette,
    path: PathBuf,
    line: Option<u32>,
    mut text: String,
    mut focused: bool,
) -> (Option<Dialog>, Option<Done>) {
    let (mut ok, mut cancel) = (false, false);
    let modal = egui::Modal::new(ui.scope_id().with("bookmark-rename")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(340.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(egui::RichText::new(t!("ide-bookmark-rename")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(12.0);
        let label = ui.label(
            egui::RichText::new(t!("ide-bookmark-description")).font(theme::regular(12.5)).color(p.text_secondary),
        );
        ui.add_space(6.0);
        let field = egui::Frame::new()
            .fill(p.text_field)
            .stroke(Stroke::new(1.5, p.accent))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(13.0))
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id)
            })
            .inner;
        if !focused {
            field.request_focus();
            focused = true;
        }
        if field.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            ok = true;
        }
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if primary_button(ui, p, &t!("settings-ok")).clicked() {
                ok = true;
            }
            if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if ok {
        let mut params = place_params(&path, line);
        params["description"] = json!(text);
        return (None, Some(Done { commands: vec![("navigate.bookmark.rename", params)] }));
    }
    if cancel || modal.should_close() {
        return (None, None);
    }
    (Some(Dialog::Rename { path, line, text, focused }), None)
}

fn name_field(ui: &mut Ui, p: &IdePalette, text: &mut String, label: egui::Id, focused: &mut bool) -> egui::Response {
    let field = egui::Frame::new()
        .fill(p.text_field)
        .stroke(Stroke::new(1.5, p.accent))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(13.0))
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label)
        })
        .inner;
    if !*focused {
        field.request_focus();
        *focused = true;
    }
    field
}

struct ListName {
    old: Option<String>,
    taken: Vec<String>,
    text: String,
    focused: bool,
    tabs: bool,
}

fn list_name(ui: &mut Ui, p: &IdePalette, dialog: ListName) -> (Option<Dialog>, Option<Done>) {
    let ListName { old, taken, mut text, mut focused, tabs } = dialog;
    let (mut ok, mut cancel) = (false, ui.input(|input| input.key_pressed(Key::Escape)));
    let name = text.trim().to_owned();
    let clash = taken.contains(&name);
    let valid = !name.is_empty() && !clash;
    let title = if tabs {
        t!("ide-bookmarks-open-tabs")
    } else if old.is_some() {
        t!("ide-bookmarks-rename-list")
    } else {
        t!("command-navigate-bookmarkList-create")
    };
    let modal =
        egui::Modal::new(ui.scope_id().with("bookmark-list-name")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(&title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            let label = ui.label(
                egui::RichText::new(t!("ide-bookmarks-list-name")).font(theme::regular(12.5)).color(p.text_secondary),
            );
            ui.add_space(6.0);
            let field = name_field(ui, p, &mut text, label.id, &mut focused);
            if (field.lost_focus() || !field.has_focus())
                && valid
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                ok = true;
            }
            if clash {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(t!("ide-bookmarks-list-taken")).font(theme::regular(12.5)).color(p.error_text),
                );
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.add_enabled_ui(valid, |ui| primary_button(ui, p, &t!("settings-ok"))).inner.clicked() {
                    ok = true;
                }
                if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    if ok && valid {
        let command = match &old {
            _ if tabs => ("navigate.bookmarkList.fromOpenTabs", json!({ "name": name })),
            Some(old) => ("navigate.bookmarkList.rename", json!({ "name": old, "newName": name })),
            None => ("navigate.bookmarkList.create", json!({ "name": name })),
        };
        return (None, Some(Done { commands: vec![command] }));
    }
    if cancel || modal.should_close() {
        return (None, None);
    }
    (Some(Dialog::ListName { old, taken, text, focused, tabs }), None)
}

fn delete_list(ui: &mut Ui, p: &IdePalette, name: String, count: usize) -> (Option<Dialog>, Option<Done>) {
    let (mut yes, mut no) = (false, ui.input(|input| input.key_pressed(Key::Escape)));
    let modal =
        egui::Modal::new(ui.scope_id().with("bookmark-list-delete")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let question = t!("ide-bookmarks-delete-question", name = name.as_str(), count = count);
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::semibold(14.0)).color(p.text)).wrap());
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if primary_button(ui, p, &t!("ide-bookmarks-delete")).clicked() {
                    yes = true;
                }
                if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    no = true;
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                yes = true;
            }
        });
    if yes {
        return (None, Some(Done { commands: vec![("navigate.bookmarkList.delete", json!({ "name": name }))] }));
    }
    if no || modal.should_close() {
        return (None, None);
    }
    (Some(Dialog::DeleteList { name, count }), None)
}

fn show_popup(
    ui: &mut Ui,
    p: &IdePalette,
    entries: Vec<Entry>,
    mut selected: Option<usize>,
    mut reveal: bool,
) -> (Option<Dialog>, Option<Done>) {
    let lines: Vec<usize> =
        entries.iter().enumerate().filter(|(_, entry)| matches!(entry, Entry::Line { .. })).map(|(at, _)| at).collect();
    let mut open: Option<usize> = None;
    let mut jump: Option<char> = None;
    let mut escape = false;
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            Event::Text(text) => match mnemonic_of(text) {
                Some(mnemonic) => {
                    let used = entries
                        .iter()
                        .any(|entry| matches!(entry, Entry::Line { mnemonic: Some(found), .. } if *found == mnemonic));
                    if used {
                        jump = Some(mnemonic);
                    }
                    false
                }
                None => true,
            },
            Event::Key { key: Key::ArrowDown | Key::ArrowUp, pressed: true, .. } => {
                let down = matches!(event, Event::Key { key: Key::ArrowDown, .. });
                let at = selected.and_then(|selected| lines.iter().position(|line| *line == selected));
                let next = match at {
                    None => 0,
                    Some(at) if down => (at + 1).min(lines.len().saturating_sub(1)),
                    Some(at) => at.saturating_sub(1),
                };
                selected = lines.get(next).copied();
                reveal = true;
                false
            }
            Event::Key { key: Key::Enter, pressed: true, .. } => {
                open = selected;
                false
            }
            Event::Key { key: Key::Escape, pressed: true, .. } => {
                escape = true;
                false
            }
            _ => true,
        })
    });
    let modal = egui::Modal::new(ui.scope_id().with("bookmark-show")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(egui::RichText::new(t!("ide-bookmarks-popup-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(10.0);
        if lines.is_empty() {
            ui.label(
                egui::RichText::new(t!("ide-bookmarks-popup-empty")).font(theme::regular(13.0)).color(p.text_secondary),
            );
            return;
        }
        egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
            let height = 26.0;
            let top = ui.cursor().top();
            let clip = ui.clip_rect();
            let first = (((clip.top() - top) / height).floor().max(0.0) as usize).min(entries.len());
            let last = ((((clip.bottom() - top) / height).ceil().max(0.0) as usize) + 1).clamp(first, entries.len());
            if reveal && let Some(at) = selected.filter(|at| *at < first || *at >= last) {
                let rect = egui::Rect::from_min_size(
                    pos2(ui.max_rect().left(), top + at as f32 * height),
                    vec2(ui.available_width(), height),
                );
                ui.scroll_to_rect(rect, None);
            }
            ui.add_space(first as f32 * height);
            for (at, entry) in entries.iter().enumerate().take(last).skip(first) {
                match entry {
                    Entry::Header(name) => {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                        ui.painter().text(
                            pos2(rect.left() + 6.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            theme::semibold(12.5),
                            p.text_secondary,
                        );
                    }
                    Entry::Line { path, line, mnemonic, description, breakpoint } => {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
                        let place = place_name(path, *line);
                        let name =
                            if description.is_empty() { place.clone() } else { format!("{place} {description}") };
                        accessibility::item(&response, egui::Role::ListItem, selected == Some(at), &name);
                        if selected == Some(at) {
                            ui.painter().rect_filled(rect, 5, p.selection);
                            if std::mem::take(&mut reveal) {
                                response.scroll_to_me(None);
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 5, p.popup_hover);
                        }
                        let icon =
                            egui::Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(14.0, 14.0));
                        if *breakpoint {
                            ui.painter().circle_filled(icon.center(), 5.0, p.breakpoint);
                        } else {
                            super::editor_view::bookmark_icon(ui.painter(), icon.shrink(1.0), *mnemonic, p);
                        }
                        let painter = ui.painter().with_clip_rect(rect);
                        let text = painter.text(
                            pos2(rect.left() + 42.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &place,
                            theme::regular(13.0),
                            p.text,
                        );
                        if !description.is_empty() {
                            painter.text(
                                pos2(text.right() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                description,
                                theme::regular(13.0),
                                p.text_secondary,
                            );
                        }
                        if response.clicked() {
                            selected = Some(at);
                        }
                        if response.double_clicked() {
                            open = Some(at);
                        }
                    }
                }
            }
            ui.add_space((entries.len() - last) as f32 * height);
        });
    });
    if let Some(mnemonic) = jump {
        let command = ("navigate.bookmark.goTo", json!({ "mnemonic": mnemonic.to_string() }));
        return (None, Some(Done { commands: vec![command] }));
    }
    if let Some(Entry::Line { path, line, .. }) = open.and_then(|at| entries.get(at)) {
        let command = ("navigate.bookmark.open", json!({ "path": path, "line": line.map(|line| line + 1) }));
        return (None, Some(Done { commands: vec![command] }));
    }
    if escape || modal.should_close() {
        return (None, None);
    }
    (Some(Dialog::Show { entries, selected, reveal }), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Registry, Session, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.go");
        std::fs::write(&main, "package main\n\nfunc a() {}\n\nfunc b() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, main, dir)
    }

    fn run(dialogs: &mut BookmarkDialogs, events: Vec<Event>) -> Option<Done> {
        let ctx = egui::Context::default();
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let mut done = None;
        for events in [events, Vec::new()] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                if done.is_none() {
                    done = dialogs.show(ui, &palette);
                }
            });
            output.textures_delta.clear();
        }
        done
    }

    fn enter() -> Event {
        Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }
    }

    fn chooser(path: &Path, chosen: char, description: &str) -> Dialog {
        Dialog::Mnemonic(Chooser {
            path: path.to_owned(),
            line: Some(2),
            chosen: Some(chosen),
            description: description.to_owned(),
            focus_field: false,
            changing: false,
        })
    }

    #[test]
    fn a_typed_mnemonic_is_chosen_and_enter_sets_it() {
        let (registry, mut session, main, _dir) = setup();
        let mut dialogs = BookmarkDialogs::default();
        dialogs.open_mnemonic(&registry, &mut session, main.clone(), Some(2));
        assert_eq!(dialogs.name(), Some("bookmarkMnemonic"));
        assert!(run(&mut dialogs, vec![Event::Text("q".to_owned())]).is_none());
        let Some(Dialog::Mnemonic(chosen)) = &dialogs.dialog else { panic!("still open") };
        assert_eq!(chosen.chosen, Some('Q'));
        dialogs.dialog = Some(chooser(&main, 'Q', "here"));
        let done = run(&mut dialogs, vec![enter()]).expect("Enter sets it");
        assert_eq!(done.commands[0].0, "navigate.bookmark.setMnemonic");
        assert_eq!(done.commands[0].1["mnemonic"], "Q");
        assert_eq!(done.commands[0].1["description"], "here");
        assert!(!dialogs.is_open());
    }

    #[test]
    fn a_mnemonic_another_line_has_asks_first_unless_told_not_to() {
        let (registry, mut session, main, _dir) = setup();
        let params = json!({ "path": main, "line": 5, "mnemonic": "5" });
        registry.dispatch(&mut session, "navigate.bookmark.setMnemonic", params).unwrap();
        let mut dialogs = BookmarkDialogs::default();
        dialogs.open_mnemonic(&registry, &mut session, main.clone(), Some(2));
        dialogs.dialog = Some(chooser(&main, '5', ""));
        assert!(run(&mut dialogs, vec![enter()]).is_none());
        assert_eq!(dialogs.name(), Some("bookmarkReplace"));
        let Some(Dialog::Replace { chooser: asked, other, .. }) = dialogs.dialog.clone() else { panic!("asks") };
        assert_eq!(other, (main.clone(), Some(4)));
        dialogs.dialog = Some(Dialog::Replace { chooser: asked, other, dont_ask: true });
        let done = run(&mut dialogs, vec![enter()]).unwrap();
        let commands: Vec<&str> = done.commands.iter().map(|(command, _)| *command).collect();
        assert_eq!(commands, ["navigate.bookmark.options", "navigate.bookmark.setMnemonic"]);
        registry.dispatch(&mut session, "navigate.bookmark.options", json!({ "replaceWithoutAsking": true })).unwrap();
        dialogs.open_mnemonic(&registry, &mut session, main.clone(), Some(2));
        dialogs.dialog = Some(chooser(&main, '5', ""));
        assert_eq!(run(&mut dialogs, vec![enter()]).unwrap().commands.len(), 1, "Don't ask again moves it at once");
    }

    #[test]
    fn the_chooser_marks_the_taken_mnemonics_and_the_question_offers_dont_ask_again() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let (registry, mut session, main, _dir) = setup();
        let taken = json!({ "path": main, "line": 5, "mnemonic": "1", "description": "func b" });
        registry.dispatch(&mut session, "navigate.bookmark.setMnemonic", taken).unwrap();
        let first = json!({ "path": main, "line": 1, "mnemonic": "Q" });
        registry.dispatch(&mut session, "navigate.bookmark.setMnemonic", first).unwrap();
        let palette = IdePalette::zen_dark();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 360.0), 2.0);
        palette.apply(screen.context());
        let mut dialogs = BookmarkDialogs::default();
        let shown = |screen: &mut crate::offscreen::Offscreen, dialogs: &mut BookmarkDialogs, name: &str| {
            let image = screen.render(|ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                let _ = dialogs.show(ui, &palette);
            });
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(name), &image).unwrap();
            }
            image
        };
        dialogs.open_mnemonic(&registry, &mut session, main.clone(), Some(2));
        let Some(Dialog::Mnemonic(opened)) = &mut dialogs.dialog else { panic!("the chooser") };
        opened.chosen = Some('K');
        opened.description = "entry".to_owned();
        let image = shown(&mut screen, &mut dialogs, "bookmark-mnemonic-chooser.png");
        let tree = screen.accessibility_tree(|ui| {
            let _ = dialogs.show(ui, &palette);
        });
        let fill = |mnemonic: &str| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(mnemonic))
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("{mnemonic} in the grid"));
            let (x, y) = (((bounds.x0 + 4.0) * 2.0) as usize, ((bounds.y0 + 4.0) * 2.0) as usize);
            image.pixels[y * image.size[0] + x]
        };
        assert_ne!(fill("1"), fill("2"), "a mnemonic another line has is marked");
        assert_eq!(fill("Q"), fill("1"), "every taken one alike");
        assert_ne!(fill("K"), fill("1"), "the chosen one stands out");
        let asked = Chooser {
            path: main.clone(),
            line: Some(2),
            chosen: Some('1'),
            description: String::new(),
            focus_field: false,
            changing: false,
        };
        dialogs.dialog = Some(Dialog::Replace { chooser: asked, other: (main.clone(), Some(4)), dont_ask: false });
        shown(&mut screen, &mut dialogs, "bookmark-mnemonic-replace.png");
        assert_eq!(dialogs.name(), Some("bookmarkReplace"));
        dialogs.open_go_to(&registry, &mut session);
        shown(&mut screen, &mut dialogs, "bookmark-go-to-mnemonic.png");
        assert_eq!(dialogs.name(), Some("goToMnemonic"));
    }

    #[test]
    fn go_to_mnemonic_jumps_by_a_typed_mnemonic_in_use() {
        let (registry, mut session, main, _dir) = setup();
        let params = json!({ "path": main, "line": 5, "mnemonic": "3" });
        registry.dispatch(&mut session, "navigate.bookmark.toggleMnemonic", params).unwrap();
        let mut dialogs = BookmarkDialogs::default();
        dialogs.open_go_to(&registry, &mut session);
        assert!(run(&mut dialogs, vec![Event::Text("4".to_owned())]).is_none(), "a free mnemonic goes nowhere");
        let done = run(&mut dialogs, vec![Event::Text("3".to_owned())]).unwrap();
        assert_eq!(done.commands, [("navigate.bookmark.goTo", json!({ "mnemonic": "3" }))]);
        assert!(!dialogs.open_rename(&registry, &mut session, main.clone(), Some(0)), "no bookmark to rename");
        assert!(dialogs.open_rename(&registry, &mut session, main.clone(), Some(4)));
        assert_eq!(dialogs.name(), Some("bookmarkRename"));
        assert!(dialogs.open_move_to_list(&registry, &mut session, main, Some(4)));
        assert_eq!(dialogs.name(), Some("bookmarkList"));
    }
    #[test]
    fn the_list_dialogs_create_rename_and_delete_through_their_commands() {
        let (registry, mut session, _main, dir) = setup();
        let project = dir.path().file_name().unwrap().to_string_lossy().into_owned();
        let mut dialogs = BookmarkDialogs::default();
        dialogs.open_list_name(&registry, &mut session, None);
        assert_eq!(dialogs.name(), Some("bookmarkListName"));
        let Some(Dialog::ListName { taken, .. }) = &dialogs.dialog else { panic!("asks for a name") };
        assert_eq!(taken, std::slice::from_ref(&project));
        let name = |dialogs: &mut BookmarkDialogs, typed: &str| {
            if let Some(Dialog::ListName { text, .. }) = &mut dialogs.dialog {
                *text = typed.to_owned();
            }
        };
        name(&mut dialogs, &project);
        assert!(run(&mut dialogs, vec![enter()]).is_none(), "a name another list has is refused");
        assert_eq!(dialogs.name(), Some("bookmarkListName"));
        name(&mut dialogs, "Review");
        let done = run(&mut dialogs, vec![enter()]).expect("Enter creates it");
        assert_eq!(done.commands, [("navigate.bookmarkList.create", json!({ "name": "Review" }))]);
        dialogs.open_list_name(&registry, &mut session, Some("Review".to_owned()));
        let Some(Dialog::ListName { text, .. }) = &dialogs.dialog else { panic!("the old name") };
        assert_eq!(text, "Review");
        dialogs.dialog = Some(Dialog::ListName {
            old: Some("Review".to_owned()),
            taken: vec![project],
            text: "Later".to_owned(),
            focused: true,
            tabs: false,
        });
        let done = run(&mut dialogs, vec![enter()]).unwrap();
        assert_eq!(done.commands, [("navigate.bookmarkList.rename", json!({ "name": "Review", "newName": "Later" }))]);
        dialogs.open_delete_list("Later".to_owned(), 2);
        assert_eq!(dialogs.name(), Some("bookmarkListDelete"));
        let escape = Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        assert!(run(&mut dialogs, vec![escape]).is_none());
        assert!(!dialogs.is_open(), "⎋ keeps the list");
        dialogs.open_delete_list("Later".to_owned(), 2);
        let done = run(&mut dialogs, vec![enter()]).unwrap();
        assert_eq!(done.commands, [("navigate.bookmarkList.delete", json!({ "name": "Later" }))]);
    }

    #[test]
    fn show_line_bookmarks_lists_them_by_list_jumps_by_mnemonic_and_opens_with_enter() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let (registry, mut session, main, _dir) = setup();
        let mut dispatch = |command: &str, params: Value| registry.dispatch(&mut session, command, params).unwrap();
        dispatch("navigate.bookmark.toggle", json!({ "path": main, "line": 1 }));
        dispatch(
            "navigate.bookmark.setMnemonic",
            json!({ "path": main, "line": 5, "mnemonic": "7", "description": "b" }),
        );
        dispatch("debug.breakpoint.toggle", json!({ "path": main, "line": 2 }));
        let mut dialogs = BookmarkDialogs::default();
        dialogs.open_show(&registry, &mut session);
        assert_eq!(dialogs.name(), Some("lineBookmarks"));
        let rows = dialogs.shown_rows();
        assert_eq!(rows[1..], ["main.go:1", "7 main.go:5 b"], "line bookmarks only, by default");
        assert!(run(&mut dialogs, vec![Event::Text("x".to_owned())]).is_none(), "no bookmark has X");
        let done = run(&mut dialogs, vec![Event::Text("7".to_owned())]).unwrap();
        assert_eq!(done.commands, [("navigate.bookmark.goTo", json!({ "mnemonic": "7" }))]);
        dialogs.open_show(&registry, &mut session);
        let down = Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        assert!(run(&mut dialogs, vec![down]).is_none());
        let done = run(&mut dialogs, vec![enter()]).unwrap();
        assert_eq!(done.commands, [("navigate.bookmark.open", json!({ "path": main, "line": 5 }))]);
        session.settings_mut().set(POPUP_LINE_ONLY, false).unwrap();
        dialogs.open_show(&registry, &mut session);
        assert_eq!(
            dialogs.shown_rows()[3..],
            [format!("[{}]", t!("ide-bookmarks-breakpoints")), "main.go:3".to_owned()]
        );
    }

    #[test]
    fn the_new_bookmark_dialogs_in_english_and_russian() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) else { return };
        let (registry, mut session, main, _dir) = setup();
        registry.dispatch(&mut session, "navigate.bookmark.toggle", json!({ "path": main, "line": 3 })).unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 360.0), 2.0);
        palette.apply(screen.context());
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            let mut dialogs = BookmarkDialogs::default();
            for name in ["list-name", "list-taken", "list-delete"] {
                match name {
                    "list-name" => dialogs.open_list_name(&registry, &mut session, Some("Review".to_owned())),
                    "list-taken" => {
                        let project = _dir.path().file_name().unwrap().to_string_lossy().into_owned();
                        dialogs.dialog = Some(Dialog::ListName {
                            old: None,
                            taken: vec![project.clone()],
                            text: project,
                            focused: true,
                            tabs: false,
                        });
                    }
                    _ => dialogs.open_delete_list("Review".to_owned(), 3),
                }
                let image = screen.render(|ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                    let _ = dialogs.show(ui, &palette);
                });
                crate::offscreen::write_png(&dir.join(format!("bookmark-{name}-{language}.png")), &image).unwrap();
            }
        }
        zen_i18n::set_language(Some("en"));
    }
}
