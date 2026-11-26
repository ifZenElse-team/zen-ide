use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use zen_core::{Command, CommandError, NoParams, RegistryError, Session};
use zen_i18n::t;
use zen_modules::folding::{FoldOption, FoldingOptions};
use zen_modules::form::{Action, Choice, Form, Item, MenuEntry, Target, Tone};
use zen_modules::{
    CodeStylePage, DebuggerOffer, FileTypeDeclaration, FileWatcherTemplates, LanguageToolchain, ModuleDescriptor,
    ModuleHost, SettingsPage, TemplateSet,
};

use crate::deps::assist::{GoModAssist, ModuleVersions};
use crate::new_project::GoProject;
use crate::toolchain::{GoInfo, Install, Toolchain, go};
use crate::{
    DOC_COMMENTS, GoHighlighter, GoModHighlighter, GoModInspector, GoModules, GoOutliner, GoRun, GoSumHighlighter,
    LANGUAGE_SERVERS, LOCALES, POSTFIX, STATEMENTS, SURROUNDERS, TEMPLATES, add_import,
};

pub static MODULE: ModuleDescriptor = ModuleDescriptor {
    id: "go",
    name: "module-go-name",
    description: "module-go-description",
    locales: LOCALES,
    register,
};

pub const FILE_TYPES: [FileTypeDeclaration; 4] = [
    FileTypeDeclaration { id: "go", name: "file-type-go", patterns: &["*.go"], hashbangs: &[] },
    FileTypeDeclaration { id: "go.mod", name: "file-type-go-module", patterns: &["go.mod", "go.work"], hashbangs: &[] },
    FileTypeDeclaration {
        id: "go.sum",
        name: "file-type-go-sum",
        patterns: &["go.sum", "go.work.sum"],
        hashbangs: &[],
    },
    FileTypeDeclaration {
        id: "go.template",
        name: "file-type-go-template",
        patterns: &["*.tmpl", "*.gotmpl", "*.gohtml"],
        hashbangs: &[],
    },
];

pub use crate::incremental::fold_options::{
    EMPTY_FUNCTIONS, EMPTY_TYPES, FORMATTED_STRINGS, IF_ERRORS, ONE_LINE_CASES, ONE_LINE_RETURNS,
};

pub const FOLDING: FoldingOptions = FoldingOptions {
    title: "settings-go-folding",
    options: &[
        FoldOption { key: IF_ERRORS, label: "settings-go-fold-if-errors", default: false },
        FoldOption { key: ONE_LINE_RETURNS, label: "settings-go-fold-one-line-returns", default: false },
        FoldOption { key: ONE_LINE_CASES, label: "settings-go-fold-one-line-cases", default: false },
        FoldOption { key: EMPTY_FUNCTIONS, label: "settings-go-fold-empty-functions", default: false },
        FoldOption { key: EMPTY_TYPES, label: "settings-go-fold-empty-types", default: false },
        FoldOption { key: FORMATTED_STRINGS, label: "settings-go-fold-formatted-strings", default: false },
    ],
};

pub const SETTINGS_PAGE: &str = "go";

pub const FILE_WATCHERS: &str = include_str!("file-watchers.json");

const SEARCH: &[&str] = &[
    "settings-go-goroot",
    "settings-go-add-sdk",
    "settings-go-add-local",
    "settings-go-remove-sdk",
    "settings-go-gopls-path",
    "settings-go-gopls-update",
    "settings-go-refresh",
    "settings-go-note",
    "settings-go-delve",
    "settings-go-dlv-path",
    "settings-go-dlv-install",
    "settings-go-dlv-update",
    "settings-go-dlv-note",
    "settings-go-on-save",
    "settings-go-reformat-on-save",
    "settings-go-optimize-imports-on-save",
    "settings-go-configure-on-save",
    "settings-go-staticcheck",
    "settings-go-staticcheck-note",
];

const ENV_SEARCH: &[&str] = &[
    "settings-go-env",
    "settings-go-env-shared",
    "settings-go-env-personal",
    "settings-go-env-note",
    "settings-go-env-effective",
];

pub fn register(host: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
    let toolchain = Toolchain::load(host.data_dir().map(|data| data.join("toolchain.json")));
    crate::toolchain::register(host.commands())?;
    host.commands().register(GoSettings)?;
    crate::project_env::commands::register(host.commands())?;
    host.commands().register(ModuleVersions)?;
    for file_type in FILE_TYPES {
        host.file_type(file_type);
    }
    host.highlighter(Arc::new(GoHighlighter));
    host.highlighter(Arc::new(GoModHighlighter));
    host.highlighter(Arc::new(GoSumHighlighter));
    host.highlighter(Arc::new(crate::GoTemplateHighlighter));
    host.file_references(Arc::new(crate::GoEmbedFiles));
    host.project_model(Arc::new(GoModules));
    host.outliner(Arc::new(GoOutliner));
    host.code_generator(Arc::new(crate::GoGenerators::default()));
    host.language_assist(Arc::new(crate::tags::GoTagAssist));
    host.run_provider(Arc::new(GoRun));
    host.test_support(Arc::new(crate::GoTests));
    let inspector = GoModInspector::default();
    host.inspector(Arc::new(inspector.clone()));
    let locations = toolchain.clone();
    let module_cache = Arc::new(move || locations.go_locations().module_cache);
    host.language_assist(Arc::new(GoModAssist::new(inspector.clone(), module_cache)));
    host.window_state(Arc::new(move |session: &mut Session| session.set_state(inspector.clone())));
    host.language_servers(zen_lsp::ServerDeclaration::parse_list(LANGUAGE_SERVERS).unwrap_or_default());
    host.templates(TemplateSet {
        language: "go",
        templates: TEMPLATES,
        add_import: Some(add_import),
        surrounders: SURROUNDERS,
        postfix: POSTFIX,
        doc_comments: DOC_COMMENTS,
        statements: STATEMENTS,
    });
    host.file_templates(crate::FILE_TEMPLATES);
    host.toolchain(Arc::new(toolchain.clone()));
    let toolchain_for_projects = toolchain.clone();
    let located = toolchain.clone();
    host.window_state(Arc::new(move |session: &mut Session| session.set_state(toolchain.clone())));
    host.window_state(Arc::new(|session: &mut Session| session.set_state(zen_modules::search::TypeNoun::Type)));
    host.debugger(DebuggerOffer {
        title: "ide-debugger-missing-title",
        detail: "ide-debugger-missing-detail",
        install: "ide-debugger-install",
        installing: "ide-debugger-installing",
        command: "toolchain.dlv.install",
    });
    host.settings_page(SettingsPage {
        id: SETTINGS_PAGE,
        title: "settings-go",
        search: SEARCH,
        form: GoSettings::ID,
        commands: &["toolchain.go.", "toolchain.gopls.choose", "toolchain.dlv.choose", "go.staticcheck.install"],
        parent: "",
    });
    host.settings_page(SettingsPage {
        id: crate::project_env::commands::SETTINGS_PAGE,
        title: "settings-go-modules",
        search: ENV_SEARCH,
        form: crate::project_env::commands::GoEnvSettings::ID,
        commands: &["go.env.set"],
        parent: SETTINGS_PAGE,
    });
    host.code_style(CodeStylePage { language: "go", title: "settings-section-code-style-go" });
    if let Ok(declaration) = zen_project::code_style::CodeStyleDeclaration::parse(crate::CODE_STYLE) {
        host.code_style_options(declaration);
    }
    host.arranger(Arc::new(crate::GoArranger));
    let vet = crate::GoVetInspector::new(Arc::new(move || located.staticcheck()));
    host.inspector(Arc::new(vet.clone()));
    host.window_state(Arc::new(move |session: &mut Session| session.set_state(vet.clone())));
    host.commands().register(crate::RunInspections)?;
    host.commands().register(crate::InspectionsStatus)?;
    host.commands().register(crate::InstallStaticcheck)?;
    host.folding_options(FOLDING);
    host.file_watcher_templates(FileWatcherTemplates { origin: "go", templates: FILE_WATCHERS });
    let sdks = toolchain_for_projects;
    host.project_generator(Arc::new(GoProject::new(Arc::new(move || {
        let info = sdks.go();
        crate::new_project::FoundSdks { sdks: info.sdks, chosen: info.sdk }
    }))));
    host.notification_group("toolchain.dlv.", "debugger");
    host.notification_group("toolchain.gopls.", "languageServers");
    Ok(())
}

impl LanguageToolchain for Toolchain {
    fn generation(&self) -> u64 {
        self.check_soon();
        Toolchain::generation(self)
    }

    fn model_env(&self) -> zen_project::model::ModelEnv {
        model_env(self.go_locations())
    }

    fn run_env(&self) -> zen_run::RunEnv {
        self.go_run_env().unwrap_or_default()
    }

    fn debugger(&self) -> Option<PathBuf> {
        self.go_debugger()
    }

    fn languages(&self) -> Vec<String> {
        vec!["go".to_owned()]
    }

    fn serves(&self, server: &zen_lsp::ServerDeclaration) -> bool {
        server.languages.iter().any(|language| language == "go")
            || server.install.as_ref().is_some_and(|recipe| recipe.kind == zen_lsp::InstallKind::Go)
    }
}

pub fn model_env(locations: crate::toolchain::GoLocations) -> zen_project::model::ModelEnv {
    zen_project::model::ModelEnv {
        sdk_version: locations
            .sdk
            .as_ref()
            .and_then(|sdk| sdk.version.as_deref())
            .map(|version| version.strip_prefix("go").unwrap_or(version).to_owned()),
        sdk_root: locations.sdk.map(|sdk| sdk.root),
        package_cache: locations.module_cache,
    }
}

pub struct GoSettings;

impl Command for GoSettings {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "toolchain.go.settings";
    const TITLE: &'static str = "Go Settings Page";
    const DESCRIPTION: &'static str = "Settings → Languages & Frameworks → Go as the page shows it: GOROOT, gopls, \
         Delve and the actions on save, each with the command (and parameter) that changes it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let toolchain = session.state::<Toolchain>().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))?;
        toolchain.check_soon();
        let mut form = settings_form(&toolchain.go());
        crate::vet::add_settings(session, &mut form);
        Ok(form)
    }
}

pub fn settings_form(info: &GoInfo) -> Form {
    let mut items = Vec::new();
    goroot(info, &mut items);
    let gopls = tool(
        info,
        &mut items,
        &Tool {
            title: "gopls".to_owned(),
            found: info.gopls.as_ref().map(|gopls| (gopls.version.clone(), gopls.path.clone(), false)),
            install: &info.install,
            saved: info.gopls_path.as_ref(),
            installing: "ide-gopls-installing",
            failed: "ide-gopls-failed",
            missing: "settings-go-gopls-missing",
            path: "settings-go-gopls-path",
            path_note: "settings-go-gopls-path-note",
            choose: "toolchain.gopls.choose",
            install_command: "toolchain.gopls.install",
            install_label: "ide-gopls-install",
            update_label: "settings-go-gopls-update",
            refresh: true,
            note: "settings-go-note",
        },
    );
    let delve = tool(
        info,
        &mut items,
        &Tool {
            title: t!("settings-go-delve"),
            found: info.dlv.as_ref().map(|dlv| (dlv.version.clone(), dlv.path.clone(), dlv.outdated)),
            install: &info.dlv_install,
            saved: info.dlv_path.as_ref(),
            installing: "settings-go-dlv-installing",
            failed: "settings-go-dlv-failed",
            missing: "settings-go-dlv-missing",
            path: "settings-go-dlv-path",
            path_note: "settings-go-dlv-path-note",
            choose: "toolchain.dlv.choose",
            install_command: "toolchain.dlv.install",
            install_label: "settings-go-dlv-install",
            update_label: "settings-go-dlv-update",
            refresh: false,
            note: "settings-go-dlv-note",
        },
    );
    items.push(Item::Group { title: t!("settings-go-on-save") });
    items.push(Item::Setting {
        key: "reformatOnSave".to_owned(),
        label: t!("settings-go-reformat-on-save"),
        default: false,
    });
    items.push(Item::Setting {
        key: "optimizeImportsOnSave".to_owned(),
        label: t!("settings-go-optimize-imports-on-save"),
        default: false,
    });
    items.push(Item::Link {
        label: t!("settings-go-configure-on-save"),
        target: Target::Page { page: "actionsOnSave".to_owned() },
    });
    Form { items, busy: gopls || delve }
}

fn goroot(info: &GoInfo, items: &mut Vec<Item>) {
    let describe = |sdk: &go::GoSdk| format!("{}   {}", sdk.version.as_deref().unwrap_or("?"), sdk.root.display());
    items.push(Item::Group { title: t!("settings-go-goroot") });
    let current = info.sdk.as_ref().map(|sdk| sdk.root.display().to_string());
    if info.sdks.is_empty() {
        items.push(Item::Text { text: t!("ide-go-sdk-missing"), tone: Tone::Secondary, spinner: false });
        items.push(Item::Link {
            label: t!("ide-go-download"),
            target: Target::Url { url: "https://go.dev/dl/".to_owned() },
        });
    }
    let mut row = Vec::new();
    if !info.sdks.is_empty() {
        row.push(Item::Choice {
            label: t!("settings-go-goroot"),
            options: info
                .sdks
                .iter()
                .map(|sdk| Choice { value: sdk.root.display().to_string(), label: describe(sdk) })
                .collect(),
            selected: current.clone(),
            action: Action::with_value("toolchain.go.choose", "root"),
        });
    }
    row.push(Item::Menu {
        label: t!("settings-go-add-sdk"),
        entries: vec![MenuEntry {
            label: t!("settings-go-add-local"),
            action: Action::with_value("toolchain.go.add", "root").picking_folder(&t!("settings-go-pick-sdk")),
        }],
    });
    if let Some(current) =
        current.filter(|current| info.custom_sdks.iter().any(|custom| custom.display().to_string() == *current))
    {
        row.push(Item::Button {
            label: t!("settings-go-remove-sdk"),
            enabled: true,
            action: Action::command("toolchain.go.remove").param("root", json!(current)),
        });
    }
    items.push(Item::Row { items: row });
    if let Some(missing) = &info.missing_sdk {
        items.push(Item::Text {
            text: t!("settings-go-sdk-gone", root = missing.display().to_string()),
            tone: Tone::Error,
            spinner: false,
        });
        if let Some(sdk) = &info.sdk {
            items.push(Item::Row {
                items: vec![Item::Button {
                    label: t!("settings-go-sdk-use", sdk = describe(sdk)),
                    enabled: true,
                    action: Action::command("toolchain.go.choose").param("root", json!(sdk.root)),
                }],
            });
        }
    }
}

struct Tool<'a> {
    title: String,
    found: Option<(Option<String>, PathBuf, bool)>,
    install: &'a Install,
    saved: Option<&'a PathBuf>,
    installing: &'static str,
    failed: &'static str,
    missing: &'static str,
    path: &'static str,
    path_note: &'static str,
    choose: &'static str,
    install_command: &'static str,
    install_label: &'static str,
    update_label: &'static str,
    refresh: bool,
    note: &'static str,
}

fn tool(info: &GoInfo, items: &mut Vec<Item>, tool: &Tool<'_>) -> bool {
    items.push(Item::Group { title: tool.title.clone() });
    if let Install::Running { line } = tool.install {
        let detail = line.clone().unwrap_or_default();
        items.push(Item::Text { text: t!(tool.installing, detail = detail), tone: Tone::Secondary, spinner: true });
        return true;
    }
    match &tool.found {
        Some((version, path, outdated)) => {
            let text = format!("{}   {}", version.as_deref().unwrap_or("?"), path.display());
            items.push(Item::Text { text, tone: Tone::Normal, spinner: false });
            if *outdated {
                let (major, minor) = go::MIN_DLV;
                items.push(Item::Text {
                    text: t!("settings-go-dlv-outdated", minimum = format!("{major}.{minor}")),
                    tone: Tone::Error,
                    spinner: false,
                });
            }
        }
        None => items.push(Item::Text { text: t!(tool.missing), tone: Tone::Secondary, spinner: false }),
    }
    if let Install::Failed { message } = tool.install {
        items.push(Item::Text { text: t!(tool.failed, detail = message.as_str()), tone: Tone::Error, spinner: false });
    }
    items.push(Item::Path {
        label: t!(tool.path),
        value: tool.saved.map(|path| path.display().to_string()),
        action: Action::with_value(tool.choose, "path"),
    });
    items.push(Item::Text { text: t!(tool.path_note), tone: Tone::Secondary, spinner: false });
    let label = if tool.found.is_some() { t!(tool.update_label) } else { t!(tool.install_label) };
    let mut row =
        vec![Item::Button { label, enabled: info.sdk.is_some(), action: Action::command(tool.install_command) }];
    if tool.refresh {
        row.push(Item::Button {
            label: t!("settings-go-refresh"),
            enabled: true,
            action: Action::command("toolchain.go.refresh"),
        });
    }
    items.push(Item::Row { items: row });
    items.push(Item::Text { text: t!(tool.note), tone: Tone::Secondary, spinner: false });
    false
}

#[cfg(test)]
mod tests {
    use zen_core::Registry;

    use super::*;
    use crate::toolchain::{Gopls, Tool as Found};

    fn info() -> GoInfo {
        let sdk = go::GoSdk { root: PathBuf::from("/sdk/go"), version: Some("go1.27.2".to_owned()) };
        GoInfo {
            sdks: vec![sdk.clone()],
            sdk: Some(sdk),
            missing_sdk: None,
            gopls: Some(Gopls { path: PathBuf::from("/bin/gopls"), version: Some("v0.18.1".to_owned()) }),
            install: Install::Idle,
            custom_sdks: Vec::new(),
            gopls_path: None,
            dlv: Some(Found {
                path: PathBuf::from("/go/bin/dlv"),
                version: Some("1.23.1".to_owned()),
                outdated: false,
            }),
            dlv_install: Install::Idle,
            dlv_path: None,
        }
    }

    fn buttons(form: &Form) -> Vec<(String, String, bool)> {
        fn walk(items: &[Item], found: &mut Vec<(String, String, bool)>) {
            for item in items {
                match item {
                    Item::Button { label, enabled, action } => {
                        found.push((label.clone(), action.command.clone(), *enabled))
                    }
                    Item::Row { items } => walk(items, found),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        walk(&form.items, &mut found);
        found
    }

    #[test]
    fn the_go_page_shows_the_toolchain_with_the_commands_that_change_it() {
        zen_i18n::register(LOCALES);
        let form = settings_form(&info());
        let texts = form.texts();
        for text in [
            t!("settings-go-goroot"),
            "gopls".to_owned(),
            "v0.18.1   /bin/gopls".to_owned(),
            t!("settings-go-delve"),
            "1.23.1   /go/bin/dlv".to_owned(),
            t!("settings-go-on-save"),
            t!("settings-go-reformat-on-save"),
            t!("settings-go-configure-on-save"),
        ] {
            assert!(texts.contains(&text.as_str()), "no {text:?} in {texts:?}");
        }
        assert!(!form.busy, "nothing installs by looking");
        assert_eq!(
            buttons(&form),
            [
                (t!("settings-go-gopls-update"), "toolchain.gopls.install".to_owned(), true),
                (t!("settings-go-refresh"), "toolchain.go.refresh".to_owned(), true),
                (t!("settings-go-dlv-update"), "toolchain.dlv.install".to_owned(), true),
            ]
        );
        let Item::Row { items } = &form.items[1] else { panic!("{:?}", form.items[1]) };
        let Item::Choice { selected, action, .. } = &items[0] else { panic!("{:?}", items[0]) };
        assert_eq!(selected.as_deref(), Some("/sdk/go"));
        assert_eq!(action.params_with(Some(json!("/sdk/other"))), json!({ "root": "/sdk/other" }));
        assert!(items.iter().all(|item| !matches!(item, Item::Button { .. })), "an SDK found cannot be removed");
    }

    #[test]
    fn the_go_page_follows_installations_old_delve_and_a_missing_sdk() {
        zen_i18n::register(LOCALES);
        let mut info = info();
        info.dlv =
            Some(Found { path: PathBuf::from("/go/bin/dlv"), version: Some("1.19.0".to_owned()), outdated: true });
        info.install = Install::Running { line: Some("go: downloading".to_owned()) };
        let form = settings_form(&info);
        assert!(form.busy);
        let texts = form.texts();
        assert!(texts.contains(&t!("ide-gopls-installing", detail = "go: downloading").as_str()), "{texts:?}");
        assert!(texts.contains(&t!("settings-go-dlv-outdated", minimum = "1.20").as_str()), "{texts:?}");
        let paths = form.items.iter().filter(|item| matches!(item, Item::Path { .. })).count();
        assert_eq!(paths, 1, "while gopls installs, only Delve's path can be changed");

        info.install = Install::Idle;
        info.dlv = None;
        info.custom_sdks = vec![PathBuf::from("/sdk/go")];
        let form = settings_form(&info);
        assert!(form.texts().contains(&t!("settings-go-dlv-missing").as_str()));
        assert!(buttons(&form).contains(&(t!("settings-go-dlv-install"), "toolchain.dlv.install".to_owned(), true)));
        assert!(buttons(&form).contains(&(t!("settings-go-remove-sdk"), "toolchain.go.remove".to_owned(), true)));

        info.sdks.clear();
        info.sdk = None;
        let form = settings_form(&info);
        assert!(form.texts().contains(&t!("ide-go-sdk-missing").as_str()));
        assert!(
            buttons(&form).iter().any(|(_, command, enabled)| command == "toolchain.dlv.install" && !enabled),
            "no SDK, no go install"
        );
    }

    #[test]
    fn a_chosen_sdk_that_is_gone_is_shown_with_a_switch_to_the_one_in_use() {
        zen_i18n::register(LOCALES);
        let mut info = info();
        let form = settings_form(&info);
        assert!(!buttons(&form).iter().any(|(_, command, _)| command == "toolchain.go.choose"), "nothing is missing");

        let gone = "/opt/homebrew/Cellar/go/1.27.2/libexec";
        info.missing_sdk = Some(PathBuf::from(gone));
        info.sdk = Some(go::GoSdk {
            root: PathBuf::from("/opt/homebrew/opt/go/libexec"),
            version: Some("go1.27.3".to_owned()),
        });
        let form = settings_form(&info);
        assert!(form.texts().contains(&t!("settings-go-sdk-gone", root = gone).as_str()), "{:?}", form.texts());
        let label = t!("settings-go-sdk-use", sdk = "go1.27.3   /opt/homebrew/opt/go/libexec");
        assert!(buttons(&form).contains(&(label, "toolchain.go.choose".to_owned(), true)), "{:?}", buttons(&form));
        let action = form
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Row { items } => items.iter().find_map(|item| match item {
                    Item::Button { action, .. } if action.command == "toolchain.go.choose" => Some(action.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .next()
            .unwrap();
        assert_eq!(action.params_with(None), json!({ "root": "/opt/homebrew/opt/go/libexec" }));
    }

    #[test]
    fn the_entry_point_registers_everything_go() {
        zen_i18n::register(LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, Some(data.path()));
        host.run(&MODULE).unwrap();
        let given = host.finish();
        assert_eq!(given.highlighters.len(), 4);
        assert_eq!((given.project_models.len(), given.outliners.len(), given.run_providers.len()), (1, 1, 1));
        assert_eq!(given.inspectors.len(), 2);
        assert_eq!(given.language_servers.iter().map(|server| server.id.as_str()).collect::<Vec<_>>(), ["gopls"]);
        assert_eq!(given.templates[0].language, "go");
        assert_eq!(given.settings_pages[0].id, "go");
        assert_eq!(given.code_styles[0].language, "go");
        assert_eq!(given.code_style_declarations[0].language, "go", "the declaration parses");
        assert_eq!(given.arrangers[0].language(), "go");
        assert!(!given.language_assists.is_empty());
        let toolchain = given.toolchain.expect("the Go toolchain");
        assert_eq!(toolchain.languages(), ["go"]);
        assert_eq!(given.debugger.unwrap().command, "toolchain.dlv.install");
        for id in ["toolchain.go.info", "toolchain.go.settings", "toolchain.dlv.install", "toolchain.gopls.choose"] {
            assert!(registry.info(id).is_some(), "{id} is registered");
        }
        let mut session = Session::new();
        for state in &given.window_states {
            state(&mut session);
        }
        let form = registry.dispatch(&mut session, "toolchain.go.settings", json!({})).unwrap();
        assert_eq!(form["items"][0]["kind"], "group");
    }

    #[test]
    fn every_text_of_the_go_code_style_is_written_in_both_languages() {
        let declaration = zen_project::code_style::CodeStyleDeclaration::parse(crate::CODE_STYLE).unwrap();
        for key in declaration.message_ids() {
            for language in ["en", "ru"] {
                let named = LOCALES.iter().filter(|(code, _)| *code == language).any(|(_, source)| {
                    source.starts_with(&format!("{key} = ")) || source.contains(&format!("\n{key} = "))
                });
                assert!(named, "{key} has no text in {language}");
            }
        }
    }

    #[test]
    fn every_go_command_is_named_in_both_languages() {
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, None);
        host.run(&MODULE).unwrap();
        drop(host.finish());
        let mut count = 0;
        for info in registry.commands() {
            let key = format!("command-{}", info.id.as_str().replace('.', "-"));
            for language in ["en", "ru"] {
                let named = LOCALES.iter().filter(|(code, _)| *code == language).any(|(_, source)| {
                    source.starts_with(&format!("{key} = ")) || source.contains(&format!("\n{key} = "))
                });
                assert!(named, "{key} has no name in {language} for Find Action");
            }
            count += 1;
        }
        assert!(count >= 10, "the toolchain's nine commands and the settings page at least");
    }

    #[test]
    fn every_go_command_parameter_has_a_one_line_description() {
        fn descriptions<'a>(schema: &'a serde_json::Value, found: &mut Vec<&'a str>) {
            match schema {
                serde_json::Value::Object(fields) => {
                    for (key, value) in fields {
                        match value {
                            serde_json::Value::String(text) if key == "description" => found.push(text),
                            _ => descriptions(value, found),
                        }
                    }
                }
                serde_json::Value::Array(items) => items.iter().for_each(|item| descriptions(item, found)),
                _ => {}
            }
        }
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, None);
        host.run(&MODULE).unwrap();
        drop(host.finish());
        let mut found = Vec::new();
        for info in registry.commands() {
            descriptions(&info.params_schema, &mut found);
        }
        assert!(!found.is_empty());
        for text in found {
            assert!(!text.contains(['\n', '\r', '\\']), "{text:?}");
            assert_eq!(text, text.trim());
        }
    }

    #[test]
    fn the_module_offers_the_go_file_watcher_templates() {
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, None);
        host.run(&MODULE).unwrap();
        let given = host.finish();
        assert_eq!(given.file_watcher_templates, [FileWatcherTemplates { origin: "go", templates: FILE_WATCHERS }]);
        let templates: Vec<serde_json::Value> = serde_json::from_str(FILE_WATCHERS).unwrap();
        let names: Vec<&str> = templates.iter().map(|template| template["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["go fmt", "goimports", "golangci-lint"]);
        assert!(
            templates.iter().all(|template| template["fileType"] == "go" && template["triggerOnExternal"] == false)
        );
    }

    #[test]
    fn the_model_env_has_the_sdk_version_without_go() {
        let sdk = go::GoSdk { root: PathBuf::from("/sdk/go"), version: Some("go1.22.3".to_owned()) };
        let locations = crate::toolchain::GoLocations { sdk: Some(sdk), module_cache: Some(PathBuf::from("/cache")) };
        let env = model_env(locations);
        assert_eq!(env.sdk_version.as_deref(), Some("1.22.3"));
        assert_eq!(env.sdk_root, Some(PathBuf::from("/sdk/go")));
        assert_eq!(env.package_cache, Some(PathBuf::from("/cache")));
        let unversioned = go::GoSdk { root: PathBuf::from("/sdk/dev"), version: None };
        let env = model_env(crate::toolchain::GoLocations { sdk: Some(unversioned), module_cache: None });
        assert_eq!(env.sdk_version, None);
        assert_eq!(model_env(crate::toolchain::GoLocations::default()), zen_project::model::ModelEnv::default());
    }
}
