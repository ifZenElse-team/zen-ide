#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_workbench::Workbench;
use zen_workbench::terminal::Terminals;

const TIMEOUT: Duration = Duration::from_secs(60);

fn setup(project: &Path) -> (Registry, Session) {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let home = format!("HOME={}", project.display());
    session.state_mut::<Terminals>().set_launcher(Arc::new(move |cwd: &Path| {
        zen_terminal::Options::command("/usr/bin/env", &["-i", &home, "PATH=/usr/bin:/bin", "PS1=$ ", "/bin/sh"], cwd)
    }));
    (registry, session)
}

fn dispatch(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Result<Value, DispatchError> {
    registry.dispatch(session, id, params)
}

fn code(error: DispatchError) -> Option<String> {
    match error {
        DispatchError::Failed { error, .. } => error.code,
        _ => None,
    }
}

fn project_with_files() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("src/main.rs"), "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n")
        .unwrap();
    project
}

#[test]
fn the_links_on_a_shells_screen_are_listed_with_their_places() {
    let project = project_with_files();
    let (registry, mut session) = setup(project.path());
    dispatch(&registry, &mut session, "terminal.session.new", json!({})).unwrap();
    let printed = "printf 'error at src/main.rs:2:9 see https://example.com/doc. gone.rs:1\\n'\n";
    dispatch(&registry, &mut session, "terminal.session.send", json!({ "text": printed })).unwrap();
    let start = Instant::now();
    let list = loop {
        let list = dispatch(&registry, &mut session, "terminal.link.list", json!({})).unwrap();
        if list["links"].as_array().unwrap().len() >= 4 {
            break list;
        }
        assert!(start.elapsed() < TIMEOUT, "the printed links: {list}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let links = list["links"].as_array().unwrap();
    let printed = &links[links.len() - 2..];
    assert_eq!(printed[0]["kind"], "file");
    assert_eq!(printed[0]["text"], "src/main.rs:2:9");
    assert_eq!(printed[0]["target"], json!(project.path().join("src/main.rs").display().to_string()));
    assert_eq!((printed[0]["line"].as_u64(), printed[0]["column"].as_u64()), (Some(2), Some(9)));
    assert_eq!(printed[0]["startColumn"], 10, "after \"error at \"");
    assert_eq!(printed[0]["endColumn"], 24);
    assert_eq!(
        printed[1],
        json!({
            "text": "https://example.com/doc",
            "kind": "url",
            "target": "https://example.com/doc",
            "row": printed[0]["row"],
            "startColumn": 30,
            "endRow": printed[0]["row"],
            "endColumn": 52,
        })
    );
    assert!(links.iter().all(|link| link["text"] != "gone.rs:1"), "a path that names nothing is no link");
}

#[test]
fn a_file_opens_in_the_editor_at_its_line_and_column() {
    let project = project_with_files();
    let (registry, mut session) = setup(project.path());
    let opened =
        dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "src/main.rs:2:9" })).unwrap();
    assert_eq!(opened["action"], "openInEditor");
    assert_eq!((opened["line"].as_u64(), opened["done"].as_bool()), (Some(2), Some(true)));
    let caret = |session: &Session| {
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        let head = document.selections().primary().head;
        let point = document.snapshot().offset_to_point(head).unwrap();
        (document.path().to_owned(), point.line, point.column)
    };
    assert_eq!(caret(&session), (project.path().join("src/main.rs"), 1, 8), "line 2, column 9");
    dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "src/main.rs:1:99" })).unwrap();
    assert_eq!((caret(&session).1, caret(&session).2), (0, 11), "the end of the first line");
    dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "src/main.rs:99" })).unwrap();
    assert_eq!(caret(&session).1, 4, "the last line, after the final line break");
    let missing = dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "src/none.rs:1" }));
    assert!(missing.is_err());
    let folder =
        dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "src", "action": "openInEditor" }));
    assert!(folder.is_err(), "a folder does not open in the editor");
}

#[test]
fn the_other_file_actions_reveal_and_copy() {
    let project = project_with_files();
    let (registry, mut session) = setup(project.path());
    let main = project.path().join("src/main.rs");
    let revealed = dispatch(
        &registry,
        &mut session,
        "terminal.link.open",
        json!({ "target": "src/main.rs:3", "action": "revealInProject" }),
    )
    .unwrap();
    assert_eq!(revealed["done"], true);
    assert_eq!(session.state_mut::<Terminals>().take_reveal_in_project(), Some(main.clone()));
    assert_eq!(session.state_mut::<Terminals>().take_reveal_in_project(), None, "asked once");
    let folder = dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": "./src" })).unwrap();
    assert_eq!(folder["action"], "revealInProject", "a folder's default");

    let finder = dispatch(
        &registry,
        &mut session,
        "terminal.link.open",
        json!({ "target": main.display().to_string(), "action": "openInFinder", "dryRun": true }),
    )
    .unwrap();
    assert_eq!(finder["done"], false);
    assert!(finder["args"].as_array().unwrap().iter().any(|arg| arg.as_str().unwrap().contains("main.rs")));

    let copied = dispatch(
        &registry,
        &mut session,
        "terminal.link.open",
        json!({ "target": "src/main.rs:3", "action": "copyPath" }),
    )
    .unwrap();
    assert_eq!(copied["copied"], json!(main.display().to_string()));
    assert_eq!(zen_workbench::file_clipboard::clipboard_or_copied_text(&mut session), Some(main.display().to_string()));
    let wrong = dispatch(
        &registry,
        &mut session,
        "terminal.link.open",
        json!({ "target": "src/main.rs", "action": "copyLink" }),
    );
    assert!(wrong.is_err(), "a URL's action on a file");
}

#[test]
fn a_url_opens_in_the_browser_only_for_a_person() {
    let project = project_with_files();
    let (registry, mut session) = setup(project.path());
    let url = "https://example.com/a?b=1";
    let opened =
        dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": url, "dryRun": true })).unwrap();
    assert_eq!((opened["action"].as_str(), opened["done"].as_bool()), (Some("openInBrowser"), Some(false)));
    assert!(opened["args"].as_array().unwrap().contains(&json!(url)));

    let refused = zen_core::macros::as_agent(&mut session, |session| {
        dispatch(&registry, session, "terminal.link.open", json!({ "target": url, "dryRun": true }))
    });
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(zen_workbench::terminal::links::USER_ONLY));

    let copied = zen_core::macros::as_agent(&mut session, |session| {
        dispatch(&registry, session, "terminal.link.open", json!({ "target": url, "action": "copyLink" }))
    })
    .unwrap();
    assert_eq!(copied["copied"], url, "agents may copy a link");
    let editor =
        dispatch(&registry, &mut session, "terminal.link.open", json!({ "target": url, "action": "openInEditor" }));
    assert!(editor.is_err());
    let agent_file = zen_core::macros::as_agent(&mut session, |session| {
        dispatch(&registry, session, "terminal.link.open", json!({ "target": "src/main.rs:1" }))
    });
    assert!(agent_file.is_ok(), "agents open files in the editor");
}
