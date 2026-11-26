mod adopt;
mod command;
mod coverage;
mod debug;
mod profile;
mod targets;
mod test2json;
mod verbose;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_run::{
    AnythingPrefix, BeforeLaunchTask, CommandLine, Configuration, CoverageProvider, DebugPlan, FieldKind, FieldSpec,
    ForeignRunConfiguration, ForeignTask, KindSpec, OutputParser, ProfileProvider, RunContext, RunProvider, RunTarget,
    ScratchStarter, TestId, TestSource,
};

pub use debug::explain_adapter_error;
pub use test2json::Test2JsonParser;
pub use verbose::VerboseParser;

pub const BUILD: &str = "go.build";

pub const TEST: &str = "go.test";

const GENERATE: &str = "go.generate";

const COMMAND_TASK: &str = "go.command";

const FOREIGN_COMMAND_TASK: &str = "GoBeforeRunTask";
const ANYTHING_COMMANDS: [&str; 7] =
    ["go run .", "go test ./...", "go build ./...", "go get", "go mod tidy", "go generate ./...", "go vet ./..."];

#[derive(Clone, Copy, Debug, Default)]
pub struct GoRun;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuildOptions {
    run_kind: String,
    package: String,
    files: String,
    directory: String,
    output_directory: String,
    run_after_build: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            run_kind: "package".to_owned(),
            package: String::new(),
            files: String::new(),
            directory: String::new(),
            output_directory: String::new(),
            run_after_build: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct GenerateOptions {
    file: String,
    directive: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TestOptions {
    test_kind: String,
    package: String,
    directory: String,
    files: String,
    pattern: String,
    bench: String,
}

impl Default for TestOptions {
    fn default() -> Self {
        Self {
            test_kind: "package".to_owned(),
            package: String::new(),
            directory: String::new(),
            files: String::new(),
            pattern: String::new(),
            bench: String::new(),
        }
    }
}

fn options<T: for<'de> Deserialize<'de> + Default>(configuration: &Configuration) -> T {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn choice(values: &[&str]) -> FieldKind {
    FieldKind::Choice(values.iter().map(|value| (*value).to_owned()).collect())
}

impl RunProvider for GoRun {
    fn id(&self) -> &str {
        "go"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![
            KindSpec {
                id: BUILD.to_owned(),
                name_message: "go-run-kind-build".to_owned(),
                fields: vec![
                    field("runKind", "go-run-field-run-kind", choice(&["package", "file", "directory"])),
                    field("package", "go-run-field-package", FieldKind::Text),
                    field("files", "go-run-field-files", FieldKind::Text),
                    field("directory", "go-run-field-directory", FieldKind::Path),
                    field("outputDirectory", "go-run-field-output-directory", FieldKind::Path),
                    field("runAfterBuild", "go-run-field-run-after-build", FieldKind::Bool),
                ],
                defaults: serde_json::to_value(BuildOptions::default()).unwrap_or_default(),
            },
            KindSpec {
                id: TEST.to_owned(),
                name_message: "go-run-kind-test".to_owned(),
                fields: vec![
                    field("testKind", "go-run-field-test-kind", choice(&["package", "directory", "file"])),
                    field("package", "go-run-field-package", FieldKind::Text),
                    field("directory", "go-run-field-directory", FieldKind::Path),
                    field("files", "go-run-field-files", FieldKind::Text),
                    field("pattern", "go-run-field-pattern", FieldKind::Text),
                    field("bench", "go-run-field-bench", FieldKind::Text),
                ],
                defaults: serde_json::to_value(TestOptions::default()).unwrap_or_default(),
            },
            KindSpec {
                id: GENERATE.to_owned(),
                name_message: "go-run-kind-generate".to_owned(),
                fields: vec![
                    field("file", "go-run-field-generate-file", FieldKind::Path),
                    field("directive", "go-run-field-generate-directive", FieldKind::Text),
                ],
                defaults: serde_json::to_value(GenerateOptions::default()).unwrap_or_default(),
            },
        ]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        targets::targets(context, path, text)
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        targets::context(root, path, text, offset)
    }

    fn single_file(&self, path: &Path, text: &str) -> Vec<RunTarget> {
        targets::single_file(path, text)
    }

    fn scratch_starter(&self, language: &str) -> Option<ScratchStarter> {
        (language == "go").then(|| ScratchStarter {
            text: "package main\n\nfunc main() {\n\t\n}\n".to_owned(),
            caret: "package main\n\nfunc main() {\n\t".chars().count(),
        })
    }

    fn scratch_home(&self, file: &Path, text: &str, context: &RunContext) -> Option<PathBuf> {
        command::scratch_home(file, text, context).map(|(_, home)| home)
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        match configuration.kind.as_str() {
            BUILD => command::build(configuration, &options(configuration), context),
            TEST => command::test(configuration, &options(configuration), context),
            GENERATE => command::generate(configuration, &options(configuration), context),
            other => Err(zen_i18n::t!("go-run-error-kind", kind = other)),
        }
    }

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        match configuration.kind.as_str() {
            TEST => {
                let test: TestOptions = options(configuration);
                let cwd = command::test_folder(&test, context).unwrap_or_else(|_| context.root.clone());
                let package = command::test_package(&test, context);
                let verbose = VerboseParser::new(package, cwd.clone(), command::package_folder(&test, context));
                Box::new(Test2JsonParser::new(cwd, &context.modules).with_verbose(verbose))
            }
            GENERATE => {
                Box::new(command::generate_parser(command::generate_folder(&options(configuration), context).0))
            }
            _ => Box::new(command::build_parser(&options(configuration), context)),
        }
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        match configuration.kind.as_str() {
            BUILD => Some(debug::build(configuration, &options(configuration), context)),
            TEST => Some(debug::test(configuration, &options(configuration), context)),
            _ => None,
        }
    }

    fn task_kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: COMMAND_TASK.to_owned(),
            name_message: "go-run-task-command".to_owned(),
            fields: vec![
                field("command", "go-run-task-field-command", FieldKind::Text),
                field("workingDirectory", "go-run-task-field-working-directory", FieldKind::Path),
            ],
            defaults: serde_json::json!({ "command": "generate ./...", "workingDirectory": "" }),
        }]
    }

    fn task_lines(&self, task: &BeforeLaunchTask, context: &RunContext) -> Option<Result<Vec<CommandLine>, String>> {
        (task.kind == COMMAND_TASK)
            .then(|| command::go_command(task.text("command"), task.text("workingDirectory"), context))
    }

    fn adopt_task(&self, foreign: &ForeignTask, root: &Path) -> Option<BeforeLaunchTask> {
        if foreign.id != FOREIGN_COMMAND_TASK {
            return None;
        }
        let option = |key: &str| foreign.options.get(key).cloned().unwrap_or_default();
        let folder = option("workingDirectory");
        let folder =
            Path::new(&folder).strip_prefix(root).map_or(folder.clone(), |relative| relative.display().to_string());
        Some(BeforeLaunchTask::new(
            COMMAND_TASK,
            serde_json::json!({ "command": option("command"), "workingDirectory": folder }),
        ))
    }

    fn explain_debug_error(&self, message: &str) -> Option<String> {
        explain_adapter_error(message)
    }

    fn adopt(&self, foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
        adopt::adopt(foreign, root)
    }

    fn coverage(&self, configuration: &Configuration) -> Option<&dyn CoverageProvider> {
        (configuration.kind == TEST).then_some(self as &dyn CoverageProvider)
    }

    fn profiler(&self, configuration: &Configuration) -> Option<&dyn ProfileProvider> {
        (configuration.kind == TEST).then_some(self as &dyn ProfileProvider)
    }

    fn rerun_failed(&self, configuration: &Configuration, failed: &[TestId]) -> Option<Configuration> {
        if configuration.kind != TEST {
            return None;
        }
        let names: Vec<&str> = failed.iter().map(|id| id.test.as_str()).collect();
        let (pattern, bench) = rerun_patterns(&names)?;
        let mut test: TestOptions = options(configuration);
        test.pattern = pattern;
        test.bench = bench;
        narrow_to_package(&mut test, failed);
        Some(Configuration {
            id: String::new(),
            temporary: true,
            options: serde_json::to_value(test).ok()?,
            ..configuration.clone()
        })
    }

    fn test_source(&self, configuration: &Configuration, test: &TestId, context: &RunContext) -> Option<TestSource> {
        if configuration.kind != TEST {
            return None;
        }
        let top = test.test.split('/').next()?;
        let folder = command::module_folder(&test.package, &context.modules)
            .map(|(_, folder)| folder)
            .unwrap_or_else(|| command::package_folder(&options(configuration), context));
        crate::test_support::source_of(&folder, top)
    }

    fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
        vec![AnythingPrefix {
            prefix: "go".to_owned(),
            help_message: "go-run-anything-help".to_owned(),
            commands: ANYTHING_COMMANDS.iter().map(|command| (*command).to_owned()).collect(),
        }]
    }

    fn anything(&self, words: &[String], cwd: &Path, context: &RunContext) -> Option<Result<Configuration, String>> {
        let (first, args) = words.split_first()?;
        if first != "go" {
            return None;
        }
        Some(command::tool(context).map(|go| {
            zen_run::external_tool::command_configuration(
                &zen_run::external_tool::joined(words),
                go,
                args.to_vec(),
                cwd,
                Vec::new(),
            )
        }))
    }
}

fn narrow_to_package(test: &mut TestOptions, tests: &[TestId]) {
    let spread = test.test_kind == "directory" || test.package.contains("...");
    let package = tests.first().map_or("", |id| id.package.as_str());
    if spread
        && !package.is_empty()
        && !command::is_folder_form(package)
        && tests.iter().all(|id| id.package == package)
    {
        test.test_kind = "package".to_owned();
        test.package = package.to_owned();
    }
}

pub fn quote_meta(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted
}

fn alternatives(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| quote_meta(name)).collect();
    match quoted.as_slice() {
        [single] => format!("^{single}$"),
        _ => format!("^({})$", quoted.join("|")),
    }
}

pub fn rerun_patterns(failed: &[&str]) -> Option<(String, String)> {
    let mut tops: Vec<&str> = Vec::new();
    for name in failed {
        let top = name.split('/').next().unwrap_or(name);
        if !top.is_empty() && !tops.contains(&top) {
            tops.push(top);
        }
    }
    if tops.is_empty() {
        return None;
    }
    let (benchmarks, tests): (Vec<&str>, Vec<&str>) = tops.iter().partition(|name| name.starts_with("Benchmark"));
    let bench = if benchmarks.is_empty() { String::new() } else { alternatives(&benchmarks) };
    let pattern = match tests.as_slice() {
        [] => "^$".to_owned(),
        [single] => {
            let leaves: Vec<&&str> = failed
                .iter()
                .filter(|name| name.split('/').next() == Some(single) && name.contains('/'))
                .filter(|name| !failed.iter().any(|other| other.starts_with(&format!("{name}/"))))
                .collect();
            match leaves.as_slice() {
                [leaf] => leaf.split('/').map(|part| format!("^{}$", quote_meta(part))).collect::<Vec<_>>().join("/"),
                _ => alternatives(&tests),
            }
        }
        _ => alternatives(&tests),
    };
    Some((pattern, bench))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(failed: &[&str]) -> Option<(String, String)> {
        rerun_patterns(failed)
    }

    #[test]
    fn failed_tests_become_patterns() {
        assert_eq!(patterns(&[]), None);
        assert_eq!(patterns(&["TestA", "TestB"]), Some(("^(TestA|TestB)$".to_owned(), String::new())));
        assert_eq!(patterns(&["TestA", "TestA/case_1"]), Some(("^TestA$/^case_1$".to_owned(), String::new())));
        assert_eq!(
            patterns(&["TestA", "TestA/x", "TestA/x/deep"]),
            Some(("^TestA$/^x$/^deep$".to_owned(), String::new()))
        );
        assert_eq!(patterns(&["TestA", "TestA/one", "TestA/two"]), Some(("^TestA$".to_owned(), String::new())));
        assert_eq!(patterns(&["TestA", "TestA/a+b(1)", "TestB"]), Some(("^(TestA|TestB)$".to_owned(), String::new())));
        assert_eq!(patterns(&["TestA", "TestA/a+b(1)"]), Some((r"^TestA$/^a\+b\(1\)$".to_owned(), String::new())));
        assert_eq!(patterns(&["BenchmarkSum"]), Some(("^$".to_owned(), "^BenchmarkSum$".to_owned())));
        assert_eq!(
            patterns(&["TestA", "BenchmarkX", "BenchmarkY"]),
            Some(("^TestA$".to_owned(), "^(BenchmarkX|BenchmarkY)$".to_owned()))
        );
    }

    #[test]
    fn run_anything_runs_go_commands_with_the_projects_go_in_the_chosen_folder() {
        zen_i18n::register(crate::LOCALES);
        let prefixes = GoRun.anything_prefixes();
        assert_eq!(prefixes[0].prefix, "go");
        for command in ["go run .", "go test ./...", "go build ./...", "go get", "go mod tidy", "go generate ./..."] {
            assert!(prefixes[0].commands.iter().any(|offered| offered == command), "{command}");
        }
        let words = |line: &str| zen_run::args::split(line);
        let context = RunContext {
            env: zen_run::RunEnv { tool: PathBuf::from("/opt/go/bin/go"), env: Vec::new() },
            ..RunContext::default()
        };
        assert!(GoRun.anything(&words("cargo build"), Path::new("/p"), &context).is_none());
        let configuration = GoRun.anything(&words("go test -run 'Test A' ./..."), Path::new("/p/sub"), &context);
        let configuration = configuration.unwrap().unwrap();
        assert_eq!(configuration.name, "go test -run 'Test A' ./...");
        let lines = zen_run::external_tool::ExternalToolProvider.command_lines(&configuration, &context).unwrap();
        assert_eq!(lines[0].program, PathBuf::from("/opt/go/bin/go"));
        assert_eq!(lines[0].args, ["test", "-run", "Test A", "./..."]);
        assert_eq!(lines[0].cwd, PathBuf::from("/p/sub"));
        let missing = GoRun.anything(&words("go version"), Path::new("/p"), &RunContext::default()).unwrap();
        assert_eq!(missing.unwrap_err(), zen_i18n::t!("go-run-error-no-go"));
    }

    #[test]
    fn the_provider_explains_delves_errors() {
        zen_i18n::register(crate::LOCALES);
        let explained = GoRun.explain_debug_error("could not launch process: debugserver or lldb-server not found");
        assert_eq!(explained, Some(zen_i18n::t!("go-debug-error-debugserver")));
        assert_eq!(GoRun.explain_debug_error("could not find function main.nosuch"), None);
    }

    fn fake_go(dir: &Path, script: &str) -> RunContext {
        let go = dir.join("go");
        std::fs::write(&go, format!("#!/bin/sh\n{script}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        RunContext {
            root: dir.to_owned(),
            env: zen_run::RunEnv { tool: go, env: vec![("GOFLAGS".to_owned(), "-mod=mod".to_owned())] },
            ..RunContext::default()
        }
    }

    fn run_lines(lines: &[CommandLine]) -> (String, Option<i32>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut text = String::new();
        let line = &lines[0];
        let mut spawned = line.clone();
        spawned.env.clone_from(&line.env);
        let sink: zen_run::ProcessSink = std::sync::Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let _process = zen_run::Process::spawn(&spawned, sink).unwrap();
        loop {
            match receiver.recv().unwrap() {
                zen_run::ProcessEvent::Output { text: out, .. } => text.push_str(&out),
                zen_run::ProcessEvent::Exited { code } => return (text, code),
            }
        }
    }

    #[test]
    fn a_go_command_task_runs_go_with_its_words_in_its_folder() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        let context =
            fake_go(dir.path(), "echo \"go $* in $(basename \"$PWD\") $GOFLAGS\"; [ \"$1\" = vet ] && exit 3; exit 0");
        let kinds = GoRun.task_kinds();
        assert_eq!(kinds[0].id, COMMAND_TASK);
        let task = BeforeLaunchTask::new(
            COMMAND_TASK,
            serde_json::json!({ "command": "generate ./...", "workingDirectory": "api" }),
        );
        let lines = GoRun.task_lines(&task, &context).unwrap().unwrap();
        assert_eq!(lines[0].args, ["generate", "./..."]);
        assert_eq!(lines[0].cwd, dir.path().join("api"));
        assert!(lines[0].display.ends_with("generate ./... #gosetup"), "{}", lines[0].display);
        let (text, code) = run_lines(&lines);
        assert_eq!((text.trim(), code), ("go generate ./... in api -mod=mod", Some(0)));
        let vet = BeforeLaunchTask::new(COMMAND_TASK, serde_json::json!({ "command": "vet ./..." }));
        let lines = GoRun.task_lines(&vet, &context).unwrap().unwrap();
        assert_eq!(lines[0].cwd, dir.path());
        assert_eq!(run_lines(&lines).1, Some(3), "a failing go command fails the task");
        let empty = BeforeLaunchTask::new(COMMAND_TASK, serde_json::json!({ "command": " " }));
        assert_eq!(GoRun.task_lines(&empty, &context), Some(Err(zen_i18n::t!("go-run-error-no-command"))));
        let other = BeforeLaunchTask::new("maven.goal", serde_json::json!({}));
        assert_eq!(GoRun.task_lines(&other, &context), None);
    }

    #[test]
    fn an_adopted_go_task_becomes_a_go_command() {
        let foreign = ForeignTask {
            id: FOREIGN_COMMAND_TASK.to_owned(),
            enabled: true,
            options: [
                ("command".to_owned(), "generate ./...".to_owned()),
                ("workingDirectory".to_owned(), "/p/api".to_owned()),
            ]
            .into_iter()
            .collect(),
        };
        let task = GoRun.adopt_task(&foreign, Path::new("/p")).unwrap();
        assert_eq!(task.kind, COMMAND_TASK);
        assert_eq!((task.text("command"), task.text("workingDirectory")), ("generate ./...", "api"));
        assert_eq!(GoRun.adopt_task(&ForeignTask { id: "Make".to_owned(), ..foreign }, Path::new("/p")), None);
    }

    #[test]
    fn metacharacters_are_escaped() {
        assert_eq!(quote_meta(r"a.b*c[d]{e}^$|\"), r"a\.b\*c\[d\]\{e\}\^\$\|\\");
        assert_eq!(quote_meta("plain_name"), "plain_name");
    }
}
