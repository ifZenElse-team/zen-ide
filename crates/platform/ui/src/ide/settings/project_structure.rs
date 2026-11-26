mod artifacts;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::artifacts::{Artifact, ArtifactsSection};
use zen_workbench::model::{FolderKind as BuildKind, Module};
use zen_workbench::project_settings::StructureAnswer;
use zen_workbench::project_structure::{
    LibrariesSection, LibraryEntry, ModulesSection, Origin, ProblemsSection, ProjectSection, SdksSection, Section,
    StructureProblem,
};

use super::{Dispatch, widgets};
use crate::file_icons::{self, FolderKind};
use crate::ide::IdePalette;
use crate::ide::glyphs::{glyph_close, glyph_plus};
use crate::{accessibility, icons, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Excluded,
    ResourceRoot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ide) enum Part {
    #[default]
    Project,
    Modules,
    Libraries,
    Artifacts,
    Sdks,
    Problems,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ide) enum ModuleTab {
    #[default]
    Sources,
    Paths,
    Dependencies,
}

#[derive(Default)]
pub(in crate::ide) struct ProjectStructurePage {
    part: Part,
    tab: ModuleTab,
    project: Option<ProjectSection>,
    modules: Vec<Module>,
    module: usize,
    libraries: Vec<LibraryEntry>,
    sdks: Option<SdksSection>,
    problems: Vec<StructureProblem>,
    artifacts: Vec<(Option<Artifact>, Artifact)>,
    suggestions: Vec<Artifact>,
    jars: Vec<(String, PathBuf)>,
    artifact: usize,
    pending: Option<Part>,
    jdk: Option<String>,
    level: Option<String>,
    pub(in crate::ide) open: Option<PathBuf>,
    pub(in crate::ide) goto: Option<&'static str>,
    original: Option<StructureAnswer>,
    excluded: BTreeSet<PathBuf>,
    resources: BTreeSet<PathBuf>,
    patterns: String,
    expanded: BTreeSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<PathBuf>>,
    selected: Option<PathBuf>,
    message: Option<String>,
}

const ROW: f32 = 22.0;
const INDENT: f32 = 16.0;
const NAV: f32 = 150.0;

type NavGroup<'a> = (Option<&'static str>, &'a [(Part, String)]);
const DEFAULT: &str = "";

impl ProjectStructurePage {
    pub(in crate::ide) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(in crate::ide) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        let pending = self.pending.take();
        self.reset();
        self.load(dispatch);
        if let Some(part) = pending {
            self.part = part;
        }
    }

    pub(in crate::ide) fn show_part(&mut self, name: &str) -> bool {
        let part = match name {
            "project" => Part::Project,
            "modules" => Part::Modules,
            "libraries" => Part::Libraries,
            "artifacts" => Part::Artifacts,
            "sdks" => Part::Sdks,
            "problems" => Part::Problems,
            _ => return false,
        };
        self.pending = Some(part);
        self.part = part;
        true
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("project.structure.get", json!({}))
            .and_then(|value| serde_json::from_value::<StructureAnswer>(value).map_err(|error| error.to_string()))
        {
            Ok(structure) => {
                self.excluded = structure.excluded.iter().cloned().collect();
                self.resources = structure.resource_roots.iter().cloned().collect();
                self.patterns = structure.exclude_patterns.join(";");
                self.expanded.extend(structure.content_roots.iter().cloned());
                self.listings.clear();
                self.original = Some(structure);
            }
            Err(_) => self.original = None,
        }
        fn read<T: serde::de::DeserializeOwned>(dispatch: &mut Dispatch<'_>, command: &str) -> Option<T> {
            dispatch(command, json!({})).ok().and_then(|value| serde_json::from_value(value).ok())
        }
        self.project = read::<ProjectSection>(dispatch, "project.structure.project");
        self.modules = read::<ModulesSection>(dispatch, "project.structure.modules")
            .map(|section| section.modules)
            .unwrap_or_default();
        self.module = self.module.min(self.modules.len().saturating_sub(1));
        self.libraries = read::<LibrariesSection>(dispatch, "project.structure.libraries")
            .map(|section| section.libraries)
            .unwrap_or_default();
        self.sdks = read::<SdksSection>(dispatch, "project.structure.sdks");
        self.problems = read::<ProblemsSection>(dispatch, "project.structure.problems")
            .map(|section| section.problems)
            .unwrap_or_default();
        let artifacts = read::<ArtifactsSection>(dispatch, "project.structure.artifacts").unwrap_or_default();
        self.artifacts = artifacts.artifacts.into_iter().map(|artifact| (Some(artifact.clone()), artifact)).collect();
        self.suggestions = artifacts.suggestions;
        self.jars = artifacts.jars.into_iter().map(|file| (file.name, file.path)).collect();
        self.artifact = self.artifact.min(self.artifacts.len().saturating_sub(1));
        self.jdk = self.chosen_jdk();
        self.level = self.chosen_level();
    }

    fn chosen_jdk(&self) -> Option<String> {
        let jdk = self.project.as_ref()?.jdk.as_ref()?;
        Some(if jdk.chosen { jdk.jdk.home.display().to_string() } else { DEFAULT.to_owned() })
    }

    fn chosen_level(&self) -> Option<String> {
        let jdk = self.project.as_ref()?.jdk.as_ref()?;
        Some(if jdk.language_level_default { "default".to_owned() } else { jdk.language_level.to_string() })
    }

    pub(in crate::ide) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        let mut failed = None;
        if self.jdk.is_some() && self.jdk != self.chosen_jdk() {
            let home = self.jdk.clone().unwrap_or_default();
            if let Err(error) = dispatch("toolchain.jdk.choose", json!({ "home": home })) {
                failed = Some(error);
            }
        }
        if self.level.is_some() && self.level != self.chosen_level() {
            let level = self.level.clone().unwrap_or_default();
            if let Err(error) = dispatch("toolchain.jdk.languageLevel", json!({ "level": level })) {
                failed = Some(error);
            }
        }
        if let Err(error) = self.commit_artifacts(dispatch) {
            failed = Some(error);
        }
        let Some(original) = self.original.clone().filter(|original| original.editable) else {
            if failed.is_some() || self.project.is_some() {
                self.load(dispatch);
                self.message = failed;
            }
            return;
        };
        let mut run = |command: &str, params: serde_json::Value| {
            if let Err(error) = dispatch(command, params) {
                failed = Some(error);
            }
        };
        for gone in original.excluded.iter().filter(|path| !self.excluded.contains(*path)) {
            run("project.directory.cancelExclusion", json!({ "path": gone }));
        }
        for gone in original.resource_roots.iter().filter(|path| !self.resources.contains(*path)) {
            run("project.directory.unmarkResourceRoot", json!({ "path": gone }));
        }
        for new in self.excluded.iter().filter(|path| !original.excluded.contains(*path)) {
            run("project.directory.markExcluded", json!({ "path": new }));
        }
        for new in self.resources.iter().filter(|path| !original.resource_roots.contains(*path)) {
            run("project.directory.markResourceRoot", json!({ "path": new }));
        }
        let patterns = split_patterns(&self.patterns);
        if patterns != original.exclude_patterns {
            run("project.structure.setExcludePatterns", json!({ "patterns": patterns }));
        }
        let selected = self.selected.take();
        let expanded = std::mem::take(&mut self.expanded);
        let (part, tab, module, artifact) = (self.part, self.tab, self.module, self.artifact);
        self.load(dispatch);
        (self.part, self.tab, self.module) = (part, tab, module.min(self.modules.len().saturating_sub(1)));
        self.artifact = artifact.min(self.artifacts.len().saturating_sub(1));
        self.expanded.extend(expanded);
        self.selected = selected;
        self.message = failed;
    }

    fn commit_artifacts(&mut self, dispatch: &mut Dispatch<'_>) -> Result<(), String> {
        let kept: Vec<String> =
            self.artifacts.iter().filter_map(|(original, _)| original.as_ref().map(|a| a.name.clone())).collect();
        let loaded = dispatch("project.structure.artifacts", json!({}))
            .ok()
            .and_then(|value| serde_json::from_value::<ArtifactsSection>(value).ok())
            .map(|section| section.artifacts)
            .unwrap_or_default();
        let mut failed = None;
        for gone in loaded.iter().filter(|artifact| !kept.contains(&artifact.name)) {
            if let Err(error) = dispatch("project.artifact.delete", json!({ "name": gone.name })) {
                failed = Some(error);
            }
        }
        for (original, artifact) in &self.artifacts {
            if original.as_ref() == Some(artifact) {
                continue;
            }
            let previous = original.as_ref().map(|original| original.name.clone());
            if let Err(error) = dispatch("project.artifact.save", json!({ "artifact": artifact, "previous": previous }))
            {
                failed = Some(error);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    fn mark_of(&self, path: &Path) -> Option<Mark> {
        if self.excluded.contains(path) {
            Some(Mark::Excluded)
        } else if self.resources.contains(path) {
            Some(Mark::ResourceRoot)
        } else {
            None
        }
    }

    fn excluded_by_pattern(&self, path: &Path) -> bool {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        split_patterns(&self.patterns).iter().any(|pattern| zen_vfs::file_types::glob_matches(pattern, &name, false))
    }

    fn toggle(&mut self, mark: Mark) {
        let Some(path) = self.selected.clone() else { return };
        let had = self.mark_of(&path) == Some(mark);
        self.excluded.remove(&path);
        self.resources.remove(&path);
        if !had {
            match mark {
                Mark::Excluded => self.excluded.insert(path),
                Mark::ResourceRoot => self.resources.insert(path),
            };
        }
    }

    fn unmark(&mut self, path: &Path) {
        self.excluded.remove(path);
        self.resources.remove(path);
    }

    #[cfg(test)]
    pub(in crate::ide) fn select(&mut self, path: &Path) {
        self.selected = Some(path.to_owned());
    }

    #[cfg(test)]
    pub(in crate::ide) fn mark_selected(&mut self, resource_root: bool) {
        self.toggle(if resource_root { Mark::ResourceRoot } else { Mark::Excluded });
    }

    #[cfg(test)]
    pub(in crate::ide) fn set_patterns(&mut self, patterns: &str) {
        patterns.clone_into(&mut self.patterns);
    }

    #[cfg(test)]
    pub(in crate::ide) fn shown_folders(&mut self) -> Vec<PathBuf> {
        let roots = self.original.as_ref().map(|original| original.content_roots.clone()).unwrap_or_default();
        let mut shown = Vec::new();
        for root in roots {
            self.collect(&root, &mut shown);
        }
        shown
    }

    #[cfg(test)]
    fn collect(&mut self, folder: &Path, shown: &mut Vec<PathBuf>) {
        shown.push(folder.to_owned());
        if self.expanded.contains(folder) {
            for child in self.children(folder) {
                self.collect(&child, shown);
            }
        }
    }

    fn children(&mut self, folder: &Path) -> Vec<PathBuf> {
        self.listings
            .entry(folder.to_owned())
            .or_insert_with(|| {
                let mut folders: Vec<PathBuf> = zen_vfs::list(folder)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|entry| entry.is_dir && !entry.is_symlink)
                    .map(|entry| entry.path)
                    .collect();
                folders.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
                folders
            })
            .clone()
    }

    pub(in crate::ide) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(original) = self.original.clone() else {
            ui.label(egui::RichText::new(t!("settings-project-structure-no-project")).color(p.text_secondary));
            return;
        };
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.error_text));
            ui.add_space(6.0);
        }
        let width = ui.available_width();
        let height = (ui.clip_rect().bottom() - ui.cursor().top() - 2.0).max(360.0);
        let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let nav = Rect::from_min_size(area.min, vec2(NAV, height));
        ui.painter().line_segment(
            [pos2(nav.right(), area.top()), pos2(nav.right(), area.bottom())],
            Stroke::new(1.0, p.button_border),
        );
        let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(nav.shrink2(vec2(0.0, 2.0))));
        self.navigation(&mut nav_ui, p);
        let content = Rect::from_min_max(pos2(nav.right() + 12.0, area.top()), area.max);
        let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content));
        match self.part {
            Part::Project => self.project_view(&mut content_ui, p),
            Part::Modules => self.modules_view(&mut content_ui, p, &original),
            Part::Libraries => self.libraries_view(&mut content_ui, p),
            Part::Artifacts => self.artifacts_view(&mut content_ui, p),
            Part::Sdks => self.sdks_view(&mut content_ui, p),
            Part::Problems => self.problems_view(&mut content_ui, p),
        }
    }

    fn sources_view(&mut self, ui: &mut Ui, p: &IdePalette, original: &StructureAnswer) {
        let original = original.clone();
        if !original.editable {
            ui.label(egui::RichText::new(t!("settings-project-structure-read-only")).color(p.text_secondary));
            ui.add_space(6.0);
        }
        self.toolbar(ui, p, &original);
        ui.add_space(4.0);
        let width = ui.available_width();
        let height = (ui.clip_rect().bottom() - ui.cursor().top() - 2.0).max(320.0);
        let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let border = Stroke::new(1.0, p.button_border);
        ui.painter().rect_stroke(area, 4, border, StrokeKind::Inside);
        let split = area.left() + (width * 0.62).max(260.0).min(width - 160.0);
        ui.painter().line_segment([pos2(split, area.top()), pos2(split, area.bottom())], border);
        let footer = 62.0;
        let tree = Rect::from_min_max(area.min, pos2(split, area.bottom() - footer));
        let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(tree.shrink(2.0)));
        egui::ScrollArea::both().id_salt("settings-project-structure-tree").auto_shrink([false, false]).show(
            &mut tree_ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for root in &original.content_roots {
                    self.folder_row(ui, p, root, 0, true);
                }
            },
        );
        let bottom = Rect::from_min_max(pos2(area.left() + 8.0, tree.bottom() + 6.0), pos2(split - 8.0, area.bottom()));
        let mut bottom_ui = ui.new_child(egui::UiBuilder::new().max_rect(bottom));
        bottom_ui.horizontal(|ui| {
            let title = ui.label(egui::RichText::new(t!("settings-project-structure-exclude-files")).color(p.text));
            let field = ui
                .add_enabled(
                    original.editable,
                    egui::TextEdit::singleline(&mut self.patterns).desired_width(ui.available_width()),
                )
                .labelled_by(title.id);
            accessibility::edit(&field, |node| node.set_label(t!("settings-project-structure-exclude-files")));
        });
        bottom_ui
            .label(egui::RichText::new(t!("settings-project-structure-exclude-files-note")).color(p.text_secondary));
        let side =
            Rect::from_min_max(pos2(split + 1.0, area.top() + 4.0), pos2(area.right() - 4.0, area.bottom() - 4.0));
        let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side));
        self.side(&mut side_ui, p, &original);
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, original: &StructureAnswer) {
        let selectable =
            self.selected.as_ref().is_some_and(|path| original.editable && !original.content_roots.contains(path));
        let current = self.selected.as_deref().and_then(|path| self.mark_of(path));
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t!("settings-project-structure-mark-as")).color(p.text));
            for (mark, key, kind) in [
                (Mark::Excluded, "settings-project-structure-excluded", FolderKind::Excluded),
                (Mark::ResourceRoot, "settings-project-structure-resource-root", FolderKind::ResourceRoot),
            ] {
                let label = t!(key);
                let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text);
                let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 34.0, 24.0), Sense::click());
                let on = current == Some(mark);
                if on {
                    ui.painter().rect_filled(rect, 4, p.selection);
                } else if selectable && response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.popup_hover);
                }
                let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(16.0, 16.0));
                file_icons::folder_icon_of(ui.painter(), icon, kind, false);
                let color = if selectable { p.text } else { p.gutter_text };
                ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0), galley, color);
                accessibility::item(&response, egui::Role::Button, on, &label);
                if !selectable {
                    accessibility::disabled(&response);
                } else if response.clicked() {
                    chosen = Some(mark);
                }
            }
        });
        if let Some(mark) = chosen {
            self.toggle(mark);
        }
    }

    fn folder_row(&mut self, ui: &mut Ui, p: &IdePalette, folder: &Path, depth: usize, root: bool) {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(240.0), ROW), Sense::click());
        let name = if root {
            folder.display().to_string()
        } else {
            folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let chosen = self.selected.as_deref() == Some(folder);
        if chosen {
            ui.painter().rect_filled(rect, 3, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3, p.hover);
        }
        let mark = self.mark_of(folder);
        let excluded = mark == Some(Mark::Excluded) || (!root && self.excluded_by_pattern(folder));
        let expanded = self.expanded.contains(folder);
        let x = rect.left() + 4.0 + depth as f32 * INDENT;
        let chevron = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(11.0, 11.0));
        if expanded {
            icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        } else {
            icons::chevron_right(ui.painter(), chevron, p.text_secondary);
        }
        let built = self.build_kind(folder);
        let excluded = excluded || built == Some(BuildKind::Excluded);
        let kind = if excluded {
            FolderKind::Excluded
        } else if mark == Some(Mark::ResourceRoot) {
            FolderKind::ResourceRoot
        } else {
            match built {
                Some(BuildKind::Sources) => FolderKind::Source,
                Some(BuildKind::TestSources) => FolderKind::Test,
                Some(BuildKind::Resources | BuildKind::TestResources) => FolderKind::ResourceRoot,
                Some(BuildKind::Excluded) => FolderKind::Excluded,
                None => FolderKind::Plain,
            }
        };
        let icon = Rect::from_center_size(pos2(x + 22.0, rect.center().y), vec2(16.0, 16.0));
        file_icons::folder_icon_of(ui.painter(), icon, kind, false);
        let color = if excluded { p.excluded } else { p.text };
        ui.painter().text(pos2(x + 34.0, rect.center().y), Align2::LEFT_CENTER, &name, theme::regular(13.0), color);
        accessibility::tree_item(&response, depth + 1, Some(expanded), chosen, &name);
        let toggled = response.double_clicked()
            || response.clicked() && ui.input(|input| input.pointer.interact_pos()).is_some_and(|at| at.x < x + 14.0);
        if toggled {
            if expanded {
                self.expanded.remove(folder);
            } else {
                self.expanded.insert(folder.to_owned());
            }
        }
        if response.clicked() {
            self.selected = Some(folder.to_owned());
        }
        if self.expanded.contains(folder) {
            for child in self.children(folder) {
                self.folder_row(ui, p, &child, depth + 1, false);
            }
        }
    }

    fn side(&mut self, ui: &mut Ui, p: &IdePalette, original: &StructureAnswer) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let label = t!("settings-project-structure-add-content-root");
        glyph_plus(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(12.0, 12.0)),
            p.gutter_text,
        );
        ui.painter().text(
            pos2(rect.left() + 22.0, rect.center().y),
            Align2::LEFT_CENTER,
            &label,
            theme::regular(13.0),
            p.gutter_text,
        );
        accessibility::name(&response, egui::Role::Link, &label);
        accessibility::disabled(&response);
        let mut removed = None;
        for root in &original.content_roots {
            let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
            ui.painter().rect_filled(rect, 0, p.hover);
            let font = theme::semibold(13.0);
            let shown = abbreviate(ui, p, root, font.clone(), width - 40.0);
            ui.painter().text(pos2(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER, &shown, font, p.text);
            let cross = Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(12.0, 12.0));
            glyph_close(ui.painter(), cross, p.gutter_text);
            accessibility::name(&response, egui::Role::Button, &t!("settings-project-structure-remove-content-root"));
            accessibility::disabled(&response);
            for (key, list, color) in [
                ("settings-project-structure-excluded-folders", &self.excluded, p.excluded),
                (
                    "settings-project-structure-resource-folders",
                    &self.resources,
                    file_icons::folder_color(FolderKind::ResourceRoot),
                ),
            ] {
                let under: Vec<&PathBuf> = list.iter().filter(|path| path.starts_with(root)).collect();
                if under.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t!(key)).font(theme::semibold(13.0)).color(color));
                for path in under {
                    let shown = path.strip_prefix(root).unwrap_or(path).display().to_string();
                    if self.marked_row(ui, p, &shown, color, original.editable) {
                        removed = Some(path.clone());
                    }
                }
            }
        }
        if let Some(module) = self.modules.get(self.module).cloned() {
            let file = file_name(&module.file);
            let under: Vec<_> = module
                .folders
                .iter()
                .filter(|folder| original.content_roots.iter().any(|root| folder.path.starts_with(root)))
                .collect();
            if !under.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(t!("settings-project-structure-from-build", file = file.as_str()))
                        .font(theme::semibold(13.0))
                        .color(p.text_secondary),
                );
                for kind in [
                    BuildKind::Sources,
                    BuildKind::TestSources,
                    BuildKind::Resources,
                    BuildKind::TestResources,
                    BuildKind::Excluded,
                ] {
                    let folders: Vec<_> = under.iter().filter(|folder| folder.kind == kind).collect();
                    if folders.is_empty() {
                        continue;
                    }
                    let color = match kind {
                        BuildKind::Sources => file_icons::folder_color(FolderKind::Source),
                        BuildKind::TestSources => file_icons::folder_color(FolderKind::Test),
                        BuildKind::Resources | BuildKind::TestResources => {
                            file_icons::folder_color(FolderKind::ResourceRoot)
                        }
                        BuildKind::Excluded => p.excluded,
                    };
                    ui.label(egui::RichText::new(t!(kind_key(kind))).font(theme::semibold(13.0)).color(color));
                    for folder in folders {
                        let root = module.root.as_path();
                        let shown = folder.path.strip_prefix(root).unwrap_or(&folder.path).display().to_string();
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(shown).color(color));
                        });
                    }
                }
            }
        }
        if let Some(path) = removed {
            self.unmark(&path);
        }
    }

    fn build_kind(&self, path: &Path) -> Option<BuildKind> {
        self.modules
            .iter()
            .flat_map(|module| &module.folders)
            .find(|folder| folder.path == path)
            .map(|folder| folder.kind)
    }

    #[cfg(test)]
    pub(in crate::ide) fn choose(&mut self, part: Part) {
        self.part = part;
    }

    #[cfg(test)]
    pub(in crate::ide) fn choose_tab(&mut self, tab: ModuleTab) {
        self.part = Part::Modules;
        self.tab = tab;
    }

    #[cfg(test)]
    pub(in crate::ide) fn set_jdk(&mut self, home: Option<&Path>, level: &str) {
        self.jdk = Some(home.map(|home| home.display().to_string()).unwrap_or_default());
        self.level = Some(level.to_owned());
    }

    #[cfg(test)]
    pub(in crate::ide) fn add_artifact(&mut self, index: usize) {
        if let Some(artifact) = self.suggestions.get(index).cloned() {
            self.add(artifact);
        }
    }

    #[cfg(test)]
    pub(in crate::ide) fn edit_artifact(&mut self, edit: impl FnOnce(&mut Artifact)) {
        if let Some((_, artifact)) = self.artifacts.get_mut(self.artifact) {
            edit(artifact);
        }
    }

    #[cfg(test)]
    pub(in crate::ide) fn artifact_names(&self) -> Vec<String> {
        self.artifacts.iter().map(|(_, artifact)| artifact.name.clone()).collect()
    }

    #[cfg(test)]
    pub(in crate::ide) fn sections(&self) -> (&[Module], &[LibraryEntry], &[StructureProblem]) {
        (&self.modules, &self.libraries, &self.problems)
    }

    fn navigation(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let problems = self.problems.len();
        let groups: [NavGroup<'_>; 3] = [
            (
                Some("settings-project-structure-project-settings"),
                &[
                    (Part::Project, t!("settings-project-structure-project")),
                    (Part::Modules, t!("settings-project-structure-modules")),
                    (Part::Libraries, t!("settings-project-structure-libraries")),
                    (Part::Artifacts, t!("settings-project-structure-artifacts")),
                ],
            ),
            (
                Some("settings-project-structure-platform-settings"),
                &[(Part::Sdks, t!("settings-project-structure-sdks"))],
            ),
            (
                None,
                &[(
                    Part::Problems,
                    if problems == 0 {
                        t!("settings-project-structure-problems")
                    } else {
                        t!("settings-project-structure-problems-count", count = problems)
                    },
                )],
            ),
        ];
        for (title, parts) in groups {
            ui.add_space(6.0);
            if let Some(title) = title {
                let (rect, _) = ui.allocate_exact_size(vec2(NAV - 4.0, ROW), Sense::hover());
                ui.painter().text(
                    pos2(rect.left() + 6.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    t!(title),
                    theme::regular(12.0),
                    p.text_secondary,
                );
            }
            for (part, label) in parts {
                let (rect, response) = ui.allocate_exact_size(vec2(NAV - 4.0, ROW), Sense::click());
                let on = self.part == *part;
                if on {
                    ui.painter().rect_filled(rect, 3, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 3, p.hover);
                }
                let color = if *part == Part::Problems && problems > 0 { p.error_text } else { p.text };
                ui.painter().text(
                    pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    theme::regular(13.0),
                    color,
                );
                accessibility::item(&response, egui::Role::Tab, on, label);
                if response.clicked() {
                    self.part = *part;
                }
            }
        }
    }

    fn build_note(&mut self, ui: &mut Ui, p: &IdePalette, file: &Path) {
        if file.as_os_str().is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(t!("settings-project-structure-from-build", file = file_name(file)))
                    .color(p.text_secondary),
            );
            if ui.link(t!("settings-project-structure-open-build-file")).clicked() {
                self.open = Some(file.to_owned());
            }
        });
    }

    fn field(ui: &mut Ui, p: &IdePalette, label: &str, add: impl FnOnce(&mut Ui)) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(130.0, ROW), Sense::hover());
            ui.painter().text(
                pos2(rect.left(), rect.center().y),
                Align2::LEFT_CENTER,
                label,
                theme::regular(13.0),
                p.text,
            );
            add(ui);
        });
        ui.add_space(4.0);
    }

    fn project_view(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(project) = self.project.clone() else {
            ui.label(egui::RichText::new(t!("settings-project-structure-unknown")).color(p.text_secondary));
            return;
        };
        widgets::group_title(ui, p, &t!("settings-project-structure-project"), None);
        ui.label(egui::RichText::new(t!("settings-project-structure-project-note")).color(p.text_secondary));
        ui.add_space(8.0);
        Self::field(ui, p, &t!("settings-project-structure-name"), |ui| {
            ui.label(egui::RichText::new(&project.name).color(p.text));
        });
        let sdk_label = t!("settings-project-structure-sdk");
        if let Some(jdk) = &project.jdk {
            let default = t!("settings-project-structure-sdk-default", jdk = jdk_label(&project));
            let mut options = vec![(DEFAULT.to_owned(), default)];
            options.extend(
                project.jdks.iter().map(|found| (found.home.display().to_string(), format!("JDK {}", found.label()))),
            );
            Self::field(ui, p, &sdk_label, |ui| {
                choice(ui, "project-structure-sdk", &sdk_label, &options, &mut self.jdk)
            });
            let level_label = t!("settings-project-structure-language-level");
            let newest = project.jdks.iter().map(|found| found.major).max().unwrap_or(jdk.jdk.major).max(jdk.jdk.major);
            let mut levels =
                vec![("default".to_owned(), t!("settings-project-structure-level-default", level = jdk.jdk.major))];
            levels.extend((8..=newest.max(8)).rev().map(|level| (level.to_string(), level.to_string())));
            Self::field(ui, p, &level_label, |ui| {
                choice(ui, "project-structure-level", &level_label, &levels, &mut self.level);
            });
        } else if project.jvm {
            Self::field(ui, p, &sdk_label, |ui| {
                ui.label(egui::RichText::new(t!("settings-project-structure-no-jdk")).color(p.error_text));
            });
        } else {
            Self::field(ui, p, &sdk_label, |ui| {
                let text = project.sdk.as_ref().map_or_else(
                    || t!("settings-project-structure-sdk-none"),
                    |sdk| format!("{} {}", sdk.name, sdk.version).trim().to_owned(),
                );
                ui.label(egui::RichText::new(text).color(p.text));
            });
            ui.label(egui::RichText::new(t!("settings-project-structure-sdk-of-language")).color(p.text_secondary));
        }
        if let Some(level) = &project.build_language_level {
            ui.label(
                egui::RichText::new(t!("settings-project-structure-build-level", level = level.as_str()))
                    .color(p.text_secondary),
            );
        }
        ui.add_space(8.0);
        let output = &project.compiler_output;
        Self::field(ui, p, &t!("settings-project-structure-compiler-output"), |ui| {
            let text = output.path.as_ref().map_or_else(
                || t!(origin_key(output.origin)),
                |path| path.strip_prefix(&project.root).unwrap_or(path).display().to_string(),
            );
            ui.label(egui::RichText::new(text).color(p.text));
        });
        if output.path.is_some() {
            ui.label(egui::RichText::new(t!(origin_key(output.origin))).color(p.text_secondary));
        }
        if let Some(file) = &project.build_file {
            ui.add_space(8.0);
            self.build_note(ui, p, file);
        }
    }

    fn modules_view(&mut self, ui: &mut Ui, p: &IdePalette, original: &StructureAnswer) {
        if !self.modules.is_empty() {
            let names: Vec<(String, String)> = self
                .modules
                .iter()
                .enumerate()
                .map(|(index, module)| (index.to_string(), module.name.clone()))
                .collect();
            let label = t!("settings-project-structure-module");
            let mut chosen = Some(self.module.to_string());
            Self::field(ui, p, &label, |ui| choice(ui, "project-structure-module", &label, &names, &mut chosen));
            self.module = chosen.and_then(|index| index.parse().ok()).unwrap_or(0);
            let module = self.modules[self.module].clone();
            self.build_note(ui, p, &module.file);
            let level = module.language_version.clone().unwrap_or_else(|| t!("settings-project-structure-level-unset"));
            Self::field(ui, p, &t!("settings-project-structure-language-level"), |ui| {
                ui.label(egui::RichText::new(level).color(p.text));
            });
        } else {
            ui.label(egui::RichText::new(t!("settings-project-structure-no-build")).color(p.text_secondary));
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            for (tab, key) in [
                (ModuleTab::Sources, "settings-project-structure-sources"),
                (ModuleTab::Paths, "settings-project-structure-paths"),
                (ModuleTab::Dependencies, "settings-project-structure-dependencies"),
            ] {
                let label = t!(key);
                let response = ui.selectable_label(self.tab == tab, &label);
                accessibility::item(&response, egui::Role::Tab, self.tab == tab, &label);
                if response.clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(4.0);
        let module = self.modules.get(self.module).cloned();
        match self.tab {
            ModuleTab::Sources => self.sources_view(ui, p, original),
            ModuleTab::Paths => match &module {
                Some(module) => {
                    let origin = if module.output.is_none() {
                        Origin::Unknown
                    } else if module.assumed {
                        Origin::Convention
                    } else {
                        Origin::Build
                    };
                    for (key, path) in [
                        ("settings-project-structure-output", &module.output),
                        ("settings-project-structure-test-output", &module.test_output),
                    ] {
                        Self::field(ui, p, &t!(key), |ui| {
                            let text = path.as_ref().map_or_else(
                                || t!("settings-project-structure-output-unknown"),
                                |path| path.strip_prefix(&module.root).unwrap_or(path).display().to_string(),
                            );
                            ui.label(egui::RichText::new(text).color(p.text));
                        });
                    }
                    ui.label(egui::RichText::new(t!(origin_key(origin))).color(p.text_secondary));
                }
                None => {
                    let origin =
                        self.project.as_ref().map_or(Origin::Unknown, |project| project.compiler_output.origin);
                    ui.label(egui::RichText::new(t!(origin_key(origin))).color(p.text_secondary));
                }
            },
            ModuleTab::Dependencies => match &module {
                Some(module) => dependencies(ui, p, module),
                None => {
                    ui.label(
                        egui::RichText::new(t!("settings-project-structure-no-dependencies")).color(p.text_secondary),
                    );
                }
            },
        }
    }

    fn libraries_view(&mut self, ui: &mut Ui, p: &IdePalette) {
        widgets::group_title(ui, p, &t!("settings-project-structure-libraries"), None);
        if self.libraries.is_empty() {
            ui.label(egui::RichText::new(t!("settings-project-structure-no-libraries")).color(p.text_secondary));
            return;
        }
        ui.label(egui::RichText::new(t!("settings-project-structure-libraries-note")).color(p.text_secondary));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("project-structure-libraries").auto_shrink([false, false]).show(
            ui,
            |ui| {
                egui::Grid::new("project-structure-libraries-grid").num_columns(3).spacing(vec2(16.0, 4.0)).show(
                    ui,
                    |ui| {
                        for entry in &self.libraries {
                            let library = &entry.library;
                            let label = library.label.clone().unwrap_or_else(|| match &library.version {
                                Some(version) => format!("{}@{version}", library.name),
                                None => library.name.clone(),
                            });
                            let missing = zen_workbench::project_structure::is_missing(library);
                            ui.label(egui::RichText::new(label).color(if missing { p.error_text } else { p.text }));
                            let place = if missing {
                                t!("settings-project-structure-not-downloaded")
                            } else {
                                library.root.display().to_string()
                            };
                            ui.label(egui::RichText::new(place).color(p.text_secondary));
                            ui.label(egui::RichText::new(entry.used_by.join(", ")).color(p.text_secondary));
                            ui.end_row();
                        }
                    },
                );
            },
        );
    }

    fn sdks_view(&mut self, ui: &mut Ui, p: &IdePalette) {
        widgets::group_title(ui, p, &t!("settings-project-structure-sdks"), None);
        let Some(sdks) = self.sdks.clone() else { return };
        egui::Grid::new("project-structure-sdks-grid").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for jdk in &sdks.jdks {
                ui.label(egui::RichText::new(format!("JDK {}", jdk.label())).color(p.text));
                ui.label(egui::RichText::new(jdk.home.display().to_string()).color(p.text_secondary));
                ui.end_row();
            }
            for sdk in &sdks.others {
                ui.label(egui::RichText::new(format!("{} {}", sdk.name, sdk.version).trim().to_owned()).color(p.text));
                ui.label(egui::RichText::new(sdk.home.display().to_string()).color(p.text_secondary));
                ui.end_row();
            }
        });
        if sdks.jdks.is_empty() && sdks.others.is_empty() {
            ui.label(egui::RichText::new(t!("settings-project-structure-sdk-none")).color(p.text_secondary));
        }
        ui.add_space(8.0);
        if ui.link(t!("settings-project-structure-manage-jdks")).clicked() {
            self.goto = Some(zen_workbench::jdk::commands::PAGE);
        }
    }

    fn problems_view(&mut self, ui: &mut Ui, p: &IdePalette) {
        widgets::group_title(ui, p, &t!("settings-project-structure-problems"), None);
        if self.problems.is_empty() {
            ui.label(egui::RichText::new(t!("settings-project-structure-no-problems")).color(p.text_secondary));
            return;
        }
        let problems = self.problems.clone();
        for problem in &problems {
            ui.horizontal_wrapped(|ui| {
                let color = if problem.warning { p.warning_text } else { p.error_text };
                ui.label(egui::RichText::new(&problem.message).color(color));
                let (part, key) = match problem.section {
                    Section::Project => (Part::Project, "settings-project-structure-project"),
                    Section::Modules => (Part::Modules, "settings-project-structure-modules"),
                    Section::Libraries => (Part::Libraries, "settings-project-structure-libraries"),
                    Section::Sdks => (Part::Sdks, "settings-project-structure-sdks"),
                };
                if ui.link(t!(key)).clicked() {
                    self.part = part;
                }
                if let Some(file) = &problem.file
                    && ui.link(file_name(file)).clicked()
                {
                    self.open = Some(file.clone());
                }
            });
            ui.add_space(4.0);
        }
    }

    fn marked_row(&self, ui: &mut Ui, p: &IdePalette, shown: &str, color: egui::Color32, editable: bool) -> bool {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW - 4.0), Sense::hover());
        let galley = ui.painter().layout_no_wrap(shown.to_owned(), theme::regular(13.0), color);
        let text_end = rect.left() + 12.0 + galley.size().x;
        ui.painter().galley(pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0), galley, color);
        let cross = Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(16.0, 16.0));
        let mut x = text_end + 2.0;
        while x < cross.left() - 2.0 {
            ui.painter().circle_filled(pos2(x, rect.center().y + 5.0), 0.5, p.gutter_text);
            x += 3.0;
        }
        let response = ui.interact(cross, ui.scope_id().with(("unmark", shown)), Sense::click());
        let name = t!("settings-project-structure-unmark", name = shown);
        let response = response.on_hover_text(&name);
        if editable && response.hovered() {
            ui.painter().rect_filled(cross, 3, p.popup_hover);
        }
        glyph_close(ui.painter(), cross.shrink(2.0), if editable { p.text_secondary } else { p.gutter_text });
        accessibility::name(&response, egui::Role::Button, &name);
        if !editable {
            accessibility::disabled(&response);
            return false;
        }
        response.clicked()
    }
}

fn file_name(file: &Path) -> String {
    file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn jdk_label(project: &ProjectSection) -> String {
    project.jdk.as_ref().map_or_else(String::new, |jdk| jdk.jdk.label())
}

fn kind_key(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Sources => "settings-project-structure-source-folders",
        BuildKind::TestSources => "settings-project-structure-test-folders",
        BuildKind::Resources => "settings-project-structure-resource-folders",
        BuildKind::TestResources => "settings-project-structure-test-resource-folders",
        BuildKind::Excluded => "settings-project-structure-excluded-folders",
    }
}

fn origin_key(origin: Origin) -> &'static str {
    match origin {
        Origin::Build => "settings-project-structure-origin-build",
        Origin::Convention => "settings-project-structure-origin-convention",
        Origin::Temporary => "settings-project-structure-origin-temporary",
        Origin::Ide => "settings-project-structure-origin-ide",
        Origin::Unknown => "settings-project-structure-output-unknown",
    }
}

fn choice(ui: &mut Ui, salt: &str, label: &str, options: &[(String, String)], chosen: &mut Option<String>) {
    let shown = options
        .iter()
        .find(|(value, _)| Some(value) == chosen.as_ref())
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let combo = egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .width(widgets::field_width(ui, 220.0, 420.0))
        .truncate()
        .show_ui(ui, |ui| {
            for (value, text) in options {
                if ui.selectable_label(Some(value) == chosen.as_ref(), text).clicked() {
                    *chosen = Some(value.clone());
                }
            }
        });
    accessibility::name(&combo.response, egui::Role::ComboBox, label);
}

fn dependencies(ui: &mut Ui, p: &IdePalette, module: &Module) {
    if module.dependencies.is_empty() {
        ui.label(egui::RichText::new(t!("settings-project-structure-no-dependencies")).color(p.text_secondary));
        return;
    }
    egui::ScrollArea::vertical().id_salt("project-structure-dependencies").auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("project-structure-dependencies-grid")
            .num_columns(3)
            .spacing(vec2(16.0, 4.0))
            .striped(true)
            .show(ui, |ui| {
                for header in [
                    "settings-project-structure-dependency",
                    "settings-project-structure-version",
                    "settings-project-structure-scope",
                ] {
                    ui.label(egui::RichText::new(t!(header)).font(theme::semibold(13.0)).color(p.text));
                }
                ui.end_row();
                for dependency in &module.dependencies {
                    ui.label(egui::RichText::new(&dependency.name).color(p.text));
                    let version = if dependency.version.is_empty() {
                        t!("settings-project-structure-version-managed")
                    } else {
                        dependency.version.clone()
                    };
                    ui.label(egui::RichText::new(version).color(p.text_secondary));
                    let mut scope = dependency.scope.clone().unwrap_or_default();
                    if dependency.indirect {
                        scope = t!("settings-project-structure-indirect");
                    }
                    ui.label(egui::RichText::new(scope).color(p.text_secondary));
                    ui.end_row();
                }
            });
    });
}

fn abbreviate(ui: &Ui, p: &IdePalette, path: &Path, font: egui::FontId, width: f32) -> String {
    let fits = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), p.text).size().x <= width;
    let full = path.display().to_string();
    if fits(&full) {
        return full;
    }
    let parts: Vec<String> = path.iter().map(|part| part.to_string_lossy().into_owned()).collect();
    let head = parts.len().min(3);
    for tail in (1..parts.len().saturating_sub(head)).rev() {
        let mut shown = PathBuf::new();
        parts[..head].iter().for_each(|part| shown.push(part));
        shown.push("…");
        parts[parts.len() - tail..].iter().for_each(|part| shown.push(part));
        let text = shown.display().to_string();
        if fits(&text) {
            return text;
        }
    }
    full
}

fn split_patterns(text: &str) -> Vec<String> {
    let mut patterns: Vec<String> = Vec::new();
    for pattern in text.split(';').map(str::trim).filter(|pattern| !pattern.is_empty()) {
        if !patterns.iter().any(|kept| kept == pattern) {
            patterns.push(pattern.to_owned());
        }
    }
    patterns
}
