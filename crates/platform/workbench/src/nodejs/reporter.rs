use std::path::{Path, PathBuf};

use serde::Deserialize;
use zen_run::{OutputParser, ParsedLine, Stream, TestEvent, TestStatus};

pub const JEST: &str = include_str!("jest-reporter.cjs");
pub const VITEST: &str = include_str!("vitest-reporter.mjs");

const PREFIX: &str = "##zen-test ";
const SEPARATOR: char = '/';
const SLASH_IN_NAME: char = '\u{2215}';

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Line {
    event: String,
    file: String,
    test: Option<Vec<String>>,
    status: String,
    duration_ms: Option<f64>,
}

pub fn tree_name(parts: &[String]) -> String {
    parts.iter().map(|part| part.replace(SEPARATOR, &SLASH_IN_NAME.to_string())).collect::<Vec<_>>().join("/")
}

pub fn runner_name(tree: &str) -> String {
    tree.replace(SEPARATOR, " ").replace(SLASH_IN_NAME, "/")
}

#[derive(Debug)]
struct Open {
    package: String,
    test: String,
    failed: bool,
}

#[derive(Debug)]
pub struct NodeTestParser {
    root: PathBuf,
    open: Vec<Open>,
    current: Option<(String, Option<String>)>,
}

impl NodeTestParser {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_owned(), open: Vec::new(), current: None }
    }

    fn package(&self, file: &str) -> String {
        let path = Path::new(file);
        let real = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        path.strip_prefix(&self.root)
            .or_else(|_| path.strip_prefix(&real))
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn open_parents(&mut self, package: &str, tree: &str, events: &mut Vec<TestEvent>) {
        let mut prefix = String::new();
        let parts: Vec<&str> = tree.split(SEPARATOR).collect();
        for part in &parts[..parts.len() - 1] {
            prefix = if prefix.is_empty() { (*part).to_owned() } else { format!("{prefix}/{part}") };
            if !self.open.iter().any(|open| open.package == package && open.test == prefix) {
                self.open.push(Open { package: package.to_owned(), test: prefix.clone(), failed: false });
                events.push(TestEvent::Started { package: package.to_owned(), test: Some(prefix.clone()) });
            }
        }
    }

    fn finished(&mut self, line: &Line) -> Vec<TestEvent> {
        let package = self.package(&line.file);
        let status = status_of(&line.status);
        let mut events = Vec::new();
        let duration_ms = line.duration_ms.map(|ms| ms.max(0.0) as u64);
        match &line.test {
            Some(parts) => {
                let test = tree_name(parts);
                self.open_parents(&package, &test, &mut events);
                events.push(TestEvent::Started { package: package.clone(), test: Some(test.clone()) });
                if status == TestStatus::Failed {
                    self.open
                        .iter_mut()
                        .filter(|open| open.package == package && test.starts_with(&format!("{}/", open.test)))
                        .for_each(|open| open.failed = true);
                }
                events.push(TestEvent::Finished { package, test: Some(test), status, duration_ms });
            }
            None => {
                let mut left = Vec::new();
                for open in std::mem::take(&mut self.open) {
                    if open.package == package {
                        left.push(open);
                    } else {
                        self.open.push(open);
                    }
                }
                for open in left.into_iter().rev() {
                    let status = if open.failed { TestStatus::Failed } else { TestStatus::Passed };
                    events.push(TestEvent::Finished {
                        package: package.clone(),
                        test: Some(open.test),
                        status,
                        duration_ms: None,
                    });
                }
                events.push(TestEvent::Finished { package, test: None, status, duration_ms });
            }
        }
        events
    }

    fn event(&mut self, line: &Line) -> Vec<TestEvent> {
        match line.event.as_str() {
            "fileStarted" => vec![TestEvent::Started { package: self.package(&line.file), test: None }],
            "testStarted" => {
                let package = self.package(&line.file);
                let test = tree_name(line.test.as_deref().unwrap_or_default());
                let mut events = Vec::new();
                self.open_parents(&package, &test, &mut events);
                events.push(TestEvent::Started { package, test: Some(test) });
                events
            }
            "output" => {
                self.current = Some((self.package(&line.file), line.test.as_deref().map(tree_name)));
                Vec::new()
            }
            "outputEnd" => {
                self.current = None;
                Vec::new()
            }
            "testFinished" | "fileFinished" => self.finished(line),
            _ => Vec::new(),
        }
    }
}

fn status_of(status: &str) -> TestStatus {
    match status {
        "passed" => TestStatus::Passed,
        "failed" => TestStatus::Failed,
        _ => TestStatus::Skipped,
    }
}

impl OutputParser for NodeTestParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        if let Some(json) = text.strip_prefix(PREFIX)
            && let Ok(line) = serde_json::from_str::<Line>(json)
        {
            return ParsedLine { console: None, events: self.event(&line) };
        }
        let mut parsed = ParsedLine::console(stream, text);
        if let Some((package, test)) = &self.current {
            parsed.events.push(TestEvent::Output { package: package.clone(), test: test.clone() });
        }
        parsed
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        self.open.clear();
        self.current = None;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(parser: &mut NodeTestParser, line: &str) -> ParsedLine {
        parser.line(Stream::Stdout, line)
    }

    #[test]
    fn reporter_lines_grow_the_tree_with_suites_and_failures_and_stay_out_of_the_console() {
        let mut parser = NodeTestParser::new(Path::new("/p"));
        let mut tree = zen_run::TestTree::default();
        let mut shown = Vec::new();
        for (at, line) in [
            r#"##zen-test {"event":"fileStarted","file":"/p/src/a.test.ts"}"#,
            r#"##zen-test {"event":"testStarted","file":"/p/src/a.test.ts","test":["math","adds"]}"#,
            r#"##zen-test {"event":"testFinished","file":"/p/src/a.test.ts","test":["math","adds"],"status":"passed","durationMs":2.5}"#,
            r#"##zen-test {"event":"output","file":"/p/src/a.test.ts","test":["math","div/zero"]}"#,
            "Expected: 1",
            r#"##zen-test {"event":"outputEnd"}"#,
            r#"##zen-test {"event":"testFinished","file":"/p/src/a.test.ts","test":["math","div/zero"],"status":"failed"}"#,
            r#"##zen-test {"event":"testFinished","file":"/p/src/a.test.ts","test":["later"],"status":"skipped"}"#,
            r#"##zen-test {"event":"fileFinished","file":"/p/src/a.test.ts","status":"failed","durationMs":9}"#,
            "plain text",
        ]
        .iter()
        .enumerate()
        {
            let parsed = feed(&mut parser, line);
            if let Some(console) = &parsed.console {
                shown.push(console.text.clone());
            }
            for event in &parsed.events {
                tree.apply(event, Some(at));
            }
        }
        assert_eq!(shown, ["Expected: 1", "plain text"]);
        let package = &tree.root().children[0];
        assert_eq!(package.name, "src/a.test.ts");
        assert_eq!(package.status, TestStatus::Failed);
        assert_eq!(package.duration_ms, Some(9));
        let suite = &package.children[0];
        assert_eq!((suite.name.as_str(), suite.status), ("math", TestStatus::Failed));
        let names: Vec<(&str, TestStatus)> =
            suite.children.iter().map(|child| (child.name.as_str(), child.status)).collect();
        assert_eq!(names, [("adds", TestStatus::Passed), ("div\u{2215}zero", TestStatus::Failed)]);
        assert_eq!(package.children[1].status, TestStatus::Skipped);
        let failed = tree.failed();
        let test = failed.iter().find(|id| id.test.contains('\u{2215}')).unwrap();
        assert_eq!(runner_name(&test.test), "math div/zero");
    }

    #[test]
    fn a_line_that_only_looks_like_a_report_is_program_output() {
        let mut parser = NodeTestParser::new(Path::new("/p"));
        let parsed = feed(&mut parser, "##zen-test not json");
        assert_eq!(parsed.console.unwrap().text, "##zen-test not json");
        assert!(parsed.events.is_empty());
    }
}
