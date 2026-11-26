use std::path::{Path, PathBuf};

use zen_i18n::t;
use zen_run::coverage::parse_go_profile;
use zen_run::{CommandLine, Configuration, CoverageProvider, CoverageReport, RunContext, args};

use super::command::{module_folder, test_folder, test_with};
use super::{GoRun, TEST, TestOptions, options};

pub(super) fn mode(configuration: &Configuration) -> &'static str {
    let race = args::split(&configuration.common.tool_arguments).iter().any(|word| word == "-race" || word == "--race");
    if race { "atomic" } else { "count" }
}

pub(super) fn resolve(file: &str, context: &RunContext, test_cwd: &Path) -> PathBuf {
    let file = file.strip_prefix('_').filter(|rest| rest.starts_with('/')).unwrap_or(file);
    if Path::new(file).is_absolute() {
        return PathBuf::from(file);
    }
    let (package, name) = file.rsplit_once('/').unwrap_or(("", file));
    if package == "command-line-arguments" {
        return test_cwd.join(name);
    }
    match module_folder(package, &context.modules) {
        Some((_, folder)) => folder.join(name),
        None => context.root.join(file),
    }
}

impl CoverageProvider for GoRun {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        if configuration.kind != TEST {
            return Err(t!("go-coverage-error-kind", kind = configuration.kind.as_str()));
        }
        let extra =
            vec![format!("-coverprofile={}", report.to_string_lossy()), format!("-covermode={}", mode(configuration))];
        test_with(configuration, &options(configuration), context, extra)
    }

    fn read(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String> {
        let text =
            std::fs::read_to_string(report).map_err(|error| t!("go-coverage-error-read", error = error.to_string()))?;
        let test: TestOptions = options(configuration);
        let cwd = test_folder(&test, context).unwrap_or_else(|_| context.root.clone());
        parse_go_profile(&text, |file| resolve(file, context, &cwd))
            .map_err(|error| t!("go-coverage-error-format", line = error.line, text = error.text))
    }
}

#[cfg(test)]
mod tests {
    use zen_run::{LineStatus, Module, RunEnv, RunProvider};

    use super::*;

    fn context() -> RunContext {
        RunContext {
            root: PathBuf::from("/work/demo"),
            env: RunEnv { tool: PathBuf::from("/sdk/go/bin/go"), env: Vec::new() },
            modules: vec![
                Module { name: "example.com/demo".to_owned(), root: PathBuf::from("/work/demo"), ..Module::default() },
                Module {
                    name: "example.com/demo/tools".to_owned(),
                    root: PathBuf::from("/work/demo/tools"),
                    ..Module::default()
                },
            ],
            ..RunContext::default()
        }
    }

    fn configuration(kind: &str, options: serde_json::Value) -> Configuration {
        Configuration { name: "c".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() }
    }

    #[test]
    fn go_tests_run_with_a_cover_profile_and_builds_do_not() {
        let tests = configuration(TEST, serde_json::json!({ "package": "./calc" }));
        let provider = GoRun.coverage(&tests).expect("tests run with coverage");
        let lines = provider.command_lines(&tests, &context(), Path::new("/tmp/zen/c.out")).unwrap();
        assert_eq!(lines[0].args, ["test", "-json", "-coverprofile=/tmp/zen/c.out", "-covermode=count", "."]);
        assert_eq!(
            lines[0].display,
            "/sdk/go/bin/go test -json -coverprofile=/tmp/zen/c.out -covermode=count . #gosetup"
        );
        let mut racing = tests.clone();
        racing.common.tool_arguments = "-race".to_owned();
        let lines = provider.command_lines(&racing, &context(), Path::new("/c.out")).unwrap();
        assert!(lines[0].args.contains(&"-covermode=atomic".to_owned()), "-race needs the atomic mode");
        assert!(GoRun.coverage(&configuration(super::super::BUILD, serde_json::json!({}))).is_none());
    }

    #[test]
    fn profile_entries_are_files_of_the_project_its_modules_and_the_tested_folder() {
        let context = context();
        let cwd = Path::new("/work/demo/calc");
        assert_eq!(resolve("example.com/demo/calc/calc.go", &context, cwd), PathBuf::from("/work/demo/calc/calc.go"));
        assert_eq!(
            resolve("example.com/demo/tools/gen/x.go", &context, cwd),
            PathBuf::from("/work/demo/tools/gen/x.go")
        );
        assert_eq!(resolve("command-line-arguments/calc.go", &context, cwd), PathBuf::from("/work/demo/calc/calc.go"));
        assert_eq!(resolve("_/abs/pkg/a.go", &context, cwd), PathBuf::from("/abs/pkg/a.go"));
        assert_eq!(resolve("other.org/x/y.go", &context, cwd), PathBuf::from("/work/demo/other.org/x/y.go"));
    }

    #[test]
    fn a_profile_is_read_into_the_files_lines() {
        zen_i18n::register(crate::LOCALES);
        let folder = tempfile::tempdir().unwrap();
        let report = folder.path().join("c.out");
        std::fs::write(&report, "mode: count\nexample.com/demo/calc/calc.go:3.24,5.2 1 0\n").unwrap();
        let tests = configuration(TEST, serde_json::json!({ "package": "./calc" }));
        let read = GoRun.read(&tests, &context(), &report).unwrap();
        let file = read.file(Path::new("/work/demo/calc/calc.go")).unwrap();
        assert_eq!(file.lines.iter().map(|line| line.status).collect::<Vec<_>>(), [LineStatus::NotCovered; 3]);
        std::fs::write(&report, "mode: count\nbroken\n").unwrap();
        assert_eq!(
            GoRun.read(&tests, &context(), &report),
            Err(t!("go-coverage-error-format", line = 2, text = "broken"))
        );
        assert!(GoRun.read(&tests, &context(), &folder.path().join("none.out")).is_err());
    }
}
