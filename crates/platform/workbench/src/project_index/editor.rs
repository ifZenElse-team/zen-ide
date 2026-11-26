use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_syntax::{IndexNode, ProjectIndex};

use super::{Built, Indexes, digest};
use crate::Workbench;
use crate::find_results::FindUsagesOptions;
use crate::find_results::provided::{self, Elsewhere, UsageFinder, UsageRow};
use crate::language::show_usages::UsageAccess;
use crate::search::SearchParams;

fn relative(session: &Session, path: &Path) -> Option<(PathBuf, String)> {
    let root = session.state::<Workbench>().and_then(Workbench::project)?.to_path_buf();
    let relative = path.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
    Some((root, relative))
}

struct Current {
    root: PathBuf,
    relative: String,
    text: String,
    built: Vec<(String, Arc<Built>)>,
}

fn current(session: &Session, path: &Path) -> Option<Current> {
    let (root, relative) = relative(session, path)?;
    let (text, _) = crate::run::file_text(session, path).ok()?;
    let hash = digest(&text);
    let built: Vec<(String, Arc<Built>)> = session
        .state::<Indexes>()?
        .all()
        .into_iter()
        .filter(|(_, built)| built.digests.get(&relative) == Some(&hash))
        .collect();
    (!built.is_empty()).then_some(Current { root, relative, text, built })
}

fn char_at(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

pub(crate) fn problems(session: &Session, path: &Path) -> Vec<crate::language::Problem> {
    let Some(Current { relative, text, built, .. }) = current(session, path) else { return Vec::new() };
    let mut found = Vec::new();
    for (plugin, built) in built {
        let index = super::visible(session, built.index.clone());
        for problem in index.problems.iter().filter(|problem| problem.path == relative) {
            let finding = zen_project::inspect::Finding {
                start: problem.start,
                end: problem.end,
                severity: if problem.severity == "error" {
                    zen_project::inspect::FindingSeverity::Error
                } else {
                    zen_project::inspect::FindingSeverity::Warning
                },
                message: problem.message.clone(),
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                source: plugin.clone(),
                code: "index".to_owned(),
                deprecated: false,
                unused: false,
            };
            found.push(crate::inspections::problem(&text, &finding));
        }
    }
    found
}

fn reference_at<'a>(index: &'a ProjectIndex, relative: &str, byte: usize) -> Option<&'a zen_syntax::IndexReference> {
    index
        .references
        .iter()
        .filter(|reference| reference.path == relative && reference.start <= byte && byte <= reference.end)
        .min_by_key(|reference| reference.end - reference.start)
}

fn node<'a>(index: &'a ProjectIndex, id: &str) -> Option<&'a IndexNode> {
    index.nodes.iter().find(|node| node.id == id)
}

fn declared_here(node: &IndexNode, relative: &str, byte: usize) -> bool {
    node.path == relative && node.start <= byte && byte <= node.end
}

pub(crate) fn go_to(session: &mut Session, path: &Path, offset: usize) -> Option<Result<(), String>> {
    let Current { root, relative, text, built } = current(session, path)?;
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    for (_, built) in built {
        let index = super::visible(session, built.index.clone());
        let Some(reference) = reference_at(&index, &relative, byte) else { continue };
        let Some(target) = node(&index, &reference.target) else {
            return Some(Err(t!("workbench-index-no-target", target = reference.target.as_str())));
        };
        if declared_here(target, &relative, byte) {
            return None;
        }
        let file = root.join(&target.path);
        if target.start == 0 && target.end == 0 {
            session.state_mut::<Indexes>().reveal = Some(file);
            return Some(Ok(()));
        }
        let (target_text, _) = crate::run::file_text(session, &file).ok()?;
        let params =
            crate::history::NavigateToParams { path: file, offset: char_at(&target_text, target.start), end: None };
        return Some(crate::history::NavigateTo.run(session, params).map(|_| ()).map_err(|error| error.to_string()));
    }
    None
}

impl Indexes {
    pub fn take_reveal(&mut self) -> Option<PathBuf> {
        self.reveal.take()
    }
}

struct IndexUsages {
    target: IndexNode,
    references: Vec<(String, Range<usize>)>,
}

impl UsageFinder for IndexUsages {
    fn title(&self) -> String {
        t!("workbench-index-usages", kind = self.target.kind.as_str(), name = self.target.name.as_str())
    }

    fn target(&self) -> String {
        self.target.name.clone()
    }

    fn scope(&self) -> String {
        t!("workbench-index-usages-scope")
    }

    fn request(&self) -> serde_json::Value {
        serde_json::json!({ "node": self.target.id })
    }

    fn search(&self) -> SearchParams {
        SearchParams {
            query: zen_text::search::Query { text: self.target.name.clone(), ..zen_text::search::Query::default() },
            mask: "*.yaml,*.yml,Kustomization".to_owned(),
            ..SearchParams::default()
        }
    }

    fn extra(&self) -> Vec<(PathBuf, String)> {
        Vec::new()
    }

    fn keep(&self, path: &Path, text: &str, _: Vec<Range<usize>>) -> Vec<(Range<usize>, Option<UsageAccess>)> {
        let name = path.to_string_lossy().replace('\\', "/");
        self.references
            .iter()
            .filter(|(file, _)| name.ends_with(&format!("/{file}")) || name == *file)
            .filter(|(_, range)| text.is_char_boundary(range.start) && text.is_char_boundary(range.end.min(text.len())))
            .map(|(_, range)| (char_at(text, range.start)..char_at(text, range.end), None))
            .collect()
    }

    fn elsewhere(&self, _cancel: &AtomicBool) -> Vec<Elsewhere> {
        Vec::new()
    }
}

pub(crate) struct Found {
    pub label: String,
    pub total: usize,
    pub tab: u64,
    pub usages: Vec<UsageRow>,
}

pub(crate) fn find_at(
    session: &mut Session,
    path: &Path,
    offset: usize,
    new_tab: Option<bool>,
    wait_ms: Option<u64>,
) -> Option<Result<Found, CommandError>> {
    let Current { relative, text, built, .. } = current(session, path)?;
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    let (target, references) = built.iter().find_map(|(_, built)| {
        let index = super::visible(session, built.index.clone());
        let reference = reference_at(&index, &relative, byte)?;
        let target = node(&index, &reference.target)?.clone();
        let references: Vec<(String, Range<usize>)> = index
            .references
            .iter()
            .filter(|other| other.target == target.id && !declared_here(&target, &other.path, other.start))
            .map(|other| (other.path.clone(), other.start..other.end))
            .collect();
        Some((target, references))
    })?;
    let finder = IndexUsages { target, references };
    let label = finder.title();
    let new_tab = new_tab.unwrap_or_else(|| FindUsagesOptions::of(session).new_tab);
    let tab = match provided::open_tab(session, Arc::new(finder), None, new_tab) {
        Ok(tab) => tab,
        Err(error) => return Some(Err(error)),
    };
    provided::wait(session, tab, Duration::from_millis(wait_ms.unwrap_or(0).min(30_000)));
    let (usages, total, _) = provided::rows(session, tab, 200).unwrap_or_default();
    Some(Ok(Found { label, total, tab, usages }))
}

const LOOK_BACK: usize = 256;

fn partial_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-')
}

type AllBuilt = Vec<(String, Arc<Built>)>;

fn completions_for<'a>(
    built: &'a [(String, Arc<Built>)],
    relative: &'a str,
) -> impl Iterator<Item = &'a zen_syntax::IndexCompletion> + 'a {
    built
        .iter()
        .flat_map(|(_, built)| built.index.completions.iter())
        .filter(move |completion| completion.files.iter().any(|file| file == relative))
}

fn indexed(session: &Session, path: &Path) -> Option<(String, AllBuilt)> {
    let (_, relative) = relative(session, path)?;
    let built = session.state::<Indexes>()?.all();
    Some((relative, built))
}

pub(crate) fn completes_in(session: &Session, path: &Path) -> bool {
    indexed(session, path).is_some_and(|(relative, built)| completions_for(&built, &relative).next().is_some())
}

pub(crate) fn complete(session: &Session, path: &Path, text: &str, caret: usize) -> Option<zen_syntax::Suggestions> {
    let (relative, built) = indexed(session, path)?;
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let mut window = caret.saturating_sub(LOOK_BACK);
    while !text.is_char_boundary(window) {
        window += 1;
    }
    let before = &text[window..caret];
    let mut from = None;
    let mut items = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for completion in completions_for(&built, &relative) {
        let Some(at) = before.rfind(completion.after.as_str()) else { continue };
        let partial = &before[at + completion.after.len()..];
        if !partial.chars().all(partial_char) {
            continue;
        }
        let start = caret - partial.len();
        if from.is_some_and(|from| from != start) {
            continue;
        }
        from = Some(start);
        for item in completion.items.iter().filter(|item| item.label.starts_with(partial)) {
            if items.len() >= zen_syntax::INDEX_COMPLETION_ITEMS {
                break;
            }
            if seen.insert(item.label.clone()) {
                items.push(zen_syntax::Suggestion {
                    label: item.label.clone(),
                    kind: "property".to_owned(),
                    insert: item.label.clone(),
                    caret: None,
                    detail: (!item.detail.is_empty()).then(|| item.detail.clone()),
                });
            }
        }
    }
    Some(zen_syntax::Suggestions { from: from?, items })
}
