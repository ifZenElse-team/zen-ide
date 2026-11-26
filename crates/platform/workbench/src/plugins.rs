pub mod colors;
pub mod convert;
pub mod emmet;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zen_core::{Registry, RegistryError, Session};
use zen_syntax::{ColorSpan, Feature, OutlineItem, TextFeatures};

use crate::language::Problem;
use crate::{Document, Workbench};

pub const CLOSING_TAGS: &str = "editor.smartKeys.closingTags";

pub type FeatureSource = Arc<dyn Fn() -> Vec<Arc<dyn TextFeatures>> + Send + Sync>;

#[derive(Default)]
pub struct PluginFeatures {
    source: Option<FeatureSource>,
    outlines: HashMap<PathBuf, (u64, Option<String>, Vec<OutlineItem>)>,
    problems: HashMap<PathBuf, (u64, Option<String>, Vec<Problem>)>,
    colors: HashMap<PathBuf, (u64, Arc<Vec<ColorSpan>>)>,
    inlays: HashMap<PathBuf, (u64, Vec<crate::language::inlay_hints::InlayHint>)>,
}

impl PluginFeatures {
    pub fn set_source(&mut self, source: FeatureSource) {
        self.source = Some(source);
        self.outlines.clear();
        self.colors.clear();
        self.problems.clear();
        self.inlays.clear();
    }

    pub fn set(&mut self, plugins: Vec<Arc<dyn TextFeatures>>) {
        self.set_source(Arc::new(move || plugins.clone()));
    }

    pub fn for_file(&self, path: &Path) -> Option<Arc<dyn TextFeatures>> {
        let name = zen_vfs::file_types::known_name_at(path, None)?;
        let plugins = self.source.as_ref().map(|source| source()).unwrap_or_default();
        let built_in: Arc<dyn TextFeatures> = Arc::new(crate::editor_config::EditorConfigSyntax);
        plugins
            .into_iter()
            .chain(std::iter::once(built_in))
            .filter(|plugin| plugin.handles(&name))
            .max_by_key(|plugin| plugin.specificity(&name))
    }

    pub fn all(&self) -> Vec<Arc<dyn TextFeatures>> {
        self.source.as_ref().map(|source| source()).unwrap_or_default()
    }

    pub fn with_feature(&self, path: &Path, feature: Feature) -> Option<Arc<dyn TextFeatures>> {
        self.for_file(path).filter(|plugin| plugin.supports(feature))
    }
}

pub fn jvm_file(plugins: &[Arc<dyn TextFeatures>], path: &Path, text: &str) -> Option<zen_syntax::JvmFile> {
    let name = zen_vfs::file_types::known_name_at(path, None)?;
    let plugin = plugins
        .iter()
        .filter(|plugin| plugin.handles(&name) && plugin.supports(Feature::Jvm))
        .max_by_key(|plugin| plugin.specificity(&name))?;
    let stem = path.file_stem()?.to_string_lossy();
    Some(plugin.jvm(text)?.named_after(&stem))
}

pub(crate) fn plugin_for(session: &Session, path: &Path, feature: Feature) -> Option<Arc<dyn TextFeatures>> {
    let plugin = match session.state::<PluginFeatures>() {
        Some(features) => features.with_feature(path, feature),
        None => PluginFeatures::default().with_feature(path, feature),
    }?;
    Some(dialect_of(session, path).and_then(|dialect| plugin.in_dialect(&dialect)).unwrap_or(plugin))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dialect_of(session: &Session, path: &Path) -> Option<String> {
    crate::database::assist::dialect(session, path)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn dialect_of(_: &Session, _: &Path) -> Option<String> {
    None
}

pub(crate) fn line_folds(plugin: &dyn TextFeatures, document: &Document) -> Vec<zen_editor::Fold> {
    let lines = plugin.folds(document.text());
    let text = document.snapshot();
    let line_end = |line: usize| {
        let Ok(next) = text.point_to_offset(zen_text::Point { line: line + 1, column: 0 }) else {
            return text.len_chars();
        };
        let end = next.saturating_sub(1);
        let crlf = end > 0 && text.slice(end - 1..end).is_ok_and(|before| before == "\r");
        if crlf { end - 1 } else { end }
    };
    let mut folds: Vec<zen_editor::Fold> = lines
        .into_iter()
        .filter(|fold| fold.end_line < text.len_lines())
        .map(|fold| {
            zen_editor::Fold::new(line_end(fold.start_line)..line_end(fold.end_line), zen_editor::FoldKind::Block)
        })
        .filter(|fold| fold.range.start < fold.range.end)
        .collect();
    folds.sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
    folds.dedup();
    folds
}

pub(crate) fn outline(session: &mut Session, path: &Path, text: &str) -> Option<Vec<OutlineItem>> {
    let plugin = plugin_for(session, path, Feature::Outline)?;
    let Some(version) = open_document(session, path).map(Document::version) else {
        return Some(plugin.outline(text));
    };
    let dialect = dialect_of(session, path);
    let cached = session.state::<PluginFeatures>().and_then(|features| features.outlines.get(path));
    if let Some((seen, read_in, items)) = cached
        && *seen == version
        && *read_in == dialect
    {
        return Some(items.clone());
    }
    let items = plugin.outline(text);
    let open = open_paths(session);
    let features = session.state_mut::<PluginFeatures>();
    features.outlines.retain(|known, _| open.contains(known));
    features.outlines.insert(path.to_owned(), (version, dialect, items.clone()));
    Some(items)
}

pub(crate) fn text_problems(plugin: &dyn TextFeatures, path: &Path, text: &str) -> Vec<Problem> {
    let source = zen_vfs::file_types::type_at(path, Some(zen_vfs::file_types::head(text)))
        .map_or_else(|| "plugin".to_owned(), |ty| ty.id);
    plugin
        .problems(text)
        .into_iter()
        .map(|found| {
            let finding = zen_project::inspect::Finding {
                start: found.range.start,
                end: found.range.end,
                severity: if found.error {
                    zen_project::inspect::FindingSeverity::Error
                } else {
                    zen_project::inspect::FindingSeverity::Warning
                },
                message: found.message,
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                source: source.clone(),
                code: "syntax".to_owned(),
                deprecated: false,
                unused: false,
            };
            crate::inspections::problem(text, &finding)
        })
        .collect()
}

pub(crate) fn problems(session: &mut Session, path: &Path) -> Vec<Problem> {
    let Some(plugin) = plugin_for(session, path, Feature::Problems) else { return Vec::new() };
    let Some((version, text)) = open_document(session, path)
        .filter(|document| document.image().is_none())
        .map(|document| (document.version(), document.text().to_owned()))
    else {
        return Vec::new();
    };
    let dialect = dialect_of(session, path);
    let cached = session.state::<PluginFeatures>().and_then(|features| features.problems.get(path));
    if let Some((seen, read_in, problems)) = cached
        && *seen == version
        && *read_in == dialect
    {
        return problems.clone();
    }
    let problems = text_problems(plugin.as_ref(), path, &text);
    let open = open_paths(session);
    let features = session.state_mut::<PluginFeatures>();
    features.problems.retain(|known, _| open.contains(known));
    features.problems.insert(path.to_owned(), (version, dialect, problems.clone()));
    problems
}

pub(crate) fn inlays(
    session: &mut Session,
    path: &Path,
) -> Option<(u64, Vec<crate::language::inlay_hints::InlayHint>)> {
    let plugin = plugin_for(session, path, Feature::Inlays)?;
    let document = open_document(session, path).filter(|document| document.image().is_none())?;
    let version = document.version();
    if let Some((seen, hints)) = session.state::<PluginFeatures>().and_then(|features| features.inlays.get(path))
        && *seen == version
    {
        return Some((version, hints.clone()));
    }
    let text = document.snapshot().clone();
    let raw = document.text().to_owned();
    let mut chars = 0;
    let mut bytes = 0;
    let hints: Vec<_> = plugin
        .inlays(&raw)
        .into_iter()
        .enumerate()
        .filter_map(|(index, inlay)| {
            chars += raw.get(bytes..inlay.offset)?.chars().count();
            bytes = inlay.offset;
            let point = text.offset_to_point(chars).ok()?;
            Some(crate::language::inlay_hints::InlayHint {
                id: index as u64,
                offset: chars,
                line: point.line as u32 + 1,
                column: point.column as u32 + 1,
                label: inlay.text,
                kind: None,
                padding_left: true,
                padding_right: false,
                tooltip: None,
                resolvable: false,
            })
        })
        .collect();
    let open = open_paths(session);
    let features = session.state_mut::<PluginFeatures>();
    features.inlays.retain(|known, _| open.contains(known));
    features.inlays.insert(path.to_owned(), (version, hints.clone()));
    Some((version, hints))
}

pub(crate) fn checks(session: &Session, path: &Path) -> bool {
    plugin_for(session, path, Feature::Problems).is_some()
}

pub(crate) fn known_problems(session: &Session, path: &Path) -> Vec<Problem> {
    session
        .state::<PluginFeatures>()
        .and_then(|features| features.problems.get(path))
        .map(|(_, _, problems)| problems.clone())
        .unwrap_or_default()
}

fn open_paths(session: &Session) -> Vec<PathBuf> {
    session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default()
}

fn open_document<'a>(session: &'a Session, path: &Path) -> Option<&'a Document> {
    session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)
}

pub(crate) fn after_typing(session: &mut Session, typed: char) {
    if !session.settings().get_or(CLOSING_TAGS, true) {
        return;
    }
    let Some((path, caret)) = session.state::<Workbench>().and_then(Workbench::active_document).and_then(|document| {
        let selections = document.selections();
        let primary = selections.primary();
        (selections.len() == 1 && primary.anchor == primary.head).then(|| (document.path().to_owned(), primary.head))
    }) else {
        return;
    };
    let Some(plugin) = plugin_for(session, &path, Feature::Typing).filter(|plugin| plugin.reacts_to(typed)) else {
        return;
    };
    let Some(text) = open_document(session, &path).map(|document| document.text().to_owned()) else { return };
    let byte = crate::language::byte_offset(&text, caret);
    let Some(added) = plugin.on_type(&text, byte, typed) else { return };
    let start = crate::language::char_offset(&text, added.range.start);
    let end = crate::language::char_offset(&text, added.range.end);
    let caret_in = added.caret.filter(|at| added.text.is_char_boundary(*at)).unwrap_or(added.text.len());
    let Some(document) = session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path)
    else {
        return;
    };
    let at = start + document.with_line_separator(added.text[..caret_in].to_owned()).chars().count();
    if document.replace_ranges(vec![(start..end, added.text)]) {
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at)));
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    emmet::register(registry)?;
    convert::register(registry)?;
    colors::register(registry)
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;

    use zen_syntax::{ColorSpan, Expansion, LineFold, OutlineItem, Outliner, Suggestion, Suggestions, SymbolKind};

    use super::*;

    #[derive(Default)]
    pub(crate) struct WebPlugin {
        pub(crate) calls: Mutex<usize>,
        pub(crate) outlines: Mutex<usize>,
        pub(crate) colors: Mutex<usize>,
        pub(crate) go: bool,
    }

    impl Outliner for WebPlugin {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".web") || (self.go && file_name.ends_with(".go"))
        }

        fn outline(&self, text: &str) -> Vec<OutlineItem> {
            *self.outlines.lock().unwrap() += 1;
            let mut offset = 0;
            let mut items = Vec::new();
            for line in text.split_inclusive('\n') {
                if let Some(name) = line.strip_prefix('#') {
                    let start = text[..offset].chars().count();
                    items.push(OutlineItem {
                        name: name.trim().to_owned(),
                        detail: None,
                        kind: SymbolKind::Rule,
                        start,
                        end: start + line.trim_end().chars().count(),
                        selection_start: start,
                        children: Vec::new(),
                    });
                }
                offset += line.len();
            }
            items
        }
    }

    impl TextFeatures for WebPlugin {
        fn supports(&self, _: Feature) -> bool {
            true
        }

        fn folds(&self, text: &str) -> Vec<LineFold> {
            *self.calls.lock().unwrap() += 1;
            let mut open = Vec::new();
            let mut folds = Vec::new();
            for (line, content) in text.split('\n').enumerate() {
                if content.trim_end().ends_with('{') {
                    open.push(line);
                } else if content.trim() == "}"
                    && let Some(start) = open.pop()
                {
                    folds.push(LineFold { start_line: start, end_line: line });
                }
            }
            folds.sort();
            folds
        }

        fn complete(&self, text: &str, caret: usize) -> Option<Suggestions> {
            let from = text[..caret].rfind('<').map_or(caret, |at| at + 1);
            let item = |label: &str, insert: &str, caret: Option<usize>| Suggestion {
                label: label.to_owned(),
                kind: "tag".to_owned(),
                insert: insert.to_owned(),
                caret,
                detail: None,
            };
            Some(Suggestions { from, items: vec![item("div", "div></div>", Some(4)), item("dialog", "dialog", None)] })
        }

        fn expand(&self, text: &str, caret: usize) -> Option<Expansion> {
            text[..caret].ends_with("ul>li").then(|| Expansion {
                range: caret - 5..caret,
                text: "<ul><li></li></ul>".to_owned(),
                caret: Some(8),
            })
        }

        fn colors(&self, text: &str) -> Vec<ColorSpan> {
            *self.colors.lock().unwrap() += 1;
            let mut spans = Vec::new();
            let mut at = 0;
            while at < text.len() {
                let rest = &text[at..];
                if let Some(hex) = rest.strip_prefix('#') {
                    let digits = hex.bytes().take_while(u8::is_ascii_hexdigit).count();
                    let value = |from: usize, len: usize| u8::from_str_radix(&hex[from..from + len], 16).unwrap();
                    let rgba = match digits {
                        3 => [value(0, 1) * 17, value(1, 1) * 17, value(2, 1) * 17, 255],
                        6 => [value(0, 2), value(2, 2), value(4, 2), 255],
                        _ => {
                            at += 1;
                            continue;
                        }
                    };
                    spans.push(ColorSpan { range: at..at + 1 + digits, rgba });
                    at += 1 + digits;
                } else if rest.starts_with("rgb(")
                    && let Some(close) = rest.find(')')
                {
                    let inner = &rest[4..close];
                    let (channels, alpha) = inner.split_once('/').unwrap_or((inner, "1"));
                    let mut parts = channels.split_whitespace().map(|part| part.parse::<u8>().unwrap());
                    let alpha = (alpha.trim().parse::<f64>().unwrap() * 255.0).round() as u8;
                    let rgba = [parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap(), alpha];
                    spans.push(ColorSpan { range: at..at + close + 1, rgba });
                    at += close + 1;
                } else {
                    at += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
            spans
        }

        fn reacts_to(&self, typed: char) -> bool {
            typed == '>'
        }

        fn on_type(&self, text: &str, caret: usize, typed: char) -> Option<Expansion> {
            (typed == '>' && text[..caret].ends_with("<b>")).then(|| Expansion {
                range: caret..caret,
                text: "</b>".to_owned(),
                caret: Some(0),
            })
        }
    }

    pub(crate) fn session(files: &[(&str, &str)]) -> (Registry, Session, tempfile::TempDir, Arc<WebPlugin>) {
        let folder = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(folder.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": folder.path() })).unwrap();
        let plugin = Arc::new(WebPlugin::default());
        session.state_mut::<PluginFeatures>().set(vec![plugin.clone()]);
        (registry, session, folder, plugin)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::testing::{WebPlugin, session};
    use super::*;
    use crate::Workbench;

    #[test]
    fn typing_lets_the_plugin_add_an_end_tag() {
        let (registry, mut session, folder, _plugin) = session(&[("page.web", "é\n"), ("notes.txt", "")]);
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("page.web") }))
            .unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 1 }] })).unwrap();
        for typed in ["<", "b", ">"] {
            registry.dispatch(&mut session, "editor.edit.type", json!({ "text": typed })).unwrap();
        }
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "é<b></b>\n");
        assert_eq!(document.selections().primary().head, 4, "the caret between the tags");
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        let undone = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(undone, "é<b>\n", "the end tag is its own undo step");
        registry
            .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }, { "head": 4 }] }))
            .unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": ">" })).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, ">é<b>>\n", "nothing is added with several carets");
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("notes.txt") }))
            .unwrap();
        for typed in ["<", "b", ">"] {
            registry.dispatch(&mut session, "editor.edit.type", json!({ "text": typed })).unwrap();
        }
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "<b>", "a file without a plug-in");
    }

    #[test]
    fn the_setting_turns_closing_tags_off() {
        let (registry, mut session, folder, _plugin) = session(&[("page.web", "")]);
        session.settings_mut().set(CLOSING_TAGS, false).unwrap();
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("page.web") }))
            .unwrap();
        for typed in ["<", "b", ">"] {
            registry.dispatch(&mut session, "editor.edit.type", json!({ "text": typed })).unwrap();
        }
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!((document.text(), document.selections().primary().head), ("<b>", 3));
    }

    #[test]
    fn the_plugin_folds_feed_editor_fold() {
        let page = "<html {\n  <body {\n    text\n  }\n}\n";
        let (registry, mut session, folder, plugin) = session(&[("page.web", page), ("other.txt", "a {\n b\n}\n")]);
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("page.web") }))
            .unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let line_end = |line: usize| page.match_indices('\n').nth(line).unwrap().0;
        assert_eq!(
            folds,
            json!([
                { "start": line_end(0), "end": line_end(4), "startLine": 0, "endLine": 4, "kind": "block", "folded": false, "stickyLine": 0 },
                { "start": line_end(1), "end": line_end(3), "startLine": 1, "endLine": 3, "kind": "block", "folded": false, "stickyLine": 1 },
            ])
        );
        let calls = *plugin.calls.lock().unwrap();
        let inner = line_end(1);
        let toggled = registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": inner })).unwrap();
        assert_eq!(toggled[1]["folded"], true);
        assert_eq!(*plugin.calls.lock().unwrap(), calls, "the answer is reused while the text is the same");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x\n" })).unwrap();
        let moved = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!((moved[1]["startLine"].as_u64(), moved[1]["folded"].as_bool()), (Some(2), Some(true)));
        registry.dispatch(&mut session, "editor.fold.collapseAll", json!({})).unwrap();
        let all = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert!(all.as_array().unwrap().iter().all(|fold| fold["folded"] == true));
        registry.dispatch(&mut session, "editor.fold.expandAll", json!({})).unwrap();
        let all = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert!(all.as_array().unwrap().iter().all(|fold| fold["folded"] == false));

        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("other.txt") }))
            .unwrap();
        let builtin = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(
            (builtin[0]["start"].as_u64(), builtin[0]["kind"].as_str()),
            (Some(3), Some("block")),
            "the bracket folds"
        );

        let (registry, mut session, folder, _plugin) = testing::session(&[("crlf.web", "a {\r\n b\r\n}\r\n")]);
        let crlf = folder.path().join("crlf.web");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": crlf })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        assert_eq!(
            (folds[0]["start"].as_u64(), folds[0]["end"].as_u64()),
            (Some(3), Some(10)),
            "before the line breaks"
        );
    }

    #[test]
    fn the_structure_comes_from_the_plugin() {
        let (registry, mut session, folder, plugin) = session(&[("page.web", "# head\nx\n# body\n")]);
        let path = folder.path().join("page.web");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let state = registry.dispatch(&mut session, "editor.structure.get", json!({})).unwrap();
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")));
        let names: Vec<&str> =
            state["symbols"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["head", "body"]);
        assert_eq!(state["symbols"][1]["kind"], "rule");
        assert_eq!(state["symbols"][1]["line"], 3);
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 9 }] })).unwrap();
        let again = registry.dispatch(&mut session, "editor.structure.get", json!({})).unwrap();
        assert_eq!(again["current"], json!([1]), "the caret in the second entry");
        assert_eq!(*plugin.outlines.lock().unwrap(), 1, "the outline is reused while the text is the same");
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
        registry.dispatch(&mut session, "editor.structure.get", json!({})).unwrap();
        assert_eq!(*plugin.outlines.lock().unwrap(), 2);
    }

    #[test]
    fn plugin_items_complete_without_a_server() {
        let (registry, mut session, folder, _plugin) = session(&[("page.web", "<p>\n<d\n")]);
        let path = folder.path().join("page.web");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 6 }] })).unwrap();
        let list = registry.dispatch(&mut session, "editor.completion.start", json!({})).unwrap();
        assert_eq!((list["open"].as_bool(), list["pending"].as_bool()), (Some(true), Some(false)));
        assert_eq!(list["prefix"], "d");
        let labels: Vec<&str> =
            list["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["div", "dialog"]);
        assert_eq!(list["items"][0]["kind"], "tag");
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "i" })).unwrap();
        let list = registry.dispatch(&mut session, "editor.completion.list", json!({})).unwrap();
        assert_eq!(list["items"][0]["label"], "div");
        let index = list["items"][0]["index"].clone();
        registry.dispatch(&mut session, "editor.completion.accept", json!({ "index": index })).unwrap();
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "<p>\n<div></div>\n");
        assert_eq!(document.selections().primary().head, 9, "the caret where the plug-in put it");
    }

    struct ModuleAssist(PathBuf);

    impl zen_modules::assist::LanguageAssist for ModuleAssist {
        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn complete(&self, context: &zen_modules::assist::AssistContext<'_>) -> Option<zen_syntax::Suggestions> {
            context.text[..context.offset].ends_with(" v").then(|| zen_syntax::Suggestions {
                from: context.offset - 1,
                items: vec![zen_syntax::Suggestion {
                    label: "v1.2.3".to_owned(),
                    kind: "value".to_owned(),
                    insert: "v1.2.3".to_owned(),
                    caret: None,
                    detail: None,
                }],
            })
        }

        fn declaration(&self, context: &zen_modules::assist::AssistContext<'_>) -> Option<Result<PathBuf, String>> {
            context.text.contains("example").then(|| Ok(self.0.clone()))
        }
    }

    #[test]
    fn a_language_assist_completes_and_goes_to_a_declaration_without_a_server() {
        let (registry, mut session, folder, _plugin) =
            session(&[("go.mod", "require example.com/a v\n"), ("other.mod", "module target\n")]);
        let path = folder.path().join("go.mod");
        let target = folder.path().join("other.mod");
        session.state_mut::<zen_modules::assist::LanguageAssists>().add(Arc::new(ModuleAssist(target.clone())));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 23 }] })).unwrap();
        let list = registry.dispatch(&mut session, "editor.completion.start", json!({})).unwrap();
        assert_eq!((list["open"].as_bool(), list["prefix"].as_str()), (Some(true), Some("v")));
        assert_eq!(list["items"][0]["label"], "v1.2.3");
        registry.dispatch(&mut session, "editor.completion.cancel", json!({})).unwrap();
        let found = registry.dispatch(&mut session, "navigate.declaration.goTo", json!({})).unwrap();
        assert_eq!(found["navigated"], true);
        let active = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(active.path(), target);
    }

    #[test]
    fn plugin_items_come_when_the_server_cannot_start() {
        let text = "package main\n\n<d\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.session.state_mut::<crate::language::Languages>().set_specs(vec![crate::language::ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(|_| Err("no gopls".into())),
            initialization_options: json!({}),
        }]);
        fixture
            .session
            .state_mut::<PluginFeatures>()
            .set(vec![Arc::new(WebPlugin { go: true, ..WebPlugin::default() })]);
        let path = fixture.path("main.go");
        fixture.dispatch("file.document.open", json!({ "path": path })).unwrap();
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": text.len() - 1 }] })).unwrap();
        let list = fixture.dispatch("editor.completion.start", json!({})).unwrap();
        assert_eq!(
            (list["pending"].as_bool(), list["items"].as_array().map(Vec::len)),
            (Some(false), Some(2)),
            "{list}"
        );
    }

    #[test]
    fn plugin_items_join_the_servers() {
        let text = "package main\n\nfunc main() { fmt. }\n";
        let mut fixture = crate::language::testing::fixture(&[("main.go", text)]);
        fixture.server.on("textDocument/completion", json!([{ "label": "Println", "kind": 3 }]));
        fixture
            .session
            .state_mut::<PluginFeatures>()
            .set(vec![Arc::new(WebPlugin { go: true, ..WebPlugin::default() })]);
        fixture.open("main.go");
        let caret = text.find(" }").unwrap();
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        let list = fixture.dispatch("editor.completion.start", json!({})).unwrap();
        let at_once = if list["pending"] == true { 2 } else { 3 };
        assert_eq!(list["items"].as_array().unwrap().len(), at_once, "the plug-in's items are there at once: {list}");
        let mut list = list;
        for _ in 0..1000 {
            list = fixture.dispatch("editor.completion.list", json!({})).unwrap();
            if list["pending"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let labels: Vec<&str> =
            list["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Println", "div", "dialog"]);
    }
}
