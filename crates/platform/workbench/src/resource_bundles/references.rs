use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use zen_core::{Command, Session};
use zen_i18n::t;
use zen_syntax::{KeyReferences, Suggestion, Suggestions, SymbolKind, TextFeatures};

use super::{format, split_name};
use crate::Workbench;
use crate::plugins::PluginFeatures;

const MAX_ITEMS: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Literal {
    pub call: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    Key,
    Bundle,
}

#[derive(Clone, Debug)]
struct KeyAt {
    key: String,
    start: usize,
}

#[derive(Default)]
pub(super) struct KeyFiles {
    read: HashMap<PathBuf, (Option<SystemTime>, u64, Vec<KeyAt>)>,
}

pub(super) fn literal_at(text: &str, caret: usize) -> Option<Literal> {
    let caret = caret.min(text.len());
    let line_start = text[..caret].rfind('\n').map_or(0, |at| at + 1);
    let line_end = text[caret..].find('\n').map_or(text.len(), |at| caret + at);
    let line = &text.as_bytes()[line_start..line_end];
    let mut at = 0;
    while at < line.len() {
        match line[at] {
            b'/' if line.get(at + 1) == Some(&b'/') => return None,
            b'\'' => {
                at += 1;
                while at < line.len() && line[at] != b'\'' {
                    at += if line[at] == b'\\' { 2 } else { 1 };
                }
                at += 1;
            }
            b'"' => {
                let open = at;
                at += 1;
                while at < line.len() && line[at] != b'"' {
                    at += if line[at] == b'\\' { 2 } else { 1 };
                }
                let close = at.min(line.len());
                let (start, end) = (line_start + open + 1, line_start + close);
                if start <= caret && caret <= end {
                    let call = call_before(&text[line_start..line_start + open])?;
                    return Some(Literal { call, start, end });
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
    None
}

fn call_before(before: &str) -> Option<String> {
    let before = before.trim_end().strip_suffix('(')?.trim_end();
    let name: String = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
    let name: String = name.chars().rev().collect();
    (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit())).then_some(name)
}

pub(super) fn project_files(session: &Session, wanted: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    session
        .state::<Workbench>()
        .and_then(Workbench::file_index)
        .map(|index| index.file_list().into_iter().map(|(path, _)| path).filter(|path| wanted(path)).collect())
        .unwrap_or_default()
}

fn plugins(session: &Session) -> Vec<Arc<dyn TextFeatures>> {
    session.state::<PluginFeatures>().map(PluginFeatures::all).unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn declarations(session: &Session, path: &Path) -> Vec<(Arc<dyn TextFeatures>, KeyReferences)> {
    let name = file_name(path);
    plugins(session)
        .into_iter()
        .flat_map(|plugin| plugin.key_references().into_iter().map(move |declared| (Arc::clone(&plugin), declared)))
        .filter(|(_, declared)| {
            declared.files.iter().any(|pattern| zen_vfs::file_types::glob_matches(pattern, &name, true))
        })
        .collect()
}

fn wanted(declarations: &[(Arc<dyn TextFeatures>, KeyReferences)], call: &str) -> Option<Wanted> {
    if declarations.iter().any(|(_, declared)| declared.keys.iter().any(|name| name == call)) {
        Some(Wanted::Key)
    } else if declarations.iter().any(|(_, declared)| declared.bundles.iter().any(|name| name == call)) {
        Some(Wanted::Bundle)
    } else {
        None
    }
}

fn key_files(
    session: &Session,
    declarations: &[(Arc<dyn TextFeatures>, KeyReferences)],
) -> Vec<(Arc<dyn TextFeatures>, PathBuf)> {
    let mut found: Vec<(Arc<dyn TextFeatures>, PathBuf)> = Vec::new();
    for (plugin, _) in declarations {
        for path in project_files(session, |path| plugin.handles(&file_name(path))) {
            if !found.iter().any(|(_, known)| *known == path) {
                found.push((Arc::clone(plugin), path));
            }
        }
    }
    found
}

fn keys_of(session: &mut Session, plugin: &dyn TextFeatures, path: &Path) -> Vec<KeyAt> {
    let open = session.state::<Workbench>().and_then(|workbench| {
        workbench.documents().iter().find(|document| document.path() == path).map(|document| document.text().to_owned())
    });
    let stamp = || std::fs::metadata(path).ok().map(|meta| (meta.modified().ok(), meta.len()));
    if open.is_none()
        && let Some((modified, len)) = stamp()
        && let Some((seen, size, keys)) = session.state::<KeyFiles>().and_then(|files| files.read.get(path))
        && *seen == modified
        && *size == len
    {
        return keys.clone();
    }
    let Some(text) = open.clone().or_else(|| zen_vfs::read_text(path).ok()) else { return Vec::new() };
    let keys: Vec<KeyAt> = plugin
        .outline(&text)
        .into_iter()
        .filter(|item| matches!(item.kind, SymbolKind::Property | SymbolKind::Key))
        .map(|item| KeyAt { key: item.name, start: item.selection_start })
        .collect();
    if open.is_none()
        && let Some((modified, len)) = stamp()
    {
        session.state_mut::<KeyFiles>().read.insert(path.to_owned(), (modified, len, keys.clone()));
    }
    keys
}

pub(super) fn bundle_name(path: &Path, project: Option<&Path>) -> Option<String> {
    let name = file_name(path);
    let (base, _) = split_name(&name)?;
    let folder = path.parent()?;
    let root = folder
        .ancestors()
        .find(|ancestor| {
            ancestor.file_name().is_some_and(|name| name == "resources" || name == "java" || name == "kotlin")
        })
        .or(project.filter(|project| folder.starts_with(project)))?;
    let relative = folder.strip_prefix(root).ok()?;
    let mut parts: Vec<String> =
        relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    parts.push(base.to_owned());
    Some(parts.join("."))
}

fn named_bundles(text: &str, declarations: &[(Arc<dyn TextFeatures>, KeyReferences)]) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices('"') {
        if let Some(literal) = literal_at(text, at + 1)
            && literal.start == at + 1
            && wanted(declarations, &literal.call) == Some(Wanted::Bundle)
        {
            let name = text[literal.start..literal.end].replace('/', ".");
            let name = name.strip_suffix(".properties").unwrap_or(&name).to_owned();
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    found
}

struct Found {
    key: String,
    bundle: String,
    locale: String,
    path: PathBuf,
    start: usize,
}

fn all_keys(session: &mut Session, declarations: &[(Arc<dyn TextFeatures>, KeyReferences)], text: &str) -> Vec<Found> {
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let named = named_bundles(text, declarations);
    let mut found = Vec::new();
    for (plugin, path) in key_files(session, declarations) {
        let Some(bundle) = bundle_name(&path, project.as_deref()) else { continue };
        if !named.is_empty() && !named.contains(&bundle) {
            continue;
        }
        let locale = split_name(&file_name(&path)).map(|(_, locale)| locale.to_owned()).unwrap_or_default();
        for key in keys_of(session, plugin.as_ref(), &path) {
            found.push(Found {
                key: key.key,
                bundle: bundle.clone(),
                locale: locale.clone(),
                path: path.clone(),
                start: key.start,
            });
        }
    }
    found.sort_by_key(|found| !found.locale.is_empty());
    found
}

fn java_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn completes_in(session: &Session, path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == super::EXTENSION) || !declarations(session, path).is_empty()
}

pub(crate) fn complete(session: &mut Session, path: &Path, text: &str, caret: usize) -> Option<Suggestions> {
    if let Some(bundle) =
        path.extension().filter(|extension| *extension == super::EXTENSION).and_then(|_| super::bundle_of(path))
    {
        return complete_in_bundle(session, &bundle, path, text, caret);
    }
    let declarations = declarations(session, path);
    if declarations.is_empty() {
        return None;
    }
    let literal = literal_at(text, caret)?;
    let wanted = wanted(&declarations, &literal.call)?;
    let mut items: BTreeMap<String, Suggestion> = BTreeMap::new();
    for found in all_keys(session, &declarations, text) {
        let label = match wanted {
            Wanted::Key => found.key,
            Wanted::Bundle => found.bundle.clone(),
        };
        items.entry(label.clone()).or_insert_with(|| Suggestion {
            insert: java_escaped(&label),
            kind: if wanted == Wanted::Key { "property" } else { "module" }.to_owned(),
            detail: Some(if wanted == Wanted::Key { found.bundle } else { file_name(&found.path) }),
            caret: None,
            label,
        });
    }
    Some(Suggestions { from: literal.start, items: items.into_values().take(MAX_ITEMS).collect() })
}

fn complete_in_bundle(
    session: &mut Session,
    bundle: &super::Bundle,
    path: &Path,
    text: &str,
    caret: usize,
) -> Option<Suggestions> {
    let caret = caret.min(text.len());
    let line_start = text[..caret].rfind(['\n', '\r']).map_or(0, |at| at + 1);
    if line_start > 0 && text[..line_start - 1].ends_with('\\') {
        return None;
    }
    let typed = text[line_start..caret].trim_start_matches([' ', '\t', '\x0c']);
    if typed.starts_with(['#', '!']) || typed.contains(['=', ':', ' ', '\t', '\\']) {
        return None;
    }
    let present: Vec<String> = format::read(text).into_iter().map(|entry| entry.key).collect();
    let mut items: BTreeMap<String, Suggestion> = BTreeMap::new();
    for file in bundle.files.iter().filter(|file| file.path != path) {
        let Ok(other) = crate::language::file_text(session.state::<Workbench>()?, &file.path) else { continue };
        for entry in format::read(&other).into_iter().filter(|entry| !present.contains(&entry.key)) {
            items.entry(entry.key.clone()).or_insert_with(|| Suggestion {
                insert: format::escape_key(&entry.key),
                kind: "property".to_owned(),
                detail: Some(entry.value.chars().take(60).collect()),
                caret: None,
                label: entry.key,
            });
        }
    }
    (!items.is_empty())
        .then(|| Suggestions { from: caret - typed.len(), items: items.into_values().take(MAX_ITEMS).collect() })
}

pub(crate) fn go_to(session: &mut Session, path: &Path, offset: usize) -> Option<Result<(), String>> {
    let declarations = declarations(session, path);
    if declarations.is_empty() {
        return None;
    }
    let (text, _) = crate::run::file_text(session, path).ok()?;
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    let literal = literal_at(&text, byte)?;
    let wanted = wanted(&declarations, &literal.call)?;
    let name = text[literal.start..literal.end].to_owned();
    if wanted == Wanted::Bundle {
        let bundle = name.replace('/', ".");
        let bundle = bundle.strip_suffix(".properties").unwrap_or(&bundle).to_owned();
        let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let target = key_files(session, &declarations)
            .into_iter()
            .map(|(_, path)| path)
            .filter(|path| bundle_name(path, project.as_deref()).as_deref() == Some(bundle.as_str()))
            .min_by_key(|path| split_name(&file_name(path)).is_some_and(|(_, locale)| !locale.is_empty()));
        return Some(match target {
            Some(path) => open(session, &path, 0, None),
            None => Err(t!("workbench-bundle-not-found", bundle = bundle.as_str())),
        });
    }
    let keys = all_keys(session, &declarations, &text);
    Some(match keys.iter().find(|found| found.key == name) {
        Some(found) => {
            let (path, start, end) = (found.path.clone(), found.start, found.start + found.key.chars().count());
            open(session, &path, start, Some(end))
        }
        None => Err(t!("workbench-bundle-no-key", key = name.as_str())),
    })
}

fn open(session: &mut Session, path: &Path, offset: usize, end: Option<usize>) -> Result<(), String> {
    let params = crate::history::NavigateToParams { path: path.to_owned(), offset, end };
    crate::history::NavigateTo.run(session, params).map(|_| ()).map_err(|error| error.to_string())
}
