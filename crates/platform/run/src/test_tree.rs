use std::ops::Range;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a test, a package or the whole run went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TestStatus {
    /// It has started and not finished.
    Running,
    /// It passed.
    Passed,
    /// It failed.
    Failed,
    /// It was skipped.
    Skipped,
    /// The run ended before it finished.
    Terminated,
}

/// What a node of the tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TestNodeKind {
    /// The whole run.
    Root,
    /// A package (Go) or another group of tests.
    Package,
    /// A test or a subtest.
    Test,
}

/// What a parser saw happen to a package or a test; `test` is absent for the package itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum TestEvent {
    /// It started.
    Started {
        /// The package.
        /// The package.
        package: String,
        /// The test's full name, subtests after `/`.
        test: Option<String>,
    },
    /// The console line parsed with this event belongs to it.
    Output {
        package: String,
        /// The test's full name.
        test: Option<String>,
    },
    /// A benchmark reported its results.
    Measured {
        /// The package.
        package: String,
        /// The benchmark's full name.
        test: String,
        /// How many times it ran.
        iterations: u64,
        /// What it measured.
        measures: Vec<Measure>,
    },
    /// It finished.
    Finished {
        /// The package.
        package: String,
        /// The test's full name.
        test: Option<String>,
        /// How it went.
        status: TestStatus,
        /// How long it took, in milliseconds, when known.
        duration_ms: Option<u64>,
    },
}

/// One number a benchmark reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    /// What it counts, e.g. `ns/op`, `B/op` or `allocs/op`.
    pub unit: String,
    /// The number as the tool printed it.
    pub value: String,
}

/// A test by its package and full name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestId {
    /// The package.
    pub package: String,
    /// The full name, subtests after `/`.
    pub test: String,
}

/// Where a test is written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestSource {
    /// The file.
    pub path: PathBuf,
    /// The line, from 1.
    pub line: u32,
}

/// A node of the test tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestNode {
    /// The name shown: the package, or the last part of the test's name.
    pub name: String,
    /// The run, a package or a test.
    pub kind: TestNodeKind,
    /// The package it is in; empty for the run.
    pub package: String,
    /// The test's full name; absent for the run and packages.
    pub test: Option<String>,
    /// How it went.
    pub status: TestStatus,
    /// How long it took, in milliseconds, when known.
    pub duration_ms: Option<u64>,
    /// How many times a benchmark ran; absent for anything else.
    #[serde(default)]
    pub iterations: Option<u64>,
    /// What a benchmark measured, in the order the tool printed it.
    #[serde(default)]
    pub measures: Vec<Measure>,
    /// The console lines that belong to it, as ranges of line indexes.
    pub lines: Vec<Range<usize>>,
    /// Its packages, tests or subtests, in the order they started.
    pub children: Vec<TestNode>,
}

impl TestNode {
    fn new(name: &str, kind: TestNodeKind, package: &str, test: Option<String>) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            package: package.to_owned(),
            test,
            status: TestStatus::Running,
            duration_ms: None,
            iterations: None,
            measures: Vec::new(),
            lines: Vec::new(),
            children: Vec::new(),
        }
    }

    fn child(&mut self, name: &str, kind: TestNodeKind, package: &str, test: Option<String>) -> &mut TestNode {
        let index = match self.children.iter().position(|child| child.name == name && child.kind == kind) {
            Some(index) => index,
            None => {
                self.children.push(TestNode::new(name, kind, package, test));
                self.children.len() - 1
            }
        };
        &mut self.children[index]
    }

    fn add_line(&mut self, line: usize) {
        match self.lines.last_mut() {
            Some(last) if last.end == line => last.end += 1,
            Some(last) if last.contains(&line) => {}
            _ => self.lines.push(line..line + 1),
        }
    }

    fn terminate(&mut self) {
        if self.status == TestStatus::Running {
            self.status = TestStatus::Terminated;
        }
        self.children.iter_mut().for_each(TestNode::terminate);
    }

    fn visit<'a>(&'a self, out: &mut Vec<&'a TestNode>) {
        out.push(self);
        self.children.iter().for_each(|child| child.visit(out));
    }
}

/// How many tests ended each way; packages and the run are not counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCounts {
    /// Every test and subtest.
    pub total: usize,
    /// Still running.
    pub running: usize,
    /// Passed.
    pub passed: usize,
    /// Failed.
    pub failed: usize,
    /// Skipped.
    pub skipped: usize,
    /// Ended by the end of the run.
    pub terminated: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTree {
    root: TestNode,
    done: bool,
}

impl Default for TestTree {
    fn default() -> Self {
        Self { root: TestNode::new("", TestNodeKind::Root, "", None), done: false }
    }
}

impl TestTree {
    pub fn restored(root: TestNode) -> Self {
        Self { root, done: true }
    }

    pub fn root(&self) -> &TestNode {
        &self.root
    }

    pub fn is_empty(&self) -> bool {
        self.root.children.is_empty()
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn apply(&mut self, event: &TestEvent, line: Option<usize>) {
        match event {
            TestEvent::Started { package, test } => {
                self.node(package, test.as_deref());
            }
            TestEvent::Measured { package, test, iterations, measures } => {
                let node = self.node(package, Some(test));
                node.iterations = Some(*iterations);
                node.measures.clone_from(measures);
            }
            TestEvent::Output { package, test } => {
                if let Some(line) = line {
                    self.root.add_line(line);
                    let package_node = self.root.child(package, TestNodeKind::Package, package, None);
                    package_node.add_line(line);
                    if let Some(test) = test {
                        let mut node = package_node;
                        let mut full = String::new();
                        for part in test.split('/') {
                            full = if full.is_empty() { part.to_owned() } else { format!("{full}/{part}") };
                            node = node.child(part, TestNodeKind::Test, package, Some(full.clone()));
                            node.add_line(line);
                        }
                    }
                }
            }
            TestEvent::Finished { package, test, status, duration_ms } => {
                let node = self.node(package, test.as_deref());
                node.status = *status;
                node.duration_ms = *duration_ms;
            }
        }
        self.update_root();
    }

    pub fn finish(&mut self) {
        self.root.children.iter_mut().for_each(TestNode::terminate);
        self.done = true;
        self.update_root();
    }

    pub fn counts(&self) -> TestCounts {
        let mut nodes = Vec::new();
        self.root.visit(&mut nodes);
        let mut counts = TestCounts::default();
        for node in nodes.into_iter().filter(|node| node.kind == TestNodeKind::Test) {
            counts.total += 1;
            match node.status {
                TestStatus::Running => counts.running += 1,
                TestStatus::Passed => counts.passed += 1,
                TestStatus::Failed => counts.failed += 1,
                TestStatus::Skipped => counts.skipped += 1,
                TestStatus::Terminated => counts.terminated += 1,
            }
        }
        counts
    }

    pub fn failed(&self) -> Vec<TestId> {
        let mut nodes = Vec::new();
        self.root.visit(&mut nodes);
        nodes
            .into_iter()
            .filter(|node| node.kind == TestNodeKind::Test && node.status == TestStatus::Failed)
            .filter_map(|node| node.test.clone().map(|test| TestId { package: node.package.clone(), test }))
            .collect()
    }

    fn node(&mut self, package: &str, test: Option<&str>) -> &mut TestNode {
        let mut node = self.root.child(package, TestNodeKind::Package, package, None);
        if let Some(test) = test {
            let mut full = String::new();
            for part in test.split('/') {
                full = if full.is_empty() { part.to_owned() } else { format!("{full}/{part}") };
                node = node.child(part, TestNodeKind::Test, package, Some(full.clone()));
            }
        }
        node
    }

    fn update_root(&mut self) {
        let statuses: Vec<TestStatus> = self.root.children.iter().map(|child| child.status).collect();
        let has = |wanted: TestStatus| statuses.contains(&wanted);
        self.root.status = if has(TestStatus::Failed) {
            TestStatus::Failed
        } else if has(TestStatus::Running) || (!self.done && statuses.is_empty()) {
            TestStatus::Running
        } else if has(TestStatus::Terminated) {
            TestStatus::Terminated
        } else if !statuses.is_empty() && statuses.iter().all(|status| *status == TestStatus::Skipped) {
            TestStatus::Skipped
        } else {
            TestStatus::Passed
        };
        let durations: Vec<u64> = self.root.children.iter().filter_map(|child| child.duration_ms).collect();
        self.root.duration_ms = (!durations.is_empty()).then(|| durations.iter().sum());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(package: &str, test: Option<&str>) -> TestEvent {
        TestEvent::Started { package: package.to_owned(), test: test.map(str::to_owned) }
    }

    fn output(package: &str, test: Option<&str>) -> TestEvent {
        TestEvent::Output { package: package.to_owned(), test: test.map(str::to_owned) }
    }

    fn finished(package: &str, test: Option<&str>, status: TestStatus, duration_ms: u64) -> TestEvent {
        TestEvent::Finished {
            package: package.to_owned(),
            test: test.map(str::to_owned),
            status,
            duration_ms: Some(duration_ms),
        }
    }

    #[test]
    fn a_tree_grows_from_events_with_subtests() {
        let mut tree = TestTree::default();
        assert_eq!(tree.root().status, TestStatus::Running);
        let events = [
            (started("app", Some("TestA")), None),
            (output("app", Some("TestA")), Some(0)),
            (started("app", Some("TestA/case_1")), None),
            (output("app", Some("TestA/case_1")), Some(1)),
            (output("app", Some("TestA/case_1")), Some(2)),
            (finished("app", Some("TestA/case_1"), TestStatus::Failed, 3), None),
            (started("app", Some("TestA/case_2")), None),
            (finished("app", Some("TestA/case_2"), TestStatus::Passed, 1), None),
            (finished("app", Some("TestA"), TestStatus::Failed, 5), None),
            (started("app", Some("TestB")), None),
            (finished("app", Some("TestB"), TestStatus::Skipped, 0), None),
            (output("app", None), Some(3)),
            (finished("app", None, TestStatus::Failed, 12), None),
        ];
        for (event, line) in &events {
            tree.apply(event, *line);
        }
        let root = tree.root();
        assert_eq!(root.status, TestStatus::Failed);
        assert_eq!(root.duration_ms, Some(12));
        let package = &root.children[0];
        assert_eq!(
            (package.name.as_str(), package.kind, package.status),
            ("app", TestNodeKind::Package, TestStatus::Failed)
        );
        assert_eq!(package.lines, vec![0..4]);
        let test_a = &package.children[0];
        assert_eq!(test_a.lines, vec![0..3]);
        assert_eq!(test_a.children[0].name, "case_1");
        assert_eq!(test_a.children[0].test.as_deref(), Some("TestA/case_1"));
        assert_eq!(test_a.children[0].lines, vec![1..3]);
        assert_eq!(test_a.children[1].status, TestStatus::Passed);
        assert_eq!(package.children[1].status, TestStatus::Skipped);
        let counts = tree.counts();
        assert_eq!(counts, TestCounts { total: 4, running: 0, passed: 1, failed: 2, skipped: 1, terminated: 0 });
        let failed: Vec<String> = tree.failed().into_iter().map(|id| id.test).collect();
        assert_eq!(failed, ["TestA", "TestA/case_1"]);
    }

    #[test]
    fn finishing_terminates_what_still_runs() {
        let mut tree = TestTree::default();
        tree.apply(&started("a", Some("TestSlow/inner")), None);
        tree.apply(&started("b", Some("TestOk")), None);
        tree.apply(&finished("b", Some("TestOk"), TestStatus::Passed, 2), None);
        tree.apply(&finished("b", None, TestStatus::Passed, 2), None);
        assert_eq!(tree.root().status, TestStatus::Running);
        assert!(!tree.is_done());
        tree.finish();
        assert!(tree.is_done());
        assert_eq!(tree.root().status, TestStatus::Terminated);
        assert_eq!(tree.root().children[0].children[0].children[0].status, TestStatus::Terminated);
        assert_eq!(tree.counts().terminated, 2);
        assert_eq!(tree.counts().passed, 1);
    }

    #[test]
    fn lines_out_of_order_and_empty_runs() {
        let mut tree = TestTree::default();
        tree.apply(&output("p", Some("T")), Some(5));
        tree.apply(&output("p", Some("T")), Some(5));
        tree.apply(&output("p", Some("T")), Some(9));
        assert_eq!(tree.root().children[0].children[0].lines, vec![5..6, 9..10]);
        let mut empty = TestTree::default();
        assert!(empty.is_empty());
        empty.finish();
        assert_eq!(empty.root().status, TestStatus::Passed);
        let mut skipped = TestTree::default();
        skipped.apply(&finished("p", None, TestStatus::Skipped, 0), None);
        assert_eq!(skipped.root().status, TestStatus::Skipped);
    }
}
