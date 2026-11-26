use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::*;
use crate::build_tools::BuildEnv;
use crate::plugin_projects::{PluginProject, PluginProjects};

const POM: &str = "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>shop</artifactId>\n\
<version>1</version>\n<parent><groupId>demo</groupId><artifactId>base</artifactId><version>9</version></parent>\n</project>\n";

fn plugin(id: &str, name: &str) -> PluginProject {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{id}/plugin.json"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    PluginProject {
        id: id.to_owned(),
        name: name.to_owned(),
        support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
    }
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    env: BuildEnv,
    bin: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(project.join("core")).unwrap();
        std::fs::create_dir_all(project.join("target")).unwrap();
        std::fs::write(project.join("pom.xml"), POM).unwrap();
        std::fs::write(project.join("core/pom.xml"), "<project/>\n").unwrap();
        std::fs::write(project.join("target/pom.xml"), "<project/>\n").unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let env = BuildEnv::default();
        crate::trust::trust_in_tests(&env, &project);
        env.set_folders(Some(vec![bin.clone()]));
        env.set_home(Some(dir.path().join("home")));
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        env.set_jdk(Some(Arc::new(move |_root: &Path| Some(jdk.clone()))));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.set_state(env.clone());
        session.state_mut::<PluginProjects>().set(vec![plugin("maven", "Maven"), plugin("gradle", "Gradle")]);
        Self { registry, session, project, env, bin, _dir: dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn pending(&mut self) -> Value {
        self.run("build.changes.get", json!({}))["pending"].clone()
    }

    fn type_into_open_document(&mut self, text: &str) {
        let document = self.session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert!(document.replace_ranges(vec![(0..0, text.to_owned())]));
    }
}

#[test]
fn only_the_build_systems_whose_plugins_support_the_project_are_listed() {
    let mut fixture = Fixture::new();
    let changes = fixture.run("build.changes.get", json!({}));
    let ids: Vec<&str> = changes["systems"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["maven"], "no Gradle script in the project");
    assert_eq!(changes["systems"][0]["reload"], "maven.project.reload");
    assert_eq!(changes["reloadAfter"], "external");
    let maven = systems(&fixture.session, &fixture.project).remove(0);
    let found = find_build_files(&fixture.project, &maven);
    assert_eq!(found, [fixture.project.join("core/pom.xml"), fixture.project.join("pom.xml")], "never in target");
}

#[test]
fn an_edit_in_the_ide_shows_load_changes_until_the_project_loads_again() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.pending(), json!([]), "the first look is the loaded state");
    let pom = fixture.project.join("pom.xml");
    fixture.run("file.document.open", json!({ "path": pom }));
    fixture.type_into_open_document("<!-- x -->");
    let pending = fixture.pending();
    assert_eq!(pending[0]["id"], "maven");
    assert_eq!(pending[0]["changed"], json!([pom]));
    assert_eq!((pending[0]["external"].clone(), pending[0]["unsaved"].clone()), (json!(false), json!(true)));
    assert_eq!(pending[0]["autoReload"], false, "an edit shows the button");
    fixture.run("file.document.save", json!({}));
    let pending = fixture.pending();
    assert_eq!((pending[0]["unsaved"].clone(), pending[0]["autoReload"].clone()), (json!(false), json!(false)));
    fixture.session.settings_mut().set(RELOAD_AFTER, ReloadAfter::Any).unwrap();
    assert_eq!(fixture.pending()[0]["autoReload"], true, "any change once saved");
    mark_loaded(&mut fixture.session, "maven", &fixture.project.clone());
    assert_eq!(fixture.pending(), json!([]), "loaded again, the button hides");
}

#[test]
fn a_change_made_outside_the_ide_loads_by_itself() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.pending(), json!([]));
    let core = fixture.project.join("core/pom.xml");
    std::fs::write(&core, "<project><artifactId>core</artifactId></project>\n").unwrap();
    let pending = fixture.pending();
    assert_eq!(pending[0]["changed"], json!([core]));
    assert_eq!((pending[0]["external"].clone(), pending[0]["autoReload"].clone()), (json!(true), json!(true)));
    std::fs::write(&core, "<project/>\n").unwrap();
    assert_eq!(fixture.pending(), json!([]), "the same text as loaded is no change");
}

#[cfg(unix)]
#[test]
fn a_failed_maven_sync_shows_its_steps_the_place_and_the_fix() {
    let mut fixture = Fixture::new();
    let pom = fixture.project.join("pom.xml");
    script(
        &fixture.bin.join("mvn"),
        &format!(
            "echo '[INFO] Scanning for projects...'\n\
             echo '[ERROR] [ERROR] Some problems were encountered while processing the POMs:'\n\
             echo '[FATAL] Non-resolvable parent POM for demo:shop:1: Could not transfer artifact demo:base:pom:9 from/to central: repo.maven.apache.org: Unknown host and parent.relativePath points at no local POM @ line 3, column 9'\n\
             echo '[ERROR]   The project demo:shop:1 ({}) has 1 error'\n\
             echo '[ERROR] -> [Help 2]'\n\
             exit 1",
            pom.display()
        ),
    );
    let reloaded = fixture.run("maven.project.reload", json!({ "waitMs": 20000 }));
    assert_eq!(reloaded["dependencyTree"]["state"], "failed");
    let sync = fixture.run("build.sync.get", json!({}))["syncs"][0].clone();
    assert_eq!(
        (sync["system"].clone(), sync["state"].clone(), sync["errors"].clone()),
        (json!("maven"), json!("failed"), json!(1))
    );
    assert!(sync["tookMs"].is_u64());
    let titles: Vec<&str> = sync["steps"].as_array().unwrap().iter().map(|s| s["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Read pom.xml files", "Import into the Java language server", "Resolve dependencies"]);
    assert_eq!(sync["steps"][1]["messages"][0]["severity"], "info", "no language server runs here");
    let error = &sync["steps"][2]["messages"][0];
    assert!(error["text"].as_str().unwrap().starts_with("Non-resolvable parent POM"), "{error}");
    assert_eq!(
        (error["file"].clone(), error["line"].clone(), error["column"].clone()),
        (json!(pom), json!(3), json!(9))
    );
    let offset = POM.lines().take(2).map(|line| line.chars().count() + 1).sum::<usize>() + 8;
    assert_eq!(error["offset"], offset);
    assert_eq!(error["fix"]["command"], "maven.options.set");
    assert_eq!(
        (error["fix"]["params"].clone(), error["fix"]["reload"].clone()),
        (json!({ "offline": true }), json!(true))
    );
    assert_eq!(sync["steps"][2]["messages"].as_array().unwrap().len(), 1, "the noise around it is left out");
    assert_eq!(fixture.pending(), json!([]), "a sync is a load");
}

#[cfg(unix)]
#[test]
fn without_maven_the_sync_offers_to_download_it() {
    let mut fixture = Fixture::new();
    assert!(fixture.registry.dispatch(&mut fixture.session, "maven.project.reload", json!({})).is_err());
    let sync = fixture.run("build.sync.get", json!({}))["syncs"][0].clone();
    assert_eq!(sync["state"], "failed");
    assert_eq!(sync["steps"][2]["messages"][0]["fix"]["command"], "maven.distribution.download");
    let _ = &fixture.env;
}

#[cfg(unix)]
#[test]
fn a_failed_gradle_sync_points_at_the_script_line() {
    let mut fixture = Fixture::new();
    let build = fixture.project.join("build.gradle");
    std::fs::write(&build, "plugins {\n  id 'java'\n  oops\n}\n").unwrap();
    script(
        &fixture.bin.join("gradle"),
        &format!(
            "echo 'FAILURE: Build failed with an exception.' >&2\n\
             echo '' >&2\n\
             echo '* Where:' >&2\n\
             echo \"Build file '{}' line: 3\" >&2\n\
             echo '' >&2\n\
             echo '* What went wrong:' >&2\n\
             echo 'A problem occurred evaluating root project shop.' >&2\n\
             echo '> Unsupported class file major version 68' >&2\n\
             echo '' >&2\n\
             echo '* Try:' >&2\n\
             exit 1",
            build.display()
        ),
    );
    fixture.run("gradle.project.reload", json!({ "waitMs": 20000 }));
    let sync = fixture.run("build.sync.get", json!({}))["syncs"][0].clone();
    assert_eq!(sync["system"], "gradle");
    let error = &sync["steps"][2]["messages"][0];
    assert_eq!(
        error["text"],
        "A problem occurred evaluating root project shop. > Unsupported class file major version 68"
    );
    assert_eq!((error["file"].clone(), error["line"].clone()), (json!(build), json!(3)));
    assert_eq!(error["fix"]["params"], json!({ "page": "projectStructure" }), "choose a JDK");
}

#[test]
fn gradle_offline_is_an_option_of_the_build() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.run("gradle.options.set", json!({})), json!({ "offline": false }));
    assert_eq!(fixture.run("gradle.options.set", json!({ "offline": true })), json!({ "offline": true }));
    let kotlin = "e: file:///p/build.gradle.kts:12:5: Unresolved reference: implementatio\nw: /p/settings.gradle.kts:1:1: Deprecated";
    let found = crate::gradle::problems::problems(kotlin, Path::new("/p"));
    assert_eq!(found.len(), 2);
    assert_eq!(
        (found[0].file.clone(), found[0].line, found[0].column),
        (Some(PathBuf::from("/p/build.gradle.kts")), Some(12), Some(5))
    );
    assert_eq!((found[1].severity, found[1].text.as_str()), (Severity::Warning, "Deprecated"));
}
