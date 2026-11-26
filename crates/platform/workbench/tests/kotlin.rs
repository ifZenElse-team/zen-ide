use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::servers::{LanguageServers, ServerDeclaration};

fn manifest() -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/kotlin/plugin.json");
    serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap()
}

fn servers() -> Vec<ServerDeclaration> {
    ServerDeclaration::parse_list(&manifest()["languageServers"].to_string()).unwrap()
}

#[test]
fn the_plugin_declares_both_kotlin_servers_for_kotlin_files_and_scripts() {
    let servers = servers();
    for server in &servers {
        assert!(server.serves("Main.kt") && server.serves("build.gradle.kts") && !server.serves("Main.java"));
        assert_eq!(server.install.as_ref().unwrap().kind, zen_workbench::language::servers::InstallKind::Archive);
    }
    let (vendor, open) = (&servers[0], &servers[1]);
    assert_eq!((vendor.id.as_str(), open.id.as_str()), ("kotlin-lsp", "kotlin-language-server"));
    assert_eq!(vendor.args, ["--stdio"]);
    let mac = vendor.install.as_ref().unwrap().archive_for("macos-aarch64").unwrap();
    assert!(mac.0.ends_with("-aarch64.sit"), "{}", mac.0);
    assert!(vendor.install.as_ref().unwrap().archive_for("windows-x86_64").is_none(), "the hint there");
    assert!(open.install.as_ref().unwrap().archive_for("windows-x86_64").is_some(), "one archive for every platform");
    let notice = vendor.notice.as_ref().unwrap();
    assert!(notice.text_in("ru-RU").contains("JetBrains") && notice.text_in("de").contains("JetBrains' terms"));
    assert!(open.notice.is_none());
}

struct Window {
    registry: Registry,
    session: Session,
    folder: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Window {
    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }

    fn text(&self, path: &Path) -> String {
        let workbench = self.session.state::<zen_workbench::Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == path);
        document.map(|document| document.text().to_owned()).unwrap_or_default()
    }

    fn until(&mut self, command: &str, params: Value, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            self.run("language.servers.sync", json!({}));
            let answer = self.registry.dispatch(&mut self.session, command, params.clone());
            match answer {
                Ok(answer) if done(&answer) => return answer,
                answer => assert!(Instant::now() < deadline, "{command}: {answer:?}"),
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

#[test]
fn the_plugin_declares_kotlin_code_style_as_editorconfig_properties() {
    let declarations =
        zen_workbench::code_style_options::CodeStyleDeclaration::parse_list(&manifest()["codeStyles"].to_string())
            .unwrap();
    let [style] = declarations.as_slice() else { panic!("{declarations:?}") };
    assert_eq!(style.language, "kotlin");
    let file = style.config_file.as_ref().unwrap();
    assert_eq!(file.names, [".editorconfig"]);
    assert_eq!(file.format, zen_project::code_style::ConfigFormat::EditorConfig);
    assert_eq!(file.section.as_deref(), Some("*.{kt,kts}"));
    for option in &style.options {
        let key = option.config_key.as_deref().unwrap_or_default();
        assert!(key.starts_with("ij_kotlin_") || key == "max_line_length", "{}: {key}", option.id);
        assert_eq!(option.editor_config.as_deref(), Some(key), "Export → EditorConfig File writes {}", option.id);
        assert!(option.server_setting.is_none() && option.formatting_option.is_none(), "{}", option.id);
    }
    let given = [("trailingComma".to_owned(), json!(true)), ("nameCountToUseStar".to_owned(), json!(99))].into();
    let written = style.write_config("root = true\n\n[*]\nindent_size = 4\n", &given);
    assert_eq!(
        written,
        "root = true\n\n[*]\nindent_size = 4\n\n[*.{kt,kts}]\nij_kotlin_allow_trailing_comma = true\nij_kotlin_name_count_to_use_star_import = 99\n"
    );
    assert_eq!(style.read_config(&written), given);
}

#[allow(clippy::print_stderr, reason = "the list of the features checked is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name}")))
}

const SETTINGS: &str = "rootProject.name = \"shop\"\n";
const BUILD: &str = "plugins {\n    kotlin(\"jvm\") version \"2.2.21\"\n}\n\nrepositories {\n    mavenCentral()\n}\n\nkotlin {\n    jvmToolchain(21)\n}\n";
const MAIN: &str = "package demo\n\nfun total(prices: List<Int>): Int = prices.sum()\n\nfun main() {\n    val sum = total(listOf(1, 2))\n    println(sum)\n}\n";

#[test]
#[ignore = "needs the vendor's Kotlin language server: ZEN_KOTLIN_LSP and JAVA_HOME (see the module's documentation)"]
fn the_vendor_kotlin_lsp_checks_completes_navigates_renames_and_formats() {
    kotlin_server_end_to_end("kotlin-lsp", &env("ZEN_KOTLIN_LSP"));
}

#[test]
#[ignore = "needs fwcd's Kotlin Language Server: ZEN_KOTLIN_LANGUAGE_SERVER and JAVA_HOME (see the module's documentation)"]
fn fwcd_kotlin_language_server_checks_completes_navigates_renames_and_formats() {
    kotlin_server_end_to_end("kotlin-language-server", &env("ZEN_KOTLIN_LANGUAGE_SERVER"));
}

fn kotlin_server_end_to_end(id: &str, unpacked: &Path) {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert!(std::env::var_os("JAVA_HOME").is_some(), "JAVA_HOME: the project's JDK, as ADR-0147 gives it");
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in
        [("settings.gradle.kts", SETTINGS), ("build.gradle.kts", BUILD), ("src/main/kotlin/demo/Main.kt", MAIN)]
    {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let data = tempfile::tempdir().unwrap();
    let own = data.path().join("language-servers");
    std::fs::create_dir_all(&own).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(unpacked, own.join(id)).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    let servers = LanguageServers::load(
        Some(data.path().join("language-servers.json")),
        std::sync::Arc::new(zen_workbench::language::servers::SystemTools),
    );
    servers.declare(self::servers());
    assert!(servers.executable(id).is_some(), "{id} in the IDE's folder");
    session.set_state(servers);
    let mut window = Window { registry, session, folder, _data: data };
    window.run("lsp.server.choose", json!({ "id": id }));

    let main = window.path("src/main/kotlin/demo/Main.kt");
    window.run("file.document.open", json!({ "path": main }));

    window.run("editor.selection.set", json!({ "selections": [{ "head": MAIN.find("listOf(1, 2)").unwrap() }] }));
    window.run("editor.edit.type", json!({ "text": "\"x\" + " }));
    let problems = window.until("problems.file.list", json!({ "path": main }), |answer| {
        !zen_workbench::language::listed_problems(answer).is_empty()
    });
    checked(&format!("{id} problems: {}", zen_workbench::language::listed_problems(&problems).len()));
    window.run("editor.edit.undo", json!({}));

    let at = MAIN.find("prices.sum").unwrap() + "prices.".len();
    window.run("editor.selection.set", json!({ "selections": [{ "head": at }] }));
    window.run("editor.edit.type", json!({ "text": "ma" }));
    window.run("editor.completion.start", json!({}));
    let list = window.until("editor.completion.list", json!({}), |list| {
        list["items"].as_array().is_some_and(|items| {
            items.iter().any(|item| item["label"].as_str().is_some_and(|label| label.starts_with("map")))
        })
    });
    window.run("editor.completion.cancel", json!({}));
    window.run("editor.edit.undo", json!({}));
    assert_eq!(window.text(&main), MAIN, "the typed prefix undone");
    checked(&format!("{id} completion: {} items for `ma`", list["items"].as_array().map_or(0, Vec::len)));

    let call = MAIN.find("total(listOf").unwrap() + 1;
    let found =
        window.until("navigate.declaration.goTo", json!({ "path": main, "offset": call, "waitMs": 5000 }), |answer| {
            answer.get("pending") != Some(&json!(true))
        });
    assert!(found.to_string().contains("Main.kt"), "{found}");
    checked(&format!("{id} go to declaration: ok"));

    let renamed = window.until(
        "refactor.rename.apply",
        json!({ "path": main, "offset": call, "newName": "priceTotal", "waitMs": 10000 }),
        |answer| answer.get("pending") != Some(&json!(true)),
    );
    let text = window.text(&main);
    assert!(text.contains("fun priceTotal(") && text.contains("priceTotal(listOf"), "{renamed}");
    checked(&format!("{id} rename: ok"));

    let before = window.text(&main).find("fun ").unwrap();
    window.run("editor.selection.set", json!({ "selections": [{ "head": before }] }));
    window.run("editor.edit.type", json!({ "text": "val   spaced   =   1\n\n" }));
    window.until("editor.code.reformat", json!({ "path": main, "waitMs": 10000 }), |answer| answer["pending"] == false);
    assert!(window.text(&main).contains("val spaced = 1\n"), "{}", window.text(&main));
    checked(&format!("{id} reformat code: ok"));
}
