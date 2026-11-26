use std::hash::{DefaultHasher, Hash, Hasher};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::QueryStatement;
use zen_db::history::Provenance;
use zen_db::registry::Entry;
use zen_db::safety::{Assessment, Danger, Effect, Risk};
use zen_db::types::{AgentAccess, ConnectionType};
use zen_i18n::t;

use super::consoles::engine;
use super::sources::{SourceFields, connection_types, find};

pub const CONFIRM_EXECUTE: &str = "confirmExecute";
pub const READ_ONLY: &str = "readOnly";
pub const AGENT_READ_ONLY: &str = "agentReadOnly";
pub const AGENT_NOT_ALLOWED: &str = "agentNotAllowed";
pub const AGENT_OUTSIDE: &str = "agentOutsideDatabase";
const LOCAL_FILES_LANGUAGES: &[&str] = &["sql:duckdb"];
pub const SKIPPED: &str = "skipped";

const MOST_LISTED: usize = 100;
const MOST_TEXT: usize = 2000;

pub fn type_of(session: &Session, entry: &Entry) -> ConnectionType {
    let (types, _) = connection_types(session);
    types.resolve(&entry.source.connection_type).with_overrides(&entry.source.overrides)
}

pub fn read_only(session: &Session, entry: &Entry) -> bool {
    entry.source.read_only || type_of(session, entry).read_only
}

pub fn agent_access(session: &Session, entry: &Entry) -> AgentAccess {
    type_of(session, entry).agent_access()
}

/// A confirmation passed back after a `confirmExecute` refusal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Confirmation {
    /// The `token` of the refusal: runs the statements it asked about, unchanged since.
    pub confirmed: Option<String>,
    /// Indexes of the refusal's statements to leave out (Skip).
    pub skip: Vec<usize>,
}

/// A statement that asks first or was refused, as a refusal lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    /// Its index among the statements of the run.
    pub index: usize,
    /// Its text, cut to 2000 characters.
    pub text: String,
    /// Whether the text was cut.
    pub cut: bool,
    /// What it does.
    pub effect: Effect,
    /// The risks it takes.
    pub dangers: Vec<Danger>,
    /// The risks in words.
    pub risks: Vec<String>,
    /// Its bind parameters as `:name = value`, secret-looking values masked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<String>,
}

/// The connection a refusal is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The connection's id.
    pub connection: String,
    /// Its name.
    pub name: String,
    /// Its connection type's id.
    #[serde(rename = "type")]
    pub kind: String,
    /// The type's name.
    pub type_name: String,
    /// The type's theme colour.
    pub color: String,
    /// Whether the connection is read-only.
    pub read_only: bool,
    /// Whether statements that change something ask first.
    pub confirm_execute: bool,
    /// What agents may run on it.
    pub agents: AgentAccess,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guarded {
    pub statements: Vec<QueryStatement>,
    pub confirmed: bool,
    pub read_only: bool,
}

pub struct Run<'a> {
    pub entry: &'a Entry,
    pub context: &'a ContextKind,
    pub provenance: &'a Provenance,
    pub confirmation: &'a Confirmation,
    pub retry: (&'a str, serde_json::Value),
}

pub fn risk_text(danger: &Danger) -> String {
    let object = danger.object.clone().unwrap_or_default();
    let known = !object.is_empty();
    match (danger.risk, known) {
        (Risk::DeleteWithoutWhere, true) => t!("database-risk-delete", object = object.as_str()),
        (Risk::DeleteWithoutWhere, false) => t!("database-risk-delete-unknown"),
        (Risk::UpdateWithoutWhere, true) => t!("database-risk-update", object = object.as_str()),
        (Risk::UpdateWithoutWhere, false) => t!("database-risk-update-unknown"),
        (Risk::Drop, true) => t!("database-risk-drop", object = object.as_str()),
        (Risk::Drop, false) => t!("database-risk-drop-unknown"),
        (Risk::Truncate, true) => t!("database-risk-truncate", object = object.as_str()),
        (Risk::Truncate, false) => t!("database-risk-truncate-unknown"),
        (Risk::Flush, _) => t!("database-risk-flush"),
    }
}

pub fn target(session: &Session, entry: &Entry) -> Target {
    let kind = type_of(session, entry);
    Target {
        connection: entry.source.id.clone(),
        name: entry.source.name.clone(),
        kind: kind.id.clone(),
        type_name: kind.name.clone(),
        color: kind.color.clone(),
        read_only: entry.source.read_only || kind.read_only,
        confirm_execute: kind.confirm_execute,
        agents: kind.agent_access(),
    }
}

fn listed(index: usize, statement: &QueryStatement, assessment: &Assessment) -> Listed {
    let cut = statement.shown().chars().count() > MOST_TEXT;
    Listed {
        index,
        text: statement.shown().chars().take(MOST_TEXT).collect(),
        cut,
        effect: assessment.effect,
        dangers: assessment.dangers.clone(),
        risks: assessment.dangers.iter().map(risk_text).collect(),
        parameters: zen_db::params::shown(&statement.params, zen_db::dialect::SqlDialect::Postgres),
    }
}

fn summaries(statements: &[&QueryStatement]) -> String {
    statements
        .iter()
        .take(3)
        .map(|statement| {
            let line = statement.shown().lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
            let short: String = line.chars().take(60).collect();
            if short.len() < line.len() { format!("{short}…") } else { short }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn token_of(connection: &str, statements: &[QueryStatement]) -> String {
    let mut hasher = DefaultHasher::new();
    connection.hash(&mut hasher);
    for statement in statements {
        statement.text.hash(&mut hasher);
        serde_json::to_string(&statement.params).unwrap_or_default().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn assess(session: &mut Session, entry: &Entry, statements: &[QueryStatement]) -> Vec<Assessment> {
    let driver = engine(session).driver(&entry.source.driver);
    statements
        .iter()
        .map(|statement| match &driver {
            _ if statement.client => Assessment::of(Effect::Session),
            Some(driver) => driver.assess(&statement.text),
            None => Assessment::of(Effect::Other),
        })
        .collect()
}

fn record(session: &mut Session, run: &Run<'_>, texts: Vec<String>, code: &str, detail: String) {
    let error = DbError::new(ErrorKind::Refused, detail).with_code(code);
    engine(session).record_refused(&run.entry.source.id, run.context, &texts, &error, run.provenance);
}

fn refuse(
    session: &mut Session,
    run: &Run<'_>,
    (statements, assessed, offending): (&[QueryStatement], &[Assessment], Vec<usize>),
    (code, message): (&str, String),
    hand_off: bool,
) -> CommandError {
    let texts: Vec<String> = offending.iter().map(|&index| statements[index].shown().to_owned()).collect();
    let verb = texts.first().and_then(|text| zen_db::sql::first_keyword(text)).unwrap_or_default();
    record(session, run, texts, code, verb);
    let shown: Vec<Listed> =
        offending.iter().take(MOST_LISTED).map(|&index| listed(index, &statements[index], &assessed[index])).collect();
    let mut data = serde_json::json!({
        "target": target(session, run.entry),
        "statements": shown,
        "total": offending.len(),
        "truncated": offending.len() > MOST_LISTED,
    });
    if hand_off {
        let text = statements.iter().map(|statement| statement.shown().trim()).collect::<Vec<_>>().join(";\n");
        data["handOff"] = serde_json::json!({
            "command": "db.console.new",
            "params": { "connection": run.entry.source.id, "text": format!("{text};\n") },
        });
    }
    CommandError::with_code(code, message).with_data(data)
}

fn reads_local_files(session: &mut Session, entry: &Entry) -> bool {
    engine(session)
        .driver(&entry.source.driver)
        .is_some_and(|driver| LOCAL_FILES_LANGUAGES.contains(&driver.capabilities().language.as_str()))
}

pub fn guard(session: &mut Session, run: &Run<'_>, statements: Vec<QueryStatement>) -> Result<Guarded, CommandError> {
    let entry = run.entry;
    let kind = type_of(session, entry);
    let assessed = assess(session, entry, &statements);
    let agent = zen_core::macros::by_agent(session);
    let access = kind.agent_access();
    let name = entry.source.name.as_str();
    let all: Vec<usize> = (0..statements.len()).collect();
    if agent && access == AgentAccess::None {
        let message = t!("database-error-agent-not-allowed", name = name, kind = kind.name.as_str());
        return Err(refuse(session, run, (&statements, &assessed, all), (AGENT_NOT_ALLOWED, message), false));
    }
    if agent && reads_local_files(session, entry) {
        let outside: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&index| !statements[index].client && zen_db::safety::reaches_outside(&statements[index].text))
            .collect();
        if !outside.is_empty() {
            let shown: Vec<&QueryStatement> = outside.iter().map(|&index| &statements[index]).collect();
            let message = t!("database-error-agent-outside", name = name, statements = summaries(&shown));
            return Err(refuse(session, run, (&statements, &assessed, outside), (AGENT_OUTSIDE, message), true));
        }
    }
    let changing: Vec<usize> = all.iter().copied().filter(|&index| !assessed[index].allowed_read_only()).collect();
    let server_judges = engine(session).driver(&entry.source.driver).is_some_and(|driver| driver.guards_read_only());
    if (entry.source.read_only || kind.read_only) && !server_judges && !changing.is_empty() {
        let shown: Vec<&QueryStatement> = changing.iter().map(|&index| &statements[index]).collect();
        let message = t!("database-error-read-only-run", name = name, statements = summaries(&shown));
        return Err(refuse(session, run, (&statements, &assessed, changing), (READ_ONLY, message), false));
    }
    let agent_reads = agent && access == AgentAccess::ReadOnly;
    if agent_reads && !changing.is_empty() {
        let shown: Vec<&QueryStatement> = changing.iter().map(|&index| &statements[index]).collect();
        let message = t!(
            "database-error-agent-read-only",
            name = name,
            kind = kind.name.as_str(),
            statements = summaries(&shown)
        );
        return Err(refuse(session, run, (&statements, &assessed, changing), (AGENT_READ_ONLY, message), true));
    }
    let asked: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&index| (kind.confirm_execute && assessed[index].changes()) || assessed[index].dangerous())
        .collect();
    if asked.is_empty() {
        return Ok(Guarded { statements, confirmed: false, read_only: agent_reads });
    }
    let token = token_of(&entry.source.id, &statements);
    if run.confirmation.confirmed.as_deref() == Some(token.as_str()) {
        let skipped: Vec<usize> = asked.iter().copied().filter(|index| run.confirmation.skip.contains(index)).collect();
        let texts = skipped.iter().map(|&index| statements[index].shown().to_owned()).collect();
        record(session, run, texts, SKIPPED, String::new());
        let kept: Vec<QueryStatement> = statements
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !skipped.contains(index))
            .map(|(_, statement)| statement)
            .collect();
        if kept.iter().all(|statement| statement.client) {
            return Err(CommandError::with_code(SKIPPED, t!("database-error-all-skipped")));
        }
        return Ok(Guarded { statements: kept, confirmed: true, read_only: agent_reads });
    }
    let dangerous = asked.iter().any(|&index| assessed[index].dangerous());
    let risks: Vec<String> =
        asked.iter().flat_map(|&index| assessed[index].dangers.iter().map(risk_text)).take(3).collect();
    let auto_commit = super::transactions::state_of(session, entry, run.context, 0).auto_commit;
    let commit = t!("database-confirm-commit", commit = if auto_commit { "auto" } else { "manual" });
    let message = if dangerous {
        t!("database-error-dangerous", name = name, risks = risks.join("; "))
    } else {
        t!("database-error-confirm-execute", name = name, kind = kind.name.as_str(), count = asked.len())
    };
    let message = format!("{message} {commit}");
    let shown: Vec<Listed> =
        asked.iter().take(MOST_LISTED).map(|&index| listed(index, &statements[index], &assessed[index])).collect();
    let (command, mut params) = run.retry.clone();
    if let Some(object) = params.as_object_mut() {
        object.insert("confirmed".to_owned(), serde_json::json!(token));
    }
    Err(CommandError::with_code(CONFIRM_EXECUTE, message).with_data(serde_json::json!({
        "token": token,
        "target": target(session, entry),
        "statements": shown,
        "asked": asked.len(),
        "total": statements.len(),
        "truncated": asked.len() > MOST_LISTED,
        "dangerous": dangerous,
        "autoCommit": auto_commit,
        "retry": { "command": command, "params": params },
    })))
}

pub fn agent_reads(session: &mut Session, entry: &Entry, sql: &str) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let kind = type_of(session, entry);
    let name = entry.source.name.as_str();
    if kind.agent_access() != AgentAccess::None
        && reads_local_files(session, entry)
        && zen_db::safety::reaches_outside(sql)
    {
        let line = sql.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
        let short: String = line.chars().take(60).collect();
        return Err(CommandError::with_code(
            AGENT_OUTSIDE,
            t!("database-error-agent-outside", name = name, statements = short),
        ));
    }
    match kind.agent_access() {
        AgentAccess::Full => Ok(()),
        AgentAccess::None => Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = name, kind = kind.name.as_str()),
        )),
        AgentAccess::ReadOnly => {
            let reads = engine(session)
                .driver(&entry.source.driver)
                .is_some_and(|driver| driver.assess(sql).allowed_read_only());
            if reads {
                return Ok(());
            }
            Err(CommandError::with_code(
                AGENT_READ_ONLY,
                t!("database-error-agent-change", name = name, kind = kind.name.as_str()),
            ))
        }
    }
}

pub fn agent_sees(session: &Session, connection: &str) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let Ok(entry) = find(session, connection) else {
        return Ok(());
    };
    let kind = type_of(session, &entry);
    if kind.agent_access() == AgentAccess::None {
        return Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = entry.source.name.as_str(), kind = kind.name.as_str()),
        ));
    }
    Ok(())
}

pub fn agent_changes(session: &Session, entry: &Entry) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let kind = type_of(session, entry);
    match kind.agent_access() {
        AgentAccess::Full => Ok(()),
        AgentAccess::ReadOnly => Err(CommandError::with_code(
            AGENT_READ_ONLY,
            t!("database-error-agent-change", name = entry.source.name.as_str(), kind = kind.name.as_str()),
        )),
        AgentAccess::None => Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = entry.source.name.as_str(), kind = kind.name.as_str()),
        )),
    }
}

pub fn agent_loosens_source(session: &Session, entry: &Entry, fields: &SourceFields) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let (types, _) = connection_types(session);
    let before = types.resolve(&entry.source.connection_type).with_overrides(&entry.source.overrides);
    let kind = fields.connection_type.as_deref().unwrap_or(&entry.source.connection_type);
    let after = types.resolve(kind).with_overrides(fields.overrides.as_ref().unwrap_or(&entry.source.overrides));
    let guards =
        |profile: Option<&str>| profile.and_then(zen_db::profile::get).is_none_or(|profile| profile.quirks.guards);
    let unguarded = match fields.profile.as_deref() {
        Some(profile) if !profile.is_empty() => guards(entry.source.profile.as_deref()) && !guards(Some(profile)),
        _ => false,
    };
    if (entry.source.read_only && fields.read_only == Some(false)) || after.looser_than(&before) || unguarded {
        return Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-loosen", name = entry.source.name.as_str()),
        ));
    }
    Ok(())
}

pub fn agent_loosens_type(
    session: &Session,
    before: &ConnectionType,
    after: &ConnectionType,
) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) && after.looser_than(before) {
        return Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-loosen-type", name = before.name.as_str()),
        ));
    }
    Ok(())
}

/// Read-only to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadOnlyParams {
    /// The connection's id.
    pub connection: String,
    /// Read-only or not; the other way round when absent.
    pub read_only: Option<bool>,
}

/// A connection's read-only state.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadOnlyAnswer {
    /// The connection's id.
    pub connection: String,
    /// Whether it is read-only now, by its own setting or its type's.
    pub read_only: bool,
    /// Its own setting.
    pub own: bool,
    /// Whether its connection type makes it read-only whatever its own setting.
    pub by_type: bool,
    /// The operations changing its open sessions, when connected.
    pub operations: Vec<u64>,
}

pub struct SetReadOnly;

impl Command for SetReadOnly {
    type Params = ReadOnlyParams;
    type Output = ReadOnlyAnswer;
    const ID: &'static str = "db.connection.readOnly";
    const TITLE: &'static str = "Read-Only Connection";
    const DESCRIPTION: &'static str = "Makes database connection `connection` read-only (`readOnly: true`), writable (`false`) or the other way round (absent), as the toolbar's lock does: kept as the person's own setting of a project connection, and applied at once to its open sessions (read-only on the server where the database has it; changes, DDL and data edits refused before they are sent). An agent may turn read-only on, never off (code `agentNotAllowed`). A read-only connection type keeps it read-only (`byType`). Connects nowhere.";

    fn run(&self, session: &mut Session, params: ReadOnlyParams) -> Result<ReadOnlyAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let own = params.read_only.unwrap_or(!entry.source.read_only);
        if !own && entry.source.read_only && zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(
                AGENT_NOT_ALLOWED,
                t!("database-error-agent-loosen", name = entry.source.name.as_str()),
            ));
        }
        if own != entry.source.read_only {
            let fields = SourceFields { read_only: Some(own), ..SourceFields::default() };
            super::sources::update(session, &params.connection, &[&fields], None)?;
        }
        let by_type = type_of(session, &entry).read_only;
        let operations = session
            .state::<super::Database>()
            .and_then(super::Database::started)
            .map(|engine| engine.set_read_only(&params.connection, own))
            .unwrap_or_default();
        Ok(ReadOnlyAnswer { connection: params.connection, read_only: own || by_type, own, by_type, operations })
    }
}

#[cfg(test)]
mod tests;
