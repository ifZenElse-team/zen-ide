use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::code_style::{self, CodeStyle};
use crate::indent_detection::DetectedIndent;
use crate::{Document, Workbench, project_settings};

pub const DETECT_INDENTS: &str = "editor.indents.detect";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetIndent)?;
    registry.register(UseIndent::<false>)?;
    registry.register(UseIndent::<true>)?;
    Ok(())
}

pub fn detection_enabled(settings: &zen_core::Settings) -> bool {
    settings.get::<bool>(DETECT_INDENTS).unwrap_or(true)
}

/// Where the indentation a file is edited with comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IndentSource {
    /// The configured code style: Editor → Code Style with the project's own code style on top.
    Configured,
    /// The file's own lines.
    Detected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfiguredStyle {
    pub style: CodeStyle,
    pub fixed: bool,
}

pub fn configured_at(session: &Session, path: &Path) -> ConfiguredStyle {
    ConfiguredStyle { style: project_settings::code_style_for_path(session, path), fixed: false }
}

pub fn with_detected(style: CodeStyle, detected: DetectedIndent) -> CodeStyle {
    match detected {
        DetectedIndent::Tabs => CodeStyle { use_tabs: true, ..style },
        DetectedIndent::Spaces { width } => CodeStyle { use_tabs: false, indent: width, ..style },
    }
}

/// The indentation of one open file, as `editor.indent.get` reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileIndent {
    /// The file.
    pub path: String,
    /// The code style Tab, Enter, ⇧Tab and the status bar use.
    pub effective: CodeStyle,
    /// Where the indentation of `effective` comes from.
    pub source: IndentSource,
    /// The configured code style; Reformat Code uses it.
    pub configured: CodeStyle,
    /// The indentation found in the file's lines when it was opened, if enough lines were indented.
    pub detected: Option<DetectedIndent>,
    /// Whether Follow the indentation a file already has is on.
    pub detection_enabled: bool,
    /// Whether the file was switched to the configured indentation (`editor.indent.useConfigured`).
    pub uses_configured: bool,
    /// Whether the detected indentation differs from the configured one, so that there is something to switch between.
    pub can_switch: bool,
}

pub fn of(session: &Session, document: &Document) -> FileIndent {
    let configured = configured_at(session, document.path());
    let enabled = detection_enabled(session.settings());
    let detected = document.detected_indent();
    let alternative = detected
        .filter(|_| enabled && !configured.fixed)
        .map(|detected| with_detected(configured.style, detected))
        .filter(|style| code_style::indent_unit(*style) != code_style::indent_unit(configured.style));
    let (effective, source) = match alternative {
        Some(style) if !document.uses_configured_indent() => (style, IndentSource::Detected),
        _ => (configured.style, IndentSource::Configured),
    };
    FileIndent {
        path: document.path().display().to_string(),
        effective,
        source,
        configured: configured.style,
        detected,
        detection_enabled: enabled,
        uses_configured: document.uses_configured_indent(),
        can_switch: alternative.is_some(),
    }
}

pub fn effective_style(session: &Session, document: &Document) -> CodeStyle {
    of(session, document).effective
}

pub fn configured_style(session: &Session, document: &Document) -> CodeStyle {
    configured_at(session, document.path()).style
}

/// The parameters of the `editor.indent.*` commands.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct IndentParams {
    /// An open file, absolute or relative to the project; the active file when absent.
    pub path: Option<PathBuf>,
}

fn find<'a>(workbench: &'a Workbench, path: Option<&Path>) -> Result<&'a Document, CommandError> {
    let document = match path {
        Some(path) => {
            let path = workbench.resolve(path);
            workbench.documents().iter().find(|document| document.path() == path)
        }
        None => workbench.active_document(),
    };
    document
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn report(session: &Session, path: Option<&Path>) -> Result<FileIndent, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    Ok(of(session, find(workbench, path)?))
}

fn has_text(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

pub struct GetIndent;

impl Command for GetIndent {
    type Params = IndentParams;
    type Output = FileIndent;
    const ID: &'static str = "editor.indent.get";
    const TITLE: &'static str = "Indentation";
    const DESCRIPTION: &'static str = "Tells how an open file is indented by Tab, Enter and ⇧Tab (the status bar's \
         indent): the effective code style, whether it comes from the configured code style or was detected from \
         the file's lines (Editor → Code Style → General → Follow the indentation a file already has), the \
         configured style Reformat Code uses, and whether the two can be switched.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: IndentParams) -> Result<FileIndent, CommandError> {
        report(session, params.path.as_deref())
    }
}

pub struct UseIndent<const CONFIGURED: bool>;

impl<const CONFIGURED: bool> Command for UseIndent<CONFIGURED> {
    type Params = IndentParams;
    type Output = FileIndent;
    const ID: &'static str = if CONFIGURED { "editor.indent.useConfigured" } else { "editor.indent.useDetected" };
    const TITLE: &'static str = if CONFIGURED { "Use Configured Indentation" } else { "Use Detected Indentation" };
    const DESCRIPTION: &'static str = if CONFIGURED {
        "Edits an open file with the configured indentation (Editor → Code Style) instead of the one detected \
         from its lines, as the status bar's indent menu does, until the file is closed."
    } else {
        "Edits an open file again with the indentation detected from its lines, as the status bar's indent menu \
         does; fails when nothing was detected or detection is off."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: IndentParams) -> Result<FileIndent, CommandError> {
        let current = report(session, params.path.as_deref())?;
        if !CONFIGURED {
            if !current.detection_enabled {
                return Err(CommandError::new(t!("workbench-indent-detection-off")));
            }
            if !current.can_switch {
                return Err(CommandError::new(t!("workbench-indent-nothing-detected")));
            }
        }
        let path = PathBuf::from(&current.path);
        let workbench = session.state_mut::<Workbench>();
        if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
            document.set_uses_configured_indent(CONFIGURED);
        }
        report(session, Some(&path))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::*;

    const TWO_SPACES: &str = "a:\n  b: 1\n  c: 2\n  d:\n    e: 3\n";
    const TABBED_JSON: &str = "{\n\t\"a\": {\n\t\t\"b\": 1\n\t},\n\t\"c\": 2\n}\n";
    const GO_SPACES: &str = "package main\n\nfunc main() {\n    a()\n    b()\n    if c {\n        d()\n    }\n}\n";

    fn open(files: &[(&str, &str)]) -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        for (name, text) in files {
            let file = dir.path().join(name);
            std::fs::write(&file, text).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        }
        (registry, session, dir)
    }

    fn get(registry: &Registry, session: &mut Session, path: &Path) -> Value {
        registry.dispatch(session, "editor.indent.get", json!({ "path": path })).unwrap()
    }

    fn tab(registry: &Registry, session: &mut Session, path: &Path) -> String {
        registry.dispatch(session, "file.document.open", json!({ "path": path })).unwrap();
        registry.dispatch(session, "editor.edit.tab", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        registry.dispatch(session, "editor.edit.undo", json!({})).unwrap();
        text.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    #[test]
    fn the_detected_indent_drives_tab_and_reformat_keeps_the_configured_one() {
        let (registry, mut session, dir) = open(&[("a.json", TABBED_JSON), ("main.go", GO_SPACES)]);
        let json_path = dir.path().join("a.json");
        let state = get(&registry, &mut session, &json_path);
        assert_eq!(state["detected"], json!({ "kind": "tabs" }));
        assert_eq!(state["source"], "detected");
        assert_eq!(state["effective"]["useTabs"], true);
        assert_eq!(state["configured"], json!({ "useTabs": false, "tabSize": 2, "indent": 2 }));
        assert_eq!(tab(&registry, &mut session, &json_path), "\t");
        let go = dir.path().join("main.go");
        assert_eq!(get(&registry, &mut session, &go)["detected"], json!({ "kind": "spaces", "width": 4 }));
        assert_eq!(tab(&registry, &mut session, &go), "    ");
        let workbench = session.state::<Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == go).unwrap();
        assert_eq!(configured_style(&session, document), CodeStyle { use_tabs: true, tab_size: 4, indent: 4 });
        assert_eq!(project_settings::indent_unit_of(&session, document), "    ");
    }

    #[test]
    fn the_status_bar_switch_goes_both_ways() {
        let (registry, mut session, dir) = open(&[("a.yaml", TWO_SPACES)]);
        let path = dir.path().join("a.yaml");
        let mut styles = BTreeMap::new();
        styles.insert("yaml".to_owned(), CodeStyle { use_tabs: false, tab_size: 4, indent: 4 });
        code_style::apply(session.settings_mut(), &styles);
        assert_eq!(get(&registry, &mut session, &path)["canSwitch"], true);
        assert_eq!(tab(&registry, &mut session, &path), "  ");
        let state = registry.dispatch(&mut session, "editor.indent.useConfigured", json!({ "path": "a.yaml" }));
        assert!(state.is_err(), "a relative path needs a project");
        let state = registry.dispatch(&mut session, "editor.indent.useConfigured", json!({})).unwrap();
        assert_eq!((state["source"].clone(), state["usesConfigured"].clone()), (json!("configured"), json!(true)));
        assert_eq!(tab(&registry, &mut session, &path), "    ");
        let state = registry.dispatch(&mut session, "editor.indent.useDetected", json!({ "path": path })).unwrap();
        assert_eq!(state["source"], "detected");
        assert_eq!(tab(&registry, &mut session, &path), "  ");
    }

    #[test]
    fn turning_detection_off_returns_every_file_to_the_configured_style() {
        let (registry, mut session, dir) = open(&[("a.json", TABBED_JSON), ("main.go", GO_SPACES)]);
        session.settings_mut().set(DETECT_INDENTS, false).unwrap();
        let json_path = dir.path().join("a.json");
        let state = get(&registry, &mut session, &json_path);
        assert_eq!((state["source"].clone(), state["detectionEnabled"].clone()), (json!("configured"), json!(false)));
        assert_eq!(state["detected"], json!({ "kind": "tabs" }), "what the file uses is still reported");
        assert_eq!(tab(&registry, &mut session, &json_path), "  ");
        assert_eq!(tab(&registry, &mut session, &dir.path().join("main.go")), "\t");
        let error = registry.dispatch(&mut session, "editor.indent.useDetected", json!({})).unwrap_err();
        assert!(error.to_string().contains(&t!("workbench-indent-detection-off")), "{error}");
        session.settings_mut().set(DETECT_INDENTS, true).unwrap();
        assert_eq!(tab(&registry, &mut session, &json_path), "\t");
    }

    #[test]
    fn a_file_with_too_few_indented_lines_keeps_the_configured_style() {
        let (registry, mut session, dir) = open(&[("a.yaml", "a:\n\tb: 1\n"), ("notes.txt", "a\n\tb\n")]);
        for name in ["a.yaml", "notes.txt"] {
            let state = get(&registry, &mut session, &dir.path().join(name));
            assert_eq!((state["detected"].clone(), state["source"].clone()), (Value::Null, json!("configured")));
        }
        assert_eq!(tab(&registry, &mut session, &dir.path().join("a.yaml")), "  ");
        let error = registry.dispatch(&mut session, "editor.indent.useDetected", json!({})).unwrap_err();
        assert!(error.to_string().contains(&t!("workbench-indent-nothing-detected")), "{error}");
    }

    #[test]
    fn go_makefiles_go_mod_and_yaml_keep_what_their_format_needs() {
        let files = [
            ("short.go", "package main\n\nfunc main() {\n    x()\n}\n"),
            ("Makefile", "define HELP\n    build\n    test\n    vet\nendef\n\nall:\n\tgo build\n"),
            ("go.mod", "module a\n\ngo 1.24\n"),
            ("tabbed.yaml", "a:\n\tb: 1\n\tc: 2\n\td: 3\n"),
            ("spaced.yaml", "a:\n    b: 1\n    c: 2\n    d: 3\n"),
        ];
        let (registry, mut session, dir) = open(&files);
        let expected =
            [("short.go", "\t"), ("Makefile", "\t"), ("go.mod", "\t"), ("tabbed.yaml", "  "), ("spaced.yaml", "    ")];
        for (name, unit) in expected {
            assert_eq!(tab(&registry, &mut session, &dir.path().join(name)), unit, "{name}");
        }
        let makefile = get(&registry, &mut session, &dir.path().join("Makefile"));
        assert_eq!((makefile["detected"].clone(), makefile["source"].clone()), (Value::Null, json!("configured")));
        let yaml = get(&registry, &mut session, &dir.path().join("tabbed.yaml"));
        assert_eq!((yaml["detected"].clone(), yaml["effective"]["useTabs"].clone()), (Value::Null, json!(false)));
    }

    #[test]
    fn the_same_indent_as_configured_has_nothing_to_switch() {
        let (registry, mut session, dir) = open(&[("b.yaml", TWO_SPACES)]);
        let state = get(&registry, &mut session, &dir.path().join("b.yaml"));
        assert_eq!(state["detected"], json!({ "kind": "spaces", "width": 2 }));
        assert_eq!((state["canSwitch"].clone(), state["source"].clone()), (json!(false), json!("configured")));
    }

    #[test]
    fn reloading_from_disk_detects_again() {
        let (registry, mut session, dir) = open(&[("c.yaml", "a: 1\n")]);
        let path = dir.path().join("c.yaml");
        assert_eq!(get(&registry, &mut session, &path)["detected"], Value::Null);
        std::fs::write(&path, "a:\n    b: 1\n    c: 2\n    d: 3\n").unwrap();
        session.state_mut::<Workbench>().active_document_mut().unwrap().reload_from_disk();
        assert_eq!(get(&registry, &mut session, &path)["detected"], json!({ "kind": "spaces", "width": 4 }));
    }

    #[test]
    fn the_detected_indent_keeps_the_tab_size() {
        let style = CodeStyle { use_tabs: false, tab_size: 8, indent: 4 };
        assert_eq!(with_detected(style, DetectedIndent::Tabs), CodeStyle { use_tabs: true, tab_size: 8, indent: 4 });
        assert_eq!(
            with_detected(style, DetectedIndent::Spaces { width: 2 }),
            CodeStyle { use_tabs: false, tab_size: 8, indent: 2 }
        );
    }
}
