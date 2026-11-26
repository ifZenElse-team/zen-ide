use std::path::PathBuf;

use zen_run::{OutputParser, ParsedLine, Stream, TestEvent, TestStatus};

use super::command::resolve_links;

#[derive(Clone, Debug)]
pub struct VerboseParser {
    package: String,
    cwd: PathBuf,
    folder: PathBuf,
    started: bool,
    done: bool,
    open: Vec<String>,
    current: Option<String>,
}

enum Line<'a> {
    Run(&'a str),
    Named(&'a str),
    Result { test: &'a str, status: TestStatus, duration_ms: Option<u64> },
    Package(TestStatus),
    Summary(TestStatus, &'a str),
    Text,
}

impl VerboseParser {
    pub fn new(package: impl Into<String>, cwd: PathBuf, folder: PathBuf) -> Self {
        Self { package: package.into(), cwd, folder, started: false, done: false, open: Vec::new(), current: None }
    }

    fn start(&mut self, events: &mut Vec<TestEvent>) {
        if !self.started {
            self.started = true;
            events.push(TestEvent::Started { package: self.package.clone(), test: None });
        }
    }

    fn finish_package(&mut self, status: TestStatus, events: &mut Vec<TestEvent>) {
        self.start(events);
        let left = match status {
            TestStatus::Passed | TestStatus::Skipped => status,
            _ => TestStatus::Failed,
        };
        for test in std::mem::take(&mut self.open).into_iter().rev() {
            events.push(TestEvent::Finished {
                package: self.package.clone(),
                test: Some(test),
                status: left,
                duration_ms: None,
            });
        }
        events.push(TestEvent::Finished { package: self.package.clone(), test: None, status, duration_ms: None });
        self.done = true;
        self.current = None;
    }

    fn output(&self, events: &mut Vec<TestEvent>) {
        events.push(TestEvent::Output { package: self.package.clone(), test: self.current.clone() });
    }
}

fn classify(text: &str) -> Line<'_> {
    let line = text.trim_start();
    if let Some(name) = line.strip_prefix("=== RUN ") {
        return Line::Run(name.trim());
    }
    for prefix in ["=== PAUSE ", "=== CONT ", "=== NAME "] {
        if let Some(name) = line.strip_prefix(prefix) {
            return Line::Named(name.trim());
        }
    }
    for (prefix, status) in [
        ("--- PASS: ", TestStatus::Passed),
        ("--- FAIL: ", TestStatus::Failed),
        ("--- SKIP: ", TestStatus::Skipped),
        ("--- BENCH: ", TestStatus::Passed),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let rest = rest.trim_end();
            let (test, duration_ms) = match rest.rsplit_once(" (") {
                Some((test, time)) => (test, seconds(time)),
                None => (rest, None),
            };
            return Line::Result { test: test.trim(), status, duration_ms };
        }
    }
    if text == line {
        match line.trim_end() {
            "PASS" => return Line::Package(TestStatus::Passed),
            "FAIL" => return Line::Package(TestStatus::Failed),
            _ => {}
        }
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("ok"), Some(package)) => return Line::Summary(TestStatus::Passed, package),
            (Some("FAIL"), Some(package)) if line.starts_with("FAIL\t") || line.starts_with("FAIL ") => {
                return Line::Summary(TestStatus::Failed, package);
            }
            _ => {}
        }
    }
    Line::Text
}

fn seconds(time: &str) -> Option<u64> {
    let value: f64 = time.trim_end_matches(')').strip_suffix('s')?.parse().ok()?;
    Some((value * 1000.0).round().max(0.0) as u64)
}

impl OutputParser for VerboseParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut parsed = ParsedLine::console(stream, text);
        resolve_links(&mut parsed, &self.cwd, &self.folder);
        let events = &mut parsed.events;
        match classify(text) {
            Line::Run(test) => {
                self.start(events);
                if !self.open.iter().any(|open| open == test) {
                    self.open.push(test.to_owned());
                }
                self.current = Some(test.to_owned());
                events.push(TestEvent::Started { package: self.package.clone(), test: Some(test.to_owned()) });
                self.output(events);
            }
            Line::Named(test) => {
                self.start(events);
                self.current = Some(test.to_owned());
                self.output(events);
            }
            Line::Result { test, status, duration_ms } => {
                self.start(events);
                self.open.retain(|open| open != test);
                self.current = Some(test.to_owned());
                self.output(events);
                events.push(TestEvent::Finished {
                    package: self.package.clone(),
                    test: Some(test.to_owned()),
                    status,
                    duration_ms,
                });
            }
            Line::Package(status) if !self.done && self.open.is_empty() => {
                self.current = None;
                self.output(events);
                self.finish_package(status, events);
            }
            Line::Summary(status, package) if !self.done && (self.open.is_empty() || package == self.package) => {
                self.current = None;
                self.output(events);
                self.finish_package(status, events);
            }
            Line::Package(_) | Line::Summary(..) if self.done => {
                self.current = None;
                self.output(events);
            }
            Line::Package(_) | Line::Summary(..) | Line::Text if self.started => self.output(events),
            Line::Package(_) | Line::Summary(..) | Line::Text => {}
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
    use std::path::Path;

    use zen_run::TestTree;

    use super::*;

    fn parse(text: &str) -> (Vec<TestEvent>, TestTree) {
        let mut parser =
            VerboseParser::new("example.com/demo/calc", PathBuf::from("/w/demo"), PathBuf::from("/w/demo/calc"));
        let mut events = Vec::new();
        let mut tree = TestTree::default();
        for (index, line) in text.lines().enumerate() {
            let parsed = parser.line(Stream::Stdout, line);
            assert!(parsed.console.is_some());
            for event in &parsed.events {
                tree.apply(event, Some(index));
            }
            events.extend(parsed.events);
        }
        events.extend(parser.finish());
        (events, tree)
    }

    fn finished(events: &[TestEvent]) -> Vec<(Option<String>, TestStatus, Option<u64>)> {
        events
            .iter()
            .filter_map(|event| match event {
                TestEvent::Finished { test, status, duration_ms, .. } => Some((test.clone(), *status, *duration_ms)),
                _ => None,
            })
            .collect()
    }

    fn name(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    #[test]
    fn passing_failing_and_skipped_tests_are_reported() {
        let (events, tree) = parse(
            "=== RUN   TestAdd\n--- PASS: TestAdd (0.01s)\n=== RUN   TestSub\n    calc_test.go:12: got 1, want 2\n\
             --- FAIL: TestSub (0.00s)\n=== RUN   TestSkip\n    calc_test.go:20: later\n--- SKIP: TestSkip (0.00s)\nFAIL\n",
        );
        assert_eq!(events[0], TestEvent::Started { package: "example.com/demo/calc".to_owned(), test: None });
        assert_eq!(
            finished(&events),
            [
                (name("TestAdd"), TestStatus::Passed, Some(10)),
                (name("TestSub"), TestStatus::Failed, Some(0)),
                (name("TestSkip"), TestStatus::Skipped, Some(0)),
                (None, TestStatus::Failed, None),
            ]
        );
        let counts = tree.counts();
        assert_eq!((counts.total, counts.passed, counts.failed, counts.skipped), (3, 1, 1, 1));
        let sub = &tree.root().children[0].children[1];
        assert_eq!(sub.name, "TestSub");
        assert_eq!(sub.lines, vec![std::ops::Range { start: 2, end: 5 }]);
    }

    #[test]
    fn subtests_nest_by_name_and_messages_follow_their_result() {
        let (events, tree) = parse(
            "=== RUN   TestTable\n=== RUN   TestTable/one\n=== RUN   TestTable/two\n\
             --- FAIL: TestTable (0.00s)\n    --- PASS: TestTable/one (0.00s)\n    --- FAIL: TestTable/two (0.00s)\n\
             \x20       calc_test.go:30: boom\nFAIL\n",
        );
        assert_eq!(
            finished(&events),
            [
                (name("TestTable"), TestStatus::Failed, Some(0)),
                (name("TestTable/one"), TestStatus::Passed, Some(0)),
                (name("TestTable/two"), TestStatus::Failed, Some(0)),
                (None, TestStatus::Failed, None),
            ]
        );
        let table = &tree.root().children[0].children[0];
        assert_eq!(table.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(table.children[1].lines, [2..3, 5..7]);
    }

    #[test]
    fn parallel_tests_pause_and_continue() {
        let (events, tree) = parse(
            "=== RUN   TestA\n=== PAUSE TestA\n=== RUN   TestB\n=== PAUSE TestB\n=== CONT  TestA\n\
             === CONT  TestB\n    b_test.go:9: hello\n=== NAME  TestA\n    a_test.go:4: world\n\
             --- PASS: TestB (0.20s)\n--- PASS: TestA (0.30s)\nPASS\n",
        );
        assert_eq!(
            finished(&events),
            [
                (name("TestB"), TestStatus::Passed, Some(200)),
                (name("TestA"), TestStatus::Passed, Some(300)),
                (None, TestStatus::Passed, None),
            ]
        );
        let package = &tree.root().children[0];
        assert_eq!(package.children[0].lines, [0..2, 4..5, 7..9, 10..11]);
        assert_eq!(package.children[1].lines, [2..4, 5..7, 9..10]);
    }

    #[test]
    fn open_tests_end_with_the_package_and_the_summary_is_not_counted_twice() {
        let (events, _) = parse("=== RUN   TestHang\npanic: boom\nFAIL\nFAIL\texample.com/demo/calc\t0.01s\n");
        assert_eq!(finished(&events), [(name("TestHang"), TestStatus::Failed, None), (None, TestStatus::Failed, None)]);
        let (events, _) = parse("=== RUN   TestA\n--- PASS: TestA (0.00s)\nok  \texample.com/demo/calc\t0.01s\n");
        assert_eq!(finished(&events), [(name("TestA"), TestStatus::Passed, Some(0)), (None, TestStatus::Passed, None)]);
    }

    #[test]
    fn a_test_printing_a_package_result_does_not_finish_the_package() {
        let (events, tree) = parse(
            "=== RUN   TestPrints\nPASS\nok  \tsomething\t0.01s\nFAIL\nFAIL other\n\
             --- FAIL: TestPrints (0.00s)\nFAIL\nFAIL\texample.com/demo/calc\t0.01s\n",
        );
        assert_eq!(
            finished(&events),
            [(name("TestPrints"), TestStatus::Failed, Some(0)), (None, TestStatus::Failed, None)]
        );
        let test = &tree.root().children[0].children[0];
        assert_eq!(test.lines, vec![0..6]);
        let (events, _) = parse("=== RUN   TestHang\nPASS\nFAIL\texample.com/demo/calc\t10.01s\n");
        assert_eq!(finished(&events), [(name("TestHang"), TestStatus::Failed, None), (None, TestStatus::Failed, None)]);
    }

    #[test]
    fn lines_before_the_first_test_are_plain_and_links_are_resolved() {
        let mut parser = VerboseParser::new("p", PathBuf::from("/w/demo"), PathBuf::from("/w/demo/calc"));
        let plain = parser.line(Stream::Stderr, "calc/calc.go:3:9: undefined: x");
        assert!(plain.events.is_empty());
        assert_eq!(plain.console.unwrap().links[0].file, Path::new("/w/demo/calc/calc.go"));
        parser.line(Stream::Stdout, "=== RUN   TestA");
        let message = parser.line(Stream::Stdout, "    calc_test.go:7: wrong");
        assert_eq!(message.events, [TestEvent::Output { package: "p".to_owned(), test: name("TestA") }]);
        let link = &message.console.unwrap().links[0];
        assert_eq!(link.file, Path::new("/w/demo/calc/calc_test.go"));
    }

    #[test]
    fn benchmark_results_and_durations_are_read() {
        assert_eq!(seconds("0.25s)"), Some(250));
        assert_eq!(seconds("x"), None);
        let (events, _) =
            parse("=== RUN   BenchmarkSum\nBenchmarkSum-8   \t1000\t  12 ns/op\n--- BENCH: BenchmarkSum\nPASS\n");
        assert_eq!(
            finished(&events),
            [(name("BenchmarkSum"), TestStatus::Passed, None), (None, TestStatus::Passed, None)]
        );
    }
}
