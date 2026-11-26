use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_syntax::{LineLexer, Lines, TextChange};

/// The SQL dialect a script is split by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Dialect {
    /// PostgreSQL: dollar quotes, `E''` strings, nested comments, psql meta-commands.
    Postgres,
    /// MySQL and MariaDB: backslash escapes, backticks, `#` comments, `DELIMITER`.
    #[serde(rename = "mysql")]
    MySql,
    /// SQL Server: `[identifiers]`, nested comments, `GO` between batches.
    SqlServer,
    /// Oracle: `q'[…]'` strings, PL/SQL blocks and units that end only at a `/` line, which also ends any statement.
    Oracle,
    /// Standard SQL: quotes, `--` and `/* */` comments, `;`.
    #[default]
    Standard,
    /// A query language of one statement per line, such as Redis commands.
    Lines,
    /// A command language whose statements span lines, such as the MongoDB shell: JavaScript quotes and comments, `;` or a blank line between statements.
    Script,
}

impl Dialect {
    pub fn of_language(language: &str) -> Self {
        match language {
            "sql:postgres" => Dialect::Postgres,
            "sql:mysql" | "sql:mariadb" => Dialect::MySql,
            "sql:sqlserver" => Dialect::SqlServer,
            "sql:oracle" => Dialect::Oracle,
            language if language.starts_with("sql") => Dialect::Standard,
            "mongo" => Dialect::Script,
            _ => Dialect::Lines,
        }
    }

    pub fn commands(self) -> bool {
        matches!(self, Dialect::Lines | Dialect::Script)
    }
}

/// When a blank line ends a statement for Execute Statement; Execute Script splits on delimiters only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BlankLines {
    /// Always, except inside a quote, a comment or a `BEGIN … END` block.
    #[default]
    Always,
    /// Only where the text before it could end a statement: outside brackets and not after a comma, an operator or a word that needs more.
    Smart,
    /// Never.
    Never,
}

/// One statement of a script, in bytes of its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    /// Its first byte: its first comment or word.
    pub start: usize,
    /// The byte after its last character, before the delimiter and trailing white space.
    pub end: usize,
    /// The byte after its delimiter; `end` when it has none.
    pub delimiter_end: usize,
    /// Whether it is a command of the client rather than of the server: a psql meta-command, a `DELIMITER` line, or a script command (`@set`, `@include`).
    pub client: bool,
}

impl Statement {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.start..self.end).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Quote {
    #[default]
    Code,
    Comment(u8),
    Single {
        backslash: bool,
    },
    Double {
        backslash: bool,
    },
    Backtick,
    Bracket,
    Dollar(Box<str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Begin,
    Case,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pending {
    #[default]
    None,
    Begin,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Head {
    #[default]
    Start,
    Create,
    Unit,
    Plain,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Between,
    AfterDelimiter,
    Inside,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LexState {
    quote: Quote,
    phase: Phase,
    code: bool,
    words: u8,
    parens: u16,
    blocks: Vec<Block>,
    pending: Pending,
    continues: bool,
    delimiter: Option<Box<str>>,
    head: Head,
}

impl LexState {
    fn breaks_at_blank(&self, mode: BlankLines) -> bool {
        if self.quote != Quote::Code
            || self.phase != Phase::Inside
            || !self.blocks.is_empty()
            || self.head == Head::Unit
        {
            return false;
        }
        match mode {
            BlankLines::Always => true,
            BlankLines::Smart => self.parens == 0 && !self.continues,
            BlankLines::Never => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Start(u32),
    End { at: u32, resume: u32, code: bool, client: bool },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineInfo {
    events: Vec<Event>,
    blank: bool,
    code_at_end: bool,
    inside_at_end: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SqlLexer {
    dialect: Dialect,
}

const TRANSACTION_WORDS: [&str; 11] = [
    "TRANSACTION",
    "WORK",
    "TRAN",
    "ISOLATION",
    "READ",
    "DEFERRABLE",
    "NOT",
    "DISTRIBUTED",
    "DEFERRED",
    "IMMEDIATE",
    "EXCLUSIVE",
];

fn continues(word: &str) -> bool {
    let bytes = word.as_bytes();
    if bytes.len() > 9 {
        return false;
    }
    let mut lower = [0_u8; 9];
    for (at, byte) in bytes.iter().enumerate() {
        lower[at] = byte.to_ascii_lowercase();
    }
    matches!(
        &lower[..bytes.len()],
        b"select"
            | b"from"
            | b"where"
            | b"and"
            | b"or"
            | b"not"
            | b"join"
            | b"inner"
            | b"left"
            | b"right"
            | b"full"
            | b"outer"
            | b"cross"
            | b"on"
            | b"using"
            | b"by"
            | b"group"
            | b"order"
            | b"having"
            | b"set"
            | b"values"
            | b"into"
            | b"as"
            | b"with"
            | b"union"
            | b"intersect"
            | b"except"
            | b"all"
            | b"distinct"
            | b"case"
            | b"when"
            | b"then"
            | b"else"
            | b"in"
            | b"is"
            | b"like"
            | b"ilike"
            | b"between"
            | b"limit"
            | b"offset"
            | b"update"
            | b"insert"
            | b"delete"
            | b"table"
            | b"returning"
            | b"window"
    )
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

fn starts_statement(word: &str) -> bool {
    const WORDS: [&str; 52] = [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "MERGE",
        "WITH",
        "CREATE",
        "ALTER",
        "DROP",
        "TRUNCATE",
        "GRANT",
        "REVOKE",
        "COMMENT",
        "EXPLAIN",
        "ANALYZE",
        "VACUUM",
        "SHOW",
        "SET",
        "RESET",
        "BEGIN",
        "START",
        "COMMIT",
        "ROLLBACK",
        "END",
        "CALL",
        "DO",
        "COPY",
        "VALUES",
        "TABLE",
        "DECLARE",
        "EXEC",
        "EXECUTE",
        "USE",
        "LOCK",
        "REFRESH",
        "REINDEX",
        "CLUSTER",
        "DESCRIBE",
        "DESC",
        "PREPARE",
        "DEALLOCATE",
        "LISTEN",
        "NOTIFY",
        "SAVEPOINT",
        "RELEASE",
        "PRINT",
        "IF",
        "REPLACE",
        "LOAD",
        "OPTIMIZE",
        "CHECKPOINT",
        "DISCARD",
    ];
    WORDS.iter().any(|known| known.eq_ignore_ascii_case(word))
}

fn line_content(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

fn word_at(line: &str, at: usize) -> &str {
    let bytes = line.as_bytes();
    let mut end = at;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    &line[at..end]
}

fn is_go(content: &str) -> bool {
    let mut words = content.split_whitespace();
    let Some(first) = words.next() else { return false };
    if !first.eq_ignore_ascii_case("go") {
        return false;
    }
    match (words.next(), words.next()) {
        (None, _) => true,
        (Some(count), None) => count.bytes().all(|byte| byte.is_ascii_digit()),
        _ => false,
    }
}

fn dollar_tag(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut end = at + 1;
    if end < bytes.len() && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'_' || bytes[end] >= 0x80) {
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] >= 0x80) {
            end += 1;
        }
    }
    (end < bytes.len() && bytes[end] == b'$').then_some(end + 1)
}

struct Cursor<'a> {
    line: &'a str,
    bytes: &'a [u8],
    events: Vec<Event>,
}

impl Cursor<'_> {
    fn at(&self, at: usize) -> u32 {
        u32::try_from(at).unwrap_or(u32::MAX)
    }
}

impl SqlLexer {
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    fn start(&self, cursor: &mut Cursor<'_>, state: &mut LexState, at: usize, code: bool) {
        match state.phase {
            Phase::Inside => {}
            Phase::AfterDelimiter if !code => return,
            Phase::Between | Phase::AfterDelimiter => {
                cursor.events.push(Event::Start(cursor.at(at)));
                state.phase = Phase::Inside;
                state.code = false;
                state.words = 0;
                state.head = Head::Start;
            }
        }
        if code {
            state.code = true;
        }
    }

    fn end(&self, cursor: &mut Cursor<'_>, state: &mut LexState, at: usize, resume: usize, client: bool) {
        if state.phase == Phase::Inside {
            cursor.events.push(Event::End {
                at: cursor.at(at),
                resume: cursor.at(resume),
                code: state.code || client,
                client,
            });
        }
        state.phase = Phase::AfterDelimiter;
        state.code = false;
        state.words = 0;
        state.parens = 0;
        state.blocks.clear();
        state.pending = Pending::None;
        state.continues = false;
        state.head = Head::Start;
    }

    fn resolve(state: &mut LexState, word: Option<&str>) -> bool {
        match std::mem::take(&mut state.pending) {
            Pending::None => false,
            Pending::Begin => {
                let transaction =
                    word.is_none_or(|word| TRANSACTION_WORDS.iter().any(|known| known.eq_ignore_ascii_case(word)));
                if !transaction {
                    state.blocks.push(Block::Begin);
                }
                false
            }
            Pending::End => {
                let Some(word) = word else {
                    state.blocks.pop();
                    return false;
                };
                let upper = word.to_ascii_uppercase();
                match upper.as_str() {
                    "IF" | "LOOP" | "WHILE" | "REPEAT" | "FOR" => true,
                    "CASE" => {
                        if state.blocks.last() == Some(&Block::Case) {
                            state.blocks.pop();
                        }
                        true
                    }
                    "TRY" | "CATCH" => {
                        state.blocks.pop();
                        true
                    }
                    _ => {
                        state.blocks.pop();
                        false
                    }
                }
            }
        }
    }

    fn head(state: &mut LexState, word: &str) {
        let upper = word.to_ascii_uppercase();
        state.head = match (state.head, upper.as_str()) {
            (Head::Start, "DECLARE" | "BEGIN") => Head::Unit,
            (Head::Start, "CREATE") => Head::Create,
            (
                Head::Create,
                "OR" | "REPLACE" | "EDITIONABLE" | "NONEDITIONABLE" | "AND" | "COMPILE" | "RESOLVE" | "NOFORCE",
            ) => Head::Create,
            (Head::Create, "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE" | "LIBRARY" | "JAVA") => {
                Head::Unit
            }
            (Head::Unit, _) => Head::Unit,
            _ => Head::Plain,
        };
    }

    fn word(&self, state: &mut LexState, word: &str) {
        if self.dialect == Dialect::Oracle {
            Self::head(state, word);
            if state.head == Head::Unit {
                state.words = 1;
                state.continues = false;
                return;
            }
        }
        let first = state.words == 0;
        state.words = 1;
        if Self::resolve(state, Some(word)) {
            state.continues = false;
            return;
        }
        if word.eq_ignore_ascii_case("begin") {
            if first {
                state.pending = Pending::Begin;
            } else {
                state.blocks.push(Block::Begin);
            }
        } else if word.eq_ignore_ascii_case("case") {
            state.blocks.push(Block::Case);
        } else if word.eq_ignore_ascii_case("end") && !state.blocks.is_empty() {
            state.pending = Pending::End;
        }
        state.continues = continues(word);
    }

    fn whole_line(&self, cursor: &mut Cursor<'_>, state: &mut LexState) -> bool {
        if state.quote != Quote::Code {
            return false;
        }
        let content = line_content(cursor.line);
        let lead = content.len() - content.trim_start().len();
        let trimmed = content.trim();
        if self.dialect == Dialect::SqlServer && is_go(trimmed) {
            self.end(cursor, state, 0, content.len(), false);
            return true;
        }
        if self.dialect == Dialect::Oracle && trimmed == "/" {
            self.end(cursor, state, 0, content.len(), false);
            return true;
        }
        if state.phase == Phase::Inside && state.code {
            return false;
        }
        if self.dialect != Dialect::Lines && crate::params::is_script_command(trimmed) {
            self.start(cursor, state, lead, true);
            self.end(cursor, state, content.len(), content.len(), true);
            return true;
        }
        match self.dialect {
            Dialect::MySql
                if word_at(content, lead).eq_ignore_ascii_case("delimiter")
                    && trimmed.len() > "delimiter".len()
                    && trimmed.as_bytes()["delimiter".len()].is_ascii_whitespace() =>
            {
                let wanted = trimmed["delimiter".len()..].split_whitespace().next().unwrap_or(";");
                self.start(cursor, state, lead, true);
                self.end(cursor, state, content.len(), content.len(), true);
                state.delimiter = (wanted != ";").then(|| wanted.into());
                true
            }
            Dialect::Postgres if trimmed.starts_with('\\') => {
                self.start(cursor, state, lead, true);
                self.end(cursor, state, content.len(), content.len(), true);
                true
            }
            Dialect::Lines => {
                if !trimmed.is_empty() {
                    let start = lead;
                    self.start(cursor, state, start, true);
                    self.end(cursor, state, content.len(), content.len(), false);
                }
                true
            }
            _ => false,
        }
    }

    fn quoted(&self, cursor: &Cursor<'_>, state: &mut LexState, mut at: usize) -> usize {
        let bytes = cursor.bytes;
        let close = |quote: u8, backslash: bool, mut at: usize, state: &mut LexState| {
            while at < bytes.len() {
                let byte = bytes[at];
                if backslash && byte == b'\\' {
                    at += 2;
                    continue;
                }
                if byte == quote {
                    if at + 1 < bytes.len() && bytes[at + 1] == quote {
                        at += 2;
                        continue;
                    }
                    state.quote = Quote::Code;
                    return at + 1;
                }
                at += 1;
            }
            bytes.len()
        };
        match state.quote.clone() {
            Quote::Code => at,
            Quote::Single { backslash } => close(b'\'', backslash, at, state),
            Quote::Double { backslash } => close(b'"', backslash, at, state),
            Quote::Backtick => close(b'`', false, at, state),
            Quote::Bracket => close(b']', false, at, state),
            Quote::Dollar(tag) => match cursor.line[at..].find(&*tag) {
                Some(found) => {
                    state.quote = Quote::Code;
                    at + found + tag.len()
                }
                None => bytes.len(),
            },
            Quote::Comment(depth) => {
                let nests = matches!(self.dialect, Dialect::Postgres | Dialect::SqlServer);
                let mut depth = depth;
                while at + 1 < bytes.len() {
                    if bytes[at] == b'*' && bytes[at + 1] == b'/' {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            state.quote = Quote::Code;
                            return at;
                        }
                        continue;
                    }
                    if nests && bytes[at] == b'/' && bytes[at + 1] == b'*' {
                        depth = depth.saturating_add(1);
                        at += 2;
                        continue;
                    }
                    at += 1;
                }
                state.quote = Quote::Comment(depth);
                bytes.len()
            }
        }
    }

    fn line_comment(&self, bytes: &[u8], at: usize) -> bool {
        if bytes[at] == b'#' && self.dialect == Dialect::MySql {
            return true;
        }
        if self.dialect == Dialect::Script {
            return bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'/');
        }
        if bytes[at] != b'-' || bytes.get(at + 1) != Some(&b'-') {
            return false;
        }
        self.dialect != Dialect::MySql || bytes.get(at + 2).is_none_or(|next| next.is_ascii_whitespace())
    }

    fn opens_quote(&self, bytes: &[u8], at: usize, line: &str) -> Option<(Quote, usize)> {
        let byte = bytes[at];
        let mysql = self.dialect == Dialect::MySql;
        let backslash = mysql || self.dialect == Dialect::Script;
        match byte {
            b'\'' => Some((Quote::Single { backslash }, at + 1)),
            b'"' => Some((Quote::Double { backslash }, at + 1)),
            b'`' if mysql => Some((Quote::Backtick, at + 1)),
            b'[' if self.dialect == Dialect::SqlServer => Some((Quote::Bracket, at + 1)),
            b'q' | b'Q' if self.dialect == Dialect::Oracle => {
                let before = at.checked_sub(1).map(|before| bytes[before]);
                if before.is_some_and(|before| is_word_byte(before) && !matches!(before, b'n' | b'N')) {
                    return None;
                }
                if bytes.get(at + 1) != Some(&b'\'') {
                    return None;
                }
                let open = *bytes.get(at + 2)?;
                if open.is_ascii_whitespace() || open >= 0x80 {
                    return None;
                }
                let close = match open {
                    b'[' => b']',
                    b'(' => b')',
                    b'{' => b'}',
                    b'<' => b'>',
                    other => other,
                };
                let tag = format!("{}'", char::from(close));
                Some((Quote::Dollar(tag.into()), at + 3))
            }
            b'$' if self.dialect == Dialect::Postgres => {
                let before = at.checked_sub(1).map(|before| bytes[before]);
                if before.is_some_and(is_word_byte) {
                    return None;
                }
                let end = dollar_tag(line, at)?;
                Some((Quote::Dollar(line[at..end].into()), end))
            }
            _ => None,
        }
    }

    fn code(&self, cursor: &mut Cursor<'_>, state: &mut LexState, at: usize) -> usize {
        let bytes = cursor.bytes;
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            return at + 1;
        }
        if let Some(delimiter) = state.delimiter.clone() {
            if cursor.line[at..].starts_with(&*delimiter) {
                let resume = at + delimiter.len();
                self.end(cursor, state, at, resume, false);
                return resume;
            }
        } else if byte == b';' && state.head == Head::Unit {
            self.start(cursor, state, at, true);
            state.continues = false;
            return at + 1;
        } else if byte == b';' {
            Self::resolve(state, None);
            if state.parens == 0 && !state.blocks.contains(&Block::Begin) {
                self.end(cursor, state, at, at + 1, false);
            } else {
                self.start(cursor, state, at, true);
                state.continues = false;
            }
            return at + 1;
        }
        if self.line_comment(bytes, at) {
            self.start(cursor, state, at, false);
            return line_content(cursor.line).len().max(at + 1);
        }
        if byte == b'/' && bytes.get(at + 1) == Some(&b'*') {
            self.start(cursor, state, at, false);
            state.quote = Quote::Comment(1);
            return at + 2;
        }
        self.start(cursor, state, at, true);
        if let Some((quote, next)) = self.opens_quote(bytes, at, cursor.line) {
            Self::resolve(state, Some(""));
            state.words = 1;
            state.continues = false;
            state.quote = quote;
            return next;
        }
        if is_word_start(byte) {
            let word = word_at(cursor.line, at);
            let next = at + word.len();
            if self.dialect == Dialect::Postgres && word.eq_ignore_ascii_case("e") && bytes.get(next) == Some(&b'\'') {
                Self::resolve(state, Some(""));
                state.words = 1;
                state.continues = false;
                state.quote = Quote::Single { backslash: true };
                return next + 1;
            }
            self.word(state, word);
            return next;
        }
        if byte.is_ascii_digit() {
            let mut next = at;
            while next < bytes.len() && (bytes[next].is_ascii_alphanumeric() || bytes[next] == b'.') {
                next += 1;
            }
            Self::resolve(state, Some(""));
            state.words = 1;
            state.continues = false;
            return next;
        }
        Self::resolve(state, Some(""));
        let script = self.dialect == Dialect::Script;
        match byte {
            b'(' | b'[' => state.parens = state.parens.saturating_add(1),
            b')' | b']' => state.parens = state.parens.saturating_sub(1),
            b'{' if script => state.parens = state.parens.saturating_add(1),
            b'}' if script => state.parens = state.parens.saturating_sub(1),
            _ => {}
        }
        state.words = 1;
        state.continues = matches!(byte, b',' | b'(' | b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>' | b'|' | b'.');
        at + 1
    }
}

impl LineLexer for SqlLexer {
    type State = LexState;
    type Line = LineInfo;

    fn lex(&self, line: &str, state: &mut LexState) -> LineInfo {
        let blank = state.quote == Quote::Code && line.trim().is_empty();
        let mut cursor = Cursor { line, bytes: line.as_bytes(), events: Vec::new() };
        if !self.whole_line(&mut cursor, state) {
            let mut at = 0;
            while at < cursor.bytes.len() {
                at = if state.quote == Quote::Code {
                    self.code(&mut cursor, state, at)
                } else {
                    self.quoted(&cursor, state, at)
                };
            }
        }
        if state.phase == Phase::AfterDelimiter {
            state.phase = Phase::Between;
        }
        LineInfo { events: cursor.events, blank, code_at_end: state.code, inside_at_end: state.phase == Phase::Inside }
    }
}

pub struct Script {
    lexer: SqlLexer,
    lines: Lines<SqlLexer>,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Script").field("dialect", &self.lexer.dialect).field("lines", &self.lines.len()).finish()
    }
}

fn trim_end(text: &str, start: usize, end: usize) -> usize {
    let slice = &text[start..end.max(start)];
    start + slice.trim_end().len()
}

impl Script {
    pub fn new(text: &str, dialect: Dialect) -> Self {
        let lexer = SqlLexer::new(dialect);
        Self { lines: Lines::new(&lexer, text), lexer }
    }

    pub fn dialect(&self) -> Dialect {
        self.lexer.dialect
    }

    pub fn edit(&mut self, text: &str, start: usize, old_end: usize, new_end: usize) -> usize {
        let range = self.lines.edit(&self.lexer, text, TextChange { start, old_end, new_end }, |_| {});
        range.len()
    }

    pub fn statements(&self, text: &str) -> Vec<Statement> {
        self.statements_in(text, 0..self.lines.len())
    }

    fn statements_in(&self, text: &str, lines: Range<usize>) -> Vec<Statement> {
        let mut found = Vec::new();
        let mut start: Option<usize> = None;
        let first = lines.start;
        if first > 0 && self.lines.state(first).phase == Phase::Inside {
            start = self.statement_start_before(first);
        }
        for index in lines.clone() {
            let base = self.lines.start(index);
            for event in &self.lines.line(index).events {
                match *event {
                    Event::Start(at) => start = Some(base + at as usize),
                    Event::End { at, resume, code, client } => {
                        if let Some(begun) = start.take()
                            && code
                        {
                            let at = base + at as usize;
                            found.push(Statement {
                                start: begun,
                                end: trim_end(text, begun, at),
                                delimiter_end: base + resume as usize,
                                client,
                            });
                        }
                    }
                }
            }
        }
        if lines.end == self.lines.len()
            && let Some(begun) = start
            && self.lines.lines().last().is_some_and(|line| line.code_at_end)
        {
            let end = trim_end(text, begun, text.len());
            found.push(Statement { start: begun, end, delimiter_end: end, client: false });
        }
        found
    }

    fn statement_start_before(&self, line: usize) -> Option<usize> {
        let mut index = line;
        while index > 0 {
            index -= 1;
            let base = self.lines.start(index);
            if let Some(Event::Start(at)) =
                self.lines.line(index).events.iter().rev().find(|event| matches!(event, Event::Start(_)))
            {
                return Some(base + *at as usize);
            }
        }
        None
    }

    fn line_range_around(&self, line: usize) -> Range<usize> {
        let mut first = line;
        while first > 0 {
            let starts_here = self.lines.state(first).phase != Phase::Inside;
            if starts_here {
                break;
            }
            first -= 1;
        }
        let mut last = line;
        while last + 1 < self.lines.len() && self.lines.line(last).inside_at_end {
            last += 1;
        }
        first..last + 1
    }

    pub fn statements_near(&self, text: &str, offset: usize) -> Vec<Statement> {
        let line = self.lines.line_of(offset.min(text.len()));
        let range = self.line_range_around(line);
        let mut from = range.start.saturating_sub(1);
        while from > 0 && self.lines.line(from).events.is_empty() {
            from -= 1;
        }
        let from = self.line_range_around(from).start;
        let mut to = range.end.min(self.lines.len() - 1);
        while to + 1 < self.lines.len() && self.lines.line(to).events.is_empty() {
            to += 1;
        }
        let to = self.line_range_around(to).end;
        self.statements_in(text, from..to)
    }

    fn pieces(&self, text: &str, statement: &Statement, mode: BlankLines) -> Vec<Statement> {
        if mode == BlankLines::Never {
            return vec![statement.clone()];
        }
        let first = self.lines.line_of(statement.start);
        let last = self.lines.line_of(statement.end.saturating_sub(1).max(statement.start));
        let mut pieces = Vec::new();
        let mut begun = statement.start;
        for index in first + 1..=last {
            let line = self.lines.line(index);
            if line.blank
                && self.lines.state(index).breaks_at_blank(mode)
                && (mode != BlankLines::Smart || self.statement_follows(text, index))
            {
                let at = self.lines.start(index);
                if at > begun {
                    pieces.push((begun, at));
                }
                begun = self.lines.end(index);
            }
        }
        pieces.push((begun, statement.delimiter_end.max(begun)));
        let count = pieces.len();
        pieces
            .into_iter()
            .enumerate()
            .filter_map(|(index, (from, to))| {
                let last = index + 1 == count;
                let slice = &text[from..to.min(text.len())];
                let lead = slice.len() - slice.trim_start().len();
                let start = from + lead;
                let end = if last { statement.end } else { trim_end(text, start, to) };
                (end > start && has_code(&text[start..end], self.lexer.dialect)).then_some(Statement {
                    start,
                    end,
                    delimiter_end: if last { statement.delimiter_end } else { end },
                    client: statement.client,
                })
            })
            .collect()
    }

    fn statement_follows(&self, text: &str, blank: usize) -> bool {
        let mut index = blank + 1;
        while index < self.lines.len() {
            let content = line_content(&text[self.lines.start(index)..self.lines.end(index)]).trim_start();
            if !content.is_empty() {
                let word = word_at(content, 0);
                return starts_statement(word)
                    || (word.is_empty() && !content.starts_with(['(', ',', ')', '+', '*', '=']));
            }
            index += 1;
        }
        false
    }

    pub fn statement_at(&self, text: &str, caret: usize, mode: BlankLines) -> Option<Statement> {
        let caret = self.caret_place(text, caret.min(text.len()));
        let mut candidates: Vec<Statement> =
            self.statements_near(text, caret).iter().flat_map(|statement| self.pieces(text, statement, mode)).collect();
        candidates.sort_by_key(|statement| statement.start);
        if let Some(found) =
            candidates.iter().find(|statement| statement.start <= caret && caret <= statement.delimiter_end)
        {
            return Some(found.clone());
        }
        let line = self.lines.line_of(caret);
        let line_start = self.lines.start(line);
        let before = candidates.iter().rev().find(|statement| statement.delimiter_end <= caret);
        if let Some(before) = before
            && before.delimiter_end >= line_start
        {
            return Some(before.clone());
        }
        let after = candidates.iter().find(|statement| statement.start >= caret);
        if let Some(after) = after
            && self.lines.line_of(after.start) == line
        {
            return Some(after.clone());
        }
        before.or(after).cloned()
    }

    fn caret_place(&self, text: &str, caret: usize) -> usize {
        let mut line = self.lines.line_of(caret);
        loop {
            let start = self.lines.start(line);
            let end = self.lines.end(line);
            let content = line_content(&text[start..end]);
            if !content.trim().is_empty() || line == 0 {
                if line == self.lines.line_of(caret) {
                    return caret;
                }
                return start + content.trim_end().len();
            }
            line -= 1;
        }
    }

    pub fn statements_from(&self, text: &str, caret: usize) -> Vec<Statement> {
        let all = self.statements(text);
        let Some(at) = self.statement_at(text, caret, BlankLines::Never) else { return Vec::new() };
        all.into_iter().filter(|statement| statement.delimiter_end > at.start).collect()
    }

    pub fn neighbour(&self, text: &str, caret: usize, forward: bool, mode: BlankLines) -> Option<Statement> {
        let current = self.statement_at(text, caret, mode);
        let all: Vec<Statement> =
            self.statements(text).iter().flat_map(|statement| self.pieces(text, statement, mode)).collect();
        match (forward, current) {
            (true, Some(current)) => all.into_iter().find(|statement| statement.start > current.start),
            (false, Some(current)) => all.into_iter().rev().find(|statement| statement.start < current.start),
            (true, None) => all.into_iter().find(|statement| statement.start >= caret),
            (false, None) => all.into_iter().rev().find(|statement| statement.end <= caret),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

pub fn has_code(text: &str, dialect: Dialect) -> bool {
    let lexer = SqlLexer::new(dialect);
    let mut state = LexState::default();
    for line in text.split_inclusive('\n') {
        let lexed = lexer.lex(line, &mut state);
        if state.code || lexed.events.iter().any(|event| matches!(event, Event::End { code: true, .. })) {
            return true;
        }
    }
    false
}

pub fn split(text: &str, dialect: Dialect) -> Vec<Statement> {
    Script::new(text, dialect).statements(text)
}

pub fn pageable(statement: &str) -> bool {
    let code = strip(statement);
    let words: Vec<String> = code
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let reads = matches!(words.first().map(String::as_str), Some("select" | "values" | "table" | "with"));
    let writes = words.iter().any(|word| {
        matches!(word.as_str(), "insert" | "update" | "delete" | "merge" | "into" | "returning" | "lock" | "copy")
    }) || code.to_ascii_lowercase().contains("for update")
        || code.to_ascii_lowercase().contains("for share")
        || code.to_ascii_lowercase().contains("for no key update")
        || code.to_ascii_lowercase().contains("for key share");
    reads && !writes
}

pub fn first_keyword(statement: &str) -> Option<String> {
    let code = strip(statement);
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|word| !word.is_empty())
        .map(str::to_ascii_uppercase)
}

pub fn controls_transaction(statement: &str) -> bool {
    matches!(
        first_keyword(statement).as_deref(),
        Some("BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "ABORT" | "SAVEPOINT" | "RELEASE")
    )
}

pub fn smart_commit_switches(statement: &str) -> bool {
    match first_keyword(statement) {
        Some(word) => {
            !matches!(word.as_str(), "SELECT" | "SHOW" | "USE" | "SET" | "EXPLAIN") && !controls_transaction(statement)
        }
        None => false,
    }
}

fn strip(statement: &str) -> String {
    strip_with(statement, Dialect::Postgres, false)
}

pub fn strip_in(statement: &str, dialect: Dialect) -> String {
    strip_with(statement, dialect, true)
}

fn strip_with(statement: &str, dialect: Dialect, identifiers: bool) -> String {
    let lexer = SqlLexer::new(dialect);
    let mut state = LexState::default();
    let mut out = String::with_capacity(statement.len());
    for line in statement.split_inclusive('\n') {
        let mut at = 0;
        let bytes = line.as_bytes();
        while at < bytes.len() {
            if state.quote == Quote::Code {
                if lexer.line_comment(bytes, at) {
                    break;
                }
                if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
                    state.quote = Quote::Comment(1);
                    at += 2;
                    out.push(' ');
                    continue;
                }
                if let Some((quote, next)) = lexer.opens_quote(bytes, at, line) {
                    let identifier = identifiers
                        && match quote {
                            Quote::Double { .. } => dialect != Dialect::MySql,
                            Quote::Backtick | Quote::Bracket => true,
                            _ => false,
                        };
                    state.quote = quote;
                    if identifier {
                        let cursor = Cursor { line, bytes, events: Vec::new() };
                        let end = lexer.quoted(&cursor, &mut state, next);
                        if state.quote == Quote::Code {
                            out.push(' ');
                            out.push_str(&line[at..end]);
                            out.push(' ');
                            at = end;
                            continue;
                        }
                        out.push_str(" '' ");
                        at = end;
                        continue;
                    }
                    out.push_str(" '' ");
                    at = next;
                    continue;
                }
                let next = at + line[at..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&line[at..next]);
                at = next;
            } else {
                let cursor = Cursor { line, bytes, events: Vec::new() };
                at = lexer.quoted(&cursor, &mut state, at);
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
