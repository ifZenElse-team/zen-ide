use std::path::{Path, PathBuf};

use serde::Deserialize;
use zen_run::{ConsoleLine, OutputParser, ParsedLine, Stream, TestEvent, TestStatus};

pub const GRADLE_MARKER: &str = "##zen-test ";

pub struct JavaTestParser {
    class: Option<String>,
    class_ms: Option<u64>,
    class_failed: bool,
    trace_of: Option<(String, String)>,
    roots: Vec<PathBuf>,
    parents: Vec<(String, bool)>,
}

impl JavaTestParser {
    pub fn new(root: &Path) -> Self {
        Self {
            class: None,
            class_ms: None,
            class_failed: false,
            trace_of: None,
            roots: source_roots(root),
            parents: Vec::new(),
        }
    }

    fn console(&self, stream: Stream, text: &str) -> ConsoleLine {
        let mut line = ConsoleLine::new(stream, text);
        link_frames(&mut line, &self.roots);
        line
    }

    fn close_class(&mut self, events: &mut Vec<TestEvent>) {
        if let Some(class) = self.class.take() {
            for (parent, failed) in std::mem::take(&mut self.parents) {
                let status = if failed { TestStatus::Failed } else { TestStatus::Passed };
                events.push(TestEvent::Finished {
                    package: class.clone(),
                    test: Some(parent),
                    status,
                    duration_ms: None,
                });
            }
            let status = if self.class_failed { TestStatus::Failed } else { TestStatus::Passed };
            events.push(TestEvent::Finished { package: class, test: None, status, duration_ms: self.class_ms.take() });
        }
        self.class_failed = false;
        self.trace_of = None;
    }

    fn surefire(&mut self, stream: Stream, raw: &str) -> ParsedLine {
        let text = strip_level(raw);
        let mut events = Vec::new();
        if let Some(class) = text.strip_prefix("Running ").map(str::trim).filter(|class| is_class_name(class)) {
            self.close_class(&mut events);
            self.class = Some(class.to_owned());
            events.push(TestEvent::Started { package: class.to_owned(), test: None });
        } else if text.starts_with("Tests run:")
            && let Some(class) = summary_class(text)
        {
            if self.class.as_deref() != Some(class) {
                self.close_class(&mut events);
                self.class = Some(class.to_owned());
                events.push(TestEvent::Started { package: class.to_owned(), test: None });
            }
            self.class_ms = elapsed_ms(text);
            self.class_failed = text.contains("<<< FAILURE!") || text.contains("<<< ERROR!");
            self.trace_of = None;
        } else if text.starts_with("Tests run:") || text == "Results:" || text.starts_with("BUILD ") {
            self.close_class(&mut events);
        } else if let Some(class) = self.class.clone()
            && let Some((test, status, ms)) = test_line(text, &class)
        {
            events.push(TestEvent::Started { package: class.clone(), test: Some(test.clone()) });
            events.push(TestEvent::Output { package: class.clone(), test: Some(test.clone()) });
            events.push(TestEvent::Finished {
                package: class.clone(),
                test: Some(test.clone()),
                status,
                duration_ms: ms,
            });
            if let Some((parent, _)) = test.split_once('/') {
                match self.parents.iter_mut().find(|(known, _)| known == parent) {
                    Some((_, failed)) => *failed |= status == TestStatus::Failed,
                    None => self.parents.push((parent.to_owned(), status == TestStatus::Failed)),
                }
            }
            self.trace_of = (status == TestStatus::Failed).then_some((class, test));
        } else if let Some((class, test)) = &self.trace_of {
            if raw.trim().is_empty() {
                self.trace_of = None;
            } else {
                events.push(TestEvent::Output { package: class.clone(), test: Some(test.clone()) });
            }
        } else if let Some(class) = &self.class
            && (!raw.starts_with('[') || text.starts_with("[stdout]") || text.starts_with("[stderr]"))
        {
            events.push(TestEvent::Output { package: class.clone(), test: None });
        }
        ParsedLine { console: Some(self.console(stream, raw)), events }
    }

    fn gradle(&mut self, json: &str) -> ParsedLine {
        let Ok(report) = serde_json::from_str::<GradleReport>(json) else {
            return ParsedLine { console: None, events: Vec::new() };
        };
        let class = report.suite.clone();
        let test = || Some(test_name(&report.test));
        match report.event.as_str() {
            "suiteStarted" if report.test == report.suite => {
                ParsedLine { console: None, events: vec![TestEvent::Started { package: class, test: None }] }
            }
            "suiteFinished" if report.test == report.suite => ParsedLine {
                console: None,
                events: vec![TestEvent::Finished {
                    package: class,
                    test: None,
                    status: status(&report.result),
                    duration_ms: report.ms,
                }],
            },
            "suiteFinished" => ParsedLine {
                console: None,
                events: vec![TestEvent::Finished {
                    package: class,
                    test: test(),
                    status: status(&report.result),
                    duration_ms: report.ms,
                }],
            },
            "testStarted" => {
                ParsedLine { console: None, events: vec![TestEvent::Started { package: class, test: test() }] }
            }
            "testFinished" => ParsedLine {
                console: None,
                events: vec![TestEvent::Finished {
                    package: class,
                    test: test(),
                    status: status(&report.result),
                    duration_ms: report.ms,
                }],
            },
            "output" => {
                let stream = if report.stream == "StdErr" { Stream::Stderr } else { Stream::Stdout };
                let text = report.text.strip_suffix('\n').unwrap_or(&report.text);
                let text = text.strip_suffix('\r').unwrap_or(text);
                ParsedLine {
                    console: Some(self.console(stream, text)),
                    events: vec![TestEvent::Output { package: class, test: test() }],
                }
            }
            _ => ParsedLine { console: None, events: Vec::new() },
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GradleReport {
    event: String,
    suite: String,
    test: String,
    result: String,
    ms: Option<u64>,
    stream: String,
    text: String,
}

impl OutputParser for JavaTestParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        match text.strip_prefix(GRADLE_MARKER) {
            Some(json) => self.gradle(json),
            None => self.surefire(stream, text),
        }
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        let mut events = Vec::new();
        self.close_class(&mut events);
        events
    }
}

fn strip_level(line: &str) -> &str {
    for level in ["[INFO] ", "[ERROR] ", "[WARNING] ", "[WARN] ", "[DEBUG] "] {
        if let Some(rest) = line.strip_prefix(level) {
            return rest.trim_end();
        }
    }
    line.trim_end()
}

fn is_class_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'))
}

fn summary_class(text: &str) -> Option<&str> {
    let (_, class) = text.rsplit_once(" in ")?;
    let class = class.trim();
    is_class_name(class).then_some(class)
}

fn elapsed_ms(text: &str) -> Option<u64> {
    let (_, rest) = text.split_once("Time elapsed: ")?;
    let number: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let seconds: f64 = number.parse().ok()?;
    let unit = rest[number.len()..].trim_start();
    let ms = if unit.starts_with("ms") { seconds } else { seconds * 1000.0 };
    Some(ms.round() as u64)
}

fn test_line(text: &str, class: &str) -> Option<(String, TestStatus, Option<u64>)> {
    let (name, rest) = if let Some(rest) = text.strip_prefix(class).and_then(|rest| rest.strip_prefix('.')) {
        let end = rest.find(" -- ").or_else(|| rest.find(" skipped")).or_else(|| rest.find("  Time elapsed"))?;
        (&rest[..end], &rest[end..])
    } else {
        let marker = format!("({class})");
        let at = text.find(&marker)?;
        (&text[..at], &text[at + marker.len()..])
    };
    if name.is_empty() || name.contains(' ') && !name.contains('(') {
        return None;
    }
    let status = if rest.contains("<<< FAILURE!") || rest.contains("<<< ERROR!") {
        TestStatus::Failed
    } else if rest.trim_start().starts_with("skipped") {
        TestStatus::Skipped
    } else if rest.contains("Time elapsed") {
        TestStatus::Passed
    } else {
        return None;
    };
    Some((test_name(name), status, elapsed_ms(rest)))
}

pub fn test_name(raw: &str) -> String {
    let raw = raw.strip_suffix("()").unwrap_or(raw);
    match raw.find(")[") {
        Some(at) => format!("{}/{}", &raw[..=at], &raw[at + 1..]),
        None => raw.to_owned(),
    }
}

fn status(result: &str) -> TestStatus {
    match result {
        "SUCCESS" => TestStatus::Passed,
        "SKIPPED" => TestStatus::Skipped,
        _ => TestStatus::Failed,
    }
}

pub fn source_roots(root: &Path) -> Vec<PathBuf> {
    fn walk(folder: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 5 || out.len() > 200 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_dir()
                || name.starts_with('.')
                || matches!(name.as_str(), "target" | "build" | "node_modules" | "out" | "bin")
            {
                continue;
            }
            if (name == "java" || name == "kotlin")
                && path.parent().and_then(Path::parent).is_some_and(|src| src.ends_with("src"))
            {
                out.push(path);
            } else {
                walk(&path, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out.sort();
    out
}

fn link_frames(line: &mut ConsoleLine, roots: &[PathBuf]) {
    let text = line.text.trim_start();
    let Some(frame) = text.strip_prefix("at ") else { return };
    let Some(open) = frame.find('(') else { return };
    let method = &frame[..open];
    let method = method.rsplit('/').next().unwrap_or(method);
    let Some((class, _)) = method.rsplit_once('.') else { return };
    let package = class.rsplit_once('.').map_or("", |(package, _)| package);
    line.links.retain_mut(|link| {
        if !link.is_bare() {
            return true;
        }
        let relative = Path::new(&package.replace('.', "/")).join(&link.file);
        match roots.iter().map(|root| root.join(&relative)).find(|path| path.is_file()) {
            Some(found) => {
                link.file = found;
                true
            }
            None => false,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zen_run::{RunOutput, TestTree};

    fn run(lines: &[&str], root: &Path) -> RunOutput {
        let mut output = RunOutput::new(Box::new(JavaTestParser::new(root)));
        for line in lines {
            output.feed(Stream::Stdout, &format!("{line}\n"));
        }
        output.end(zen_run::RunState::Finished, Some(1));
        output
    }

    fn summary(tree: &TestTree) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(node: &zen_run::TestNode, depth: usize, out: &mut Vec<String>) {
            for child in &node.children {
                out.push(format!("{}{} {:?} {:?}", "  ".repeat(depth), child.name, child.status, child.duration_ms));
                walk(child, depth + 1, out);
            }
        }
        walk(tree.root(), 0, &mut out);
        out
    }

    fn sources() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("src/test/java/com/acme");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("CalcTest.java"), "class CalcTest {}").unwrap();
        std::fs::create_dir_all(root.path().join("target/src/test/java")).unwrap();
        root
    }

    const SUREFIRE: &[&str] = &[
        "[INFO] -------------------------------------------------------",
        "[INFO]  T E S T S",
        "[INFO] Running com.acme.CalcTest",
        "[INFO] [stdout] adding",
        "[ERROR] Tests run: 5, Failures: 1, Errors: 0, Skipped: 1, Time elapsed: 0.028 s <<< FAILURE! -- in com.acme.CalcTest",
        "[ERROR] com.acme.CalcTest.adds -- Time elapsed: 0.006 s",
        "[ERROR] com.acme.CalcTest.fails -- Time elapsed: 0.003 s <<< FAILURE!",
        "org.opentest4j.AssertionFailedError: two and two ==> expected: <5> but was: <4>",
        "\tat org.junit.jupiter.api.AssertEquals.assertEquals(AssertEquals.java:150)",
        "\tat com.acme.CalcTest.fails(CalcTest.java:19)",
        "",
        "[ERROR] com.acme.CalcTest.positive(int)[1] -- Time elapsed: 0.003 s",
        "[ERROR] com.acme.CalcTest.positive(int)[2] -- Time elapsed: 0.001 s",
        "[ERROR] com.acme.CalcTest.skipped skipped",
        "[INFO] ",
        "[INFO] Results:",
        "[ERROR] Tests run: 5, Failures: 1, Errors: 0, Skipped: 1",
        "[ERROR] BUILD FAILURE",
    ];

    #[test]
    fn surefire_plain_reports_make_the_tree() {
        let root = sources();
        let output = run(SUREFIRE, root.path());
        assert_eq!(
            summary(output.tree()),
            [
                "com.acme.CalcTest Failed Some(28)",
                "  adds Passed Some(6)",
                "  fails Failed Some(3)",
                "  positive(int) Passed None",
                "    [1] Passed Some(3)",
                "    [2] Passed Some(1)",
                "  skipped Skipped None",
            ]
        );
        let fails = &output.tree().root().children[0].children[1];
        let lines: Vec<&ConsoleLine> =
            fails.lines.iter().flat_map(|range| range.clone()).map(|index| &output.console().lines()[index]).collect();
        assert_eq!(lines.len(), 4, "{lines:?}");
        let frame = lines.iter().find(|line| line.text.contains("CalcTest.java:19")).unwrap();
        assert_eq!(frame.links[0].file, root.path().join("src/test/java/com/acme/CalcTest.java"));
        let library = lines.iter().find(|line| line.text.contains("AssertEquals.java")).unwrap();
        assert!(library.links.is_empty(), "a library frame has no link");
        let class = &output.tree().root().children[0];
        assert!(
            class
                .lines
                .iter()
                .flat_map(|range| range.clone())
                .any(|index| output.console().lines()[index].text.contains("adding"))
        );
    }

    #[test]
    fn gradle_reports_make_the_same_tree() {
        let root = sources();
        let lines = [
            r#"> Task :test"#,
            r#"##zen-test {"event":"suiteStarted","suite":"com.acme.CalcTest","test":"com.acme.CalcTest","display":"CalcTest"}"#,
            r#"##zen-test {"event":"testStarted","suite":"com.acme.CalcTest","test":"adds()","display":"adds()"}"#,
            r#"##zen-test {"event":"output","stream":"StdOut","text":"adding\n","suite":"com.acme.CalcTest","test":"adds()"}"#,
            r#"##zen-test {"event":"testFinished","result":"SUCCESS","ms":11,"suite":"com.acme.CalcTest","test":"adds()"}"#,
            r#"##zen-test {"event":"testStarted","suite":"com.acme.CalcTest","test":"fails()"}"#,
            r#"##zen-test {"event":"output","stream":"StdErr","text":"org.opentest4j.AssertionFailedError: two","suite":"com.acme.CalcTest","test":"fails()"}"#,
            r#"##zen-test {"event":"output","stream":"StdErr","text":"\tat app//com.acme.CalcTest.fails(CalcTest.java:19)","suite":"com.acme.CalcTest","test":"fails()"}"#,
            r#"##zen-test {"event":"testFinished","result":"FAILURE","ms":3,"suite":"com.acme.CalcTest","test":"fails()"}"#,
            r#"##zen-test {"event":"suiteStarted","suite":"com.acme.CalcTest","test":"positive(int)"}"#,
            r#"##zen-test {"event":"testStarted","suite":"com.acme.CalcTest","test":"positive(int)[1]"}"#,
            r#"##zen-test {"event":"testFinished","result":"SUCCESS","ms":4,"suite":"com.acme.CalcTest","test":"positive(int)[1]"}"#,
            r#"##zen-test {"event":"suiteFinished","result":"SUCCESS","ms":14,"suite":"com.acme.CalcTest","test":"positive(int)"}"#,
            r#"##zen-test {"event":"testStarted","suite":"com.acme.CalcTest","test":"skipped()"}"#,
            r#"##zen-test {"event":"testFinished","result":"SKIPPED","ms":0,"suite":"com.acme.CalcTest","test":"skipped()"}"#,
            r#"##zen-test {"event":"suiteFinished","result":"FAILURE","ms":39,"suite":"com.acme.CalcTest","test":"com.acme.CalcTest"}"#,
            "5 tests completed, 1 failed, 1 skipped",
        ];
        let output = run(&lines, root.path());
        let tree = summary(output.tree());
        assert_eq!(tree[0], "com.acme.CalcTest Failed Some(39)");
        assert!(tree.contains(&"  adds Passed Some(11)".to_owned()), "{tree:?}");
        assert!(tree.contains(&"  fails Failed Some(3)".to_owned()), "{tree:?}");
        assert!(tree.contains(&"    [1] Passed Some(4)".to_owned()), "{tree:?}");
        assert!(tree.contains(&"  skipped Skipped Some(0)".to_owned()), "{tree:?}");
        let texts: Vec<&str> = output.console().lines().iter().map(|line| line.text.as_str()).collect();
        assert!(!texts.iter().any(|text| text.contains("##zen-test")), "the markers are not shown: {texts:?}");
        assert!(texts.contains(&"adding"));
        let frame = output.console().lines().iter().find(|line| line.text.contains("CalcTest.java:19")).unwrap();
        assert_eq!(frame.links[0].file, root.path().join("src/test/java/com/acme/CalcTest.java"));
    }

    #[test]
    fn names_durations_and_surefire_2() {
        assert_eq!(test_name("adds()"), "adds");
        assert_eq!(test_name("positive(int)[2]"), "positive(int)/[2]");
        assert_eq!(elapsed_ms("Time elapsed: 1.5 s"), Some(1500));
        assert_eq!(
            test_line("adds(com.acme.CalcTest)  Time elapsed: 0.01 sec  <<< FAILURE!", "com.acme.CalcTest"),
            Some(("adds".to_owned(), TestStatus::Failed, Some(10)))
        );
        assert_eq!(summary_class("Tests run: 1, Failures: 0 - in com.acme.A"), Some("com.acme.A"));
        assert_eq!(test_line("Downloading from central", "com.acme.CalcTest"), None);
    }
}
