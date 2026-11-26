use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_syntax::{Conversion, Feature, LineFold, OutlineItem, Outliner, Suggestions, TextFeatures, TextProblem};
use zen_workbench::plugins::PluginFeatures;

struct Kotlin;

impl Outliner for Kotlin {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".kt")
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Kotlin {
    fn supports(&self, feature: Feature) -> bool {
        feature == Feature::Conversion
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<zen_syntax::Expansion> {
        None
    }

    fn converts_from(&self) -> Vec<String> {
        zen_plugin_kotlin::convert::SOURCES.split(',').map(str::to_owned).collect()
    }

    fn convert(&self, from: &str, text: &str) -> Option<Conversion> {
        let converted = zen_plugin_kotlin::convert::convert(from, text)?;
        let problems = converted
            .problems
            .into_iter()
            .map(|problem| TextProblem { range: problem.start..problem.end, error: false, message: problem.message })
            .collect();
        Some(Conversion { text: converted.text, problems })
    }
}

fn dispatch(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    registry.dispatch(session, id, params).map_err(|error| error.to_string())
}

fn project(files: &[(&str, &str)]) -> (Registry, Session, tempfile::TempDir) {
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
    session.state_mut::<PluginFeatures>().set(vec![Arc::new(Kotlin)]);
    (registry, session, folder)
}

#[test]
fn a_java_file_becomes_a_kotlin_file_beside_it_and_opens() {
    let java = "package app;\n\npublic class Greeter {\n    public String greet(String name) {\n        return \"Hello, \" + name;\n    }\n}\n";
    let (registry, mut session, folder) =
        project(&[("src/app/Greeter.java", java), ("src/app/Other.java", "class Other {}\n")]);
    dispatch(&registry, &mut session, "file.document.open", json!({ "path": "src/app/Greeter.java" })).unwrap();
    assert!(registry.is_available(&session, "file.convert.toLanguage"), "a plug-in converts");
    let path = folder.path().join("src/app/Greeter.java");
    assert!(zen_workbench::plugins::convert::convertible(&session, &path));
    assert!(!zen_workbench::plugins::convert::convertible(&session, &folder.path().join("src/app/Greeter.kt")));

    let answer = dispatch(&registry, &mut session, "file.convert.toLanguage", json!({ "to": "kt" })).unwrap();
    let kotlin = folder.path().join("src/app/Greeter.kt");
    assert_eq!(answer["removed"], true, "{answer}");
    assert_eq!(answer["problems"], json!([]));
    assert!(!folder.path().join("src/app/Greeter.java").exists(), "the original went to the Trash");
    let text = std::fs::read_to_string(&kotlin).unwrap();
    assert!(text.starts_with("package app\n\nclass Greeter {\n    fun greet(name: String): String {"), "{text}");
    let active = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().path().to_owned();
    assert_eq!(active, kotlin, "the new file is shown");

    let kept = dispatch(
        &registry,
        &mut session,
        "file.convert.toLanguage",
        json!({ "path": "src/app/Other.java", "to": "kt", "keep": true }),
    )
    .unwrap();
    assert_eq!(kept["removed"], false);
    assert!(folder.path().join("src/app/Other.java").exists() && folder.path().join("src/app/Other.kt").exists());
    let again = dispatch(
        &registry,
        &mut session,
        "file.convert.toLanguage",
        json!({ "path": "src/app/Other.java", "to": "kt" }),
    );
    assert!(again.unwrap_err().contains("exists already"), "an existing file is never overwritten");
    let other = dispatch(
        &registry,
        &mut session,
        "file.convert.toLanguage",
        json!({ "path": "src/app/Other.java", "to": "scala" }),
    );
    assert!(other.unwrap_err().contains("No plug-in converts"));
}

#[test]
fn code_is_converted_as_text_with_the_places_to_check() {
    let (registry, mut session, _folder) = project(&[]);
    let answer = dispatch(
        &registry,
        &mut session,
        "editor.convert.text",
        json!({ "from": "java", "to": "kt", "text": "int count = 0;\ncount++;\n" }),
    )
    .unwrap();
    assert_eq!(answer["text"], "var count = 0\ncount++\n", "{answer}");
    let broken = dispatch(
        &registry,
        &mut session,
        "editor.convert.text",
        json!({ "from": "java", "to": "kt", "text": "class A {\n    int x = ;\n}\n" }),
    )
    .unwrap();
    assert_eq!(broken["problems"][0]["line"], 2, "{broken}");
    assert_eq!(broken["problems"][0]["column"], 5);
    assert!(broken["text"].as_str().unwrap().contains("int x = ;"), "the Java is kept: {broken}");
}
