use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::{PreviewKind, PreviewMode, Previews, SplitOrientation};
use crate::Workbench;

const ICON: &str =
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#;

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        std::fs::write(dir.path().join("project/icon.svg"), ICON).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path().join("project") })).unwrap();
        session.state_mut::<Previews>().set_data_dir(dir.path().join("data"));
        let mut fixture = Self { registry, session, dir };
        fixture.run("file.document.open", json!({ "path": fixture.icon() }));
        fixture
    }

    fn icon(&self) -> std::path::PathBuf {
        self.dir.path().join("project/icon.svg")
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }
}

#[test]
fn an_svg_opens_as_text_with_its_preview_beside_it() {
    let mut fixture = Fixture::new();
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.image().is_none(), "an SVG is a text document now");
    assert_eq!(document.text(), ICON);
    let answer = fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
    assert_eq!(
        (&answer["kind"], &answer["mode"], &answer["state"]),
        (&json!("svg"), &json!("editorAndPreview"), &json!("ready"))
    );
    assert_eq!(answer["size"], json!([16.0, 16.0]));
}

#[test]
fn the_preview_follows_the_unsaved_text_and_tells_why_an_invalid_one_does_not_render() {
    let mut fixture = Fixture::new();
    fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
    let bigger = ICON.replace("width=\"16\" height=\"16\"><rect", "width=\"32\" height=\"24\"><rect");
    fixture.run("editor.text.write", json!({ "path": fixture.icon(), "text": bigger, "save": false }));
    let answer = fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
    assert_eq!((&answer["state"], &answer["size"]), (&json!("ready"), &json!([32.0, 24.0])), "{answer}");
    assert_eq!(std::fs::read_to_string(fixture.icon()).unwrap(), ICON, "not saved");

    fixture.run(
        "editor.text.write",
        json!({ "path": fixture.icon(), "text": "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect", "save": false }),
    );
    let answer = fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
    assert_eq!(answer["state"], "invalid", "{answer}");
    assert!(!answer["error"].as_str().unwrap().is_empty());
}

#[test]
fn the_switch_sets_the_files_own_mode_and_keeps_it() {
    let mut fixture = Fixture::new();
    let other = fixture.dir.path().join("project/other.svg");
    std::fs::write(&other, ICON).unwrap();
    let set = fixture.run("editor.preview.setMode", json!({ "mode": "previewOnly" }));
    assert_eq!((&set["mode"], &set["scope"]), (&json!("previewOnly"), &json!("file")), "{set}");
    fixture.run("editor.preview.editorOnly", json!({}));
    let answer = fixture.run("editor.preview.getMode", json!({}));
    assert_eq!((&answer["mode"], &answer["scope"]), (&json!("editorOnly"), &json!("file")));
    let untouched = fixture.run("editor.preview.getMode", json!({ "path": other }));
    assert_eq!(
        (&untouched["mode"], &untouched["scope"]),
        (&json!("editorAndPreview"), &json!("default")),
        "another SVG keeps its kind's mode"
    );

    let mut again = Previews::default();
    again.set_data_dir(fixture.dir.path().join("data"));
    assert_eq!(again.mode_of(&fixture.icon(), PreviewKind::Svg), PreviewMode::EditorOnly, "kept in previews.json");
    assert_eq!(again.mode(PreviewKind::Svg), PreviewMode::EditorAndPreview, "the kind's mode is not changed");
    fixture.run("editor.preview.editorAndPreview", json!({}));
    assert_eq!(fixture.run("editor.preview.getMode", json!({ "path": "icon.svg" }))["mode"], "editorAndPreview");
    assert!(fixture.fail("editor.preview.setMode", json!({ "mode": "sideways" })).contains("sideways"));
}

#[test]
fn the_kinds_mode_applies_to_the_files_never_switched() {
    let mut fixture = Fixture::new();
    let other = fixture.dir.path().join("project/other.svg");
    std::fs::write(&other, ICON).unwrap();
    fixture.run("editor.preview.setMode", json!({ "mode": "previewOnly", "path": other }));
    let set = fixture.run("editor.preview.setMode", json!({ "mode": "editorOnly", "scope": "type" }));
    assert_eq!((&set["mode"], &set["scope"]), (&json!("editorOnly"), &json!("type")), "{set}");
    let answer = fixture.run("editor.preview.getMode", json!({ "path": other }));
    assert_eq!((&answer["mode"], &answer["scope"]), (&json!("previewOnly"), &json!("file")), "its own stays");

    fixture.run("editor.preview.setMode", json!({ "mode": "editorAndPreview" }));
    fixture.run("editor.preview.setMode", json!({ "mode": "previewOnly", "scope": "type" }));
    let answer = fixture.run("editor.preview.getMode", json!({}));
    assert_eq!(
        (&answer["mode"], &answer["scope"]),
        (&json!("previewOnly"), &json!("type")),
        "setting the kind's mode from a file drops that file's own"
    );
}

#[test]
fn previews_json_remembers_the_last_files_and_reads_the_first_format() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(super::FILE), r#"{ "svg": "previewOnly" }"#).unwrap();
    let mut previews = Previews::default();
    previews.set_data_dir(dir.path().to_owned());
    assert_eq!(
        previews.mode(PreviewKind::Svg),
        PreviewMode::PreviewOnly,
        "the modes of kinds alone, as 0.38.43 kept them"
    );
    assert_eq!(previews.scope_of(std::path::Path::new("/a.svg"), PreviewKind::Svg), super::ModeScope::Type);

    for index in 0..=super::MAX_FILES {
        previews.set_file_mode(&std::path::PathBuf::from(format!("/f{index}.svg")), PreviewMode::EditorOnly);
    }
    let mut again = Previews::default();
    again.set_data_dir(dir.path().to_owned());
    let first = std::path::Path::new("/f0.svg");
    let last = std::path::PathBuf::from(format!("/f{}.svg", super::MAX_FILES));
    assert_eq!(again.mode_of(first, PreviewKind::Svg), PreviewMode::PreviewOnly, "the oldest is forgotten");
    assert_eq!(again.mode_of(&last, PreviewKind::Svg), PreviewMode::EditorOnly);
    assert_eq!(again.mode_of(std::path::Path::new("/f1.svg"), PreviewKind::Svg), PreviewMode::EditorOnly);
}

#[test]
fn a_file_without_a_preview_is_refused_and_other_images_stay_pictures() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.dir.path().join("project/notes.txt"), "text").unwrap();
    let error = fixture.fail("editor.preview.getMode", json!({ "path": "notes.txt" }));
    assert!(error.contains("notes.txt"), "{error}");
    let png = fixture.dir.path().join("project/dot.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
    fixture.run("file.document.open", json!({ "path": png }));
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.image().is_some(), "a PNG is still a picture");
}

#[test]
fn the_preview_zooms_as_an_image_does() {
    let mut fixture = Fixture::new();
    fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
    assert_eq!(fixture.run("editor.image.zoom", json!({ "action": "actual" })), json!(1.0));
    let scale = fixture.run("editor.image.zoom", json!({ "action": "in" })).as_f64().unwrap();
    assert!((scale - 1.25).abs() < 1e-6, "{scale}");
    fixture.run("editor.preview.editorOnly", json!({}));
    assert!(
        !fixture.fail("editor.image.zoom", json!({ "action": "in" })).is_empty(),
        "no preview shown, nothing to zoom"
    );
}

#[test]
fn the_split_orientation_is_kept_per_file_across_starts() {
    let mut fixture = Fixture::new();
    let info = fixture.run("editor.preview.getMode", json!({}));
    assert_eq!(info["orientation"], "vertical", "side by side by default: {info}");
    let flipped = fixture.run("editor.preview.toggleOrientation", json!({}));
    assert_eq!(flipped["orientation"], "horizontal", "{flipped}");

    std::fs::write(fixture.dir.path().join("project/other.svg"), ICON).unwrap();
    let other = fixture.dir.path().join("project/other.svg");
    fixture.run("file.document.open", json!({ "path": other }));
    assert_eq!(
        fixture.run("editor.preview.getMode", json!({}))["orientation"],
        "vertical",
        "the other file has its own"
    );
    let set =
        fixture.run("editor.preview.setOrientation", json!({ "orientation": "vertical", "path": fixture.icon() }));
    assert_eq!(set["orientation"], "vertical");
    fixture.run("editor.preview.setOrientation", json!({ "orientation": "horizontal", "path": fixture.icon() }));

    let mut later = Previews::default();
    later.set_data_dir(fixture.dir.path().join("data"));
    assert_eq!(later.orientation_of(&fixture.icon()), SplitOrientation::Horizontal);
    assert_eq!(later.orientation_of(&other), SplitOrientation::Vertical);
}

#[test]
fn a_previews_file_from_before_orientations_still_reads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(super::FILE),
        r#"{ "kinds": { "markdown": "previewOnly" }, "files": [["/a.md", "editorOnly"]] }"#,
    )
    .unwrap();
    let mut previews = Previews::default();
    previews.set_data_dir(dir.path().to_owned());
    assert_eq!(previews.mode(PreviewKind::Markdown), PreviewMode::PreviewOnly);
    assert_eq!(previews.mode_of(std::path::Path::new("/a.md"), PreviewKind::Markdown), PreviewMode::EditorOnly);
    assert_eq!(previews.orientation_of(std::path::Path::new("/a.md")), SplitOrientation::Vertical);
}

#[test]
fn only_the_markdown_kind_reports_its_layout_to_the_settings() {
    let mut previews = Previews::default();
    previews.set_mode_by_command(PreviewKind::Svg, PreviewMode::PreviewOnly);
    assert_eq!(previews.take_markdown_layout_set(), None);
    previews.set_mode_by_command(PreviewKind::Markdown, PreviewMode::EditorOnly);
    assert_eq!(previews.take_markdown_layout_set(), Some(PreviewMode::EditorOnly));
    assert_eq!(previews.take_markdown_layout_set(), None, "once");
    previews.set_mode(PreviewKind::Markdown, PreviewMode::PreviewOnly);
    assert_eq!(previews.take_markdown_layout_set(), None, "the settings' own change does not come back");
}
