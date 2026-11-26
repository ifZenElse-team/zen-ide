use serde_json::json;

use super::*;

const RUST: &str = r#"{
    "language": "rust",
    "title": { "en": "Rust" },
    "options": [
        { "id": "max_width", "label": { "en": "Width" }, "tab": "wrappingAndBraces",
          "kind": { "type": "integer", "min": 1, "max": 1000 }, "default": 100,
          "configKey": "max_width", "editorConfig": "max_line_length", "formattingOption": "zenMaxWidth" }
    ],
    "configFile": { "names": ["rustfmt.toml", ".rustfmt.toml"],
        "arguments": { "section": "rust-analyzer", "path": "rustfmt.extraArgs", "flag": "--config" } }
}"#;

const GO: &str = r#"{
    "language": "go",
    "title": { "en": "Go" },
    "options": [
        { "id": "gofumpt", "label": { "en": "gofumpt" }, "tab": "wrappingAndBraces", "kind": { "type": "boolean" },
          "default": false, "serverSetting": { "section": "gopls", "path": "gofumpt" } },
        { "id": "tidy", "label": { "en": "Tidy" }, "tab": "wrappingAndBraces", "kind": { "type": "boolean" },
          "default": true }
    ],
    "arrangement": [ { "id": "const", "label": { "en": "c" } }, { "id": "var", "label": { "en": "v" } },
        { "id": "type", "label": { "en": "t" } }, { "id": "func", "label": { "en": "f" } } ]
}"#;

const JAVA: &str = r#"{
    "language": "java",
    "title": { "en": "Java" },
    "options": [
        { "id": "braces", "label": { "en": "Braces" }, "tab": "wrappingAndBraces",
          "kind": { "type": "choice", "choices": [
              { "value": "end_of_line", "label": { "en": "End of line" } },
              { "value": "next_line", "label": { "en": "Next line" } } ] },
          "default": "end_of_line", "configKey": "f.brace_method" },
        { "id": "stars", "label": { "en": "Stars" }, "tab": "imports", "kind": { "type": "integer", "min": 1, "max": 999 },
          "default": 99, "serverSetting": { "section": "java", "path": "sources.organizeImports.starThreshold" } }
    ],
    "configFile": { "names": [".zen/java-formatter.xml"], "format": "eclipseProfile",
        "location": { "section": "java", "path": "format.settings.url" } }
}"#;

struct GoLike;

impl Arranger for GoLike {
    fn language(&self) -> &str {
        "go"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn arrange(&self, text: &str, order: &[String]) -> Option<String> {
        let mut lines: Vec<&str> = text.lines().collect();
        let rank = |line: &&str| order.iter().position(|kind| line.starts_with(kind.as_str())).unwrap_or(order.len());
        lines.sort_by_key(rank);
        let arranged = lines.join("\n") + "\n";
        (arranged != text).then_some(arranged)
    }

    fn before_reformat(&self, text: &str, option: &dyn Fn(&str) -> bool) -> Option<String> {
        option("tidy").then(|| text.replace("  ", " "))
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
}

impl Fixture {
    fn new(plugin: Option<&'static str>) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        let mut styles = CodeStyles::new(
            vec![CodeStyleDeclaration::parse(RUST).unwrap()],
            vec![Arc::new(GoLike) as Arc<dyn Arranger>],
        );
        if let Some(plugin) = plugin {
            styles = styles.with_plugins(Arc::new(move || vec![CodeStyleDeclaration::parse(plugin).unwrap()]));
        }
        session.set_state(styles);
        Self { registry, session, project }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn file(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn open(&mut self, name: &str, text: &str) -> PathBuf {
        let path = self.file(name);
        std::fs::write(&path, text).unwrap();
        self.run("file.document.open", json!({ "path": path }));
        path
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn scheme(&mut self, project: bool) {
        self.session.settings_mut().set(project_settings::PROJECT_CODE_STYLE, project).unwrap();
    }
}

fn values_of(language: &str, id: &str, value: Value) -> (String, Values) {
    (language.to_owned(), Values::from([(id.to_owned(), value)]))
}

#[test]
fn a_plugin_declares_a_page_as_data_and_a_module_keeps_its_language() {
    let mut fixture = Fixture::new(Some(GO));
    let listed = fixture.run("editor.codeStyle.list", json!({}));
    let languages: Vec<&str> =
        listed.as_array().unwrap().iter().map(|style| style["declaration"]["language"].as_str().unwrap()).collect();
    assert_eq!(languages, ["rust", "go"]);
    assert_eq!(listed[0]["ide"]["max_width"], 100);
    assert_eq!(listed[1]["arrangement"], json!(["const", "var", "type", "func"]));
    let duplicate = CodeStyles::new(vec![CodeStyleDeclaration::parse(RUST).unwrap()], Vec::new())
        .with_plugins(Arc::new(|| vec![CodeStyleDeclaration::parse(RUST).unwrap()]));
    assert_eq!(duplicate.declarations().len(), 1, "a plug-in never replaces a module's page");
}

#[test]
fn the_ide_values_reach_formatting_options_and_the_servers_with_the_default_scheme() {
    let mut fixture = Fixture::new(Some(GO));
    fixture.scheme(false);
    let given = BTreeMap::from([values_of("rust", "max_width", json!(80)), values_of("go", "gofumpt", json!(true))]);
    apply(fixture.session.settings_mut(), &given);
    let main = fixture.file("main.rs");
    assert_eq!(formatting_options(&fixture.session, &main).get("zenMaxWidth"), Some(&json!(80)));
    let sections = server_sections(&fixture.session);
    assert_eq!(sections["go"]["gopls"], json!({ "gofumpt": true }));
    assert_eq!(sections["rust"]["rust-analyzer"], json!({ "rustfmt": { "extraArgs": ["--config", "max_width=80"] } }));
    assert_eq!(editor_config_pairs(&fixture.session, "rust"), [("max_line_length".to_owned(), "80".to_owned())]);
    let exported = crate::editor_config::scheme_text(&fixture.session, &fixture.file(".editorconfig"));
    assert!(
        exported.contains("[*.rs]\nindent_size = 4\nindent_style = space\ntab_width = 4\nmax_line_length = 80\n"),
        "{exported}"
    );
}

#[test]
fn with_the_project_scheme_rustfmt_reads_the_projects_file_and_gets_no_arguments() {
    let mut fixture = Fixture::new(None);
    fixture.scheme(true);
    apply(fixture.session.settings_mut(), &BTreeMap::from([values_of("rust", "max_width", json!(80))]));
    std::fs::write(fixture.file(".rustfmt.toml"), "edition = \"2024\"\nmax_width = 120\n").unwrap();
    assert!(!server_sections(&fixture.session).contains_key("rust"));
    let main = fixture.file("main.rs");
    assert_eq!(formatting_options(&fixture.session, &main).get("zenMaxWidth"), Some(&json!(120)));
    let written =
        fixture.run("editor.codeStyle.writeProject", json!({ "language": "rust", "values": { "max_width": 99 } }));
    assert_eq!(written["changed"], true);
    assert_eq!(written["file"], json!(fixture.file(".rustfmt.toml")), "the file the project has wins");
    let text = std::fs::read_to_string(fixture.file(".rustfmt.toml")).unwrap();
    assert_eq!(text, "edition = \"2024\"\nmax_width = 99\n");
    let refused = fixture.registry.dispatch(
        &mut fixture.session,
        "editor.codeStyle.writeProject",
        json!({ "language": "rust", "values": { "max_width": 0 } }),
    );
    assert!(refused.is_err(), "a value out of range is refused");
}

#[test]
fn rearrange_code_reorders_by_the_chosen_order_as_one_undo_step() {
    let mut fixture = Fixture::new(Some(GO));
    fixture.open("main.go", "func f\nvar v\nconst c\n");
    let done = fixture.run("editor.code.rearrange", json!({}));
    assert_eq!(done, json!({ "changed": true, "error": null }));
    assert_eq!(fixture.text(), "const c\nvar v\nfunc f\n");
    fixture.run("editor.edit.undo", json!({}));
    assert_eq!(fixture.text(), "func f\nvar v\nconst c\n");
    fixture.session.settings_mut().set(&key("go", ARRANGEMENT), json!(["func", "var"])).unwrap();
    let unchanged = fixture.run("editor.code.rearrange", json!({}));
    assert_eq!(unchanged["changed"], false, "unlisted kinds keep the declared order after the chosen ones");
    fixture.open("lib.rs", "fn main() {}\n");
    let refused = fixture.run("editor.code.rearrange", json!({}));
    assert_eq!(refused["changed"], false);
    assert!(refused["error"].as_str().unwrap().contains("no arrangement rules"));
}

#[test]
fn reformat_code_rearranges_first_when_asked() {
    let mut fixture = Fixture::new(Some(GO));
    fixture.open("main.go", "func f\nconst c\n");
    let _ = fixture.registry.dispatch(&mut fixture.session, "editor.code.reformat", json!({ "waitMs": 0 }));
    assert_eq!(fixture.text(), "func f\nconst c\n", "off by default");
    fixture.session.settings_mut().set(REARRANGE_ON_REFORMAT, true).unwrap();
    let _ = fixture.registry.dispatch(&mut fixture.session, "editor.code.reformat", json!({ "waitMs": 0 }));
    assert_eq!(fixture.text(), "const c\nfunc f\n");
    let _ = fixture.registry.dispatch(
        &mut fixture.session,
        "editor.code.reformat",
        json!({ "waitMs": 0, "rearrange": false }),
    );
}

#[test]
fn reformat_code_lets_the_language_tidy_first_as_its_code_style_says() {
    let mut fixture = Fixture::new(Some(GO));
    fixture.open("main.go", "const  c\n");
    let _ = fixture.registry.dispatch(&mut fixture.session, "editor.code.reformat", json!({ "waitMs": 0 }));
    assert_eq!(fixture.text(), "const c\n", "the option's default is read");
    fixture.open("other.go", "const  d\n");
    apply(fixture.session.settings_mut(), &BTreeMap::from([values_of("go", "tidy", json!(false))]));
    let _ = fixture.registry.dispatch(&mut fixture.session, "editor.code.reformat", json!({ "waitMs": 0 }));
    assert_eq!(fixture.text(), "const  d\n");
}

fn profiles_in(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> =
        std::fs::read_dir(folder).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    found.sort();
    found
}

#[test]
fn the_ide_writes_its_own_formatter_profile_and_tells_the_server_where_it_is() {
    let mut fixture = Fixture::new(Some(JAVA));
    let data = tempfile::tempdir().unwrap();
    fixture.session.state_mut::<crate::workspaces::Workspaces>().set_data_dir(data.path().to_owned());
    fixture.scheme(false);
    assert_eq!(
        server_sections(&fixture.session)["java"]["java"],
        json!({}),
        "no profile while every value is the default"
    );
    apply(fixture.session.settings_mut(), &BTreeMap::from([values_of("java", "braces", json!("next_line"))]));
    let folder = data.path().join("code-style/java");
    let sections = server_sections(&fixture.session);
    let [profile] = profiles_in(&folder).try_into().unwrap();
    assert!(profile.file_name().unwrap().to_string_lossy().starts_with("java-formatter-"), "{}", profile.display());
    assert_eq!(sections["java"]["java"], json!({ "format": { "settings": { "url": profile.to_string_lossy() } } }));
    let text = std::fs::read_to_string(&profile).unwrap();
    assert!(text.contains("<setting id=\"f.brace_method\" value=\"next_line\"/>"), "{text}");
    assert_eq!(server_sections(&fixture.session), sections, "the same values keep the same file");
    assert!(!fixture.file(".zen/java-formatter.xml").exists(), "the IDE's scheme leaves the project alone");
}

#[test]
fn a_changed_profile_gets_a_new_file_so_that_the_server_reads_it_again() {
    let mut fixture = Fixture::new(Some(JAVA));
    let data = tempfile::tempdir().unwrap();
    fixture.session.state_mut::<crate::workspaces::Workspaces>().set_data_dir(data.path().to_owned());
    fixture.scheme(false);
    let url =
        |fixture: &Fixture| server_sections(&fixture.session)["java"]["java"]["format"]["settings"]["url"].clone();
    apply(fixture.session.settings_mut(), &BTreeMap::from([values_of("java", "braces", json!("next_line"))]));
    let first = url(&fixture);
    fixture.session.settings_mut().set(&key("java", "braces"), "end_of_line").unwrap();
    assert_eq!(url(&fixture), Value::Null, "back at the defaults the server keeps its own");
    fixture.session.settings_mut().set(&key("java", "braces"), "next_line").unwrap();
    assert_eq!(url(&fixture), first, "the same values, the same file");
    let folder = data.path().join("code-style/java");
    assert_eq!(profiles_in(&folder).len(), 1, "older profiles are removed");
}

#[test]
fn with_the_project_scheme_the_server_reads_the_projects_formatter_profile() {
    let mut fixture = Fixture::new(Some(JAVA));
    fixture.scheme(true);
    assert_eq!(
        server_sections(&fixture.session)["java"]["java"],
        json!({}),
        "a project without the file keeps the server's defaults"
    );
    let written = fixture
        .run("editor.codeStyle.writeProject", json!({ "language": "java", "values": { "braces": "next_line" } }));
    let profile = fixture.file(".zen/java-formatter.xml");
    assert_eq!(written, json!({ "file": profile, "changed": true }));
    let sections = server_sections(&fixture.session);
    assert_eq!(
        sections["java"]["java"]["format"]["settings"]["url"],
        json!(profile.to_string_lossy()),
        "without the IDE's folder, the file itself"
    );
    let listed = fixture.run("editor.codeStyle.list", json!({}));
    let java = listed.as_array().unwrap().iter().find(|style| style["declaration"]["language"] == "java").unwrap();
    assert_eq!(java["project"]["braces"], "next_line");
    assert_eq!(java["usesProject"], true);
    fixture.session.settings_mut().set(&key("java", "stars"), 5).unwrap();
    fixture.session.settings_mut().set(&key("java", "braces"), "end_of_line").unwrap();
    let values = effective_values(&fixture.session, &CodeStyleDeclaration::parse(JAVA).unwrap());
    assert_eq!(
        (values["braces"].clone(), values["stars"].clone()),
        (json!("next_line"), json!(5)),
        "what the file does not keep stays the IDE's"
    );
    let data = tempfile::tempdir().unwrap();
    fixture.session.state_mut::<crate::workspaces::Workspaces>().set_data_dir(data.path().to_owned());
    let sections = server_sections(&fixture.session);
    let copy = PathBuf::from(sections["java"]["java"]["format"]["settings"]["url"].as_str().unwrap());
    assert!(copy.starts_with(data.path()), "{}", copy.display());
    assert_eq!(std::fs::read_to_string(copy).unwrap(), std::fs::read_to_string(profile).unwrap());
}

#[test]
fn an_agent_neither_reads_nor_writes_a_formatter_file_hidden_from_it() {
    let mut fixture = Fixture::new(Some(RUST));
    let file = fixture.file("rustfmt.toml");
    std::fs::write(&file, "max_width = 77\n").unwrap();
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["rustfmt.toml".to_owned()]);
    let (registry, session) = (&fixture.registry, &mut fixture.session);
    let write = json!({ "language": "rust", "values": { "max_width": 90 } });
    let refused = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "editor.codeStyle.writeProject", write.clone())
    });
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "max_width = 77\n");
    let listed =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "editor.codeStyle.list", json!({})))
            .unwrap();
    assert!(!listed.to_string().contains("77"), "{listed}");
    assert!(registry.dispatch(session, "editor.codeStyle.writeProject", write).is_ok(), "a person's goes through");
    assert!(std::fs::read_to_string(&file).unwrap().contains("90"));
}

#[test]
fn writing_the_project_style_never_overwrites_a_change_made_outside_an_open_file() {
    let mut fixture = Fixture::new(Some(RUST));
    let file = fixture.file("rustfmt.toml");
    std::fs::write(&file, "max_width = 77\n").unwrap();
    let (registry, session) = (&fixture.registry, &mut fixture.session);
    registry.dispatch(session, "file.document.open", json!({ "path": file })).unwrap();
    registry.dispatch(session, "editor.edit.insert", json!({ "text": "# mine\n" })).unwrap();
    std::fs::write(&file, "max_width = 12\n").unwrap();
    let write = json!({ "language": "rust", "values": { "max_width": 90 } });
    assert!(registry.dispatch(session, "editor.codeStyle.writeProject", write).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "max_width = 12\n", "their change stays");
    let document = session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.text().starts_with("# mine"), "and so does mine");
}

#[test]
fn two_projects_keep_their_own_copy_of_the_formatter_profile() {
    let data = tempfile::tempdir().unwrap();
    let copy = |text: &str| {
        let mut fixture = Fixture::new(Some(JAVA));
        fixture.session.state_mut::<crate::workspaces::Workspaces>().set_data_dir(data.path().to_owned());
        fixture.scheme(true);
        let file = fixture.file(".zen/java-formatter.xml");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        let sections = server_sections(&fixture.session);
        let url = sections["java"]["java"]["format"]["settings"]["url"].as_str().unwrap().to_owned();
        (fixture, PathBuf::from(url))
    };
    let (_first, one) = copy("<profiles><!-- one --></profiles>\n");
    let (_second, two) = copy("<profiles><!-- two --></profiles>\n");
    assert_ne!(one, two);
    assert!(one.is_file(), "opening the second project leaves the first one's copy");
    assert!(two.is_file());
}
