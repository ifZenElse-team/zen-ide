pub mod data;
mod script;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{Column, Constraint, ConstraintKind, Index, Object, ObjectKind, Policy, Schema, Trigger};

pub use script::{Note, NoteKind, ScriptOptions, SyncScript, column_definition, generic_create, sync_script};

/// How an object or a part of it differs, from the target's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    /// Only in the source: the sync script creates it in the target.
    Added,
    /// Only in the target: the sync script drops it when drops are asked for.
    Removed,
    /// In both, different.
    Changed,
}

/// The part of an object an item difference is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// A column.
    Column,
    /// A primary key, unique key, check or exclusion constraint.
    Constraint,
    /// A foreign key.
    ForeignKey,
    /// An index that enforces no constraint.
    Index,
    /// A trigger.
    Trigger,
    /// A row security policy.
    Policy,
    /// A view's query, a routine's source, a sequence's settings or a type's shape.
    Definition,
    /// A property of the object itself: its comment, row security, partitioning.
    Property,
}

/// One part of an object that differs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemDiff {
    /// What part it is.
    pub part: Part,
    /// Its name (the target's when it was matched under another name).
    pub name: String,
    /// How it differs.
    pub change: Change,
    /// The attributes that differ (`type`, `nullable`, `default`, `definition`, …) of a changed part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    /// The source's version in one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The target's version in one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// An object that differs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDiff {
    /// What it is.
    pub kind: ObjectKind,
    /// Its name.
    pub name: String,
    /// The path segment that names it in its folder (a routine's with its arguments).
    pub key: String,
    /// How it differs.
    pub change: Change,
    /// The parts that differ, for a changed object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemDiff>,
    /// Whether one side's details were not read, so only its name was compared.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// The differences between two schemas.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDiff {
    /// The objects that differ, by kind and name.
    pub objects: Vec<ObjectDiff>,
    /// How many objects are the same on both sides.
    pub same: usize,
    /// How many objects were compared by name only because their details were not read.
    pub partial: usize,
}

impl SchemaDiff {
    pub fn counts(&self) -> (usize, usize, usize) {
        let count = |change: Change| self.objects.iter().filter(|object| object.change == change).count();
        (count(Change::Added), count(Change::Removed), count(Change::Changed))
    }
}

/// What to compare.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CompareOptions {
    /// Only these kinds of object; every kind when empty.
    pub kinds: Vec<ObjectKind>,
    /// Leave comments out of the comparison.
    pub ignore_comments: bool,
    /// Compare system objects too.
    pub system: bool,
}

pub fn normal(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.trim_end_matches(';').trim_end().to_owned()
}

fn quoted_forms(schema: &str) -> [String; 3] {
    [format!("\"{}\".", schema.replace('"', "\"\"")), format!("`{}`.", schema.replace('`', "``")), format!("{schema}.")]
}

fn starts_name(previous: Option<char>) -> bool {
    previous.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '"' || c == '`' || c == '.'))
}

pub fn requalify(text: &str, from: &str, to: Option<&str>) -> String {
    if from.is_empty() {
        return text.to_owned();
    }
    let forms = quoted_forms(from);
    let replacements: [String; 3] = match to {
        Some(to) => quoted_forms(to),
        None => [String::new(), String::new(), String::new()],
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut previous: Option<char> = None;
    'outer: while !rest.is_empty() {
        if starts_name(previous) {
            for (form, replacement) in forms.iter().zip(&replacements) {
                if rest.starts_with(form.as_str()) {
                    out.push_str(replacement);
                    rest = &rest[form.len()..];
                    previous = Some('.');
                    continue 'outer;
                }
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        previous = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

pub fn retarget(schema: &Schema, to: &str) -> Schema {
    let from = schema.name.clone();
    let mut copy = schema.clone();
    copy.name = to.to_owned();
    if from == to {
        return copy;
    }
    let fix = |text: &mut String| *text = requalify(text, &from, Some(to));
    for object in &mut copy.objects {
        for constraint in &mut object.constraints {
            fix(&mut constraint.definition);
            if let Some(reference) = constraint.references.as_mut()
                && reference.schema == from
            {
                reference.schema = to.to_owned();
            }
        }
        for index in &mut object.indexes {
            fix(&mut index.definition);
        }
        for trigger in &mut object.triggers {
            fix(&mut trigger.definition);
        }
        if let Some(definition) = object.definition.as_mut() {
            fix(definition);
        }
        if let Some(definition) = object.routine.as_mut().and_then(|routine| routine.definition.as_mut()) {
            fix(definition);
        }
        if let Some(owner) = object.sequence.as_mut().and_then(|sequence| sequence.owned_by.as_mut()) {
            fix(owner);
        }
    }
    copy
}

struct Sides<'a> {
    source: &'a str,
    target: &'a str,
    options: &'a CompareOptions,
}

impl Sides<'_> {
    fn source_text(&self, text: &str) -> String {
        normal(&requalify(text, self.source, None))
    }

    fn target_text(&self, text: &str) -> String {
        normal(&requalify(text, self.target, None))
    }

    fn same_text(&self, source: &str, target: &str) -> bool {
        self.source_text(source).eq_ignore_ascii_case(&self.target_text(target))
    }

    fn same_optional(&self, source: Option<&str>, target: Option<&str>) -> bool {
        match (source, target) {
            (None, None) => true,
            (Some(source), Some(target)) => self.same_text(source, target),
            _ => false,
        }
    }
}

fn same_type(source: &str, target: &str) -> bool {
    normal(source).eq_ignore_ascii_case(&normal(target))
}

fn no_null_default(default: Option<&str>) -> Option<&str> {
    default.filter(|default| !default.trim().eq_ignore_ascii_case("null"))
}

pub fn column_line(column: &Column) -> String {
    let mut line = column.type_name.clone();
    if let Some(identity) = &column.identity {
        line.push_str(if identity == "always" { " identity always" } else { " identity" });
    }
    if let Some(generated) = &column.generated {
        line.push_str(&format!(" generated {}", normal(generated)));
    }
    if !column.nullable {
        line.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default {
        line.push_str(&format!(" DEFAULT {}", normal(default)));
    }
    line
}

fn column_fields(sides: &Sides<'_>, source: &Column, target: &Column) -> Vec<String> {
    let mut fields = Vec::new();
    if !same_type(&source.type_name, &target.type_name) {
        fields.push("type");
    }
    if source.nullable != target.nullable {
        fields.push("nullable");
    }
    if !sides.same_optional(no_null_default(source.default.as_deref()), no_null_default(target.default.as_deref())) {
        fields.push("default");
    }
    if source.identity != target.identity {
        fields.push("identity");
    }
    if !sides.same_optional(source.generated.as_deref(), target.generated.as_deref()) {
        fields.push("generated");
    }
    if source.collation != target.collation {
        fields.push("collation");
    }
    if !sides.options.ignore_comments && source.comment != target.comment {
        fields.push("comment");
    }
    fields.into_iter().map(str::to_owned).collect()
}

fn pair<T>(
    source: &[T],
    target: &[T],
    name: impl Fn(&T) -> String,
    signature: impl Fn(&T, bool) -> Option<String>,
) -> (Vec<(usize, usize)>, Vec<usize>, Vec<usize>) {
    let mut pairs = Vec::new();
    let mut taken = vec![false; target.len()];
    let mut left = Vec::new();
    for (index, item) in source.iter().enumerate() {
        let wanted = name(item);
        match target.iter().enumerate().position(|(at, other)| !taken[at] && name(other) == wanted) {
            Some(at) => {
                taken[at] = true;
                pairs.push((index, at));
            }
            None => left.push(index),
        }
    }
    let mut unpaired = Vec::new();
    for index in left {
        let wanted = name(&source[index]).to_lowercase();
        match target.iter().enumerate().position(|(at, other)| !taken[at] && name(other).to_lowercase() == wanted) {
            Some(at) => {
                taken[at] = true;
                pairs.push((index, at));
            }
            None => unpaired.push(index),
        }
    }
    let mut only_source = Vec::new();
    for index in unpaired {
        let wanted = signature(&source[index], true);
        let found = wanted.as_ref().and_then(|wanted| {
            target
                .iter()
                .enumerate()
                .position(|(at, other)| !taken[at] && signature(other, false).as_ref() == Some(wanted))
        });
        match found {
            Some(at) => {
                taken[at] = true;
                pairs.push((index, at));
            }
            None => only_source.push(index),
        }
    }
    pairs.sort_unstable();
    let only_target = (0..target.len()).filter(|at| !taken[*at]).collect();
    (pairs, only_source, only_target)
}

fn lower_names(names: &[String]) -> String {
    names.iter().map(|name| name.to_lowercase()).collect::<Vec<_>>().join(",")
}

fn constraint_part(constraint: &Constraint) -> Part {
    if constraint.kind == ConstraintKind::ForeignKey { Part::ForeignKey } else { Part::Constraint }
}

fn constraint_signature(constraint: &Constraint) -> String {
    let kind = format!("{:?}", constraint.kind);
    if constraint.kind == ConstraintKind::PrimaryKey {
        return kind;
    }
    let reference = constraint
        .references
        .as_ref()
        .map(|reference| format!("{}({})", reference.table.to_lowercase(), lower_names(&reference.columns)))
        .unwrap_or_default();
    format!("{kind}:{}:{reference}", lower_names(&constraint.columns))
}

fn constraints(sides: &Sides<'_>, source: &Object, target: &Object, items: &mut Vec<ItemDiff>) {
    let signature = |constraint: &Constraint, _: bool| Some(constraint_signature(constraint));
    let (pairs, only_source, only_target) =
        pair(&source.constraints, &target.constraints, |constraint| constraint.name.clone(), signature);
    for (at, other) in pairs {
        let (from, to) = (&source.constraints[at], &target.constraints[other]);
        let mut fields = Vec::new();
        if from.kind != to.kind {
            fields.push("kind".to_owned());
        }
        if !sides.same_text(&from.definition, &to.definition) {
            fields.push("definition".to_owned());
        }
        if !sides.options.ignore_comments && from.comment != to.comment {
            fields.push("comment".to_owned());
        }
        if !fields.is_empty() {
            items.push(ItemDiff {
                part: constraint_part(to),
                name: to.name.clone(),
                change: Change::Changed,
                fields,
                source: Some(normal(&from.definition)),
                target: Some(normal(&to.definition)),
            });
        }
    }
    for at in only_source {
        let constraint = &source.constraints[at];
        items.push(ItemDiff {
            part: constraint_part(constraint),
            name: constraint.name.clone(),
            change: Change::Added,
            fields: Vec::new(),
            source: Some(normal(&constraint.definition)),
            target: None,
        });
    }
    for at in only_target {
        let constraint = &target.constraints[at];
        items.push(ItemDiff {
            part: constraint_part(constraint),
            name: constraint.name.clone(),
            change: Change::Removed,
            fields: Vec::new(),
            source: None,
            target: Some(normal(&constraint.definition)),
        });
    }
}

pub fn plain_indexes(object: &Object) -> Vec<&Index> {
    object
        .indexes
        .iter()
        .filter(|index| {
            !index.primary
                && index.constraint.is_none()
                && !object.constraints.iter().any(|constraint| {
                    constraint.name == index.name
                        && matches!(constraint.kind, ConstraintKind::PrimaryKey | ConstraintKind::Unique)
                })
        })
        .collect()
}

fn index_signature(index: &Index) -> String {
    format!("{}:{}:{}", index.unique, index.method.to_lowercase(), lower_names(&index.columns))
}

fn indexes(sides: &Sides<'_>, source: &Object, target: &Object, items: &mut Vec<ItemDiff>) {
    let (from_list, to_list) = (plain_indexes(source), plain_indexes(target));
    let (pairs, only_source, only_target) =
        pair(&from_list, &to_list, |index| index.name.clone(), |index, _| Some(index_signature(index)));
    for (at, other) in pairs {
        let (from, to) = (from_list[at], to_list[other]);
        let mut fields = Vec::new();
        if from.unique != to.unique {
            fields.push("unique".to_owned());
        }
        if lower_names(&from.columns) != lower_names(&to.columns) {
            fields.push("columns".to_owned());
        }
        if !from.method.eq_ignore_ascii_case(&to.method) {
            fields.push("method".to_owned());
        }
        let renamed = from.name != to.name;
        let definitions = |index: &Index, own: &str| normal(&index.definition.replace(own, ""));
        if fields.is_empty() && !renamed && !sides.same_text(&definitions(from, &from.name), &definitions(to, &to.name))
        {
            fields.push("definition".to_owned());
        }
        if !fields.is_empty() {
            items.push(ItemDiff {
                part: Part::Index,
                name: to.name.clone(),
                change: Change::Changed,
                fields,
                source: Some(normal(&from.definition)),
                target: Some(normal(&to.definition)),
            });
        }
    }
    for at in only_source {
        let index = from_list[at];
        items.push(ItemDiff {
            part: Part::Index,
            name: index.name.clone(),
            change: Change::Added,
            fields: Vec::new(),
            source: Some(normal(&index.definition)),
            target: None,
        });
    }
    for at in only_target {
        let index = to_list[at];
        items.push(ItemDiff {
            part: Part::Index,
            name: index.name.clone(),
            change: Change::Removed,
            fields: Vec::new(),
            source: None,
            target: Some(normal(&index.definition)),
        });
    }
}

fn triggers(sides: &Sides<'_>, source: &Object, target: &Object, items: &mut Vec<ItemDiff>) {
    let (pairs, only_source, only_target) =
        pair(&source.triggers, &target.triggers, |trigger: &Trigger| trigger.name.clone(), |_, _| None);
    for (at, other) in pairs {
        let (from, to) = (&source.triggers[at], &target.triggers[other]);
        let mut fields = Vec::new();
        if !sides.same_text(&from.definition, &to.definition) {
            fields.push("definition".to_owned());
        }
        if from.enabled != to.enabled {
            fields.push("enabled".to_owned());
        }
        if !fields.is_empty() {
            items.push(ItemDiff {
                part: Part::Trigger,
                name: to.name.clone(),
                change: Change::Changed,
                fields,
                source: Some(normal(&from.definition)),
                target: Some(normal(&to.definition)),
            });
        }
    }
    for (list, indexes, change) in
        [(&source.triggers, only_source, Change::Added), (&target.triggers, only_target, Change::Removed)]
    {
        for at in indexes {
            let trigger = &list[at];
            let line = Some(normal(&trigger.definition));
            items.push(ItemDiff {
                part: Part::Trigger,
                name: trigger.name.clone(),
                change,
                fields: Vec::new(),
                source: if change == Change::Added { line.clone() } else { None },
                target: if change == Change::Removed { line } else { None },
            });
        }
    }
}

fn policy_line(policy: &Policy) -> String {
    let mut line = format!(
        "{} {} to {}",
        if policy.permissive { "permissive" } else { "restrictive" },
        policy.command,
        policy.roles.join(", ")
    );
    if let Some(using) = &policy.using {
        line.push_str(&format!(" using ({})", normal(using)));
    }
    if let Some(check) = &policy.check {
        line.push_str(&format!(" with check ({})", normal(check)));
    }
    line
}

fn policies(sides: &Sides<'_>, source: &Object, target: &Object, items: &mut Vec<ItemDiff>) {
    let (pairs, only_source, only_target) =
        pair(&source.policies, &target.policies, |policy: &Policy| policy.name.clone(), |_, _| None);
    for (at, other) in pairs {
        let (from, to) = (&source.policies[at], &target.policies[other]);
        let same = from.command == to.command
            && from.permissive == to.permissive
            && from.roles == to.roles
            && sides.same_optional(from.using.as_deref(), to.using.as_deref())
            && sides.same_optional(from.check.as_deref(), to.check.as_deref());
        if !same {
            items.push(ItemDiff {
                part: Part::Policy,
                name: to.name.clone(),
                change: Change::Changed,
                fields: vec!["definition".to_owned()],
                source: Some(policy_line(from)),
                target: Some(policy_line(to)),
            });
        }
    }
    for (list, indexes, change) in
        [(&source.policies, only_source, Change::Added), (&target.policies, only_target, Change::Removed)]
    {
        for at in indexes {
            let policy = &list[at];
            let line = Some(policy_line(policy));
            items.push(ItemDiff {
                part: Part::Policy,
                name: policy.name.clone(),
                change,
                fields: Vec::new(),
                source: if change == Change::Added { line.clone() } else { None },
                target: if change == Change::Removed { line } else { None },
            });
        }
    }
}

fn columns(sides: &Sides<'_>, source: &Object, target: &Object, items: &mut Vec<ItemDiff>) {
    let (pairs, only_source, only_target) =
        pair(&source.columns, &target.columns, |column: &Column| column.name.clone(), |_, _| None);
    for (at, other) in pairs {
        let (from, to) = (&source.columns[at], &target.columns[other]);
        let fields = column_fields(sides, from, to);
        if !fields.is_empty() {
            items.push(ItemDiff {
                part: Part::Column,
                name: to.name.clone(),
                change: Change::Changed,
                fields,
                source: Some(column_line(from)),
                target: Some(column_line(to)),
            });
        }
    }
    for at in only_source {
        let column = &source.columns[at];
        items.push(ItemDiff {
            part: Part::Column,
            name: column.name.clone(),
            change: Change::Added,
            fields: Vec::new(),
            source: Some(column_line(column)),
            target: None,
        });
    }
    for at in only_target {
        let column = &target.columns[at];
        items.push(ItemDiff {
            part: Part::Column,
            name: column.name.clone(),
            change: Change::Removed,
            fields: Vec::new(),
            source: None,
            target: Some(column_line(column)),
        });
    }
}

fn property(items: &mut Vec<ItemDiff>, name: &str, source: Option<String>, target: Option<String>) {
    if source != target {
        items.push(ItemDiff {
            part: Part::Property,
            name: name.to_owned(),
            change: Change::Changed,
            fields: vec![name.to_owned()],
            source,
            target,
        });
    }
}

fn definition(items: &mut Vec<ItemDiff>, field: &str, source: Option<String>, target: Option<String>) {
    items.push(ItemDiff {
        part: Part::Definition,
        name: field.to_owned(),
        change: Change::Changed,
        fields: vec![field.to_owned()],
        source,
        target,
    });
}

fn object_items(sides: &Sides<'_>, source: &Object, target: &Object) -> Vec<ItemDiff> {
    let mut items = Vec::new();
    if source.kind.has_columns() || source.kind == ObjectKind::Type {
        columns(sides, source, target, &mut items);
    }
    constraints(sides, source, target, &mut items);
    indexes(sides, source, target, &mut items);
    triggers(sides, source, target, &mut items);
    policies(sides, source, target, &mut items);
    let short = |text: &Option<String>| text.as_deref().map(normal);
    match source.kind {
        ObjectKind::View | ObjectKind::MaterializedView => {
            if !sides.same_optional(source.definition.as_deref(), target.definition.as_deref()) {
                definition(&mut items, "query", short(&source.definition), short(&target.definition));
            }
        }
        ObjectKind::Package => {
            if !sides.same_optional(source.definition.as_deref(), target.definition.as_deref()) {
                definition(&mut items, "source", short(&source.definition), short(&target.definition));
            }
        }
        ObjectKind::Function | ObjectKind::Procedure => {
            let text = |object: &Object| object.routine.as_ref().and_then(|routine| routine.definition.clone());
            let (from, to) = (text(source), text(target));
            let result = |object: &Object| object.routine.as_ref().and_then(|routine| routine.result.clone());
            if !sides.same_optional(from.as_deref(), to.as_deref()) || result(source) != result(target) {
                definition(&mut items, "source", short(&from), short(&to));
            }
        }
        ObjectKind::Sequence => {
            let mut from = source.sequence.clone();
            let mut to = target.sequence.clone();
            for sequence in [&mut from, &mut to].into_iter().flatten() {
                sequence.owned_by = None;
            }
            if from != to {
                let line = |sequence: &Option<crate::schema::SequenceInfo>| {
                    sequence.as_ref().map(|s| {
                        format!(
                            "{} start {} increment {} min {} max {} cache {}{}",
                            s.data_type,
                            s.start,
                            s.increment,
                            s.min,
                            s.max,
                            s.cache,
                            if s.cycle { " cycle" } else { "" }
                        )
                    })
                };
                definition(&mut items, "sequence", line(&from), line(&to));
            }
        }
        ObjectKind::Type => {
            if source.type_info != target.type_info {
                let line = |object: &Object| {
                    object.type_info.as_ref().map(|info| {
                        let mut line = info.kind.clone();
                        if !info.labels.is_empty() {
                            line.push_str(&format!(" ({})", info.labels.join(", ")));
                        }
                        if let Some(base) = &info.base {
                            line.push_str(&format!(" {base}"));
                        }
                        line
                    })
                };
                definition(&mut items, "type", line(source), line(target));
            }
        }
        ObjectKind::Table | ObjectKind::ForeignTable | ObjectKind::Event => {
            if source.kind == ObjectKind::Event
                && !sides.same_optional(source.definition.as_deref(), target.definition.as_deref())
            {
                definition(&mut items, "source", short(&source.definition), short(&target.definition));
            }
        }
    }
    if !sides.options.ignore_comments {
        property(&mut items, "comment", source.comment.clone(), target.comment.clone());
    }
    if source.kind == ObjectKind::Table {
        let security = |object: &Object| object.row_security.then(|| "on".to_owned());
        property(&mut items, "rowSecurity", security(source), security(target));
        let key = |object: &Object| object.partitioning.as_ref().and_then(|partitioning| partitioning.key.clone());
        property(&mut items, "partitioning", key(source), key(target));
    }
    items
}

fn wanted(options: &CompareOptions, object: &Object) -> bool {
    (options.system || !object.system) && (options.kinds.is_empty() || options.kinds.contains(&object.kind))
}

pub fn compare(source: &Schema, target: &Schema, options: &CompareOptions) -> SchemaDiff {
    let sides = Sides { source: &source.name, target: &target.name, options };
    let from: Vec<&Object> = source.objects.iter().filter(|object| wanted(options, object)).collect();
    let to: Vec<&Object> = target.objects.iter().filter(|object| wanted(options, object)).collect();
    let (pairs, only_source, only_target) =
        pair(&from, &to, |object| format!("{:?}:{}", object.kind, object.key()), |_, _| None);
    let mut diff = SchemaDiff::default();
    for (at, other) in pairs {
        let (source_object, target_object) = (from[at], to[other]);
        if !source_object.detailed || !target_object.detailed {
            diff.partial += 1;
            diff.same += 1;
            continue;
        }
        let items = object_items(&sides, source_object, target_object);
        if items.is_empty() {
            diff.same += 1;
        } else {
            diff.objects.push(ObjectDiff {
                kind: target_object.kind,
                name: target_object.name.clone(),
                key: target_object.key(),
                change: Change::Changed,
                items,
                partial: false,
            });
        }
    }
    for (list, indexes, change) in [(&from, only_source, Change::Added), (&to, only_target, Change::Removed)] {
        for at in indexes {
            let object = list[at];
            diff.objects.push(ObjectDiff {
                kind: object.kind,
                name: object.name.clone(),
                key: object.key(),
                change,
                items: Vec::new(),
                partial: !object.detailed,
            });
        }
    }
    diff.objects.sort_by(|a, b| (a.kind, a.name.to_lowercase(), &a.key).cmp(&(b.kind, b.name.to_lowercase(), &b.key)));
    diff
}

pub fn find<'a>(schema: &'a Schema, kind: ObjectKind, key: &str) -> Option<&'a Object> {
    schema
        .objects
        .iter()
        .find(|object| object.kind == kind && object.key() == key)
        .or_else(|| schema.objects.iter().find(|object| object.kind == kind && object.key().eq_ignore_ascii_case(key)))
}

#[cfg(test)]
mod tests;
