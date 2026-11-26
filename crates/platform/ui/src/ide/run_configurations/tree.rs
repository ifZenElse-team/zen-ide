use std::path::Path;

use egui::{Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::Configuration;
use zen_workbench::compound;

use super::super::glyphs::{Glyph, glyph_folder, glyph_minus, glyph_plus_menu};
use super::super::{IdePalette, doc_popups};
use super::{Control, Draft, MacroItem, ROW, Refs, dispatch_message, fields, icon_button, text_row};
use crate::{accessibility, theme};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ide) struct TemplateEntry {
    pub(in crate::ide) configuration: Configuration,
    pub(in crate::ide) original: Configuration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ide) struct MemberState {
    pub(in crate::ide) selected: Option<usize>,
    pub(in crate::ide) adding: bool,
}

pub(super) fn load_templates(registry: &Registry, session: &mut Session) -> Vec<TemplateEntry> {
    let listed = registry.dispatch(session, "run.template.list", json!({})).unwrap_or_default();
    listed["templates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|info| {
            let template = &info["template"];
            let configuration = Configuration {
                kind: template["kind"].as_str()?.to_owned(),
                common: serde_json::from_value(template["common"].clone()).unwrap_or_default(),
                options: template["options"].clone(),
                ..Configuration::default()
            };
            Some(TemplateEntry { original: configuration.clone(), configuration })
        })
        .collect()
}

pub(super) fn apply_templates(draft: &Draft, registry: &Registry, session: &mut Session) -> (bool, Option<String>) {
    let mut changed = false;
    let mut failed = None;
    for entry in draft.templates.iter().filter(|entry| entry.configuration != entry.original) {
        let template = json!({
            "kind": entry.configuration.kind,
            "common": entry.configuration.common,
            "options": entry.configuration.options,
        });
        match registry.dispatch(session, "run.template.update", json!({ "template": template })) {
            Ok(_) => changed = true,
            Err(error) => failed = Some(dispatch_message(error)),
        }
    }
    (changed, failed)
}

impl Draft {
    pub(in crate::ide) fn matches(&self, index: usize) -> bool {
        let wanted = self.search.trim().to_lowercase();
        wanted.is_empty()
            || self.entries.get(index).is_some_and(|entry| entry.configuration.name.to_lowercase().contains(&wanted))
    }

    pub(in crate::ide) fn folders(&self, kind: &str) -> Vec<String> {
        let mut folders: Vec<String> = Vec::new();
        for entry in self.entries.iter().filter(|entry| entry.configuration.kind == kind) {
            let folder = &entry.configuration.folder;
            if !folder.is_empty() && !folders.contains(folder) {
                folders.push(folder.clone());
            }
        }
        folders
    }

    pub(in crate::ide) fn new_folder(&mut self) {
        let Some(index) = self.current.filter(|index| *index < self.entries.len()) else { return };
        if self.entries[index].configuration.is_adopted() {
            return;
        }
        let kind = self.entries[index].configuration.kind.clone();
        let taken = self.folders(&kind);
        let base = t!("ide-run-configurations-new-folder-name");
        let name = std::iter::once(base.clone())
            .chain((1..).map(|number| format!("{base} ({number})")))
            .find(|name| !taken.contains(name))
            .unwrap_or_default();
        self.entries[index].configuration.folder = name.clone();
        self.folder_name = name.clone();
        self.folder = Some((kind, name));
    }

    pub(in crate::ide) fn select_folder(&mut self, kind: &str, folder: &str) {
        self.folder = Some((kind.to_owned(), folder.to_owned()));
        self.folder_name = folder.to_owned();
    }

    pub(in crate::ide) fn rename_folder(&mut self, name: &str) {
        let Some((kind, old)) = self.folder.clone() else { return };
        let name = name.trim();
        if name.is_empty() || (name != old && self.folders(&kind).iter().any(|taken| taken == name)) {
            return;
        }
        for entry in self.entries.iter_mut().filter(|entry| entry.configuration.kind == kind) {
            if entry.configuration.folder == old && !entry.configuration.is_adopted() {
                entry.configuration.folder = name.to_owned();
            }
        }
        self.folder = Some((kind, name.to_owned()));
    }

    pub(in crate::ide) fn remove_folder(&mut self) {
        let Some((kind, old)) = self.folder.take() else { return };
        for entry in self.entries.iter_mut().filter(|entry| entry.configuration.kind == kind) {
            if entry.configuration.folder == old && !entry.configuration.is_adopted() {
                entry.configuration.folder.clear();
            }
        }
    }

    pub(in crate::ide) fn move_to_folder(&mut self, index: usize, kind: &str, folder: &str) {
        if let Some(entry) = self.entries.get_mut(index)
            && entry.configuration.kind == kind
            && !entry.configuration.is_adopted()
        {
            entry.configuration.folder = folder.to_owned();
        }
    }

    pub(in crate::ide) fn member_choices(&self, index: usize) -> Vec<usize> {
        let Some(owner) = self.entries.get(index) else { return Vec::new() };
        let taken = compound::members(&owner.configuration);
        (0..self.entries.len())
            .filter(|other| *other != index)
            .filter(|other| {
                let id = &self.entries[*other].configuration.id;
                !id.is_empty() && !taken.contains(id) && !self.entries[*other].configuration.temporary
            })
            .collect()
    }

    pub(in crate::ide) fn add_member(&mut self, other: usize) {
        let (Some(index), Some(id)) =
            (self.current, self.entries.get(other).map(|entry| entry.configuration.id.clone()))
        else {
            return;
        };
        let Some(owner) = self.entries.get_mut(index) else { return };
        let mut members = compound::members(&owner.configuration);
        members.push(id);
        compound::set_members(&mut owner.configuration, &members);
        self.members = MemberState { selected: Some(members.len() - 1), adding: false };
    }

    pub(in crate::ide) fn remove_member(&mut self) {
        let (Some(index), Some(member)) = (self.current, self.members.selected) else { return };
        let Some(owner) = self.entries.get_mut(index) else { return };
        let mut members = compound::members(&owner.configuration);
        if member < members.len() {
            members.remove(member);
            compound::set_members(&mut owner.configuration, &members);
        }
        self.members.selected = (!members.is_empty()).then(|| member.min(members.len() - 1));
    }

    pub(in crate::ide) fn compound_problem(&self, index: usize) -> Option<String> {
        let entry = self.entries.get(index)?;
        if !compound::is_compound(&entry.configuration) {
            return None;
        }
        let configurations: Vec<Configuration> = self.entries.iter().map(|entry| entry.configuration.clone()).collect();
        let member = compound::cycle(&configurations, &entry.configuration)?;
        Some(t!("ide-run-configurations-error-cycle", name = entry.configuration.name.as_str(), member = member))
    }

    fn member_name(&self, id: &str) -> String {
        self.entries
            .iter()
            .find(|entry| entry.configuration.id == id)
            .map(|entry| {
                let kind = self.kind_of(&entry.configuration).map(|kind| t!(&kind.spec.name_message));
                match kind {
                    Some(kind) => format!("{} ({kind})", entry.configuration.name),
                    None => entry.configuration.name.clone(),
                }
            })
            .unwrap_or_else(|| t!("ide-run-configurations-member-missing", id = id))
    }
}

pub(super) fn search_row(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    draft: &mut Draft,
    controls: &mut Vec<(Control, Rect)>,
) {
    let mut search_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let hint = t!("ide-run-configurations-search");
    let response = search_ui.add(
        egui::TextEdit::singleline(&mut draft.search)
            .id_salt("run-configurations-search")
            .hint_text(egui::RichText::new(&hint).color(p.text_secondary))
            .desired_width(rect.width()),
    );
    accessibility::labelled(&response, &hint);
    controls.push((Control::Search, response.rect));
}

pub(super) fn templates_link(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    draft: &mut Draft,
    controls: &mut Vec<(Control, Rect)>,
) {
    let text = if draft.editing_templates {
        t!("ide-run-configurations-back-to-configurations")
    } else {
        t!("ide-run-configurations-edit-templates")
    };
    let galley = ui.painter().layout_no_wrap(text, theme::regular(13.0), p.link);
    let link = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley.size());
    let response = ui.interact(link, ui.scope_id().with("run-configurations-templates"), Sense::click());
    accessibility::name(&response, egui::Role::Link, galley.text());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().hline(link.x_range(), link.bottom(), Stroke::new(1.0, p.link));
    }
    ui.painter().galley(link.min, galley, p.link);
    controls.push((Control::Templates, link));
    if response.clicked() {
        draft.editing_templates = !draft.editing_templates;
        draft.folder = None;
        if draft.template_current.is_none() && !draft.templates.is_empty() {
            draft.template_current = Some(0);
        }
    }
}

fn row(ui: &mut Ui, sense: Sense) -> (Rect, egui::Response) {
    ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense)
}

fn header_text(ui: &Ui, p: &IdePalette, rect: Rect, indent: f32, text: &str) {
    ui.painter().with_clip_rect(rect).text(
        pos2(rect.left() + indent, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::regular(13.0),
        p.text,
    );
}

pub(super) fn templates_list(ui: &mut Ui, p: &IdePalette, draft: &mut Draft, controls: &mut Vec<(Control, Rect)>) {
    let (header, _) = row(ui, Sense::hover());
    header_text(ui, p, header, 8.0, &t!("ide-run-configurations-templates"));
    for index in 0..draft.templates.len() {
        let kind = draft.templates[index].configuration.kind.clone();
        let name = draft
            .kinds
            .iter()
            .find(|known| known.spec.id == kind)
            .map_or_else(|| kind.clone(), |known| t!(&known.spec.name_message));
        let (rect, response) = row(ui, Sense::click());
        let selected = draft.template_current == Some(index);
        if selected {
            ui.painter().rect_filled(rect, 4, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        accessibility::tree_item(&response, 2, None, selected, &name);
        header_text(ui, p, rect, 28.0, &name);
        controls.push((Control::Template(index), rect));
        if response.clicked() {
            draft.template_current = Some(index);
        }
    }
}

struct Target {
    rect: Rect,
    kind: String,
    folder: String,
}

pub(super) fn configurations_tree(ui: &mut Ui, p: &IdePalette, draft: &mut Draft, controls: &mut Vec<(Control, Rect)>) {
    let mut kinds: Vec<(String, String)> =
        draft.kinds.iter().map(|kind| (kind.spec.id.clone(), t!(&kind.spec.name_message))).collect();
    for entry in &draft.entries {
        if !kinds.iter().any(|(id, _)| *id == entry.configuration.kind) {
            kinds.push((entry.configuration.kind.clone(), entry.configuration.kind.clone()));
        }
    }
    let mut targets: Vec<Target> = Vec::new();
    let mut dropped: Option<(usize, Pos2)> = None;
    let dragging = ui.ctx().dragged_id().is_some();
    let pointer = ui.input(|input| input.pointer.interact_pos());
    for (kind, name) in kinds {
        let members: Vec<usize> = (0..draft.entries.len())
            .filter(|index| draft.entries[*index].configuration.kind == kind && draft.matches(*index))
            .collect();
        if members.is_empty() {
            continue;
        }
        let (header, _) = row(ui, Sense::hover());
        header_text(ui, p, header, 8.0, &name);
        targets.push(Target { rect: header, kind: kind.clone(), folder: String::new() });
        for folder in draft.folders(&kind) {
            let inside: Vec<usize> =
                members.iter().copied().filter(|index| draft.entries[*index].configuration.folder == folder).collect();
            if inside.is_empty() && !draft.search.trim().is_empty() {
                continue;
            }
            let (rect, response) = row(ui, Sense::click());
            let selected = draft.folder.as_ref() == Some(&(kind.clone(), folder.clone()));
            if selected {
                ui.painter().rect_filled(rect, 4, p.selection);
            } else if response.hovered() || (dragging && pointer.is_some_and(|at| rect.contains(at))) {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            accessibility::tree_item(&response, 2, Some(true), selected, &folder);
            let icon = Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), Vec2::splat(12.0));
            glyph_folder(ui.painter(), icon, p.text_secondary);
            header_text(ui, p, rect, 44.0, &folder);
            controls.push((Control::Folder(kind.clone(), folder.clone()), rect));
            targets.push(Target { rect, kind: kind.clone(), folder: folder.clone() });
            if response.clicked() {
                draft.select_folder(&kind, &folder);
            }
            for index in inside {
                configuration_row(ui, p, draft, index, 16.0, controls, &mut dropped);
            }
        }
        let loose: Vec<usize> =
            members.into_iter().filter(|index| draft.entries[*index].configuration.folder.is_empty()).collect();
        for index in loose {
            configuration_row(ui, p, draft, index, 0.0, controls, &mut dropped);
        }
    }
    if let Some((index, at)) = dropped
        && let Some(target) = targets.iter().find(|target| target.rect.contains(at))
    {
        draft.move_to_folder(index, &target.kind, &target.folder);
    }
}

fn configuration_row(
    ui: &mut Ui,
    p: &IdePalette,
    draft: &mut Draft,
    index: usize,
    indent: f32,
    controls: &mut Vec<(Control, Rect)>,
    dropped: &mut Option<(usize, Pos2)>,
) {
    let (rect, response) = row(ui, Sense::click_and_drag());
    let selected = draft.current == Some(index) && draft.folder.is_none();
    if selected {
        ui.painter().rect_filled(rect, 4, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let configuration = &draft.entries[index].configuration;
    accessibility::tree_item(&response, if indent > 0.0 { 3 } else { 2 }, None, selected, &configuration.name);
    let invalid = draft.problems(index).iter().any(|(_, blocking)| *blocking);
    let color = if invalid {
        p.problem("error")
    } else if configuration.temporary {
        p.text_secondary
    } else {
        p.text
    };
    let icon = Rect::from_center_size(pos2(rect.left() + 30.0 + indent, rect.center().y), Vec2::splat(10.0));
    super::super::glyphs::glyph_run(ui.painter(), icon, if configuration.temporary { p.text_secondary } else { p.run });
    ui.painter().with_clip_rect(rect).text(
        pos2(rect.left() + 44.0 + indent, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &configuration.name,
        theme::regular(13.0),
        color,
    );
    controls.push((Control::Row(index), rect));
    if response.drag_stopped()
        && let Some(at) = ui.input(|input| input.pointer.latest_pos())
    {
        *dropped = Some((index, at));
    }
    if response.clicked() {
        draft.current = Some(index);
        draft.folder = None;
        draft.tasks = super::TaskState::default();
        draft.members = MemberState::default();
    }
}

pub(super) fn folder_form(ui: &mut Ui, p: &IdePalette, draft: &mut Draft, controls: &mut Vec<(Control, Rect)>) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let mut name = draft.folder_name.clone();
    if text_row(ui, p, &t!("ide-run-configurations-folder-name"), &mut name, Control::FolderName, controls) {
        draft.folder_name = name.clone();
        draft.rename_folder(&name);
    }
}

#[allow(clippy::too_many_arguments, reason = "the page and what its fields need")]
pub(super) fn template_form(
    ui: &mut Ui,
    p: &IdePalette,
    draft: &mut Draft,
    root: Option<&Path>,
    refs: &Refs<'_>,
    macros: &[MacroItem],
    system: &[(String, String)],
    controls: &mut Vec<(Control, Rect)>,
) {
    let Some(index) = draft.template_current.filter(|index| *index < draft.templates.len()) else {
        ui.label(egui::RichText::new(t!("ide-run-configurations-templates-empty")).color(p.text_secondary));
        return;
    };
    let kind = draft.kind_of(&draft.templates[index].configuration).cloned();
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.label(egui::RichText::new(t!("ide-run-configurations-template-note")).color(p.text_secondary));
    let configuration = &mut draft.templates[index].configuration;
    fields(ui, p, configuration, kind.as_ref(), root, refs, macros, system, false, controls);
}

pub(super) fn compound_form(
    ui: &mut Ui,
    p: &IdePalette,
    draft: &mut Draft,
    index: usize,
    controls: &mut Vec<(Control, Rect)>,
) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let editable = !draft.entries[index].configuration.is_adopted();
    ui.add_enabled_ui(editable, |ui| {
        let configuration = &mut draft.entries[index].configuration;
        text_row(ui, p, &t!("ide-run-configurations-name"), &mut configuration.name, Control::Name, controls);
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new(t!("ide-run-configurations-members")).font(theme::semibold(13.0)).color(p.text));
    let members = compound::members(&draft.entries[index].configuration);
    let choices = draft.member_choices(index);
    let selected = draft.members.selected.filter(|member| *member < members.len());
    let tools: [(Control, Glyph, String, bool); 2] = [
        (Control::MemberAdd, glyph_plus_menu, t!("ide-run-configurations-member-add"), editable && !choices.is_empty()),
        (
            Control::MemberRemove,
            glyph_minus,
            t!("ide-run-configurations-member-remove"),
            editable && selected.is_some(),
        ),
    ];
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let mut x = bar.left();
    for (control, glyph, name, enabled) in tools {
        let rect = Rect::from_min_size(pos2(x, bar.top() + 1.0), Vec2::splat(24.0));
        let response = icon_button(ui, p, rect, glyph, &name, enabled, &control);
        controls.push((control.clone(), rect));
        if enabled && response.clicked() {
            match control {
                Control::MemberAdd => draft.members.adding = !draft.members.adding,
                _ => draft.remove_member(),
            }
        }
        x += 28.0;
    }
    let height = members.len().max(1) as f32 * ROW + 8.0;
    let (list, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(list, 6, p.frame);
    if members.is_empty() {
        ui.painter().text(
            pos2(list.left() + 10.0, list.center().y),
            egui::Align2::LEFT_CENTER,
            t!("ide-run-configurations-members-empty"),
            theme::regular(13.0),
            p.text_secondary,
        );
    }
    for (member, id) in members.iter().enumerate() {
        let rect = Rect::from_min_size(list.min + vec2(4.0, 4.0 + member as f32 * ROW), vec2(list.width() - 8.0, ROW));
        let response = ui.interact(rect, ui.scope_id().with(("run-configurations-member", member)), Sense::click());
        if selected == Some(member) {
            ui.painter().rect_filled(rect, 4, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        let name = draft.member_name(id);
        accessibility::name(&response, egui::Role::ListItem, &name);
        header_text(ui, p, rect, 8.0, &name);
        controls.push((Control::MemberRow(member), rect));
        if response.clicked() {
            draft.members.selected = Some(member);
        }
    }
    for (problem, _) in draft.problems(index) {
        ui.label(egui::RichText::new(problem).color(p.problem("error")));
    }
}

pub(super) fn member_menu(
    ui: &Ui,
    p: &IdePalette,
    draft: &mut Draft,
    anchor: Rect,
    controls: &mut Vec<(Control, Rect)>,
) {
    let Some(index) = draft.current else { return };
    let choices = draft.member_choices(index);
    let mut chosen = None;
    let area = egui::Area::new(ui.scope_id().with("run-configurations-member-add"))
        .order(egui::Order::Tooltip)
        .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
        .show(ui.ctx(), |ui| {
            doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(240.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for other in choices {
                    let name = draft.member_name(&draft.entries[other].configuration.id);
                    let (rect, response) = row(ui, Sense::click());
                    accessibility::name(&response, egui::Role::MenuItem, &name);
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    }
                    header_text(ui, p, rect, 8.0, &name);
                    controls.push((Control::MemberAddItem(other), rect));
                    if response.clicked() {
                        chosen = Some(other);
                    }
                }
            });
        });
    if let Some(other) = chosen {
        draft.add_member(other);
        return;
    }
    let outside = ui.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        draft.members.adding = false;
    }
}
