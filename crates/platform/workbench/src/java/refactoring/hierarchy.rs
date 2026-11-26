use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{CommandError, Session};
use zen_i18n::t;
use zen_lsp::LspError;

use super::{
    Job, Planned, Progress, apply_planned, conflict_labels, drop_job, failed_form, failed_plan, field, is_java, lookup,
    pending_plan, plan_of, served, started, text_of, with_state,
};
use crate::Workbench;
use crate::java::members::{
    self, ClassDecl, Member, MemberKind, block, class_at, classes, indent_of, insertion, member_indent, needed_imports,
    reindent, removal, words,
};
use crate::language::places::TextCache;
use crate::language::refactor::{is_identifier, relative};
use crate::language::refactoring_dialog::{
    ColumnKind, FieldKind, FieldOption, RefactoringApplied, RefactoringConflict, RefactoringField, RefactoringForm,
    RefactoringPlan, RefactoringProvider, TableColumn, Values,
};
use crate::language::{LookupParams, byte_offset, char_offset, request_at, request_in, with_languages};

pub const PULL_UP: &str = "javaPullUp";
pub const PUSH_DOWN: &str = "javaPushDown";
pub const EXTRACT_SUPERCLASS: &str = "javaExtractSuperclass";
pub const EXTRACT_INTERFACE: &str = "javaExtractInterface";

pub(super) enum Ask {
    At { path: PathBuf, offset: usize, method: &'static str, extra: Value },
    With { path: PathBuf, method: String, params: Value },
    Code { path: PathBuf, offset: usize, method: &'static str, extra: Value },
    Refactor { path: PathBuf, offset: usize, extra: Value },
}

impl Ask {
    fn path(&self) -> &Path {
        match self {
            Self::At { path, .. } | Self::With { path, .. } | Self::Code { path, .. } | Self::Refactor { path, .. } => {
                path
            }
        }
    }
}

pub(super) fn ask_all(
    session: &mut Session,
    slot: &'static str,
    key: String,
    asks: Vec<Ask>,
    wait: Duration,
) -> Result<Progress<Vec<Value>>, CommandError> {
    with_state(session, |state, languages, workbench| {
        languages.sync(workbench);
        let versions: Vec<Option<i32>> = asks.iter().map(|ask| languages.server_version(ask.path())).collect();
        let key = format!("{key}\n{versions:?}");
        let reuse = state.asked.get(slot).is_some_and(|job| {
            job.key == key && !job.lookups.iter().any(|lookup| matches!(lookup.answer, Some(Err(_))))
        });
        if !reuse {
            drop_job(languages, state.asked.remove(slot));
            let mut lookups = Vec::new();
            for ask in asks {
                let (path, offset, method) = match &ask {
                    Ask::At { path, offset, method, .. } | Ask::Code { path, offset, method, .. } => {
                        (path.clone(), *offset, (*method).to_owned())
                    }
                    Ask::With { path, method, .. } => (path.clone(), 0, method.clone()),
                    Ask::Refactor { path, offset, .. } => (path.clone(), *offset, "java/getRefactorEdit".to_owned()),
                };
                let request = match ask {
                    Ask::At { path, offset, method, extra } => {
                        request_at(languages, workbench, &path, offset, method, extra)
                    }
                    Ask::With { path, method, params } => {
                        request_in(languages, workbench, &path, &method, move |_, _| params)
                    }
                    Ask::Code { path, offset, method, extra } => {
                        request_in(languages, workbench, &path, method, move |uri, text| {
                            let mut params = super::code_action_params(text, uri, offset);
                            if let (Some(params), Value::Object(extra)) = (params.as_object_mut(), extra) {
                                params.extend(extra);
                            }
                            params
                        })
                    }
                    Ask::Refactor { path, offset, extra } => {
                        request_in(languages, workbench, &path, "java/getRefactorEdit", move |uri, text| {
                            let mut params = json!({ "context": super::code_action_params(text, uri, offset) });
                            if let (Some(params), Value::Object(extra)) = (params.as_object_mut(), extra) {
                                params.extend(extra);
                            }
                            params
                        })
                    }
                };
                match request {
                    Ok(request) => lookups.push(lookup(request, &method, &path, offset)),
                    Err(message) => {
                        for mut lookup in lookups {
                            if let Some(request) = lookup.request.take()
                                && let Some(client) = languages.client_for(&lookup.path)
                            {
                                client.cancel(&request.pending);
                            }
                        }
                        started(languages.lookup_error(&path, message))?;
                        return Ok(Progress::Pending);
                    }
                }
            }
            state.asked.insert(slot, Job::new(key, lookups));
        }
        let Some(job) = state.asked.get_mut(slot) else { return Ok(Progress::Pending) };
        if !job.poll(Instant::now() + wait) {
            return Ok(Progress::Pending);
        }
        let mut answers = Vec::new();
        for index in 0..job.lookups.len() {
            match job.answer(index) {
                Ok(answer) => answers.push(answer),
                Err(error) => return Ok(Progress::Failed(error)),
            }
        }
        Ok(Progress::Ready(answers))
    })
}

pub(super) fn forget(session: &mut Session, slots: &[&'static str]) {
    with_state(session, |state, languages, _| {
        for slot in slots {
            drop_job(languages, state.asked.remove(*slot));
        }
    });
}

pub(super) struct Source {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) at: usize,
}

impl Source {
    pub(super) fn find(session: &mut Session, context: &Value, not_here: &str) -> Result<Self, String> {
        let params: LookupParams =
            crate::language::refactoring_dialog::context(context).map_err(|error| error.message)?;
        let workbench = session.state::<Workbench>().ok_or_else(|| t!("workbench-error-no-document"))?;
        let (path, offset) = params.place(workbench).map_err(|error| error.message)?;
        if !is_java(&path) {
            return Err(t!(not_here));
        }
        let text = with_languages(session, |languages, workbench| {
            languages.sync(workbench);
            text_of(languages, workbench, &path)
        })
        .ok_or_else(|| t!("workbench-nav-no-file", path = path.display().to_string()))?;
        let at = byte_offset(&text, offset);
        Ok(Self { path, text, at })
    }

    pub(super) fn class(&self, not_here: &str) -> Result<ClassDecl, String> {
        class_at(&self.text, self.at).ok_or_else(|| t!(not_here))
    }

    pub(super) fn char_at(&self, byte: usize) -> usize {
        char_offset(&self.text, byte)
    }
}

pub(super) fn accepts_java(session: &mut Session, context: &Value) -> bool {
    match Source::find(session, context, "workbench-java-hierarchy-not-here") {
        Ok(source) => class_at(&source.text, source.at).is_some() && served(session, &source.path),
        Err(_) => false,
    }
}

type TextEdit = (usize, usize, String);

#[derive(Default)]
pub(super) struct Edits {
    files: Vec<(PathBuf, String, Vec<TextEdit>)>,
    created: Vec<(PathBuf, String)>,
    raw: Option<Value>,
    pub(super) conflicts: Vec<(PathBuf, Value, String)>,
}

impl Edits {
    pub(super) fn edit(&mut self, path: &Path, text: &str, from: usize, to: usize, new: String) {
        match self.files.iter_mut().find(|(known, _, _)| known == path) {
            Some((_, _, edits)) => edits.push((from, to, new)),
            None => self.files.push((path.to_owned(), text.to_owned(), vec![(from, to, new)])),
        }
    }

    pub(super) fn create(&mut self, path: PathBuf, content: String) {
        self.created.push((path, content));
    }

    pub(super) fn conflict(&mut self, path: &Path, text: &str, from: usize, to: usize, message: String) {
        let range = json!({
            "start": zen_lsp::position::position(text, from),
            "end": zen_lsp::position::position(text, to.max(from))
        });
        self.conflicts.push((path.to_owned(), range, message));
    }

    pub(super) fn conflict_at(&mut self, usage: &Usage, message: String) {
        self.conflicts.push((usage.path.clone(), usage.range.clone(), message));
    }

    pub(super) fn workspace_edit(&self) -> Value {
        if let Some(raw) = &self.raw {
            return raw.clone();
        }
        let mut changes = Vec::new();
        for (path, content) in &self.created {
            let uri = zen_lsp::position::file_uri(path);
            changes.push(
                json!({ "kind": "create", "uri": uri, "options": { "overwrite": false, "ignoreIfExists": false } }),
            );
            changes.push(json!({
                "textDocument": { "uri": uri, "version": null },
                "edits": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "newText": content }]
            }));
        }
        for (path, text, edits) in &self.files {
            let mut sorted = edits.clone();
            sorted.sort_by_key(|(from, to, _)| (*from, *to));
            let mut merged: Vec<TextEdit> = Vec::new();
            for (from, to, new) in sorted {
                match merged.last_mut() {
                    Some(last) if from < last.1 || (from == last.1 && from < to && last.0 < last.1) => {
                        last.1 = last.1.max(to);
                        last.2.push_str(&new);
                    }
                    _ => merged.push((from, to, new)),
                }
            }
            let edits: Vec<Value> = merged
                .iter()
                .map(|(from, to, new)| {
                    json!({
                        "range": {
                            "start": zen_lsp::position::position(text, *from),
                            "end": zen_lsp::position::position(text, *to)
                        },
                        "newText": new
                    })
                })
                .collect();
            changes.push(json!({
                "textDocument": { "uri": zen_lsp::position::file_uri(path), "version": null },
                "edits": edits
            }));
        }
        json!({ "documentChanges": changes })
    }
}

pub(super) enum Outcome {
    Pending,
    Failed(String),
    Ready(Edits),
}

impl<T> From<Progress<T>> for Outcome {
    fn from(progress: Progress<T>) -> Self {
        match progress {
            Progress::Pending => Self::Pending,
            Progress::Failed(error) => Self::Failed(error),
            Progress::Ready(_) => Self::Pending,
        }
    }
}

fn planned(session: &mut Session, edits: &Edits) -> Result<Planned, String> {
    with_languages(session, |languages, workbench| {
        let steps =
            crate::language::edits::parse(&edits.workspace_edit(), languages).map_err(|error| error.message())?;
        let mut cache = TextCache::new();
        let conflicts = edits
            .conflicts
            .iter()
            .map(|(path, range, message)| {
                let shown = relative(workbench.project(), path);
                RefactoringConflict {
                    message: message.clone(),
                    place: cache.place(languages, path, range),
                    relative: Some(shown),
                }
            })
            .collect();
        Ok(Planned { steps, conflicts, renamed: None })
    })
}

pub(super) fn plan_outcome(session: &mut Session, outcome: Outcome) -> Result<RefactoringPlan, CommandError> {
    match outcome {
        Outcome::Pending => Ok(pending_plan()),
        Outcome::Failed(error) => Ok(failed_plan(error)),
        Outcome::Ready(edits) => match planned(session, &edits) {
            Ok(planned) => Ok(with_languages(session, |_, workbench| plan_of(&planned, workbench))),
            Err(error) => Ok(failed_plan(error)),
        },
    }
}

pub(super) fn apply_outcome(
    session: &mut Session,
    outcome: Outcome,
    action: &str,
) -> Result<RefactoringApplied, CommandError> {
    match outcome {
        Outcome::Pending => Err(CommandError::new(LspError::Timeout.to_string())),
        Outcome::Failed(error) => Err(CommandError::new(error)),
        Outcome::Ready(edits) => {
            let planned = planned(session, &edits).map_err(CommandError::new)?;
            apply_planned(session, planned, action)
        }
    }
}

pub(super) fn pending_form() -> RefactoringForm {
    RefactoringForm { pending: true, ..RefactoringForm::default() }
}

pub(super) fn choice(id: &str, label: String, value: &str, options: Vec<(String, String)>) -> RefactoringField {
    let mut choice = field(id, label, FieldKind::Choice, json!(value));
    choice.options = options.into_iter().map(|(value, label)| FieldOption { value, label }).collect();
    choice
}

pub(super) fn table(id: &str, label: String, rows: Vec<Value>, columns: Vec<TableColumn>) -> RefactoringField {
    let mut table = field(id, label, FieldKind::Table, json!(rows));
    table.columns = columns;
    table.fixed = true;
    table
}

pub(super) fn text_field(id: &str, label: String, value: &str) -> RefactoringField {
    field(id, label, FieldKind::Text, json!(value))
}

pub(super) fn given(values: &Values, key: &str, default: &str) -> String {
    values.get(key).and_then(Value::as_str).unwrap_or(default).trim().to_owned()
}

pub(super) struct Chosen {
    pub(super) signature: String,
    pub(super) abstract_: bool,
}

pub(super) fn chosen(values: &Values, key: &str, form_rows: &[Value]) -> Vec<Chosen> {
    let rows = values.get(key).and_then(Value::as_array).map_or(form_rows, Vec::as_slice);
    rows.iter()
        .filter(|row| row.get("selected").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|row| {
            Some(Chosen {
                signature: row.get("signature")?.as_str()?.to_owned(),
                abstract_: row.get("abstract").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

fn movable(member: &Member) -> bool {
    matches!(member.kind, MemberKind::Field | MemberKind::Method)
}

fn can_be_abstract(member: &Member) -> bool {
    member.kind == MemberKind::Method && member.body.is_some() && !member.has("static") && !member.has("private")
}

pub(super) fn member_rows(class: &ClassDecl, at: usize, abstract_column: bool, all: bool) -> Vec<Value> {
    let caret = class.member_at(at).map(Member::signature);
    class
        .members
        .iter()
        .filter(|member| movable(member))
        .map(|member| {
            let signature = member.signature();
            let selected = all || caret.as_deref() == Some(signature.as_str());
            let mut row = json!({ "selected": selected, "member": member.label(), "signature": signature });
            if abstract_column && can_be_abstract(member) {
                row["abstract"] = json!(false);
            }
            row
        })
        .collect()
}

pub(super) fn member_columns(abstract_label: Option<String>) -> Vec<TableColumn> {
    let mut columns = vec![
        TableColumn::new("selected", String::new(), ColumnKind::Checkbox),
        TableColumn::new("member", t!("workbench-java-hierarchy-member"), ColumnKind::Label),
    ];
    if let Some(label) = abstract_label {
        columns.push(TableColumn::new("abstract", label, ColumnKind::Checkbox));
    }
    columns
}

fn picked<'a>(class: &'a ClassDecl, chosen: &[Chosen]) -> Vec<(&'a Member, bool)> {
    class
        .members
        .iter()
        .filter_map(|member| {
            let signature = member.signature();
            chosen
                .iter()
                .find(|chosen| chosen.signature == signature && movable(member))
                .map(|chosen| (member, chosen.abstract_ && can_be_abstract(member)))
        })
        .collect()
}

fn declaration(text: &str, member: &Member, interface: bool) -> String {
    let (_, close) = member.parens.unwrap_or((member.end, member.end));
    let end = member.body.map_or(close + 1, |(open, _)| open);
    let head = text[member.head..end].trim_end();
    let kept: Vec<&str> = member
        .modifiers
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| !matches!(*name, "abstract" | "final" | "synchronized" | "native" | "static" | "default"))
        .filter(|name| !(interface && matches!(*name, "public" | "protected")))
        .collect();
    let mut out = kept.join(" ");
    if !interface {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str("abstract");
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(head);
    out.push(';');
    out
}

fn block_text(text: &str, member: &Member, to_indent: &str, widen: bool) -> String {
    let (from, to) = block(text, member);
    let mut slice = text[from..to].to_owned();
    if widen && let Some(("private", at)) = member.visibility() {
        let local = at - from;
        slice.replace_range(local..local + "private".len(), "protected");
    }
    reindent(&slice, &indent_of(text, member.first), to_indent)
}

fn uses_outside(text: &str, class: &ClassDecl, moving: &[&Member]) -> Vec<(String, usize)> {
    let staying: Vec<&Member> = class.members.iter().filter(|member| !moving.contains(member)).collect();
    let mut found = Vec::new();
    for member in moving {
        for word in words(text, member.first, member.end) {
            if word.qualified {
                continue;
            }
            if staying.iter().any(|other| {
                matches!(other.kind, MemberKind::Field | MemberKind::Method) && other.names.contains(&word.text)
            }) {
                found.push((word.text, word.at));
            }
        }
    }
    found
}

fn used_by_staying(text: &str, class: &ClassDecl, moving: &[&Member], name: &str) -> bool {
    class
        .members
        .iter()
        .filter(|member| !moving.contains(member))
        .any(|member| words(text, member.first, member.end).iter().any(|word| word.text == name))
}

pub(super) struct PullUp<'a> {
    pub(super) source: &'a Path,
    pub(super) text: &'a str,
    pub(super) class: &'a ClassDecl,
    pub(super) destination: &'a Path,
    pub(super) destination_text: &'a str,
    pub(super) target: &'a ClassDecl,
    pub(super) chosen: &'a [Chosen],
}

pub(super) fn pull_up(pull: &PullUp<'_>, edits: &mut Edits) -> Result<(), String> {
    let PullUp { source, text, class, destination, destination_text, target, chosen } = *pull;
    let picked = picked(class, chosen);
    if picked.is_empty() {
        return Err(t!("workbench-java-hierarchy-choose-members"));
    }
    let same_file = source == destination;
    let moving: Vec<&Member> = picked.iter().filter(|(_, abstract_)| !abstract_).map(|(member, _)| *member).collect();
    let indent = member_indent(destination_text, target);
    let mut inserted = String::new();
    let mut made_abstract = false;
    for (member, abstract_) in &picked {
        if let Some(existing) = target.members.iter().find(|known| known.same(member)) {
            let message =
                t!("workbench-java-hierarchy-exists", class = target.name.as_str(), member = member.signature());
            edits.conflict(
                destination,
                destination_text,
                existing.name_at,
                existing.name_at + existing.name.len(),
                message,
            );
        }
        let declare =
            *abstract_ || (target.is_interface() && member.kind == MemberKind::Method && !member.has("static"));
        if target.is_interface() && member.kind == MemberKind::Field && !(member.has("static") && member.has("final")) {
            return Err(t!("workbench-java-pull-up-interface-field", name = member.name.as_str()));
        }
        if target.is_interface() && member.has("private") && member.kind == MemberKind::Method {
            return Err(t!("workbench-java-pull-up-interface-private", name = member.signature()));
        }
        if target.is_interface() && declare && !member.has("public") {
            let message = t!("workbench-java-pull-up-interface-public", name = member.signature());
            edits.conflict(source, text, member.name_at, member.name_at + member.name.len(), message);
        }
        if !inserted.is_empty() {
            inserted.push('\n');
        }
        if declare {
            inserted.push_str(&format!("{indent}{}\n", declaration(text, member, target.is_interface())));
            made_abstract |= !target.is_interface();
        } else {
            let widen = member.has("private") && used_by_staying(text, class, &moving, &member.name);
            inserted.push_str(&block_text(text, member, &indent, widen));
        }
    }
    for (name, at) in uses_outside(text, class, &moving) {
        if target.members.iter().any(|known| known.names.contains(&name)) {
            continue;
        }
        let message = t!("workbench-java-hierarchy-uses-staying", name = name.as_str(), class = class.name.as_str());
        edits.conflict(source, text, at, at + name.len(), message);
    }
    for member in &moving {
        let (from, to) = removal(text, member);
        edits.edit(source, text, from, to, String::new());
    }
    let (from, to, new) = insertion(destination_text, target, &inserted);
    edits.edit(destination, destination_text, from, to, new);
    if made_abstract && !target.has("abstract") {
        edits.edit(destination, destination_text, target.keyword_at, target.keyword_at, "abstract ".to_owned());
        let message = t!("workbench-java-hierarchy-made-abstract", class = target.name.as_str());
        edits.conflict(destination, destination_text, target.name_at, target.name_at + target.name.len(), message);
    }
    if !same_file {
        let texts: Vec<&str> = vec![inserted.as_str()];
        let needed = needed_imports(text, destination_text, &texts);
        if let Some((at, lines)) = members::import_insertion(destination_text, &needed) {
            edits.edit(destination, destination_text, at, at, lines);
        }
    }
    Ok(())
}

pub(super) struct Target {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) class: ClassDecl,
}

pub(super) fn targets(session: &mut Session, items: &[Value]) -> Vec<Target> {
    with_languages(session, |languages, workbench| {
        items
            .iter()
            .filter_map(|item| {
                let path = zen_lsp::position::uri_path(item.get("uri")?.as_str()?)?;
                if !is_java(&path) {
                    return None;
                }
                let text = text_of(languages, workbench, &path)?;
                let name = item.get("name")?.as_str()?.split('<').next()?.trim().to_owned();
                let line = item.pointer("/selectionRange/start/line").and_then(Value::as_u64);
                let found = classes(&text).into_iter().filter(|class| class.name == name).min_by_key(|class| {
                    let class_line = text[..class.name_at].matches('\n').count() as u64;
                    line.map_or(0, |line| class_line.abs_diff(line))
                })?;
                Some(Target { path, text, class: found })
            })
            .collect()
    })
}

pub(super) fn hierarchy(
    session: &mut Session,
    source: &Source,
    class: &ClassDecl,
    direction: &'static str,
    wait: Duration,
) -> Result<Progress<Vec<Value>>, CommandError> {
    let offset = source.char_at(class.name_at);
    let ask =
        Ask::At { path: source.path.clone(), offset, method: "textDocument/prepareTypeHierarchy", extra: json!({}) };
    let key = format!("{:?}\n{offset}", source.path);
    let item = match ask_all(session, "javaPrepareHierarchy", key, vec![ask], wait)? {
        Progress::Ready(answers) => answers.into_iter().next().and_then(|answer| answer.get(0).cloned()),
        Progress::Pending => return Ok(Progress::Pending),
        Progress::Failed(error) => return Ok(Progress::Failed(error)),
    };
    let Some(item) = item else {
        return Ok(Progress::Failed(t!("workbench-java-hierarchy-no-type", name = class.name.as_str())));
    };
    let method = format!("typeHierarchy/{direction}");
    let ask = Ask::With { path: source.path.clone(), method, params: json!({ "item": item }) };
    let key = format!("{item}");
    Ok(match ask_all(session, direction, key, vec![ask], wait)? {
        Progress::Ready(answers) => Progress::Ready(
            answers
                .into_iter()
                .next()
                .and_then(|answer| answer.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter(|item| {
                    item.get("uri")
                        .and_then(Value::as_str)
                        .and_then(zen_lsp::position::uri_path)
                        .is_some_and(|path| path.is_file())
                })
                .collect(),
        ),
        Progress::Pending => Progress::Pending,
        Progress::Failed(error) => Progress::Failed(error),
    })
}

struct PullMembersUp;

impl PullMembersUp {
    fn found(
        session: &mut Session,
        context: &Value,
        wait: Duration,
    ) -> Result<Progress<(Source, ClassDecl, Vec<Target>)>, CommandError> {
        let source = match Source::find(session, context, "workbench-java-hierarchy-not-here") {
            Ok(source) => source,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let class = match source.class("workbench-java-hierarchy-not-here") {
            Ok(class) => class,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let items = match hierarchy(session, &source, &class, "supertypes", wait)? {
            Progress::Ready(items) => items,
            Progress::Pending => return Ok(Progress::Pending),
            Progress::Failed(error) => return Ok(Progress::Failed(error)),
        };
        let targets = targets(session, &items);
        if targets.is_empty() {
            return Ok(Progress::Failed(t!("workbench-java-pull-up-no-super", name = class.name.as_str())));
        }
        Ok(Progress::Ready((source, class, targets)))
    }

    fn outcome(
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<Outcome, CommandError> {
        let (source, class, targets) = match Self::found(session, context, wait)? {
            Progress::Ready(found) => found,
            other => return Ok(other.into()),
        };
        let uri = values.get("destination").and_then(Value::as_str).map(str::to_owned);
        let target = match uri {
            Some(uri) => targets.iter().find(|target| zen_lsp::position::file_uri(&target.path) == uri.as_str()),
            None => targets.first(),
        };
        let Some(target) = target else {
            return Ok(Outcome::Failed(t!("workbench-java-pull-up-no-super", name = class.name.as_str())));
        };
        let chosen = chosen(values, "members", &member_rows(&class, source.at, true, false));
        let mut edits = Edits::default();
        let pull = PullUp {
            source: &source.path,
            text: &source.text,
            class: &class,
            destination: &target.path,
            destination_text: if target.path == source.path { &source.text } else { &target.text },
            target: &target.class,
            chosen: &chosen,
        };
        Ok(match pull_up(&pull, &mut edits) {
            Ok(()) => Outcome::Ready(edits),
            Err(error) => Outcome::Failed(error),
        })
    }
}

impl RefactoringProvider for PullMembersUp {
    fn id(&self) -> &str {
        PULL_UP
    }

    fn title(&self) -> String {
        t!("workbench-java-pull-up-title")
    }

    fn action(&self) -> Option<&str> {
        Some("pullMembersUp")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let (source, class, targets) = match Self::found(session, context, wait)? {
            Progress::Ready(found) => found,
            Progress::Pending => return Ok(pending_form()),
            Progress::Failed(error) => return Ok(failed_form(error)),
        };
        let options: Vec<(String, String)> = targets
            .iter()
            .map(|target| {
                let package = members::package(&target.text);
                let label = if package.is_empty() {
                    target.class.name.clone()
                } else {
                    format!("{package}.{}", target.class.name)
                };
                (zen_lsp::position::file_uri(&target.path), label)
            })
            .collect();
        let first = options.first().map(|(value, _)| value.clone()).unwrap_or_default();
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-pull-up-title-of", name = class.name.as_str()),
            fields: vec![
                choice("destination", t!("workbench-java-pull-up-to"), &first, options),
                table(
                    "members",
                    t!("workbench-java-hierarchy-members"),
                    member_rows(&class, source.at, true, false),
                    member_columns(Some(t!("workbench-java-hierarchy-make-abstract"))),
                ),
            ],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        plan_outcome(session, outcome)
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        apply_outcome(session, outcome, crate::local_history::PULL_MEMBERS_UP)
    }

    fn cancel(&self, session: &mut Session) {
        forget(session, &["javaPrepareHierarchy", "supertypes"]);
    }
}

pub(super) struct PushDown<'a> {
    pub(super) source: &'a Path,
    pub(super) text: &'a str,
    pub(super) class: &'a ClassDecl,
    pub(super) targets: &'a [Target],
    pub(super) chosen: &'a [Chosen],
    pub(super) usages: &'a [(String, Vec<Usage>)],
}

pub(super) fn push_down(push: &PushDown<'_>, edits: &mut Edits) -> Result<(), String> {
    let PushDown { source, text, class, targets, chosen, usages } = *push;
    let picked = picked(class, chosen);
    if picked.is_empty() {
        return Err(t!("workbench-java-hierarchy-choose-members"));
    }
    let moving: Vec<&Member> = picked.iter().map(|(member, _)| *member).collect();
    let mut made_abstract = false;
    for (member, keep_abstract) in &picked {
        for word in words(text, member.first, member.end) {
            let private = class
                .members
                .iter()
                .any(|other| !moving.contains(&other) && other.has("private") && other.names.contains(&word.text));
            if private && !word.qualified {
                let message =
                    t!("workbench-java-push-down-private", name = word.text.as_str(), class = class.name.as_str());
                edits.conflict(source, text, word.at, word.at + word.text.len(), message);
            }
        }
        if *keep_abstract {
            let start = member.modifiers.first().map_or(member.head, |(_, at)| *at);
            edits.edit(source, text, start, member.end, declaration(text, member, false));
            made_abstract = true;
        } else {
            let (from, to) = removal(text, member);
            edits.edit(source, text, from, to, String::new());
            let signature = member.signature();
            for usage in usages.iter().filter(|(known, _)| *known == signature).flat_map(|(_, found)| found) {
                let inside_target = targets.iter().any(|target| {
                    same_path(&target.path, &usage.path)
                        && target.class.keyword_at <= usage.from
                        && usage.to <= target.class.close + 1
                });
                let inside_member =
                    same_path(source, &usage.path) && member.first <= usage.from && usage.to <= member.end;
                if !inside_target && !inside_member {
                    let message = t!(
                        "workbench-java-push-down-used",
                        name = member.name.as_str(),
                        file = usage.file(),
                        line = usage.line
                    );
                    edits.conflict_at(usage, message);
                }
            }
        }
    }
    if made_abstract && !class.has("abstract") {
        edits.edit(source, text, class.keyword_at, class.keyword_at, "abstract ".to_owned());
        let message = t!("workbench-java-hierarchy-made-abstract", class = class.name.as_str());
        edits.conflict(source, text, class.name_at, class.name_at + class.name.len(), message);
    }
    for target in targets {
        let target_text = if same_path(&target.path, source) { text } else { target.text.as_str() };
        let indent = member_indent(target_text, &target.class);
        let mut inserted = String::new();
        for (member, keep_abstract) in &picked {
            if let Some(existing) = target.class.members.iter().find(|known| known.same(member)) {
                if !keep_abstract {
                    let message = t!(
                        "workbench-java-hierarchy-exists",
                        class = target.class.name.as_str(),
                        member = member.signature()
                    );
                    edits.conflict(
                        &target.path,
                        target_text,
                        existing.name_at,
                        existing.name_at + existing.name.len(),
                        message,
                    );
                }
                continue;
            }
            if !inserted.is_empty() {
                inserted.push('\n');
            }
            inserted.push_str(&block_text(text, member, &indent, false));
        }
        if inserted.is_empty() {
            continue;
        }
        let (from, to, new) = insertion(target_text, &target.class, &inserted);
        edits.edit(&target.path, target_text, from, to, new);
        if !same_path(&target.path, source) {
            let needed = needed_imports(text, target_text, &[inserted.as_str()]);
            if let Some((at, lines)) = members::import_insertion(target_text, &needed) {
                edits.edit(&target.path, target_text, at, at, lines);
            }
        }
    }
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || super::real(a) == super::real(b)
}

struct PushMembersDown;

impl PushMembersDown {
    fn found(
        session: &mut Session,
        context: &Value,
        wait: Duration,
    ) -> Result<Progress<(Source, ClassDecl, Vec<Target>)>, CommandError> {
        let source = match Source::find(session, context, "workbench-java-hierarchy-not-here") {
            Ok(source) => source,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let class = match source.class("workbench-java-hierarchy-not-here") {
            Ok(class) if class.is_class() => class,
            Ok(_) => return Ok(Progress::Failed(t!("workbench-java-push-down-class"))),
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let items = match hierarchy(session, &source, &class, "subtypes", wait)? {
            Progress::Ready(items) => items,
            Progress::Pending => return Ok(Progress::Pending),
            Progress::Failed(error) => return Ok(Progress::Failed(error)),
        };
        let targets: Vec<Target> = targets(session, &items)
            .into_iter()
            .filter(|target| target.class.extends.iter().any(|name| name.name == class.name))
            .collect();
        if targets.is_empty() {
            return Ok(Progress::Failed(t!("workbench-java-push-down-no-sub", name = class.name.as_str())));
        }
        Ok(Progress::Ready((source, class, targets)))
    }

    fn outcome(
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<Outcome, CommandError> {
        let (source, class, targets) = match Self::found(session, context, wait)? {
            Progress::Ready(found) => found,
            other => return Ok(other.into()),
        };
        let chosen = chosen(values, "members", &member_rows(&class, source.at, true, false));
        let asked: Vec<&Member> =
            picked(&class, &chosen).into_iter().filter(|(_, keep)| !keep).map(|(member, _)| member).collect();
        let asks: Vec<Ask> = asked
            .iter()
            .map(|member| Ask::At {
                path: source.path.clone(),
                offset: source.char_at(member.name_at),
                method: "textDocument/references",
                extra: json!({ "context": { "includeDeclaration": false } }),
            })
            .collect();
        let key = asked.iter().map(|member| member.signature()).collect::<Vec<_>>().join("\n");
        let answers = if asks.is_empty() {
            Vec::new()
        } else {
            match ask_all(session, "javaPushDownUsages", format!("{:?}\n{key}", source.path), asks, wait)? {
                Progress::Ready(answers) => answers,
                other => return Ok(other.into()),
            }
        };
        let usages: Vec<(String, Vec<Usage>)> = asked
            .iter()
            .zip(&answers)
            .map(|(member, answer)| (member.signature(), reference_places(session, answer)))
            .collect();
        let mut edits = Edits::default();
        let push = PushDown {
            source: &source.path,
            text: &source.text,
            class: &class,
            targets: &targets,
            chosen: &chosen,
            usages: &usages,
        };
        Ok(match push_down(&push, &mut edits) {
            Ok(()) => Outcome::Ready(edits),
            Err(error) => Outcome::Failed(error),
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Usage {
    pub(super) path: PathBuf,
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) line: usize,
    pub(super) range: Value,
}

impl Usage {
    pub(super) fn file(&self) -> String {
        self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

pub(super) fn reference_places(session: &mut Session, answer: &Value) -> Vec<Usage> {
    let items = answer.as_array().cloned().unwrap_or_default();
    with_languages(session, |languages, workbench| {
        items
            .iter()
            .filter_map(|item| {
                let path = zen_lsp::position::uri_path(item.get("uri")?.as_str()?)?;
                let text = text_of(languages, workbench, &path)?;
                let range = item.get("range")?.clone();
                let position = |key: &str| -> Option<usize> {
                    let at = range.get(key)?;
                    let position = lsp_types::Position {
                        line: at.get("line")?.as_u64()? as u32,
                        character: at.get("character")?.as_u64()? as u32,
                    };
                    zen_lsp::position::offset(&text, position)
                };
                let (from, to) = (position("start")?, position("end")?);
                let line = text[..from].matches('\n').count() + 1;
                Some(Usage { path, from, to, line, range })
            })
            .collect()
    })
}

impl RefactoringProvider for PushMembersDown {
    fn id(&self) -> &str {
        PUSH_DOWN
    }

    fn title(&self) -> String {
        t!("workbench-java-push-down-title")
    }

    fn action(&self) -> Option<&str> {
        Some("pushMembersDown")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let (source, class, targets) = match Self::found(session, context, wait)? {
            Progress::Ready(found) => found,
            Progress::Pending => return Ok(pending_form()),
            Progress::Failed(error) => return Ok(failed_form(error)),
        };
        let names: Vec<&str> = targets.iter().map(|target| target.class.name.as_str()).collect();
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-push-down-title-of", name = class.name.as_str()),
            message: Some(t!("workbench-java-push-down-to", classes = names.join(", "))),
            fields: vec![table(
                "members",
                t!("workbench-java-hierarchy-members"),
                member_rows(&class, source.at, true, false),
                member_columns(Some(t!("workbench-java-push-down-keep-abstract"))),
            )],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        plan_outcome(session, outcome)
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        apply_outcome(session, outcome, crate::local_history::PUSH_MEMBERS_DOWN)
    }

    fn cancel(&self, session: &mut Session) {
        forget(session, &["javaPrepareHierarchy", "subtypes", "javaPushDownUsages"]);
    }
}

pub(super) struct NewSuperclass<'a> {
    pub(super) source: &'a Path,
    pub(super) text: &'a str,
    pub(super) class: &'a ClassDecl,
    pub(super) name: &'a str,
    pub(super) chosen: &'a [Chosen],
}

pub(super) fn extract_superclass(extract: &NewSuperclass<'_>, edits: &mut Edits) -> Result<PathBuf, String> {
    let NewSuperclass { source, text, class, name, chosen } = *extract;
    if !is_identifier(Some("java"), name) {
        return Err(t!("workbench-rename-invalid-name", name = name));
    }
    let path = source.parent().map(|folder| folder.join(format!("{name}.java"))).unwrap_or_default();
    if path.exists() || classes(text).iter().any(|known| known.name == name) {
        return Err(t!("workbench-java-hierarchy-file-exists", name = name));
    }
    let picked = picked(class, chosen);
    let moving: Vec<&Member> = picked.iter().filter(|(_, abstract_)| !abstract_).map(|(member, _)| *member).collect();
    let indent = member_indent(text, class);
    let unit = indent.trim_start_matches(&indent_of(text, class.keyword_at)).to_owned();
    let mut body = String::new();
    let mut any_abstract = false;
    for (member, abstract_) in &picked {
        if !body.is_empty() {
            body.push('\n');
        }
        if *abstract_ {
            body.push_str(&format!("{unit}{}\n", declaration(text, member, false)));
            any_abstract = true;
        } else {
            let widen = member.has("private") && used_by_staying(text, class, &moving, &member.name);
            body.push_str(&block_text(text, member, &unit, widen));
        }
    }
    for (used, at) in uses_outside(text, class, &moving) {
        let message = t!("workbench-java-hierarchy-uses-staying", name = used.as_str(), class = class.name.as_str());
        edits.conflict(source, text, at, at + used.len(), message);
    }
    let old = class.extends.first().map(|parent| text[parent.start..parent.end].to_owned());
    if old.is_some() {
        for member in class.members.iter().filter(|member| member.kind == MemberKind::Constructor) {
            let Some((open, close)) = member.body else { continue };
            for word in words(text, open, close) {
                let call = text[word.at + word.text.len()..].trim_start();
                if word.text == "super" && call.starts_with('(') && !call[1..].trim_start().starts_with(')') {
                    let message = t!("workbench-java-superclass-constructor", class = name);
                    edits.conflict(source, text, word.at, word.at + 5, message);
                }
            }
        }
    }
    let package = members::package(text);
    let header = format!(
        "{}{}class {name}{} {{\n",
        if class.has("public") { "public " } else { "" },
        if any_abstract { "abstract " } else { "" },
        old.as_ref().map(|old| format!(" extends {old}")).unwrap_or_default()
    );
    let imports = needed_imports(text, "", &[body.as_str(), header.as_str()]);
    let mut content = String::new();
    if !package.is_empty() {
        content.push_str(&format!("package {package};\n\n"));
    }
    if !imports.is_empty() {
        content.push_str(&imports.join("\n"));
        content.push_str("\n\n");
    }
    content.push_str(&header);
    content.push_str(&body);
    content.push_str("}\n");
    edits.create(path.clone(), content);
    match class.extends.first() {
        Some(parent) => edits.edit(source, text, parent.start, parent.end, name.to_owned()),
        None => edits.edit(source, text, class.after_name, class.after_name, format!(" extends {name}")),
    }
    for member in &moving {
        let (from, to) = removal(text, member);
        edits.edit(source, text, from, to, String::new());
    }
    Ok(path)
}

struct ExtractSuperclass;

impl ExtractSuperclass {
    fn found(session: &mut Session, context: &Value) -> Result<(Source, ClassDecl), String> {
        let source = Source::find(session, context, "workbench-java-hierarchy-not-here")?;
        let class = source.class("workbench-java-hierarchy-not-here")?;
        if !class.is_class() {
            return Err(t!("workbench-java-superclass-class"));
        }
        Ok((source, class))
    }

    fn outcome(session: &mut Session, context: &Value, values: &Values) -> Outcome {
        let (source, class) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Outcome::Failed(error),
        };
        let name = given(values, "name", &format!("Base{}", class.name));
        let chosen = chosen(values, "members", &member_rows(&class, source.at, true, false));
        let mut edits = Edits::default();
        let extract =
            NewSuperclass { source: &source.path, text: &source.text, class: &class, name: &name, chosen: &chosen };
        match extract_superclass(&extract, &mut edits) {
            Ok(_) => Outcome::Ready(edits),
            Err(error) => Outcome::Failed(error),
        }
    }
}

impl RefactoringProvider for ExtractSuperclass {
    fn id(&self) -> &str {
        EXTRACT_SUPERCLASS
    }

    fn title(&self) -> String {
        t!("workbench-java-superclass-title")
    }

    fn action(&self) -> Option<&str> {
        Some("extractSuperclass")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let (source, class) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Ok(failed_form(error)),
        };
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-superclass-title-of", name = class.name.as_str()),
            fields: vec![
                text_field("name", t!("workbench-java-superclass-name"), &format!("Base{}", class.name)),
                table(
                    "members",
                    t!("workbench-java-hierarchy-members"),
                    member_rows(&class, source.at, true, false),
                    member_columns(Some(t!("workbench-java-hierarchy-make-abstract"))),
                ),
            ],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        _: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let outcome = Self::outcome(session, context, values);
        plan_outcome(session, outcome)
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        _: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let outcome = Self::outcome(session, context, values);
        apply_outcome(session, outcome, crate::local_history::EXTRACT_SUPERCLASS)
    }

    fn cancel(&self, _: &mut Session) {}
}

struct ExtractInterface;

impl ExtractInterface {
    fn status(
        session: &mut Session,
        context: &Value,
        wait: Duration,
    ) -> Result<Progress<(Source, Value)>, CommandError> {
        let source = match Source::find(session, context, "workbench-java-hierarchy-not-here") {
            Ok(source) => source,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let class = match source.class("workbench-java-hierarchy-not-here") {
            Ok(class) => class,
            Err(error) => return Ok(Progress::Failed(error)),
        };
        let offset = source.char_at(class.name_at);
        let ask = Ask::Code {
            path: source.path.clone(),
            offset,
            method: "java/checkExtractInterfaceStatus",
            extra: json!({}),
        };
        let key = format!("{:?}\n{offset}", source.path);
        Ok(match ask_all(session, "javaExtractInterfaceStatus", key, vec![ask], wait)? {
            Progress::Ready(answers) => {
                let status = answers.into_iter().next().unwrap_or_default();
                if let Some(error) = status.pointer("/destinationResponse/errorMessage").and_then(Value::as_str) {
                    Progress::Failed(error.to_owned())
                } else if status.get("subTypeName").and_then(Value::as_str).is_none() {
                    Progress::Failed(t!("workbench-java-interface-refused"))
                } else {
                    Progress::Ready((source, status))
                }
            }
            Progress::Pending => Progress::Pending,
            Progress::Failed(error) => Progress::Failed(error),
        })
    }

    fn destinations(status: &Value) -> Vec<Value> {
        status.pointer("/destinationResponse/destinations").and_then(Value::as_array).cloned().unwrap_or_default()
    }

    fn default_destination(status: &Value) -> String {
        let list = Self::destinations(status);
        list.iter()
            .find(|destination| destination.get("isParentOfSelectedFile").and_then(Value::as_bool).unwrap_or(false))
            .or_else(|| list.first())
            .and_then(|destination| destination.get("uri").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned()
    }

    fn rows(status: &Value, source: &Source) -> Vec<Value> {
        let caret = class_at(&source.text, source.at)
            .and_then(|class| class.member_at(source.at).map(|member| member.name.clone()));
        status
            .get("members")
            .and_then(Value::as_array)
            .map(|members| {
                members
                    .iter()
                    .map(|member| {
                        let name = member.get("name").and_then(Value::as_str).unwrap_or_default();
                        let parameters: Vec<&str> = member
                            .get("parameters")
                            .and_then(Value::as_array)
                            .map(|list| list.iter().filter_map(Value::as_str).collect())
                            .unwrap_or_default();
                        let kind = member.get("typeName").and_then(Value::as_str).unwrap_or_default();
                        json!({
                            "selected": caret.as_deref() == Some(name),
                            "member": format!("{name}({}): {kind}", parameters.join(", ")),
                            "handle": member.get("handleIdentifier").cloned().unwrap_or_default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn outcome(
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<Outcome, CommandError> {
        let (source, status) = match Self::status(session, context, wait)? {
            Progress::Ready(found) => found,
            other => return Ok(other.into()),
        };
        let subtype = status.get("subTypeName").and_then(Value::as_str).unwrap_or_default().to_owned();
        let name = given(values, "name", &format!("I{subtype}"));
        if !is_identifier(Some("java"), &name) {
            return Ok(Outcome::Failed(t!("workbench-rename-invalid-name", name = name.as_str())));
        }
        let uri = given(values, "destination", &Self::default_destination(&status));
        let list = Self::destinations(&status);
        let Some(destination) =
            list.iter().find(|destination| destination.get("uri").and_then(Value::as_str) == Some(uri.as_str()))
        else {
            return Ok(Outcome::Failed(t!("workbench-java-move-choose")));
        };
        if let Some(folder) = zen_lsp::position::uri_path(&uri)
            && folder.join(format!("{name}.java")).exists()
        {
            return Ok(Outcome::Failed(t!("workbench-java-hierarchy-file-exists", name = name.as_str())));
        }
        let rows = Self::rows(&status, &source);
        let rows = values.get("members").and_then(Value::as_array).cloned().unwrap_or(rows);
        let handles: Vec<Value> = rows
            .iter()
            .filter(|row| row.get("selected").and_then(Value::as_bool).unwrap_or(false))
            .filter_map(|row| row.get("handle").cloned())
            .collect();
        let offset = class_at(&source.text, source.at).map_or(0, |class| source.char_at(class.name_at));
        let ask = Ask::Refactor {
            path: source.path.clone(),
            offset,
            extra: json!({
                "command": "extractInterface",
                "options": { "tabSize": 4, "insertSpaces": true },
                "commandArguments": [handles, name, destination]
            }),
        };
        let key = format!("{:?}\n{handles:?}\n{name}\n{uri}", source.path);
        let answer = match ask_all(session, "javaExtractInterface", key, vec![ask], wait)? {
            Progress::Ready(answers) => answers.into_iter().next().unwrap_or_default(),
            other => return Ok(other.into()),
        };
        if let Some(error) = answer.get("errorMessage").and_then(Value::as_str) {
            return Ok(Outcome::Failed(error.to_owned()));
        }
        let Some(edit) = answer.get("edit").filter(|edit| !edit.is_null()) else {
            return Ok(Outcome::Failed(t!("workbench-java-interface-refused")));
        };
        Ok(Outcome::Ready(server_edits(session, &without_generated_comments(edit))))
    }
}

pub(super) fn without_generated_comments(edit: &Value) -> Value {
    let mut edit = edit.clone();
    let clean = |text: &str| -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(at) = rest.find("/** (non-Javadoc)") {
            let Some(close) = rest[at..].find("*/") else { break };
            out.push_str(&rest[..at]);
            let after = &rest[at + close + 2..];
            let trimmed = after.trim_start_matches([' ', '\t']);
            let trimmed = trimmed.strip_prefix('\n').unwrap_or(trimmed);
            let indent: String = trimmed.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            rest = &trimmed[indent.len()..];
        }
        out.push_str(rest);
        out
    };
    if let Some(changes) = edit.get_mut("documentChanges").and_then(Value::as_array_mut) {
        for change in changes {
            if let Some(edits) = change.get_mut("edits").and_then(Value::as_array_mut) {
                for edit in edits {
                    if let Some(text) = edit.get("newText").and_then(Value::as_str) {
                        edit["newText"] = json!(clean(text));
                    }
                }
            }
        }
    }
    edit
}

fn server_edits(session: &mut Session, edit: &Value) -> Edits {
    let refined =
        with_languages(session, |languages, workbench| super::refine(edit, |path| text_of(languages, workbench, path)));
    Edits { raw: Some(refined), ..Edits::default() }
}

impl RefactoringProvider for ExtractInterface {
    fn id(&self) -> &str {
        EXTRACT_INTERFACE
    }

    fn title(&self) -> String {
        t!("workbench-java-interface-title")
    }

    fn action(&self) -> Option<&str> {
        Some("extractInterface")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let (source, status) = match Self::status(session, context, wait)? {
            Progress::Ready(found) => found,
            Progress::Pending => return Ok(pending_form()),
            Progress::Failed(error) => return Ok(failed_form(error)),
        };
        let subtype = status.get("subTypeName").and_then(Value::as_str).unwrap_or_default();
        let options: Vec<(String, String)> = Self::destinations(&status)
            .iter()
            .map(|destination| {
                (
                    destination.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    destination.get("displayName").and_then(Value::as_str).unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let columns = vec![
            TableColumn::new("selected", String::new(), ColumnKind::Checkbox),
            TableColumn::new("member", t!("workbench-java-hierarchy-member"), ColumnKind::Label),
        ];
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-interface-title-of", name = subtype),
            fields: vec![
                text_field("name", t!("workbench-java-interface-name"), &format!("I{subtype}")),
                choice(
                    "destination",
                    t!("workbench-java-move-to-package"),
                    &Self::default_destination(&status),
                    options,
                ),
                table("members", t!("workbench-java-interface-members"), Self::rows(&status, &source), columns),
            ],
            ..RefactoringForm::default()
        }))
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        plan_outcome(session, outcome)
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let outcome = Self::outcome(session, context, values, wait)?;
        let applied = apply_outcome(session, outcome, crate::local_history::EXTRACT_INTERFACE);
        if applied.is_ok() {
            forget(session, &["javaExtractInterfaceStatus", "javaExtractInterface"]);
        }
        applied
    }

    fn cancel(&self, session: &mut Session) {
        forget(session, &["javaExtractInterfaceStatus", "javaExtractInterface"]);
    }
}

pub(super) fn providers() -> Vec<std::sync::Arc<dyn RefactoringProvider>> {
    vec![
        std::sync::Arc::new(PullMembersUp),
        std::sync::Arc::new(PushMembersDown),
        std::sync::Arc::new(ExtractSuperclass),
        std::sync::Arc::new(ExtractInterface),
    ]
}

#[cfg(test)]
pub(super) mod tests;
