use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::console::Stream;
use crate::coverage::{CoverageProvider, CoverageReport, parse_go_profile};
use crate::parser::{OutputParser, ParsedLine};
use crate::profile::{Profile, ProfileKind, ProfileProvider};
use crate::test_tree::{TestEvent, TestId, TestSource, TestStatus};
use crate::{
    AdapterLine, AdapterViews, CommandLine, Configuration, DebugPlan, FieldKind, FieldSpec, ForeignRunConfiguration,
    KindSpec, Replay, RunContext, RunProvider, RunTarget, ScratchStarter,
};

pub const KIND: &str = "fake.run";

pub const FAKE_ADAPTER: &str = "fake-dap";

pub const FAKE_ADDRESS_FLAG: &str = "--client-addr";

pub const ADOPTED_TYPE: &str = "FakeRunConfiguration";

#[derive(Clone, Default)]
pub struct FakeProvider {
    script: Arc<Mutex<Replay>>,
    targets: Arc<Mutex<Vec<RunTarget>>>,
    context: Arc<Mutex<Option<Configuration>>>,
    single_file: Arc<Mutex<Vec<RunTarget>>>,
    replays: Arc<AtomicUsize>,
    coverage: Arc<Mutex<Option<String>>>,
    scratch_home: Arc<Mutex<Option<(String, PathBuf)>>>,
    profile: Arc<Mutex<Option<Vec<u8>>>>,
}

impl FakeProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_script(&self, lines: &[(Stream, &str)], exit_code: i32) {
        if let Ok(mut script) = self.script.lock() {
            *script =
                Replay { lines: lines.iter().map(|(stream, text)| (*stream, (*text).to_owned())).collect(), exit_code };
        }
    }

    pub fn set_scratch_home(&self, import: &str, home: PathBuf) {
        if let Ok(mut slot) = self.scratch_home.lock() {
            *slot = Some((import.to_owned(), home));
        }
    }

    pub fn set_targets(&self, targets: Vec<RunTarget>) {
        if let Ok(mut slot) = self.targets.lock() {
            *slot = targets;
        }
    }

    pub fn set_context(&self, configuration: Option<Configuration>) {
        if let Ok(mut slot) = self.context.lock() {
            *slot = configuration;
        }
    }

    pub fn set_single_file(&self, targets: Vec<RunTarget>) {
        if let Ok(mut slot) = self.single_file.lock() {
            *slot = targets;
        }
    }

    pub fn set_profile(&self, profile: Option<Vec<u8>>) {
        if let Ok(mut slot) = self.profile.lock() {
            *slot = profile;
        }
    }

    pub fn set_coverage(&self, profile: Option<&str>) {
        if let Ok(mut slot) = self.coverage.lock() {
            *slot = profile.map(str::to_owned);
        }
    }

    pub fn replays(&self) -> usize {
        self.replays.load(Ordering::SeqCst)
    }

    pub fn configuration(name: &str) -> Configuration {
        Configuration {
            name: name.to_owned(),
            kind: KIND.to_owned(),
            options: serde_json::json!({ "target": name }),
            ..Configuration::default()
        }
    }
}

impl RunProvider for FakeProvider {
    fn id(&self) -> &str {
        "fake"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "fake-run-kind".to_owned(),
            fields: vec![FieldSpec {
                key: "target".to_owned(),
                label_message: "fake-run-target".to_owned(),
                kind: FieldKind::Text,
            }],
            defaults: serde_json::json!({ "target": "", "fail": null, "noDebug": null, "noDebugger": null, "views": null }),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        self.targets.lock().map(|targets| targets.clone()).unwrap_or_default()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        self.context.lock().ok().and_then(|context| context.clone())
    }

    fn scratch_home(&self, _file: &Path, text: &str, context: &RunContext) -> Option<PathBuf> {
        let slot = self.scratch_home.lock().ok()?;
        let (import, home) = slot.as_ref()?;
        (text.contains(import.as_str()) && !context.modules.is_empty()).then(|| home.clone())
    }

    fn scratch_starter(&self, language: &str) -> Option<ScratchStarter> {
        (language == "yaml").then(|| ScratchStarter { text: "key: \n".to_owned(), caret: 5 })
    }

    fn single_file(&self, _path: &Path, _text: &str) -> Vec<RunTarget> {
        self.single_file.lock().map(|targets| targets.clone()).unwrap_or_default()
    }

    fn adopt(&self, foreign: &ForeignRunConfiguration, _root: &Path) -> Option<Configuration> {
        (foreign.type_id == ADOPTED_TYPE).then(|| Configuration {
            name: foreign.name.clone(),
            kind: KIND.to_owned(),
            options: serde_json::json!({ "target": foreign.options.get("target").cloned().unwrap_or_default() }),
            ..Configuration::default()
        })
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        if let Some(message) = configuration.options.get("fail").and_then(|fail| fail.as_str()) {
            return Err(message.to_owned());
        }
        Ok(vec![CommandLine {
            program: PathBuf::from("fake"),
            args: vec![configuration.name.clone()],
            cwd: configuration.common.working_directory_in(&context.root),
            env: configuration.common.env.clone(),
            display: format!("fake {}", configuration.name),
        }])
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(FakeParser::default())
    }

    fn rerun_failed(&self, configuration: &Configuration, failed: &[TestId]) -> Option<Configuration> {
        let tests: Vec<&str> = failed.iter().map(|id| id.test.as_str()).collect();
        Some(Configuration {
            id: String::new(),
            name: format!("{} (failed)", configuration.name),
            temporary: true,
            options: serde_json::json!({ "target": configuration.name, "tests": tests }),
            ..configuration.clone()
        })
    }

    fn test_source(&self, _configuration: &Configuration, _test: &TestId, context: &RunContext) -> Option<TestSource> {
        Some(TestSource { path: context.root.join("main_test.go"), line: 2 })
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        if configuration.options.get("noDebug").and_then(serde_json::Value::as_bool) == Some(true) {
            return None;
        }
        if let Some(message) = configuration.options.get("fail").and_then(|fail| fail.as_str()) {
            return Some(Err(message.to_owned()));
        }
        let cwd = configuration.common.working_directory_in(&context.root);
        let missing = configuration.options.get("noDebugger").and_then(serde_json::Value::as_bool) == Some(true);
        let line = CommandLine {
            program: if missing { PathBuf::new() } else { PathBuf::from(FAKE_ADAPTER) },
            args: vec![configuration.name.clone()],
            cwd: cwd.clone(),
            env: configuration.common.env.clone(),
            display: format!("{FAKE_ADAPTER} {}", configuration.name),
        };
        let views = configuration.options.get("views").and_then(|views| views.as_str()).map(|command| AdapterViews {
            command: command.to_owned(),
            logical: vec![serde_json::json!({ "raw": false })],
            raw: vec![serde_json::json!({ "raw": true })],
        });
        let launch = serde_json::json!({
            "request": "launch",
            "mode": "exec",
            "program": configuration.name,
            "cwd": cwd,
        });
        Some(Ok(DebugPlan {
            build: Vec::new(),
            adapter: AdapterLine {
                line,
                address_flag: FAKE_ADDRESS_FLAG.to_owned(),
                server: None,
                views,
                builtin: None,
            },
            launch,
            debuggee: None,
        }))
    }

    fn replay(&self, _configuration: &Configuration) -> Option<Replay> {
        self.replays.fetch_add(1, Ordering::SeqCst);
        self.script.lock().ok().map(|script| script.clone())
    }

    fn coverage(&self, configuration: &Configuration) -> Option<&dyn CoverageProvider> {
        let refused = configuration.options.get("noCoverage").and_then(serde_json::Value::as_bool) == Some(true);
        let scripted = self.coverage.lock().is_ok_and(|profile| profile.is_some());
        (scripted && !refused).then_some(self as &dyn CoverageProvider)
    }

    fn profiler(&self, _configuration: &Configuration) -> Option<&dyn ProfileProvider> {
        let scripted = self.profile.lock().is_ok_and(|profile| profile.is_some());
        scripted.then_some(self as &dyn ProfileProvider)
    }
}

impl ProfileProvider for FakeProvider {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        _kind: ProfileKind,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let profile = self.profile.lock().ok().and_then(|profile| profile.clone()).unwrap_or_default();
        std::fs::write(report, profile).map_err(|error| error.to_string())?;
        RunProvider::command_lines(self, configuration, context)
    }

    fn read(&self, _configuration: &Configuration, context: &RunContext, report: &Path) -> Result<Profile, String> {
        crate::pprof::read(report, |file| context.root.join(file))
    }
}

fn varint(mut value: u64, into: &mut Vec<u8>) {
    while value >= 0x80 {
        into.push(value as u8 | 0x80);
        value >>= 7;
    }
    into.push(value as u8);
}

fn number(tag: u64, value: u64, into: &mut Vec<u8>) {
    varint(tag << 3, into);
    varint(value, into);
}

fn message(tag: u64, body: &[u8], into: &mut Vec<u8>) {
    varint(tag << 3 | 2, into);
    varint(body.len() as u64, into);
    into.extend_from_slice(body);
}

pub fn sample_pprof() -> Vec<u8> {
    let mut out = Vec::new();
    let mut metric = Vec::new();
    number(1, 1, &mut metric);
    number(2, 2, &mut metric);
    message(1, &metric, &mut out);
    for (leaf, cost) in [(2, 3_000), (1, 1_000)] {
        let mut sample = Vec::new();
        number(1, leaf, &mut sample);
        if leaf == 2 {
            number(1, 1, &mut sample);
        }
        number(2, cost, &mut sample);
        message(2, &sample, &mut out);
    }
    for id in [1, 2] {
        let mut line = Vec::new();
        number(1, id, &mut line);
        let mut location = Vec::new();
        number(1, id, &mut location);
        message(4, &line, &mut location);
        message(4, &location, &mut out);
    }
    for (id, name, line) in [(1, 3, 1), (2, 5, 5)] {
        let mut function = Vec::new();
        number(1, id, &mut function);
        number(2, name, &mut function);
        number(4, 4, &mut function);
        number(5, line, &mut function);
        message(5, &function, &mut out);
    }
    for text in ["", "cpu", "nanoseconds", "main.main", "main.go", "main.work"] {
        message(6, text.as_bytes(), &mut out);
    }
    out
}

pub fn read_go_profile(report: &Path, root: &Path) -> Result<CoverageReport, String> {
    let text = std::fs::read_to_string(report).map_err(|error| error.to_string())?;
    parse_go_profile(&text, |file| root.join(file)).map_err(|error| format!("line {}: {}", error.line, error.text))
}

impl CoverageProvider for FakeProvider {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let profile = self.coverage.lock().ok().and_then(|profile| profile.clone()).unwrap_or_default();
        if let Some(folder) = report.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        std::fs::write(report, profile).map_err(|error| error.to_string())?;
        let mut lines = RunProvider::command_lines(self, configuration, context)?;
        for line in &mut lines {
            line.display.push_str(" -cover");
        }
        Ok(lines)
    }

    fn read(
        &self,
        _configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String> {
        read_go_profile(report, &context.root)
    }
}

pub const SHELL_KIND: &str = "fake.shell";

#[derive(Clone, Copy, Debug, Default)]
pub struct ShellProvider;

impl ShellProvider {
    pub fn configuration(name: &str, script: &str) -> Configuration {
        Configuration {
            name: name.to_owned(),
            kind: SHELL_KIND.to_owned(),
            options: serde_json::json!({ "script": script }),
            ..Configuration::default()
        }
    }
}

impl RunProvider for ShellProvider {
    fn id(&self) -> &str {
        "shell"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: SHELL_KIND.to_owned(),
            name_message: "fake-shell-kind".to_owned(),
            fields: Vec::new(),
            defaults: serde_json::json!({ "script": "" }),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let script = configuration.options["script"].as_str().unwrap_or_default().to_owned();
        Ok(vec![CommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), script.clone()],
            cwd: configuration.common.working_directory_in(&context.root),
            env: configuration.common.env.clone(),
            display: format!("sh -c {script}"),
        }])
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(crate::PlainParser)
    }

    fn coverage(&self, _configuration: &Configuration) -> Option<&dyn CoverageProvider> {
        Some(self)
    }
}

pub const COVERAGE_REPORT_VARIABLE: &str = "ZEN_COVERAGE_REPORT";

impl CoverageProvider for ShellProvider {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String> {
        let mut lines = RunProvider::command_lines(self, configuration, context)?;
        for line in &mut lines {
            line.env.push((COVERAGE_REPORT_VARIABLE.to_owned(), report.to_string_lossy().into_owned()));
        }
        Ok(lines)
    }

    fn read(
        &self,
        _configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String> {
        read_go_profile(report, &context.root)
    }
}

#[derive(Default)]
struct FakeParser {
    running: Vec<(String, Option<String>)>,
}

impl OutputParser for FakeParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut words = text.split_whitespace();
        let verb = text.strip_prefix('@').and_then(|_| words.next());
        let package = words.next().unwrap_or_default().to_owned();
        let test = words.next().filter(|test| *test != "-").map(str::to_owned);
        let duration_ms = words.next().and_then(|ms| ms.parse().ok());
        let status = match verb {
            Some("@start") => {
                self.running.push((package.clone(), test.clone()));
                return ParsedLine { console: None, events: vec![TestEvent::Started { package, test }] };
            }
            Some("@pass") => TestStatus::Passed,
            Some("@fail") => TestStatus::Failed,
            Some("@skip") => TestStatus::Skipped,
            _ => {
                let mut parsed = ParsedLine::console(stream, text);
                if let Some((package, test)) = self.running.last().cloned() {
                    parsed.events.push(TestEvent::Output { package, test });
                }
                return parsed;
            }
        };
        self.running.retain(|running| *running != (package.clone(), test.clone()));
        ParsedLine { console: None, events: vec![TestEvent::Finished { package, test, status, duration_ms }] }
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_debugs_with_its_marked_adapter() {
        let provider = FakeProvider::new();
        let context = RunContext { root: PathBuf::from("/p"), ..RunContext::default() };
        let plan = provider.debug(&FakeProvider::configuration("app"), &context).unwrap().unwrap();
        assert!(plan.build.is_empty());
        assert_eq!(plan.adapter.line.program, PathBuf::from(FAKE_ADAPTER));
        assert_eq!(plan.adapter.address_flag, "--client-addr");
        assert_eq!(plan.launch["program"], "app");
        let mut failing = FakeProvider::configuration("x");
        failing.options = serde_json::json!({ "fail": "no" });
        assert_eq!(provider.debug(&failing, &context), Some(Err("no".to_owned())));
        failing.options = serde_json::json!({ "noDebugger": true });
        let missing = provider.debug(&failing, &context).unwrap().unwrap();
        assert!(missing.adapter.line.program.as_os_str().is_empty(), "a plan without a debugger");
        failing.options = serde_json::json!({ "noDebug": true });
        assert_eq!(provider.debug(&failing, &context), None);
        assert_eq!(ShellProvider.debug(&failing, &context), None);
    }

    #[test]
    fn the_fake_parser_reports_scripted_tests() {
        let mut parser = FakeParser::default();
        assert_eq!(
            parser.line(Stream::Stdout, "@start p T").events,
            vec![TestEvent::Started { package: "p".to_owned(), test: Some("T".to_owned()) }]
        );
        let output = parser.line(Stream::Stdout, "hello");
        assert_eq!(output.console.map(|line| line.text), Some("hello".to_owned()));
        assert_eq!(output.events, vec![TestEvent::Output { package: "p".to_owned(), test: Some("T".to_owned()) }]);
        let finished = parser.line(Stream::Stdout, "@fail p T 7");
        assert_eq!(
            finished.events,
            vec![TestEvent::Finished {
                package: "p".to_owned(),
                test: Some("T".to_owned()),
                status: TestStatus::Failed,
                duration_ms: Some(7)
            }]
        );
        assert!(parser.line(Stream::Stdout, "after").events.is_empty());
        assert_eq!(parser.line(Stream::Stdout, "@pass p - 9").events.len(), 1);
    }
}
