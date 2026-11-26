use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::test_support::{NewTest, TestFramework, TestMember, TestSupport, TestSupports};

use super::goto::{LookupState, TargetKind, none_found, offer};
use super::places::Place;
use super::{LookupParams, file_text};
use crate::Workbench;
use crate::history::{NavigateTo, NavigateToParams};

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn support_for(session: &Session, path: &Path) -> Result<Arc<dyn TestSupport>, CommandError> {
    session
        .state::<TestSupports>()
        .and_then(|supports| supports.for_file(path))
        .ok_or_else(|| CommandError::new(t!("workbench-tests-unknown-language", file = display(path))))
}

fn root_of(session: &Session, path: &Path) -> PathBuf {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .or_else(|| path.parent().map(Path::to_owned))
        .unwrap_or_default()
}

fn text_of(session: &mut Session, path: &Path) -> Option<String> {
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        if crate::hidden_files::is_hidden(&patterns, root.as_deref(), path) {
            return None;
        }
    }
    file_text(session.state_mut::<Workbench>(), path).ok()
}

fn line_at(text: &str, offset: usize) -> usize {
    text.chars().take(offset).filter(|c| *c == '\n').count()
}

fn innermost(members: &[TestMember], line: usize) -> Option<&TestMember> {
    members.iter().rfind(|member| member.holds(line))
}

pub(crate) fn place_of(path: &Path, text: &str, line: usize, name: &str) -> Place {
    let line_start =
        if line == 0 { 0 } else { text.match_indices('\n').nth(line - 1).map_or(text.len(), |(at, _)| at + 1) };
    let line_end = text[line_start..].find('\n').map_or(text.len(), |end| line_start + end);
    let line_text = &text[line_start..line_end];
    let lead = line_text.len() - line_text.trim_start().len();
    let name_at = if name.is_empty() { None } else { line_text.find(name) };
    let start = line_start + name_at.unwrap_or(lead);
    let end = name_at.map_or(start, |at| line_start + at + name.len());
    let preview = line_text.trim();
    let from = text[line_start + lead..start.max(line_start + lead)].chars().count();
    let to = from + text[start..end].chars().count();
    let chars = |byte: usize| text[..byte].chars().count();
    Place {
        path: path.to_owned(),
        start: chars(start),
        end: chars(end),
        line: line as u32 + 1,
        column: text[line_start..start].chars().count() as u32 + 1,
        preview: Some(preview.to_owned()),
        highlight: Some([from, to]),
    }
}

struct Found {
    kind: TargetKind,
    symbol: Option<String>,
    targets: Vec<Place>,
}

fn existing(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    for file in files {
        if file.is_file() && !seen.contains(&file) {
            seen.push(file);
        }
    }
    seen
}

fn add(targets: &mut Vec<Place>, place: Place) {
    if !targets.iter().any(|known| known.path == place.path && known.line == place.line) {
        targets.push(place);
    }
}

fn find(session: &mut Session, path: &Path, line: usize) -> Result<Found, CommandError> {
    let support = support_for(session, path)?;
    let root = root_of(session, path);
    let text =
        text_of(session, path).ok_or_else(|| CommandError::new(t!("workbench-nav-no-file", path = display(path))))?;
    let members = support.members(path, &text);
    let at = innermost(&members, line).cloned();
    let test_file = support.is_test_file(path);
    let in_test = test_file || at.as_ref().is_some_and(|member| member.test);
    let mut targets = Vec::new();
    if in_test {
        let mut files = support.subject_files(&root, path);
        if !test_file {
            files.insert(0, path.to_owned());
        }
        let paired = existing(files.clone());
        files.extend(support.searched_files(path, false));
        for file in existing(files) {
            let Some(other) = text_of(session, &file) else { continue };
            let subjects: Vec<TestMember> =
                support.members(&file, &other).into_iter().filter(|member| !member.test).collect();
            if let Some(test) = &at {
                for subject in subjects.iter().filter(|subject| support.tests(subject, test)) {
                    add(&mut targets, place_of(&file, &other, subject.line, &subject.name));
                }
            }
        }
        if targets.is_empty() {
            for file in paired.into_iter().filter(|file| file != path || at.is_some()) {
                let Some(other) = text_of(session, &file) else { continue };
                let first = support.members(&file, &other).into_iter().find(|member| !member.test);
                let (line, name) = first.map_or((0, String::new()), |member| (member.line, member.name));
                add(&mut targets, place_of(&file, &other, line, &name));
            }
        }
        return Ok(Found { kind: TargetKind::TestSubject, symbol: at.map(|member| member.name), targets });
    }
    let files = support.test_files(&root, path);
    let paired = existing(files.clone());
    let mut searched = files;
    searched.extend(support.searched_files(path, true));
    for file in existing(searched) {
        let Some(other) = text_of(session, &file) else { continue };
        let tests: Vec<TestMember> = support.members(&file, &other).into_iter().filter(|member| member.test).collect();
        if let Some(subject) = &at {
            for test in tests.iter().filter(|test| support.tests(subject, test)) {
                add(&mut targets, place_of(&file, &other, test.line, &test.name));
            }
        }
    }
    if targets.is_empty() {
        for file in paired {
            let Some(other) = text_of(session, &file) else { continue };
            let first = support.members(&file, &other).into_iter().find(|member| member.test);
            match first {
                Some(member) => add(&mut targets, place_of(&file, &other, member.line, &member.name)),
                None if file != path => add(&mut targets, place_of(&file, &other, 0, "")),
                None => {}
            }
        }
    }
    let symbol =
        at.map(|member| member.name).or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()));
    Ok(Found { kind: TargetKind::Test, symbol, targets })
}

fn place_params(session: &mut Session, params: &LookupParams) -> Result<(PathBuf, usize), CommandError> {
    let (path, offset) = params.place(session.state_mut::<Workbench>())?;
    let text = text_of(session, &path).unwrap_or_default();
    Ok((path, line_at(&text, offset)))
}

pub struct GoToTest;

impl Command for GoToTest {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "navigate.test.goTo";
    const TITLE: &'static str = "Go to Test";
    const DESCRIPTION: &'static str = "From code to its tests and back (Navigate → Test, ⇧⌘T), by the naming rules of the file's \
         language (a built-in module's or a plug-in's `tests`): from a function, method or type at a place (the caret by \
         default) to the tests named after it, else to its test file; from a test to the code it tests, else to the subject \
         file. One target: goes there; several: lists them to choose from (the editor shows a chooser, navigate.lookup.poll \
         reads it); none from code: `createTest` is true and Create New Test… is offered (navigate.test.options, \
         navigate.test.create).";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        let (path, line) = place_params(session, &params)?;
        let found = find(session, &path, line)?;
        let mut state = LookupState { kind: found.kind, symbol: found.symbol, ..LookupState::default() };
        match found.targets.len() {
            0 if found.kind == TargetKind::Test => state.create_test = true,
            0 => state.error = Some(none_found(found.kind)),
            1 => {
                let target = &found.targets[0];
                NavigateTo
                    .run(session, NavigateToParams { path: target.path.clone(), offset: target.start, end: None })?;
                state.navigated = true;
            }
            _ => {}
        }
        state.targets = found.targets;
        offer(session, state.clone());
        Ok(state)
    }
}

/// A member of the subject file the Create Test dialog can write a test for.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OfferedMember {
    #[serde(flatten)]
    pub member: TestMember,
    /// The name shown and given to `navigate.test.create`: `Type.method` for a method.
    pub label: String,
    /// Whether the caret is in it, so it is checked first.
    pub selected: bool,
}

/// What the Create Test dialog offers for a subject file.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestOptions {
    /// The file the test is for.
    pub subject: PathBuf,
    /// The frameworks the project can use, the usual one first.
    pub frameworks: Vec<TestFramework>,
    /// The test's name to start with: its file's name without the extension (Go, Rust) or its class (JVM languages).
    pub name: String,
    /// The folders the test can go to, the usual one first.
    pub destinations: Vec<PathBuf>,
    /// The functions, methods and types tests can be written for.
    pub members: Vec<OfferedMember>,
}

fn options(session: &mut Session, path: &Path, line: Option<usize>) -> Result<TestOptions, CommandError> {
    let support = support_for(session, path)?;
    let root = root_of(session, path);
    let text =
        text_of(session, path).ok_or_else(|| CommandError::new(t!("workbench-nav-no-file", path = display(path))))?;
    let members: Vec<TestMember> = support.members(path, &text).into_iter().filter(|member| !member.test).collect();
    let at = line.and_then(|line| innermost(&members, line)).map(|member| (member.line, member.name.clone()));
    let defaults = support.defaults(&root, path);
    Ok(TestOptions {
        subject: path.to_owned(),
        frameworks: support.frameworks(&root, path),
        name: defaults.name,
        destinations: defaults.destinations,
        members: members
            .into_iter()
            .map(|member| OfferedMember {
                label: member.label(),
                selected: at.as_ref().is_some_and(|(line, name)| member.line == *line && member.name == *name),
                member,
            })
            .collect(),
    })
}

/// The subject of a new test: a file and, optionally, the place in it whose member is checked first.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsParams {
    /// The subject file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The line whose member is checked first, one-based; the caret's when absent and the file is the active one.
    #[serde(default)]
    pub line: Option<u32>,
}

fn subject_place(
    session: &mut Session,
    path: Option<PathBuf>,
    line: Option<u32>,
) -> Result<(PathBuf, Option<usize>), CommandError> {
    let workbench = session.state::<Workbench>();
    let active = workbench.and_then(Workbench::active_document);
    let path = match path {
        Some(path) if path.is_relative() => root_of(session, &path).join(path),
        Some(path) => path,
        None => active
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    let caret = active
        .filter(|document| document.path() == path)
        .map(|document| line_at(document.text(), document.selections().primary().head));
    Ok((path, line.map(|line| line.saturating_sub(1) as usize).or(caret)))
}

pub struct TestOptionsCommand;

impl Command for TestOptionsCommand {
    type Params = OptionsParams;
    type Output = TestOptions;
    const ID: &'static str = "navigate.test.options";
    const TITLE: &'static str = "Create Test Options";
    const DESCRIPTION: &'static str = "What the Create Test dialog offers for a subject file (the active one by default): the \
         testing frameworks its project can use, the test's name, the folders it can go to and the members tests can be \
         written for, the one at the line (the caret by default) selected. navigate.test.create writes the test.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<TestOptions, CommandError> {
        let (path, line) = subject_place(session, params.path, params.line)?;
        options(session, &path, line)
    }
}

/// A test to create, as the Create Test dialog fills it in.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateParams {
    /// The subject file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The framework's id from navigate.test.options; its first when absent.
    #[serde(default)]
    pub framework: Option<String>,
    /// The test's name; navigate.test.options's when absent.
    #[serde(default)]
    pub name: Option<String>,
    /// The folder the test goes to, absolute or relative to the project; navigate.test.options's first when absent.
    #[serde(default)]
    pub destination: Option<PathBuf>,
    /// The labels of the members to write test stubs for (`Type.method` for a method), from navigate.test.options.
    #[serde(default)]
    pub members: Vec<String>,
}

/// The test file written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTest {
    pub path: PathBuf,
    /// The line of the first stub written, one-based; the caret is there.
    pub line: u32,
    /// Whether the file is new; otherwise the stubs were added to it as an unsaved change.
    pub created: bool,
}

fn common_edit(old: &str, new: &str) -> (std::ops::Range<usize>, String) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
    let room = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars.iter().rev().zip(new_chars.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    (prefix..old_chars.len() - suffix, new_chars[prefix..new_chars.len() - suffix].iter().collect())
}

fn create(session: &mut Session, params: CreateParams) -> Result<CreatedTest, CommandError> {
    let (path, _) = subject_place(session, params.path, None)?;
    let offered = options(session, &path, None)?;
    let support = support_for(session, &path)?;
    let root = root_of(session, &path);
    let framework = params
        .framework
        .or_else(|| offered.frameworks.first().map(|framework| framework.id.clone()))
        .ok_or_else(|| CommandError::new(t!("workbench-tests-no-framework")))?;
    let name = params.name.map(|name| name.trim().to_owned()).unwrap_or(offered.name);
    if name.is_empty() || name.contains(['/', '\\']) {
        return Err(CommandError::new(t!("workbench-tests-bad-name", name = name)));
    }
    let destination = match params.destination.or_else(|| offered.destinations.first().cloned()) {
        Some(folder) if folder.is_relative() => root.join(folder),
        Some(folder) => folder,
        None => return Err(CommandError::new(t!("workbench-tests-no-destination"))),
    };
    let mut members = Vec::new();
    for label in &params.members {
        let member = offered.members.iter().find(|offered| offered.label == *label).ok_or_else(|| {
            CommandError::new(t!("workbench-tests-unknown-member", member = label.as_str(), file = display(&path)))
        })?;
        members.push(member.member.clone());
    }
    let offered_here = offered.destinations.iter().any(|folder| root.join(folder) == destination);
    let project = session.state::<Workbench>().and_then(Workbench::project).map(crate::hidden_files::real_path);
    let request = NewTest { framework, name, destination, members };
    let test_path = support.test_file(&path, &request);
    let inside = project
        .as_ref()
        .map_or(offered_here, |project| crate::hidden_files::real_path(&test_path).starts_with(project));
    if !inside {
        return Err(CommandError::new(t!("workbench-tests-outside-project", file = display(&test_path))));
    }
    crate::hidden_files::guard_change(session, &root, &[&test_path])?;
    let subject_text = text_of(session, &path).unwrap_or_default();
    let open = session
        .state::<Workbench>()
        .is_some_and(|workbench| workbench.documents().iter().any(|document| document.path() == test_path));
    let current = if open || test_path.is_file() { text_of(session, &test_path) } else { None };
    let made = support.generate(&request, &path, &subject_text, current.as_deref()).map_err(CommandError::new)?;
    let created = current.is_none();
    if created {
        if let Some(folder) = test_path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| {
                CommandError::new(t!(
                    "workbench-tests-cannot-write",
                    file = display(&test_path),
                    error = error.to_string()
                ))
            })?;
        }
        zen_vfs::write_atomic(&test_path, made.text.as_bytes()).map_err(|error| {
            CommandError::new(t!("workbench-tests-cannot-write", file = display(&test_path), error = error.to_string()))
        })?;
    }
    crate::open_document(session, &test_path)?;
    if let Some(current) = &current
        && *current != made.text
    {
        let (range, text) = common_edit(current, &made.text);
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        if let Some(reason) = document.read_only() {
            return Err(CommandError::new(reason.to_owned()));
        }
        document.replace_ranges(vec![(range, text)]);
    }
    let text = text_of(session, &test_path).unwrap_or(made.text);
    let place = place_of(&test_path, &text, made.line, "");
    NavigateTo.run(session, NavigateToParams { path: test_path.clone(), offset: place.start, end: None })?;
    Ok(CreatedTest { path: test_path, line: made.line as u32 + 1, created })
}

pub struct CreateTest;

impl Command for CreateTest {
    type Params = CreateParams;
    type Output = CreatedTest;
    const ID: &'static str = "navigate.test.create";
    const TITLE: &'static str = "Create Test";
    const DESCRIPTION: &'static str = "Writes a test for a subject file (the active one by default), as the Create Test dialog \
         does: with the framework, name and destination folder of navigate.test.options (its first choices when absent) and \
         a test stub for each member named; members that have a test already get none. A new test file is written to disk; \
         an existing one gets the stubs as an unsaved change. Opens the test with the caret on the first stub.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<CreatedTest, CommandError> {
        create(session, params)
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoToTest)?;
    registry.register(TestOptionsCommand)?;
    registry.register(CreateTest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_run::test_support::DeclaredTests;

    use super::*;

    const CART: &str = "package com.acme;\n\nclass Cart {\n    int total() {\n        return 0;\n    }\n\n    void clear() {\n    }\n}\n";
    const CART_TEST: &str = "package com.acme;\n\nclass CartTest {\n    void testTotal() {\n    }\n\n    void testTotalOfNothing() {\n    }\n}\n";

    fn java() -> Arc<dyn TestSupport> {
        let declared = json!([{
            "id": "java",
            "extensions": ["java"],
            "pairs": [{ "subject": "src/main/java/{path}{name}.java", "test": "src/test/java/{path}{name}Test.java" }],
            "members": "^\\s*(?:int|void) (?P<name>\\w+)\\(",
            "tests": ["test{Name}"],
            "packageLine": "package ${PACKAGE};\n\n",
            "insert": "beforeLastBrace",
            "frameworks": [{ "id": "junit5", "name": "JUnit 5", "file": "${PACKAGE_LINE}class ${NAME} {\n${METHODS}}\n", "method": "\n    void ${TEST}() {\n    }\n" }]
        }]);
        Arc::new(DeclaredTests::parse_list(&declared).pop().unwrap())
    }

    fn fixture(files: &[(&str, &str)]) -> (Registry, Session, tempfile::TempDir) {
        fixture_with(files, java())
    }

    fn fixture_with(files: &[(&str, &str)], plugins: Arc<dyn TestSupport>) -> (Registry, Session, tempfile::TempDir) {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session.state_mut::<TestSupports>().set_plugins(Arc::new(move || vec![Arc::clone(&plugins)]));
        (registry, session, dir)
    }

    fn caret(session: &Session) -> (PathBuf, usize) {
        let document = session.state::<Workbench>().and_then(Workbench::active_document).unwrap();
        (document.path().to_owned(), line_at(document.text(), document.selections().primary().head))
    }

    fn go(registry: &Registry, session: &mut Session, path: &Path, line: u32) -> Value {
        registry.dispatch(session, "file.document.open", json!({ "path": path })).unwrap();
        registry.dispatch(session, "navigate.test.goTo", json!({ "path": path, "line": line, "column": 5 })).unwrap()
    }

    const SUBJECT: &str = "src/main/java/com/acme/Cart.java";
    const TEST: &str = "src/test/java/com/acme/CartTest.java";

    #[test]
    fn go_to_test_jumps_from_a_method_to_its_test_and_back() {
        let (registry, mut session, dir) =
            fixture(&[(SUBJECT, CART), (TEST, "class CartTest {\n    void testClear() {\n    }\n}\n")]);
        let (subject, test) = (dir.path().join(SUBJECT), dir.path().join(TEST));
        let state = go(&registry, &mut session, &subject, 8);
        assert_eq!((state["navigated"].as_bool(), state["kind"].as_str()), (Some(true), Some("test")), "{state}");
        assert_eq!(caret(&session), (test.clone(), 1));
        let back = go(&registry, &mut session, &test, 2);
        assert_eq!((back["navigated"].as_bool(), back["kind"].as_str()), (Some(true), Some("testSubject")), "{back}");
        assert_eq!(caret(&session), (subject.clone(), 7));
        let polled = registry.dispatch(&mut session, "navigate.lookup.poll", json!({})).unwrap();
        assert_eq!(polled["navigated"], true, "the editor reads the outcome once");
        assert_eq!(registry.dispatch(&mut session, "navigate.lookup.poll", json!({})).unwrap()["navigated"], false);
    }

    #[test]
    fn several_tests_are_offered_to_choose_and_no_test_offers_create_new_test() {
        let (registry, mut session, dir) = fixture(&[(SUBJECT, CART), (TEST, CART_TEST)]);
        let subject = dir.path().join(SUBJECT);
        let state = go(&registry, &mut session, &subject, 4);
        let lines: Vec<u64> =
            state["targets"].as_array().unwrap().iter().map(|target| target["line"].as_u64().unwrap()).collect();
        assert_eq!((state["navigated"].as_bool(), lines), (Some(false), vec![4, 7]), "{state}");
        assert_eq!(state["symbol"], "total");
        let polled = registry.dispatch(&mut session, "navigate.lookup.poll", json!({})).unwrap();
        assert_eq!(polled["targets"].as_array().map(Vec::len), Some(2), "the chooser's targets");
        let other = dir.path().join("src/main/java/com/acme/Shelf.java");
        std::fs::write(&other, "class Shelf {\n    void put() {\n    }\n}\n").unwrap();
        let none = go(&registry, &mut session, &other, 2);
        assert_eq!(
            (none["createTest"].as_bool(), none["targets"].as_array().map(Vec::len)),
            (Some(true), Some(0)),
            "{none}"
        );
        let orphan = dir.path().join("src/test/java/com/acme/GoneTest.java");
        std::fs::write(&orphan, "class GoneTest {\n}\n").unwrap();
        let lost = go(&registry, &mut session, &orphan, 1);
        assert_eq!(
            (lost["createTest"].as_bool(), lost["error"].as_str()),
            (None, Some(t!("workbench-nav-no-test-subject").as_str()))
        );
        let unknown = dir.path().join("notes.txt");
        std::fs::write(&unknown, "x").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": unknown })).unwrap();
        assert!(registry.dispatch(&mut session, "navigate.test.goTo", json!({})).is_err());
    }

    #[test]
    fn create_test_offers_the_frameworks_name_destination_and_members_and_writes_the_stubs() {
        let (registry, mut session, dir) = fixture(&[(SUBJECT, CART)]);
        let subject = dir.path().join(SUBJECT);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": subject })).unwrap();
        let options = registry.dispatch(&mut session, "navigate.test.options", json!({ "line": 8 })).unwrap();
        assert_eq!(options["frameworks"], json!([{ "id": "junit5", "name": "JUnit 5" }]));
        assert_eq!(options["name"], "CartTest");
        assert_eq!(options["destinations"], json!([dir.path().join("src/test/java/com/acme")]));
        let members: Vec<(&str, bool)> = options["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|member| (member["label"].as_str().unwrap(), member["selected"].as_bool().unwrap()))
            .collect();
        assert_eq!(members, [("total", false), ("clear", true)]);
        let created = registry.dispatch(&mut session, "navigate.test.create", json!({ "members": ["total"] })).unwrap();
        let test = dir.path().join(TEST);
        assert_eq!(
            (created["path"].as_str(), created["line"].as_u64(), created["created"].as_bool()),
            (test.to_str(), Some(5), Some(true))
        );
        assert_eq!(
            std::fs::read_to_string(&test).unwrap(),
            "package com.acme;\n\nclass CartTest {\n\n    void testTotal() {\n    }\n}\n"
        );
        assert_eq!(caret(&session), (test.clone(), 4));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": subject })).unwrap();
        let more =
            registry.dispatch(&mut session, "navigate.test.create", json!({ "members": ["total", "clear"] })).unwrap();
        assert_eq!((more["created"].as_bool(), more["line"].as_u64()), (Some(false), Some(8)));
        let document = session.state::<Workbench>().and_then(Workbench::active_document).unwrap();
        assert!(
            document.text().ends_with("    void testTotal() {\n    }\n\n    void testClear() {\n    }\n}\n"),
            "{}",
            document.text()
        );
        assert!(
            registry
                .dispatch(&mut session, "navigate.test.create", json!({ "path": subject, "members": ["nope"] }))
                .is_err()
        );
        assert!(
            registry.dispatch(&mut session, "navigate.test.create", json!({ "path": subject, "name": "a/b" })).is_err()
        );
    }

    #[test]
    fn create_test_writes_only_inside_the_project_and_never_a_file_hidden_from_agents() {
        let (registry, mut session, dir) = fixture(&[(SUBJECT, CART)]);
        let subject = dir.path().join(SUBJECT);
        let elsewhere = tempfile::tempdir().unwrap();
        for destination in [elsewhere.path().to_path_buf(), PathBuf::from("../outside"), dir.path().join("src/../..")] {
            let refused = registry.dispatch(
                &mut session,
                "navigate.test.create",
                json!({ "path": subject, "destination": destination, "members": ["total"] }),
            );
            assert!(refused.is_err(), "{destination:?}: {refused:?}");
        }
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0, "nothing written outside");
        assert!(!dir.path().parent().unwrap().join("outside").exists());
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["src/test/**".to_owned()]);
        let hidden = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "navigate.test.create", json!({ "path": subject, "members": ["total"] }))
        });
        match hidden {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{error:?}")
            }
            other => panic!("not refused: {other:?}"),
        }
        assert!(!dir.path().join(TEST).exists(), "nothing written where agents may not look");
        let person =
            registry.dispatch(&mut session, "navigate.test.create", json!({ "path": subject, "members": ["total"] }));
        assert!(person.is_ok(), "{person:?}");
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["src/main/**".to_owned()]);
        let read = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "navigate.test.options", json!({ "path": subject }))
        });
        assert!(read.is_err(), "an agent reads no member of a hidden file: {read:?}");
        let test = dir.path().join(TEST);
        let back = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "navigate.test.goTo", json!({ "path": test, "offset": 0 }))
        });
        assert!(!back.map(|state| state.to_string()).unwrap_or_default().contains("Cart.java"), "no hidden target");
    }

    fn plugin(manifest: &str) -> Arc<dyn TestSupport> {
        let manifest: Value = serde_json::from_str(manifest).unwrap();
        Arc::new(DeclaredTests::parse_list(&manifest["tests"]).pop().unwrap())
    }

    const POM: &str = "<project>\n  <dependencies>\n    <dependency>\n      <groupId>org.junit.jupiter</groupId>\n      <artifactId>junit-jupiter</artifactId>\n      <scope>test</scope>\n    </dependency>\n  </dependencies>\n</project>\n";
    const SHOP_CART: &str = "package com.acme;\n\npublic class Cart {\n    public int total() {\n        return sum();\n    }\n\n    public void clear() {\n    }\n}\n";

    #[test]
    fn the_java_plugin_goes_to_a_test_both_ways_and_creates_a_junit_5_test_in_a_maven_module() {
        let java = plugin(include_str!("../../../../../plugins/java/plugin.json"));
        let subject_path = "shop/src/main/java/com/acme/Cart.java";
        let (registry, mut session, dir) = fixture_with(&[("shop/pom.xml", POM), (subject_path, SHOP_CART)], java);
        let subject = dir.path().join(subject_path);
        let none = go(&registry, &mut session, &subject, 4);
        assert_eq!(none["createTest"], true, "{none}");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": subject })).unwrap();
        let options = registry.dispatch(&mut session, "navigate.test.options", json!({ "line": 4 })).unwrap();
        let frameworks: Vec<&str> =
            options["frameworks"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(frameworks, ["junit5", "junit4", "testng"]);
        assert_eq!(options["name"], "CartTest");
        assert_eq!(options["destinations"], json!([dir.path().join("shop/src/test/java/com/acme")]));
        let labels: Vec<&str> =
            options["members"].as_array().unwrap().iter().map(|m| m["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["total", "clear"]);
        let created = registry.dispatch(&mut session, "navigate.test.create", json!({ "members": ["total"] })).unwrap();
        let test = dir.path().join("shop/src/test/java/com/acme/CartTest.java");
        assert_eq!((created["path"].as_str(), created["line"].as_u64()), (test.to_str(), Some(20)));
        assert_eq!(
            std::fs::read_to_string(&test).unwrap(),
            "package com.acme;\n\n\
             import org.junit.jupiter.api.AfterEach;\n\
             import org.junit.jupiter.api.BeforeEach;\n\
             import org.junit.jupiter.api.Test;\n\n\
             import static org.junit.jupiter.api.Assertions.*;\n\n\
             class CartTest {\n\n\
             \x20   @BeforeEach\n\
             \x20   void setUp() {\n\
             \x20   }\n\n\
             \x20   @AfterEach\n\
             \x20   void tearDown() {\n\
             \x20   }\n\n\
             \x20   @Test\n\
             \x20   void total() {\n\
             \x20   }\n\
             }\n"
        );
        let there = go(&registry, &mut session, &subject, 4);
        assert_eq!(there["navigated"], true, "{there}");
        assert_eq!(caret(&session), (test.clone(), 19));
        let back = go(&registry, &mut session, &test, 20);
        assert_eq!((back["navigated"].as_bool(), back["kind"].as_str()), (Some(true), Some("testSubject")), "{back}");
        assert_eq!(caret(&session), (subject.clone(), 3));
        let file = go(&registry, &mut session, &test, 1);
        assert_eq!(file["navigated"], true, "{file}");
        assert_eq!(caret(&session).0, subject, "from outside a test to the class it tests");
    }

    #[test]
    fn the_kotlin_plugin_goes_from_a_function_to_its_backticked_test_and_back() {
        let kotlin = plugin(include_str!("../../../../../plugins/kotlin/plugin.json"));
        let subject_path = "src/main/kotlin/com/acme/Cart.kt";
        let test_path = "src/test/kotlin/com/acme/CartTests.kt";
        let (registry, mut session, dir) = fixture_with(
            &[
                (
                    subject_path,
                    "package com.acme\n\nclass Cart {\n    fun total(): Int = 0\n\n    fun Cart.clear() {}\n}\n",
                ),
                (
                    test_path,
                    "package com.acme\n\nclass CartTests {\n    @Test\n    fun `total of nothing is zero`() {}\n}\n",
                ),
            ],
            kotlin,
        );
        let (subject, test) = (dir.path().join(subject_path), dir.path().join(test_path));
        let there = go(&registry, &mut session, &subject, 4);
        assert_eq!(there["navigated"], true, "{there}");
        assert_eq!(caret(&session), (test.clone(), 4));
        let back = go(&registry, &mut session, &test, 5);
        assert_eq!(back["navigated"], true, "{back}");
        assert_eq!(caret(&session), (subject.clone(), 3));
        let receiver = go(&registry, &mut session, &subject, 6);
        assert_eq!(receiver["symbol"], "clear", "{receiver}");
        assert_eq!(caret(&session), (test, 4), "a function with no test of its own leads to the test file");
    }
}
