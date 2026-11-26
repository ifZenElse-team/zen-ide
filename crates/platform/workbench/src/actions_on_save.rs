use std::ops::Range;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session, Settings};
use zen_i18n::t;

use crate::language::format::{OPTIMIZE_IMPORTS_ON_SAVE, REFORMAT_ON_SAVE};
use crate::{Workbench, code_style};

pub const FILE: &str = ".zen/actionsOnSave.json";
pub const REFORMAT_FILES: &str = "editor.onSave.reformatFiles";
pub const REFORMAT_SCOPE: &str = "editor.onSave.reformatScope";
pub const OPTIMIZE_FILES: &str = "editor.onSave.optimizeFiles";

/// What Reformat code touches on a save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReformatScope {
    /// The whole file.
    #[default]
    WholeFile,
    /// Only the lines that differ from the version control's last commit.
    ChangedLines,
}

/// Settings → Tools → Actions on Save of one project, or the defaults of new projects.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ActionsOnSave {
    /// Reformat code on a save.
    pub reformat: bool,
    /// The languages Reformat code applies to; empty is every language.
    pub reformat_files: Vec<String>,
    /// What Reformat code touches.
    pub reformat_scope: ReformatScope,
    /// Optimize imports on a save.
    pub optimize_imports: bool,
    /// The languages Optimize imports applies to; empty is every language.
    pub optimize_files: Vec<String>,
}

impl ActionsOnSave {
    pub fn apply_to(&self, settings: &mut Settings) {
        let _ = settings.set(REFORMAT_ON_SAVE, self.reformat);
        let _ = settings.set(REFORMAT_FILES, &self.reformat_files);
        let _ = settings.set(REFORMAT_SCOPE, self.reformat_scope);
        let _ = settings.set(OPTIMIZE_IMPORTS_ON_SAVE, self.optimize_imports);
        let _ = settings.set(OPTIMIZE_FILES, &self.optimize_files);
    }

    pub fn from_session(settings: &Settings) -> Self {
        Self {
            reformat: settings.get_or(REFORMAT_ON_SAVE, false),
            reformat_files: settings.get::<Vec<String>>(REFORMAT_FILES).unwrap_or_default(),
            reformat_scope: settings.get_or(REFORMAT_SCOPE, ReformatScope::WholeFile),
            optimize_imports: settings.get_or(OPTIMIZE_IMPORTS_ON_SAVE, false),
            optimize_files: settings.get::<Vec<String>>(OPTIMIZE_FILES).unwrap_or_default(),
        }
    }

    pub fn reformats(&self, path: &Path) -> bool {
        self.reformat && covers(&self.reformat_files, path)
    }

    pub fn optimizes(&self, path: &Path) -> bool {
        self.optimize_imports && covers(&self.optimize_files, path)
    }
}

fn covers(languages: &[String], path: &Path) -> bool {
    languages.is_empty() || languages.iter().any(|language| *language == code_style::language_of(path))
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

const MAX_FILE: u64 = 64 << 10;

fn read(root: &Path) -> Option<ActionsOnSave> {
    let path = root.join(FILE);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn own(session: &Session, root: &Path) -> Option<ActionsOnSave> {
    crate::trust::trusted(session, root).then(|| read(root)).flatten()
}

pub fn of(session: &Session) -> ActionsOnSave {
    project_root(session)
        .and_then(|root| own(session, &root))
        .unwrap_or_else(|| ActionsOnSave::from_session(session.settings()))
}

pub fn changed_lines(base: &str, text: &str) -> Vec<Range<usize>> {
    zen_vcs::diff::hunks(base, text).into_iter().map(|hunk| hunk.after).collect()
}

pub fn touches(changed: &[Range<usize>], first: usize, last: usize) -> bool {
    changed.iter().any(|lines| {
        let end = lines.end.max(lines.start + 1);
        lines.start <= last && first < end
    })
}

pub fn keeping_changed(old: &str, new: &str, base: &str) -> String {
    let changed = changed_lines(base, old);
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let mut kept = String::with_capacity(old.len());
    let mut at = 0;
    for hunk in zen_vcs::diff::hunks(old, new) {
        kept.extend(old_lines[at..hunk.before.start].iter().copied());
        let last = hunk.before.end.max(hunk.before.start + 1) - 1;
        if hunk.before.len() == hunk.after.len() {
            for (before, after) in hunk.before.clone().zip(hunk.after.clone()) {
                kept.push_str(if touches(&changed, before, before) { new_lines[after] } else { old_lines[before] });
            }
        } else if touches(&changed, hunk.before.start, last) {
            kept.extend(new_lines[hunk.after.clone()].iter().copied());
        } else {
            kept.extend(old_lines[hunk.before.clone()].iter().copied());
        }
        at = hunk.before.end;
    }
    kept.extend(old_lines[at..].iter().copied());
    kept
}

/// The Actions on Save of the open project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectActions {
    /// Whether a project is open.
    pub project: bool,
    /// Whether it keeps its own in `.zen/actionsOnSave.json`; otherwise the defaults for new projects apply.
    pub own: bool,
    /// The actions that apply.
    pub actions: ActionsOnSave,
    /// Whether the project is under version control, which Changed lines needs.
    pub version_control: bool,
}

pub struct GetActions;

impl Command for GetActions {
    type Params = NoParams;
    type Output = ProjectActions;
    const ID: &'static str = "project.actionsOnSave.get";
    const TITLE: &'static str = "Actions on Save of Project";
    const DESCRIPTION: &'static str = "Tells the actions a save runs in the open project: its own from \
         .zen/actionsOnSave.json, or the defaults for new projects (the settings reformatOnSave, \
         optimizeImportsOnSave, reformatOnSaveFiles, reformatOnSaveScope and optimizeImportsOnSaveFiles).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProjectActions, CommandError> {
        let root = project_root(session);
        if let Some(root) = &root {
            crate::hidden_files::guard_read(session, &[root.join(FILE)])?;
        }
        let own = root.as_deref().is_some_and(|root| own(session, root).is_some());
        Ok(ProjectActions {
            project: root.is_some(),
            own,
            actions: of(session),
            version_control: crate::vcs::repository(session).is_ok(),
        })
    }
}

/// The actions a project's saves run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetActionsParams {
    /// The actions; project.actionsOnSave.get gives their shape.
    pub actions: ActionsOnSave,
}

pub struct SetActions;

impl Command for SetActions {
    type Params = SetActionsParams;
    type Output = ProjectActions;
    const ID: &'static str = "project.actionsOnSave.set";
    const TITLE: &'static str = "Set Actions on Save of Project";
    const DESCRIPTION: &'static str = "Writes the actions a save runs in the open project to \
         .zen/actionsOnSave.json; from then on the defaults for new projects no longer apply to it.";

    fn run(&self, session: &mut Session, params: SetActionsParams) -> Result<ProjectActions, CommandError> {
        let root =
            project_root(session).ok_or_else(|| CommandError::new(t!("workbench-actions-on-save-no-project")))?;
        crate::trust::guard_agent_build(session, &root)?;
        let path = root.join(FILE);
        crate::hidden_files::guard_change(session, &root, std::slice::from_ref(&path))?;
        if crate::guard::refuses_in(Some(&root), &path) {
            return Err(CommandError::new(crate::guard::refusal(&path)));
        }
        let text =
            serde_json::to_string_pretty(&params.actions).map_err(|error| CommandError::new(error.to_string()))?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| CommandError::new(error.to_string()))?;
        }
        crate::disk::edit_file(session, &path, |_| Some(format!("{text}\n")))?;
        GetActions.run(session, NoParams {})
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetActions)?;
    registry.register(SetActions)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        (registry, session, dir)
    }

    fn write_own(dir: &tempfile::TempDir, text: &str) {
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(dir.path().join(FILE), text).unwrap();
    }

    #[test]
    fn a_projects_own_actions_count_only_once_it_is_trusted() {
        let (registry, mut session, dir) = setup();
        write_own(&dir, "{ \"reformat\": true }");
        assert!(of(&session).reformat);
        registry.dispatch(&mut session, "project.trust.revoke", json!({})).unwrap();
        assert!(!of(&session).reformat, "safe mode takes the person's defaults");
        let answer = registry.dispatch(&mut session, "project.actionsOnSave.get", json!({})).unwrap();
        assert_eq!(answer["own"], false, "{answer}");
    }

    #[test]
    fn a_file_too_large_or_not_a_file_is_not_read() {
        let (_registry, session, dir) = setup();
        write_own(&dir, &format!("{{ \"reformat\": true }}{}", " ".repeat(MAX_FILE as usize)));
        assert!(!of(&session).reformat);
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        std::fs::create_dir(dir.path().join(FILE)).unwrap();
        assert!(!of(&session).reformat);
    }

    #[test]
    fn an_agent_neither_reads_nor_writes_the_file_when_it_is_hidden() {
        let (registry, mut session, dir) = setup();
        write_own(&dir, "{ \"reformat\": true }");
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".zen/".to_owned()]);
        let refused = zen_core::macros::as_agent(&mut session, |session| {
            let read = registry.dispatch(session, "project.actionsOnSave.get", json!({}));
            let params = json!({ "actions": ActionsOnSave::default() });
            (read, registry.dispatch(session, "project.actionsOnSave.set", params))
        });
        assert!(format!("{:?}", refused.0).contains(crate::hidden_files::REDACTED_CODE), "{:?}", refused.0);
        assert!(format!("{:?}", refused.1).contains(crate::hidden_files::REDACTED_CODE), "{:?}", refused.1);
        assert!(std::fs::read_to_string(dir.path().join(FILE)).unwrap().contains("true"));
        assert!(registry.dispatch(&mut session, "project.actionsOnSave.get", json!({})).is_ok());
    }

    #[test]
    fn a_project_without_a_file_takes_the_defaults_and_a_file_wins_over_them() {
        let (registry, mut session, dir) = setup();
        session.settings_mut().set(REFORMAT_ON_SAVE, true).unwrap();
        let answer = registry.dispatch(&mut session, "project.actionsOnSave.get", json!({})).unwrap();
        assert_eq!((answer["project"].as_bool(), answer["own"].as_bool()), (Some(true), Some(false)));
        assert_eq!(answer["actions"]["reformat"], true);
        let actions = ActionsOnSave { optimize_imports: true, ..ActionsOnSave::default() };
        registry.dispatch(&mut session, "project.actionsOnSave.set", json!({ "actions": actions })).unwrap();
        assert!(dir.path().join(FILE).exists());
        assert_eq!(of(&session), actions);
    }

    #[test]
    fn setting_refuses_when_no_project_is_open() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let params = json!({ "actions": ActionsOnSave::default() });
        assert!(registry.dispatch(&mut session, "project.actionsOnSave.set", params).is_err());
    }

    #[test]
    fn the_files_narrow_an_action_to_its_languages() {
        let actions = ActionsOnSave {
            reformat: true,
            reformat_files: vec!["go".to_owned()],
            optimize_imports: true,
            ..ActionsOnSave::default()
        };
        assert!(actions.reformats(Path::new("/p/a.go")));
        assert!(!actions.reformats(Path::new("/p/a.rs")));
        assert!(actions.optimizes(Path::new("/p/a.rs")), "an empty list is every language");
    }

    #[test]
    fn the_actions_go_through_the_session() {
        let mut settings = Settings::default();
        let actions = ActionsOnSave {
            reformat: true,
            reformat_files: vec!["go".to_owned()],
            reformat_scope: ReformatScope::ChangedLines,
            optimize_imports: true,
            optimize_files: vec!["rust".to_owned()],
        };
        actions.apply_to(&mut settings);
        assert_eq!(ActionsOnSave::from_session(&settings), actions);
    }

    #[test]
    fn only_the_changed_lines_keep_a_formatters_result() {
        let base = "a  1\nb  2\nc  3\n";
        let old = "a  1\nb  22\nc  3\n";
        let new = "a 1\nb 22\nc 3\n";
        assert_eq!(keeping_changed(old, new, base), "a  1\nb 22\nc  3\n");
    }

    #[test]
    fn touching_counts_a_deletion_as_the_line_it_stands_at() {
        let deletion = vec![3..3, 20..21];
        let lines = vec![3..5, 20..21];
        assert!(touches(&deletion, 3, 3));
        assert!(!touches(&deletion, 4, 4));
        assert!(touches(&lines, 4, 9));
        assert!(!touches(&lines, 5, 9));
    }
}
