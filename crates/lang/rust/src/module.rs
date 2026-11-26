use std::path::PathBuf;
use std::sync::Arc;

use zen_core::{Command, CommandError, NoParams, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Choice, Form, Item, MenuEntry, Target, Tone};
use zen_modules::{
    CodeStylePage, FileTypeDeclaration, LanguageToolchain, ModuleDescriptor, ModuleHost, SettingsPage, TemplateSet,
};

use crate::toolchain::{Install, RustInfo, Toolchain};
use crate::{CargoModels, CargoRun, DOC_COMMENTS, LANGUAGE_SERVERS, LOCALES, POSTFIX, RustHighlighter, STATEMENTS};

pub static MODULE: ModuleDescriptor = ModuleDescriptor {
    id: "rust",
    name: "module-rust-name",
    description: "module-rust-description",
    locales: LOCALES,
    register,
};

pub const FILE_TYPE: FileTypeDeclaration =
    FileTypeDeclaration { id: "rust", name: "file-type-rust", patterns: &["*.rs"], hashbangs: &[] };

pub const SETTINGS_PAGE: &str = "rust";

const SEARCH: &[&str] = &[
    "settings-rust-toolchain",
    "settings-rust-add-toolchain",
    "settings-rust-stdlib",
    "settings-rust-stdlib-download",
    "settings-rust-analyzer-path",
    "settings-rust-analyzer-update",
    "settings-rust-refresh",
    "settings-rust-analyzer-note",
    "settings-rust-on-save",
    "settings-rust-reformat-on-save",
    "settings-rust-ra-performance",
    "settings-rust-ra-check-on-save",
    "settings-rust-ra-check-command",
    "settings-rust-ra-check-scope",
    "settings-rust-ra-target-dir",
    "settings-rust-ra-build-scripts",
    "settings-rust-ra-proc-macros",
    "settings-rust-ra-cache-priming",
    "settings-rust-ra-memory",
];

type Options = &'static [(&'static str, Option<&'static str>)];

const TUNED: &[(&str, &str, Options, &str)] = &[
    ("checkOnSave", "settings-rust-ra-check-on-save", ON_OFF, "true"),
    ("check.command", "settings-rust-ra-check-command", &[("check", None), ("clippy", None)], "check"),
    (
        "check.workspace",
        "settings-rust-ra-check-scope",
        &[("false", Some("settings-rust-ra-check-package")), ("true", Some("settings-rust-ra-check-workspace"))],
        "true",
    ),
    (
        "cargo.targetDir",
        "settings-rust-ra-target-dir",
        &[("true", Some("settings-rust-ra-target-own")), ("false", Some("settings-rust-ra-target-shared"))],
        "false",
    ),
    (
        "cargo.buildScripts.enable",
        "settings-rust-ra-build-scripts",
        &[("true", Some("settings-rust-ra-run")), ("false", Some("settings-rust-ra-dont-run"))],
        "true",
    ),
    (
        "procMacro.enable",
        "settings-rust-ra-proc-macros",
        &[("true", Some("settings-rust-ra-expand")), ("false", Some("settings-rust-ra-dont-expand"))],
        "true",
    ),
    ("cachePriming.enable", "settings-rust-ra-cache-priming", ON_OFF, "true"),
    ("lru.capacity", "settings-rust-ra-memory", &[("128", None), ("256", None), ("512", None), ("1024", None)], "128"),
];

const ON_OFF: Options = &[("true", Some("settings-rust-ra-on")), ("false", Some("settings-rust-ra-off"))];

pub fn register(host: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
    let toolchain = Toolchain::load(host.data_dir().map(|data| data.join("toolchain-rust.json")));
    crate::toolchain::register(host.commands())?;
    host.commands().register(RustSettings)?;
    host.file_type(FILE_TYPE);
    host.excluded_folder(zen_modules::ExcludedFolderDeclaration { name: "target", beside: "Cargo.toml" });
    host.highlighter(Arc::new(RustHighlighter));
    let models = Arc::new(CargoModels::new(toolchain.clone()));
    host.project_model(models.clone());
    host.run_provider(Arc::new(CargoRun::new(toolchain.clone(), models)));
    host.test_support(Arc::new(crate::RustTests));
    host.language_servers(zen_lsp::ServerDeclaration::parse_list(LANGUAGE_SERVERS).unwrap_or_default());
    host.templates(TemplateSet {
        language: "rust",
        templates: "[]",
        add_import: None,
        surrounders: "[]",
        postfix: POSTFIX,
        doc_comments: DOC_COMMENTS,
        statements: STATEMENTS,
    });
    host.toolchain(Arc::new(toolchain.clone()));
    host.window_state(Arc::new(move |session: &mut Session| session.set_state(toolchain.clone())));
    host.settings_page(SettingsPage {
        id: SETTINGS_PAGE,
        title: "settings-rust",
        search: SEARCH,
        form: RustSettings::ID,
        commands: &["toolchain.rust.", "toolchain.rustAnalyzer.", "toolchain.rustSrc."],
        parent: "",
    });
    host.code_style(CodeStylePage { language: "rust", title: "settings-section-code-style-rust" });
    if let Ok(declaration) = zen_project::code_style::CodeStyleDeclaration::parse(crate::CODE_STYLE) {
        host.code_style_options(declaration);
    }
    host.notification_group("toolchain.rustAnalyzer.", "languageServers");
    Ok(())
}

impl LanguageToolchain for Toolchain {
    fn generation(&self) -> u64 {
        Toolchain::generation(self)
    }

    fn model_env(&self) -> zen_project::model::ModelEnv {
        zen_project::model::ModelEnv::default()
    }

    fn run_env(&self) -> zen_run::RunEnv {
        zen_run::RunEnv::default()
    }

    fn debugger(&self) -> Option<PathBuf> {
        None
    }

    fn languages(&self) -> Vec<String> {
        vec!["rust".to_owned()]
    }

    fn serves(&self, server: &zen_lsp::ServerDeclaration) -> bool {
        server.languages.iter().any(|language| language == "rust")
            || server
                .install
                .as_ref()
                .is_some_and(|recipe| matches!(recipe.kind, zen_lsp::InstallKind::Rustup | zen_lsp::InstallKind::Cargo))
    }
}

pub struct RustSettings;

impl Command for RustSettings {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "toolchain.rust.settings";
    const TITLE: &'static str = "Rust Settings Page";
    const DESCRIPTION: &'static str = "Settings → Languages & Frameworks → Rust as the page shows it: the toolchain, the \
         standard library's sources, rust-analyzer and the actions on save, each with the command (and parameter) \
         that changes it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let toolchain =
            session.state::<Toolchain>().ok_or_else(|| CommandError::new(t!("rust-toolchain-error-none")))?;
        Ok(settings_form(&toolchain.rust()))
    }
}

pub fn settings_form(info: &RustInfo) -> Form {
    let mut items = Vec::new();
    toolchain(info, &mut items);
    let stdlib = stdlib(info, &mut items);
    let analyzer = rust_analyzer(info, &mut items);
    performance(info, &mut items);
    items.push(Item::Group { title: t!("settings-rust-on-save") });
    items.push(Item::Setting {
        key: "reformatOnSave".to_owned(),
        label: t!("settings-rust-reformat-on-save"),
        default: false,
    });
    items.push(Item::Link {
        label: t!("settings-rust-configure-on-save"),
        target: Target::Page { page: "actionsOnSave".to_owned() },
    });
    Form { items, busy: stdlib || analyzer }
}

fn toolchain(info: &RustInfo, items: &mut Vec<Item>) {
    items.push(Item::Group { title: t!("settings-rust-toolchain") });
    let describe = |found: &crate::toolchain::RustToolchain| {
        format!("{}   {}", found.version.as_deref().unwrap_or("?"), found.bin.display())
    };
    if info.toolchains.is_empty() {
        items.push(Item::Text { text: t!("settings-rust-toolchain-missing"), tone: Tone::Secondary, spinner: false });
        items.push(Item::Link {
            label: t!("settings-rust-download"),
            target: Target::Url { url: "https://rustup.rs/".to_owned() },
        });
    }
    let mut row = Vec::new();
    if !info.toolchains.is_empty() {
        row.push(Item::Choice {
            label: t!("settings-rust-toolchain-location"),
            options: info
                .toolchains
                .iter()
                .map(|found| Choice { value: found.bin.display().to_string(), label: describe(found) })
                .collect(),
            selected: info.toolchain.as_ref().map(|found| found.bin.display().to_string()),
            action: Action::with_value("toolchain.rust.choose", "bin"),
        });
    }
    row.push(Item::Menu {
        label: t!("settings-rust-add-toolchain"),
        entries: vec![MenuEntry {
            label: t!("settings-rust-add-local"),
            action: Action::with_value("toolchain.rust.choose", "bin")
                .picking_folder(&t!("settings-rust-pick-toolchain")),
        }],
    });
    items.push(Item::Row { items: row });
    if let Some(missing) = &info.missing_toolchain {
        items.push(Item::Text {
            text: t!("settings-rust-toolchain-gone", path = missing.display().to_string()),
            tone: Tone::Error,
            spinner: false,
        });
    }
    let rustup = match &info.rustup {
        Some(rustup) => t!("settings-rust-rustup", path = rustup.display().to_string()),
        None => t!("settings-rust-no-rustup"),
    };
    items.push(Item::Text { text: rustup, tone: Tone::Secondary, spinner: false });
}

fn stdlib(info: &RustInfo, items: &mut Vec<Item>) -> bool {
    items.push(Item::Group { title: t!("settings-rust-stdlib") });
    if let Install::Running { line } = &info.stdlib_install {
        let detail = line.clone().unwrap_or_default();
        items.push(Item::Text {
            text: t!("settings-rust-stdlib-installing", detail = detail),
            tone: Tone::Secondary,
            spinner: true,
        });
        return true;
    }
    let text = match (&info.stdlib, info.stdlib_installed) {
        (Some(path), true) => path.display().to_string(),
        (Some(_), false) => t!("settings-rust-stdlib-missing"),
        (None, _) => t!("settings-rust-toolchain-missing"),
    };
    items.push(Item::Text { text, tone: Tone::Normal, spinner: false });
    if let Install::Failed { message } = &info.stdlib_install {
        items.push(Item::Text {
            text: t!("settings-rust-stdlib-failed", detail = message.as_str()),
            tone: Tone::Error,
            spinner: false,
        });
    }
    if !info.stdlib_installed {
        items.push(Item::Row {
            items: vec![Item::Button {
                label: t!("settings-rust-stdlib-download"),
                enabled: info.rustup.is_some(),
                action: Action::command("toolchain.rustSrc.install"),
            }],
        });
    }
    false
}

fn rust_analyzer(info: &RustInfo, items: &mut Vec<Item>) -> bool {
    items.push(Item::Group { title: "rust-analyzer".to_owned() });
    if let Install::Running { line } = &info.install {
        let detail = line.clone().unwrap_or_default();
        items.push(Item::Text {
            text: t!("settings-rust-analyzer-installing", detail = detail),
            tone: Tone::Secondary,
            spinner: true,
        });
        return true;
    }
    match &info.rust_analyzer {
        Some(found) => items.push(Item::Text {
            text: format!("{}   {}", found.version.as_deref().unwrap_or("?"), found.path.display()),
            tone: Tone::Normal,
            spinner: false,
        }),
        None => {
            items.push(Item::Text { text: t!("settings-rust-analyzer-missing"), tone: Tone::Secondary, spinner: false })
        }
    }
    if let Install::Failed { message } = &info.install {
        items.push(Item::Text {
            text: t!("settings-rust-analyzer-failed", detail = message.as_str()),
            tone: Tone::Error,
            spinner: false,
        });
    }
    items.push(Item::Path {
        label: t!("settings-rust-analyzer-path"),
        value: info.rust_analyzer_path.as_ref().map(|path| path.display().to_string()),
        action: Action::with_value("toolchain.rustAnalyzer.choose", "path"),
    });
    items.push(Item::Text { text: t!("settings-rust-analyzer-path-note"), tone: Tone::Secondary, spinner: false });
    let label = if info.rust_analyzer.is_some() {
        t!("settings-rust-analyzer-update")
    } else {
        t!("settings-rust-analyzer-install")
    };
    items.push(Item::Row {
        items: vec![
            Item::Button {
                label,
                enabled: info.rustup.is_some(),
                action: Action::command("toolchain.rustAnalyzer.install"),
            },
            Item::Button {
                label: t!("settings-rust-refresh"),
                enabled: true,
                action: Action::command("toolchain.rust.refresh"),
            },
        ],
    });
    items.push(Item::Text { text: t!("settings-rust-analyzer-note"), tone: Tone::Secondary, spinner: false });
    false
}

fn performance(info: &RustInfo, items: &mut Vec<Item>) {
    items.push(Item::Group { title: t!("settings-rust-ra-performance") });
    let settings = crate::analyzer::effective(
        &crate::toolchain::analyzer_defaults(),
        &info.rust_analyzer_settings,
        &serde_json::Value::Null,
    );
    for (key, label, options, default) in TUNED {
        let selected = match crate::analyzer::get(&settings, key) {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(serde_json::Value::Null) | None => (*default).to_owned(),
            Some(value) => value.to_string(),
        };
        let choice = Item::Choice {
            label: t!(label),
            options: options
                .iter()
                .map(|(value, label)| Choice {
                    value: (*value).to_owned(),
                    label: label.map_or_else(|| (*value).to_owned(), |label| t!(label)),
                })
                .collect(),
            selected: Some(selected),
            action: Action::with_value("toolchain.rustAnalyzer.configure", "value").param("key", *key),
        };
        let label = Item::Text { text: format!("{}:", t!(label)), tone: Tone::Normal, spinner: false };
        items.extend([label, choice]);
    }
    let excluded: Vec<String> = crate::analyzer::get(&settings, "files.exclude")
        .and_then(serde_json::Value::as_array)
        .map(|folders| folders.iter().filter_map(|folder| folder.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let excluded = if excluded.is_empty() {
        t!("settings-rust-ra-excluded-none")
    } else {
        t!("settings-rust-ra-excluded", folders = excluded.join(", "))
    };
    items.push(Item::Text { text: excluded, tone: Tone::Normal, spinner: false });
    items.push(Item::Text { text: t!("settings-rust-ra-note"), tone: Tone::Secondary, spinner: false });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use zen_core::Registry;

    use super::*;
    use crate::toolchain::{RustAnalyzer, RustToolchain};

    #[test]
    fn every_postfix_template_takes_the_expression_and_is_described_in_both_languages() {
        let templates: Vec<serde_json::Value> = serde_json::from_str(POSTFIX).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for template in &templates {
            let key = template["key"].as_str().unwrap();
            assert!(seen.insert(key), "{key} is declared once");
            assert_eq!(template["language"], "rust");
            assert!(template["text"].as_str().unwrap().contains("$EXPR$"), "{key}");
            let description = template["description"].as_str().unwrap();
            for language in ["en", "ru"] {
                let described = LOCALES
                    .iter()
                    .filter(|(code, _)| *code == language)
                    .any(|(_, source)| source.lines().any(|line| line.starts_with(&format!("{description} = "))));
                assert!(described, "{description} in {language}");
            }
        }
        for key in ["let", "if", "not", "for", "match", "some", "ok", "return"] {
            assert!(seen.contains(key), "{key}");
        }
        let stubs: Vec<serde_json::Value> = serde_json::from_str(DOC_COMMENTS).unwrap();
        assert_eq!(stubs[0]["opening"], "///");
        let rules: Vec<serde_json::Value> = serde_json::from_str(STATEMENTS).unwrap();
        assert_eq!(rules[0]["terminator"], ";");
    }

    fn info() -> RustInfo {
        let stable =
            RustToolchain { bin: PathBuf::from("/h/.cargo/bin"), version: Some("1.99.1".to_owned()), rustup: true };
        RustInfo {
            toolchains: vec![stable.clone()],
            toolchain: Some(stable),
            missing_toolchain: None,
            rustup: Some(PathBuf::from("/h/.cargo/bin/rustup")),
            stdlib: Some(PathBuf::from("/r/lib/rustlib/src/rust/library")),
            stdlib_installed: true,
            stdlib_install: Install::Idle,
            rust_analyzer: Some(RustAnalyzer {
                path: PathBuf::from("/r/bin/rust-analyzer"),
                version: Some("1.99.1".to_owned()),
            }),
            rust_analyzer_path: None,
            install: Install::Idle,
            rust_analyzer_settings: json!({}),
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
    fn the_rust_page_shows_the_toolchain_with_the_commands_that_change_it() {
        zen_i18n::register(LOCALES);
        let form = settings_form(&info());
        let texts = form.texts();
        for text in [
            t!("settings-rust-toolchain"),
            t!("settings-rust-stdlib"),
            "/r/lib/rustlib/src/rust/library".to_owned(),
            "rust-analyzer".to_owned(),
            "1.99.1   /r/bin/rust-analyzer".to_owned(),
            t!("settings-rust-reformat-on-save"),
        ] {
            assert!(texts.contains(&text.as_str()), "no {text:?} in {texts:?}");
        }
        assert!(!form.busy);
        assert_eq!(
            buttons(&form),
            [
                (t!("settings-rust-analyzer-update"), "toolchain.rustAnalyzer.install".to_owned(), true),
                (t!("settings-rust-refresh"), "toolchain.rust.refresh".to_owned(), true),
            ]
        );
        let Item::Row { items } = &form.items[1] else { panic!("{:?}", form.items[1]) };
        let Item::Choice { selected, action, .. } = &items[0] else { panic!("{:?}", items[0]) };
        assert_eq!(selected.as_deref(), Some("/h/.cargo/bin"));
        assert_eq!(action.params_with(Some(json!("/other/bin"))), json!({ "bin": "/other/bin" }));
    }

    #[test]
    fn the_rust_page_shows_rust_analyzers_settings_for_large_workspaces() {
        zen_i18n::register(LOCALES);
        let mut info = info();
        info.rust_analyzer_settings = json!({ "lru": { "capacity": 512 }, "check": { "command": "clippy" } });
        let form = settings_form(&info);
        let choices: Vec<(String, Option<String>, serde_json::Value)> = form
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Choice { label, selected, action, .. }
                    if action.command == "toolchain.rustAnalyzer.configure" =>
                {
                    Some((label.clone(), selected.clone(), action.params_with(Some(json!("true")))))
                }
                _ => None,
            })
            .collect();
        let find = |label: &str| choices.iter().find(|(shown, ..)| *shown == t!(label)).cloned().expect(label);
        assert_eq!(find("settings-rust-ra-check-scope").1.as_deref(), Some("false"));
        assert_eq!(find("settings-rust-ra-check-on-save").1.as_deref(), Some("true"));
        assert_eq!(find("settings-rust-ra-target-dir").1.as_deref(), Some("true"));
        assert_eq!(find("settings-rust-ra-cache-priming").1.as_deref(), Some("true"), "rust-analyzer's own default");
        assert_eq!(find("settings-rust-ra-memory").1.as_deref(), Some("512"));
        assert_eq!(find("settings-rust-ra-check-command").1.as_deref(), Some("clippy"));
        assert_eq!(find("settings-rust-ra-check-scope").2, json!({ "key": "check.workspace", "value": "true" }));
        assert_eq!(choices.len(), TUNED.len());
        let texts = form.texts();
        let excluded = t!("settings-rust-ra-excluded", folders = ".claude/worktrees");
        assert!(texts.contains(&excluded.as_str()), "{texts:?}");
        assert!(texts.contains(&t!("settings-rust-ra-performance").as_str()));
    }

    #[test]
    fn rust_analyzers_settings_are_commands_for_the_user_and_the_project() {
        zen_i18n::register(LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, Some(data.path()));
        host.run(&MODULE).unwrap();
        let given = host.finish();
        assert_eq!(
            given.excluded_folders,
            [zen_modules::ExcludedFolderDeclaration { name: "target", beside: "Cargo.toml" }],
            "a Cargo package's target is excluded"
        );
        let mut session = Session::new();
        for state in &given.window_states {
            state(&mut session);
        }
        let root = project.path().display().to_string();
        let answer = registry
            .dispatch(
                &mut session,
                "toolchain.rustAnalyzer.configure",
                json!({ "key": "cachePriming.enable", "value": false, "project": root }),
            )
            .unwrap();
        assert_eq!(answer["project"], json!({ "cachePriming": { "enable": false } }));
        assert_eq!(answer["effective"]["cachePriming"]["enable"], false);
        assert_eq!(answer["effective"]["check"]["workspace"], false, "the defaults are under it");
        assert!(project.path().join(".zen/rust-analyzer.json").is_file(), "the repository carries it");
        registry
            .dispatch(
                &mut session,
                "toolchain.rustAnalyzer.configure",
                json!({ "key": "lru.capacity", "value": "256" }),
            )
            .unwrap();
        let settings =
            registry.dispatch(&mut session, "toolchain.rustAnalyzer.settings", json!({ "project": root })).unwrap();
        assert_eq!(settings["ide"], json!({ "lru": { "capacity": 256 } }));
        assert_eq!(settings["effective"]["lru"]["capacity"], 256);
        assert_eq!(settings["effective"]["cachePriming"]["enable"], false);
        let plain = registry.dispatch(&mut session, "toolchain.rustAnalyzer.settings", json!({})).unwrap();
        assert!(plain.get("projectFile").is_none());
        assert_eq!(plain["project"], json!({}));
    }

    #[test]
    fn the_rust_page_offers_what_is_missing() {
        zen_i18n::register(LOCALES);
        let mut info = info();
        info.rust_analyzer = None;
        info.stdlib_installed = false;
        let form = settings_form(&info);
        assert!(form.texts().contains(&t!("settings-rust-analyzer-missing").as_str()));
        assert!(buttons(&form).contains(&(
            t!("settings-rust-stdlib-download"),
            "toolchain.rustSrc.install".to_owned(),
            true
        )));
        assert!(buttons(&form).contains(&(
            t!("settings-rust-analyzer-install"),
            "toolchain.rustAnalyzer.install".to_owned(),
            true
        )));
        info.install = Install::Running { line: Some("info: downloading component".to_owned()) };
        let form = settings_form(&info);
        assert!(form.busy);
        assert!(
            form.texts()
                .contains(&t!("settings-rust-analyzer-installing", detail = "info: downloading component").as_str())
        );
        info.rustup = None;
        info.install = Install::Idle;
        let form = settings_form(&info);
        assert!(
            buttons(&form).iter().all(|(_, command, enabled)| command == "toolchain.rust.refresh" || !enabled),
            "no rustup, no install"
        );
        let empty = RustInfo { toolchains: Vec::new(), toolchain: None, stdlib: None, ..info };
        assert!(settings_form(&empty).texts().contains(&t!("settings-rust-toolchain-missing").as_str()));
    }

    #[test]
    fn the_entry_point_registers_everything_rust() {
        zen_i18n::register(LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, Some(data.path()));
        host.run(&MODULE).unwrap();
        let given = host.finish();
        assert_eq!(given.highlighters.len(), 1);
        assert_eq!(given.project_models.len(), 1);
        assert_eq!(given.project_models[0].id(), "cargo");
        assert_eq!(given.run_providers[0].id(), "cargo");
        let kinds: Vec<String> = given.run_providers[0].kinds().into_iter().map(|kind| kind.id).collect();
        assert_eq!(kinds, ["cargo.run", "cargo.test", "cargo.bench", "rustc.file"]);
        assert_eq!(
            given.language_servers.iter().map(|server| server.id.as_str()).collect::<Vec<_>>(),
            ["rust-analyzer"]
        );
        assert_eq!(given.language_servers[0].initialization_options["cargo"]["targetDir"], true);
        assert_eq!(given.settings_pages[0].id, "rust");
        assert_eq!(given.code_styles[0].language, "rust");
        assert_eq!(given.code_style_declarations[0].language, "rust", "the declaration parses");
        let toolchain = given.toolchain.expect("the Rust toolchain");
        assert_eq!(toolchain.languages(), ["rust"]);
        assert!(toolchain.serves(&given.language_servers[0]));
        for id in [
            "toolchain.rust.info",
            "toolchain.rust.settings",
            "toolchain.rustAnalyzer.install",
            "toolchain.rustSrc.install",
        ] {
            assert!(registry.info(id).is_some(), "{id} is registered");
        }
        let mut session = Session::new();
        for state in &given.window_states {
            state(&mut session);
        }
        let form = registry.dispatch(&mut session, "toolchain.rust.settings", json!({})).unwrap();
        assert_eq!(form["items"][0]["kind"], "group");
    }

    #[test]
    fn every_text_of_the_rust_code_style_is_written_in_both_languages() {
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
    fn every_rust_command_is_named_in_both_languages() {
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
        assert_eq!(count, 9, "the toolchain's eight commands and the settings page");
    }
}
