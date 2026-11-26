use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;

use super::{Terminals, has_sessions, missing_session, started};

pub const AGENTS: &str = "terminal.agents";

pub const SESSION: &str = "{session}";

pub const TERMINAL_ID: &str = "ZEN_TERMINAL_ID";

pub const AGENTS_FOLDER: &str = "ZEN_TERMINAL_AGENTS";

pub type AgentSource = Arc<dyn Fn() -> Vec<(serde_json::Value, bool)> + Send + Sync>;

/// How the IDE knows an AI agent in a terminal and resumes its conversation after a restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentAdapter {
    /// A short id of letters, digits and dashes, such as `claude-code`; it names the agent's report file.
    pub id: String,
    /// The agent's name as people know it.
    pub name: String,
    /// The program names that run the agent (`claude`); one of the first three words of the running
    /// program's command line must be one of them, without its folder and extension.
    pub programs: Vec<String>,
    /// The command that resumes a conversation, word by word, with `{session}` where its id goes.
    pub resume: Vec<String>,
    /// Where the id is in the report the agent's hook writes, as a JSON pointer (`/session_id` by default).
    #[serde(default = "default_session_field")]
    pub session_field: String,
    /// The agent's documentation of resuming, a web address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

fn default_session_field() -> String {
    "/session_id".to_owned()
}

impl AgentAdapter {
    pub fn parse(value: &serde_json::Value) -> Option<AgentAdapter> {
        let adapter: AgentAdapter = serde_json::from_value(value.clone()).ok()?;
        adapter.is_valid().then_some(adapter)
    }

    pub fn is_valid(&self) -> bool {
        let word = |text: &str| !text.is_empty() && !text.chars().any(char::is_control);
        valid_id(&self.id)
            && word(&self.name)
            && !self.programs.is_empty()
            && self.programs.iter().all(|program| word(program) && !program.contains(['/', ' ']))
            && self.resume.first().is_some_and(|program| word(program) && program != SESSION)
            && self.resume.iter().all(|part| word(part))
            && self.resume.iter().filter(|part| part.contains(SESSION)).count() == 1
            && self.session_field.starts_with('/')
    }

    pub fn recognises(&self, command: &[String]) -> bool {
        command.iter().take(3).any(|word| {
            let name = word.rsplit('/').next().unwrap_or(word);
            let stem = name.strip_suffix(".js").or_else(|| name.strip_suffix(".mjs")).unwrap_or(name);
            self.programs.iter().any(|program| program == stem || program == name)
        })
    }

    pub fn session_of(&self, report: &serde_json::Value) -> Option<String> {
        let id = report.pointer(&self.session_field)?.as_str()?;
        valid_session(id).then(|| id.to_owned())
    }

    pub fn resume_line(&self, session: &str) -> Option<String> {
        if !valid_session(session) {
            return None;
        }
        let words: Vec<String> = self.resume.iter().map(|part| quote(&part.replace(SESSION, session))).collect();
        Some(words.join(" "))
    }

    pub fn hook(&self) -> String {
        hook_command(&self.id)
    }
}

pub fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && !id.starts_with('-')
}

pub fn valid_terminal(terminal: &str) -> bool {
    (1..=64).contains(&terminal.len()) && terminal.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

pub fn valid_session(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
        && id.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric())
}

pub fn hook_command(agent: &str) -> String {
    format!(
        "[ -n \"${TERMINAL_ID}\" ] && [ -d \"${AGENTS_FOLDER}\" ] && cat > \"${AGENTS_FOLDER}/${TERMINAL_ID}-{agent}.json\"; true"
    )
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/' | '=' | '@'));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', "'\\''")) }
}

pub fn from_values(values: &[serde_json::Value]) -> Vec<AgentAdapter> {
    values.iter().filter_map(AgentAdapter::parse).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdapterSource {
    Plugin,
    BundledPlugin,
    Settings,
}

impl AdapterSource {
    pub fn resumes_by_itself(self) -> bool {
        self != AdapterSource::Plugin
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedAdapter {
    #[serde(flatten)]
    pub adapter: AgentAdapter,
    pub source: AdapterSource,
    pub hook: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterList {
    pub adapters: Vec<ListedAdapter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reports: Option<std::path::PathBuf>,
}

impl Terminals {
    pub fn set_plugin_agents(&mut self, source: AgentSource) {
        self.plugin_agents = Some(source);
    }

    pub fn set_user_agents(&mut self, adapters: Vec<AgentAdapter>) {
        self.user_agents = adapters;
    }

    pub fn adapters(&self) -> Vec<(AgentAdapter, AdapterSource)> {
        let mut all: Vec<(AgentAdapter, AdapterSource)> = self
            .user_agents
            .iter()
            .filter(|adapter| adapter.is_valid())
            .map(|adapter| (adapter.clone(), AdapterSource::Settings))
            .collect();
        let plugins = self.plugin_agents.as_ref().map(|source| source()).unwrap_or_default();
        for (value, bundled) in plugins {
            let Some(adapter) = AgentAdapter::parse(&value) else { continue };
            if all.iter().all(|(known, _)| known.id != adapter.id) {
                let source = if bundled { AdapterSource::BundledPlugin } else { AdapterSource::Plugin };
                all.push((adapter, source));
            }
        }
        all
    }

    pub fn adapter(&self, id: &str) -> Option<AgentAdapter> {
        self.adapters().into_iter().map(|(adapter, _)| adapter).find(|adapter| adapter.id == id)
    }
}

/// The parameters of `terminal.agent.report`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportParams {
    /// The terminal session's id from `terminal.session.list`; else `terminal`, else the focused session.
    #[serde(default)]
    pub id: Option<u64>,
    /// The terminal's `ZEN_TERMINAL_ID`, as an agent's hook sees it in its environment.
    #[serde(default)]
    pub terminal: Option<String>,
    /// The adapter's id from `terminal.agent.list`, such as `claude-code`.
    pub agent: String,
    /// The agent's conversation id: letters, digits and `-_.:`, at most 128.
    pub session: String,
}

pub struct ListAgents;

impl Command for ListAgents {
    type Params = NoParams;
    type Output = AdapterList;
    const ID: &'static str = "terminal.agent.list";
    const TITLE: &'static str = "Terminal Agent Adapters";
    const DESCRIPTION: &'static str = "Lists the agent resume adapters: from plug-ins and the user's Settings (Tools → \
         Terminal → AI Agents), each with `id`, `name`, `programs`, `resume` (with `{session}`), `sessionField`, \
         `docs`, `source` and `hook`, the command an agent's session-start hook runs to report its conversation id \
         to the IDE (it writes the hook's JSON input to `reports`, the folder in `ZEN_TERMINAL_AGENTS`, as \
         `<ZEN_TERMINAL_ID>-<id>.json`). After a restart a tab whose running program an adapter recognises, and whose \
         id was reported, runs the resume command in its new shell when the adapter is the user's or a bundled \
         plug-in's (`source` `settings` or `bundledPlugin`); another plug-in's is only offered to the person.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<AdapterList, CommandError> {
        super::sync_settings(session);
        let terminals = session.state_mut::<Terminals>();
        let adapters = terminals
            .adapters()
            .into_iter()
            .map(|(adapter, source)| ListedAdapter { hook: adapter.hook(), adapter, source })
            .collect();
        Ok(AdapterList { adapters, reports: terminals.host().and_then(zen_terminal::Host::agents_folder) })
    }
}

pub struct ReportAgent;

impl Command for ReportAgent {
    type Params = ReportParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.agent.report";
    const TITLE: &'static str = "Report Terminal Agent Session";
    const DESCRIPTION: &'static str = "Tells the IDE the conversation id of the agent running in a terminal (`id`, or \
         `terminal` as in `ZEN_TERMINAL_ID`), for the adapter `agent`, so that after a restart the tab resumes that \
         conversation; the same as the file an agent's hook writes. Only for an adapter that exists: agents cannot \
         add or change adapters (set_settings refuses `terminalAgents`). Only the adapter's own resume command is ever \
         run with it, the id quoted.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: ReportParams) -> Result<NoParams, CommandError> {
        super::sync_settings(session);
        let terminals = started(session)?;
        let adapter = terminals
            .adapter(&params.agent)
            .ok_or_else(|| CommandError::new(t!("terminal-agent-unknown", agent = params.agent.clone())))?;
        if !valid_session(&params.session) {
            return Err(CommandError::new(t!("terminal-agent-bad-session")));
        }
        let terminal = match (params.id, params.terminal) {
            (Some(id), _) => terminals.pane(id).ok_or_else(|| missing_session(id))?.terminal().to_owned(),
            (None, Some(terminal)) => terminals
                .tabs
                .iter()
                .flat_map(|tab| tab.root.panes())
                .find(|pane| pane.terminal() == terminal)
                .map(|pane| pane.terminal().to_owned())
                .ok_or_else(|| CommandError::new(t!("terminal-agent-no-terminal", terminal = terminal.clone())))?,
            (None, None) => {
                let id = terminals.session_or_focused(None)?;
                terminals.pane(id).ok_or_else(|| missing_session(id))?.terminal().to_owned()
            }
        };
        let host = terminals.host().ok_or_else(|| CommandError::new(t!("terminal-agent-no-host")))?;
        let mut report = serde_json::Map::new();
        let field = adapter.session_field.trim_start_matches('/').to_owned();
        report.insert(field, serde_json::Value::String(params.session));
        host.report_agent(&terminal, &adapter.id, &serde_json::Value::Object(report))
            .map_err(|error| CommandError::new(t!("terminal-agent-report-failed", error = error.to_string())))?;
        Ok(NoParams {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> AgentAdapter {
        AgentAdapter {
            id: "claude-code".to_owned(),
            name: "Claude Code".to_owned(),
            programs: vec!["claude".to_owned()],
            resume: vec!["claude".to_owned(), "--resume".to_owned(), SESSION.to_owned()],
            session_field: default_session_field(),
            docs: None,
        }
    }

    #[test]
    fn an_adapter_knows_its_program_also_under_node_and_builds_a_quoted_resume() {
        let adapter = claude();
        assert!(adapter.is_valid());
        assert!(adapter.recognises(&["/usr/local/bin/claude".to_owned(), "--model".to_owned()]));
        assert!(adapter.recognises(&["node".to_owned(), "/opt/homebrew/bin/claude".to_owned()]));
        assert!(!adapter.recognises(&["vim".to_owned(), "claude.md".to_owned()]));
        assert_eq!(adapter.resume_line("550e8400-e29b"), Some("claude --resume 550e8400-e29b".to_owned()));
        assert_eq!(adapter.resume_line("x; rm -rf ~"), None, "an id is never more than an id");
        assert_eq!(adapter.session_of(&serde_json::json!({ "session_id": "abc" })), Some("abc".to_owned()));
        assert_eq!(adapter.session_of(&serde_json::json!({ "session_id": "$(boom)" })), None);
    }

    #[test]
    fn an_adapter_without_one_place_for_the_id_or_with_a_bad_id_is_refused() {
        let mut adapter = claude();
        adapter.resume = vec!["claude".to_owned(), "--continue".to_owned()];
        assert!(!adapter.is_valid());
        let mut adapter = claude();
        adapter.id = "Claude Code".to_owned();
        assert!(!adapter.is_valid());
        let mut adapter = claude();
        adapter.resume = vec![SESSION.to_owned()];
        assert!(!adapter.is_valid(), "the id is never the program");
        assert_eq!(quote("it's"), "'it'\\''s'");
    }

    #[cfg(unix)]
    #[test]
    fn a_reported_id_never_reaches_a_shell_as_anything_but_one_word() {
        let mut adapter = claude();
        adapter.resume = vec!["printf".to_owned(), "[%s]\\n".to_owned(), "it's {session}".to_owned()];
        assert!(adapter.is_valid());
        for evil in [
            "$(touch x)",
            "`id`",
            "a b",
            "a;b",
            "-rf",
            "--x",
            "a\nb",
            "a'b",
            "a\"b",
            "a|b",
            "a&b",
            "${HOME}",
            "~",
            "*",
            "",
        ] {
            assert_eq!(adapter.resume_line(evil), None, "{evil:?} is not an id");
            assert_eq!(adapter.session_of(&serde_json::json!({ "session_id": evil })), None);
        }
        for good in ["conv-42", "550e8400-e29b-41d4-a716-446655440000", "a.b:c_d"] {
            let line = adapter.resume_line(good).unwrap();
            let out = std::process::Command::new("/bin/sh").args(["-c", &line]).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), format!("[it's {good}]\n"), "{line}");
        }
    }

    #[test]
    fn the_hook_writes_its_input_where_the_ide_reads_it() {
        assert_eq!(
            hook_command("codex"),
            "[ -n \"$ZEN_TERMINAL_ID\" ] && [ -d \"$ZEN_TERMINAL_AGENTS\" ] && cat > \"$ZEN_TERMINAL_AGENTS/$ZEN_TERMINAL_ID-codex.json\"; true"
        );
    }
}
