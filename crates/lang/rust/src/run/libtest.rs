use std::path::{Path, PathBuf};

use zen_run::{OutputParser, ParsedLine, Stream, TestEvent, TestStatus};

pub fn binary_label(krate: &str, source: &str) -> String {
    if source == "src/lib.rs" { krate.to_owned() } else { format!("{krate} ({source})") }
}

pub struct LibtestParser {
    cwd: PathBuf,
    named: std::collections::VecDeque<String>,
    package: Option<String>,
    unnamed: bool,
    held: std::collections::VecDeque<Vec<TestEvent>>,
    failure: Option<String>,
}

impl LibtestParser {
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_owned(),
            named: std::collections::VecDeque::new(),
            package: None,
            unnamed: false,
            held: std::collections::VecDeque::new(),
            failure: None,
        }
    }

    fn emit(&mut self, parsed: &mut ParsedLine, events: Vec<TestEvent>) {
        match self.held.back_mut() {
            Some(held) if self.unnamed => held.extend(events),
            _ => parsed.events.extend(events),
        }
    }

    fn console(&self, stream: Stream, text: &str) -> ParsedLine {
        let mut parsed = ParsedLine::console(stream, text);
        if let Some(console) = &mut parsed.console {
            for link in &mut console.links {
                if !link.file.is_absolute() {
                    link.file = self.cwd.join(&link.file);
                }
            }
        }
        parsed
    }
}

fn package_of(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(krate) = line.strip_prefix("Doc-tests ") {
        return Some(format!("{} (doc-tests)", krate.trim()));
    }
    let rest = line.strip_prefix("Running ")?;
    let rest = rest.strip_prefix("unittests ").unwrap_or(rest);
    let (source, binary) = rest.rsplit_once(" (")?;
    let binary = binary.strip_suffix(')')?;
    let file = binary.rsplit(['/', '\\']).next()?;
    let file = file.strip_suffix(".exe").unwrap_or(file);
    let krate = file.rsplit_once('-').map_or(file, |(name, _)| name);
    Some(binary_label(krate, source.trim()))
}

pub const NAME_SLASH: char = '\u{2215}';

pub fn tree_name(name: &str) -> String {
    name.replace('/', &NAME_SLASH.to_string())
}

pub fn harness_name(name: &str) -> String {
    name.replace(NAME_SLASH, "/")
}

fn result_of(line: &str) -> Option<(String, TestStatus)> {
    let rest = line.strip_prefix("test ")?;
    let (name, outcome) = rest.split_once(" ... ")?;
    let outcome = outcome.trim();
    let status = if outcome == "ok" || outcome.starts_with("bench:") {
        TestStatus::Passed
    } else if outcome == "FAILED" {
        TestStatus::Failed
    } else if outcome.starts_with("ignored") {
        TestStatus::Skipped
    } else {
        return None;
    };
    Some((tree_name(name.trim()), status))
}

fn duration_of(line: &str) -> Option<u64> {
    let (_, rest) = line.split_once("finished in ")?;
    let seconds: f64 = rest.trim().strip_suffix('s')?.parse().ok()?;
    Some((seconds * 1000.0).round() as u64)
}

impl OutputParser for LibtestParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut parsed = self.console(stream, text);
        let line = zen_run::strip_ansi(text);
        let trimmed = line.trim();
        if let Some(package) = package_of(&line) {
            parsed.events.push(TestEvent::Started { package: package.clone(), test: None });
            if let Some(held) = self.held.pop_front() {
                for mut event in held {
                    match &mut event {
                        TestEvent::Started { package: held, .. }
                        | TestEvent::Output { package: held, .. }
                        | TestEvent::Measured { package: held, .. }
                        | TestEvent::Finished { package: held, .. } => held.clone_from(&package),
                    }
                    parsed.events.push(event);
                }
                if self.held.is_empty() && self.unnamed {
                    self.unnamed = false;
                    self.package = Some(package);
                }
            } else {
                self.named.push_back(package);
            }
            return parsed;
        }
        if trimmed.starts_with("running ") && (trimmed.ends_with(" tests") || trimmed.ends_with(" test")) {
            self.failure = None;
            match self.named.pop_front() {
                Some(package) => self.package = Some(package),
                None => {
                    self.package = Some(String::new());
                    self.unnamed = true;
                    self.held.push_back(Vec::new());
                }
            }
            return parsed;
        }
        let Some(package) = self.package.clone() else { return parsed };
        if let Some((test, status)) = result_of(trimmed) {
            let mut events = vec![TestEvent::Started { package: package.clone(), test: Some(test.clone()) }];
            if !self.unnamed {
                events.push(TestEvent::Output { package: package.clone(), test: Some(test.clone()) });
            }
            events.push(TestEvent::Finished { package, test: Some(test), status, duration_ms: None });
            self.emit(&mut parsed, events);
            return parsed;
        }
        if let Some(name) = trimmed.strip_prefix("---- ").and_then(|rest| rest.strip_suffix(" stdout ----")) {
            self.failure = Some(tree_name(name));
        } else if trimmed == "failures:" {
            self.failure = None;
        } else if let Some(result) = trimmed.strip_prefix("test result: ") {
            self.failure = None;
            let status = if result.starts_with("ok") { TestStatus::Passed } else { TestStatus::Failed };
            let mut events = Vec::new();
            if !self.unnamed {
                events.push(TestEvent::Output { package: package.clone(), test: None });
            }
            events.push(TestEvent::Finished { package, test: None, status, duration_ms: duration_of(trimmed) });
            self.emit(&mut parsed, events);
            self.package = None;
            self.unnamed = false;
            return parsed;
        }
        if !self.unnamed {
            parsed.events.push(TestEvent::Output { package, test: self.failure.clone() });
        }
        parsed
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        self.package = None;
        self.failure = None;
        self.unnamed = false;
        self.held.clear();
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use zen_run::{TestNodeKind, TestTree};

    use super::*;

    const OUTPUT: &str = "   Compiling calc v0.1.0 (/w/calc)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.29s
     Running unittests src/main.rs (target/debug/deps/app-88df655c7232644e)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/calc-edb46006486a1d4c)

running 3 tests
test tests::slow ... ignored, needs a database
test tests::adds ... ok
test tests::multiplies_wrongly ... FAILED

failures:

---- tests::multiplies_wrongly stdout ----

thread 'tests::multiplies_wrongly' (28167374) panicked at calc/src/lib.rs:24:9:
assertion `left == right` failed: two times three
  left: 6
 right: 5
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::multiplies_wrongly

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.25s

     Running tests/outside.rs (target/debug/deps/outside-0123456789abcdef)

running 1 test
test adds_from_outside ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests calc

running 1 test
test calc/src/lib.rs - add (line 5) ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
";

    fn tree(output: &str) -> (TestTree, Vec<zen_run::ConsoleLine>) {
        let mut parser = LibtestParser::new(Path::new("/w"));
        let mut tree = TestTree::default();
        let mut console = Vec::new();
        for text in output.lines() {
            let stream = if text.trim_start().starts_with("Running") || text.contains("Compiling") {
                Stream::Stderr
            } else {
                Stream::Stdout
            };
            let parsed = parser.line(stream, text);
            let index = parsed.console.as_ref().map(|line| {
                console.push(line.clone());
                console.len() - 1
            });
            for event in &parsed.events {
                tree.apply(event, index);
            }
        }
        for event in parser.finish() {
            tree.apply(&event, None);
        }
        tree.finish();
        (tree, console)
    }

    #[test]
    fn every_test_binary_is_a_package_with_its_tests() {
        let (tree, console) = tree(OUTPUT);
        let packages: Vec<(&str, TestStatus, Option<u64>)> = tree
            .root()
            .children
            .iter()
            .map(|package| (package.name.as_str(), package.status, package.duration_ms))
            .collect();
        assert_eq!(
            packages,
            [
                ("app (src/main.rs)", TestStatus::Passed, Some(0)),
                ("calc", TestStatus::Failed, Some(250)),
                ("outside (tests/outside.rs)", TestStatus::Passed, Some(0)),
                ("calc (doc-tests)", TestStatus::Passed, Some(310)),
            ]
        );
        let calc = &tree.root().children[1];
        let tests: Vec<(&str, TestStatus)> =
            calc.children.iter().map(|test| (test.name.as_str(), test.status)).collect();
        assert_eq!(
            tests,
            [
                ("tests::slow", TestStatus::Skipped),
                ("tests::adds", TestStatus::Passed),
                ("tests::multiplies_wrongly", TestStatus::Failed)
            ]
        );
        assert!(calc.children.iter().all(|test| test.kind == TestNodeKind::Test));
        let counts = tree.counts();
        assert_eq!((counts.total, counts.passed, counts.failed, counts.skipped), (5, 3, 1, 1));
        assert_eq!(tree.root().status, TestStatus::Failed);
        assert_eq!(tree.failed()[0].test, "tests::multiplies_wrongly");
        let doc = &tree.root().children[3].children[0];
        assert_eq!(harness_name(&doc.name), "calc/src/lib.rs - add (line 5)", "one test, not a path of subtests");

        let failed = &calc.children[2];
        let lines: Vec<&str> =
            failed.lines.iter().flat_map(|range| range.clone()).map(|index| console[index].text.as_str()).collect();
        assert!(lines.iter().any(|line| line.contains("panicked at calc/src/lib.rs:24:9")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("left: 6")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("failures:")), "{lines:?}");
        let panic = lines.iter().position(|line| line.contains("panicked")).unwrap();
        let index = failed.lines.iter().flat_map(|range| range.clone()).nth(panic).unwrap();
        let link = &console[index].links[0];
        assert_eq!((link.file.as_path(), link.line, link.column), (Path::new("/w/calc/src/lib.rs"), 24, Some(9)));
    }

    #[test]
    fn names_and_tests_from_the_two_streams_pair_up_in_any_order() {
        let names_first = "     Running tests/outside.rs (target/debug/deps/outside-0123)\n   Doc-tests calc\n\nrunning 1 test\ntest adds_from_outside ... ok\n\ntest result: ok. 1 passed; finished in 0.00s\n\nrunning 1 test\ntest calc/src/lib.rs - add (line 5) ... ok\n\ntest result: ok. 1 passed; finished in 0.31s\n";
        let tests_first = "\nrunning 1 test\ntest adds_from_outside ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; finished in 0.00s\n     Running tests/outside.rs (target/debug/deps/outside-0123)\n   Doc-tests calc\n\nrunning 1 test\ntest calc/src/lib.rs - add (line 5) ... ok\n\ntest result: ok. 1 passed; finished in 0.31s\n";
        for (output, outcome) in [(names_first, TestStatus::Passed), (tests_first, TestStatus::Failed)] {
            let (tree, _) = tree(output);
            type Package<'a> = (&'a str, Vec<(String, TestStatus)>, TestStatus);
            let packages: Vec<Package<'_>> = tree
                .root()
                .children
                .iter()
                .map(|package| {
                    let tests = package.children.iter().map(|test| (harness_name(&test.name), test.status)).collect();
                    (package.name.as_str(), tests, package.status)
                })
                .collect();
            assert_eq!(
                packages,
                [
                    ("outside (tests/outside.rs)", vec![("adds_from_outside".to_owned(), outcome)], outcome),
                    (
                        "calc (doc-tests)",
                        vec![("calc/src/lib.rs - add (line 5)".to_owned(), TestStatus::Passed)],
                        TestStatus::Passed
                    ),
                ],
                "{output}"
            );
        }
    }

    #[test]
    fn two_binaries_ahead_of_both_names_keep_their_own_tests() {
        let output = "\nrunning 1 test\ntest first ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; finished in 0.00s\n\nrunning 1 test\ntest second ... ok\n\ntest result: ok. 1 passed; finished in 0.00s\n     Running tests/one.rs (target/debug/deps/one-0123)\n     Running tests/two.rs (target/debug/deps/two-0123)\n\nrunning 1 test\ntest third ... ok\n\ntest result: ok. 1 passed; finished in 0.00s\n     Running tests/three.rs (target/debug/deps/three-0123)\n";
        let (tree, _) = tree(output);
        let packages: Vec<(&str, Vec<&str>, TestStatus)> = tree
            .root()
            .children
            .iter()
            .map(|package| {
                (
                    package.name.as_str(),
                    package.children.iter().map(|test| test.name.as_str()).collect(),
                    package.status,
                )
            })
            .collect();
        assert_eq!(
            packages,
            [
                ("one (tests/one.rs)", vec!["first"], TestStatus::Failed),
                ("two (tests/two.rs)", vec!["second"], TestStatus::Passed),
                ("three (tests/three.rs)", vec!["third"], TestStatus::Passed),
            ]
        );
    }

    #[test]
    fn labels_follow_the_binaries() {
        assert_eq!(
            package_of("     Running unittests src/lib.rs (target/debug/deps/zen_core-5a6037c5dd85e27f)").as_deref(),
            Some("zen_core")
        );
        assert_eq!(
            package_of("Running unittests src/bin/tool.rs (target/debug/deps/tool-0a)").as_deref(),
            Some("tool (src/bin/tool.rs)")
        );
        assert_eq!(
            package_of("Running benches/speed.rs (target/release/deps/speed-0b)").as_deref(),
            Some("speed (benches/speed.rs)")
        );
        assert_eq!(package_of("Running `target/debug/app`"), None);
        assert_eq!(
            result_of("test b ... bench:       1,234 ns/iter (+/- 56)"),
            Some(("b".to_owned(), TestStatus::Passed))
        );
        assert_eq!(result_of("test a has been running for over 60 seconds"), None);
        assert_eq!(duration_of("test result: ok. 1 passed; finished in 1.5s"), Some(1500));
    }

    #[test]
    fn a_build_error_runs_no_tests_and_links_its_place() {
        let (tree, console) = tree(
            "   Compiling app v0.1.0 (/w/app)\nerror[E0308]: mismatched types\n --> app/src/main.rs:3:23\n  |\nerror: could not compile `app` (bin \"app\" test) due to 1 previous error\n",
        );
        assert!(tree.is_empty());
        let link = console.iter().find_map(|line| line.links.first()).unwrap();
        assert_eq!((link.file.as_path(), link.line), (Path::new("/w/app/src/main.rs"), 3));
    }
}
