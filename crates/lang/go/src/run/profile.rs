use std::path::Path;

use zen_i18n::t;
use zen_run::{CommandLine, Configuration, Profile, ProfileKind, ProfileProvider, RunContext};

use super::command::{test_folder, test_with};
use super::coverage::resolve;
use super::{GoRun, TestOptions, options};

const TEST_BINARY: &str = "profiled.test";

impl ProfileProvider for GoRun {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        kind: ProfileKind,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let flag = match kind {
            ProfileKind::Cpu => "-cpuprofile",
            ProfileKind::Memory => "-memprofile",
        };
        let binary = report.with_file_name(TEST_BINARY);
        let extra = vec![format!("{flag}={}", report.to_string_lossy()), format!("-o={}", binary.to_string_lossy())];
        test_with(configuration, &options(configuration), context, extra)
    }

    fn read(&self, configuration: &Configuration, context: &RunContext, report: &Path) -> Result<Profile, String> {
        if !report.is_file() {
            return Err(t!("go-profile-no-data", path = report.to_string_lossy()));
        }
        let test: TestOptions = options(configuration);
        let cwd = test_folder(&test, context).unwrap_or_else(|_| context.root.clone());
        zen_run::pprof::read(report, |file| resolve(file, context, &cwd))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zen_run::{Module, RunEnv, RunProvider};

    use super::*;

    fn context() -> RunContext {
        RunContext {
            root: PathBuf::from("/work/demo"),
            env: RunEnv { tool: PathBuf::from("/sdk/go/bin/go"), env: Vec::new() },
            modules: vec![Module {
                name: "example.com/demo".to_owned(),
                root: PathBuf::from("/work/demo"),
                ..Module::default()
            }],
            ..RunContext::default()
        }
    }

    fn configuration(kind: &str) -> Configuration {
        let options = serde_json::json!({ "testKind": "package", "package": "." });
        Configuration { name: "c".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() }
    }

    #[test]
    fn go_tests_record_a_profile_and_keep_the_test_binary_out_of_the_project() {
        let tests = configuration(super::super::TEST);
        let provider = GoRun.profiler(&tests).expect("tests run with the profiler");
        let report = Path::new("/tmp/zen/profile.pb.gz");
        let lines = provider.command_lines(&tests, &context(), ProfileKind::Cpu, report).unwrap();
        assert_eq!(
            lines[0].args[..4],
            ["test", "-json", "-cpuprofile=/tmp/zen/profile.pb.gz", "-o=/tmp/zen/profiled.test"]
        );
        let lines = provider.command_lines(&tests, &context(), ProfileKind::Memory, report).unwrap();
        assert_eq!(lines[0].args[2], "-memprofile=/tmp/zen/profile.pb.gz");
    }

    #[test]
    fn a_program_run_is_not_offered_the_profiler() {
        assert!(GoRun.profiler(&configuration(super::super::BUILD)).is_none());
    }

    #[test]
    fn a_run_that_wrote_no_profile_says_so() {
        let tests = configuration(super::super::TEST);
        let error = ProfileProvider::read(&GoRun, &tests, &context(), Path::new("/no/such/profile")).unwrap_err();
        assert_eq!(error, t!("go-profile-no-data", path = "/no/such/profile"));
    }
}
