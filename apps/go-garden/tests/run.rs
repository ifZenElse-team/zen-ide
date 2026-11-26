use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::fake::FakeProvider;
use zen_run::{RunEnv, RunTarget, Stream, TargetKind};

fn gogarden() -> (Registry, Session, tempfile::TempDir, FakeProvider) {
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    zen_lang_go::toolchain::register(&mut registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    let fake = FakeProvider::new();
    session.state_mut::<zen_workbench::run::Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());
    (registry, session, dir, fake)
}

fn target(line: u32, kind: TargetKind, name: &str) -> RunTarget {
    RunTarget {
        line,
        kind,
        name: name.to_owned(),
        package: "app".to_owned(),
        configuration: FakeProvider::configuration(name),
    }
}

#[test]
fn targets_context_runs_console_and_tests_through_the_registry() {
    let (registry, mut session, dir, fake) = gogarden();
    let file = dir.path().join("main_test.go");
    std::fs::write(&file, "package main\n\nfunc main() {}\n\nfunc TestA(t *testing.T) {}\n").unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    fake.set_targets(vec![target(4, TargetKind::Test, "TestA"), target(2, TargetKind::Main, "main")]);
    fake.set_context(Some(FakeProvider::configuration("TestA")));
    fake.set_script(
        &[
            (Stream::Stdout, "@start app TestA"),
            (Stream::Stdout, "main_test.go:5: checked"),
            (Stream::Stdout, "@pass app TestA 2"),
            (Stream::Stdout, "@pass app - 3"),
        ],
        0,
    );

    let targets = registry.dispatch(&mut session, "run.targets.list", json!({ "path": file, "full": true })).unwrap();
    let listed: Vec<(Value, Value, Value)> = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| (target["line"].clone(), target["kind"].clone(), target["name"].clone()))
        .collect();
    assert_eq!(listed, [(json!(3), json!("main"), json!("main")), (json!(5), json!("test"), json!("TestA"))]);
    assert_eq!(targets["targets"][1]["provider"], "fake");

    let context = registry.dispatch(&mut session, "run.context.get", json!({})).unwrap();
    assert_eq!(context["configuration"]["name"], "TestA");
    assert_eq!(context["existing"], Value::Null);

    let started = registry.dispatch(&mut session, "run.process.start", json!({ "context": {} })).unwrap();
    let run_id = started["runId"].as_u64().unwrap();
    let output = registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id, "waitMs": 1000 })).unwrap();
    assert_eq!(output["state"], "finished");
    assert_eq!(output["exitCode"], 0);
    let lines = output["lines"].as_array().unwrap();
    assert_eq!(lines[0]["text"], "fake TestA");
    assert_eq!(lines[1]["text"], "main_test.go:5: checked");
    assert_eq!(lines[1]["links"][0]["line"], 5);
    let tests = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id })).unwrap();
    assert_eq!(tests["done"], true);
    assert_eq!(tests["counts"]["passed"], 1);
    assert_eq!(tests["tree"]["children"][0]["children"][0]["test"], "TestA");
    assert_eq!(tests["tree"]["children"][0]["children"][0]["status"], "passed");

    let context = registry.dispatch(&mut session, "run.context.get", json!({})).unwrap();
    assert!(context["existing"].is_string(), "the context's temporary configuration is saved");
    let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
    assert_eq!(listed["selected"], context["existing"]);

    let gutter = json!({ "configuration": targets["targets"][0]["configuration"] });
    let from_gutter = registry.dispatch(&mut session, "run.process.start", gutter).unwrap();
    assert_ne!(from_gutter["runId"], started["runId"]);
    let runs = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    assert_eq!(runs["runs"][1]["name"], "main");
    assert_eq!(fake.replays(), 2);
}

#[cfg(unix)]
#[test]
fn a_gutter_icon_and_the_caret_run_one_go_configuration() {
    let (registry, mut session, dir, _fake) = gogarden();
    let env = RunEnv { tool: std::path::PathBuf::from("/bin/echo"), env: Vec::new() };
    session.state_mut::<zen_workbench::run::Runs>().set_providers(vec![Arc::new(zen_lang_go::GoRun)], env);
    std::fs::create_dir_all(dir.path().join("calc")).unwrap();
    let file = dir.path().join("calc/calc_test.go");
    let text = "package calc\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n\tt.Log(\"x\")\n}\n";
    std::fs::write(&file, text).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();

    let targets = registry.dispatch(&mut session, "run.targets.list", json!({ "path": file, "full": true })).unwrap();
    let target = targets["targets"][0].clone();
    assert_eq!((target["line"].clone(), target["name"].clone()), (json!(5), json!("TestAdd")));
    let gutter = json!({ "context": { "path": file, "offset": "package calc\n\nimport \"testing\"\n\n".len() } });
    registry.dispatch(&mut session, "run.process.start", gutter).unwrap();
    let caret = json!({ "context": { "path": file, "offset": text.find("t.Log").unwrap() } });
    registry.dispatch(&mut session, "run.process.start", caret).unwrap();

    let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
    let configurations = listed["configurations"].as_array().unwrap();
    assert_eq!(configurations.len(), 1, "both run one temporary configuration: {configurations:?}");
    let kept = &configurations[0];
    assert_eq!(kept["temporary"], true);
    assert_eq!(kept["options"], target["configuration"]["options"], "the gutter's target is that configuration");
    assert_eq!(kept["options"]["package"], "./calc");
    assert_eq!(listed["selected"], kept["id"]);
    let runs = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    assert!(runs["runs"].as_array().unwrap().iter().all(|run| run["configurationId"] == kept["id"]));
}

#[cfg(unix)]
#[test]
fn a_real_go_program_is_built_run_and_its_program_removed_when_go_is_installed() {
    let Some(go) = ["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go", "/usr/bin/go"]
        .map(std::path::PathBuf::from)
        .into_iter()
        .find(|candidate| candidate.is_file())
    else {
        return;
    };
    let (registry, mut session, dir, _fake) = gogarden();
    let cache = tempfile::tempdir().unwrap();
    let folder = |name: &str| cache.path().join(name).to_string_lossy().into_owned();
    let env = [
        ("GOPATH", folder("gopath")),
        ("GOCACHE", folder("gocache")),
        ("GOMODCACHE", folder("modcache")),
        ("GOFLAGS", "-mod=mod".to_owned()),
        ("GOPROXY", "off".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOTELEMETRY", "off".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .to_vec();
    let providers: Vec<Arc<dyn zen_run::RunProvider>> = vec![Arc::new(zen_lang_go::GoRun)];
    session.state_mut::<zen_workbench::run::Runs>().set_providers(providers, RunEnv { tool: go, env });
    std::fs::write(dir.path().join("go.mod"), "module example.com/hello\n\ngo 1.21\n").unwrap();
    std::fs::create_dir_all(dir.path().join("cmd/hello")).unwrap();
    let main = dir.path().join("cmd/hello/main.go");
    std::fs::write(&main, "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hello from go\")\n}\n")
        .unwrap();

    let started = registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).unwrap();
    let run_id = started["runId"].as_u64().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let output = loop {
        let params = json!({ "runId": run_id, "waitMs": 1000 });
        let output = registry.dispatch(&mut session, "run.output.get", params).unwrap();
        if output["state"] != "running" {
            break output;
        }
        assert!(std::time::Instant::now() < deadline, "the build does not end");
    };
    let texts: Vec<&str> =
        output["lines"].as_array().unwrap().iter().map(|line| line["text"].as_str().unwrap()).collect();
    assert_eq!(output["exitCode"], 0, "{texts:?}");
    assert!(texts.contains(&"hello from go"), "{texts:?}");
    let program =
        session.state::<zen_workbench::run::Runs>().unwrap().run(run_id).unwrap().command_lines[1].program.clone();
    let folder = program.parent().unwrap();
    assert!(
        folder.parent() == Some(std::env::temp_dir().as_path())
            && folder.file_name().unwrap().to_string_lossy().starts_with("zen-run-"),
        "{}",
        program.display()
    );
    assert!(!program.exists() && !program.parent().unwrap().exists(), "the run's folder is removed");
}
