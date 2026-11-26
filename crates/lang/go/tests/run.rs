use std::path::{Path, PathBuf};

use serde_json::json;
use zen_lang_go::GoRun;
use zen_run::{
    CommandLine, Configuration, LineStatus, Module, RunContext, RunEnv, RunOutput, RunProvider, RunState, Stream,
    TargetKind, TestCounts, TestId, TestNode, TestStatus,
};

const CALC_TEST: &str = include_str!("fixtures/run/calc/calc_test.go");
const CALC: &str = include_str!("fixtures/run/calc/calc.go");
const MAIN: &str = include_str!("fixtures/run/cmd/app/main.go");

fn root() -> PathBuf {
    PathBuf::from("/work/demo")
}

fn context() -> RunContext {
    RunContext {
        root: root(),
        env: RunEnv { tool: PathBuf::from("/sdk/go/bin/go"), env: vec![("GOROOT".to_owned(), "/sdk/go".to_owned())] },
        modules: vec![Module { name: "example.com/demo".to_owned(), root: root(), ..Module::default() }],
        scratch: PathBuf::new(),
        debugger: PathBuf::new(),
    }
}

fn configuration(kind: &str, options: serde_json::Value) -> Configuration {
    Configuration { name: "c".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() }
}

fn lines(configuration: &Configuration) -> Vec<CommandLine> {
    GoRun.command_lines(configuration, &context()).unwrap()
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn offset_of(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].chars().count()
}

#[test]
fn kinds_have_fields_and_defaults() {
    let kinds = GoRun.kinds();
    let ids: Vec<&str> = kinds.iter().map(|kind| kind.id.as_str()).collect();
    assert_eq!(ids, ["go.build", "go.test", "go.generate"]);
    assert_eq!(GoRun.id(), "go");
    assert_eq!(kinds[0].name_message, "go-run-kind-build");
    assert_eq!(kinds[0].defaults["runKind"], "package");
    assert_eq!(kinds[0].defaults["runAfterBuild"], true);
    assert_eq!(kinds[1].defaults["testKind"], "package");
    let keys: Vec<&str> = kinds[1].fields.iter().map(|field| field.key.as_str()).collect();
    assert_eq!(keys, ["testKind", "package", "directory", "files", "pattern", "bench"]);
    assert!(kinds.iter().flat_map(|kind| &kind.fields).all(|field| field.label_message.starts_with("go-run-")));
}

#[test]
fn every_field_label_and_choice_has_a_message() {
    zen_i18n::register(zen_lang_go::LOCALES);
    let shown = |id: &str| {
        let text = zen_i18n::t!(id);
        assert_ne!(text, id, "{id} has no message");
    };
    shown("go-run-tool-arguments");
    for kind in GoRun.kinds() {
        shown(&kind.name_message);
        for field in &kind.fields {
            shown(&field.label_message);
            if let zen_run::FieldKind::Choice(values) = &field.kind {
                for value in values {
                    shown(&format!("{}-{value}", field.label_message));
                }
            }
        }
    }
}

#[test]
fn test_files_have_targets_by_line() {
    let path = root().join("calc/calc_test.go");
    let targets = GoRun.targets(&context(), &path, CALC_TEST);
    let found: Vec<(u32, TargetKind, &str)> =
        targets.iter().map(|target| (target.line, target.kind, target.name.as_str())).collect();
    assert_eq!(
        found,
        [
            (7, TargetKind::Test, "TestAdd"),
            (13, TargetKind::Test, "TestTable"),
            (23, TargetKind::Benchmark, "BenchmarkAdd"),
            (29, TargetKind::Fuzz, "FuzzAdd"),
            (33, TargetKind::Example, "ExampleAdd"),
        ]
    );
    let add = &targets[0].configuration;
    assert_eq!(add.kind, "go.test");
    assert_eq!(add.name, "TestAdd (calc)");
    assert_eq!(add.options["package"], "./calc");
    assert_eq!(targets[0].package, "example.com/demo/calc");
    assert_eq!(add.options["pattern"], "^TestAdd$");
    assert_eq!(add.options["bench"], "");
    let bench = &targets[2].configuration;
    assert_eq!(
        (bench.options["pattern"].as_str(), bench.options["bench"].as_str()),
        (Some("^$"), Some("^BenchmarkAdd$"))
    );
    assert!(GoRun.targets(&context(), &root().join("calc/calc.go"), CALC).is_empty());
    assert!(GoRun.targets(&context(), &root().join("calc/notes.txt"), CALC_TEST).is_empty());
    for target in &targets {
        let line_start: usize =
            CALC_TEST.split('\n').take(target.line as usize).map(|line| line.chars().count() + 1).sum();
        let at_line = GoRun.context(&root(), &path, CALC_TEST, line_start);
        assert_eq!(at_line.as_ref(), Some(&target.configuration), "{} is what its line runs", target.name);
    }
    let outside = RunContext { modules: Vec::new(), ..context() };
    assert_eq!(GoRun.targets(&outside, &path, CALC_TEST)[0].package, "", "no module, no import path");
}

#[test]
fn main_has_a_build_target() {
    let targets = GoRun.targets(&context(), &root().join("cmd/app/main.go"), MAIN);
    assert_eq!(targets.len(), 1);
    assert_eq!((targets[0].line, targets[0].kind, targets[0].name.as_str()), (6, TargetKind::Main, "main"));
    let build = &targets[0].configuration;
    assert_eq!((build.kind.as_str(), build.name.as_str()), ("go.build", "go build app"));
    assert_eq!(build.options["package"], "./cmd/app");
    assert_eq!(targets[0].package, "example.com/demo/cmd/app");
    assert_eq!(build.options["runAfterBuild"], true);
}

#[test]
fn a_scratch_file_builds_alone_from_its_own_folder_and_runs() {
    let scratch = PathBuf::from("/data/Application Support/scratches/scratch.go");
    let targets = GoRun.single_file(&scratch, MAIN);
    assert_eq!(targets.len(), 1);
    assert_eq!((targets[0].kind, targets[0].name.as_str()), (TargetKind::Main, "main"));
    assert_eq!(targets[0].configuration.name, "go build scratch.go");
    assert!(GoRun.single_file(&scratch, "package lib\n\nfunc main() {}\n").is_empty(), "not package main");
    assert!(GoRun.single_file(&scratch.with_file_name("scratch_test.go"), CALC_TEST).is_empty());
    assert!(GoRun.single_file(&scratch.with_file_name("scratch.rs"), "fn main() {}\n").is_empty());

    let own = RunContext { scratch: PathBuf::from("/tmp/zen-run-1f"), ..context() };
    let lines = GoRun.command_lines(&targets[0].configuration, &own).unwrap();
    assert_eq!(lines.len(), 2);
    let binary = format!("/tmp/zen-run-1f/scratch{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(lines[0].program, PathBuf::from("/sdk/go/bin/go"), "the project's Go");
    assert_eq!(lines[0].args, strings(&["build", "-o", binary.as_str(), "scratch.go"]));
    assert_eq!(lines[0].cwd, PathBuf::from("/data/Application Support/scratches"), "outside the project's module");
    assert_eq!(lines[1].program, PathBuf::from(&binary));
    assert_eq!(lines[1].cwd, root());
}

#[test]
fn a_new_go_scratch_starts_as_a_program_with_the_caret_inside_main() {
    let starter = GoRun.scratch_starter("go").unwrap();
    assert_eq!(starter.text, "package main\n\nfunc main() {\n\t\n}\n");
    assert_eq!(&starter.text[starter.caret..], "\n}\n");
    assert_eq!(GoRun.scratch_starter("rust"), None);
}

fn scratch_lines(text: &str) -> (tempfile::TempDir, PathBuf, Vec<CommandLine>) {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.go");
    std::fs::write(&scratch, text).unwrap();
    let own = RunContext { scratch: dir.path().join("run"), ..context() };
    let configuration = GoRun.single_file(&scratch, text)[0].configuration.clone();
    let lines = GoRun.command_lines(&configuration, &own).unwrap();
    (dir, scratch, lines)
}

#[test]
fn a_scratch_with_only_the_standard_library_stays_outside_the_module() {
    let (_dir, _, lines) = scratch_lines("package main\n\nimport \"fmt\"\n\nfunc main() { fmt.Println() }\n");
    assert!(lines[0].args.iter().all(|arg| !arg.starts_with("-overlay")));
    assert_ne!(lines[0].cwd, root());
}

#[test]
fn a_scratch_importing_other_packages_is_overlaid_into_the_projects_module() {
    let cases = [
        "package main\n\nimport (\n\t\"fmt\"\n\tlog \"github.com/acme/log\"\n)\n\nfunc main() { fmt.Println(log.X) }\n",
        "package main\n\nimport \"example.com/demo/calc\"\n\nfunc main() { calc.Add(1, 2) }\n",
    ];
    for text in cases {
        let (dir, scratch, lines) = scratch_lines(text);
        assert_eq!(lines[0].cwd, root(), "resolved by the project's go.mod: {text}");
        let overlay = dir.path().join("run/overlay.json");
        assert!(lines[0].args.contains(&format!("-overlay={}", overlay.display())), "{:?}", lines[0].args);
        assert_eq!(lines[0].args.last().unwrap(), "./zen_scratch/scratch");
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&overlay).unwrap()).unwrap();
        let replaced = &written["Replace"]["/work/demo/zen_scratch/scratch/scratch.go"];
        assert_eq!(replaced.as_str(), scratch.to_str());
    }
}

#[test]
fn a_debug_build_of_a_scratch_takes_the_same_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.go");
    let text = "package main\n\nimport \"example.com/demo/calc\"\n\nfunc main() { calc.Add(1, 2) }\n";
    std::fs::write(&scratch, text).unwrap();
    let own = RunContext { scratch: dir.path().join("run"), ..context() };
    let configuration = GoRun.single_file(&scratch, text)[0].configuration.clone();
    let plan = GoRun.debug(&configuration, &own).unwrap().unwrap();
    assert_eq!(plan.build[0].cwd, root());
    assert!(plan.build[0].args.iter().any(|arg| arg.starts_with("-overlay=")), "{:?}", plan.build[0].args);
    assert_eq!(plan.build[0].args.last().unwrap(), "./zen_scratch/scratch");
}

#[test]
fn a_scratch_borrows_no_module_when_the_project_gives_none() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.go");
    let text = "package main\n\nimport \"github.com/acme/log\"\n\nfunc main() { log.X() }\n";
    std::fs::write(&scratch, text).unwrap();
    let safe = RunContext { modules: Vec::new(), ..context() };
    let lines = GoRun.command_lines(&GoRun.single_file(&scratch, text)[0].configuration, &safe).unwrap();
    assert_eq!(lines[0].cwd, dir.path(), "a project not trusted lends nothing");
}

#[test]
#[ignore = "builds a scratch against a project package with the real go on PATH"]
fn a_scratch_uses_a_project_package_with_the_real_go() {
    let Some(go) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|folder| folder.join("go"))
        .find(|go| go.is_file())
    else {
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let project = data.path().join("project");
    std::fs::create_dir_all(project.join("lib")).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/p\n\ngo 1.21\n").unwrap();
    std::fs::write(project.join("lib/lib.go"), "package lib\n\nfunc Hi() string { return \"hi from lib\" }\n").unwrap();
    let scratches = data.path().join("scratches");
    std::fs::create_dir_all(&scratches).unwrap();
    let scratch = scratches.join("scratch.go");
    let text = "package main\n\nimport (\n\t\"fmt\"\n\n\t\"example.com/p/lib\"\n)\n\nfunc main() {\n\tfmt.Println(lib.Hi())\n}\n";
    std::fs::write(&scratch, text).unwrap();
    let context = RunContext {
        root: project.clone(),
        env: RunEnv { tool: go, env: Vec::new() },
        modules: vec![Module { name: "example.com/p".to_owned(), root: project.clone(), ..Module::default() }],
        scratch: data.path().join("zen-run-real"),
        debugger: PathBuf::new(),
    };
    let configuration = GoRun.single_file(&scratch, text)[0].configuration.clone();
    let mut output = String::new();
    for line in GoRun.command_lines(&configuration, &context).unwrap() {
        let result =
            std::process::Command::new(&line.program).args(&line.args).current_dir(&line.cwd).output().unwrap();
        output.push_str(&String::from_utf8_lossy(&result.stdout));
        assert!(result.status.success(), "{}: {}", line.display, String::from_utf8_lossy(&result.stderr));
    }
    assert_eq!(output, "hi from lib\n");
    assert!(!project.join("zen_scratch").exists(), "the project stays as it was");
}

#[test]
fn a_scratch_has_its_home_where_its_run_overlays_it() {
    let (dir, scratch, lines) =
        scratch_lines("package main\n\nimport \"example.com/demo/calc\"\n\nfunc main() { calc.Add(1, 2) }\n");
    let text = std::fs::read_to_string(&scratch).unwrap();
    let home = GoRun.scratch_home(&scratch, &text, &context()).unwrap();
    let overlay: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("run/overlay.json")).unwrap()).unwrap();
    assert_eq!(overlay["Replace"][home.to_str().unwrap()].as_str(), scratch.to_str());
    assert_eq!(lines[0].args.last().unwrap(), "./zen_scratch/scratch");
}

#[test]
fn a_scratch_of_the_standard_library_or_another_language_has_no_home() {
    let context = context();
    let standard = "package main\n\nimport \"fmt\"\n\nfunc main() { fmt.Println() }\n";
    assert_eq!(GoRun.scratch_home(Path::new("/s/scratch.go"), standard, &context), None);
    let imports = "package main\n\nimport \"example.com/demo/calc\"\n";
    assert_eq!(GoRun.scratch_home(Path::new("/s/scratch.txt"), imports, &context), None);
    assert_eq!(GoRun.scratch_home(Path::new("/work/demo/in/module.go"), imports, &context), None);
}

#[test]
fn every_scratch_has_a_package_of_its_own_with_a_name_the_go_tools_accept() {
    let context = context();
    let imports = "package main\n\nimport \"example.com/demo/calc\"\n";
    let home = |name: &str| GoRun.scratch_home(&Path::new("/s").join(name), imports, &context).unwrap();
    assert_eq!(home("scratch.go"), Path::new("/work/demo/zen_scratch/scratch/scratch.go"));
    assert_eq!(home("scratch_1.go"), Path::new("/work/demo/zen_scratch/scratch_1/scratch_1.go"));
    assert_eq!(home("_my try.v2.go"), Path::new("/work/demo/zen_scratch/my_try_v2/_my try.v2.go"));
}

#[test]
#[ignore = "builds and runs a scratch file with the real go on PATH"]
fn a_scratch_file_runs_with_the_real_go() {
    let Some(go) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|folder| folder.join("go"))
        .find(|go| go.is_file())
    else {
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let scratches = data.path().join("Application Support/scratches");
    std::fs::create_dir_all(&scratches).unwrap();
    let scratch = scratches.join("scratch.go");
    let text = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hi from go\")\n}\n";
    std::fs::write(&scratch, text).unwrap();
    std::fs::write(scratches.join("scratch_1.go"), "package main\n\nfunc main() {}\n").unwrap();
    let project = data.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/p\n\ngo 1.21\n").unwrap();
    let context = RunContext {
        root: project,
        env: RunEnv { tool: go, env: Vec::new() },
        scratch: data.path().join("zen-run-real"),
        ..RunContext::default()
    };
    let configuration = GoRun.single_file(&scratch, text)[0].configuration.clone();
    let mut output = String::new();
    for line in GoRun.command_lines(&configuration, &context).unwrap() {
        let result = std::process::Command::new(&line.program)
            .args(&line.args)
            .current_dir(&line.cwd)
            .envs(line.env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
            .output()
            .unwrap();
        output.push_str(&String::from_utf8_lossy(&result.stdout));
        assert!(result.status.success(), "{}: {}", line.display, String::from_utf8_lossy(&result.stderr));
    }
    assert_eq!(output, "hi from go\n");
}

#[test]
fn the_context_is_the_test_the_package_or_the_program() {
    let test_path = root().join("calc/calc_test.go");
    let inside = GoRun.context(&root(), &test_path, CALC_TEST, offset_of(CALC_TEST, "t.Fatal")).unwrap();
    assert_eq!(inside.options["pattern"], "^TestAdd$");
    assert_eq!(inside.options["package"], "./calc");
    let on_name = GoRun.context(&root(), &test_path, CALC_TEST, offset_of(CALC_TEST, "func ExampleAdd")).unwrap();
    assert_eq!(on_name.options["pattern"], "^ExampleAdd$");
    let helper = GoRun.context(&root(), &test_path, CALC_TEST, offset_of(CALC_TEST, "helper")).unwrap();
    assert_eq!((helper.name.as_str(), helper.options["pattern"].as_str()), ("go test calc", Some("")));
    let program = GoRun.context(&root(), &root().join("cmd/app/main.go"), MAIN, offset_of(MAIN, "\"hello\"")).unwrap();
    assert_eq!((program.kind.as_str(), program.options["package"].as_str()), ("go.build", Some("./cmd/app")));
    assert_eq!(GoRun.context(&root(), &root().join("calc/calc.go"), CALC, 0), None);
    let at_root = GoRun.context(&root(), &root().join("main.go"), MAIN, 0).unwrap();
    assert_eq!(at_root.options["package"], ".");
    let unicode = "package main\n// привет\nfunc main() {}\n";
    assert!(GoRun.context(&root(), &root().join("u.go"), unicode, 14).is_some());
}

#[test]
fn test_command_lines_per_kind() {
    let package = configuration("go.test", json!({ "package": "./calc", "pattern": "^TestAdd$" }));
    let line = &lines(&package)[0];
    assert_eq!(line.program, PathBuf::from("/sdk/go/bin/go"));
    assert_eq!(line.args, strings(&["test", "-json", "-run", "^TestAdd$", "."]));
    assert_eq!(line.cwd, PathBuf::from("/work/demo/calc"));
    assert_eq!(line.env, [("GOROOT".to_owned(), "/sdk/go".to_owned())]);
    assert_eq!(line.display, "/sdk/go/bin/go test -json -run '^TestAdd$' . #gosetup");

    let mut full = configuration(
        "go.test",
        json!({ "testKind": "package", "package": "example.com/demo/calc", "pattern": "^$", "bench": "." }),
    );
    full.common.tool_arguments = "-count=1 -race".to_owned();
    full.common.program_arguments = "-flag 'two words'".to_owned();
    full.common.env = vec![("CGO_ENABLED".to_owned(), "0".to_owned())];
    let line = &lines(&full)[0];
    assert_eq!(
        line.args,
        strings(&[
            "test",
            "-json",
            "-run",
            "^$",
            "-bench",
            ".",
            "-benchmem",
            "-count=1",
            "-race",
            "example.com/demo/calc",
            "-args",
            "-flag",
            "two words"
        ])
    );
    assert_eq!(line.cwd, root());
    assert_eq!(line.env.last(), Some(&("CGO_ENABLED".to_owned(), "0".to_owned())));

    let directory = configuration("go.test", json!({ "testKind": "directory", "directory": "calc" }));
    let line = &lines(&directory)[0];
    assert_eq!(
        (line.args.last().map(String::as_str), line.cwd.as_path()),
        (Some("./..."), Path::new("/work/demo/calc"))
    );

    let files = configuration("go.test", json!({ "testKind": "file", "files": "calc/calc_test.go calc/calc.go" }));
    let line = &lines(&files)[0];
    assert_eq!(line.args, strings(&["test", "-json", "calc_test.go", "calc.go"]));
    assert_eq!(line.cwd, PathBuf::from("/work/demo/calc"));

    let std = configuration("go.test", json!({ "package": "net/http" }));
    assert_eq!(lines(&std)[0].cwd, root());
}

#[test]
fn build_command_lines_build_then_run() {
    let mut app = configuration("go.build", json!({ "package": "./cmd/app" }));
    app.common.program_arguments = "--port 8080".to_owned();
    app.common.tool_arguments = "-race".to_owned();
    app.common.working_directory = Some(PathBuf::from("data"));
    let found = lines(&app);
    assert_eq!(found.len(), 2);
    let binary = std::env::temp_dir().join("zen-run").join(format!("app{}", std::env::consts::EXE_SUFFIX));
    let binary_text = binary.to_string_lossy().into_owned();
    assert_eq!(found[0].args, strings(&["build", "-o", &binary_text, "-race", "."]));
    assert_eq!(found[0].cwd, PathBuf::from("/work/demo/cmd/app"));
    assert!(found[0].display.ends_with(" #gosetup"));
    assert_eq!(found[1].program, binary);
    assert_eq!(found[1].args, strings(&["--port", "8080"]));
    assert_eq!(found[1].cwd, PathBuf::from("/work/demo/data"));
    assert_eq!(found[1].env, found[0].env);

    let only =
        configuration("go.build", json!({ "runKind": "directory", "directory": "cmd/app", "runAfterBuild": false }));
    let found = lines(&only);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].args, strings(&["build", "."]));

    let kept =
        configuration("go.build", json!({ "runKind": "file", "files": "cmd/app/main.go", "outputDirectory": "bin" }));
    let found = lines(&kept);
    let out = format!("/work/demo/bin/main{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(found[0].args, strings(&["build", "-o", &out, "main.go"]));
    assert_eq!(found[1].program, PathBuf::from(&out));

    let own = RunContext { scratch: PathBuf::from("/tmp/zen-run/7-3"), ..context() };
    let found = GoRun.command_lines(&app, &own).unwrap();
    let program = PathBuf::from(format!("/tmp/zen-run/7-3/app{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(found[0].args[2], program.to_string_lossy(), "a run builds into its own folder");
    assert_eq!(found[1].program, program);

    let import = configuration("go.build", json!({ "package": "example.com/demo/cmd/app" }));
    let found = lines(&import);
    assert_eq!(found[0].cwd, root());
    assert_eq!(found[0].args.last().map(String::as_str), Some("example.com/demo/cmd/app"));
}

#[test]
fn configurations_that_cannot_run_say_why() {
    let mut no_go = context();
    no_go.env.tool = PathBuf::new();
    let test = configuration("go.test", json!({ "package": "./calc" }));
    assert!(GoRun.command_lines(&test, &no_go).is_err());
    assert!(GoRun.command_lines(&configuration("go.test", json!({})), &context()).is_err());
    assert!(GoRun.command_lines(&configuration("go.test", json!({ "testKind": "file" })), &context()).is_err());
    assert!(GoRun.command_lines(&configuration("go.test", json!({ "testKind": "odd" })), &context()).is_err());
    assert!(GoRun.command_lines(&configuration("go.vet", json!({})), &context()).is_err());
}

fn run(fixture: &str, configuration: &Configuration) -> RunOutput {
    let mut output = RunOutput::new(GoRun.parser(configuration, &context()));
    for line in fixture.lines() {
        output.feed(Stream::Stdout, line);
    }
    output.end(RunState::Finished, Some(1));
    output
}

fn package_tests() -> Configuration {
    configuration("go.test", json!({ "package": "./calc" }))
}

fn find<'a>(node: &'a TestNode, test: &str) -> &'a TestNode {
    fn walk<'a>(node: &'a TestNode, test: &str) -> Option<&'a TestNode> {
        if node.test.as_deref() == Some(test) {
            return Some(node);
        }
        node.children.iter().find_map(|child| walk(child, test))
    }
    walk(node, test).unwrap_or_else(|| panic!("no {test}"))
}

fn texts(output: &RunOutput, node: &TestNode) -> Vec<String> {
    node.lines.iter().flat_map(|range| range.clone()).map(|line| output.console().lines()[line].text.clone()).collect()
}

#[test]
fn passing_failing_skipped_subtests_and_parallel_tests() {
    let output = run(include_str!("fixtures/test2json/tests.jsonl"), &package_tests());
    let tree = output.tree();
    assert_eq!(tree.counts(), TestCounts { total: 10, running: 0, passed: 6, failed: 3, skipped: 1, terminated: 0 });
    let package = &tree.root().children[0];
    assert_eq!((package.name.as_str(), package.status), ("example.com/demo/calc", TestStatus::Failed));
    assert_eq!(package.duration_ms, Some(609));
    assert_eq!(find(package, "TestTable/two_words").status, TestStatus::Failed);
    assert_eq!(find(package, "TestParallel/b").status, TestStatus::Passed);
    assert_eq!(find(package, "TestSkipped").status, TestStatus::Skipped);
    assert_eq!(find(package, "ExampleAdd").status, TestStatus::Passed);
    let wrong = find(package, "TestWrong");
    assert_eq!(
        texts(&output, wrong),
        [
            "=== RUN   TestWrong",
            "    calc_test.go:15: checking",
            "    calc_test.go:17: Add(2, 2) = 4, want 5",
            "--- FAIL: TestWrong (0.00s)"
        ]
    );
    let error = &output.console().lines()[wrong.lines[0].start + 2];
    assert_eq!(error.stream, Stream::Stderr);
    assert_eq!(error.links[0].file, PathBuf::from("/work/demo/calc/calc_test.go"));
    assert_eq!(error.links[0].line, 17);
    let failed: Vec<String> = tree.failed().into_iter().map(|id| id.test).collect();
    assert_eq!(failed, ["TestWrong", "TestTable", "TestTable/two_words"]);
    assert_eq!(
        output.console().lines().last().map(|line| line.text.as_str()),
        Some("FAIL\texample.com/demo/calc\t0.608s")
    );
}

#[test]
fn a_panic_fails_the_test_with_links_to_the_stack() {
    let output = run(include_str!("fixtures/test2json/panic.jsonl"), &package_tests());
    let package = &output.tree().root().children[0];
    let boom = find(package, "TestBoom");
    assert_eq!(boom.status, TestStatus::Failed);
    let frame = texts(&output, boom).into_iter().position(|text| text.contains("p_test.go:7")).unwrap();
    let line = &output.console().lines()[boom.lines[0].start + frame];
    assert_eq!(line.links[0].file, PathBuf::from("/work/demo/panicky/p_test.go"));
    assert_eq!(line.links[0].line, 7);
}

#[test]
fn a_build_failure_fails_the_package_and_keeps_links() {
    let output = run(
        include_str!("fixtures/test2json/build-fail.jsonl"),
        &configuration("go.test", json!({ "package": "example.com/demo/broken" })),
    );
    let tree = output.tree();
    assert_eq!(tree.root().status, TestStatus::Failed);
    let package = &tree.root().children[0];
    assert_eq!((package.name.as_str(), package.status), ("example.com/demo/broken", TestStatus::Failed));
    assert_eq!(tree.counts().total, 0);
    let error = output.console().lines().iter().find(|line| line.text.contains("undefined")).unwrap();
    assert_eq!(error.stream, Stream::Stderr);
    assert_eq!(error.links[0].file, PathBuf::from("/work/demo/broken/b.go"));
    assert_eq!((error.links[0].line, error.links[0].column), (3, Some(23)));
    assert_eq!(texts(&output, package)[0], "# example.com/demo/broken [example.com/demo/broken.test]");
}

#[test]
fn benchmarks_examples_and_packages_without_tests() {
    let bench = run(include_str!("fixtures/test2json/bench.jsonl"), &package_tests());
    let package = &bench.tree().root().children[0];
    let sum = find(package, "BenchmarkSum");
    assert_eq!(sum.status, TestStatus::Passed);
    assert!(texts(&bench, sum).iter().any(|text| text.contains("ns/op")));
    assert_eq!(bench.console().lines()[0].text, "goos: darwin");

    let example = run(include_str!("fixtures/test2json/example.jsonl"), &package_tests());
    let wrong = find(&example.tree().root().children[0], "ExampleWrong");
    assert_eq!(wrong.status, TestStatus::Failed);
    assert_eq!(texts(&example, wrong)[2..], ["got:", "got this", "want:", "want that"]);

    let none = run(include_str!("fixtures/test2json/no-test-files.jsonl"), &package_tests());
    assert_eq!(none.tree().root().status, TestStatus::Skipped);

    let files = run(include_str!("fixtures/test2json/files.jsonl"), &package_tests());
    assert_eq!(files.tree().root().children[0].name, "command-line-arguments");
    assert_eq!(files.tree().counts().passed, 1);
}

#[test]
fn lines_that_are_not_json_pass_through() {
    let mut parser = GoRun.parser(&package_tests(), &context());
    let parsed = parser.line(Stream::Stderr, "go: cannot find main module; see 'go help modules'");
    assert_eq!(
        parsed.console.map(|line| (line.stream, line.text)),
        Some((Stream::Stderr, "go: cannot find main module; see 'go help modules'".to_owned()))
    );
    assert!(parsed.events.is_empty());
    let broken = parser.line(Stream::Stdout, "{not json");
    assert_eq!(broken.console.map(|line| line.text), Some("{not json".to_owned()));
    let relative = parser.line(Stream::Stderr, "./x.go:3:1: oops");
    assert_eq!(relative.console.unwrap().links[0].file, PathBuf::from("/work/demo/calc/x.go"));
    assert!(parser.finish().is_empty());
}

#[test]
fn a_test_still_running_when_its_package_ends_takes_the_package_status() {
    let lines = [
        r#"{"Action":"start","Package":"p"}"#,
        r#"{"Action":"run","Package":"p","Test":"TestHang"}"#,
        r#"{"Action":"output","Package":"p","Test":"TestHang","Output":"panic: test timed out after 1s\n"}"#,
        r#"{"Action":"fail","Package":"p","Elapsed":1.0004}"#,
    ];
    let output = run(&lines.join("\n"), &package_tests());
    let package = &output.tree().root().children[0];
    assert_eq!(find(package, "TestHang").status, TestStatus::Failed);
    assert_eq!(package.duration_ms, Some(1000));
}

#[test]
fn rerunning_failed_tests_narrows_the_pattern() {
    let failed = |tests: &[&str]| -> Vec<TestId> {
        tests
            .iter()
            .map(|test| TestId { package: "example.com/demo/calc".to_owned(), test: (*test).to_owned() })
            .collect()
    };
    let mut original = package_tests();
    original.id = "go.test-1".to_owned();
    let rerun = GoRun.rerun_failed(&original, &failed(&["TestWrong", "TestTable", "TestTable/two_words"])).unwrap();
    assert_eq!(rerun.options["pattern"], "^(TestWrong|TestTable)$");
    assert_eq!(rerun.options["package"], "./calc");
    assert!(rerun.temporary);
    assert!(rerun.id.is_empty());
    let one = GoRun.rerun_failed(&original, &failed(&["TestTable", "TestTable/a.b"])).unwrap();
    assert_eq!(one.options["pattern"], r"^TestTable$/^a\.b$");
    assert_eq!(GoRun.rerun_failed(&original, &[]), None);
    let all = configuration("go.test", json!({ "testKind": "directory", "directory": "." }));
    let id = |package: &str| TestId { package: package.to_owned(), test: "TestWrong".to_owned() };
    let one = GoRun.rerun_failed(&all, &[id("example.com/calc")]).unwrap();
    assert_eq!((&one.options["testKind"], &one.options["package"]), (&json!("package"), &json!("example.com/calc")));
    let two = GoRun.rerun_failed(&all, &[id("example.com/calc"), id("example.com/other")]).unwrap();
    assert_eq!(two.options["testKind"], "directory", "tests of several packages keep the configuration's scope");
    assert_eq!(GoRun.rerun_failed(&original, &[id("example.com/calc")]).unwrap().options["package"], "./calc");
    assert_eq!(GoRun.rerun_failed(&configuration("go.build", json!({})), &failed(&["TestA"])), None);
}

#[test]
fn a_temporary_program_is_removed_when_the_run_ends() {
    let name = format!("zen-go-run-test-{}", std::process::id());
    let build = configuration("go.build", json!({ "package": format!("./{name}") }));
    let program = GoRun.command_lines(&build, &context()).unwrap()[1].program.clone();
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, "binary").unwrap();
    let mut parser = GoRun.parser(&build, &context());
    let line = parser.line(Stream::Stderr, "./main.go:4:2: declared and not used: x").console.unwrap();
    assert_eq!(line.links[0].file, PathBuf::from(format!("/work/demo/{name}/main.go")));
    parser.finish();
    assert!(!program.exists());
    let kept = configuration("go.build", json!({ "package": "./app", "outputDirectory": "bin" }));
    let mut parser = GoRun.parser(&kept, &context());
    parser.finish();
}

fn find_go() -> Option<PathBuf> {
    let candidates = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|folder| folder.join("go")).collect::<Vec<_>>())
        .unwrap_or_default();
    candidates
        .into_iter()
        .chain(["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go"].map(PathBuf::from))
        .find(|candidate| candidate.is_file())
}

#[test]
fn a_real_go_test_run_when_go_is_installed() {
    let Some(go) = find_go() else { return };
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("calc")).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/real\n\ngo 1.21\n").unwrap();
    std::fs::write(project.join("calc/calc.go"), "package calc\n\nfunc Add(a, b int) int { return a + b }\n").unwrap();
    std::fs::write(
        project.join("calc/calc_test.go"),
        "package calc\n\nimport \"testing\"\n\nfunc TestOk(t *testing.T) {\n\tt.Run(\"sub\", func(t *testing.T) {})\n}\n\nfunc TestBad(t *testing.T) {\n\tt.Errorf(\"got %d\", Add(1, 1))\n}\n",
    )
    .unwrap();
    let cache = |name: &str| temp.path().join(name).to_string_lossy().into_owned();
    let env: Vec<(String, String)> = [
        ("GOPATH", cache("gopath")),
        ("GOCACHE", cache("gocache")),
        ("GOMODCACHE", cache("modcache")),
        ("GOFLAGS", "-mod=mod".to_owned()),
        ("GOPROXY", "off".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOTELEMETRY", "off".to_owned()),
        ("GOENV", "off".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .into();
    let context = RunContext {
        root: project.clone(),
        env: RunEnv { tool: go, env },
        modules: vec![Module { name: "example.com/real".to_owned(), root: project.clone(), ..Module::default() }],
        scratch: temp.path().join("scratch"),
        debugger: PathBuf::new(),
    };
    let tests = configuration("go.test", json!({ "package": "./calc" }));
    let line = GoRun.command_lines(&tests, &context).unwrap().remove(0);
    let result = std::process::Command::new(&line.program)
        .args(&line.args)
        .current_dir(&line.cwd)
        .envs(line.env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap();
    let mut output = RunOutput::new(GoRun.parser(&tests, &context));
    for text in String::from_utf8_lossy(&result.stdout).lines() {
        output.feed(Stream::Stdout, text);
    }
    for text in String::from_utf8_lossy(&result.stderr).lines() {
        output.feed(Stream::Stderr, text);
    }
    output.end(RunState::Finished, result.status.code());
    let counts = output.tree().counts();
    assert_eq!((counts.passed, counts.failed), (2, 1), "{:?}", output.console().lines());
    let error = output.console().lines().iter().find(|line| line.text.contains("got 2")).unwrap();
    assert_eq!(error.links[0].file, project.join("calc/calc_test.go"));
}

#[test]
#[ignore = "needs Go: cargo test -p zen-lang-go --test run -- --ignored"]
fn a_real_go_test_run_with_coverage() {
    let go = find_go().expect("Go is installed");
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("calc")).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/real\n\ngo 1.21\n").unwrap();
    std::fs::write(
        project.join("calc/calc.go"),
        "package calc\n\nfunc Sign(a int) int {\n\tif a < 0 {\n\t\treturn -1\n\t}\n\treturn 1\n}\n\nfunc Unused() int {\n\treturn 0\n}\n",
    )
    .unwrap();
    std::fs::write(
        project.join("calc/calc_test.go"),
        "package calc\n\nimport \"testing\"\n\nfunc TestSign(t *testing.T) {\n\tif Sign(2) != 1 {\n\t\tt.Fail()\n\t}\n}\n",
    )
    .unwrap();
    let cache = |name: &str| temp.path().join(name).to_string_lossy().into_owned();
    let env: Vec<(String, String)> = [
        ("GOPATH", cache("gopath")),
        ("GOCACHE", cache("gocache")),
        ("GOMODCACHE", cache("modcache")),
        ("GOFLAGS", "-mod=mod".to_owned()),
        ("GOPROXY", "off".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOTELEMETRY", "off".to_owned()),
        ("GOENV", "off".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .into();
    let context = RunContext {
        root: project.clone(),
        env: RunEnv { tool: go, env },
        modules: vec![Module { name: "example.com/real".to_owned(), root: project.clone(), ..Module::default() }],
        scratch: temp.path().join("scratch"),
        debugger: PathBuf::new(),
    };
    let tests = configuration("go.test", json!({ "package": "./calc" }));
    let coverage = GoRun.coverage(&tests).expect("Go tests run with coverage");
    let report = temp.path().join("coverage/c.out");
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    let line = coverage.command_lines(&tests, &context, &report).unwrap().remove(0);
    let result = std::process::Command::new(&line.program)
        .args(&line.args)
        .current_dir(&line.cwd)
        .envs(line.env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    let read = coverage.read(&tests, &context, &report).unwrap();
    let file = read.file(&project.join("calc/calc.go")).expect("calc.go is measured");
    let status = |line: u32| file.line(line).map(|measured| measured.status);
    assert_eq!(status(3), None, "a declaration is not a statement");
    assert_eq!(status(4), Some(LineStatus::Covered));
    assert_eq!(status(5), Some(LineStatus::NotCovered));
    assert_eq!(status(6), None, "a block that ends at the first column leaves its last line out");
    assert_eq!(status(7), Some(LineStatus::Covered));
    assert_eq!(status(11), Some(LineStatus::NotCovered));
    assert_eq!(file.line(7).map(|measured| measured.hits), Some(1));
}

fn real_env(temp: &Path) -> Vec<(String, String)> {
    let cache = |name: &str| temp.join(name).to_string_lossy().into_owned();
    [
        ("GOPATH", cache("gopath")),
        ("GOCACHE", cache("gocache")),
        ("GOMODCACHE", cache("modcache")),
        ("GOFLAGS", "-mod=mod".to_owned()),
        ("GOPROXY", "off".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOTELEMETRY", "off".to_owned()),
        ("GOENV", "off".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .into()
}

fn execute(line: &CommandLine) -> std::process::Output {
    std::process::Command::new(&line.program)
        .args(&line.args)
        .current_dir(&line.cwd)
        .envs(line.env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap()
}

#[test]
#[ignore = "needs Go: cargo test -p zen-lang-go --test run -- --ignored"]
fn a_real_benchmark_reports_its_results_with_allocations() {
    let go = find_go().expect("Go is installed");
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/real\n\ngo 1.24\n").unwrap();
    std::fs::write(
        project.join("a_test.go"),
        "package a\n\nimport \"testing\"\n\nvar sink []byte\n\nfunc BenchmarkMake(b *testing.B) {\n\tb.Run(\"small\", func(b *testing.B) {\n\t\tfor b.Loop() {\n\t\t\tsink = make([]byte, 64)\n\t\t}\n\t})\n}\n",
    )
    .unwrap();
    let context = RunContext {
        root: project.clone(),
        env: RunEnv { tool: go, env: real_env(temp.path()) },
        modules: vec![Module { name: "example.com/real".to_owned(), root: project.clone(), ..Module::default() }],
        scratch: temp.path().join("scratch"),
        debugger: PathBuf::new(),
    };
    let mut benchmark = configuration("go.test", json!({ "package": ".", "pattern": "^$", "bench": "." }));
    benchmark.common.tool_arguments = "-benchtime=10x".to_owned();
    let line = GoRun.command_lines(&benchmark, &context).unwrap().remove(0);
    assert!(line.args.iter().any(|word| word == "-benchmem"), "{:?}", line.args);
    let result = execute(&line);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    let mut output = RunOutput::new(GoRun.parser(&benchmark, &context));
    for text in String::from_utf8_lossy(&result.stdout).lines() {
        output.feed(Stream::Stdout, text);
    }
    output.end(RunState::Finished, result.status.code());
    let node = find(output.tree().root(), "BenchmarkMake/small");
    assert_eq!(node.iterations, Some(10));
    let units: Vec<&str> = node.measures.iter().map(|measure| measure.unit.as_str()).collect();
    assert_eq!(units, ["ns/op", "B/op", "allocs/op"]);
}

#[test]
#[ignore = "needs Go: cargo test -p zen-lang-go --test run -- --ignored"]
fn a_real_go_generate_line_runs_only_that_directive_in_the_module() {
    let go = find_go().expect("Go is installed");
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("api")).unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/real\n\ngo 1.24\n").unwrap();
    let text = "package api\n\n//go:generate touch one(1).txt\n//go:generate touch two.txt\n";
    let path = project.join("api/gen.go");
    std::fs::write(&path, text).unwrap();
    let context = RunContext {
        root: project.clone(),
        env: RunEnv { tool: go, env: real_env(temp.path()) },
        modules: vec![Module { name: "example.com/real".to_owned(), root: project.clone(), ..Module::default() }],
        scratch: temp.path().join("scratch"),
        debugger: PathBuf::new(),
    };
    let targets = GoRun.targets(&context, &path, text);
    let two = targets.iter().find(|target| target.name == "touch two.txt").unwrap();
    assert_eq!(two.kind, TargetKind::Generate);
    let line = GoRun.command_lines(&two.configuration, &context).unwrap().remove(0);
    assert_eq!(line.cwd, project.join("api"));
    let result = execute(&line);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(project.join("api/two.txt").is_file());
    assert!(!project.join("api/one(1).txt").exists());
    let one = GoRun.targets(&context, &path, text).remove(0);
    let line = GoRun.command_lines(&one.configuration, &context).unwrap().remove(0);
    assert!(execute(&line).status.success());
    assert!(project.join("api/one(1).txt").is_file(), "brackets in a directive are matched as text");
}

const SPIN_TEST: &str = "package spin\n\nimport (\n\t\"testing\"\n\t\"time\"\n)\n\nfunc spin(until time.Time) int {\n\ttotal := 0\n\tfor time.Now().Before(until) {\n\t\ttotal++\n\t}\n\treturn total\n}\n\nvar sink []byte\n\nfunc TestSpin(t *testing.T) {\n\tif spin(time.Now().Add(300*time.Millisecond)) == 0 {\n\t\tt.Fatal(\"no\")\n\t}\n\tfor i := 0; i < 2000; i++ {\n\t\tsink = make([]byte, 1<<20)\n\t}\n}\n";

fn record(kind: zen_run::ProfileKind, go: &Path, folder: &Path) -> zen_run::Profile {
    let context = RunContext {
        root: folder.to_path_buf(),
        env: RunEnv {
            tool: go.to_path_buf(),
            env: vec![
                ("GOPATH".to_owned(), folder.join("gopath").to_string_lossy().into_owned()),
                ("GOCACHE".to_owned(), folder.join("gocache").to_string_lossy().into_owned()),
                ("GOFLAGS".to_owned(), "-mod=mod".to_owned()),
                ("GOPROXY".to_owned(), "off".to_owned()),
                ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
                ("GOTELEMETRY".to_owned(), "off".to_owned()),
                ("GOWORK".to_owned(), "off".to_owned()),
            ],
        },
        modules: vec![Module { name: "example.com/spin".to_owned(), root: folder.to_path_buf(), ..Module::default() }],
        ..RunContext::default()
    };
    let tests = configuration("go.test", json!({ "kind": "package", "package": "." }));
    let report = folder.join("out/profile.pb.gz");
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    let provider = GoRun.profiler(&tests).unwrap();
    let line = provider.command_lines(&tests, &context, kind, &report).unwrap().remove(0);
    let mut command = std::process::Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd).envs(line.env.iter().map(|(name, value)| (name, value)));
    let run = command.output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stdout));
    assert!(!folder.join("spin.test").exists(), "the test binary stays out of the project");
    provider.read(&tests, &context, &report).unwrap()
}

fn go_on_this_computer() -> Option<PathBuf> {
    ["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[test]
fn a_real_go_test_records_cpu_and_memory_profiles_we_can_read() {
    let Some(go) = go_on_this_computer() else { return };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap();
    std::fs::write(folder.join("go.mod"), "module example.com/spin\n\ngo 1.21\n").unwrap();
    std::fs::write(folder.join("spin_test.go"), SPIN_TEST).unwrap();

    let cpu = record(zen_run::ProfileKind::Cpu, &go, &folder);
    assert_eq!(cpu.metrics[cpu.default_metric].name, "cpu");
    let frame = cpu.frames.iter().find(|frame| frame.function.ends_with(".spin")).expect("spin was sampled");
    assert_eq!(frame.file.as_deref(), Some(folder.join("spin_test.go").as_path()));
    assert_eq!(frame.line, 8);
    assert!(cpu.total(cpu.default_metric) > 0);

    let memory = record(zen_run::ProfileKind::Memory, &go, &folder);
    let allocated = memory.metric_index("alloc_space").expect("a heap profile counts allocated bytes");
    assert!(memory.total(allocated) > 0);
    assert!(
        memory
            .functions(allocated)
            .iter()
            .any(|cost| memory.frames[cost.frame as usize].function.ends_with("TestSpin"))
    );
}
