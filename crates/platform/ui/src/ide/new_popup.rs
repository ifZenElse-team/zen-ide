use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::file_templates::FileTemplates;
use zen_workbench::file_templates::commands::{
    NewItem as ListedItem, NewItemKind, is_source_folder, marked_source_folders, menu_items, new_items,
};

use super::{IdePalette, IdeView, TreeAction};
use crate::theme;

pub(super) const PLUGGED: &[(&str, &str, Option<&str>)] =
    &[("file", "view.scratch.new", None), ("end", "view.editorConfig.new", Some("folder"))];

const ROW: f32 = 24.0;

const CACHED: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NewChoice {
    File,
    Directory,
    Package,
    Template { name: String, asked: Vec<String>, group: Option<String> },
    Action(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NewItem {
    pub(super) choice: NewChoice,
    pub(super) title: String,
    extension: Option<String>,
}

impl NewItem {
    pub(super) fn key(&self) -> String {
        match &self.choice {
            NewChoice::File => "new:file".to_owned(),
            NewChoice::Directory => "new:directory".to_owned(),
            NewChoice::Package => "new:package".to_owned(),
            NewChoice::Template { name, .. } => format!("new:template:{name}"),
            NewChoice::Action(action) => format!("new:action:{action}"),
        }
    }
}

fn with_plugged(listed: Vec<ListedItem>) -> Vec<NewItem> {
    let keymap = crate::keymap::current();
    let bindings = keymap.bindings();
    let plugged = |after: &str| -> Vec<NewItem> {
        PLUGGED
            .iter()
            .filter(|(place, action, _)| *place == after && bindings.iter().any(|binding| binding.action == *action))
            .map(|(_, action, _)| NewItem {
                choice: NewChoice::Action(action),
                title: super::keymap_page::action_title(action),
                extension: None,
            })
            .collect()
    };
    let mut items = Vec::new();
    let mut folders = false;
    for item in listed {
        let choice = match item.kind {
            NewItemKind::File => NewChoice::File,
            NewItemKind::Directory => NewChoice::Directory,
            NewItemKind::Package => NewChoice::Package,
            NewItemKind::Template => NewChoice::Template {
                name: item.template.clone().unwrap_or_default(),
                asked: item.asked.clone(),
                group: item.group.clone(),
            },
        };
        if choice == NewChoice::Directory {
            folders = true;
            items.extend(plugged("file"));
        }
        items.push(NewItem { choice, title: item.title, extension: item.extension });
    }
    if folders {
        items.extend(plugged("end"));
    }
    items
}

pub(super) fn items(session: &Session, folder: &Path) -> Vec<NewItem> {
    let Some(project) = session.state::<Workbench>().and_then(Workbench::project) else { return Vec::new() };
    let templates = session.state::<FileTemplates>().cloned().unwrap_or_default();
    let source = is_source_folder(project, folder, &marked_source_folders(session));
    with_plugged(new_items(&templates, project, folder, source))
}

pub(super) fn menu_new_items(session: &Session) -> Vec<NewItem> {
    let Some(project) = session.state::<Workbench>().and_then(Workbench::project) else { return Vec::new() };
    let templates = session.state::<FileTemplates>().cloned().unwrap_or_default();
    with_plugged(menu_items(&templates, project))
}

#[derive(Default)]
pub(super) struct NewItemsCache {
    kept: Vec<(PathBuf, u64, u64, String, Instant, Vec<NewItem>)>,
    menu: Option<(u64, u64, String, Instant, Vec<NewItem>)>,
}

const KEPT_FOLDERS: usize = 4;

#[derive(Debug)]
pub(super) struct NewPopup {
    folder: PathBuf,
    items: Vec<NewItem>,
    search: String,
    focused: bool,
    selected: usize,
    at: Pos2,
    reveal: bool,
}

impl NewPopup {
    fn shown(&self) -> Vec<&NewItem> {
        let query = self.search.trim().to_lowercase();
        self.items.iter().filter(|item| item.title.to_lowercase().contains(&query)).collect()
    }
}

impl IdeView {
    pub(super) fn cached_new_items(&mut self, session: &Session, folder: &Path) -> Vec<NewItem> {
        let generation = session.state::<FileTemplates>().map_or(0, FileTemplates::generation);
        let revision = session.state::<Workbench>().map_or(0, Workbench::revision);
        let language = zen_i18n::language();
        let kept = &mut self.new_items.kept;
        if let Some((.., items)) = kept.iter().find(|(kept, kept_generation, kept_revision, kept_language, at, _)| {
            kept == folder
                && *kept_generation == generation
                && *kept_revision == revision
                && *kept_language == language
                && at.elapsed() < CACHED
        }) {
            return items.clone();
        }
        let listed = items(session, folder);
        kept.retain(|(kept, ..)| kept != folder);
        if kept.len() >= KEPT_FOLDERS {
            kept.remove(0);
        }
        kept.push((folder.to_owned(), generation, revision, language, Instant::now(), listed.clone()));
        listed
    }

    pub(super) fn cached_menu_items(&mut self, session: &Session) -> Vec<NewItem> {
        let generation = session.state::<FileTemplates>().map_or(0, FileTemplates::generation);
        let revision = session.state::<Workbench>().map_or(0, Workbench::revision);
        let language = zen_i18n::language();
        if let Some((.., items)) =
            self.new_items.menu.as_ref().filter(|(kept_generation, kept_revision, kept, at, _)| {
                *kept_generation == generation
                    && *kept_revision == revision
                    && *kept == language
                    && at.elapsed() < CACHED
            })
        {
            return items.clone();
        }
        let listed = menu_new_items(session);
        self.new_items.menu = Some((generation, revision, language, Instant::now(), listed.clone()));
        listed
    }

    pub(super) fn new_folder(&self, session: &Session) -> Option<PathBuf> {
        let workbench = session.state::<Workbench>()?;
        let project = workbench.project()?.to_owned();
        let from_tree = self.selection.lead().filter(|lead| lead.starts_with(&project)).map(|lead| {
            if lead.is_dir() { lead.clone() } else { lead.parent().map_or_else(|| project.clone(), Path::to_owned) }
        });
        let from_editor = workbench.active_document().and_then(|document| document.path().parent()).map(Path::to_owned);
        let chosen = if self.tree_focused || from_editor.is_none() { from_tree.or(from_editor) } else { from_editor };
        Some(
            chosen
                .filter(|folder| zen_workbench::inside_project(&project, folder) && folder.is_dir())
                .unwrap_or(project),
        )
    }

    pub(super) fn open_new_popup(
        &mut self,
        session: &Session,
        folder: Option<PathBuf>,
        at: Option<Pos2>,
    ) -> Result<(), String> {
        let workbench = session.state::<Workbench>().ok_or_else(|| t!("ide-new-no-project"))?;
        let project = workbench.project().ok_or_else(|| t!("ide-new-no-project"))?.to_owned();
        let folder = match folder {
            Some(folder) => workbench.resolve(&folder),
            None => self.new_folder(session).unwrap_or_else(|| project.clone()),
        };
        if !zen_workbench::inside_project(&project, &folder) || !folder.is_dir() {
            return Err(t!("ide-new-not-a-folder", path = folder.display().to_string()));
        }
        let items = self.cached_new_items(session, &folder);
        let at = at.or_else(|| self.project_header.map(|header| header.left_bottom() + vec2(24.0, 32.0)));
        self.new_popup = Some(NewPopup {
            folder,
            items,
            search: String::new(),
            focused: false,
            selected: 0,
            at: at.unwrap_or(pos2(240.0, 120.0)),
            reveal: true,
        });
        Ok(())
    }

    pub(super) fn new_popup_keys(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(popup) = self.new_popup.as_mut() else { return };
        let count = popup.shown().len().max(1);
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            popup.selected = popup.selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            popup.selected = (popup.selected + 1) % count;
        }
        popup.reveal |= up || down;
        if escape {
            self.new_popup = None;
        } else if enter {
            let chosen = popup.shown().get(popup.selected).copied().cloned();
            let folder = popup.folder.clone();
            self.new_popup = None;
            if let Some(item) = chosen {
                self.choose_new(ui, registry, session, folder, item.choice);
            }
        }
    }

    pub(super) fn new_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(popup) = self.new_popup.as_mut() else { return };
        let mut chosen = None;
        let screen = ui.ctx().content_rect();
        let at = pos2(popup.at.x.min(screen.right() - 260.0).max(screen.left()), popup.at.y.max(screen.top()));
        let area = egui::Area::new(ui.scope_id().with("new-popup")).order(egui::Order::Foreground).fixed_pos(at).show(
            ui.ctx(),
            |ui| {
                p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_width(240.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let before = popup.search.clone();
                    let hint = t!("ide-new-search");
                    let edit = egui::TextEdit::singleline(&mut popup.search)
                        .id_salt("new-popup-search")
                        .hint_text(egui::RichText::new(hint.clone()).color(p.text_secondary))
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(13.0))
                        .desired_width(f32::INFINITY);
                    let search = egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| ui.add(edit))
                        .inner;
                    crate::accessibility::edit(&search, |node| node.set_label(hint));
                    if !popup.focused {
                        popup.focused = true;
                        search.request_focus();
                    }
                    if popup.search != before {
                        popup.selected = 0;
                        popup.reveal = true;
                    }
                    let y = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, p.hover));
                    ui.add_space(4.0);
                    let shown: Vec<NewItem> = popup.shown().into_iter().cloned().collect();
                    egui::ScrollArea::vertical().max_height(ROW * 16.0).auto_shrink([false, true]).show(ui, |ui| {
                        for (index, item) in shown.iter().enumerate() {
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            if index == popup.selected {
                                ui.painter().rect_filled(rect, 4, p.selection);
                                if popup.reveal {
                                    response.scroll_to_me(None);
                                }
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 4, p.popup_hover);
                            }
                            crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || {
                                item.title.clone()
                            });
                            crate::accessibility::edit(&response, |node| node.set_selected(index == popup.selected));
                            row(ui, rect, item, &p);
                            if response.clicked() {
                                chosen = Some(item.clone());
                            }
                        }
                    });
                });
            },
        );
        popup.reveal = false;
        if let Some(item) = chosen {
            let folder = popup.folder.clone();
            self.new_popup = None;
            self.choose_new(ui, registry, session, folder, item.choice);
            return;
        }
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if pressed_outside {
            self.new_popup = None;
        }
    }

    pub(super) fn choose_new(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        folder: PathBuf,
        choice: NewChoice,
    ) {
        match choice {
            NewChoice::File => self.pending_tree_action = Some(TreeAction::NewFile(folder)),
            NewChoice::Directory => self.pending_tree_action = Some(TreeAction::NewDirectory(folder)),
            NewChoice::Package => self.pending_tree_action = Some(TreeAction::NewPackage(folder)),
            NewChoice::Template { group: Some(group), .. } => {
                if let Err(error) = self.open_new_in_group(registry, session, &group, folder) {
                    self.post(ui, error);
                }
            }
            NewChoice::Template { name, asked, .. } if asked.is_empty() => {
                let params = json!({ "template": name, "directory": folder });
                self.dispatch(ui, registry, session, "file.templates.create", params);
            }
            NewChoice::Template { name, .. } => {
                if let Err(error) = self.open_new_from_template(registry, session, Some(&name), Some(folder), true) {
                    self.post(ui, error);
                }
            }
            NewChoice::Action(action) => {
                let keymap = crate::keymap::current();
                let binding = keymap.bindings().into_iter().find(|binding| binding.action == action);
                if let Some(binding) = binding {
                    let mut params = binding.params.clone();
                    let key = PLUGGED.iter().find(|(_, plugged, _)| *plugged == action).and_then(|(.., key)| *key);
                    if let (Some(object), Some(key)) = (params.as_object_mut(), key) {
                        object.insert(key.to_owned(), json!(folder));
                    }
                    self.run_action(ui, registry, session, &binding.command, params);
                }
            }
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn choose_new_by_key(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, key: &str) {
        let Some(folder) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            return;
        };
        let item = self.cached_menu_items(session).into_iter().find(|item| item.key() == key);
        if let Some(item) = item {
            self.choose_new(ui, registry, session, folder, item.choice);
        }
    }

    pub(super) fn new_popup_open(&self) -> bool {
        self.new_popup.is_some()
    }

    #[cfg(test)]
    pub(super) fn new_popup_titles(&self) -> Vec<String> {
        self.new_popup
            .as_ref()
            .map(|popup| popup.shown().iter().map(|item| item.title.clone()).collect())
            .unwrap_or_default()
    }
}

fn row(ui: &Ui, rect: Rect, item: &NewItem, p: &IdePalette) {
    let painter = ui.painter();
    let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(16.0));
    match &item.choice {
        NewChoice::Directory | NewChoice::Package => crate::file_icons::folder_icon(painter, icon, "", false, false),
        NewChoice::File => crate::file_icons::file_icon(painter, icon, "file"),
        NewChoice::Template { .. } | NewChoice::Action(_) => {
            let extension = item.extension.as_deref().unwrap_or_default();
            crate::file_icons::file_icon(painter, icon, &format!("file.{extension}"));
        }
    }
    let text = painter.layout_no_wrap(item.title.clone(), theme::regular(13.0), p.text);
    let left = rect.left() + 30.0;
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
    painter.with_clip_rect(clip).galley(pos2(left, rect.center().y - text.size().y / 2.0), text, p.text);
}
