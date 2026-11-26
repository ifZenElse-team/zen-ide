use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_syntax::{
    Expansion, Feature, IndexDeclaration, IndexNode, IndexProblem, LineFold, OutlineItem, Outliner, ProjectIndex,
    Suggestions, TextFeatures,
};

use super::*;

struct Fake;

impl Outliner for Fake {
    fn handles(&self, _file_name: &str) -> bool {
        false
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Fake {
    fn supports(&self, _feature: Feature) -> bool {
        false
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }

    fn plugin_id(&self) -> Option<&str> {
        Some("fake")
    }

    fn index_declaration(&self) -> Option<IndexDeclaration> {
        Some(IndexDeclaration { files: vec!["*.yaml".to_owned()] })
    }

    fn index(&self, input: &[u8]) -> Option<ProjectIndex> {
        let input: Value = serde_json::from_slice(input).unwrap();
        let paths: Vec<&str> = input["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        let mut kinds = vec!["fake".to_owned()];
        kinds.extend(input["remotes"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_owned));
        let mut index = ProjectIndex { kinds, ..ProjectIndex::default() };
        for file in input["files"].as_array().unwrap() {
            let (path, text) = (file["path"].as_str().unwrap(), file["text"].as_str().unwrap());
            index.nodes.push(IndexNode {
                id: format!("n:{path}"),
                kind: "File".to_owned(),
                name: path.to_owned(),
                path: path.to_owned(),
                start: 0,
                end: text.len(),
                detail: format!("folder sub seen: {}", paths.contains(&"sub")),
                ..IndexNode::default()
            });
            index.nodes.push(IndexNode {
                id: format!("bad:{path}"),
                path: path.to_owned(),
                end: text.len() + 9,
                ..IndexNode::default()
            });
            index.problems.push(IndexProblem {
                path: path.to_owned(),
                start: 0,
                end: 4,
                severity: "warning".to_owned(),
                message: "seen".to_owned(),
            });
        }
        Some(index)
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().canonicalize().unwrap().join("fleet");
    for (file, text) in [
        ("a.yaml", "kind: A\n"),
        ("sub/b.yaml", "kind: A\n"),
        ("sub/c.yml", "kind: C\n"),
        ("node_modules/d.yaml", "kind: D\n"),
        (".git/e.yaml", "kind: E\n"),
    ] {
        let path = project.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(crate::build_tools::BuildEnv::default());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Indexes>().set_plugins(Arc::new(|| vec![Arc::new(Fake) as Arc<dyn TextFeatures>]));
    Fixture { registry, session, project, _dir: dir }
}

impl Fixture {
    fn get(&mut self, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, "plugins.index.get", params).unwrap()
    }
}

fn names(answer: &Value) -> Vec<String> {
    answer["indexes"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_index_reads_the_declared_files_of_the_project_and_keeps_only_what_points_into_them() {
    let mut fixture = fixture();
    let answer = fixture.get(json!({ "waitMs": 10000 }));
    assert_eq!(answer["indexing"], false, "{answer}");
    assert_eq!(names(&answer), ["a.yaml", "sub/b.yaml"], "not *.yml, node_modules or .git: {answer}");
    assert_eq!(answer["indexes"][0]["nodes"][0]["detail"], "folder sub seen: true", "folders are in `paths`");
    assert_eq!(answer["indexes"][0]["problems"].as_array().unwrap().len(), 2);
    assert_eq!(answer["indexes"][0]["truncated"], false);
    let one = fixture.get(json!({ "plugin": "fake", "path": fixture.project.join("sub/b.yaml") }));
    assert_eq!(names(&one), ["sub/b.yaml"], "narrowed to a file, absolute or relative");
    assert!(fixture.registry.dispatch(&mut fixture.session, "plugins.index.get", json!({ "plugin": "nope" })).is_err());
}

#[test]
fn past_the_limits_the_index_says_it_is_cut() {
    let mut fixture = fixture();
    fixture.session.state_mut::<Indexes>().set_limits(Limits { files: 1, ..Limits::default() });
    let answer = fixture.get(json!({ "waitMs": 10000 }));
    assert_eq!(names(&answer), ["a.yaml"]);
    assert_eq!(answer["indexes"][0]["truncated"], true, "{answer}");
}

#[test]
fn a_changed_file_is_indexed_again_after_a_pause_and_never_on_the_caller_s_thread() {
    let mut fixture = fixture();
    fixture.get(json!({ "waitMs": 10000 }));
    let added = fixture.project.join("sub/new.yaml");
    std::fs::write(&added, "kind: N\n").unwrap();
    let now = Instant::now();
    files_changed(&mut fixture.session, std::slice::from_ref(&added), now);
    poll(&mut fixture.session, now + Duration::from_millis(100));
    assert!(fixture.session.state::<Indexes>().unwrap().entries["fake"].running.is_none(), "it waits for the pause");
    assert_eq!(names(&fixture.get(json!({ "plugin": "fake" }))).len(), 2, "the old index is answered meanwhile");
    poll(&mut fixture.session, now + AFTER_CHANGE + Duration::from_millis(1));
    assert!(fixture.session.state::<Indexes>().unwrap().indexing(), "built in the background");
    let answer = fixture.get(json!({ "waitMs": 10000 }));
    assert_eq!(names(&answer), ["a.yaml", "sub/b.yaml", "sub/new.yaml"]);
    std::fs::write(fixture.project.join("notes.txt"), "x").unwrap();
    files_changed(&mut fixture.session, &[fixture.project.join("notes.txt")], Instant::now());
    assert!(
        !fixture.session.state::<Indexes>().unwrap().entries["fake"].stale,
        "a file it does not read changes nothing"
    );
}

#[test]
fn agents_see_nothing_of_the_files_hidden_from_them() {
    let mut fixture = fixture();
    fixture.get(json!({ "waitMs": 10000 }));
    fixture.session.state_mut::<hidden_files::HiddenFromAgents>().set(&["sub/**".to_owned()]);
    let Fixture { registry, session, .. } = &mut fixture;
    let answer = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "plugins.index.get", json!({ "references": true }))
    })
    .unwrap();
    assert_eq!(names(&answer), ["a.yaml"], "{answer}");
    assert_eq!(answer["indexes"][0]["problems"].as_array().unwrap().len(), 1);
}

struct Linker;

impl Outliner for Linker {
    fn handles(&self, _file_name: &str) -> bool {
        false
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Linker {
    fn supports(&self, _feature: Feature) -> bool {
        false
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }

    fn plugin_id(&self) -> Option<&str> {
        Some("linker")
    }

    fn index_declaration(&self) -> Option<IndexDeclaration> {
        Some(IndexDeclaration { files: vec!["*.yaml".to_owned()] })
    }

    fn index(&self, _input: &[u8]) -> Option<ProjectIndex> {
        let reference = |path: &str, start: usize, end: usize, target: &str| zen_syntax::IndexReference {
            path: path.to_owned(),
            start,
            end,
            target: target.to_owned(),
        };
        Some(ProjectIndex {
            kinds: vec!["linker".to_owned()],
            nodes: vec![
                IndexNode {
                    id: "n:a".to_owned(),
                    kind: "Thing".to_owned(),
                    name: "A".to_owned(),
                    path: "a.yaml".to_owned(),
                    start: 0,
                    end: 8,
                    ..IndexNode::default()
                },
                IndexNode {
                    id: "path:sub".to_owned(),
                    kind: "Folder".to_owned(),
                    name: "sub".to_owned(),
                    path: "sub".to_owned(),
                    ..IndexNode::default()
                },
            ],
            references: vec![
                reference("sub/b.yaml", 6, 7, "n:a"),
                reference("a.yaml", 0, 4, "path:sub"),
                reference("a.yaml", 6, 7, "n:a"),
            ],
            problems: vec![IndexProblem {
                path: "sub/b.yaml".to_owned(),
                start: 6,
                end: 7,
                severity: "error".to_owned(),
                message: "B is broken".to_owned(),
            }],
            completions: vec![zen_syntax::IndexCompletion {
                files: vec!["sub/b.yaml".to_owned()],
                after: ".Values.".to_owned(),
                items: ["image.tag", "image.repository", "replicas"]
                    .map(|label| zen_syntax::IndexCompletionItem {
                        label: label.to_owned(),
                        detail: "values.yaml".to_owned(),
                    })
                    .to_vec(),
            }],
            ..ProjectIndex::default()
        })
    }
}

fn linked() -> Fixture {
    let mut fixture = fixture();
    fixture.session.state_mut::<Indexes>().set_plugins(Arc::new(|| vec![Arc::new(Linker) as Arc<dyn TextFeatures>]));
    fixture.get(json!({ "waitMs": 10000 }));
    fixture
}

impl Fixture {
    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn active(&self) -> Option<(PathBuf, usize)> {
        let workbench = self.session.state::<Workbench>()?;
        let document = workbench.active_document()?;
        Some((document.path().to_path_buf(), 0))
    }
}

#[test]
fn an_index_problem_shows_in_its_file_while_the_text_is_the_one_indexed() {
    let mut fixture = linked();
    let path = fixture.project.join("sub/b.yaml");
    let listed = fixture.run("problems.file.list", json!({ "path": path }));
    let text = listed.to_string();
    assert!(text.contains("B is broken") && text.contains("linker"), "{text}");
    fixture.run("file.document.open", json!({ "path": path }));
    fixture.run("editor.text.replace", json!({ "path": path, "oldText": "kind: A", "newText": "kind:  A" }));
    let listed = fixture.run("problems.file.list", json!({ "path": path }));
    assert!(!listed.to_string().contains("B is broken"), "an edited file waits for its new index: {listed}");
}

#[test]
fn go_to_declaration_follows_an_index_reference_to_a_file_or_a_folder() {
    let mut fixture = linked();
    let b = fixture.project.join("sub/b.yaml");
    fixture.run("file.document.open", json!({ "path": b }));
    let went = fixture.run("navigate.declaration.goTo", json!({ "path": b, "offset": 6 }));
    assert_eq!(went["navigated"], true, "{went}");
    assert_eq!(fixture.active().map(|(path, _)| path), Some(fixture.project.join("a.yaml")));
    let a = fixture.project.join("a.yaml");
    fixture.run("navigate.declaration.goTo", json!({ "path": a, "offset": 1 }));
    assert_eq!(
        fixture.session.state_mut::<Indexes>().take_reveal(),
        Some(fixture.project.join("sub")),
        "a folder is shown"
    );
    let _ =
        fixture.registry.dispatch(&mut fixture.session, "navigate.declaration.goTo", json!({ "path": a, "offset": 6 }));
    assert_eq!(
        fixture.session.state_mut::<Indexes>().take_reveal(),
        None,
        "a name on its own declaration leads nowhere new"
    );
}

#[test]
fn find_usages_lists_the_references_to_a_node() {
    let mut fixture = linked();
    let b = fixture.project.join("sub/b.yaml");
    fixture.run("file.document.open", json!({ "path": b }));
    let found =
        fixture.run("navigate.usages.find", json!({ "path": b, "offset": 6, "waitMs": 10000, "compact": true }));
    assert_eq!(found["total"], 1, "the reference in sub/b.yaml, not the declaration's own: {found}");
    assert_eq!(found["symbol"], "Usages of Thing A", "{found}");
}

#[test]
fn the_index_is_told_the_project_s_git_remotes() {
    let mut fixture = fixture();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git").args(args).current_dir(&fixture.project).output().unwrap();
        assert!(status.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&status.stderr));
    };
    git(&["init", "-q"]);
    git(&["remote", "add", "origin", "git@example.org:team/fleet.git"]);
    git(&["remote", "add", "mirror", "https://example.org/team/fleet"]);
    let answer = fixture.get(json!({ "waitMs": 10000 }));
    let kinds = answer["indexes"][0]["kinds"].as_array().unwrap();
    assert_eq!(
        kinds,
        &[json!("fake"), json!("git@example.org:team/fleet.git"), json!("https://example.org/team/fleet")]
    );
}

#[test]
fn completion_offers_the_index_s_names_after_their_prefix_in_the_files_they_are_for() {
    let mut fixture = linked();
    let b = fixture.project.join("sub/b.yaml");
    fixture.run("file.document.open", json!({ "path": b }));
    let text = "kind: A\nimage: {{ .Values.im";
    fixture.run("editor.text.replace", json!({ "path": b, "oldText": "kind: A\n", "newText": text }));
    fixture.run("navigate.place.goTo", json!({ "path": b, "offset": text.len() }));
    let listed = fixture.run("editor.completion.start", json!({}));
    let labels: Vec<&str> =
        listed["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
    assert!(labels.contains(&"image.tag") && labels.contains(&"image.repository"), "{listed}");
    assert!(!labels.contains(&"replicas"), "only names that start with what is typed: {listed}");
    let a = fixture.project.join("a.yaml");
    assert!(complete(&fixture.session, &a, ".Values.", 8).is_none(), "a.yaml is not one of the files");
    assert!(complete(&fixture.session, &b, "x: .Values.a b", 14).is_none(), "a space ends the name");
}
