use std::path::{Path, PathBuf};

use egui::{Align2, Key, Modifiers, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::agent::ActionError::{self, Dispatch, Refused};
use super::{IdePalette, IdeView, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const WIDTH: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    ProjectView,
    Structure,
    Commit,
    Finder,
    Terminal,
}

impl Target {
    pub(super) const ALL: [Self; 5] = [Self::ProjectView, Self::Structure, Self::Commit, Self::Finder, Self::Terminal];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ProjectView => "projectView",
            Self::Structure => "structure",
            Self::Commit => "commit",
            Self::Finder => "finder",
            Self::Terminal => "terminal",
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.id() == id)
    }

    pub(super) fn label(self) -> String {
        match self {
            Self::ProjectView => t!("ide-select-in-project-view"),
            Self::Structure => t!("ide-structure-title"),
            Self::Commit => t!("ide-select-in-commit"),
            Self::Finder => super::file_manager_label(),
            Self::Terminal => t!("ide-terminal-title"),
        }
    }
}

struct Shown {
    path: PathBuf,
    targets: Vec<Target>,
    selected: usize,
}

#[derive(Default)]
pub(super) struct SelectIn {
    shown: Option<Shown>,
}

impl SelectIn {
    pub(super) fn open(&mut self, path: &Path, targets: Vec<Target>) {
        self.shown = (!targets.is_empty()).then(|| Shown { path: path.to_owned(), targets, selected: 0 });
    }

    pub(super) fn close(&mut self) {
        self.shown = None;
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub(super) fn show(&mut self, ui: &mut Ui, palette: &IdePalette) -> Option<(Target, PathBuf)> {
        let shown = self.shown.as_mut()?;
        let count = shown.targets.len();
        let (mut picked, mut close) = (None, false);
        ui.input_mut(|input| {
            if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                shown.selected = (shown.selected + 1) % count;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                shown.selected = shown.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if input.consume_key(Modifiers::NONE, Key::Enter) {
                picked = Some(shown.selected);
            }
            if input.consume_key(Modifiers::NONE, Key::Escape) {
                close = true;
            }
            for (index, key) in DIGITS.into_iter().enumerate().take(count) {
                if input.consume_key(Modifiers::NONE, key) {
                    picked = Some(index);
                }
            }
        });
        super::filter_popups::take_typing(ui);
        let screen = ui.ctx().content_rect();
        let height = ROW * count as f32 + 40.0;
        let area = egui::Area::new(ui.scope_id().with("select-in"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - WIDTH / 2.0, screen.center().y - height / 2.0 - 40.0));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(WIDTH - 8.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(t!("ide-select-in-title")).font(theme::semibold(13.0)).color(palette.text),
                    );
                });
                ui.add_space(6.0);
                for (index, target) in shown.targets.iter().enumerate() {
                    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let selected = index == shown.selected;
                    if row.hovered() && !selected && ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO) {
                        shown.selected = index;
                    }
                    if index == shown.selected {
                        ui.painter().rect_filled(rect, 4, palette.selection);
                    }
                    let label = target.label();
                    let name = format!("{}. {label}", index + 1);
                    accessibility::name_with(&row, egui::Role::ListBoxOption, || name.clone());
                    accessibility::edit(&row, |node| node.set_selected(index == shown.selected));
                    let painter = ui.painter();
                    let number = painter.text(
                        pos2(rect.left() + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}.", index + 1),
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    painter.text(
                        pos2(number.right() + 6.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        theme::regular(13.0),
                        palette.text,
                    );
                    if row.clicked() {
                        picked = Some(index);
                    }
                }
                ui.add_space(4.0);
            });
        });
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !response.response.rect.contains(pos))
        });
        if let Some(index) = picked {
            let shown = self.shown.take()?;
            return Some((shown.targets[index], shown.path));
        }
        if close || pressed_outside {
            self.shown = None;
        }
        None
    }
}

impl IdeView {
    pub(super) fn select_in_file(
        &self,
        session: &Session,
        params: &Value,
        agent: bool,
    ) -> Result<PathBuf, ActionError> {
        let workbench = session.state::<Workbench>();
        let given = params.get("path").and_then(Value::as_str).map(PathBuf::from);
        let given = given.map(|path| match workbench.and_then(Workbench::project) {
            Some(project) if path.is_relative() => project.join(path),
            _ => path,
        });
        let in_tree = (!agent && self.tree_focused && self.project_open)
            .then(|| self.selection.lead().filter(|path| path.exists()).cloned())
            .flatten();
        let path =
            given.or(in_tree).or_else(|| workbench.and_then(Workbench::active_document).map(|d| d.path().to_owned()));
        match path {
            Some(path) if path.exists() => Ok(path),
            Some(path) => Err(Refused(format!("{} does not exist", path.display()))),
            None => Err(Refused("Select In needs a file open in the editor, or a `path`".to_owned())),
        }
    }

    pub(super) fn select_in_targets(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
    ) -> Vec<Target> {
        let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let in_tree = project.as_deref().is_some_and(|project| {
            self.refresh_libraries(ctx, registry, session, project);
            zen_workbench::inside_project(project, path)
                || self.libraries.as_ref().is_some_and(|libraries| libraries.shown_path(project, path).is_some())
        });
        self.refresh_changes(registry, session);
        let changed = self.vcs.info.root.is_some() && self.commit_window.is_changed(path);
        Target::ALL
            .into_iter()
            .filter(|target| match target {
                Target::ProjectView => in_tree,
                Target::Structure => path.is_file(),
                Target::Commit => changed,
                Target::Finder | Target::Terminal => cfg!(not(target_arch = "wasm32")),
            })
            .collect()
    }

    pub(super) fn select_in_target(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        (target, path): (Target, &Path),
        params: &Value,
        agent: bool,
    ) -> Result<Value, ActionError> {
        let answer = json!({ "target": target.id(), "path": path });
        match target {
            Target::ProjectView => {
                let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let Some(project) = project else { return Err(Refused("no project is open".to_owned())) };
                self.refresh_libraries(ctx, registry, session, &project);
                let shown = Some(path)
                    .filter(|path| !zen_workbench::inside_project(&project, path))
                    .and_then(|path| self.libraries.as_ref()?.shown_path(&project, path))
                    .unwrap_or_else(|| path.to_owned());
                self.select_paths(&project, vec![shown.clone()]).map_err(Refused)?;
                self.show_tool("project");
                self.tree_focused = true;
                Ok(json!({ "target": target.id(), "path": shown }))
            }
            Target::Structure => {
                if !path.is_file() {
                    return Err(Refused(format!("{} is not a file", path.display())));
                }
                let active =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                if active.as_deref() != Some(path) {
                    registry.dispatch(session, "file.document.open", json!({ "path": path })).map_err(Dispatch)?;
                }
                self.show_tool("structure");
                self.structure_window.select_at_caret();
                Ok(answer)
            }
            Target::Commit => {
                self.refresh_changes(registry, session);
                if !self.commit_window.select(path) {
                    return Err(Refused(format!("{} has no changes to commit", path.display())));
                }
                self.commit_window.focus_tree = true;
                self.show_tool("commit");
                Ok(answer)
            }
            Target::Finder => {
                let dry_run = params.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
                registry
                    .dispatch(session, "file.fs.reveal", json!({ "path": path, "dryRun": dry_run }))
                    .map_err(Dispatch)?;
                Ok(answer)
            }
            Target::Terminal => {
                let opened = self.perform_action(
                    ctx,
                    registry,
                    session,
                    "view.project.openInTerminal",
                    json!({ "path": path }),
                    agent,
                )?;
                Ok(json!({ "target": target.id(), "path": path, "terminal": opened }))
            }
        }
    }
}

const DIGITS: [Key; 9] =
    [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, popup: &mut SelectIn, events: Vec<egui::Event>) -> Option<(Target, PathBuf)> {
        let mut out = None;
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            out = popup.show(ui, &IdePalette::zen_dark());
        });
        output.textures_delta.clear();
        out
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn targets_have_stable_names() {
        for target in Target::ALL {
            assert_eq!(Target::from_id(target.id()), Some(target));
        }
        assert_eq!(Target::from_id("navigationBar"), None);
    }

    #[test]
    fn a_digit_or_enter_picks_a_target_and_escape_closes() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let path = Path::new("/p/main.go");
        let mut popup = SelectIn::default();
        popup.open(path, vec![Target::ProjectView, Target::Structure, Target::Finder, Target::Terminal]);
        assert!(popup.is_open());
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Num3)]), Some((Target::Finder, path.to_owned())));
        assert!(!popup.is_open());

        popup.open(path, vec![Target::ProjectView, Target::Structure]);
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Num5)]), None, "no fifth target");
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::ArrowUp)]), None);
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Enter)]), Some((Target::Structure, path.to_owned())));

        popup.open(path, vec![Target::ProjectView]);
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Escape)]), None);
        assert!(!popup.is_open());
        popup.open(path, Vec::new());
        assert!(!popup.is_open(), "nothing to choose");
    }
}
