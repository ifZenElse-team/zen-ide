use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::sql::{Dialect, strip_in};

/// What a statement does, as confirmations and read-only connections judge it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    /// Reads only: `SELECT`, `SHOW`, `EXPLAIN` without `ANALYZE` of a change, `WITH` without writes.
    Read,
    /// Changes the session only: `SET`, `USE`, `RESET`, never a read-only setting.
    Session,
    /// Starts or ends a transaction without making it writable.
    Transaction,
    /// Changes data: `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `COPY`, a call, or a read with side effects.
    Data,
    /// Changes structure or the server: `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `GRANT`.
    Structure,
    /// Anything not known to be safe.
    Other,
}

/// A risk that asks before it runs on every connection type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    /// `DELETE` without a `WHERE`, or with one that is always true.
    DeleteWithoutWhere,
    /// `UPDATE` without a `WHERE`, or with one that is always true.
    UpdateWithoutWhere,
    /// `DROP` of an object, or `ALTER … DROP` of a part of one.
    Drop,
    /// `TRUNCATE`.
    Truncate,
    /// `FLUSHALL` or `FLUSHDB` of a key-value database.
    Flush,
}

/// A risk of a statement and the object it hits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Danger {
    /// The risk.
    pub risk: Risk,
    /// The object it hits, as written (`public.orders`, `COLUMN email`), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
}

/// What a statement does and the risks it takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    /// What it does.
    pub effect: Effect,
    /// The risks that ask first on every connection type.
    pub dangers: Vec<Danger>,
}

impl Assessment {
    pub fn of(effect: Effect) -> Self {
        Self { effect, dangers: Vec::new() }
    }

    pub fn changes(&self) -> bool {
        matches!(self.effect, Effect::Data | Effect::Structure | Effect::Other)
    }

    pub fn allowed_read_only(&self) -> bool {
        !self.changes() && self.dangers.is_empty()
    }

    pub fn dangerous(&self) -> bool {
        !self.dangers.is_empty()
    }

    pub fn reads(&self) -> bool {
        self.effect == Effect::Read && self.dangers.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word { lower: String, raw: String },
    Quoted(String),
    Literal,
    Punct(char),
}

impl Token {
    fn word(&self) -> Option<&str> {
        match self {
            Token::Word { lower, .. } => Some(lower),
            _ => None,
        }
    }

    fn is(&self, word: &str) -> bool {
        self.word() == Some(word)
    }

    fn raw(&self) -> String {
        match self {
            Token::Word { raw, .. } | Token::Quoted(raw) => raw.clone(),
            Token::Literal => "''".to_owned(),
            Token::Punct(c) => c.to_string(),
        }
    }
}

fn tokenize(code: &str) -> Vec<Token> {
    let chars: Vec<char> = code.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c.is_whitespace() {
            at += 1;
        } else if c == '\'' {
            tokens.push(Token::Literal);
            at += 1;
            while at < chars.len() && chars[at] == '\'' {
                at += 1;
            }
        } else if matches!(c, '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let start = at;
            at += 1;
            while at < chars.len() && chars[at] != close {
                at += 1;
            }
            at = (at + 1).min(chars.len());
            tokens.push(Token::Quoted(chars[start..at].iter().collect()));
        } else if c.is_alphanumeric() || c == '_' || c == '$' || c == '@' {
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '$' | '@')) {
                at += 1;
            }
            let raw: String = chars[start..at].iter().collect();
            tokens.push(Token::Word { lower: raw.to_lowercase(), raw });
        } else {
            tokens.push(Token::Punct(c));
            at += 1;
        }
    }
    tokens
}

const READ_FIRST: &[&str] =
    &["select", "values", "table", "show", "describe", "desc", "explain", "help", "with", "fetch", "declare", "exists"];

const TRANSACTION_FIRST: &[&str] =
    &["begin", "start", "commit", "end", "rollback", "abort", "savepoint", "release", "xa"];

const SESSION_FIRST: &[&str] = &["set", "reset", "use", "discard", "listen", "unlisten", "pragma", "move", "close"];

const DATA_FIRST: &[&str] = &[
    "insert", "update", "delete", "merge", "replace", "upsert", "copy", "load", "call", "exec", "execute", "do",
    "import", "lock", "handler", "notify", "unlock",
];

const STRUCTURE_FIRST: &[&str] = &[
    "create",
    "alter",
    "drop",
    "truncate",
    "rename",
    "comment",
    "grant",
    "revoke",
    "reindex",
    "vacuum",
    "analyze",
    "cluster",
    "refresh",
    "optimize",
    "attach",
    "detach",
    "security",
    "reassign",
    "repair",
    "flush",
    "kill",
    "install",
    "uninstall",
    "purge",
    "shutdown",
    "undrop",
    "exchange",
    "system",
    "checkpoint",
];

const DATA_VERBS: &[&str] = &["insert", "update", "delete", "merge", "replace", "upsert", "copy"];

const SIDE_EFFECTS: &[&str] = &[
    "nextval",
    "setval",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_advisory_lock",
    "pg_advisory_xact_lock",
    "pg_try_advisory_lock",
    "pg_try_advisory_xact_lock",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_switch_wal",
    "pg_notify",
    "pg_logical_emit_message",
    "set_config",
    "lo_unlink",
    "lo_import",
    "lo_export",
    "dblink_exec",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "load_extension",
];

const LOOSENING: &[&str] = &[
    "write",
    "read_only",
    "readonly",
    "query_only",
    "writable_schema",
    "default_transaction_read_only",
    "transaction_read_only",
    "session_authorization",
    "authorization",
    "role",
];

const OBJECT_KINDS: &[&str] = &[
    "table",
    "view",
    "materialized",
    "database",
    "schema",
    "index",
    "sequence",
    "function",
    "procedure",
    "trigger",
    "type",
    "user",
    "role",
    "column",
    "constraint",
    "extension",
    "keyspace",
    "dictionary",
    "temporary",
    "temp",
    "foreign",
    "event",
    "policy",
    "publication",
    "subscription",
    "server",
    "tablespace",
    "domain",
    "aggregate",
    "operator",
    "rule",
    "statistics",
    "partition",
    "collection",
];

const NOT_A_VERB_AFTER: &[&str] = &["on", "for", "of", "before", "after", "or", "do", "key", "grant", "instead"];

fn first_word(tokens: &[Token]) -> Option<&str> {
    tokens.iter().find(|token| !matches!(token, Token::Punct('('))).and_then(Token::word)
}

fn previous(tokens: &[Token], at: usize) -> Option<&Token> {
    at.checked_sub(1).and_then(|before| tokens.get(before))
}

fn verb_at(tokens: &[Token], at: usize) -> bool {
    match previous(tokens, at) {
        None | Some(Token::Punct('(' | ')' | ';')) => true,
        Some(token) => token.is("analyze") || token.is("verbose"),
    }
}

fn writes_inside(tokens: &[Token]) -> bool {
    tokens.iter().enumerate().any(|(at, token)| match token.word() {
        Some(word) if DATA_VERBS.contains(&word) => verb_at(tokens, at),
        Some("into") => true,
        Some(word) if SIDE_EFFECTS.contains(&word) => tokens.get(at + 1) == Some(&Token::Punct('(')),
        _ => false,
    })
}

fn loosens(tokens: &[Token]) -> bool {
    tokens.iter().any(|token| token.word().is_some_and(|word| LOOSENING.contains(&word)))
}

fn effect_of(tokens: &[Token]) -> Effect {
    let Some(first) = first_word(tokens) else { return Effect::Read };
    if READ_FIRST.contains(&first) {
        if writes_inside(tokens) { Effect::Data } else { Effect::Read }
    } else if TRANSACTION_FIRST.contains(&first) {
        if loosens(tokens) { Effect::Other } else { Effect::Transaction }
    } else if SESSION_FIRST.contains(&first) {
        let assigns = first == "pragma" && tokens.iter().any(|token| matches!(token, Token::Punct('=')));
        if loosens(tokens) || assigns { Effect::Other } else { Effect::Session }
    } else if DATA_FIRST.contains(&first) {
        Effect::Data
    } else if STRUCTURE_FIRST.contains(&first) {
        Effect::Structure
    } else {
        Effect::Other
    }
}

fn name_at(tokens: &[Token], mut at: usize) -> Option<String> {
    let mut name = String::new();
    while let Some(token) = tokens.get(at) {
        match token {
            Token::Word { .. } | Token::Quoted(_) if name.is_empty() || name.ends_with('.') => {
                name.push_str(&token.raw())
            }
            Token::Punct('.') if !name.is_empty() => name.push('.'),
            _ => break,
        }
        at += 1;
    }
    let name = name.trim_end_matches('.').to_owned();
    (!name.is_empty()).then(|| name.chars().take(120).collect())
}

fn skip_words(tokens: &[Token], mut at: usize, words: &[&str]) -> usize {
    while tokens.get(at).and_then(Token::word).is_some_and(|word| words.contains(&word)) {
        at += 1;
    }
    at
}

fn dropped(tokens: &[Token], at: usize) -> Option<String> {
    let mut at = skip_words(tokens, at, &["if", "exists"]);
    let mut kinds = Vec::new();
    while let Some(word) = tokens.get(at).and_then(Token::word).filter(|word| OBJECT_KINDS.contains(word)) {
        kinds.push(word.to_ascii_uppercase());
        at += 1;
    }
    at = skip_words(tokens, at, &["if", "exists", "concurrently"]);
    let name = name_at(tokens, at);
    match (kinds.is_empty(), name) {
        (true, name) => name,
        (false, Some(name)) => Some(format!("{} {name}", kinds.join(" "))),
        (false, None) => Some(kinds.join(" ")),
    }
}

fn always_true(part: &[Token]) -> bool {
    let mut part = part;
    while part.len() >= 2 && part.first() == Some(&Token::Punct('(')) && part.last() == Some(&Token::Punct(')')) {
        part = &part[1..part.len() - 1];
    }
    let text: String = part.iter().map(|token| token.raw().to_lowercase()).collect();
    if matches!(text.as_str(), "true" | "1" | "notfalse" | "not0" | "'t'") {
        return true;
    }
    match part {
        [left, Token::Punct('='), right] => left == right,
        [left, Token::Punct('<'), Token::Punct('>'), right] | [left, Token::Punct('!'), Token::Punct('='), right] => {
            matches!((left.word(), right.word()), (Some(a), Some(b)) if a != b && a.parse::<f64>().is_ok() && b.parse::<f64>().is_ok())
        }
        _ => false,
    }
}

fn condition_always_true(condition: &[Token]) -> bool {
    if condition.is_empty() {
        return true;
    }
    let mut depth = 0i32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (at, token) in condition.iter().enumerate() {
        match token {
            Token::Punct('(') => depth += 1,
            Token::Punct(')') => depth -= 1,
            _ if depth == 0 && token.is("or") => {
                parts.push(&condition[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&condition[start..]);
    parts.into_iter().any(always_true)
}

fn filtered(tokens: &[Token], from: usize) -> bool {
    let mut depth = 0i32;
    let mut at = from;
    while let Some(token) = tokens.get(at) {
        match token {
            Token::Punct('(') => depth += 1,
            Token::Punct(')') if depth == 0 => return false,
            Token::Punct(')') => depth -= 1,
            Token::Punct(';') if depth == 0 => return false,
            _ if depth == 0 && token.is("on") => return true,
            _ if depth == 0 && token.is("where") => {
                let start = at + 1;
                let mut end = start;
                let mut inner = 0i32;
                while let Some(token) = tokens.get(end) {
                    match token {
                        Token::Punct('(') => inner += 1,
                        Token::Punct(')') if inner == 0 => break,
                        Token::Punct(')') => inner -= 1,
                        Token::Punct(';') if inner == 0 => break,
                        _ if inner == 0
                            && token.word().is_some_and(|word| matches!(word, "returning" | "order" | "limit")) =>
                        {
                            break;
                        }
                        _ => {}
                    }
                    end += 1;
                }
                return !condition_always_true(&tokens[start..end]);
            }
            _ => {}
        }
        at += 1;
    }
    false
}

fn changed_rows(tokens: &[Token], dangers: &mut Vec<Danger>) {
    let alter = first_word(tokens) == Some("alter");
    for (at, token) in tokens.iter().enumerate() {
        let delete = token.is("delete");
        if !delete && !token.is("update") {
            continue;
        }
        let verb = verb_at(tokens, at)
            || (alter
                && previous(tokens, at).and_then(Token::word).is_none_or(|word| !NOT_A_VERB_AFTER.contains(&word)));
        if !verb || filtered(tokens, at + 1) {
            continue;
        }
        let object = if delete {
            let from = tokens[at + 1..].iter().position(|token| token.is("from")).map(|found| at + 1 + found + 1);
            from.and_then(|from| name_at(tokens, skip_words(tokens, from, &["only"])))
        } else {
            name_at(tokens, skip_words(tokens, at + 1, &["only", "low_priority", "ignore"]))
        };
        let risk = if delete { Risk::DeleteWithoutWhere } else { Risk::UpdateWithoutWhere };
        dangers.push(Danger { risk, object });
    }
}

fn dangers_of(tokens: &[Token]) -> Vec<Danger> {
    let mut dangers = Vec::new();
    let start = tokens.iter().position(|token| !matches!(token, Token::Punct('('))).unwrap_or(0);
    match first_word(tokens) {
        Some("drop") => dangers.push(Danger { risk: Risk::Drop, object: dropped(tokens, start + 1) }),
        Some("truncate") => {
            let at = skip_words(tokens, start + 1, &["table", "only", "if", "exists"]);
            dangers.push(Danger { risk: Risk::Truncate, object: name_at(tokens, at) });
        }
        Some("alter") => {
            let mut depth = 0i32;
            for (at, token) in tokens.iter().enumerate() {
                match token {
                    Token::Punct('(') => depth += 1,
                    Token::Punct(')') => depth -= 1,
                    _ if depth == 0 && token.is("drop") => {
                        let kept = previous(tokens, at).is_some_and(|before| before.is("set") || before.is("on"))
                            || tokens
                                .get(at + 1)
                                .and_then(Token::word)
                                .is_some_and(|next| matches!(next, "default" | "not" | "identity" | "expression"));
                        if !kept {
                            dangers.push(Danger { risk: Risk::Drop, object: dropped(tokens, at + 1) });
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    changed_rows(tokens, &mut dangers);
    dangers
}

pub fn assess(statement: &str, dialect: Dialect) -> Assessment {
    if dialect.commands() {
        return assess_command(statement, false);
    }
    let tokens = tokenize(&strip_in(statement, dialect));
    let mut effect = effect_of(&tokens);
    if dialect == Dialect::MySql
        && statement.contains("/*!")
        && matches!(effect, Effect::Read | Effect::Session | Effect::Transaction)
    {
        effect = Effect::Other;
    }
    Assessment { effect, dangers: dangers_of(&tokens) }
}

pub fn assess_command(statement: &str, reads: bool) -> Assessment {
    let verb = statement.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let effect = if reads || verb.is_empty() { Effect::Read } else { Effect::Data };
    let dangers = if matches!(verb.as_str(), "flushall" | "flushdb") {
        vec![Danger { risk: Risk::Flush, object: None }]
    } else {
        Vec::new()
    };
    Assessment { effect, dangers }
}

const NOT_CALLS: &[&str] = &[
    "all",
    "and",
    "any",
    "array",
    "as",
    "avg",
    "between",
    "by",
    "case",
    "cast",
    "coalesce",
    "connection_id",
    "count",
    "cross",
    "cube",
    "current_database",
    "current_date",
    "current_schema",
    "current_timestamp",
    "current_user",
    "database",
    "distinct",
    "else",
    "except",
    "exists",
    "filter",
    "from",
    "full",
    "group",
    "having",
    "ilike",
    "in",
    "inner",
    "intersect",
    "into",
    "is",
    "join",
    "lateral",
    "left",
    "length",
    "like",
    "limit",
    "localtimestamp",
    "lower",
    "materialized",
    "max",
    "min",
    "natural",
    "not",
    "now",
    "of",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "pg_backend_pid",
    "pg_sleep",
    "recursive",
    "right",
    "rollup",
    "row",
    "schema",
    "select",
    "session_user",
    "sets",
    "sleep",
    "some",
    "sqlite_version",
    "sum",
    "table",
    "tablesample",
    "then",
    "union",
    "upper",
    "user",
    "using",
    "values",
    "version",
    "when",
    "where",
    "with",
    "within",
];

pub fn calls_functions(statement: &str, dialect: Dialect) -> bool {
    if dialect.commands() {
        return false;
    }
    let tokens = tokenize(&strip_in(statement, dialect));
    tokens.windows(2).any(|pair| {
        pair[1] == Token::Punct('(')
            && match &pair[0] {
                Token::Word { lower, .. } => !NOT_CALLS.contains(&lower.as_str()),
                Token::Quoted(_) => true,
                _ => false,
            }
    })
}

mod outside;

pub use outside::reaches_outside;

#[cfg(test)]
mod tests;
