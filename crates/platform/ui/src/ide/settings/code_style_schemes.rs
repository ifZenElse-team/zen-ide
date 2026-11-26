use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::Ui;
use serde_json::json;
use zen_i18n::t;
use zen_project::settings::{CodeStyle, CodeStyleGeneral, SchemeFile};

use super::Dispatch;
use super::model::{CodeStyleScheme, IdeCodeStyle, IdeSettings};

const PROJECT: &str = "Project";
const DEFAULT: &str = "Default";
const EXPORT_NAME: &str = "code-style.xml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SchemeAction {
    CopyToProject,
    CopyToIde,
    Duplicate,
    Reset,
    Rename,
    Delete,
    ExportXml,
    Import,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Naming {
    pub(super) action: SchemeAction,
    pub(super) name: String,
}

#[derive(Default)]
pub(super) struct SchemePage {
    pub(super) tab: usize,
    pub(super) project: Option<SchemeFile>,
    loaded: Option<SchemeFile>,
    pub(super) naming: Option<Naming>,
    pub(super) ask_replace: bool,
    pub(super) guides: Option<String>,
    pub(super) do_not_format: Option<String>,
    pub(super) message: Option<(String, bool)>,
    #[cfg(test)]
    pub(super) picked: Option<PathBuf>,
}

pub(super) struct SchemeEdit<'a> {
    pub(super) draft: &'a mut IdeSettings,
    project: Option<&'a mut SchemeFile>,
}

impl<'a> SchemeEdit<'a> {
    pub(super) fn new(draft: &'a mut IdeSettings, project: Option<&'a mut SchemeFile>) -> Self {
        let project = project.filter(|_| draft.code_style_scheme == CodeStyleScheme::Project);
        Self { draft, project }
    }

    pub(super) fn style_for(&self, language: &str) -> CodeStyle {
        let base = self.draft.code_style_for(language);
        let style = self.project.as_ref().and_then(|project| project.styles.get(language));
        style.map_or(base, |style| style.apply_to(base))
    }

    pub(super) fn set_style(&mut self, language: &str, style: CodeStyle) {
        match &mut self.project {
            Some(project) => project.set_style(language, style),
            None => {
                self.draft.ide_scheme_mut().0.insert(language.to_owned(), style);
            }
        }
    }

    pub(super) fn general(&self) -> CodeStyleGeneral {
        let base = self.draft.ide_scheme().1;
        self.project.as_ref().map_or_else(|| base.clone(), |project| project.general_over(base))
    }

    pub(super) fn set_general(&mut self, general: CodeStyleGeneral) {
        match &mut self.project {
            Some(project) => project.set_general(&general),
            None => *self.draft.ide_scheme_mut().1 = general,
        }
    }
}

pub(super) fn guides_text(guides: &[u16]) -> String {
    guides.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
}

pub(super) fn parsed_guides(text: &str) -> Vec<u16> {
    let mut guides: Vec<u16> =
        text.split([',', ';', ' ']).filter_map(|part| part.trim().parse().ok()).filter(|column| *column > 0).collect();
    guides.sort_unstable();
    guides.dedup();
    guides
}

pub(super) fn parsed_patterns(text: &str) -> Vec<String> {
    text.split(';').map(str::trim).filter(|pattern| !pattern.is_empty()).map(str::to_owned).collect()
}

fn unique_name(draft: &IdeSettings, wanted: &str) -> String {
    let free = |name: &str| name_problem(draft, name).is_none();
    let wanted = if wanted.trim().is_empty() { t!("settings-code-style-imported-name") } else { wanted.to_owned() };
    if free(&wanted) {
        return wanted;
    }
    (2..).map(|number| format!("{wanted} {number}")).find(|name| free(name)).unwrap_or(wanted)
}

pub(super) fn name_problem(draft: &IdeSettings, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some(t!("settings-code-style-scheme-name-empty"));
    }
    (name == PROJECT || name == DEFAULT || draft.code_style_schemes.contains_key(name))
        .then(|| t!("settings-code-style-scheme-name-taken", name = name))
}

fn to_ide(scheme: &SchemeFile, draft: &IdeSettings) -> IdeCodeStyle {
    let (languages, general) = draft.ide_scheme();
    IdeCodeStyle { languages: scheme.languages_over(languages), general: scheme.general_over(general) }
}

fn from_file(scheme: &SchemeFile) -> IdeCodeStyle {
    IdeCodeStyle {
        languages: scheme.languages_over(&BTreeMap::new()),
        general: scheme.general_over(&CodeStyleGeneral::default()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_export() -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(EXPORT_NAME).add_filter("XML", &["xml"]).save_file()
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_import() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("XML", &["xml"]).pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_export() -> Option<PathBuf> {
    None
}

#[cfg(target_arch = "wasm32")]
fn pick_import() -> Option<PathBuf> {
    None
}

impl SchemePage {
    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        *self = Self { tab: self.tab, ..Self::default() };
        let Ok(answer) = dispatch("project.codeStyle.get", json!({})) else { return };
        if answer["exists"] == true {
            self.project = serde_json::from_value(answer["scheme"].clone()).ok();
            self.loaded = self.project.clone();
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.project == self.loaded {
            return;
        }
        let Some(scheme) = &self.project else { return };
        match dispatch("project.codeStyle.set", json!({ "scheme": scheme })) {
            Ok(_) => self.loaded = self.project.clone(),
            Err(error) => self.message = Some((error, true)),
        }
    }

    fn forget_texts(&mut self) {
        self.guides = None;
        self.do_not_format = None;
    }

    fn selects_project(&self, draft: &IdeSettings) -> bool {
        draft.code_style_scheme == CodeStyleScheme::Project && self.project.is_some()
    }

    fn current_file(&self, draft: &IdeSettings) -> SchemeFile {
        match self.project.as_ref().filter(|_| self.selects_project(draft)) {
            Some(project) => project.clone(),
            None => {
                let (languages, general) = draft.ide_scheme();
                SchemeFile::of(draft.selected_copy().unwrap_or(DEFAULT), languages, general)
            }
        }
    }

    pub(super) fn selected_name(&self, draft: &IdeSettings) -> String {
        match draft.code_style_scheme {
            CodeStyleScheme::Project => CodeStyleScheme::Project.name(),
            CodeStyleScheme::Default => {
                draft.selected_copy().map_or_else(|| CodeStyleScheme::Default.name(), str::to_owned)
            }
        }
    }

    pub(super) fn choose_ide(&mut self, draft: &mut IdeSettings, copy: Option<String>) {
        draft.code_style_scheme = CodeStyleScheme::Default;
        draft.code_style_copy = copy;
        self.forget_texts();
    }

    pub(super) fn choose_project(&mut self, draft: &mut IdeSettings) {
        if self.project.is_none() {
            let (languages, general) = draft.ide_scheme();
            self.project = Some(SchemeFile::of(PROJECT, languages, general));
        }
        draft.code_style_scheme = CodeStyleScheme::Project;
        draft.code_style_copy = None;
        self.forget_texts();
    }

    fn copy_to_project(&mut self, draft: &mut IdeSettings) {
        let (languages, general) = draft.ide_scheme();
        self.project = Some(SchemeFile::of(PROJECT, languages, general));
        draft.code_style_scheme = CodeStyleScheme::Project;
        draft.code_style_copy = None;
        self.ask_replace = false;
        self.forget_texts();
    }

    pub(super) fn replace_project(&mut self, draft: &mut IdeSettings) {
        self.copy_to_project(draft);
    }

    pub(super) fn menu(&self, ui: &mut Ui, draft: &IdeSettings, chosen: &mut Option<SchemeAction>) {
        let project = draft.code_style_scheme == CodeStyleScheme::Project;
        let copy = draft.selected_copy().is_some();
        let mut item = |ui: &mut Ui, key: &str, action: SchemeAction, shown: bool| {
            if shown && crate::components::menu_item(ui, t!(key)) {
                *chosen = Some(action);
                ui.close();
            }
        };
        item(ui, "settings-code-style-copy-to-project", SchemeAction::CopyToProject, !project);
        item(ui, "settings-code-style-copy-to-ide", SchemeAction::CopyToIde, project && self.project.is_some());
        item(ui, "settings-code-style-duplicate", SchemeAction::Duplicate, !project);
        item(ui, "settings-code-style-reset", SchemeAction::Reset, true);
        item(ui, "settings-code-style-rename", SchemeAction::Rename, copy);
        item(ui, "settings-code-style-delete", SchemeAction::Delete, copy);
    }

    pub(super) fn run(&mut self, draft: &mut IdeSettings, action: SchemeAction, dispatch: Option<&mut Dispatch<'_>>) {
        self.message = None;
        match action {
            SchemeAction::CopyToProject if self.project.is_some() => self.ask_replace = true,
            SchemeAction::CopyToProject => self.copy_to_project(draft),
            SchemeAction::CopyToIde | SchemeAction::Duplicate => {
                let name = unique_name(draft, &t!("settings-code-style-copy-name", name = self.selected_name(draft)));
                self.naming = Some(Naming { action, name });
            }
            SchemeAction::Rename => {
                let name = draft.selected_copy().unwrap_or_default().to_owned();
                self.naming = Some(Naming { action, name });
            }
            SchemeAction::Reset => {
                if self.selects_project(draft) {
                    self.project = Some(SchemeFile { name: PROJECT.to_owned(), ..SchemeFile::default() });
                } else {
                    let (languages, general) = draft.ide_scheme_mut();
                    languages.clear();
                    *general = CodeStyleGeneral::default();
                }
                self.forget_texts();
            }
            SchemeAction::Delete => {
                if let Some(copy) = draft.selected_copy().map(str::to_owned) {
                    draft.code_style_schemes.remove(&copy);
                }
                self.choose_ide(draft, None);
            }
            SchemeAction::ExportXml => self.export(draft, dispatch),
            SchemeAction::Import => self.import(draft, dispatch),
        }
    }

    fn picked_export(&mut self) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(path) = self.picked.take() {
            return Some(path);
        }
        pick_export()
    }

    fn picked_import(&mut self) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(path) = self.picked.take() {
            return Some(path);
        }
        pick_import()
    }

    fn export(&mut self, draft: &IdeSettings, dispatch: Option<&mut Dispatch<'_>>) {
        let (Some(dispatch), Some(path)) = (dispatch, self.picked_export()) else { return };
        let scheme = self.current_file(draft);
        let params = json!({ "path": path, "scheme": scheme, "overwrite": true });
        self.message = Some(match dispatch("project.codeStyle.export", params) {
            Ok(_) => (t!("settings-code-style-exported-xml", path = path.display().to_string()), false),
            Err(error) => (error, true),
        });
    }

    fn import(&mut self, draft: &mut IdeSettings, dispatch: Option<&mut Dispatch<'_>>) {
        let (Some(dispatch), Some(path)) = (dispatch, self.picked_import()) else { return };
        let read = dispatch("project.codeStyle.import", json!({ "path": path }))
            .and_then(|answer| serde_json::from_value::<SchemeFile>(answer).map_err(|error| error.to_string()));
        match read {
            Ok(scheme) => {
                let name = unique_name(draft, &scheme.name);
                draft.code_style_schemes.insert(name.clone(), from_file(&scheme));
                self.choose_ide(draft, Some(name.clone()));
                self.message = Some((t!("settings-code-style-imported", name = name), false));
            }
            Err(error) => self.message = Some((error, true)),
        }
    }

    pub(super) fn finish_naming(&mut self, draft: &mut IdeSettings) {
        let Some(naming) = self.naming.clone() else { return };
        let name = naming.name.trim().to_owned();
        let unchanged = naming.action == SchemeAction::Rename && draft.selected_copy() == Some(name.as_str());
        if !unchanged && let Some(problem) = name_problem(draft, &name) {
            self.message = Some((problem, true));
            return;
        }
        self.naming = None;
        self.message = None;
        let scheme = match naming.action {
            SchemeAction::Rename => {
                draft.selected_copy().map(str::to_owned).and_then(|old| draft.code_style_schemes.remove(&old))
            }
            SchemeAction::CopyToIde => self.project.as_ref().map(|project| to_ide(project, draft)),
            _ => {
                let (languages, general) = draft.ide_scheme();
                Some(IdeCodeStyle { languages: languages.clone(), general: general.clone() })
            }
        };
        if let Some(scheme) = scheme {
            draft.code_style_schemes.insert(name.clone(), scheme);
            self.choose_ide(draft, Some(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch_nothing(_: &str, _: serde_json::Value) -> Result<serde_json::Value, String> {
        Err("no".to_owned())
    }

    fn page() -> (SchemePage, IdeSettings) {
        (SchemePage::default(), IdeSettings { code_style_scheme: CodeStyleScheme::Default, ..IdeSettings::default() })
    }

    #[test]
    fn duplicate_rename_and_delete_keep_a_copy_of_the_ide_scheme() {
        let (mut page, mut draft) = page();
        draft.code_style_general.hard_wrap_at = 90;
        page.run(&mut draft, SchemeAction::Duplicate, None);
        page.naming.as_mut().unwrap().name = "Team".to_owned();
        page.finish_naming(&mut draft);
        assert_eq!(draft.selected_copy(), Some("Team"));
        assert_eq!(draft.ide_scheme().1.hard_wrap_at, 90);
        draft.ide_scheme_mut().1.hard_wrap_at = 80;
        assert_eq!(draft.code_style_general.hard_wrap_at, 90, "the copy is on its own");
        page.run(&mut draft, SchemeAction::Rename, None);
        page.naming.as_mut().unwrap().name = "Crew".to_owned();
        page.finish_naming(&mut draft);
        assert_eq!(draft.selected_copy(), Some("Crew"));
        assert_eq!(draft.code_style_schemes.len(), 1);
        page.run(&mut draft, SchemeAction::Delete, None);
        assert!(draft.code_style_schemes.is_empty());
        assert_eq!(draft.selected_copy(), None);
    }

    #[test]
    fn a_name_in_use_is_refused_and_the_naming_stays_open() {
        let (mut page, mut draft) = page();
        page.run(&mut draft, SchemeAction::Duplicate, None);
        page.naming.as_mut().unwrap().name = PROJECT.to_owned();
        page.finish_naming(&mut draft);
        assert!(page.naming.is_some());
        assert!(page.message.as_ref().is_some_and(|(_, failed)| *failed));
        assert!(draft.code_style_schemes.is_empty());
    }

    #[test]
    fn reset_brings_back_the_defaults_of_the_selected_scheme() {
        let (mut page, mut draft) = page();
        draft.code_style_general.hard_wrap_at = 90;
        draft.code_style.insert("go".to_owned(), CodeStyle::default());
        page.run(&mut draft, SchemeAction::Reset, None);
        assert_eq!(draft.code_style_general, CodeStyleGeneral::default());
        assert!(draft.code_style.is_empty());
    }

    #[test]
    fn copy_to_project_asks_before_replacing_the_projects_scheme() {
        let (mut page, mut draft) = page();
        draft.code_style_general.hard_wrap_at = 90;
        page.run(&mut draft, SchemeAction::CopyToProject, None);
        assert_eq!(draft.code_style_scheme, CodeStyleScheme::Project);
        let general = page.project.as_ref().unwrap().general_over(&CodeStyleGeneral::default());
        assert_eq!(general.hard_wrap_at, 90);
        page.choose_ide(&mut draft, None);
        page.run(&mut draft, SchemeAction::CopyToProject, None);
        assert!(page.ask_replace);
        assert_eq!(draft.code_style_scheme, CodeStyleScheme::Default);
    }

    #[test]
    fn edits_go_to_the_selected_scheme() {
        let (mut page, mut draft) = page();
        page.choose_project(&mut draft);
        let mut project = page.project.clone();
        let mut edit = SchemeEdit::new(&mut draft, project.as_mut());
        let go = CodeStyle { use_tabs: false, tab_size: 2, indent: 2 };
        edit.set_style("go", go);
        edit.set_general(CodeStyleGeneral { hard_wrap_at: 80, ..CodeStyleGeneral::default() });
        assert_eq!(edit.style_for("go"), go);
        assert_eq!(edit.general().hard_wrap_at, 80);
        assert!(draft.code_style.is_empty(), "the IDE's stays");
        assert_eq!(draft.code_style_general.hard_wrap_at, 120);
        draft.code_style_scheme = CodeStyleScheme::Default;
        let mut edit = SchemeEdit::new(&mut draft, project.as_mut());
        edit.set_style("go", go);
        assert_eq!(draft.code_style["go"], go);
    }

    #[test]
    fn a_project_without_a_scheme_gets_none_until_the_user_chooses_it() {
        let mut draft = IdeSettings::default();
        let mut edit = SchemeEdit::new(&mut draft, None);
        let go = CodeStyle { use_tabs: false, tab_size: 2, indent: 2 };
        edit.set_style("go", go);
        assert_eq!(draft.code_style["go"], go);
    }

    #[test]
    fn the_text_fields_are_read_leniently() {
        assert_eq!(parsed_guides("100, 80;x 80 0"), vec![80, 100]);
        assert_eq!(guides_text(&[80, 100]), "80, 100");
        assert_eq!(parsed_patterns(" *.min.js; ;gen/*"), vec!["*.min.js", "gen/*"]);
    }

    #[test]
    fn failed_commands_leave_the_scheme_alone() {
        let (mut page, mut draft) = page();
        page.picked = Some(PathBuf::from("/nowhere.xml"));
        let mut dispatch = dispatch_nothing;
        page.run(&mut draft, SchemeAction::Import, Some(&mut dispatch as &mut Dispatch<'_>));
        assert!(draft.code_style_schemes.is_empty());
        assert!(page.message.as_ref().is_some_and(|(_, failed)| *failed));
    }
}
