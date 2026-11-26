use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{AnythingPrefix, Configuration};

use crate::Workbench;
use crate::debug::StartSession;
use crate::project_model::ProjectModels;
use crate::run::{Runs, StartParams, StartRun, project_root, read_file, run_context};
use crate::terminal::{NewParams, NewSession, SendParams, SendText};

pub const HISTORY_LIMIT: usize = 50;

pub const HELP: &str = "?";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnythingHistory {
    entries: Vec<String>,
}

impl AnythingHistory {
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn set_entries(&mut self, entries: Vec<String>) {
        self.entries = Vec::new();
        for entry in entries.into_iter().rev() {
            self.remember(&entry);
        }
    }

    pub fn remember(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || text == HELP {
            return;
        }
        self.entries.retain(|entry| entry != text);
        self.entries.insert(0, text.to_owned());
        self.entries.truncate(HISTORY_LIMIT);
    }
}

/// The folder a command typed in Run Anything runs in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AnythingContext {
    /// The project's folder.
    #[default]
    Project,
    /// The folder of the module the current file belongs to.
    Module,
    /// The current file's folder.
    File,
}

/// What a suggestion of Run Anything is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    /// A command run before, newest first.
    Recent,
    /// A run configuration.
    Configuration,
    /// A command a run provider offers, e.g. `go test ./...`.
    Command,
    /// A prefix and what it runs, as `?` lists them.
    Help,
}

/// One suggestion of Run Anything.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnythingItem {
    pub kind: ItemKind,
    /// The text run.anything.run takes: a command line, a configuration's name, or a prefix to type further.
    pub text: String,
    /// The configuration's id, for a configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_id: Option<String>,
    /// Whether the configuration is a temporary one, shown greyed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub temporary: bool,
    /// Whether the configuration is adopted from another IDE's files.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adopted: bool,
    /// What it is or does: a configuration's kind, a prefix's commands.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// The parameters of `run.anything.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// What is typed; `?` lists the prefixes.
    pub query: String,
}

/// What `run.anything.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnythingList {
    /// The suggestions in the popup's order: recent commands, then run configurations, then the providers' commands.
    pub items: Vec<AnythingItem>,
}

/// The parameters of `run.anything.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunParams {
    /// What to run: a configuration's name, a provider's command (`go test ./...`, `cargo build`,
    /// `mvn clean install`, `gradle build`), or any other command line, which runs in a new Terminal tab.
    pub text: String,
    /// A configuration to run by its id instead of `text`.
    pub configuration_id: Option<String>,
    /// Starts a configuration under the debugger (⇧); a command runs as usual.
    pub debug: bool,
    /// The folder a command runs in.
    pub context: AnythingContext,
    /// The file `module` and `file` refer to; the active editor's when absent.
    pub path: Option<PathBuf>,
}

/// How `run.anything.run` ran it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Way {
    /// A run configuration, now the selected one.
    Configuration,
    /// A run provider's command, in the Run tool window.
    Command,
    /// A command line, in a new Terminal tab.
    Terminal,
}

/// What `run.anything.run` started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnythingStarted {
    pub way: Way,
    /// The run, for `run.output.get`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// The debug session, when it was debugged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u64>,
    /// The terminal session, for `terminal.session.read`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<u64>,
    /// The folder a command ran in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

fn prefixes(session: &Session) -> Vec<AnythingPrefix> {
    let providers = session.state::<Runs>().map(|runs| runs.providers().to_vec()).unwrap_or_default();
    let mut prefixes: Vec<AnythingPrefix> = Vec::new();
    for prefix in providers.iter().flat_map(|provider| provider.anything_prefixes()) {
        if !prefixes.iter().any(|known| known.prefix == prefix.prefix) {
            prefixes.push(prefix);
        }
    }
    prefixes
}

fn matches(query: &str, text: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || {
        let text = text.to_lowercase();
        text.contains(&query) || zen_index::NamePattern::new(&query).matches(&text).is_some()
    }
}

pub fn list(session: &mut Session, query: &str) -> Result<Vec<AnythingItem>, CommandError> {
    if query.trim() == HELP {
        return Ok(prefixes(session)
            .into_iter()
            .map(|prefix| AnythingItem {
                kind: ItemKind::Help,
                text: format!("{} ", prefix.prefix),
                configuration_id: None,
                temporary: false,
                adopted: false,
                description: t!(&prefix.help_message),
            })
            .collect());
    }
    let item = |kind, text: &str| AnythingItem {
        kind,
        text: text.to_owned(),
        configuration_id: None,
        temporary: false,
        adopted: false,
        description: String::new(),
    };
    // WHY: a person's recent commands can hold secrets typed on a command line; agents do not read them.
    let history = if zen_core::macros::by_agent(session) {
        Vec::new()
    } else {
        session.state::<AnythingHistory>().map(|history| history.entries.clone()).unwrap_or_default()
    };
    let mut items: Vec<AnythingItem> =
        history.iter().filter(|text| matches(query, text)).map(|text| item(ItemKind::Recent, text)).collect();
    let root = project_root(session)?;
    let file = read_file(session, &root)?;
    let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
    let mut configurations: Vec<&Configuration> =
        file.configurations.iter().filter(|kept| matches(query, &kept.name)).collect();
    configurations.sort_by_key(|kept| kept.temporary);
    items.extend(configurations.into_iter().map(|kept| {
        AnythingItem {
            configuration_id: Some(kept.id.clone()),
            temporary: kept.temporary,
            adopted: kept.is_adopted(),
            description: kinds
                .iter()
                .find(|kind| kind.spec.id == kept.kind)
                .map(|kind| t!(&kind.spec.name_message))
                .unwrap_or_default(),
            ..item(ItemKind::Configuration, &kept.name)
        }
    }));
    let typed = query.trim();
    if !typed.is_empty() {
        let lower = typed.to_lowercase();
        let first = zen_run::args::split(typed).into_iter().next().unwrap_or_default();
        for prefix in prefixes(session) {
            let description = t!(&prefix.help_message);
            let mut commands: Vec<String> = Vec::new();
            if first == prefix.prefix && !prefix.commands.iter().any(|command| command == typed) {
                commands.push(typed.to_owned());
            }
            commands.extend(prefix.commands.iter().filter(|command| command.starts_with(&lower)).cloned());
            for command in commands {
                if !items.iter().any(|known| known.text == command) {
                    items.push(AnythingItem { description: description.clone(), ..item(ItemKind::Command, &command) });
                }
            }
        }
    }
    Ok(items)
}

pub fn working_directory(
    session: &mut Session,
    context: AnythingContext,
    path: Option<&Path>,
) -> Result<PathBuf, CommandError> {
    let root = project_root(session)?;
    if context == AnythingContext::Project {
        return Ok(root);
    }
    let file = match path {
        Some(path) if path.is_absolute() => Some(path.to_owned()),
        Some(path) => Some(root.join(path)),
        None => {
            session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
        }
    };
    let Some(file) = file else { return Ok(root) };
    let folder = if file.is_dir() { file.clone() } else { file.parent().map_or_else(|| root.clone(), Path::to_owned) };
    if context == AnythingContext::File {
        return Ok(folder);
    }
    let models = session.state_mut::<ProjectModels>();
    models.ensure(Some(&root));
    let module = models
        .model()
        .modules
        .iter()
        .filter(|module| !module.root.as_os_str().is_empty() && folder.starts_with(&module.root))
        .max_by_key(|module| module.root.components().count())
        .map(|module| module.root.clone());
    Ok(module.unwrap_or(root))
}

pub fn run(session: &mut Session, params: RunParams) -> Result<AnythingStarted, CommandError> {
    let root = project_root(session)?;
    let text = params.text.trim().to_owned();
    let file = read_file(session, &root)?;
    let chosen = match &params.configuration_id {
        Some(id) => Some(
            file.get(id)
                .cloned()
                .ok_or_else(|| CommandError::new(t!("workbench-run-unknown-configuration", id = id.as_str())))?,
        ),
        None => file.configurations.iter().find(|kept| kept.name == text).cloned(),
    };
    if let Some(configuration) = chosen {
        remember(session, &configuration.name);
        let start = StartParams { configuration_id: Some(configuration.id), ..StartParams::default() };
        if params.debug {
            let started = StartSession.run(session, start)?;
            return Ok(AnythingStarted {
                way: Way::Configuration,
                run_id: Some(started.run_id),
                session_id: Some(started.session_id),
                terminal_id: None,
                cwd: None,
            });
        }
        let started = StartRun.run(session, start)?;
        return Ok(AnythingStarted {
            way: Way::Configuration,
            run_id: Some(started.run_id),
            session_id: None,
            terminal_id: None,
            cwd: None,
        });
    }
    if text.is_empty() || text == HELP {
        return Err(CommandError::new(t!("workbench-run-anything-empty")));
    }
    crate::trust::guard_agent_project(session)?;
    let cwd = working_directory(session, params.context, params.path.as_deref())?;
    let words = zen_run::args::split(&text);
    let providers = session.state::<Runs>().map(|runs| runs.providers().to_vec()).unwrap_or_default();
    let context = run_context(session, &root);
    if let Some(found) = providers.iter().find_map(|provider| provider.anything(&words, &cwd, &context)) {
        let configuration = found.map_err(|error| {
            CommandError::new(t!("workbench-run-cannot-start", name = text.as_str(), error = error))
        })?;
        remember(session, &text);
        if params.debug && configuration.kind != zen_run::external_tool::KIND {
            let start = StartParams { configuration: Some(configuration), ..StartParams::default() };
            let started = StartSession.run(session, start)?;
            return Ok(AnythingStarted {
                way: Way::Command,
                run_id: Some(started.run_id),
                session_id: Some(started.session_id),
                terminal_id: None,
                cwd: Some(cwd),
            });
        }
        crate::trust::guard_run(session, &project_root(session)?, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        return Ok(AnythingStarted {
            way: Way::Command,
            run_id: Some(started.run_id),
            session_id: None,
            terminal_id: None,
            cwd: Some(cwd),
        });
    }
    let opened = NewSession.run(session, NewParams { cwd: Some(cwd.clone()) })?;
    SendText.run(session, SendParams { id: Some(opened.id), text: format!("{text}\n") })?;
    remember(session, &text);
    Ok(AnythingStarted {
        way: Way::Terminal,
        run_id: None,
        session_id: None,
        terminal_id: Some(opened.id),
        cwd: Some(cwd),
    })
}

pub struct ListAnything;

impl Command for ListAnything {
    type Params = ListParams;
    type Output = AnythingList;
    const ID: &'static str = "run.anything.list";
    const TITLE: &'static str = "Run Anything Suggestions";
    const DESCRIPTION: &'static str = "What Run Anything (⌃⌃) suggests for a query: the recent commands of the \
         project's window, newest first, then the run configurations (permanent, adopted, temporary) whose names \
         match, then the commands the run providers offer for the typed prefix (`go`, `cargo`, `mvn`, `gradle`). \
         `?` lists the prefixes. Run one with run.anything.run.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<AnythingList, CommandError> {
        Ok(AnythingList { items: list(session, &params.query)? })
    }
}

pub struct RunAnything;

impl Command for RunAnything {
    type Params = RunParams;
    type Output = AnythingStarted;
    const ID: &'static str = "run.anything.run";
    const TITLE: &'static str = "Run Anything";
    const DESCRIPTION: &'static str = "Runs what is typed in Run Anything (⌃⌃) and remembers it among the recent \
         commands: a run configuration by name or configurationId (then the selected one; debug starts it under the \
         debugger as ⇧⏎ does), a run provider's command (`go test ./...`, `cargo build`, `mvn clean install`, \
         `gradle build`) in the Run tool window, or any other command line in a new Terminal tab. A command runs in \
         the project's folder, the current file's module (context module) or its folder (context file, as ⌥⏎).";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<AnythingStarted, CommandError> {
        run(session, params)
    }
}

fn remember(session: &mut Session, text: &str) {
    if !zen_core::macros::by_agent(session) {
        session.state_mut::<AnythingHistory>().remember(text);
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListAnything)?;
    registry.register(RunAnything)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::{Value, json};
    use zen_run::fake::FakeProvider;
    use zen_run::{CommandLine, KindSpec, OutputParser, PlainParser, RunContext, RunEnv, RunProvider, RunTarget};

    use super::*;

    struct Say;

    impl RunProvider for Say {
        fn id(&self) -> &str {
            "say"
        }

        fn kinds(&self) -> Vec<KindSpec> {
            Vec::new()
        }

        fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
            Vec::new()
        }

        fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
            None
        }

        fn command_lines(
            &self,
            _configuration: &Configuration,
            _context: &RunContext,
        ) -> Result<Vec<CommandLine>, String> {
            Err("not a kind".to_owned())
        }

        fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
            Box::new(PlainParser)
        }

        fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
            vec![AnythingPrefix {
                prefix: "say".to_owned(),
                help_message: "workbench-run-anything-empty".to_owned(),
                commands: vec!["say hello".to_owned(), "say goodbye".to_owned()],
            }]
        }

        fn anything(
            &self,
            words: &[String],
            cwd: &Path,
            _context: &RunContext,
        ) -> Option<Result<Configuration, String>> {
            let (first, args) = words.split_first()?;
            (first == "say").then(|| {
                Ok(zen_run::external_tool::command_configuration(
                    &zen_run::external_tool::joined(words),
                    PathBuf::from("/bin/sh"),
                    vec!["-c".to_owned(), format!("echo {} in $(pwd -P)", args.join(" "))],
                    cwd,
                    Vec::new(),
                ))
            })
        }
    }

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(FakeProvider::new()), Arc::new(Say)];
        session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        (registry, session, dir)
    }

    fn create(registry: &Registry, session: &mut Session, name: &str) -> String {
        let configuration = serde_json::to_value(FakeProvider::configuration(name)).unwrap();
        let created = registry.dispatch(session, "run.configuration.create", json!({ "configuration": configuration }));
        created.unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn texts(registry: &Registry, session: &mut Session, query: &str) -> Vec<(String, String)> {
        let listed = registry.dispatch(session, "run.anything.list", json!({ "query": query })).unwrap();
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| (item["kind"].as_str().unwrap().to_owned(), item["text"].as_str().unwrap().to_owned()))
            .collect()
    }

    fn pair(kind: &str, text: &str) -> (String, String) {
        (kind.to_owned(), text.to_owned())
    }

    fn output(registry: &Registry, session: &mut Session, run_id: u64) -> String {
        crate::run::wait_for_run(session, run_id, Duration::from_secs(60)).unwrap();
        let read = registry.dispatch(session, "run.output.get", json!({ "runId": run_id })).unwrap();
        read["lines"].as_array().unwrap().iter().filter_map(|line| line["text"].as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn the_empty_query_lists_recent_commands_then_configurations_with_temporary_ones_last() {
        let (registry, mut session, _dir) = setup();
        create(&registry, &mut session, "server");
        let mut temporary = FakeProvider::configuration("scratch");
        temporary.temporary = true;
        let temporary = serde_json::to_value(temporary).unwrap();
        registry.dispatch(&mut session, "run.configuration.create", json!({ "configuration": temporary })).unwrap();
        create(&registry, &mut session, "client");
        session.state_mut::<AnythingHistory>().set_entries(vec!["say hello".to_owned(), "make".to_owned()]);
        assert_eq!(
            texts(&registry, &mut session, ""),
            [
                pair("recent", "say hello"),
                pair("recent", "make"),
                pair("configuration", "server"),
                pair("configuration", "client"),
                pair("configuration", "scratch"),
            ]
        );
        let listed = registry.dispatch(&mut session, "run.anything.list", json!({ "query": "" })).unwrap();
        assert_eq!(listed["items"][4]["temporary"], json!(true));
        assert_eq!(listed["items"][2]["description"], json!(zen_i18n::t!("fake-run-kind")));
        assert_eq!(texts(&registry, &mut session, "serv"), [pair("configuration", "server")]);
        assert_eq!(texts(&registry, &mut session, "cl"), [pair("configuration", "client")]);
    }

    #[test]
    fn a_prefix_offers_the_providers_commands_and_question_mark_lists_the_prefixes() {
        let (registry, mut session, _dir) = setup();
        assert_eq!(texts(&registry, &mut session, "say h"), [pair("command", "say h"), pair("command", "say hello")]);
        assert_eq!(texts(&registry, &mut session, "say hello"), [pair("command", "say hello")]);
        assert_eq!(
            texts(&registry, &mut session, "sa"),
            [pair("command", "say hello"), pair("command", "say goodbye")]
        );
        assert!(texts(&registry, &mut session, "zzz").is_empty());
        assert_eq!(texts(&registry, &mut session, "?"), [pair("help", "say ")]);
    }

    #[test]
    fn agents_neither_read_the_recent_commands_nor_add_to_them() {
        let (registry, mut session, _dir) = setup();
        session.state_mut::<AnythingHistory>().set_entries(vec!["mysql -psecret".to_owned()]);
        let listed = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.anything.list", json!({ "query": "" })).unwrap()
        });
        assert!(!listed.to_string().contains("secret"), "{listed}");
        zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.anything.run", json!({ "text": "say hi" })).unwrap()
        });
        assert_eq!(session.state::<AnythingHistory>().unwrap().entries(), ["mysql -psecret"]);
        let listed = registry.dispatch(&mut session, "run.anything.list", json!({ "query": "" })).unwrap();
        assert!(listed.to_string().contains("mysql -psecret"), "the person still sees them: {listed}");
    }

    #[test]
    fn a_configuration_runs_by_name_becomes_selected_and_is_remembered_first() {
        let (registry, mut session, _dir) = setup();
        create(&registry, &mut session, "server");
        let client = create(&registry, &mut session, "client");
        session.state_mut::<AnythingHistory>().set_entries(vec!["say hello".to_owned()]);
        let started = registry.dispatch(&mut session, "run.anything.run", json!({ "text": "client" })).unwrap();
        assert_eq!(started["way"], json!("configuration"));
        assert!(started["runId"].is_u64());
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["selected"], json!(client));
        let history = session.state::<AnythingHistory>().unwrap().entries().to_vec();
        assert_eq!(history, ["client", "say hello"]);
        let unknown = registry.dispatch(&mut session, "run.anything.run", json!({ "configurationId": "nope" }));
        assert!(unknown.is_err());
        assert!(registry.dispatch(&mut session, "run.anything.run", json!({ "text": " " })).is_err());
    }

    #[test]
    fn a_providers_command_runs_in_the_run_tool_window_in_the_chosen_folder() {
        let (registry, mut session, dir) = setup();
        let root = dir.path().canonicalize().unwrap();
        let folder = root.join("pkg");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("main.txt");
        std::fs::write(&file, "x").unwrap();
        let started = registry.dispatch(&mut session, "run.anything.run", json!({ "text": "say hi" })).unwrap();
        assert_eq!(started["way"], json!("command"));
        let run_id = started["runId"].as_u64().unwrap();
        assert!(output(&registry, &mut session, run_id).contains(&format!("hi in {}", root.display())));
        let in_file = json!({ "text": "say there", "context": "file", "path": file });
        let started: Value = registry.dispatch(&mut session, "run.anything.run", in_file).unwrap();
        assert_eq!(started["cwd"], json!(folder));
        let run_id = started["runId"].as_u64().unwrap();
        assert!(output(&registry, &mut session, run_id).contains(&format!("there in {}", folder.display())));
        let history = session.state::<AnythingHistory>().unwrap().entries().to_vec();
        assert_eq!(history, ["say there", "say hi"]);
    }

    #[test]
    fn the_history_keeps_each_command_once_newest_first_and_at_most_its_limit() {
        let mut history = AnythingHistory::default();
        for index in 0..HISTORY_LIMIT + 5 {
            history.remember(&format!("command {index}"));
        }
        history.remember(" command 10 ");
        history.remember("?");
        assert_eq!(history.entries().len(), HISTORY_LIMIT);
        assert_eq!(history.entries()[0], "command 10");
        assert_eq!(history.entries().iter().filter(|entry| *entry == "command 10").count(), 1);
        let kept = history.entries().to_vec();
        let mut restored = AnythingHistory::default();
        restored.set_entries(kept.clone());
        assert_eq!(restored.entries(), kept);
    }

    #[test]
    fn the_module_context_is_the_module_holding_the_file() {
        let (_registry, mut session, dir) = setup();
        let root = dir.path().to_owned();
        let file = root.join("a/b/c.go");
        let project = working_directory(&mut session, AnythingContext::Project, Some(&file)).unwrap();
        assert_eq!(project, root);
        let folder = working_directory(&mut session, AnythingContext::File, Some(&file)).unwrap();
        assert_eq!(folder, root.join("a/b"));
        let module = working_directory(&mut session, AnythingContext::Module, Some(&file)).unwrap();
        assert_eq!(module, root, "no module: the project");
    }
}
