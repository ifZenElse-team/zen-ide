use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use zen_i18n::t;
use zen_run::coverage::parse_lcov;
use zen_run::{CommandLine, Configuration, CoverageProvider, CoverageReport, RunContext};

use super::{CargoOptions, CargoRun, TEST, cargo_args, options};

pub(super) const LLVM_COV: &str = "cargo-llvm-cov";

pub(super) fn llvm_cov_args(
    options: &CargoOptions,
    tool_arguments: &str,
    program_arguments: &str,
    report: &Path,
) -> Result<Vec<String>, String> {
    if options.target_kind == "doc" {
        return Err(t!("rust-coverage-error-doc"));
    }
    let mut args = cargo_args(TEST, options, tool_arguments, program_arguments)?;
    args[0] = "llvm-cov".to_owned();
    let at = args.iter().position(|word| word == "--").unwrap_or(args.len());
    let output = ["--lcov".to_owned(), "--output-path".to_owned(), report.to_string_lossy().into_owned()];
    args.splice(at..at, output);
    Ok(args)
}

pub(super) fn finds_llvm_cov(cargo: &Path, path: Option<&OsStr>, cargo_home: Option<PathBuf>) -> bool {
    let file = format!("{LLVM_COV}{}", std::env::consts::EXE_SUFFIX);
    let beside = cargo.parent().map(Path::to_path_buf);
    let on_path = path.map(|path| std::env::split_paths(path).collect::<Vec<_>>()).unwrap_or_default();
    let home = cargo_home.map(|home| home.join("bin"));
    beside.into_iter().chain(on_path).chain(home).any(|folder| folder.join(&file).is_file())
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

impl CoverageProvider for CargoRun {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        if configuration.kind != TEST {
            return Err(t!("rust-coverage-error-kind", kind = configuration.kind.as_str()));
        }
        let args = llvm_cov_args(
            &options(configuration),
            &configuration.common.tool_arguments,
            &configuration.common.program_arguments,
            report,
        )?;
        let setup = vec![("CARGO_LLVM_COV_SETUP".to_owned(), "no".to_owned())];
        let line = self.cargo_line(configuration, context, args, setup)?;
        let path = line.env.iter().rev().find(|(name, _)| name == "PATH").map(|(_, value)| value.clone());
        let path = path.map(std::ffi::OsString::from).or_else(|| std::env::var_os("PATH"));
        if !finds_llvm_cov(&line.program, path.as_deref(), cargo_home()) {
            return Err(t!("rust-coverage-error-no-llvm-cov"));
        }
        Ok(vec![line])
    }

    fn read(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String> {
        let text = std::fs::read_to_string(report)
            .map_err(|error| t!("rust-coverage-error-read", error = error.to_string()))?;
        let cwd = configuration.common.working_directory_in(&context.root);
        parse_lcov(&text, &cwd).map_err(|error| t!("rust-coverage-error-format", line = error.line, text = error.text))
    }
}

#[cfg(test)]
mod tests {
    use zen_run::{LineStatus, RunProvider};

    use super::*;

    #[test]
    fn tests_run_with_coverage_through_cargo_llvm_cov() {
        let options = CargoOptions {
            package: "calc".to_owned(),
            target_kind: "lib".to_owned(),
            filter: "tests::adds".to_owned(),
            exact: true,
            ..CargoOptions::default()
        };
        assert_eq!(
            llvm_cov_args(&options, "--locked", "", Path::new("/tmp/lcov.info")).unwrap().join(" "),
            "llvm-cov --no-fail-fast -p calc --lib --locked --lcov --output-path /tmp/lcov.info -- tests::adds --exact"
        );
        assert_eq!(
            llvm_cov_args(&CargoOptions::default(), "", "", Path::new("/l")).unwrap().join(" "),
            "llvm-cov --no-fail-fast --lcov --output-path /l"
        );
        zen_i18n::register(crate::LOCALES);
        let doc = CargoOptions { target_kind: "doc".to_owned(), ..CargoOptions::default() };
        assert_eq!(llvm_cov_args(&doc, "", "", Path::new("/l")), Err(t!("rust-coverage-error-doc")));
        let tests = Configuration { kind: TEST.to_owned(), ..Configuration::default() };
        assert!(CargoRun::default().coverage(&tests).is_some());
        let run = Configuration { kind: super::super::RUN.to_owned(), ..Configuration::default() };
        assert!(CargoRun::default().coverage(&run).is_none());
    }

    #[test]
    fn cargo_llvm_cov_is_found_beside_cargo_on_the_path_or_in_cargo_home_and_never_installed() {
        let folder = tempfile::tempdir().unwrap();
        let bin = |name: &str| {
            let dir = folder.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let (toolchain, path, home) = (bin("toolchain"), bin("path"), bin("home/bin"));
        let cargo = toolchain.join("cargo");
        let file = format!("{LLVM_COV}{}", std::env::consts::EXE_SUFFIX);
        let path_var = std::env::join_paths([&path]).unwrap();
        let home_folder = Some(folder.path().join("home"));
        assert!(!finds_llvm_cov(&cargo, Some(&path_var), home_folder.clone()));
        std::fs::write(home.join(&file), "").unwrap();
        assert!(finds_llvm_cov(&cargo, Some(&path_var), home_folder.clone()));
        assert!(!finds_llvm_cov(&cargo, Some(&path_var), None));
        std::fs::write(path.join(&file), "").unwrap();
        assert!(finds_llvm_cov(&cargo, Some(&path_var), None));
        std::fs::write(toolchain.join(&file), "").unwrap();
        assert!(finds_llvm_cov(&cargo, None, None));
    }

    #[test]
    fn an_lcov_report_is_read_against_the_working_folder() {
        zen_i18n::register(crate::LOCALES);
        let folder = tempfile::tempdir().unwrap();
        let report = folder.path().join("lcov.info");
        std::fs::write(&report, "SF:src/lib.rs\nDA:1,2\nDA:2,0\nend_of_record\n").unwrap();
        let tests = Configuration { kind: TEST.to_owned(), ..Configuration::default() };
        let context = RunContext { root: PathBuf::from("/work/calc"), ..RunContext::default() };
        let read = CargoRun::default().read(&tests, &context, &report).unwrap();
        let lib = read.file(Path::new("/work/calc/src/lib.rs")).unwrap();
        assert_eq!(
            lib.lines.iter().map(|line| line.status).collect::<Vec<_>>(),
            [LineStatus::Covered, LineStatus::NotCovered]
        );
        std::fs::write(&report, "SF:a.rs\nDA:x\n").unwrap();
        assert_eq!(
            CargoRun::default().read(&tests, &context, &report),
            Err(t!("rust-coverage-error-format", line = 2, text = "DA:x"))
        );
    }
}
