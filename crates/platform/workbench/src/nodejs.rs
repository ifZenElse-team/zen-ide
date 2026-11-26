pub mod adapter;
pub mod reporter;
pub mod test_files;
mod websocket;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_run::{
    AdapterLine, CommandLine, Configuration, DebugPlan, FieldKind, FieldSpec, KindSpec, OutputParser, PlainParser,
    RunContext, RunProvider, RunTarget, TargetKind, TestId,
};

pub const PLUGIN: &str = "nodejs";

pub const KIND: &str = "node";

pub const TEST_KIND: &str = "node.test";

pub const RUNNERS: [&str; 3] = ["auto", "jest", "vitest"];

const SCRIPTS: [&str; 6] = ["js", "mjs", "cjs", "ts", "mts", "cts"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeOptions {
    pub file: String,
    pub node_options: String,
    pub interpreter: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeTestOptions {
    pub runner: String,
    pub files: String,
    pub test_name: String,
    pub node_options: String,
    pub interpreter: String,
}

impl Default for NodeTestOptions {
    fn default() -> Self {
        Self {
            runner: "auto".to_owned(),
            files: String::new(),
            test_name: String::new(),
            node_options: String::new(),
            interpreter: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NodeRun {
    folders: Option<Vec<PathBuf>>,
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn quoted(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn is_script(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| SCRIPTS.contains(&extension))
}

fn script_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

fn file_configuration(file: &str) -> Configuration {
    let options = NodeOptions { file: file.to_owned(), ..NodeOptions::default() };
    Configuration {
        name: script_name(file),
        kind: KIND.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn test_configuration(name: &str, file: &str, test_name: &str) -> Configuration {
    let options =
        NodeTestOptions { files: quoted(file), test_name: test_name.to_owned(), ..NodeTestOptions::default() };
    Configuration {
        name: name.to_owned(),
        kind: TEST_KIND.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn found_configuration(file: &str, found: &test_files::Found) -> Configuration {
    let text = test_files::escaped(&found.path.join(" "));
    let pattern = if found.suite { format!("^{text}( |$)") } else { format!("^{text}$") };
    test_configuration(found.path.last().map_or("", String::as_str), file, &pattern)
}

fn manifest_declares(folder: &Path, runner: &str) -> bool {
    let manifest = std::fs::read(folder.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or_default();
    ["dependencies", "devDependencies"].iter().any(|key| manifest[key].get(runner).is_some())
}

fn package_folder(root: &Path, file: &Path) -> PathBuf {
    file.ancestors()
        .skip(1)
        .take_while(|folder| folder.starts_with(root))
        .find(|folder| folder.join("package.json").is_file())
        .unwrap_or(root)
        .to_owned()
}

fn runner_entry(root: &Path, folder: &Path, runner: &str) -> Option<PathBuf> {
    let entry = match runner {
        "jest" => "jest/bin/jest.js",
        _ => "vitest/vitest.mjs",
    };
    folder
        .ancestors()
        .take_while(|at| at.starts_with(root))
        .map(|at| at.join("node_modules").join(entry))
        .find(|path| path.is_file())
}

fn runner_of(chosen: &str, root: &Path, folder: &Path) -> Result<(&'static str, PathBuf), String> {
    let known = |name: &str| ["vitest", "jest"].into_iter().find(|runner| *runner == name);
    let wanted: Vec<&'static str> = match chosen {
        "" | "auto" => {
            let declared: Vec<&'static str> =
                ["vitest", "jest"].into_iter().filter(|runner| manifest_declares(folder, runner)).collect();
            if declared.is_empty() { vec!["vitest", "jest"] } else { declared }
        }
        other => vec![known(other).ok_or_else(|| t!("workbench-node-unknown-runner", runner = other))?],
    };
    wanted
        .iter()
        .find_map(|runner| runner_entry(root, folder, runner).map(|entry| (*runner, entry)))
        .ok_or_else(|| t!("workbench-node-no-runner", runner = wanted[0]))
}

struct Launch {
    node: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    inspect_child: bool,
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    listener.local_addr().map(|address| address.port()).map_err(|error| error.to_string())
}

fn vitest_debug_args(entry: &Path) -> Result<Vec<String>, String> {
    let manifest = entry.with_file_name("package.json");
    let major = std::fs::read(manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|manifest| manifest["version"].as_str()?.split('.').next()?.parse::<u32>().ok())
        .unwrap_or(0);
    let mut args = vec![
        format!("--inspect-brk=127.0.0.1:{}", free_port()?),
        "--pool=forks".to_owned(),
        "--no-file-parallelism".to_owned(),
    ];
    args.push(if major >= 4 { "--maxWorkers=1" } else { "--poolOptions.forks.singleFork" }.to_owned());
    Ok(args)
}

impl NodeRun {
    pub fn in_folders(folders: Vec<PathBuf>) -> Self {
        Self { folders: Some(folders) }
    }

    fn folders(&self) -> Vec<PathBuf> {
        self.folders.clone().unwrap_or_else(zen_lsp::servers::search_folders)
    }

    fn node(&self, interpreter: &str) -> Result<PathBuf, String> {
        let wanted = if interpreter.trim().is_empty() { "node" } else { interpreter.trim() };
        zen_lsp::servers::find_executable(wanted, &self.folders())
            .ok_or_else(|| t!("workbench-node-not-found", program = wanted))
    }

    fn env(&self, configuration: &Configuration) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Ok(path) = std::env::join_paths(self.folders()) {
            env.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
        }
        env.extend(configuration.common.env.iter().cloned());
        env
    }

    fn file_launch(&self, configuration: &Configuration, context: &RunContext) -> Result<Launch, String> {
        let options: NodeOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        if options.file.trim().is_empty() {
            return Err(t!("workbench-node-no-file"));
        }
        let file = context.root.join(&options.file);
        let cwd = match &configuration.common.working_directory {
            Some(folder) if !folder.as_os_str().is_empty() => context.root.join(folder),
            _ => file.parent().map_or_else(|| context.root.clone(), Path::to_owned),
        };
        let mut args = zen_run::args::split(&options.node_options);
        args.push(file.to_string_lossy().into_owned());
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        Ok(Launch {
            node: self.node(&options.interpreter)?,
            args,
            cwd,
            env: self.env(configuration),
            inspect_child: false,
        })
    }

    fn test_launch(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        debugging: bool,
    ) -> Result<Launch, String> {
        let options: NodeTestOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        let files = zen_run::args::split(&options.files);
        let first = files.first().map_or_else(|| context.root.join("package.json"), |file| context.root.join(file));
        let folder = package_folder(&context.root, &first);
        let (runner, entry) = runner_of(&options.runner, &context.root, &folder)?;
        let scratch = &context.scratch;
        zen_run::create_private_folder(scratch).map_err(|error| error.to_string())?;
        let (name, source) = match runner {
            "jest" => ("zen-jest-reporter.cjs", reporter::JEST),
            _ => ("zen-vitest-reporter.mjs", reporter::VITEST),
        };
        let reporter = scratch.join(name);
        std::fs::write(&reporter, source).map_err(|error| error.to_string())?;
        let reporter = reporter.to_string_lossy();
        let mut args = zen_run::args::split(&options.node_options);
        args.push(entry.to_string_lossy().into_owned());
        let absolute = files.iter().map(|file| context.root.join(file).to_string_lossy().into_owned());
        if runner == "jest" {
            args.extend(["--reporters=default".to_owned(), format!("--reporters={reporter}")]);
            if debugging {
                args.push("--runInBand".to_owned());
            }
            if !files.is_empty() {
                args.push("--runTestsByPath".to_owned());
                args.extend(absolute);
            }
        } else {
            args.extend(["run".to_owned(), "--reporter=default".to_owned(), format!("--reporter={reporter}")]);
            if debugging {
                args.extend(vitest_debug_args(&entry)?);
            }
            args.extend(absolute);
        }
        if !options.test_name.is_empty() {
            args.extend(["-t".to_owned(), options.test_name.clone()]);
        }
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = match &configuration.common.working_directory {
            Some(folder) if !folder.as_os_str().is_empty() => context.root.join(folder),
            _ => folder,
        };
        Ok(Launch {
            node: self.node(&options.interpreter)?,
            args,
            cwd,
            env: self.env(configuration),
            inspect_child: runner == "vitest",
        })
    }

    fn line(launch: Launch) -> CommandLine {
        let display =
            std::iter::once("node".to_owned()).chain(launch.args.iter().cloned()).collect::<Vec<_>>().join(" ");
        CommandLine { program: launch.node, args: launch.args, cwd: launch.cwd, env: launch.env, display }
    }
}

impl RunProvider for NodeRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let common = || {
            [
                field("nodeOptions", "workbench-node-field-options", FieldKind::Text),
                field("interpreter", "workbench-node-field-interpreter", FieldKind::Path),
            ]
        };
        let mut file = vec![field("file", "workbench-node-field-file", FieldKind::Path)];
        file.extend(common());
        let mut test = vec![
            field(
                "runner",
                "workbench-node-field-runner",
                FieldKind::Choice(RUNNERS.iter().map(|runner| (*runner).to_owned()).collect()),
            ),
            field("files", "workbench-node-field-files", FieldKind::Text),
            field("testName", "workbench-node-field-test-name", FieldKind::Text),
        ];
        test.extend(common());
        vec![
            KindSpec {
                id: KIND.to_owned(),
                name_message: "workbench-node-kind".to_owned(),
                fields: file,
                defaults: serde_json::to_value(NodeOptions::default()).unwrap_or_default(),
            },
            KindSpec {
                id: TEST_KIND.to_owned(),
                name_message: "workbench-node-test-kind".to_owned(),
                fields: test,
                defaults: serde_json::to_value(NodeTestOptions::default()).unwrap_or_default(),
            },
        ]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        if !test_files::is_test_file(path) {
            return Vec::new();
        }
        let file = relative(&context.root, path);
        test_files::find_tests(text)
            .iter()
            .map(|found| RunTarget {
                line: found.line,
                kind: TargetKind::Test,
                name: found.path.join(" "),
                package: file.clone(),
                configuration: found_configuration(&file, found),
            })
            .collect()
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let file = relative(root, path);
        if test_files::is_test_file(path) {
            let line = u32::try_from(text.get(..offset.min(text.len()))?.matches('\n').count()).ok()?;
            let found = test_files::find_tests(text);
            let at = found.iter().rev().find(|test| test.line <= line);
            return Some(match at {
                Some(found) => found_configuration(&file, found),
                None => test_configuration(&script_name(&file), &file, ""),
            });
        }
        is_script(path).then(|| file_configuration(&file))
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        match configuration.kind.as_str() {
            KIND => Ok(vec![Self::line(self.file_launch(configuration, context)?)]),
            TEST_KIND => Ok(vec![Self::line(self.test_launch(configuration, context, false)?)]),
            other => Err(t!("workbench-node-unknown-kind", kind = other)),
        }
    }

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        if configuration.kind == TEST_KIND {
            Box::new(reporter::NodeTestParser::new(&context.root))
        } else {
            Box::new(PlainParser)
        }
    }

    fn plugin(&self) -> Option<&str> {
        Some(PLUGIN)
    }

    fn rerun_failed(&self, configuration: &Configuration, failed: &[TestId]) -> Option<Configuration> {
        if configuration.kind != TEST_KIND || failed.is_empty() {
            return None;
        }
        let mut files: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let is_suite = |id: &TestId| {
            let inside = format!("{}/", id.test);
            failed.iter().any(|other| other.package == id.package && other.test.starts_with(&inside))
        };
        for id in failed.iter().filter(|id| !is_suite(id)) {
            if !files.contains(&id.package) {
                files.push(id.package.clone());
            }
            let name = test_files::escaped(&reporter::runner_name(&id.test));
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let mut options: NodeTestOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        options.files = files.iter().map(|file| quoted(file)).collect::<Vec<_>>().join(" ");
        options.test_name = format!("^(?:{})$", names.join("|"));
        Some(Configuration {
            id: String::new(),
            temporary: true,
            options: serde_json::to_value(options).ok()?,
            ..configuration.clone()
        })
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        let launch = match configuration.kind.as_str() {
            KIND => self.file_launch(configuration, context),
            TEST_KIND => self.test_launch(configuration, context, true),
            _ => return None,
        };
        Some(launch.map(|launch| {
            let body = serde_json::json!({
                "program": launch.node,
                "args": launch.args,
                "cwd": launch.cwd,
                "env": launch.env,
                "inspectChild": launch.inspect_child,
            });
            let inspect = if launch.inspect_child { "" } else { "--inspect-brk " };
            let mut line = Self::line(launch);
            line.display = format!("node {inspect}{}", line.args.join(" "));
            DebugPlan {
                adapter: AdapterLine { line, builtin: Some(adapter::ADAPTER.to_owned()), ..AdapterLine::default() },
                launch: body,
                ..DebugPlan::default()
            }
        }))
    }
}

#[cfg(test)]
mod tests;
