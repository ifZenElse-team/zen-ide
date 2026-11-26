pub mod args;
pub mod capture;
pub mod console;
pub mod coverage;
pub mod env_file;
pub mod external_tool;
#[cfg(feature = "fake")]
pub mod fake;
pub mod links;
pub mod macros;
pub mod output;
pub mod parser;
pub mod pprof;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
pub mod profile;
#[cfg(target_arch = "wasm32")]
mod stub;
pub mod test_support;
pub mod test_tree;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use capture::{Captured, PIPE_GRACE, capture, capture_child, join_within, wait_within};
pub use console::{Console, ConsoleLine, DEFAULT_BUFFER_KB, MAX_BUFFER_KB, Stream, strip_ansi};
pub use coverage::{
    BranchCounts, CoverageCounts, CoverageProvider, CoverageRange, CoverageReport, FileCoverage, LineCoverage,
    LineStatus,
};
pub use links::{LinkSpan, detect};
pub use output::{RunOutput, RunState, SharedOutput};
pub use parser::{OutputParser, ParsedLine, PlainParser};
pub use profile::{
    CallNode, FunctionCost, Profile, ProfileFrame, ProfileKind, ProfileMetric, ProfileProvider, ProfileSample,
};
pub const LOCALES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en/run.ftl")), ("ru", include_str!("../locales/ru/run.ftl"))];

#[cfg(not(target_arch = "wasm32"))]
pub use process::{Process, Programs, kill_group, own_group};
#[cfg(target_arch = "wasm32")]
pub use stub::{Process, Programs, kill_group, own_group};
pub use test_tree::{Measure, TestCounts, TestEvent, TestId, TestNode, TestNodeKind, TestSource, TestStatus, TestTree};
pub use zen_project::model::Module;
pub use zen_project::run::{BeforeLaunchTask, Common, Configuration, RunConfigurations, Template};
pub use zen_project::settings::{ForeignRunConfiguration, ForeignTask};

pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub type ProcessSink = Arc<dyn Fn(ProcessEvent) + Send + Sync>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunEnv {
    pub tool: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunContext {
    pub root: PathBuf,
    pub env: RunEnv,
    pub modules: Vec<Module>,
    pub scratch: PathBuf,
    pub debugger: PathBuf,
}

/// A kind of configuration a provider runs, as the Edit Configurations dialog lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KindSpec {
    /// The kind's id, e.g. `go.test`.
    pub id: String,
    /// The message id of the kind's name, e.g. `go-run-kind-test`.
    pub name_message: String,
    /// The fields of the kind's options, in the dialog's order.
    pub fields: Vec<FieldSpec>,
    /// The options a new configuration of this kind starts with.
    pub defaults: serde_json::Value,
}

/// One field of a kind's options.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpec {
    /// The key in [`Configuration::options`].
    pub key: String,
    /// The message id of the field's label.
    pub label_message: String,
    /// What the field holds and how it is edited.
    pub kind: FieldKind,
}

/// What a field holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "type", content = "values")]
pub enum FieldKind {
    /// A line of text.
    Text,
    /// A file or folder, relative to the project.
    Path,
    /// One of these values.
    Choice(Vec<String>),
    /// On or off.
    Bool,
    /// Environment variables, as `[[name, value]]`.
    Env,
    /// The name of one of the project's run configurations.
    Configuration,
    /// The names of one or more of the project's artifacts (Project Structure → Artifacts).
    Artifacts,
    /// The id of an external tool.
    ExternalTool,
    /// A file watcher as `<level>:<id>` (`project:prettier`), or empty for every one.
    FileWatcher,
}

/// What a gutter icon runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    /// A program's entry point, e.g. Go's `func main`.
    Main,
    /// A test.
    Test,
    /// A benchmark.
    Benchmark,
    /// A runnable example.
    Example,
    /// A fuzz test.
    Fuzz,
    /// A line that generates code, e.g. Go's `//go:generate`.
    Generate,
}

/// Something a file can run from a line, shown as a gutter icon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunTarget {
    /// The line the icon is on, from 0.
    pub line: u32,
    /// What it runs.
    pub kind: TargetKind,
    /// The function's or test's name.
    pub name: String,
    /// The package its test results name, e.g. Go's import path; empty when not known.
    #[serde(default)]
    pub package: String,
    /// The configuration that runs it, without an id; the same as the context at its line gives.
    pub configuration: Configuration,
}

/// One program to start, with everything it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandLine {
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub cwd: PathBuf,
    /// Environment variables added to the IDE's.
    pub env: Vec<(String, String)>,
    /// The command line as the console shows it before the output.
    pub display: String,
}

/// How to start a debug adapter that connects back to the IDE (ADR-0052).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdapterLine {
    /// The adapter's command line; an empty program means the debugger is not found.
    pub line: CommandLine,
    /// The flag the IDE's listening address is given with, as one `<flag>=<host:port>` argument appended to the line, e.g. `--client-addr`.
    pub address_flag: String,
    /// An adapter a language server runs instead of a program: the line is then not started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerAdapter>,
    /// How to switch the adapter between showing collections by their elements and by their fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<AdapterViews>,
    /// An adapter that is part of the IDE, by name: the line is shown and not started (ADR-0268).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
}

/// A command that switches how a debug adapter shows collections, maps and arrays: by their
/// elements (the logical view) or by their fields (the raw view). With a `server` adapter it is a
/// `workspace/executeCommand` of the language server with these arguments, as java-debug's
/// settings in Eclipse JDT LS; otherwise a custom request to the adapter with the first argument.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdapterViews {
    /// The command or request.
    pub command: String,
    /// The arguments that turn the logical view on.
    pub logical: Vec<serde_json::Value>,
    /// The arguments that show the raw fields.
    pub raw: Vec<serde_json::Value>,
}

/// A debug adapter that a language server runs, as java-debug runs in Eclipse JDT LS (ADR-0153):
/// the IDE asks the server for the port it listens on with the command `command` and connects to
/// it, after filling the `launch` arguments with what the `resolve` commands answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerAdapter {
    /// The language server's id, e.g. `jdtls`.
    pub server: String,
    /// The `workspace/executeCommand` command that starts the adapter and answers its port.
    pub command: String,
    /// Commands whose answers fill `launch` fields first, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolve: Vec<LaunchResolve>,
    /// Whether the adapter reloads changed classes into the running program, as java-debug does:
    /// it reports a finished build with a `hotcodereplace` event, and the custom request
    /// `redefineClasses` answers the classes it replaced (ADR-0154).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hot_code_replace: bool,
}

/// A command of the language server whose answer fills fields of the `launch` arguments, e.g.
/// java-debug's `vscode.java.resolveClasspath`, whose answer's `/1` is the `classPaths`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResolve {
    /// The `workspace/executeCommand` command.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub arguments: Vec<serde_json::Value>,
    /// The launch field each part of the answer goes to, by the part's JSON pointer (`""` for
    /// the whole answer), e.g. `{"/1": "classPaths", "/0": "modulePaths"}`.
    pub fields: std::collections::BTreeMap<String, String>,
}

/// A program debugged by attaching to it, started after the build (a test run of Maven or
/// Gradle that waits for the debugger, ADR-0153): the adapter connects once it printed `ready`,
/// and its output and tests are the session's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Debuggee {
    /// The program.
    pub line: CommandLine,
    /// Text of the output line that says it waits for the debugger.
    pub ready: String,
    /// An agent jar to load, taken from the classpath the build resolved for this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ClasspathAgent>,
}

/// An agent jar found by name in the classpath file a run's build writes, loaded when the debuggee
/// starts; only an entry of that file is ever used.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClasspathAgent {
    /// The argument file holding the resolved classpath, as one quoted value of entries joined by the
    /// system's path separator.
    pub classpath: PathBuf,
    /// The start of the jar's file name.
    pub prefix: String,
    /// The option that loads it; the jar's path follows it directly.
    pub option: String,
    /// Arguments starting with this come before it; at the start when none does.
    pub after: String,
}

pub const MAX_CLASSPATH_FILE: u64 = 4 << 20;

pub fn classpath_entries(text: &str) -> Vec<PathBuf> {
    let value = text.trim();
    let value = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')).unwrap_or(value);
    let mut unescaped = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                unescaped.push(next);
            }
        } else {
            unescaped.push(c);
        }
    }
    std::env::split_paths(&unescaped).filter(|entry| !entry.as_os_str().is_empty()).collect()
}

impl ClasspathAgent {
    pub fn find(&self) -> Option<PathBuf> {
        let size = std::fs::metadata(&self.classpath).ok()?.len();
        if size > MAX_CLASSPATH_FILE || self.prefix.is_empty() {
            return None;
        }
        let text = std::fs::read_to_string(&self.classpath).ok()?;
        classpath_entries(&text).into_iter().find(|entry| {
            let name = entry.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            entry.is_absolute() && name.starts_with(&self.prefix) && name.ends_with(".jar") && entry.is_file()
        })
    }

    pub fn applied(&self, line: &CommandLine, jar: &Path) -> CommandLine {
        let mut line = line.clone();
        let argument = format!("{}{}", self.option, jar.display());
        let at = line
            .args
            .iter()
            .rposition(|arg| !self.after.is_empty() && arg.starts_with(&self.after))
            .map_or(0, |index| index + 1);
        let previous = at.checked_sub(1).and_then(|index| line.args.get(index)).cloned();
        line.args.insert(at, argument.clone());
        line.display = match previous.and_then(|before| line.display.find(&before).map(|found| found + before.len())) {
            Some(end) => format!("{} {argument}{}", &line.display[..end], &line.display[end..]),
            None => match line.display.split_once(' ') {
                Some((program, rest)) => format!("{program} {argument} {rest}"),
                None => format!("{} {argument}", line.display),
            },
        };
        line
    }
}

/// What debugging a configuration takes: programs to build first, the adapter, and its `launch` arguments (ADR-0052).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugPlan {
    /// The programs to start first, in order, each only when the one before exited with 0.
    pub build: Vec<CommandLine>,
    /// The debug adapter.
    pub adapter: AdapterLine,
    /// The arguments of the adapter's `launch` request, in the adapter's own shape; an `attach`
    /// request when they have `"request": "attach"`.
    pub launch: serde_json::Value,
    /// The program to attach to, started after the build and run alongside the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debuggee: Option<Debuggee>,
}

/// A first word that Run Anything hands to a provider, as `go` or `mvn`, with the commands it suggests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnythingPrefix {
    /// The word, e.g. `go`.
    pub prefix: String,
    /// The message id of what the commands do, as `?` lists them.
    pub help_message: String,
    /// Whole commands offered while the query starts with the word, e.g. `go test ./...`.
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessEvent {
    Output { stream: Stream, text: String },
    Exited { code: Option<i32> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    pub lines: Vec<(Stream, String)>,
    pub exit_code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScratchStarter {
    pub text: String,
    pub caret: usize,
}

pub trait RunProvider: Send + Sync {
    fn id(&self) -> &str;

    fn kinds(&self) -> Vec<KindSpec>;

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget>;

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration>;

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String>;

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser>;

    fn plugin(&self) -> Option<&str> {
        None
    }

    fn replaces(&self) -> Option<&str> {
        None
    }

    fn single_file(&self, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn scratch_starter(&self, _language: &str) -> Option<ScratchStarter> {
        None
    }

    fn scratch_home(&self, _file: &Path, _text: &str, _context: &RunContext) -> Option<PathBuf> {
        None
    }

    fn adopt(&self, _foreign: &ForeignRunConfiguration, _root: &Path) -> Option<Configuration> {
        None
    }

    fn rerun_failed(&self, _configuration: &Configuration, _failed: &[TestId]) -> Option<Configuration> {
        None
    }

    fn test_source(&self, _configuration: &Configuration, _test: &TestId, _context: &RunContext) -> Option<TestSource> {
        None
    }

    fn debug(&self, _configuration: &Configuration, _context: &RunContext) -> Option<Result<DebugPlan, String>> {
        None
    }

    fn explain_debug_error(&self, _message: &str) -> Option<String> {
        None
    }

    fn task_kinds(&self) -> Vec<KindSpec> {
        Vec::new()
    }

    fn task_lines(&self, _task: &BeforeLaunchTask, _context: &RunContext) -> Option<Result<Vec<CommandLine>, String>> {
        None
    }

    fn adopt_task(&self, _foreign: &ForeignTask, _root: &Path) -> Option<BeforeLaunchTask> {
        None
    }

    fn replay(&self, _configuration: &Configuration) -> Option<Replay> {
        None
    }

    fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
        Vec::new()
    }

    fn anything(&self, _words: &[String], _cwd: &Path, _context: &RunContext) -> Option<Result<Configuration, String>> {
        None
    }

    fn coverage(&self, _configuration: &Configuration) -> Option<&dyn CoverageProvider> {
        None
    }

    fn profiler(&self, _configuration: &Configuration) -> Option<&dyn ProfileProvider> {
        None
    }
}

pub fn create_private_folder(folder: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder)
}

pub fn provider_for<'a>(providers: &'a [Arc<dyn RunProvider>], kind: &str) -> Option<&'a Arc<dyn RunProvider>> {
    providers.iter().find(|provider| provider.kinds().iter().any(|spec| spec.id == kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(classpath: &Path) -> ClasspathAgent {
        ClasspathAgent {
            classpath: classpath.to_owned(),
            prefix: "probe-agent-".to_owned(),
            option: "-javaagent:".to_owned(),
            after: "-agentlib:jdwp".to_owned(),
        }
    }

    #[test]
    fn the_agent_is_one_of_the_resolved_classpath_entries_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("lib").join("probe-agent-1.2.jar");
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, "jar").unwrap();
        let other = dir.path().join("lib").join("other.jar");
        std::fs::write(&other, "jar").unwrap();
        let joined = std::env::join_paths([other.as_path(), jar.as_path()]).unwrap();
        let classpath = dir.path().join("classpath.args");
        let quoted = format!("\"{}\"\n", joined.to_string_lossy().replace('\\', "\\\\"));
        std::fs::write(&classpath, quoted).unwrap();
        assert_eq!(classpath_entries(&std::fs::read_to_string(&classpath).unwrap()), vec![other.clone(), jar.clone()]);
        assert_eq!(agent(&classpath).find(), Some(jar.clone()));

        let missing = dir.path().join("lib").join("probe-agent-9.jar");
        std::fs::write(&classpath, format!("\"{}\"", missing.display())).unwrap();
        assert_eq!(agent(&classpath).find(), None, "an entry that is not a file");
        std::fs::write(&classpath, "\"probe-agent-1.2.jar\"").unwrap();
        std::fs::write(dir.path().join("probe-agent-1.2.jar"), "jar").unwrap();
        assert_eq!(agent(&classpath).find(), None, "a relative entry is never taken");
        std::fs::write(&classpath, format!("\"{}\"", other.display())).unwrap();
        assert_eq!(agent(&classpath).find(), None, "no entry has the name");
        assert_eq!(agent(&dir.path().join("none.args")).find(), None, "no classpath file");
    }

    #[test]
    fn the_agent_goes_after_the_named_argument_and_into_the_shown_line() {
        let line = CommandLine {
            program: PathBuf::from("/jdk/bin/java"),
            args: vec!["-agentlib:jdwp=x".into(), "-cp".into(), "@cp".into(), "Main".into()],
            display: "java -agentlib:jdwp=x -cp @cp Main".into(),
            ..CommandLine::default()
        };
        let applied = agent(Path::new("/cp")).applied(&line, Path::new("/lib/probe-agent-1.jar"));
        assert_eq!(applied.args[1], "-javaagent:/lib/probe-agent-1.jar");
        assert_eq!(applied.display, "java -agentlib:jdwp=x -javaagent:/lib/probe-agent-1.jar -cp @cp Main");
        let plain = CommandLine { args: vec!["Main".into()], display: "java Main".into(), ..CommandLine::default() };
        let first = agent(Path::new("/cp")).applied(&plain, Path::new("/lib/probe-agent-1.jar"));
        assert_eq!(first.args[0], "-javaagent:/lib/probe-agent-1.jar");
        assert_eq!(first.display, "java -javaagent:/lib/probe-agent-1.jar Main");
    }

    #[cfg(unix)]
    #[test]
    fn a_private_folder_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("zen-run-x").join("single");
        create_private_folder(&folder).unwrap();
        assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
        create_private_folder(&folder).unwrap();
    }

    #[test]
    fn field_kinds_serialise_with_a_type_tag() {
        let choice = serde_json::to_value(FieldKind::Choice(vec!["a".to_owned()])).unwrap();
        assert_eq!(choice, serde_json::json!({ "type": "choice", "values": ["a"] }));
        assert_eq!(serde_json::to_value(FieldKind::Path).unwrap(), serde_json::json!({ "type": "path" }));
    }
}
