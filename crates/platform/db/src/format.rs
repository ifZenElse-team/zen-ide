use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::{Lexing, SqlDialect};
use crate::filter::{Kind, Token, scan};
use crate::sql::{Dialect, split};

/// How reserved keywords are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KeywordCase {
    /// As they are typed.
    #[default]
    AsIs,
    /// In capitals.
    Upper,
    /// In small letters.
    Lower,
}

/// Where the comma between list items goes when each item has a line of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Commas {
    /// At the end of the item's line.
    #[default]
    Trailing,
    /// At the start of the next item's line.
    Leading,
}

/// The SQL options of Code Style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatOptions {
    /// How reserved keywords are written (as typed by default).
    pub keyword_case: KeywordCase,
    /// Where commas between list items go (trailing by default).
    pub commas: Commas,
    /// Whether a clause keyword (`SELECT`, `WHERE`) stands on its own line with its items below it (off by default).
    pub clause_on_own_line: bool,
    /// Whether each item of a select list, `GROUP BY`, `ORDER BY`, `SET` or `VALUES` has a line of its own (on by default).
    pub item_per_line: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::AsIs,
            commas: Commas::Trailing,
            clause_on_own_line: false,
            item_per_line: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flavor {
    pub dialect: Dialect,
    pub lexing: Lexing,
}

impl Flavor {
    pub fn of_language(language: &str) -> Self {
        let dialect = Dialect::of_language(language);
        match SqlDialect::of_language(language) {
            Some(sql) => Self { dialect, lexing: sql.lexing() },
            None => Self::of_dialect(dialect),
        }
    }

    pub fn of_dialect(dialect: Dialect) -> Self {
        let lexing = match dialect {
            Dialect::Postgres => SqlDialect::Postgres.lexing(),
            Dialect::MySql => SqlDialect::MySql.lexing(),
            Dialect::Oracle => SqlDialect::Oracle.lexing(),
            Dialect::SqlServer => Lexing {
                backticks: false,
                brackets: true,
                double_quoted_strings: false,
                backslashes: false,
                dollar_quotes: false,
                hash_comments: false,
            },
            Dialect::Standard | Dialect::Lines | Dialect::Script => Lexing {
                backticks: true,
                brackets: true,
                double_quoted_strings: false,
                backslashes: false,
                dollar_quotes: false,
                hash_comments: false,
            },
        };
        Self { dialect, lexing }
    }

    pub fn of_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "pgsql" | "psql" => Self::of_dialect(Dialect::Postgres),
            "mysql" => Self::of_dialect(Dialect::MySql),
            _ => Self::of_dialect(Dialect::Standard),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Formatted {
    pub edits: Vec<(Range<usize>, String)>,
    pub statements: usize,
    pub kept: usize,
}

impl Formatted {
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for (range, replacement) in &self.edits {
            out.push_str(&text[at..range.start]);
            out.push_str(replacement);
            at = range.end;
        }
        out.push_str(&text[at..]);
        out
    }
}

pub fn format(
    text: &str,
    flavor: Flavor,
    options: &FormatOptions,
    indent: &str,
    within: Option<Range<usize>>,
    cancelled: &dyn Fn() -> bool,
) -> Option<Formatted> {
    let mut formatted = Formatted::default();
    if flavor.dialect == Dialect::Lines {
        return Some(formatted);
    }
    let (mut line_start, mut scanned) = (0usize, 0usize);
    for statement in split(text, flavor.dialect) {
        if statement.client {
            continue;
        }
        if let Some(within) = &within
            && (statement.end < within.start || statement.start > within.end)
        {
            continue;
        }
        if cancelled() {
            return None;
        }
        formatted.statements += 1;
        let range = statement.range();
        let original = &text[range.clone()];
        if let Some(at) = text[scanned..range.start].rfind('\n') {
            line_start = scanned + at + 1;
        }
        scanned = range.start;
        let prefix = &text[line_start..range.start];
        if flavor.dialect == Dialect::Standard && original.contains(DIALECT_MARKS) {
            formatted.kept += 1;
            continue;
        }
        let base = if prefix.chars().all(|c| c == ' ' || c == '\t') { prefix } else { "" };
        match format_statement(original, flavor.lexing, options, base, indent) {
            Some(new) if new != original => formatted.edits.push((range, new)),
            Some(_) => {}
            None => formatted.kept += 1,
        }
    }
    Some(formatted)
}

const DIALECT_MARKS: [char; 4] = ['$', '#', '\\', '`'];

pub fn format_text(text: &str, flavor: Flavor, options: &FormatOptions, indent: &str) -> String {
    format(text, flavor, options, indent, None, &|| false).map_or_else(|| text.to_owned(), |done| done.apply(text))
}

const RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BETWEEN",
    "BOTH",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "LATERAL",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "NULLS",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PRIMARY",
    "RECURSIVE",
    "REFERENCES",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "SELECT",
    "SET",
    "SOME",
    "TABLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

const STARTS: &[&str] = &[
    "ANALYZE",
    "BEGIN",
    "CALL",
    "COMMIT",
    "COPY",
    "DESCRIBE",
    "EXPLAIN",
    "PRAGMA",
    "RELEASE",
    "ROLLBACK",
    "SAVEPOINT",
    "SHOW",
    "START",
    "TRUNCATE",
    "USE",
    "VACUUM",
];

const BEFORE_PAREN: &[&str] = &[
    "ALL",
    "AND",
    "ANY",
    "AS",
    "CHECK",
    "EXISTS",
    "FROM",
    "IN",
    "INTO",
    "JOIN",
    "KEY",
    "NOT",
    "ON",
    "OR",
    "OVER",
    "REFERENCES",
    "SELECT",
    "SOME",
    "TABLE",
    "THEN",
    "UNIQUE",
    "USING",
    "VALUES",
    "WHEN",
    "ELSE",
    "WHERE",
    "WITH",
    "UNION",
    "CAST",
    "LATERAL",
];

const JOINS: &[&str] = &["NATURAL", "INNER", "LEFT", "RIGHT", "FULL", "CROSS", "OUTER"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gap {
    None,
    Space,
    Line,
}

struct Tok<'a> {
    kind: Kind,
    text: &'a str,
    upper: String,
    gap: Gap,
    gap_after: Gap,
}

impl Tok<'_> {
    fn is(&self, word: &str) -> bool {
        self.kind == Kind::Word && self.upper == word
    }

    fn symbol(&self, symbol: &str) -> bool {
        self.kind == Kind::Symbol && self.text == symbol
    }

    fn line_comment(&self) -> bool {
        self.kind == Kind::Comment && !self.text.starts_with("/*")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clause {
    None,
    List,
    Ctes,
    Conditions,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Block,
    List,
    Inline,
}

#[derive(Clone, Copy, Debug)]
struct Ctx {
    frame: Frame,
    level: usize,
    close: usize,
    clause: Clause,
    cases: usize,
    between: bool,
    first: bool,
    item_break: bool,
}

impl Ctx {
    fn new(frame: Frame, level: usize, close: usize) -> Self {
        Self { frame, level, close, clause: Clause::None, cases: 0, between: false, first: true, item_break: false }
    }

    fn continuation(&self) -> usize {
        match self.frame {
            Frame::Block if self.first || self.clause == Clause::Ctes => self.level,
            Frame::Block => self.level + 1,
            Frame::List | Frame::Inline => self.level,
        }
    }
}

fn gap(text: &str) -> Gap {
    if text.is_empty() {
        Gap::None
    } else if text.contains('\n') {
        Gap::Line
    } else {
        Gap::Space
    }
}

fn tokens<'a>(statement: &'a str, tokens: &[Token]) -> Vec<Tok<'a>> {
    let mut out: Vec<Tok<'a>> = Vec::with_capacity(tokens.len());
    let mut previous = 0;
    for (index, token) in tokens.iter().enumerate() {
        let text = &statement[token.start..token.end];
        let next = tokens.get(index + 1).map_or(statement.len(), |next| next.start);
        out.push(Tok {
            kind: token.kind,
            text,
            upper: if token.kind == Kind::Word { text.to_ascii_uppercase() } else { String::new() },
            gap: gap(&statement[previous..token.start]),
            gap_after: gap(&statement[token.end..next]),
        });
        previous = token.end;
    }
    out
}

fn cased(tok: &Tok<'_>, toks: &[Tok<'_>], index: usize, case: KeywordCase) -> Option<String> {
    if case == KeywordCase::AsIs || tok.kind != Kind::Word {
        return None;
    }
    let first = toks[..index].iter().all(|tok| tok.kind == Kind::Comment);
    let key = tok.upper == "KEY"
        && toks[..index]
            .iter()
            .rev()
            .find(|tok| tok.kind != Kind::Comment)
            .is_some_and(|tok| tok.is("PRIMARY") || tok.is("FOREIGN"));
    if !RESERVED.contains(&tok.upper.as_str()) && !(first && STARTS.contains(&tok.upper.as_str())) && !key {
        return None;
    }
    let previous = index.checked_sub(1).map(|at| &toks[at]);
    if tok.gap == Gap::None
        && previous.is_some_and(|previous| {
            previous.kind == Kind::Symbol && matches!(previous.text, "." | "@" | ":" | "$" | "?")
        })
    {
        return None;
    }
    if let Some(next) = toks.get(index + 1)
        && tok.gap_after == Gap::None
        && (next.symbol(".") || (next.symbol("(") && !BEFORE_PAREN.contains(&tok.upper.as_str())))
    {
        return None;
    }
    let text = match case {
        KeywordCase::Upper => tok.text.to_ascii_uppercase(),
        KeywordCase::Lower => tok.text.to_ascii_lowercase(),
        KeywordCase::AsIs => return None,
    };
    (text != tok.text).then_some(text)
}

fn laid_out(toks: &[Tok<'_>]) -> bool {
    let Some(first) = toks.iter().find(|tok| tok.kind != Kind::Comment) else { return false };
    if first.kind != Kind::Word {
        return false;
    }
    if toks.iter().any(|tok| {
        tok.kind == Kind::Word
            && matches!(tok.upper.as_str(), "BEGIN" | "DECLARE" | "LOOP" | "DELIMITER" | "GO" | "ELSIF")
    }) {
        return false;
    }
    match first.upper.as_str() {
        "SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES" | "REPLACE" => true,
        "CREATE" => matches!(created(toks), Some("TABLE" | "VIEW")),
        _ => false,
    }
}

fn created<'t>(toks: &'t [Tok<'_>]) -> Option<&'t str> {
    let mut words = toks.iter().filter(|tok| tok.kind == Kind::Word).map(|tok| tok.upper.as_str());
    if words.next() != Some("CREATE") {
        return None;
    }
    words.find(|word| {
        !matches!(
            *word,
            "OR" | "REPLACE" | "TEMP" | "TEMPORARY" | "UNLOGGED" | "MATERIALIZED" | "GLOBAL" | "LOCAL" | "RECURSIVE"
        )
    })
}

struct Writer<'o> {
    out: String,
    base: &'o str,
    unit: &'o str,
    line_level: usize,
    line_start: bool,
}

impl Writer<'_> {
    fn newline(&mut self, level: usize) {
        if self.line_start {
            let keep = self.out.rfind('\n').map_or(0, |at| at + 1);
            self.out.truncate(keep);
        } else {
            let trimmed = self.out.trim_end_matches([' ', '\t']).len();
            self.out.truncate(trimmed);
            self.out.push('\n');
        }
        self.out.push_str(self.base);
        for _ in 0..level {
            self.out.push_str(self.unit);
        }
        self.line_level = level;
        self.line_start = true;
    }

    fn space(&mut self) {
        if !self.line_start && !self.out.ends_with([' ', '\t', '\n']) && !self.out.is_empty() {
            self.out.push(' ');
        }
    }

    fn push(&mut self, text: &str) {
        self.out.push_str(text);
        self.line_start = false;
    }
}

struct Layout<'t, 'a> {
    toks: &'t [Tok<'a>],
    options: &'t FormatOptions,
    create_table: bool,
    stack: Vec<Ctx>,
}

impl Layout<'_, '_> {
    fn ctx(&self) -> &Ctx {
        self.stack.last().expect("the statement's frame")
    }

    fn previous_word(&self, index: usize) -> Option<&str> {
        self.toks[..index]
            .iter()
            .rev()
            .find(|tok| tok.kind != Kind::Comment)
            .filter(|tok| tok.kind == Kind::Word)
            .map(|tok| tok.upper.as_str())
    }

    fn next_code(&self, index: usize) -> Option<usize> {
        (index + 1..self.toks.len()).find(|at| self.toks[*at].kind != Kind::Comment)
    }

    fn next_word(&self, index: usize) -> Option<&str> {
        self.next_code(index)
            .map(|at| &self.toks[at])
            .filter(|tok| tok.kind == Kind::Word)
            .map(|tok| tok.upper.as_str())
    }

    fn joins_ahead(&self, index: usize) -> bool {
        let mut at = index;
        loop {
            let tok = &self.toks[at];
            if tok.kind != Kind::Word {
                return false;
            }
            if tok.upper == "JOIN" || tok.upper == "STRAIGHT_JOIN" {
                return true;
            }
            if !JOINS.contains(&tok.upper.as_str()) {
                return false;
            }
            match self.next_code(at) {
                Some(next) => at = next,
                None => return false,
            }
        }
    }

    fn clause_at(&self, index: usize) -> Option<Clause> {
        let ctx = self.ctx();
        let tok = &self.toks[index];
        if ctx.frame != Frame::Block || ctx.cases > 0 || tok.kind != Kind::Word {
            return None;
        }
        if index > 0 && tok.gap == Gap::None && !matches!(self.toks[index - 1].text, "(" | ")" | ",") {
            return None;
        }
        let previous = self.previous_word(index);
        let clause = match tok.upper.as_str() {
            "WITH" if ctx.first => Clause::Ctes,
            "SELECT" | "WINDOW" | "RETURNING" => Clause::List,
            "VALUES"
                if previous != Some("DEFAULT")
                    && !self.toks[..index]
                        .iter()
                        .rev()
                        .find(|tok| tok.kind != Kind::Comment)
                        .is_some_and(|tok| tok.kind == Kind::Symbol && !matches!(tok.text, "(" | ")")) =>
            {
                Clause::List
            }
            "SET" if !matches!(previous, Some("CHARACTER" | "CHARSET" | "UPDATE")) => Clause::List,
            "FROM" if !matches!(previous, Some("DISTINCT" | "DELETE")) => Clause::List,
            "WHERE" | "HAVING" => Clause::Conditions,
            "GROUP" | "ORDER" if self.next_word(index) == Some("BY") => Clause::List,
            "LIMIT" | "OFFSET" | "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" | "INSERT" | "DELETE" | "REPLACE" => {
                Clause::Other
            }
            "FETCH" if matches!(self.next_word(index), Some("FIRST" | "NEXT")) => Clause::Other,
            "FOR" if matches!(self.next_word(index), Some("UPDATE" | "SHARE" | "NO" | "KEY")) => Clause::Other,
            "UPDATE" if !matches!(previous, Some("FOR" | "DO" | "KEY" | "NO")) => Clause::Other,
            "ON" if matches!(self.next_word(index), Some("CONFLICT" | "DUPLICATE")) => Clause::Other,
            "JOIN" | "STRAIGHT_JOIN" if !previous.is_some_and(|word| JOINS.contains(&word)) => Clause::Other,
            word if JOINS.contains(&word)
                && !previous.is_some_and(|word| JOINS.contains(&word))
                && self.joins_ahead(index) =>
            {
                Clause::Other
            }
            _ => return None,
        };
        Some(clause)
    }

    fn break_at(&self, index: usize) -> Option<usize> {
        let ctx = self.ctx();
        let tok = &self.toks[index];
        if self.clause_at(index).is_some() {
            return (!ctx.first).then_some(ctx.level);
        }
        if ctx.frame == Frame::Block
            && ctx.cases == 0
            && ctx.clause == Clause::Conditions
            && !ctx.between
            && (tok.is("AND") || tok.is("OR"))
            && tok.gap != Gap::None
        {
            return Some(ctx.level + 1);
        }
        if tok.symbol(")") && matches!(ctx.frame, Frame::Block | Frame::List) && self.stack.len() > 1 {
            return Some(ctx.close);
        }
        if tok.symbol(",") && self.options.commas == Commas::Leading && self.splits_items() {
            return Some(ctx.continuation());
        }
        None
    }

    fn splits_items(&self) -> bool {
        let ctx = self.ctx();
        match ctx.frame {
            Frame::List => true,
            Frame::Block => {
                ctx.cases == 0
                    && (ctx.clause == Clause::Ctes || (ctx.clause == Clause::List && self.options.item_per_line))
            }
            Frame::Inline => false,
        }
    }

    fn opens(&self, index: usize) -> Frame {
        match self.next_code(index).map(|at| &self.toks[at]) {
            Some(next) if next.is("SELECT") || next.is("WITH") || next.is("VALUES") => return Frame::Block,
            _ => {}
        }
        let ctx = self.ctx();
        if self.create_table && self.stack.len() == 1 && ctx.frame == Frame::Block {
            return Frame::List;
        }
        Frame::Inline
    }
}

fn format_statement(
    statement: &str,
    lexing: Lexing,
    options: &FormatOptions,
    base: &str,
    unit: &str,
) -> Option<String> {
    let scanned = scan(statement, lexing);
    if scanned.unterminated {
        return None;
    }
    let mut open = Vec::new();
    for token in scanned.tokens.iter().filter(|token| token.kind == Kind::Symbol) {
        match &statement[token.start..token.end] {
            "(" => open.push(')'),
            "[" => open.push(']'),
            ")" if open.pop() != Some(')') => return None,
            "]" if open.pop() != Some(']') => return None,
            _ => {}
        }
    }
    if !open.is_empty() {
        return None;
    }
    let toks = tokens(statement, &scanned.tokens);
    let out = if laid_out(&toks) {
        lay_out(&toks, options, base, unit)
    } else {
        recase(statement, &scanned.tokens, &toks, options.keyword_case)
    };
    same_tokens(statement, &out, lexing).then_some(out)
}

fn recase(statement: &str, tokens: &[Token], toks: &[Tok<'_>], case: KeywordCase) -> String {
    let mut out = String::with_capacity(statement.len());
    let mut at = 0;
    for (index, (token, tok)) in tokens.iter().zip(toks).enumerate() {
        if let Some(text) = cased(tok, toks, index, case) {
            out.push_str(&statement[at..token.start]);
            out.push_str(&text);
            at = token.end;
        }
    }
    out.push_str(&statement[at..]);
    out
}

fn lay_out(toks: &[Tok<'_>], options: &FormatOptions, base: &str, unit: &str) -> String {
    let create_table = created(toks) == Some("TABLE");
    let mut layout = Layout { toks, options, create_table, stack: vec![Ctx::new(Frame::Block, 0, 0)] };
    let mut writer = Writer { out: String::new(), base, unit, line_level: 0, line_start: true };
    let mut pending: Option<usize> = None;
    for (index, tok) in toks.iter().enumerate() {
        let text = cased(tok, toks, index, options.keyword_case).unwrap_or_else(|| tok.text.to_owned());
        if tok.kind == Kind::Comment {
            if index > 0 && tok.gap == Gap::Line {
                let level = layout
                    .next_code(index)
                    .and_then(|next| layout.break_at(next))
                    .unwrap_or_else(|| pending.unwrap_or_else(|| layout.ctx().continuation()));
                writer.newline(level);
            } else if index > 0 && tok.gap != Gap::None {
                writer.space();
            }
            writer.push(&text);
            if tok.line_comment() || tok.gap_after == Gap::Line {
                pending = Some(pending.unwrap_or_else(|| layout.ctx().continuation()));
            }
            continue;
        }
        let clause = layout.clause_at(index);
        let breaks = layout.break_at(index);
        let previous = index.checked_sub(1).map(|at| &toks[at]);
        let item_break = {
            let ctx = layout.ctx();
            ctx.item_break
                && clause.is_none()
                && !matches!(tok.upper.as_str(), "BY" | "DISTINCT" | "ALL" | "RECURSIVE" | "JOIN" | "OUTER")
        };
        if let Some(level) = breaks {
            writer.newline(level);
        } else if item_break {
            writer.newline(layout.ctx().level + 1);
        } else if let Some(level) = pending {
            writer.newline(level);
        } else {
            let joined = index == 0
                || writer.line_start
                || previous.is_some_and(|previous| previous.symbol("("))
                || tok.symbol(")")
                || tok.symbol(",");
            if !joined && (previous.is_some_and(|previous| previous.symbol(",")) || tok.gap != Gap::None) {
                writer.space();
            }
        }
        pending = None;
        if item_break {
            layout.stack.last_mut().expect("frame").item_break = false;
        }
        if tok.symbol("(") {
            let frame = layout.opens(index);
            writer.push(&text);
            let line = writer.line_level;
            match frame {
                Frame::Block => {
                    layout.stack.push(Ctx::new(Frame::Block, line + 1, line));
                    pending = Some(line + 1);
                }
                Frame::List => {
                    layout.stack.push(Ctx::new(Frame::List, line + 1, line));
                    pending = Some(line + 1);
                }
                Frame::Inline => layout.stack.push(Ctx::new(Frame::Inline, line + 1, line)),
            }
            continue;
        }
        if tok.symbol("[") {
            writer.push(&text);
            let line = writer.line_level;
            layout.stack.push(Ctx::new(Frame::Inline, line + 1, line));
            continue;
        }
        if tok.symbol(")") || tok.symbol("]") {
            writer.push(&text);
            if layout.stack.len() > 1 {
                layout.stack.pop();
            }
            layout.stack.last_mut().expect("frame").first = false;
            continue;
        }
        if tok.symbol(",") {
            let splits = layout.splits_items();
            writer.push(&text);
            if splits {
                if options.commas == Commas::Trailing {
                    pending = Some(layout.ctx().continuation());
                } else {
                    writer.push(" ");
                }
            }
            continue;
        }
        writer.push(&text);
        let ctx = layout.stack.last_mut().expect("frame");
        ctx.first = false;
        if let Some(clause) = clause {
            ctx.clause = clause;
            ctx.between = false;
            ctx.item_break = options.clause_on_own_line && matches!(clause, Clause::List | Clause::Conditions);
        }
        match tok.upper.as_str() {
            "CASE" if tok.kind == Kind::Word => ctx.cases += 1,
            "END" if tok.kind == Kind::Word && ctx.cases > 0 => ctx.cases -= 1,
            "BETWEEN" if tok.kind == Kind::Word => ctx.between = true,
            "AND" if tok.kind == Kind::Word && ctx.between => ctx.between = false,
            "ON" if tok.kind == Kind::Word
                && ctx.frame == Frame::Block
                && ctx.cases == 0
                && clause.is_none()
                && previous.is_none_or(|previous| !previous.is("DISTINCT")) =>
            {
                ctx.clause = Clause::Conditions;
            }
            _ => {}
        }
    }
    writer.out.trim_end().to_owned()
}

fn same_tokens(before: &str, after: &str, lexing: Lexing) -> bool {
    let old = scan(before, lexing);
    let new = scan(after, lexing);
    !new.unterminated
        && old.tokens.len() == new.tokens.len()
        && old.tokens.iter().zip(&new.tokens).all(|(a, b)| {
            let (x, y) = (&before[a.start..a.end], &after[b.start..b.end]);
            a.kind == b.kind && if a.kind == Kind::Word { x.eq_ignore_ascii_case(y) } else { x == y }
        })
}

#[cfg(test)]
mod tests;
