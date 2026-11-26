use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::scope::{Clause, Name, Parsed, Source, Tk, Tok, name_of};
use super::{Catalogue, Reader, Target, resolve_token};
use crate::dialect::SqlDialect;
use crate::filter::{Kind, scan};

const STRUCTURE: &[&str] = &["create", "alter", "drop", "comment", "grant", "revoke", "rename"];

const WRITES: &[&str] = &["insert", "update", "delete", "merge", "replace", "upsert"];

/// How a statement uses what it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// It reads it.
    Read,
    /// It writes it: the table of an INSERT, UPDATE, DELETE or TRUNCATE, a column it sets.
    Write,
}

/// A place of a statement that names what was looked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// The name's bytes in the statement, without its quotes.
    pub range: Range<usize>,
    /// How the statement uses it; none for a statement that changes structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<Access>,
}

fn wanted(target: &Target) -> Option<&str> {
    match target {
        Target::Column { column, .. } => Some(column),
        Target::Object { path } | Target::Schema { path } => {
            path.last().map(|name| name.split_once('(').map_or(name.as_str(), |(plain, _)| plain))
        }
    }
}

fn bare(text: &str, name: &Name) -> Range<usize> {
    if !name.quoted || name.range.len() < 2 {
        return name.range.clone();
    }
    let inner = name.range.start + 1..name.range.end - 1;
    if text.get(inner.clone()) == Some(name.text.as_str()) { inner } else { name.range.clone() }
}

fn holds(text: &str, word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    text.to_ascii_lowercase().contains(&word)
}

fn sets(reader: &Reader<'_, '_>, index: usize) -> bool {
    if reader.symbol(index + 1) != Some("=") {
        return false;
    }
    let mut before = index.checked_sub(1);
    if let Some(dot) = before.filter(|at| reader.symbol(*at) == Some(".")) {
        before = dot.checked_sub(2);
    }
    let Some(before) = before else { return false };
    let tok = reader.parsed.toks[before];
    reader.symbol(before) == Some(",")
        || (tok.kind == Tk::Word && reader.text[tok.start..tok.end].eq_ignore_ascii_case("set"))
}

fn access(reader: &Reader<'_, '_>, index: usize, target: &Target) -> Option<Access> {
    let parsed = reader.parsed;
    let first = parsed.blocks.first().and_then(|block| block.first.as_deref()).unwrap_or_default();
    if STRUCTURE.contains(&first) {
        return None;
    }
    if first == "truncate" {
        return Some(Access::Write);
    }
    if !WRITES.contains(&first) {
        return Some(Access::Read);
    }
    let tok = parsed.toks[index];
    let written = match target {
        Target::Column { .. } => {
            parsed.column_lists.iter().any(|list| list.range.start <= tok.start && tok.end <= list.range.end)
                || (parsed.blocks[parsed.block_at(tok.start)].clause_at(tok.start) == Clause::Set
                    && sets(reader, index))
        }
        Target::Object { .. } => {
            parsed.blocks.first().and_then(|block| block.tables.iter().min_by_key(|table| table.at)).is_some_and(
                |table| match &table.source {
                    Source::Relation(parts) => parts.last().is_some_and(|last| last.range == (tok.start..tok.end)),
                    _ => false,
                },
            )
        }
        Target::Schema { .. } => false,
    };
    Some(if written { Access::Write } else { Access::Read })
}

pub fn usages(text: &str, view: &Catalogue<'_>, target: &Target) -> Vec<Usage> {
    let Some(wanted) = wanted(target) else { return Vec::new() };
    if wanted.is_empty() || !holds(text, wanted) {
        return Vec::new();
    }
    let parsed = Parsed::new(text, view.dialect());
    let reader = Reader { text, parsed: &parsed, view };
    let mut found = Vec::new();
    for (index, tok) in parsed.toks.iter().enumerate() {
        if !matches!(tok.kind, Tk::Word | Tk::Quoted) {
            continue;
        }
        let name = name_of(text, tok);
        if !name.text.eq_ignore_ascii_case(wanted) {
            continue;
        }
        if resolve_token(&reader, index).is_some_and(|resolved| resolved.target == *target) {
            found.push(Usage { range: bare(text, &name), access: access(&reader, index, target) });
        }
    }
    found
}

fn dollar_body(raw: &str) -> Option<Range<usize>> {
    let rest = raw.strip_prefix('$')?;
    let tag = rest.find('$')?;
    let open = tag + 2;
    let close = raw[open..].rfind(&raw[..open]).map(|at| at + open)?;
    (open <= close).then_some(open..close)
}

pub fn mentions(text: &str, dialect: SqlDialect, name: &str, qualifier: Option<&str>) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if !name.is_empty() && holds(text, name) {
        mentions_in(text, 0, dialect, name, qualifier, &mut found);
    }
    found
}

fn mentions_in(
    text: &str,
    base: usize,
    dialect: SqlDialect,
    name: &str,
    qualifier: Option<&str>,
    found: &mut Vec<Range<usize>>,
) {
    let tokens: Vec<_> =
        scan(text, dialect.lexing()).tokens.into_iter().filter(|tok| tok.kind != Kind::Comment).collect();
    let word = |at: usize| {
        tokens.get(at).filter(|tok| matches!(tok.kind, Kind::Word | Kind::Quoted)).map(|tok| {
            let kind = if tok.kind == Kind::Quoted { Tk::Quoted } else { Tk::Word };
            name_of(text, &Tok { kind, start: tok.start, end: tok.end })
        })
    };
    let symbol = |at: usize| tokens.get(at).filter(|tok| tok.kind == Kind::Symbol).map(|tok| &text[tok.start..tok.end]);
    for (index, tok) in tokens.iter().enumerate() {
        if tok.kind == Kind::Literal {
            if let Some(body) = dollar_body(&text[tok.start..tok.end]) {
                let inner = tok.start + body.start..tok.start + body.end;
                mentions_in(&text[inner.clone()], base + inner.start, dialect, name, qualifier, found);
            }
            continue;
        }
        let Some(found_name) = word(index) else { continue };
        if !found_name.text.eq_ignore_ascii_case(name) {
            continue;
        }
        if index >= 2 && symbol(index - 1) == Some(".") {
            let by = word(index - 2);
            if qualifier.is_some_and(|wanted| by.is_none_or(|by| !by.text.eq_ignore_ascii_case(wanted))) {
                continue;
            }
        }
        let range = bare(text, &found_name);
        found.push(base + range.start..base + range.end);
    }
}
