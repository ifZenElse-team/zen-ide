use std::path::Path;

use zen_run::{Configuration, RunContext, RunProvider, TargetKind, TestId};

use super::*;

fn fake_node(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join("node");
    std::fs::write(&path, "#!/bin/sh\necho node \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn context(root: &Path) -> RunContext {
    RunContext { root: root.to_owned(), scratch: root.join("scratch"), ..RunContext::default() }
}

fn project(runner: &str, entry: &str) -> (tempfile::TempDir, NodeRun) {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let bin = fake_node(dir.path());
    let provider = NodeRun::in_folders(vec![bin]);
    std::fs::write(dir.path().join("package.json"), format!(r#"{{ "devDependencies": {{ "{runner}": "*" }} }}"#))
        .unwrap();
    let entry = dir.path().join("node_modules").join(entry);
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(entry, "").unwrap();
    (dir, provider)
}

#[test]
fn a_file_runs_with_node_in_its_folder_with_options_and_arguments() {
    let (dir, provider) = project("jest", "jest/bin/jest.js");
    let mut configuration = file_configuration("src/main.ts");
    configuration.options["nodeOptions"] = serde_json::json!("--trace-warnings");
    configuration.common.program_arguments = "--port 'a b'".to_owned();
    let lines = provider.command_lines(&configuration, &context(dir.path())).unwrap();
    assert_eq!(lines[0].program, dir.path().join("bin/node"));
    let file = dir.path().join("src/main.ts").to_string_lossy().into_owned();
    assert_eq!(lines[0].args, ["--trace-warnings", file.as_str(), "--port", "a b"]);
    assert_eq!(lines[0].cwd, dir.path().join("src"));
    assert!(lines[0].env.iter().any(|(name, _)| name == "PATH"));
    assert_eq!(configuration.name, "main.ts");
}

#[test]
fn node_that_is_not_found_and_a_missing_file_say_so() {
    let (dir, _) = project("jest", "jest/bin/jest.js");
    let missing = NodeRun::in_folders(vec![dir.path().join("nowhere")]);
    let error = missing.command_lines(&file_configuration("a.js"), &context(dir.path())).unwrap_err();
    assert!(error.contains("node"), "{error}");
    let empty = Configuration { kind: KIND.to_owned(), ..Configuration::default() };
    assert!(missing.command_lines(&empty, &context(dir.path())).is_err());
}

#[test]
fn jest_runs_from_the_project_with_our_reporter_beside_its_own() {
    let (dir, provider) = project("jest", "jest/bin/jest.js");
    let configuration = test_configuration("adds", "src/a.test.ts", "^adds$");
    let context = context(dir.path());
    let lines = provider.command_lines(&configuration, &context).unwrap();
    let args = &lines[0].args;
    assert!(args[0].ends_with("node_modules/jest/bin/jest.js"), "{args:?}");
    assert_eq!(args[1], "--reporters=default");
    let reporter = args[2].strip_prefix("--reporters=").unwrap();
    assert!(std::fs::read_to_string(reporter).unwrap().contains("##zen-test"));
    let file = dir.path().join("src/a.test.ts").to_string_lossy().into_owned();
    assert_eq!(args[3..], ["--runTestsByPath".to_owned(), file, "-t".to_owned(), "^adds$".to_owned()]);
    assert_eq!(lines[0].cwd, dir.path());
}

#[test]
fn vitest_runs_once_with_our_reporter_and_a_runner_never_comes_from_the_network() {
    let (dir, provider) = project("vitest", "vitest/vitest.mjs");
    let configuration = test_configuration("a", "a.test.ts", "");
    let lines = provider.command_lines(&configuration, &context(dir.path())).unwrap();
    assert!(lines[0].args[0].ends_with("vitest/vitest.mjs"));
    assert_eq!(lines[0].args[1..3], ["run", "--reporter=default"]);
    assert!(lines[0].args.iter().any(|arg| arg.ends_with("zen-vitest-reporter.mjs")));

    let (empty, provider) = project("mocha", "mocha/bin/mocha.js");
    let error = provider.command_lines(&configuration, &context(empty.path())).unwrap_err();
    assert!(error.contains("node_modules"), "{error}");
    assert!(!error.contains("npx"));
}

#[test]
fn the_tests_of_a_test_file_are_targets_and_the_caret_runs_the_nearest() {
    let root = Path::new("/p");
    let text = "describe('math', () => {\n  it('adds', () => {\n    x();\n  });\n});\n";
    let provider = NodeRun::default();
    let targets = provider.targets(&context(root), &root.join("src/a.test.ts"), text);
    let found: Vec<(u32, &str, TargetKind)> = targets.iter().map(|t| (t.line, t.name.as_str(), t.kind)).collect();
    assert_eq!(found, [(0, "math", TargetKind::Test), (1, "math adds", TargetKind::Test)]);
    let options: NodeTestOptions = serde_json::from_value(targets[1].configuration.options.clone()).unwrap();
    assert_eq!(options.test_name, "^math adds$");
    assert_eq!(options.files, "'src/a.test.ts'");
    let suite: NodeTestOptions = serde_json::from_value(targets[0].configuration.options.clone()).unwrap();
    assert_eq!(suite.test_name, "^math( |$)");

    let offset = text.find("x()").unwrap();
    let at = provider.context(root, &root.join("src/a.test.ts"), text, offset).unwrap();
    assert_eq!((at.kind.as_str(), at.name.as_str()), (TEST_KIND, "adds"));
    let plain = provider.context(root, &root.join("src/main.js"), "run();", 0).unwrap();
    assert_eq!((plain.kind.as_str(), plain.name.as_str()), (KIND, "main.js"));
    assert!(provider.context(root, &root.join("src/a.css"), "", 0).is_none());
    assert!(provider.targets(&context(root), &root.join("src/main.ts"), text).is_empty());
}

#[test]
fn rerun_failed_names_the_failed_tests_exactly_in_their_files() {
    let provider = NodeRun::default();
    let configuration = test_configuration("all", "", "");
    let failed = [
        TestId { package: "src/a.test.ts".to_owned(), test: "math".to_owned() },
        TestId { package: "src/a.test.ts".to_owned(), test: "math/adds (fast)".to_owned() },
        TestId { package: "src/b.test.ts".to_owned(), test: "div\u{2215}zero".to_owned() },
    ];
    let rerun = provider.rerun_failed(&configuration, &failed).unwrap();
    let options: NodeTestOptions = serde_json::from_value(rerun.options).unwrap();
    assert_eq!(options.files, "'src/a.test.ts' 'src/b.test.ts'");
    assert_eq!(options.test_name, r"^(?:math adds \(fast\)|div\/zero)$");
    assert!(rerun.temporary);
    assert!(provider.rerun_failed(&file_configuration("a.js"), &failed).is_none());
    assert!(provider.rerun_failed(&configuration, &[]).is_none());
}

#[test]
fn a_debug_plan_uses_the_builtin_adapter_and_vitest_opens_one_worker_to_it() {
    let (dir, provider) = project("jest", "jest/bin/jest.js");
    let plan = provider.debug(&file_configuration("a.js"), &context(dir.path())).unwrap().unwrap();
    assert_eq!(plan.adapter.builtin.as_deref(), Some(adapter::ADAPTER));
    assert_eq!(plan.launch["program"], serde_json::json!(dir.path().join("bin/node")));
    let jest = provider.debug(&test_configuration("a", "a.test.ts", ""), &context(dir.path())).unwrap().unwrap();
    assert!(jest.launch["args"].as_array().unwrap().iter().any(|arg| arg == "--runInBand"));

    let (dir, provider) = project("vitest", "vitest/vitest.mjs");
    let vitest = provider.debug(&test_configuration("a", "a.test.ts", ""), &context(dir.path())).unwrap().unwrap();
    assert_eq!(vitest.launch["inspectChild"], true, "the worker listens, not the runner");
    let args: Vec<&str> = vitest.launch["args"].as_array().unwrap().iter().filter_map(|arg| arg.as_str()).collect();
    assert!(args.iter().any(|arg| arg.starts_with("--inspect-brk=127.0.0.1:")), "{args:?}");
    assert!(args.contains(&"--pool=forks") && args.contains(&"--poolOptions.forks.singleFork"), "{args:?}");
    assert!(args.contains(&"--no-file-parallelism"), "{args:?}");
    std::fs::write(dir.path().join("node_modules/vitest/package.json"), r#"{ "version": "4.1.0" }"#).unwrap();
    let four = provider.debug(&test_configuration("a", "a.test.ts", ""), &context(dir.path())).unwrap().unwrap();
    assert!(four.launch["args"].as_array().unwrap().iter().any(|arg| arg == "--maxWorkers=1"));
}

#[test]
fn in_safe_mode_neither_a_run_nor_a_debug_session_starts() {
    let (dir, provider) = project("jest", "jest/bin/jest.js");
    let root = dir.path().canonicalize().unwrap();
    let mut registry = zen_core::Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = zen_core::Session::new();
    session.set_state(crate::build_tools::BuildEnv::default());
    registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": root })).unwrap();
    session
        .state_mut::<crate::run::Runs>()
        .set_providers(vec![std::sync::Arc::new(provider)], zen_run::RunEnv::default());
    let configuration = serde_json::json!({ "kind": KIND, "name": "main.js", "options": { "file": "main.js" } });
    for command in ["run.process.start", "debug.session.start"] {
        let params = serde_json::json!({ "configuration": configuration });
        let refused = registry.dispatch(&mut session, command, params).unwrap_err();
        assert!(refused.to_string().contains("safe mode"), "{command}: {refused}");
    }
}

#[test]
fn it_belongs_to_the_nodejs_plugin() {
    assert_eq!(NodeRun::default().plugin(), Some(PLUGIN));
    let kinds: Vec<String> = NodeRun::default().kinds().into_iter().map(|kind| kind.id).collect();
    assert_eq!(kinds, [KIND, TEST_KIND]);
}
