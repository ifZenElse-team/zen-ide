use std::ops::Range;

use crate::dialect::SqlDialect;
use crate::filter::{Kind, scan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tk {
    Word,
    Quoted,
    Number,
    Literal,
    Comment,
    Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Tok {
    pub kind: Tk,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Name {
    pub text: String,
    pub quoted: bool,
    pub range: Range<usize>,
}

impl Name {
    pub(super) fn matches(&self, name: &str) -> bool {
        if self.quoted { self.text == name } else { self.text.eq_ignore_ascii_case(name) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Relation(Vec<Name>),
    Derived(usize),
    Function,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TableRef {
    pub source: Source,
    pub alias: Option<Name>,
    pub at: usize,
    pub ddl: bool,
}

impl TableRef {
    pub(super) fn visible(&self) -> Option<&Name> {
        self.alias.as_ref().or(match &self.source {
            Source::Relation(parts) => parts.last(),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Cte {
    pub name: Name,
    pub columns: Vec<String>,
    pub block: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Clause {
    None,
    Select,
    From,
    On,
    Where,
    GroupBy,
    Having,
    OrderBy,
    Set,
    Values,
    Returning,
    Limit,
    Other,
}

impl Clause {
    pub(super) fn takes_columns(self) -> bool {
        matches!(
            self,
            Clause::Select
                | Clause::On
                | Clause::Where
                | Clause::GroupBy
                | Clause::Having
                | Clause::OrderBy
                | Clause::Set
                | Clause::Returning
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Output {
    Named(String),
    Star(Option<Name>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Block {
    pub range: Range<usize>,
    pub parent: Option<usize>,
    pub tables: Vec<TableRef>,
    pub ctes: Vec<Cte>,
    pub outputs: Vec<Output>,
    pub clauses: Vec<(usize, Clause)>,
    pub parens: Vec<Range<usize>>,
    pub first: Option<String>,
}

impl Block {
    pub(super) fn clause_at(&self, at: usize) -> Clause {
        self.clauses.iter().rev().find(|(start, _)| *start <= at).map_or(Clause::None, |(_, clause)| *clause)
    }

    pub(super) fn in_parens(&self, at: usize) -> bool {
        self.parens.iter().any(|range| range.start <= at && at <= range.end)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ColumnList {
    pub range: Range<usize>,
    pub block: usize,
    pub table: usize,
}

#[derive(Clone, Debug)]
pub(super) struct Parsed {
    pub toks: Vec<Tok>,
    pub all: Vec<Tok>,
    pub unterminated: bool,
    pub blocks: Vec<Block>,
    pub column_lists: Vec<ColumnList>,
    pub created: Vec<Name>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Nothing,
    Table { ddl: bool },
    AfterTable(usize),
    Alias(usize),
    CteName,
    AfterCte(usize),
    CteBody(usize),
    Created,
}

fn is_word(tok: &Tok) -> bool {
    matches!(tok.kind, Tk::Word | Tk::Quoted)
}

const CLAUSE_WORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "group",
    "order",
    "having",
    "limit",
    "offset",
    "fetch",
    "join",
    "left",
    "right",
    "inner",
    "outer",
    "full",
    "cross",
    "natural",
    "on",
    "using",
    "union",
    "intersect",
    "except",
    "set",
    "values",
    "returning",
    "window",
    "for",
    "into",
    "straight_join",
    "prewhere",
    "final",
    "sample",
    "array",
    "global",
    "as",
    "lateral",
    "only",
    "with",
    "format",
    "settings",
    "when",
    "then",
    "else",
    "end",
    "and",
    "or",
    "not",
    "default",
    "do",
    "partition",
    "tablesample",
    "use",
    "force",
    "ignore",
];

pub(super) fn ends_table(word: &str) -> bool {
    CLAUSE_WORDS.contains(&word.to_ascii_lowercase().as_str())
}

impl Parsed {
    pub(super) fn new(text: &str, dialect: SqlDialect) -> Self {
        let scanned = scan(text, dialect.lexing());
        let all: Vec<Tok> = scanned
            .tokens
            .iter()
            .map(|token| {
                let kind = match token.kind {
                    Kind::Word if text.as_bytes()[token.start].is_ascii_digit() => Tk::Number,
                    Kind::Word => Tk::Word,
                    Kind::Quoted => Tk::Quoted,
                    Kind::Literal => Tk::Literal,
                    Kind::Comment => Tk::Comment,
                    Kind::Symbol => Tk::Symbol,
                };
                Tok { kind, start: token.start, end: token.end }
            })
            .collect();
        let toks: Vec<Tok> = all.iter().copied().filter(|tok| tok.kind != Tk::Comment).collect();
        let mut parsed = Parsed {
            toks,
            all,
            unterminated: scanned.unterminated,
            blocks: Vec::new(),
            column_lists: Vec::new(),
            created: Vec::new(),
        };
        let matching = parsed.matching(text);
        let mut reader = Reader { text, matching };
        let count = parsed.toks.len();
        reader.block(&mut parsed, 0..count, None, 0..text.len());
        parsed
    }

    fn matching(&self, text: &str) -> Vec<usize> {
        let mut matching = vec![self.toks.len(); self.toks.len()];
        let mut open: Vec<usize> = Vec::new();
        for (index, tok) in self.toks.iter().enumerate() {
            if tok.kind != Tk::Symbol {
                continue;
            }
            match &text[tok.start..tok.end] {
                "(" => open.push(index),
                ")" => {
                    if let Some(start) = open.pop() {
                        matching[start] = index;
                    }
                }
                _ => {}
            }
        }
        matching
    }

    pub(super) fn block_at(&self, at: usize) -> usize {
        self.blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.range.start <= at && at <= block.range.end)
            .max_by_key(|(_, block)| block.range.start)
            .map_or(0, |(index, _)| index)
    }

    pub(super) fn chain(&self, block: usize) -> Vec<usize> {
        let mut chain = vec![block];
        let mut at = block;
        while let Some(parent) = self.blocks.get(at).and_then(|block| block.parent) {
            chain.push(parent);
            at = parent;
        }
        chain
    }

    pub(super) fn cte(&self, block: usize, name: &Name) -> Option<&Cte> {
        self.chain(block)
            .into_iter()
            .find_map(|at| self.blocks[at].ctes.iter().find(|cte| name.matches(&cte.name.text)))
    }

    pub(super) fn token_at(&self, at: usize) -> Option<usize> {
        self.toks.iter().position(|tok| tok.start <= at && at <= tok.end && is_word(tok))
    }
}

pub(super) fn name_of(text: &str, tok: &Tok) -> Name {
    let raw = &text[tok.start..tok.end];
    if tok.kind != Tk::Quoted {
        return Name { text: raw.to_owned(), quoted: false, range: tok.start..tok.end };
    }
    let open = raw.chars().next().unwrap_or('"');
    let close = if open == '[' { ']' } else { open };
    let inner = raw.strip_prefix(open).unwrap_or(raw);
    let inner = inner.strip_suffix(close).unwrap_or(inner);
    let doubled: String = [close, close].iter().collect();
    let text = if open == '[' { inner.to_owned() } else { inner.replace(&doubled, &close.to_string()) };
    Name { text, quoted: true, range: tok.start..tok.end }
}

struct Reader<'t> {
    text: &'t str,
    matching: Vec<usize>,
}

impl Reader<'_> {
    fn word(&self, parsed: &Parsed, index: usize) -> Option<String> {
        let tok = parsed.toks.get(index)?;
        (tok.kind == Tk::Word).then(|| self.text[tok.start..tok.end].to_ascii_lowercase())
    }

    fn symbol(&self, parsed: &Parsed, index: usize) -> Option<&str> {
        let tok = parsed.toks.get(index)?;
        (tok.kind == Tk::Symbol).then(|| &self.text[tok.start..tok.end])
    }

    fn starts_query(&self, parsed: &Parsed, index: usize) -> bool {
        matches!(self.word(parsed, index).as_deref(), Some("select" | "with" | "values" | "table"))
    }

    fn bytes_inside(&self, parsed: &Parsed, open: usize, close: usize) -> Range<usize> {
        let start = parsed.toks[open].end;
        let end = parsed.toks.get(close).map_or(self.text.len(), |tok| tok.start);
        start..end.max(start)
    }

    fn name_parts(&self, parsed: &Parsed, mut index: usize, end: usize) -> (Vec<Name>, usize) {
        let mut parts = vec![name_of(self.text, &parsed.toks[index])];
        while index + 2 < end
            && self.symbol(parsed, index + 1) == Some(".")
            && parsed.toks.get(index + 2).is_some_and(is_word)
        {
            index += 2;
            parts.push(name_of(self.text, &parsed.toks[index]));
        }
        if index + 1 < end && self.symbol(parsed, index + 1) == Some(".") {
            index += 1;
        }
        (parts, index + 1)
    }

    fn block(&mut self, parsed: &mut Parsed, range: Range<usize>, parent: Option<usize>, bytes: Range<usize>) -> usize {
        let id = parsed.blocks.len();
        parsed.blocks.push(Block {
            range: bytes,
            parent,
            tables: Vec::new(),
            ctes: Vec::new(),
            outputs: Vec::new(),
            clauses: Vec::new(),
            parens: Vec::new(),
            first: None,
        });
        let mut expect = Expect::Nothing;
        let mut clause = Clause::None;
        let mut outputs_done = false;
        let mut item_last: Option<usize> = None;
        let mut item_before_last: Option<usize> = None;
        let mut previous: Option<String> = None;
        let mut i = range.start;
        while i < range.end {
            let tok = parsed.toks[i];
            if tok.kind == Tk::Symbol && &self.text[tok.start..tok.end] == "(" {
                let close = self.matching[i].min(range.end);
                let inside = self.bytes_inside(parsed, i, close);
                if self.starts_query(parsed, i + 1) {
                    let child = self.block(parsed, i + 1..close, Some(id), inside.clone());
                    match expect {
                        Expect::Table { ddl } => {
                            parsed.blocks[id].tables.push(TableRef {
                                source: Source::Derived(child),
                                alias: None,
                                at: tok.start,
                                ddl,
                            });
                            expect = Expect::AfterTable(parsed.blocks[id].tables.len() - 1);
                        }
                        Expect::CteBody(cte) => {
                            if let Some(cte) = parsed.blocks[id].ctes.get_mut(cte) {
                                cte.block = Some(child);
                            }
                            expect = Expect::Nothing;
                        }
                        _ => {
                            parsed.blocks[id].parens.push(inside);
                            expect = Expect::Nothing;
                        }
                    }
                } else {
                    match expect {
                        Expect::AfterTable(table) => {
                            let insert = parsed.blocks[id].tables[table].ddl
                                || parsed.blocks[id]
                                    .first
                                    .as_deref()
                                    .is_some_and(|first| matches!(first, "insert" | "replace" | "merge"));
                            if insert {
                                parsed.column_lists.push(ColumnList { range: inside.clone(), block: id, table });
                                expect = Expect::Nothing;
                            } else {
                                parsed.blocks[id].tables[table].source = Source::Function;
                                expect = Expect::AfterTable(table);
                            }
                            self.nested(parsed, i + 1..close, id);
                        }
                        Expect::AfterCte(cte) => {
                            let columns: Vec<String> = (i + 1..close)
                                .filter(|at| parsed.toks.get(*at).is_some_and(is_word))
                                .map(|at| name_of(self.text, &parsed.toks[at]).text)
                                .collect();
                            if let Some(cte) = parsed.blocks[id].ctes.get_mut(cte) {
                                cte.columns = columns;
                            }
                        }
                        Expect::CteBody(_) => expect = Expect::Nothing,
                        _ => {
                            if clause != Clause::From || !matches!(expect, Expect::Table { .. }) {
                                expect = Expect::Nothing;
                            }
                            self.nested(parsed, i + 1..close, id);
                        }
                    }
                    parsed.blocks[id].parens.push(inside);
                }
                item_before_last = item_last;
                item_last = Some(i);
                i = close + 1;
                continue;
            }
            let word = self.word(parsed, i);
            if parsed.blocks[id].first.is_none()
                && let Some(word) = &word
            {
                parsed.blocks[id].first = Some(word.clone());
            }
            if clause == Clause::Select && !outputs_done {
                let ends_item = tok.kind == Tk::Symbol && &self.text[tok.start..tok.end] == ",";
                let ends_list = word.as_deref().is_some_and(|word| {
                    matches!(
                        word,
                        "from"
                            | "into"
                            | "where"
                            | "group"
                            | "order"
                            | "having"
                            | "limit"
                            | "union"
                            | "except"
                            | "intersect"
                            | "window"
                    )
                });
                if ends_item || ends_list {
                    self.output(parsed, id, item_last, item_before_last);
                    item_last = None;
                    item_before_last = None;
                    if ends_list {
                        outputs_done = true;
                    }
                } else if !matches!(word.as_deref(), Some("select" | "distinct" | "all")) || item_last.is_some() {
                    item_before_last = item_last;
                    item_last = Some(i);
                }
            }
            match expect {
                Expect::Table { ddl } if is_word(&tok) => {
                    let lower = word.clone().unwrap_or_default();
                    if matches!(lower.as_str(), "only" | "lateral" | "if" | "not" | "exists" | "table") {
                        previous = word;
                        i += 1;
                        continue;
                    }
                    if tok.kind == Tk::Quoted || !ends_table(&lower) {
                        let (parts, next) = self.name_parts(parsed, i, range.end);
                        parsed.blocks[id].tables.push(TableRef {
                            source: Source::Relation(parts),
                            alias: None,
                            at: tok.start,
                            ddl,
                        });
                        expect = Expect::AfterTable(parsed.blocks[id].tables.len() - 1);
                        previous = None;
                        item_before_last = item_last;
                        item_last = Some(next - 1);
                        i = next;
                        continue;
                    }
                    expect = Expect::Nothing;
                }
                Expect::AfterTable(table) | Expect::Alias(table) if is_word(&tok) => {
                    let lower = word.clone().unwrap_or_default();
                    if lower == "as" {
                        expect = Expect::Alias(table);
                        previous = word;
                        i += 1;
                        continue;
                    }
                    if tok.kind == Tk::Quoted || !ends_table(&lower) {
                        parsed.blocks[id].tables[table].alias = Some(name_of(self.text, &tok));
                        expect = Expect::Nothing;
                        previous = None;
                        i += 1;
                        continue;
                    }
                    expect = Expect::Nothing;
                }
                Expect::CteName if is_word(&tok) => {
                    if word.as_deref() == Some("recursive") {
                        i += 1;
                        continue;
                    }
                    parsed.blocks[id].ctes.push(Cte {
                        name: name_of(self.text, &tok),
                        columns: Vec::new(),
                        block: None,
                    });
                    expect = Expect::AfterCte(parsed.blocks[id].ctes.len() - 1);
                    i += 1;
                    continue;
                }
                Expect::AfterCte(cte) if word.as_deref() == Some("as") => {
                    expect = Expect::CteBody(cte);
                    i += 1;
                    continue;
                }
                Expect::CteBody(_) if matches!(word.as_deref(), Some("materialized" | "not")) => {
                    i += 1;
                    continue;
                }
                Expect::Created if is_word(&tok) => {
                    let lower = word.clone().unwrap_or_default();
                    if !matches!(lower.as_str(), "if" | "not" | "exists") {
                        let (parts, next) = self.name_parts(parsed, i, range.end);
                        if let Some(last) = parts.last() {
                            parsed.created.push(last.clone());
                        }
                        expect = Expect::Nothing;
                        i = next;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                _ => {}
            }
            if tok.kind == Tk::Symbol && &self.text[tok.start..tok.end] == "," {
                match clause {
                    Clause::From => expect = Expect::Table { ddl: false },
                    _ if parsed.blocks[id].ctes.last().is_some_and(|cte| cte.block.is_some())
                        && clause == Clause::None =>
                    {
                        expect = Expect::CteName;
                    }
                    _ => {}
                }
            }
            if let Some(lower) = word.as_deref() {
                let next = self.word(parsed, i + 1);
                let first = parsed.blocks[id].first.clone().unwrap_or_default();
                let set = |clause_now: Clause, clause: &mut Clause, at: usize, parsed: &mut Parsed| {
                    *clause = clause_now;
                    parsed.blocks[id].clauses.push((at, clause_now));
                };
                match lower {
                    "select" => set(Clause::Select, &mut clause, tok.start, parsed),
                    "from" => {
                        let distinct = previous.as_deref() == Some("distinct");
                        if !distinct {
                            set(Clause::From, &mut clause, tok.start, parsed);
                            expect = Expect::Table { ddl: false };
                        }
                    }
                    "join" | "straight_join" => {
                        set(Clause::From, &mut clause, tok.start, parsed);
                        expect = Expect::Table { ddl: false };
                    }
                    "on" => {
                        if matches!(next.as_deref(), Some("conflict" | "duplicate")) {
                            set(Clause::Other, &mut clause, tok.start, parsed);
                        } else if matches!(first.as_str(), "create" | "grant" | "revoke" | "comment" | "alter") {
                            expect = Expect::Table { ddl: true };
                        } else {
                            set(Clause::On, &mut clause, tok.start, parsed);
                        }
                    }
                    "using" => {
                        if self.symbol(parsed, i + 1) == Some("(") {
                            set(Clause::On, &mut clause, tok.start, parsed);
                        } else if first == "delete" {
                            set(Clause::From, &mut clause, tok.start, parsed);
                            expect = Expect::Table { ddl: false };
                        }
                    }
                    "where" => set(Clause::Where, &mut clause, tok.start, parsed),
                    "prewhere" => set(Clause::Where, &mut clause, tok.start, parsed),
                    "group" => set(Clause::GroupBy, &mut clause, tok.start, parsed),
                    "having" => set(Clause::Having, &mut clause, tok.start, parsed),
                    "order" => set(Clause::OrderBy, &mut clause, tok.start, parsed),
                    "limit" | "offset" | "fetch" => set(Clause::Limit, &mut clause, tok.start, parsed),
                    "for" if clause != Clause::Other => set(Clause::Limit, &mut clause, tok.start, parsed),
                    "returning" => set(Clause::Returning, &mut clause, tok.start, parsed),
                    "values" => set(Clause::Values, &mut clause, tok.start, parsed),
                    "window" => set(Clause::Other, &mut clause, tok.start, parsed),
                    "union" | "intersect" | "except" => set(Clause::None, &mut clause, tok.start, parsed),
                    "set" if first == "update" || clause == Clause::Other => {
                        set(Clause::Set, &mut clause, tok.start, parsed)
                    }
                    "update" => {
                        if first == "update" && i == range.start {
                            expect = Expect::Table { ddl: false };
                        } else if previous.as_deref() == Some("key") {
                            set(Clause::Set, &mut clause, tok.start, parsed);
                        }
                    }
                    "into" if previous.as_deref() != Some("select") && clause != Clause::Select => {
                        expect = Expect::Table { ddl: false };
                    }
                    "table" => {
                        let creates = matches!(first.as_str(), "create") && clause != Clause::Other;
                        if creates {
                            expect = Expect::Created;
                        } else if i == range.start
                            || matches!(
                                first.as_str(),
                                "alter"
                                    | "drop"
                                    | "truncate"
                                    | "lock"
                                    | "comment"
                                    | "analyze"
                                    | "optimize"
                                    | "describe"
                            )
                        {
                            expect = Expect::Table { ddl: i != range.start };
                        }
                    }
                    "view" if first == "create" => expect = Expect::Created,
                    "view" if matches!(first.as_str(), "alter" | "drop" | "comment" | "refresh") => {
                        expect = Expect::Table { ddl: true }
                    }
                    "truncate" | "describe" | "desc" if i == range.start => {
                        if next.as_deref() != Some("table") {
                            expect = Expect::Table { ddl: true };
                        }
                    }
                    "with" if i == range.start || clause == Clause::None => {
                        if !matches!(
                            next.as_deref(),
                            Some("totals" | "time" | "ordinality" | "check" | "grant" | "data" | "no")
                        ) {
                            expect = Expect::CteName;
                        }
                    }
                    "as" if clause == Clause::None && first == "create" => {
                        set(Clause::None, &mut clause, tok.start, parsed);
                    }
                    "do" if previous.as_deref() == Some("nothing") || next.as_deref() == Some("update") => {}
                    _ => {}
                }
            }
            previous = word.or_else(|| (tok.kind == Tk::Symbol).then(|| self.text[tok.start..tok.end].to_owned()));
            if clause == Clause::Select && outputs_done {
                item_last = None;
            }
            i += 1;
        }
        if clause == Clause::Select && !outputs_done {
            self.output(parsed, id, item_last, item_before_last);
        }
        id
    }

    fn nested(&mut self, parsed: &mut Parsed, range: Range<usize>, block: usize) {
        let mut i = range.start;
        while i < range.end {
            let tok = parsed.toks[i];
            if tok.kind == Tk::Symbol && &self.text[tok.start..tok.end] == "(" {
                let close = self.matching[i].min(range.end);
                let inside = self.bytes_inside(parsed, i, close);
                if self.starts_query(parsed, i + 1) {
                    self.block(parsed, i + 1..close, Some(block), inside);
                } else {
                    self.nested(parsed, i + 1..close, block);
                }
                i = close + 1;
                continue;
            }
            i += 1;
        }
    }

    fn output(&self, parsed: &mut Parsed, block: usize, last: Option<usize>, before: Option<usize>) {
        let Some(last) = last else { return };
        let tok = parsed.toks[last];
        let text = &self.text[tok.start..tok.end];
        let output = if tok.kind == Tk::Symbol && text == "*" {
            let qualifier = before
                .filter(|before| *before + 1 == last && self.symbol(parsed, *before) == Some("."))
                .and_then(|dot| dot.checked_sub(1))
                .and_then(|at| parsed.toks.get(at).filter(|tok| is_word(tok)).map(|tok| name_of(self.text, tok)));
            Output::Star(qualifier)
        } else if is_word(&tok) {
            let name = name_of(self.text, &tok);
            if tok.kind == Tk::Word && words_reserved(&name.text) {
                return;
            }
            Output::Named(name.text)
        } else {
            return;
        };
        parsed.blocks[block].outputs.push(output);
    }
}

fn words_reserved(word: &str) -> bool {
    super::words::is_reserved(word)
}
