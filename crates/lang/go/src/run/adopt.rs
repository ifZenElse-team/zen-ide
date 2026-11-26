use std::path::{Path, PathBuf};

use zen_run::{Common, Configuration, ForeignRunConfiguration};

use super::{BUILD, BuildOptions, TEST, TestOptions};

const APPLICATION: &str = "GoApplicationRunConfiguration";
const TEST_TYPE: &str = "GoTestRunConfiguration";

pub(super) fn adopt(foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
    let option = |key: &str| foreign.options.get(key).map(String::as_str).unwrap_or_default().trim().to_owned();
    let path = |key: &str| relative(root, &option(key));
    let target = match option("kind").to_ascii_uppercase().as_str() {
        "FILE" => "file",
        "DIRECTORY" => "directory",
        _ => "package",
    }
    .to_owned();
    let files = quote_file(&path("filePath"));
    let directory = folder(&path("directory"));
    let (kind, options) = match foreign.type_id.as_str() {
        APPLICATION => {
            let options = BuildOptions {
                run_kind: target,
                package: option("package"),
                files,
                directory,
                output_directory: path("output_directory"),
                run_after_build: true,
            };
            (BUILD, serde_json::to_value(options).ok()?)
        }
        TEST_TYPE => {
            let pattern = option("pattern");
            let (pattern, bench) = match option("framework").as_str() {
                "gocheck" => return None,
                "gobench" => ("^$".to_owned(), if pattern.is_empty() { ".".to_owned() } else { pattern }),
                _ => (pattern, String::new()),
            };
            let options =
                TestOptions { test_kind: target, package: option("package"), directory, files, pattern, bench };
            (TEST, serde_json::to_value(options).ok()?)
        }
        _ => return None,
    };
    let working_directory = Some(path("working_directory")).filter(|folder| !folder.is_empty()).map(PathBuf::from);
    let common = Common {
        working_directory,
        env: foreign.env.clone(),
        program_arguments: option("parameters"),
        tool_arguments: option("go_parameters"),
        ..Common::default()
    };
    Some(Configuration {
        name: foreign.name.clone(),
        kind: kind.to_owned(),
        common,
        options,
        ..Configuration::default()
    })
}

fn relative(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    match path.strip_prefix(root) {
        Ok(inside) => inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => value.to_owned(),
    }
}

fn folder(relative: &str) -> String {
    if relative.is_empty() { ".".to_owned() } else { relative.to_owned() }
}

fn quote_file(file: &str) -> String {
    if file.chars().any(char::is_whitespace) { format!("'{}'", file.replace('\'', r"'\''")) } else { file.to_owned() }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_run::RunProvider;

    use super::*;
    use crate::run::GoRun;

    fn foreign(type_id: &str, options: &[(&str, &str)]) -> ForeignRunConfiguration {
        ForeignRunConfiguration {
            source_file: PathBuf::from("/p/.run/x.run.xml"),
            name: "Serve".to_owned(),
            type_id: type_id.to_owned(),
            factory: None,
            options: options.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
            env: vec![("MODE".to_owned(), "dev".to_owned())],
            shared: true,
            before_launch: Vec::new(),
        }
    }

    fn adopted(type_id: &str, options: &[(&str, &str)]) -> Option<Configuration> {
        GoRun.adopt(&foreign(type_id, options), Path::new("/p"))
    }

    #[test]
    fn an_application_becomes_a_build_that_runs_the_program() {
        let configuration = adopted(
            APPLICATION,
            &[
                ("kind", "PACKAGE"),
                ("package", "example.org/app/cmd/serve"),
                ("filePath", "/p/cmd/serve/main.go"),
                ("directory", "/p"),
                ("output_directory", "/p/bin"),
                ("working_directory", "/p/data"),
                ("parameters", "--listen :7311"),
                ("go_parameters", "-race"),
                ("module.name", "app"),
                ("method.v", "2"),
            ],
        )
        .unwrap();
        assert_eq!((configuration.name.as_str(), configuration.kind.as_str()), ("Serve", BUILD));
        assert_eq!(
            configuration.options,
            json!({
                "runKind": "package",
                "package": "example.org/app/cmd/serve",
                "files": "cmd/serve/main.go",
                "directory": ".",
                "outputDirectory": "bin",
                "runAfterBuild": true
            })
        );
        assert_eq!(
            configuration.common,
            Common {
                working_directory: Some(PathBuf::from("data")),
                env: vec![("MODE".to_owned(), "dev".to_owned())],
                program_arguments: "--listen :7311".to_owned(),
                tool_arguments: "-race".to_owned(),
                ..Common::default()
            }
        );
        assert!(configuration.id.is_empty() && !configuration.temporary);
    }

    #[test]
    fn a_test_keeps_its_target_and_pattern() {
        let configuration = adopted(
            TEST_TYPE,
            &[("kind", "DIRECTORY"), ("directory", "/p/internal"), ("pattern", "^TestParse$"), ("framework", "gotest")],
        )
        .unwrap();
        assert_eq!(configuration.kind, TEST);
        assert_eq!(configuration.options["testKind"], "directory");
        assert_eq!(configuration.options["directory"], "internal");
        assert_eq!(configuration.options["pattern"], "^TestParse$");
        assert_eq!(configuration.options["bench"], "");
        assert_eq!(configuration.common.working_directory, None);
        let file = adopted(TEST_TYPE, &[("kind", "FILE"), ("filePath", "/p/my dir/a_test.go")]).unwrap();
        assert_eq!(file.options["files"], "'my dir/a_test.go'");
    }

    #[test]
    fn benchmarks_run_through_bench_and_gocheck_is_not_adopted() {
        let bench = adopted(TEST_TYPE, &[("framework", "gobench"), ("pattern", "BenchmarkSort")]).unwrap();
        assert_eq!(
            (bench.options["pattern"].as_str(), bench.options["bench"].as_str()),
            (Some("^$"), Some("BenchmarkSort"))
        );
        let all = adopted(TEST_TYPE, &[("framework", "gobench")]).unwrap();
        assert_eq!(all.options["bench"], ".");
        assert_eq!(adopted(TEST_TYPE, &[("framework", "gocheck")]), None);
        assert_eq!(adopted("GoRemoteDebugConfigurationType", &[]), None);
    }

    #[test]
    fn paths_outside_the_project_stay_absolute() {
        let configuration = adopted(APPLICATION, &[("kind", "DIRECTORY"), ("directory", "/elsewhere/tool")]).unwrap();
        assert_eq!(configuration.options["directory"], "/elsewhere/tool");
        assert_eq!(configuration.options["runKind"], "directory");
    }
}
