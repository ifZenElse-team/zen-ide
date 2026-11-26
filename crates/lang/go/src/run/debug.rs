use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use zen_i18n::t;
use zen_run::{AdapterLine, CommandLine, Configuration, DebugPlan, RunContext, args};

use super::command::{binary, build_target, display, environment, go_line, package_folder, scratch, test_target, tool};
use super::{BuildOptions, TestOptions};

const NO_OPTIMIZATIONS: &str = "-gcflags=all=-N -l";

fn tool_arguments(configuration: &Configuration) -> (Vec<String>, bool) {
    let mut trimmed = false;
    let words = args::split(&configuration.common.tool_arguments)
        .into_iter()
        .filter(|word| {
            let trimpath = matches!(word.as_str(), "-trimpath" | "--trimpath" | "-trimpath=true" | "--trimpath=true");
            trimmed |= trimpath;
            !trimpath
        })
        .collect();
    (words, trimmed)
}

fn build_line(
    configuration: &Configuration,
    context: &RunContext,
    mut words: Vec<String>,
    cwd: PathBuf,
    target: Vec<String>,
) -> Result<CommandLine, String> {
    let go = tool(context)?;
    let (tool_args, trimmed) = tool_arguments(configuration);
    words.extend(tool_args);
    words.extend(target);
    let mut env = environment(configuration, context);
    env.push(("PWD".to_owned(), cwd.to_string_lossy().into_owned()));
    let mut line = go_line(go, words, cwd, env);
    if trimmed {
        line.display = format!("{} ({})", line.display, t!("go-debug-trimpath-dropped"));
    }
    Ok(line)
}

fn adapter(context: &RunContext, cwd: &Path) -> AdapterLine {
    let program = context.debugger.clone();
    let args = vec!["dap".to_owned()];
    let display = if program.as_os_str().is_empty() { String::new() } else { display(&program, &args) };
    AdapterLine {
        line: CommandLine { program, args, cwd: cwd.to_path_buf(), env: context.env.env.clone(), display },
        address_flag: "--client-addr".to_owned(),
        server: None,
        views: None,
        builtin: None,
    }
}

fn launch(configuration: &Configuration, program: &Path, args: Vec<String>, cwd: &Path) -> Value {
    let env: Map<String, Value> =
        configuration.common.env.iter().map(|(name, value)| (name.clone(), Value::from(value.as_str()))).collect();
    json!({
        "request": "launch",
        "mode": "exec",
        "program": program.to_string_lossy(),
        "args": args,
        "cwd": cwd.to_string_lossy(),
        "env": env,
        "stopOnEntry": false,
    })
}

pub(super) fn build(
    configuration: &Configuration,
    options: &BuildOptions,
    context: &RunContext,
) -> Result<DebugPlan, String> {
    if !options.run_after_build {
        return Err(t!("go-debug-error-no-run"));
    }
    let target = build_target(options, context)?;
    let program = binary(options, &target.name, context).unwrap_or_else(|| scratch(context).join(&target.name));
    let words =
        vec!["build".to_owned(), NO_OPTIMIZATIONS.to_owned(), "-o".to_owned(), program.to_string_lossy().into_owned()];
    let line = build_line(configuration, context, words, target.cwd, target.args)?;
    let cwd = configuration.common.working_directory_in(&context.root);
    let program_args = args::split(&configuration.common.program_arguments);
    Ok(DebugPlan {
        build: vec![line],
        adapter: adapter(context, &cwd),
        launch: launch(configuration, &program, program_args, &cwd),
        debuggee: None,
    })
}

pub(super) fn test(
    configuration: &Configuration,
    options: &TestOptions,
    context: &RunContext,
) -> Result<DebugPlan, String> {
    if options.test_kind == "directory" {
        return Err(t!("go-debug-error-directory"));
    }
    let target = test_target(options, context)?;
    let name = if target.name.is_empty() { "main" } else { target.name.as_str() };
    let program = scratch(context).join(format!("{name}.test{}", std::env::consts::EXE_SUFFIX));
    let words = vec![
        "test".to_owned(),
        "-c".to_owned(),
        NO_OPTIMIZATIONS.to_owned(),
        "-o".to_owned(),
        program.to_string_lossy().into_owned(),
    ];
    let line = build_line(configuration, context, words, target.cwd, target.args)?;
    let cwd = package_folder(options, context);
    let mut test_args = vec!["-test.v".to_owned(), "-test.paniconexit0".to_owned()];
    if !options.pattern.is_empty() {
        test_args.extend(["-test.run".to_owned(), options.pattern.clone()]);
    }
    if !options.bench.is_empty() {
        test_args.extend(["-test.bench".to_owned(), options.bench.clone()]);
    }
    test_args.extend(args::split(&configuration.common.program_arguments));
    Ok(DebugPlan {
        build: vec![line],
        adapter: adapter(context, &cwd),
        launch: launch(configuration, &program, test_args, &cwd),
        debuggee: None,
    })
}

pub fn explain_adapter_error(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("debugserver") || has("lldb-server") || has("backend unavailable") {
        Some(t!("go-debug-error-debugserver"))
    } else if has("delve is too old") || has("too new for this version of delve") {
        Some(t!("go-debug-error-delve-too-old"))
    } else if has("too old for this version of delve") {
        Some(t!("go-debug-error-go-too-old"))
    } else if has("ptrace") || (has("could not attach") && has("operation not permitted")) {
        Some(t!("go-debug-error-ptrace"))
    } else if has("could not open debug info") || has("no debug info") || has("decoding dwarf") {
        Some(t!("go-debug-error-no-debug-info"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use zen_run::{Module, RunEnv};

    use super::*;

    fn context(debugger: &str) -> RunContext {
        zen_i18n::register(crate::LOCALES);
        RunContext {
            root: PathBuf::from("/w/demo"),
            env: RunEnv {
                tool: PathBuf::from("/sdk/go/bin/go"),
                env: vec![("GOROOT".to_owned(), "/sdk/go".to_owned()), ("PATH".to_owned(), "/sdk/go/bin".to_owned())],
            },
            modules: vec![Module {
                name: "example.com/demo".to_owned(),
                root: PathBuf::from("/w/demo"),
                ..Module::default()
            }],
            scratch: PathBuf::from("/tmp/zen-run/7-1"),
            debugger: PathBuf::from(debugger),
        }
    }

    fn configuration(kind: &str, options: Value) -> Configuration {
        let mut configuration =
            Configuration { name: "c".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() };
        configuration.common.env = vec![("APP_MODE".to_owned(), "dev".to_owned())];
        configuration
    }

    fn exe(path: &str) -> String {
        format!("{path}{}", std::env::consts::EXE_SUFFIX)
    }

    fn plan(configuration: &Configuration, context: &RunContext) -> Result<DebugPlan, String> {
        match configuration.kind.as_str() {
            super::super::BUILD => {
                build(configuration, &serde_json::from_value(configuration.options.clone()).unwrap(), context)
            }
            _ => test(configuration, &serde_json::from_value(configuration.options.clone()).unwrap(), context),
        }
    }

    #[test]
    fn a_build_configuration_builds_without_optimizations_and_execs_the_program() {
        let mut build = configuration("go.build", json!({ "package": "example.com/demo/cmd/app" }));
        build.common.program_arguments = "--port 8080 'a b'".to_owned();
        build.common.tool_arguments = "-race".to_owned();
        build.common.working_directory = Some(PathBuf::from("cmd"));
        let plan = plan(&build, &context("/home/u/go/bin/dlv")).unwrap();
        let program = exe("/tmp/zen-run/7-1/app");
        let line = &plan.build[0];
        assert_eq!(line.program, Path::new("/sdk/go/bin/go"));
        assert_eq!(
            line.args,
            ["build", "-gcflags=all=-N -l", "-o", program.as_str(), "-race", "example.com/demo/cmd/app"]
        );
        assert_eq!(line.cwd, Path::new("/w/demo"));
        assert!(line.display.ends_with(" #gosetup"));
        assert!(line.display.contains("'-gcflags=all=-N -l'"));
        assert!(line.env.contains(&("APP_MODE".to_owned(), "dev".to_owned())));
        assert_eq!(line.env.last(), Some(&("PWD".to_owned(), "/w/demo".to_owned())));
        assert_eq!(plan.adapter.address_flag, "--client-addr");
        assert_eq!(plan.adapter.line.program, Path::new("/home/u/go/bin/dlv"));
        assert_eq!(plan.adapter.line.args, ["dap"]);
        assert_eq!(plan.adapter.line.cwd, Path::new("/w/demo/cmd"));
        assert_eq!(plan.adapter.line.env, context("").env.env);
        assert_eq!(plan.adapter.line.display, "/home/u/go/bin/dlv dap");
        assert_eq!(
            plan.launch,
            json!({
                "request": "launch",
                "mode": "exec",
                "program": program,
                "args": ["--port", "8080", "a b"],
                "cwd": "/w/demo/cmd",
                "env": { "APP_MODE": "dev" },
                "stopOnEntry": false,
            })
        );
    }

    #[test]
    fn an_output_directory_keeps_the_program_there() {
        let build = configuration(
            "go.build",
            json!({ "runKind": "directory", "directory": "cmd/app", "outputDirectory": "bin" }),
        );
        let plan = plan(&build, &context("dlv")).unwrap();
        assert_eq!(plan.launch["program"], exe("/w/demo/bin/app"));
        assert_eq!(plan.build[0].args.last().map(String::as_str), Some("."));
        assert_eq!(plan.build[0].cwd, Path::new("/w/demo/cmd/app"));
    }

    #[test]
    fn a_test_configuration_compiles_the_test_binary_and_runs_it_verbosely() {
        let mut tests = configuration(
            "go.test",
            json!({ "package": "example.com/demo/calc", "pattern": "^TestAdd$", "bench": "^BenchmarkAdd$" }),
        );
        tests.common.program_arguments = "-fixture big".to_owned();
        let plan = plan(&tests, &context("dlv")).unwrap();
        let program = exe("/tmp/zen-run/7-1/calc.test");
        let line = &plan.build[0];
        assert_eq!(line.args, ["test", "-c", "-gcflags=all=-N -l", "-o", program.as_str(), "example.com/demo/calc"]);
        assert_eq!(line.cwd, Path::new("/w/demo"));
        assert_eq!(
            plan.launch,
            json!({
                "request": "launch",
                "mode": "exec",
                "program": program,
                "args": ["-test.v", "-test.paniconexit0", "-test.run", "^TestAdd$", "-test.bench", "^BenchmarkAdd$", "-fixture", "big"],
                "cwd": "/w/demo/calc",
                "env": { "APP_MODE": "dev" },
                "stopOnEntry": false,
            })
        );
        assert_eq!(plan.adapter.line.cwd, Path::new("/w/demo/calc"));
    }

    #[test]
    fn folder_and_file_tests_run_in_their_folder() {
        let folder = plan(&configuration("go.test", json!({ "package": "./calc" })), &context("dlv")).unwrap();
        assert_eq!(folder.build[0].cwd, Path::new("/w/demo/calc"));
        assert_eq!(folder.build[0].args.last().map(String::as_str), Some("."));
        assert_eq!(folder.launch["cwd"], "/w/demo/calc");
        assert_eq!(folder.launch["args"], json!(["-test.v", "-test.paniconexit0"]));
        let files = plan(
            &configuration("go.test", json!({ "testKind": "file", "files": "calc/calc_test.go calc/calc.go" })),
            &context("dlv"),
        )
        .unwrap();
        assert_eq!(files.build[0].args[5..], ["calc_test.go", "calc.go"]);
        assert_eq!(files.launch["program"], exe("/tmp/zen-run/7-1/calc_test.test"));
        assert_eq!(files.launch["cwd"], "/w/demo/calc");
    }

    #[test]
    fn what_cannot_be_debugged_is_refused() {
        let directory = configuration("go.test", json!({ "testKind": "directory", "directory": "." }));
        assert_eq!(plan(&directory, &context("dlv")), Err(t!("go-debug-error-directory")));
        let no_run = configuration("go.build", json!({ "package": "./cmd/app", "runAfterBuild": false }));
        assert_eq!(plan(&no_run, &context("dlv")), Err(t!("go-debug-error-no-run")));
        let mut no_go = context("dlv");
        no_go.env.tool = PathBuf::new();
        assert_eq!(
            plan(&configuration("go.build", json!({ "package": "./cmd/app" })), &no_go),
            Err(t!("go-run-error-no-go"))
        );
    }

    #[test]
    fn a_missing_debugger_gives_an_adapter_without_a_program() {
        let plan = plan(&configuration("go.build", json!({ "package": "./cmd/app" })), &context("")).unwrap();
        assert!(plan.adapter.line.program.as_os_str().is_empty());
        assert!(plan.adapter.line.display.is_empty());
        assert_eq!(plan.adapter.address_flag, "--client-addr");
        assert_eq!(plan.build.len(), 1);
    }

    #[test]
    fn trimpath_is_left_out_and_said_so() {
        let mut build = configuration("go.build", json!({ "package": "./cmd/app" }));
        build.common.tool_arguments = "-trimpath -tags=dev".to_owned();
        let plan = plan(&build, &context("dlv")).unwrap();
        let line = &plan.build[0];
        assert!(!line.args.iter().any(|word| word.contains("trimpath")));
        assert!(line.args.contains(&"-tags=dev".to_owned()));
        assert!(line.display.ends_with(&format!("#gosetup ({})", t!("go-debug-trimpath-dropped"))));
    }

    #[test]
    fn known_delve_errors_are_explained() {
        zen_i18n::register(crate::LOCALES);
        let explained = |message: &str| explain_adapter_error(message);
        assert_eq!(
            explained(
                "could not launch process: debugserver or lldb-server not found: install Xcode's command line tools or lldb-server"
            ),
            Some(t!("go-debug-error-debugserver"))
        );
        assert_eq!(
            explained(
                "Version of Delve is too old for Go version go1.27.1 (maximum supported version 1.25, suppress this error with --check-go-version=false)"
            ),
            Some(t!("go-debug-error-delve-too-old"))
        );
        assert_eq!(
            explained(
                "Go version go1.19 is too old for this version of Delve (minimum supported version 1.23, suppress this error with --check-go-version=false)"
            ),
            Some(t!("go-debug-error-go-too-old"))
        );
        assert_eq!(explained("could not attach to pid 42: operation not permitted"), Some(t!("go-debug-error-ptrace")));
        assert_eq!(explained("could not open debug info"), Some(t!("go-debug-error-no-debug-info")));
        assert_eq!(explained("could not find function main.nosuch"), None);
        for id in [
            "go-debug-error-debugserver",
            "go-debug-error-delve-too-old",
            "go-debug-error-go-too-old",
            "go-debug-error-ptrace",
            "go-debug-error-no-debug-info",
            "go-debug-error-no-run",
            "go-debug-error-directory",
            "go-debug-trimpath-dropped",
        ] {
            assert_ne!(t!(id), id, "{id} has no message");
        }
    }
}
