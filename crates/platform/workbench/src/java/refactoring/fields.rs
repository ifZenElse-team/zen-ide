use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::{CommandError, Session};
use zen_i18n::t;

use super::hierarchy::{
    Ask, Edits, Outcome, Source, Usage, accepts_java, apply_outcome, ask_all, choice, forget, given, plan_outcome,
    reference_places, table, text_field,
};
use super::{Progress, VISIBILITIES, conflict_labels, failed_form};
use crate::java::members::{
    self, ClassDecl, Member, MemberKind, capitalized, indent_of, insertion, member_indent, needed_imports,
    split_arguments, trimmed, words,
};
use crate::java::scan::tokens;
use crate::language::refactor::is_identifier;
use crate::language::refactoring_dialog::{
    ColumnKind, RefactoringApplied, RefactoringForm, RefactoringPlan, RefactoringProvider, TableColumn, Values,
};

pub const ENCAPSULATE_FIELDS: &str = "javaEncapsulateFields";
pub const INTRODUCE_PARAMETER_OBJECT: &str = "javaIntroduceParameterObject";

pub(super) fn providers() -> Vec<std::sync::Arc<dyn RefactoringProvider>> {
    vec![std::sync::Arc::new(EncapsulateFields), std::sync::Arc::new(IntroduceParameterObject)]
}

fn visibility_options() -> Vec<(String, String)> {
    VISIBILITIES
        .iter()
        .map(|value| {
            let label =
                if value.is_empty() { t!("workbench-java-signature-package-private") } else { (*value).to_owned() };
            ((*value).to_owned(), label)
        })
        .collect()
}

fn with_visibility(visibility: &str) -> String {
    if visibility.is_empty() { String::new() } else { format!("{visibility} ") }
}

pub(super) fn accessor_names(member: &Member) -> (String, String) {
    let base = capitalized(&member.name);
    let getter = if member.kind_text == "boolean" { format!("is{base}") } else { format!("get{base}") };
    let setter = if member.has("final") { String::new() } else { format!("set{base}") };
    (getter, setter)
}

fn encapsulable(member: &Member) -> bool {
    member.kind == MemberKind::Field && member.names.len() == 1
}

fn field_rows(class: &ClassDecl, at: usize) -> Vec<Value> {
    let caret = class.member_at(at).filter(|member| encapsulable(member)).map(|member| member.name.clone());
    class
        .members
        .iter()
        .filter(|member| encapsulable(member))
        .map(|member| {
            let (getter, setter) = accessor_names(member);
            json!({
                "selected": caret.as_ref().is_none_or(|name| *name == member.name),
                "field": member.label(),
                "name": member.name,
                "getter": getter,
                "setter": setter
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(super) struct Encapsulated {
    pub(super) name: String,
    pub(super) getter: String,
    pub(super) setter: String,
}

fn encapsulated(values: &Values, form_rows: &[Value]) -> Vec<Encapsulated> {
    let rows = values.get("fields").and_then(Value::as_array).map_or(form_rows, Vec::as_slice);
    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    rows.iter()
        .filter(|row| row.get("selected").and_then(Value::as_bool).unwrap_or(false))
        .map(|row| Encapsulated { name: text(row, "name"), getter: text(row, "getter"), setter: text(row, "setter") })
        .collect()
}

pub(super) struct Encapsulation<'a> {
    pub(super) source: &'a Path,
    pub(super) text: &'a str,
    pub(super) class: &'a ClassDecl,
    pub(super) fields: &'a [Encapsulated],
    pub(super) field_visibility: &'a str,
    pub(super) accessor_visibility: &'a str,
    pub(super) usages: &'a [(String, Vec<Usage>)],
    pub(super) texts: &'a [(PathBuf, String)],
}

pub(super) fn encapsulate(plan: &Encapsulation<'_>, edits: &mut Edits) -> Result<(), String> {
    let Encapsulation { source, text, class, fields, field_visibility, accessor_visibility, usages, texts } = *plan;
    if fields.is_empty() {
        return Err(t!("workbench-java-encapsulate-choose"));
    }
    let indent = member_indent(text, class);
    let unit = indent.strip_prefix(indent_of(text, class.keyword_at).as_str()).unwrap_or("    ").to_owned();
    let mut accessors = String::new();
    for field in fields {
        let Some(member) = class.members.iter().find(|member| encapsulable(member) && member.name == field.name) else {
            return Err(t!("workbench-java-encapsulate-no-field", name = field.name.as_str()));
        };
        for name in [&field.getter, &field.setter] {
            if !name.is_empty() && !is_identifier(Some("java"), name) {
                return Err(t!("workbench-rename-invalid-name", name = name.as_str()));
            }
        }
        if field.getter.is_empty() && field.setter.is_empty() {
            return Err(t!("workbench-java-encapsulate-no-accessor", name = field.name.as_str()));
        }
        match member.visibility() {
            Some((old, at)) if old != field_visibility => {
                let new = field_visibility.to_owned();
                let to = if new.is_empty() {
                    text[at + old.len()..].len() - text[at + old.len()..].trim_start().len() + at + old.len()
                } else {
                    at + old.len()
                };
                edits.edit(source, text, at, to, new);
            }
            None if !field_visibility.is_empty() => {
                let at = member.modifiers.first().map_or(member.head, |(_, at)| *at);
                edits.edit(source, text, at, at, format!("{field_visibility} "));
            }
            _ => {}
        }
        let statik = if member.has("static") { "static " } else { "" };
        let exists = |name: &str, arity: usize| {
            class
                .members
                .iter()
                .any(|known| known.kind == MemberKind::Method && known.name == name && known.parameters.len() == arity)
        };
        let kind = &member.kind_text;
        let name = &member.name;
        if !field.getter.is_empty() && !exists(&field.getter, 0) {
            accessors.push_str(&format!(
                "\n{indent}{}{statik}{kind} {}() {{\n{indent}{unit}return {name};\n{indent}}}\n",
                with_visibility(accessor_visibility),
                field.getter
            ));
        }
        if !field.setter.is_empty() && !exists(&field.setter, 1) {
            let target = if member.has("static") { format!("{}.{name}", class.name) } else { format!("this.{name}") };
            accessors.push_str(&format!(
                "\n{indent}{}{statik}void {}({kind} {name}) {{\n{indent}{unit}{target} = {name};\n{indent}}}\n",
                with_visibility(accessor_visibility),
                field.setter
            ));
        }
        let found =
            usages.iter().find(|(known, _)| *known == field.name).map(|(_, found)| found.as_slice()).unwrap_or(&[]);
        for usage in found {
            let inside = same(source, &usage.path) && class.keyword_at <= usage.from && usage.to <= class.close + 1;
            if inside {
                continue;
            }
            let Some(used_text) = texts.iter().find(|(path, _)| same(path, &usage.path)).map(|(_, text)| text.as_str())
            else {
                continue;
            };
            match rewrite_access(used_text, usage.from, usage.to, field) {
                Ok((from, to, new)) => edits.edit(&usage.path, used_text, from, to, new),
                Err(reason) => edits.conflict_at(usage, reason),
            }
        }
    }
    if !accessors.is_empty() {
        let accessors = accessors.trim_start_matches('\n').to_owned();
        let (from, to, new) = insertion(text, class, &accessors);
        edits.edit(source, text, from, to, new);
    }
    Ok(())
}

fn same(a: &Path, b: &Path) -> bool {
    a == b || super::real(a) == super::real(b)
}

fn statement_start(text: &str, at: usize) -> bool {
    let before = text[..at].trim_end();
    before.ends_with(';')
        || before.ends_with('{')
        || before.ends_with('}')
        || before.ends_with(')') && !before.ends_with("()")
        || before.ends_with("else")
}

pub(super) fn rewrite_access(
    text: &str,
    from: usize,
    to: usize,
    field: &Encapsulated,
) -> Result<(usize, usize, String), String> {
    let tokens = tokens(text);
    let offset = |token: &crate::java::scan::Token<'_>| token.text.as_ptr() as usize - text.as_ptr() as usize;
    let Some(index) = tokens.iter().position(|token| offset(token) == from) else {
        return Err(t!("workbench-java-encapsulate-unsure", name = field.name.as_str()));
    };
    let line = &text[members::line_start(text, from)..from];
    if line.trim_start().starts_with("import") {
        return Err(t!("workbench-java-encapsulate-import", name = field.name.as_str()));
    }
    let mut qualifier_index = index;
    while qualifier_index >= 2 && tokens[qualifier_index - 1].text == "." {
        let previous = tokens[qualifier_index - 2].text;
        if previous == ")" || previous == "]" {
            qualifier_index = usize::MAX;
            break;
        }
        qualifier_index -= 2;
    }
    let qualifier = (qualifier_index != usize::MAX).then(|| text[offset(&tokens[qualifier_index])..from].to_owned());
    let start = if qualifier_index == usize::MAX { from } else { offset(&tokens[qualifier_index]) };
    let next = |at: usize| tokens.get(at).map(|token| token.text);
    let glued =
        |at: usize| tokens.get(at).zip(tokens.get(at + 1)).is_some_and(|(a, b)| offset(a) + a.text.len() == offset(b));
    let prefix = qualifier_index.min(index).checked_sub(1);
    let prefix_op = prefix
        .filter(|at| {
            *at >= 1 && glued(*at - 1) && matches!(next(*at), Some("+") | Some("-")) && next(*at - 1) == next(*at)
        })
        .map(|at| (at - 1, next(at).unwrap_or_default()));
    let after = index + 1;
    let op = match (next(after), next(after + 1)) {
        (Some("="), Some("=")) if glued(after) => None,
        (Some("="), _) => Some("="),
        (Some(sign @ ("+" | "-")), Some(again)) if again == sign && glued(after) => {
            Some(if sign == "+" { "++" } else { "--" })
        }
        (Some(sign @ ("+" | "-" | "*" | "/" | "%" | "&" | "|" | "^")), Some("=")) if glued(after) => Some(sign),
        (Some("<" | ">"), Some("<" | ">")) if next(after + 2) == Some("=") => {
            return Err(t!("workbench-java-encapsulate-unsure", name = field.name.as_str()));
        }
        _ => None,
    };
    let write = op.is_some() || prefix_op.is_some();
    if !write {
        if field.getter.is_empty() {
            return Err(t!("workbench-java-encapsulate-no-getter", name = field.name.as_str()));
        }
        return Ok((from, to, format!("{}()", field.getter)));
    }
    if field.setter.is_empty() {
        return Err(t!("workbench-java-encapsulate-no-setter", name = field.name.as_str()));
    }
    if qualifier.is_none() {
        return Err(t!("workbench-java-encapsulate-unsure", name = field.name.as_str()));
    }
    let statement_from = prefix_op.map_or(start, |(at, _)| offset(&tokens[at]));
    if !statement_start(text, statement_from) {
        return Err(t!("workbench-java-encapsulate-value", name = field.name.as_str()));
    }
    let semicolon = |from_token: usize| -> Option<usize> {
        let mut depth = 0usize;
        for token in &tokens[from_token..] {
            match token.text {
                "(" | "{" | "[" => depth += 1,
                ")" | "}" | "]" => depth = depth.checked_sub(1)?,
                ";" if depth == 0 => return Some(offset(token)),
                _ => {}
            }
        }
        None
    };
    let unsure = || t!("workbench-java-encapsulate-unsure", name = field.name.as_str());
    let read_back = |qualifier: &Option<String>| -> Result<String, String> {
        if field.getter.is_empty() {
            return Err(t!("workbench-java-encapsulate-no-getter", name = field.name.as_str()));
        }
        match qualifier {
            Some(qualifier) if !qualifier.contains('(') => Ok(format!("{qualifier}{}()", field.getter)),
            _ => Err(unsure()),
        }
    };
    let setter = &field.setter;
    if let Some((at, sign)) = prefix_op {
        let end = semicolon(index).ok_or_else(unsure)?;
        if text[to..end].trim() != "" {
            return Err(t!("workbench-java-encapsulate-value", name = field.name.as_str()));
        }
        let qualified = qualifier.clone().unwrap_or_default();
        return Ok((offset(&tokens[at]), to, format!("{qualified}{setter}({} {sign} 1)", read_back(&qualifier)?)));
    }
    match op {
        Some("=") => {
            let end = semicolon(after).ok_or_else(unsure)?;
            let value = text[offset(&tokens[after]) + 1..end].trim();
            Ok((from, end, format!("{setter}({value})")))
        }
        Some(sign @ ("++" | "--")) => {
            let close = offset(&tokens[after + 1]) + 1;
            let end = semicolon(after).ok_or_else(unsure)?;
            if text[close..end].trim() != "" {
                return Err(t!("workbench-java-encapsulate-value", name = field.name.as_str()));
            }
            Ok((from, close, format!("{setter}({} {} 1)", read_back(&qualifier)?, &sign[..1])))
        }
        Some(sign) => {
            let end = semicolon(after).ok_or_else(unsure)?;
            let value = text[offset(&tokens[after + 1]) + 1..end].trim();
            let simple = value.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.');
            let value = if simple { value.to_owned() } else { format!("({value})") };
            Ok((from, end, format!("{setter}({} {sign} {value})", read_back(&qualifier)?)))
        }
        None => Err(unsure()),
    }
}

struct EncapsulateFields;

impl EncapsulateFields {
    fn found(session: &mut Session, context: &Value) -> Result<(Source, ClassDecl), String> {
        let source = Source::find(session, context, "workbench-java-hierarchy-not-here")?;
        let class = source.class("workbench-java-hierarchy-not-here")?;
        if !class.members.iter().any(encapsulable) {
            return Err(t!("workbench-java-encapsulate-none", name = class.name.as_str()));
        }
        Ok((source, class))
    }

    fn outcome(
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<Outcome, CommandError> {
        let (source, class) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Ok(Outcome::Failed(error)),
        };
        let fields = encapsulated(values, &field_rows(&class, source.at));
        let asked: Vec<&Member> = fields
            .iter()
            .filter_map(|field| class.members.iter().find(|member| encapsulable(member) && member.name == field.name))
            .collect();
        let asks: Vec<Ask> = asked
            .iter()
            .map(|member| Ask::At {
                path: source.path.clone(),
                offset: source.char_at(member.name_at),
                method: "textDocument/references",
                extra: json!({ "context": { "includeDeclaration": false } }),
            })
            .collect();
        let key = asked.iter().map(|member| member.name.clone()).collect::<Vec<_>>().join("\n");
        let answers = if asks.is_empty() {
            Vec::new()
        } else {
            match ask_all(session, "javaEncapsulateUsages", format!("{:?}\n{key}", source.path), asks, wait)? {
                Progress::Ready(answers) => answers,
                other => return Ok(other.into()),
            }
        };
        let usages: Vec<(String, Vec<Usage>)> = asked
            .iter()
            .zip(&answers)
            .map(|(member, answer)| (member.name.clone(), reference_places(session, answer)))
            .collect();
        let texts = usage_texts(session, &usages);
        let field_visibility = given(values, "fieldVisibility", "private");
        let accessor_visibility = given(values, "accessorVisibility", "public");
        let plan = Encapsulation {
            source: &source.path,
            text: &source.text,
            class: &class,
            fields: &fields,
            field_visibility: &field_visibility,
            accessor_visibility: &accessor_visibility,
            usages: &usages,
            texts: &texts,
        };
        let mut edits = Edits::default();
        Ok(match encapsulate(&plan, &mut edits) {
            Ok(()) => Outcome::Ready(edits),
            Err(error) => Outcome::Failed(error),
        })
    }
}

fn usage_texts(session: &mut Session, usages: &[(String, Vec<Usage>)]) -> Vec<(PathBuf, String)> {
    let mut paths: Vec<PathBuf> =
        usages.iter().flat_map(|(_, found)| found.iter().map(|usage| usage.path.clone())).collect();
    paths.sort();
    paths.dedup();
    crate::language::with_languages(session, |languages, workbench| {
        paths
            .into_iter()
            .filter_map(|path| super::text_of(languages, workbench, &path).map(|text| (path, text)))
            .collect()
    })
}

impl RefactoringProvider for EncapsulateFields {
    fn id(&self) -> &str {
        ENCAPSULATE_FIELDS
    }

    fn title(&self) -> String {
        t!("workbench-java-encapsulate-title")
    }

    fn action(&self) -> Option<&str> {
        Some("encapsulateFields")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let (source, class) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Ok(failed_form(error)),
        };
        let columns = vec![
            TableColumn::new("selected", String::new(), ColumnKind::Checkbox),
            TableColumn::new("field", t!("workbench-java-encapsulate-field"), ColumnKind::Label),
            TableColumn::new("getter", t!("workbench-java-encapsulate-getter"), ColumnKind::Text),
            TableColumn::new("setter", t!("workbench-java-encapsulate-setter"), ColumnKind::Text),
        ];
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-encapsulate-title-of", name = class.name.as_str()),
            fields: vec![
                table("fields", t!("workbench-java-encapsulate-fields"), field_rows(&class, source.at), columns),
                choice(
                    "fieldVisibility",
                    t!("workbench-java-encapsulate-field-visibility"),
                    "private",
                    visibility_options(),
                ),
                choice(
                    "accessorVisibility",
                    t!("workbench-java-encapsulate-accessor-visibility"),
                    "public",
                    visibility_options(),
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
        apply_outcome(session, outcome, crate::local_history::ENCAPSULATE_FIELDS)
    }

    fn cancel(&self, session: &mut Session) {
        forget(session, &["javaEncapsulateUsages"]);
    }
}

pub(super) struct ParameterObject<'a> {
    pub(super) source: &'a Path,
    pub(super) text: &'a str,
    pub(super) class: &'a ClassDecl,
    pub(super) method: &'a Member,
    pub(super) class_name: &'a str,
    pub(super) parameter: &'a str,
    pub(super) record: bool,
    pub(super) selected: &'a [String],
    pub(super) calls: &'a [Usage],
    pub(super) texts: &'a [(PathBuf, String)],
}

pub(super) fn introduce_parameter_object(plan: &ParameterObject<'_>, edits: &mut Edits) -> Result<PathBuf, String> {
    let ParameterObject { source, text, class, method, class_name, parameter, record, selected, calls, texts } = *plan;
    for name in [class_name, parameter] {
        if !is_identifier(Some("java"), name) {
            return Err(t!("workbench-rename-invalid-name", name = name));
        }
    }
    let chosen: Vec<usize> = method
        .parameters
        .iter()
        .enumerate()
        .filter(|(_, p)| selected.contains(&p.name))
        .map(|(index, _)| index)
        .collect();
    let Some(&first) = chosen.first() else { return Err(t!("workbench-java-parameter-object-choose")) };
    if chosen.iter().any(|index| method.parameters[*index].kind.ends_with("...")) {
        return Err(t!("workbench-java-parameter-object-varargs"));
    }
    if method.parameters.iter().any(|p| p.name == parameter && !selected.contains(&p.name)) {
        return Err(t!("workbench-java-parameter-object-name-taken", name = parameter));
    }
    if text[method.first..method.head].contains("@Override") {
        return Err(t!("workbench-java-parameter-object-override", name = method.name.as_str()));
    }
    let path = source.parent().map(|folder| folder.join(format!("{class_name}.java"))).unwrap_or_default();
    if path.exists() || members::classes(text).iter().any(|known| known.name == class_name) {
        return Err(t!("workbench-java-hierarchy-file-exists", name = class_name));
    }
    let accessor = |name: &str| if record { format!("{name}()") } else { format!("get{}()", capitalized(name)) };
    let Some((open, close)) = method.parens else { return Err(t!("workbench-java-parameter-object-not-here")) };
    let mut list = Vec::new();
    for (index, p) in method.parameters.iter().enumerate() {
        if index == first {
            list.push(format!("{class_name} {parameter}"));
        } else if !chosen.contains(&index) {
            list.push(text[p.start..p.name_at + p.name.len()].trim().to_owned());
        }
    }
    edits.edit(source, text, open + 1, close, list.join(", "));
    if let Some((body_open, body_close)) = method.body {
        let tokens = tokens(text);
        let offset = |token: &crate::java::scan::Token<'_>| token.text.as_ptr() as usize - text.as_ptr() as usize;
        for word in words(text, body_open, body_close) {
            if word.qualified || !chosen.iter().any(|index| method.parameters[*index].name == word.text) {
                continue;
            }
            let index = tokens.iter().position(|token| offset(token) == word.at).unwrap_or_default();
            let next: Vec<&str> = tokens.iter().skip(index + 1).take(2).map(|token| token.text).collect();
            let previous = index.checked_sub(1).map(|at| tokens[at].text).unwrap_or_default();
            let assigned = matches!(next.as_slice(), ["=", second] if *second != "=")
                || matches!(
                    next.as_slice(),
                    ["+", "+"] | ["-", "-"] | ["+" | "-" | "*" | "/" | "%" | "&" | "|" | "^", "="]
                )
                || (index >= 2 && matches!((tokens[index - 2].text, previous), ("+", "+") | ("-", "-")));
            if assigned {
                return Err(t!("workbench-java-parameter-object-assigned", name = word.text.as_str()));
            }
            let declared = is_identifier(Some("java"), previous)
                && !matches!(previous, "return" | "new" | "case" | "throw" | "else")
                || matches!(previous, ">" | "]")
                || matches!(next.as_slice(), ["-", ">"]);
            if declared {
                return Err(t!("workbench-java-parameter-object-shadowed", name = word.text.as_str()));
            }
            edits.edit(
                source,
                text,
                word.at,
                word.at + word.text.len(),
                format!("{parameter}.{}", accessor(&word.text)),
            );
        }
    }
    for call in calls {
        if same(&call.path, source) && method.first <= call.from && call.to <= method.end {
            return Err(t!("workbench-java-parameter-object-recursive", name = method.name.as_str()));
        }
        let Some(call_text) = texts.iter().find(|(path, _)| same(path, &call.path)).map(|(_, text)| text.as_str())
        else {
            continue;
        };
        let name_end = name_end(call_text, call, &method.name);
        let rest = &call_text[name_end..];
        let gap = rest.len() - rest.trim_start().len();
        if !rest.trim_start().starts_with('(') {
            edits.conflict_at(call, t!("workbench-java-parameter-object-reference", name = method.name.as_str()));
            continue;
        }
        let open = name_end + gap;
        let Some((close, arguments)) = split_arguments(call_text, open) else { continue };
        if arguments.len() != method.parameters.len() {
            edits.conflict_at(call, t!("workbench-java-parameter-object-arguments", name = method.name.as_str()));
            continue;
        }
        let argument = |index: usize| {
            let (from, to) = trimmed(call_text, arguments[index]);
            call_text[from..to].to_owned()
        };
        let mut new = Vec::new();
        for index in 0..arguments.len() {
            if index == first {
                let inner: Vec<String> = chosen.iter().map(|chosen| argument(*chosen)).collect();
                new.push(format!("new {class_name}({})", inner.join(", ")));
            } else if !chosen.contains(&index) {
                new.push(argument(index));
            }
        }
        edits.edit(&call.path, call_text, open + 1, close, new.join(", "));
    }
    let fields: Vec<(&str, &str)> = chosen
        .iter()
        .map(|index| (method.parameters[*index].kind.as_str(), method.parameters[*index].name.as_str()))
        .collect();
    let unit = member_indent(text, class)
        .strip_prefix(indent_of(text, class.keyword_at).as_str())
        .unwrap_or("    ")
        .to_owned();
    let mut content = String::new();
    let package = members::package(text);
    if !package.is_empty() {
        content.push_str(&format!("package {package};\n\n"));
    }
    let kinds: Vec<&str> = fields.iter().map(|(kind, _)| *kind).collect();
    let imports = needed_imports(text, "", &kinds);
    if !imports.is_empty() {
        content.push_str(&imports.join("\n"));
        content.push_str("\n\n");
    }
    if record {
        let components: Vec<String> = fields.iter().map(|(kind, name)| format!("{kind} {name}")).collect();
        content.push_str(&format!("public record {class_name}({}) {{\n}}\n", components.join(", ")));
    } else {
        content.push_str(&format!("public class {class_name} {{\n"));
        for (kind, name) in &fields {
            content.push_str(&format!("{unit}private final {kind} {name};\n"));
        }
        let arguments: Vec<String> = fields.iter().map(|(kind, name)| format!("{kind} {name}")).collect();
        content.push_str(&format!("\n{unit}public {class_name}({}) {{\n", arguments.join(", ")));
        for (_, name) in &fields {
            content.push_str(&format!("{unit}{unit}this.{name} = {name};\n"));
        }
        content.push_str(&format!("{unit}}}\n"));
        for (kind, name) in &fields {
            content.push_str(&format!(
                "\n{unit}public {kind} get{}() {{\n{unit}{unit}return {name};\n{unit}}}\n",
                capitalized(name)
            ));
        }
        content.push_str("}\n");
    }
    edits.create(path.clone(), content);
    Ok(path)
}

fn name_end(text: &str, call: &Usage, name: &str) -> usize {
    let words = words(text, call.from, call.to.max(call.from + 1));
    words
        .iter()
        .find(|word| word.text == name)
        .or_else(|| words.iter().find(|word| matches!(word.text.as_str(), "this" | "super")))
        .map_or(call.to, |word| word.at + word.text.len())
}

struct IntroduceParameterObject;

impl IntroduceParameterObject {
    fn found(session: &mut Session, context: &Value) -> Result<(Source, ClassDecl, Member), String> {
        let source = Source::find(session, context, "workbench-java-parameter-object-not-here")?;
        let class = source.class("workbench-java-parameter-object-not-here")?;
        let method = class
            .member_at(source.at)
            .filter(|member| matches!(member.kind, MemberKind::Method | MemberKind::Constructor))
            .cloned()
            .ok_or_else(|| t!("workbench-java-parameter-object-not-here"))?;
        if method.parameters.is_empty() {
            return Err(t!("workbench-java-parameter-object-no-parameters", name = method.name.as_str()));
        }
        Ok((source, class, method))
    }

    fn default_name(method: &Member) -> String {
        format!("{}Parameters", capitalized(&method.name))
    }

    fn rows(method: &Member) -> Vec<Value> {
        method
            .parameters
            .iter()
            .map(|p| json!({ "selected": true, "parameter": format!("{} {}", p.kind, p.name), "name": p.name }))
            .collect()
    }

    fn outcome(
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<Outcome, CommandError> {
        let (source, class, method) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Ok(Outcome::Failed(error)),
        };
        let offset = source.char_at(method.name_at);
        let asks = vec![
            Ask::At {
                path: source.path.clone(),
                offset,
                method: "textDocument/references",
                extra: json!({ "context": { "includeDeclaration": false } }),
            },
            Ask::At { path: source.path.clone(), offset, method: "textDocument/implementation", extra: json!({}) },
        ];
        let answers =
            match ask_all(session, "javaParameterObjectUsages", format!("{:?}\n{offset}", source.path), asks, wait)? {
                Progress::Ready(answers) => answers,
                other => return Ok(other.into()),
            };
        let overriding: Vec<Usage> = reference_places(session, answers.get(1).unwrap_or(&Value::Null))
            .into_iter()
            .filter(|usage| !(same(&usage.path, &source.path) && usage.from == method.name_at))
            .collect();
        if let Some(other) = overriding.first() {
            return Ok(Outcome::Failed(t!(
                "workbench-java-parameter-object-overridden",
                name = method.name.as_str(),
                file = other.file()
            )));
        }
        let calls = reference_places(session, answers.first().unwrap_or(&Value::Null));
        let texts = usage_texts(session, &[(String::new(), calls.clone())]);
        let rows = values.get("parameters").and_then(Value::as_array).cloned().unwrap_or_else(|| Self::rows(&method));
        let selected: Vec<String> = rows
            .iter()
            .filter(|row| row.get("selected").and_then(Value::as_bool).unwrap_or(false))
            .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        let class_name = given(values, "className", &Self::default_name(&method));
        let parameter = given(values, "parameterName", "parameters");
        let record = given(values, "kind", "class") == "record";
        let plan = ParameterObject {
            source: &source.path,
            text: &source.text,
            class: &class,
            method: &method,
            class_name: &class_name,
            parameter: &parameter,
            record,
            selected: &selected,
            calls: &calls,
            texts: &texts,
        };
        let mut edits = Edits::default();
        Ok(match introduce_parameter_object(&plan, &mut edits) {
            Ok(_) => Outcome::Ready(edits),
            Err(error) => Outcome::Failed(error),
        })
    }
}

impl RefactoringProvider for IntroduceParameterObject {
    fn id(&self) -> &str {
        INTRODUCE_PARAMETER_OBJECT
    }

    fn title(&self) -> String {
        t!("workbench-java-parameter-object-title")
    }

    fn action(&self) -> Option<&str> {
        Some("introduceParameterObject")
    }

    fn accepts(&self, session: &mut Session, context: &Value) -> bool {
        accepts_java(session, context)
    }

    fn form(&self, session: &mut Session, context: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        let (_, _, method) = match Self::found(session, context) {
            Ok(found) => found,
            Err(error) => return Ok(failed_form(error)),
        };
        let columns = vec![
            TableColumn::new("selected", String::new(), ColumnKind::Checkbox),
            TableColumn::new("parameter", t!("workbench-java-parameter-object-parameter"), ColumnKind::Label),
        ];
        let kinds = vec![
            ("class".to_owned(), t!("workbench-java-parameter-object-class")),
            ("record".to_owned(), t!("workbench-java-parameter-object-record")),
        ];
        Ok(conflict_labels(RefactoringForm {
            title: t!("workbench-java-parameter-object-title-of", name = method.name.as_str()),
            fields: vec![
                text_field("className", t!("workbench-java-parameter-object-class-name"), &Self::default_name(&method)),
                choice("kind", t!("workbench-java-parameter-object-kind"), "class", kinds),
                text_field("parameterName", t!("workbench-java-parameter-object-parameter-name"), "parameters"),
                table("parameters", t!("workbench-java-signature-parameters"), Self::rows(&method), columns),
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
        apply_outcome(session, outcome, crate::local_history::INTRODUCE_PARAMETER_OBJECT)
    }

    fn cancel(&self, session: &mut Session) {
        forget(session, &["javaParameterObjectUsages"]);
    }
}

#[cfg(test)]
mod tests;
