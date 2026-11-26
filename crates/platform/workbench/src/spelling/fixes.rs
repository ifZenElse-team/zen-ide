use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_project::inspect::{FixKind, InspectionFix};

use super::dictionaries::{self, DictionaryLevel};
use super::{FoundTypo, Spelling, TypoContext};
use crate::Workbench;
use crate::language::Problem;

pub const SAVE_TO: &str = "view.spelling.saveTo";

const SUGGESTIONS: usize = 10;

const MOST_SUGGESTIONS: usize = 50;

fn fix(id: String, title: String, kind: FixKind, action: &str, params: serde_json::Value) -> InspectionFix {
    InspectionFix {
        id,
        title,
        kind,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "action": action, "params": params }),
    }
}

pub(super) fn of(path: &Path, typo: &FoundTypo, single: bool) -> Vec<InspectionFix> {
    let action = if single { dictionaries::SaveWord::ID } else { SAVE_TO };
    vec![fix(
        format!("spelling.save:{}", typo.start),
        t!("workbench-spelling-save", word = typo.word.as_str()),
        FixKind::Action,
        action,
        json!({ "word": typo.word, "path": path }),
    )]
}

pub(crate) fn with_suggestions(session: &mut Session, path: &Path, problem: &mut Problem) {
    if !super::is_typo(problem) {
        return;
    }
    let params = TypoParams { path: Some(path.to_owned()), offset: Some(problem.start) };
    let Ok(at) = GetTypo.run(session, params) else { return };
    let start = at.typo.start;
    let mut fixes = Vec::new();
    if at.typo.context == TypoContext::Code {
        if let Some(rename) = at.name.as_ref().and_then(|name| name.renames.first()) {
            fixes.push(fix(
                format!("spelling.rename:{start}"),
                t!("workbench-spelling-rename-to", name = rename.as_str()),
                FixKind::Action,
                "refactor.rename.apply",
                json!({ "path": path, "offset": start, "newName": rename }),
            ));
        }
    } else if !at.suggestions.is_empty() {
        fixes.push(InspectionFix {
            id: format!("spelling.fix:{start}"),
            title: t!("workbench-spelling-fix-typo"),
            kind: FixKind::Heading,
            batch: None,
            label: None,
            inline: false,
            args: serde_json::Value::Null,
        });
        for (index, word) in at.suggestions.iter().enumerate() {
            fixes.push(fix(
                format!("spelling.change:{start}:{index}"),
                word.clone(),
                FixKind::Action,
                ChangeTypo::ID,
                json!({ "path": path, "offset": start, "to": word }),
            ));
        }
    }
    fixes.append(&mut problem.fixes);
    problem.fixes = fixes;
}

/// The parameters of `spelling.suggestions.list`.
#[derive(Deserialize, JsonSchema)]
pub struct SuggestionsParams {
    /// The misspelt word.
    pub word: String,
    /// How many to give at most (10 by default, at most 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct ListSuggestions;

impl Command for ListSuggestions {
    type Params = SuggestionsParams;
    type Output = Vec<String>;
    const ID: &'static str = "spelling.suggestions.list";
    const TITLE: &'static str = "Spelling Suggestions";
    const DESCRIPTION: &'static str = "The words closest to a misspelt word by edit distance (at most two letters inserted, \
         removed, replaced or swapped), the closest first, in the word's case, from the bundled English dictionary and \
         the user's dictionaries: what ⌥⏎ lists under Fix typo.";

    fn run(&self, session: &mut Session, params: SuggestionsParams) -> Result<Vec<String>, CommandError> {
        let limit = params.limit.unwrap_or(SUGGESTIONS).min(MOST_SUGGESTIONS);
        Ok(suggestions(session, &params.word, limit))
    }
}

fn suggestions(session: &mut Session, word: &str, limit: usize) -> Vec<String> {
    let (_, user) = dictionaries::refresh(session, false);
    let mut all: Vec<&zen_spell::Dictionary> = vec![zen_spell::english()];
    all.extend(user.iter().map(AsRef::as_ref));
    zen_spell::suggestions(&all, word, limit)
}

/// The parameters of `spelling.typo.get` and the place of `spelling.typo.change`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypoParams {
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// A character offset in the typo, from its start to its end; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// The name a misspelt word of code is part of.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypoName {
    /// Its first character (a character offset of the file).
    pub start: usize,
    /// The character after its last.
    pub end: usize,
    /// The name as written, e.g. `parseUrlHadnler`.
    pub name: String,
    /// The names it could be renamed to, one for each suggestion, e.g. `parseUrlHandler`.
    pub renames: Vec<String>,
}

/// What `spelling.typo.get` answers.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypoAt {
    /// The file.
    pub path: PathBuf,
    /// The typo.
    #[serde(flatten)]
    pub typo: FoundTypo,
    /// The words to change it to, the closest first.
    pub suggestions: Vec<String>,
    /// For a typo in a declared name, the whole name and what to rename it to (refactor.rename.apply).
    pub name: Option<TypoName>,
}

fn place(session: &Session, params: &TypoParams) -> Result<(PathBuf, usize), CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let path = params
        .path
        .as_ref()
        .map(|path| workbench.resolve(path))
        .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let offset = match params.offset {
        Some(offset) => offset,
        None => workbench
            .documents()
            .iter()
            .find(|document| document.path() == path)
            .map(|document| document.selections().primary().head)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    Ok((path, offset))
}

fn typo_at(session: &mut Session, params: &TypoParams) -> Result<(PathBuf, FoundTypo), CommandError> {
    let (path, offset) = place(session, params)?;
    let typos = super::current_typos(session, &path, super::WAIT)
        .ok_or_else(|| CommandError::new(t!("workbench-spelling-not-checked")))?;
    typos
        .into_iter()
        .find(|typo| typo.start <= offset && offset <= typo.end)
        .map(|typo| (path, typo))
        .ok_or_else(|| CommandError::new(t!("workbench-spelling-no-typo")))
}

fn name_around(text: &str, typo: &FoundTypo) -> Option<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let part = |c: &char| c.is_alphanumeric() || *c == '_';
    let start = typo.start - chars.get(..typo.start)?.iter().rev().take_while(|c| part(c)).count();
    let end = typo.end + chars.get(typo.end..)?.iter().take_while(|c| part(c)).count();
    Some((start, end, chars[start..end].iter().collect()))
}

pub struct GetTypo;

impl Command for GetTypo {
    type Params = TypoParams;
    type Output = TypoAt;
    const ID: &'static str = "spelling.typo.get";
    const TITLE: &'static str = "Typo at Caret";
    const DESCRIPTION: &'static str = "The typo at a place of an open file (the caret of the active one by default), once \
         the spelling check of its text is done: the word, where it is, what to change it to (the words ⌥⏎ lists under \
         Fix typo, spelling.typo.change) and, in a declared name, the whole name and what to rename it to (Typo: Rename \
         to '…' offers the first, refactor.rename.apply with newName). Fails when there is no typo there.";

    fn run(&self, session: &mut Session, params: TypoParams) -> Result<TypoAt, CommandError> {
        let (path, typo) = typo_at(session, &params)?;
        let suggestions = suggestions(session, &typo.word, SUGGESTIONS);
        let name = if typo.context == TypoContext::Code {
            session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
                .and_then(|document| name_around(document.text(), &typo))
                .map(|(start, end, name)| {
                    let chars: Vec<char> = name.chars().collect();
                    let (from, to) = (typo.start - start, typo.end - start);
                    let renames = suggestions
                        .iter()
                        .map(|suggestion| {
                            let head: String = chars[..from].iter().collect();
                            let tail: String = chars[to..].iter().collect();
                            format!("{head}{suggestion}{tail}")
                        })
                        .collect();
                    TypoName { start, end, name, renames }
                })
        } else {
            None
        };
        Ok(TypoAt { path, typo, suggestions, name })
    }
}

/// The parameters of `spelling.typo.change`.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeParams {
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// A character offset in the typo; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The word to put in its place.
    pub to: String,
}

/// What `spelling.typo.change` did.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypoChanged {
    /// The file.
    pub path: PathBuf,
    /// The word that was there.
    pub word: String,
    /// The first character replaced.
    pub start: usize,
    /// The word put in its place.
    pub to: String,
}

pub struct ChangeTypo;

impl Command for ChangeTypo {
    type Params = ChangeParams;
    type Output = TypoChanged;
    const ID: &'static str = "spelling.typo.change";
    const TITLE: &'static str = "Change Typo";
    const DESCRIPTION: &'static str = "Replaces the typo at a place of an open file (the caret of the active one by default) \
         with `to`, usually one of spelling.typo.get's suggestions: one change of the editor, unsaved, which ⌘Z undoes. \
         A typo in a declared name is better renamed everywhere with refactor.rename.apply.";

    fn run(&self, session: &mut Session, params: ChangeParams) -> Result<TypoChanged, CommandError> {
        let place = TypoParams { path: params.path, offset: params.offset };
        let (path, typo) = typo_at(session, &place)?;
        let to = params.to.trim().to_owned();
        if to.is_empty() || to.chars().any(char::is_whitespace) {
            return Err(CommandError::new(t!("workbench-spelling-bad-word", word = params.to.as_str())));
        }
        let document = session
            .state_mut::<Workbench>()
            .documents
            .iter_mut()
            .find(|document| document.path() == path)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        if let Some(reason) = document.read_only() {
            return Err(CommandError::new(reason.to_owned()));
        }
        if !document.replace_ranges(vec![(typo.start..typo.end, to.clone())]) {
            return Err(CommandError::new(t!("workbench-spelling-not-changed")));
        }
        Ok(TypoChanged { path, word: typo.word, start: typo.start, to })
    }
}

pub(crate) fn undo(session: &mut Session) -> Option<bool> {
    let document = session.state::<Workbench>()?.active_document()?;
    let (path, version) = (document.path().to_owned(), document.version());
    let spelling = session.state_mut::<Spelling>();
    spelling.saved.retain(|saved| saved.path != path || saved.version == version);
    let index = spelling.saved.iter().rposition(|saved| saved.path == path)?;
    let saved = spelling.saved.remove(index);
    let level: Option<DictionaryLevel> = Some(saved.level);
    Some(dictionaries::remove(session, &saved.word, level).is_ok_and(|files| !files.is_empty()))
}
