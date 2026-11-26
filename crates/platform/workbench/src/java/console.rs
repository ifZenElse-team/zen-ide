use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{CommandLine, RunContext};

use crate::Workbench;
use crate::java_run::JavaRun;
use crate::notifications::{Group, Kind, NewNotification};
use crate::terminal::Terminals;

pub const MAX_WAIT: Duration = Duration::from_secs(600);

/// Which interactive console: the JDK's JShell or the Kotlin REPL of `kotlinc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleKind {
    /// JShell, the project JDK's `jshell`.
    Jshell,
    /// The Kotlin REPL, `kotlinc` found on this computer.
    Kotlin,
}

impl ConsoleKind {
    fn title(self) -> String {
        match self {
            ConsoleKind::Jshell => t!("workbench-console-jshell"),
            ConsoleKind::Kotlin => t!("workbench-console-kotlin"),
        }
    }

    fn classpath_flag(self) -> &'static str {
        match self {
            ConsoleKind::Jshell => "--class-path",
            ConsoleKind::Kotlin => "-cp",
        }
    }
}

/// The parameters of `java.jshell.open` and `kotlin.repl.open`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The module whose classpath the console gets, relative to the project (`app`, `libs/core`);
    /// the module of the active editor's file by default, else the project's folder.
    pub module: Option<String>,
    /// Builds the module and gives the console its classpath (true by default in a Java or Kotlin
    /// project); false starts the console with the JDK or Kotlin standard library only.
    pub classpath: Option<bool>,
    /// Waits up to this long (at most 10 minutes) for the classpath to be made and the console to start.
    pub wait_ms: Option<u64>,
}

/// What opening a console answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    /// The Terminal session the console runs in, for `terminal.session.send` and `terminal.session.read`;
    /// absent while the classpath is still being made.
    pub session: Option<u64>,
    /// The module whose classpath it has, relative to the project; empty for the project's folder.
    pub module: String,
    /// Whether the module's classpath was given to it.
    pub classpath: bool,
    /// The program it runs, with its arguments.
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

impl Prepared {
    fn display(&self) -> String {
        let name = self.program.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let shown: Vec<String> = self
            .args
            .iter()
            .map(|arg| {
                if arg.chars().count() > 120 {
                    format!("{}…", arg.chars().take(120).collect::<String>())
                } else {
                    arg.clone()
                }
            })
            .collect();
        std::iter::once(name).chain(shown).collect::<Vec<_>>().join(" ")
    }
}

struct Pending {
    kind: ConsoleKind,
    params: OpenParams,
    folder: Option<PathBuf>,
    receiver: Receiver<Result<Prepared, String>>,
}

struct Running {
    kind: ConsoleKind,
    session: u64,
    params: OpenParams,
    folder: Option<PathBuf>,
}

#[derive(Default)]
pub struct Consoles {
    pending: Vec<Pending>,
    running: Vec<Running>,
    folders: Option<Vec<PathBuf>>,
}

impl Consoles {
    pub fn set_tool_folders(&mut self, folders: Option<Vec<PathBuf>>) {
        self.folders = folders;
    }

    pub fn session(&self, kind: ConsoleKind) -> Option<u64> {
        self.running.iter().rev().find(|running| running.kind == kind).map(|running| running.session)
    }

    pub fn preparing(&self, kind: ConsoleKind) -> bool {
        self.pending.iter().any(|pending| pending.kind == kind)
    }
}

pub fn unquote(text: &str) -> String {
    let text = text.trim();
    let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn console_args(kind: ConsoleKind, classpath: Option<&str>) -> Vec<String> {
    match classpath.filter(|classpath| !classpath.trim().is_empty()) {
        Some(classpath) => vec![kind.classpath_flag().to_owned(), classpath.to_owned()],
        None => Vec::new(),
    }
}

fn program(kind: ConsoleKind, context: &RunContext, java: &JavaRun) -> Result<(PathBuf, Option<PathBuf>), String> {
    match kind {
        ConsoleKind::Jshell => {
            let home = crate::java_run::jdk_home("", context)?;
            let jshell = home.join("bin").join(if cfg!(windows) { "jshell.exe" } else { "jshell" });
            if !jshell.is_file() {
                return Err(t!("workbench-console-no-jshell", path = home.display().to_string()));
            }
            Ok((jshell, Some(home)))
        }
        ConsoleKind::Kotlin => {
            let kotlinc = java.kotlinc().ok_or_else(|| t!("workbench-console-no-kotlinc"))?;
            let home = crate::java_run::jdk_home("", context).ok();
            Ok((kotlinc, home))
        }
    }
}

fn environment(context: &RunContext, home: Option<&Path>) -> Vec<(String, String)> {
    let mut env = context.env.env.clone();
    if let Some(home) = home {
        env.retain(|(key, _)| key != "JAVA_HOME");
        env.push(("JAVA_HOME".to_owned(), home.display().to_string()));
    }
    env
}

fn module_for(session: &Session, root: &Path, asked: Option<&str>) -> String {
    if let Some(module) = asked {
        return module.trim().trim_matches('/').to_owned();
    }
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .map(|document| document.path().to_owned())
        .filter(|path| path.starts_with(root))
        .map(|path| crate::java_run::module_of(root, &path))
        .unwrap_or_default()
}

fn prepare(
    lines: Vec<CommandLine>,
    classpath: PathBuf,
    base: Prepared,
    kind: ConsoleKind,
    task: Option<crate::tasks::TaskHandle>,
) -> Result<Prepared, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(task) = &task {
        task.cancel_with(Arc::clone(&cancel));
    }
    for line in &lines {
        crate::build_tools::capture(line, task.as_ref(), &cancel)?;
    }
    let text = std::fs::read_to_string(&classpath).map_err(|error| format!("{}: {error}", classpath.display()))?;
    let mut args = console_args(kind, Some(&unquote(&text)));
    args.extend(base.args);
    Ok(Prepared { args, ..base })
}

pub fn open(session: &mut Session, kind: ConsoleKind, params: OpenParams) -> Result<Opened, CommandError> {
    poll(session);
    let root = crate::run::project_root(session)?;
    let module = module_for(session, &root, params.module.as_deref());
    let mut context = crate::run::run_context(session, &root);
    let java = session
        .state::<Consoles>()
        .and_then(|consoles| consoles.folders.clone())
        .map_or_else(JavaRun::default, JavaRun::in_folders);
    let (program, home) = program(kind, &context, &java).map_err(CommandError::new)?;
    let cwd = if module.is_empty() { root.clone() } else { root.join(&module) };
    if !cwd.is_dir() {
        return Err(CommandError::new(t!("workbench-console-no-module", module = module.as_str())));
    }
    let base = Prepared { program, args: Vec::new(), cwd, env: environment(&context, home.as_deref()) };
    let classpath = params.classpath.unwrap_or_else(|| crate::jdk::is_jvm_project(&root));
    if !classpath {
        let (session_id, command) = start(session, kind, &base, params.clone(), None)?;
        return Ok(Opened { session: Some(session_id), module, classpath: false, command });
    }
    crate::trust::guard_project_classes(session, &root)?;
    let folder = crate::run::scratch_folder();
    context.scratch = folder.clone();
    let (_, lines, classpath_file) = java
        .project_classpath(&module, "", &context)
        .map_err(|error| CommandError::new(t!("workbench-console-cannot-start", name = kind.title(), error = error)))?;
    let task = session.state_mut::<crate::tasks::Tasks>().start(
        &t!("workbench-console-preparing", name = kind.title()),
        "java",
        false,
    );
    let (sender, receiver) = channel();
    let job_base = base.clone();
    let spawned = std::thread::Builder::new().name("zen-console-classpath".to_owned()).spawn(move || {
        let result = prepare(lines, classpath_file, job_base, kind, Some(task.clone()));
        let outcome = result.as_ref().map(|_| String::new()).map_err(Clone::clone);
        let _ = sender.send(result);
        task.finish(outcome);
    });
    if let Err(error) = spawned {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(CommandError::new(error.to_string()));
    }
    let command = base.display();
    let pending = Pending { kind, params: params.clone(), folder: Some(folder), receiver };
    let Some(wait) = params.wait_ms.map(|ms| Duration::from_millis(ms).min(MAX_WAIT)) else {
        session.state_mut::<Consoles>().pending.push(pending);
        return Ok(Opened { session: None, module, classpath: true, command });
    };
    match pending.receiver.recv_timeout(wait) {
        Ok(result) => {
            let (session_id, command) = finish(session, pending.kind, pending.params, pending.folder, result)?;
            Ok(Opened { session: Some(session_id), module, classpath: true, command })
        }
        Err(RecvTimeoutError::Timeout) => {
            session.state_mut::<Consoles>().pending.push(pending);
            Ok(Opened { session: None, module, classpath: true, command })
        }
        Err(RecvTimeoutError::Disconnected) => Err(CommandError::new(t!("workbench-console-failed"))),
    }
}

fn finish(
    session: &mut Session,
    kind: ConsoleKind,
    params: OpenParams,
    folder: Option<PathBuf>,
    result: Result<Prepared, String>,
) -> Result<(u64, String), CommandError> {
    match result {
        Ok(prepared) => start(session, kind, &prepared, params, folder),
        Err(error) => {
            if let Some(folder) = folder {
                let _ = std::fs::remove_dir_all(folder);
            }
            Err(CommandError::new(t!("workbench-console-cannot-start", name = kind.title(), error = error)))
        }
    }
}

fn start(
    session: &mut Session,
    kind: ConsoleKind,
    prepared: &Prepared,
    params: OpenParams,
    folder: Option<PathBuf>,
) -> Result<(u64, String), CommandError> {
    crate::terminal::restore_pending(session)?;
    let options = zen_terminal::Options {
        program: Some(prepared.program.clone()),
        args: prepared.args.clone(),
        env: prepared.env.clone(),
        ..zen_terminal::Options::shell(&prepared.cwd)
    };
    let started = session.state_mut::<Terminals>().run_in_new_tab(options, kind.title());
    let id = match started {
        Ok(id) => id,
        Err(error) => {
            if let Some(folder) = folder {
                let _ = std::fs::remove_dir_all(folder);
            }
            return Err(error);
        }
    };
    session.state_mut::<Consoles>().running.push(Running { kind, session: id, params, folder });
    Ok((id, prepared.display()))
}

pub fn poll(session: &mut Session) {
    if session.state::<Consoles>().is_none() {
        return;
    }
    let alive: Vec<u64> = session
        .state::<Terminals>()
        .map(|terminals| {
            terminals
                .tabs()
                .iter()
                .flat_map(|tab| tab.root().panes())
                .filter(|pane| !pane.session().has_exited())
                .map(crate::terminal::Pane::id)
                .collect()
        })
        .unwrap_or_default();
    let consoles = session.state_mut::<Consoles>();
    consoles.running.retain(|running| {
        let keep = alive.contains(&running.session);
        if !keep && let Some(folder) = &running.folder {
            let _ = std::fs::remove_dir_all(folder);
        }
        keep
    });
    let mut finished = Vec::new();
    let mut index = 0;
    while index < consoles.pending.len() {
        match consoles.pending[index].receiver.try_recv() {
            Ok(result) => finished.push((consoles.pending.remove(index), result)),
            Err(std::sync::mpsc::TryRecvError::Empty) => index += 1,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let pending = consoles.pending.remove(index);
                finished.push((pending, Err(t!("workbench-console-failed"))));
            }
        }
    }
    for (pending, result) in finished {
        if let Err(error) = finish(session, pending.kind, pending.params, pending.folder, result) {
            crate::notifications::post(session, NewNotification::message(Group::General, Kind::Error, error.message));
        }
    }
}

pub fn stop(session: &mut Session, kind: ConsoleKind) -> Result<Option<OpenParams>, CommandError> {
    poll(session);
    let consoles = session.state_mut::<Consoles>();
    let had_pending = consoles.pending.iter().any(|pending| pending.kind == kind);
    let mut params = None;
    consoles.pending.retain(|pending| {
        if pending.kind == kind {
            params = Some(pending.params.clone());
            if let Some(folder) = &pending.folder {
                let _ = std::fs::remove_dir_all(folder);
            }
        }
        pending.kind != kind
    });
    if had_pending {
        return Ok(params);
    }
    let Some(index) = consoles.running.iter().rposition(|running| running.kind == kind) else {
        return Err(CommandError::new(t!("workbench-console-not-running", name = kind.title())));
    };
    let running = consoles.running.remove(index);
    session.state_mut::<Terminals>().close(running.session);
    if let Some(folder) = &running.folder {
        let _ = std::fs::remove_dir_all(folder);
    }
    Ok(Some(running.params))
}

macro_rules! console_commands {
    ($open:ident, $restart:ident, $stop:ident, $kind:expr, $prefix:literal, $name:literal, $open_doc:literal) => {
        pub struct $open;

        impl Command for $open {
            type Params = OpenParams;
            type Output = Opened;
            const ID: &'static str = concat!($prefix, ".open");
            const TITLE: &'static str = $name;
            const DESCRIPTION: &'static str = $open_doc;

            fn run(&self, session: &mut Session, params: OpenParams) -> Result<Opened, CommandError> {
                open(session, $kind, params)
            }
        }

        pub struct $restart;

        impl Command for $restart {
            type Params = NoParams;
            type Output = Opened;
            const ID: &'static str = concat!($prefix, ".restart");
            const TITLE: &'static str = concat!("Restart ", $name);
            const DESCRIPTION: &'static str = concat!(
                "Stops the ",
                $name,
                " console and starts it again with the same module, building its classpath anew, so the console \
                 sees the latest compiled code; refused when none was opened."
            );

            fn run(&self, session: &mut Session, _: NoParams) -> Result<Opened, CommandError> {
                let params = stop(session, $kind)?.unwrap_or_default();
                open(session, $kind, OpenParams { wait_ms: None, ..params })
            }
        }

        pub struct $stop;

        impl Command for $stop {
            type Params = NoParams;
            type Output = NoParams;
            const ID: &'static str = concat!($prefix, ".stop");
            const TITLE: &'static str = concat!("Stop ", $name);
            const DESCRIPTION: &'static str = concat!(
                "Ends the newest ",
                $name,
                " console and closes its Terminal tab (or stops making its classpath); refused when none runs."
            );

            fn is_available(&self, session: &Session) -> bool {
                session
                    .state::<Consoles>()
                    .is_some_and(|consoles| consoles.session($kind).is_some() || consoles.preparing($kind))
            }

            fn run(&self, session: &mut Session, _: NoParams) -> Result<NoParams, CommandError> {
                stop(session, $kind)?;
                Ok(NoParams {})
            }
        }
    };
}

console_commands!(
    OpenJshell,
    RestartJshell,
    StopJshell,
    ConsoleKind::Jshell,
    "java.jshell",
    "JShell Console",
    "Opens JShell, the project JDK's interactive Java console, in a new tab of the Terminal tool window. By \
     default it gets the classpath of a module (the active editor file's, or `module`): the module is built first \
     the way an Application run builds it (Maven, Gradle or the IDE's javac), as a background task, and JShell \
     starts with `--class-path` once that is done. Type Java snippets at its prompt; ↑ and ↓ go through the input \
     history, which JShell keeps between sessions, and /exit ends it. Answers the Terminal `session` (absent while \
     the classpath is made; pass waitMs to wait) for terminal.session.send and terminal.session.read."
);

console_commands!(
    OpenKotlinRepl,
    RestartKotlinRepl,
    StopKotlinRepl,
    ConsoleKind::Kotlin,
    "kotlin.repl",
    "Kotlin REPL",
    "Opens the Kotlin REPL of `kotlinc` in a new tab of the Terminal tool window, with the classpath of a module \
     (the active editor file's, or `module`) made as for JShell. The IDE does not ship or download the Kotlin \
     compiler: `kotlinc` must be installed (KOTLIN_HOME, PATH, Homebrew or SDKMAN); refused otherwise with what is \
     missing. Answers the Terminal `session`, as java.jshell.open does."
);

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(OpenJshell)?;
    registry.register(RestartJshell)?;
    registry.register(StopJshell)?;
    registry.register(OpenKotlinRepl)?;
    registry.register(RestartKotlinRepl)?;
    registry.register(StopKotlinRepl)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_classpath_argument_file_is_read_back_as_one_value() {
        assert_eq!(unquote("\"/a b/x.jar:/c\\\\d:/e\\\"f\"\n"), "/a b/x.jar:/c\\d:/e\"f");
        assert_eq!(unquote("/plain:/path"), "/plain:/path");
        assert_eq!(console_args(ConsoleKind::Jshell, Some("/a:/b")), ["--class-path", "/a:/b"]);
        assert_eq!(console_args(ConsoleKind::Kotlin, Some("/a")), ["-cp", "/a"]);
        assert!(console_args(ConsoleKind::Jshell, Some("  ")).is_empty());
    }

    #[cfg(unix)]
    fn tab_names(session: &Session) -> Vec<String> {
        session
            .state::<Terminals>()
            .map(|terminals| terminals.tabs().iter().filter_map(|tab| tab.name().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    #[cfg(unix)]
    #[test]
    fn a_console_with_the_projects_classes_waits_for_trust_for_a_person_too() {
        use crate::java::testing::{executable, session};
        let jdk = tempfile::tempdir().unwrap();
        executable(&jdk.path().join("bin/jshell"), "#!/bin/sh\nexec cat\n");
        executable(&jdk.path().join("bin/java"), "#!/bin/sh\n");
        executable(&jdk.path().join("bin/javac"), "#!/bin/sh\n");
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("pom.xml"), "<project/>").unwrap();
        let (registry, mut session) = session(jdk.path(), project.path());
        session.set_state(crate::build_tools::BuildEnv::default());

        for params in [serde_json::json!({}), serde_json::json!({ "classpath": true })] {
            let refused = registry.dispatch(&mut session, "java.jshell.open", params).unwrap_err();
            assert!(format!("{refused:?}").contains(crate::trust::NOT_TRUSTED), "{refused:?}");
        }
        let tasks = session.state::<crate::tasks::Tasks>().map(crate::tasks::Tasks::list).unwrap_or_default();
        assert!(tasks.iter().all(|task| !task.title.contains("JShell")), "no build began: {tasks:?}");
        assert_eq!(tab_names(&session), Vec::<String>::new());

        let alone =
            registry.dispatch(&mut session, "java.jshell.open", serde_json::json!({ "classpath": false })).unwrap();
        assert_eq!(alone["classpath"], false, "the JDK alone loads nothing of the project: {alone}");
        let stopped = registry.dispatch(&mut session, "java.jshell.stop", serde_json::json!({}));
        assert!(stopped.is_ok(), "{stopped:?}");
    }

    #[cfg(unix)]
    #[test]
    fn jshell_opens_in_a_terminal_tab_with_the_modules_classpath_and_restarts_and_stops() {
        use crate::java::testing::{executable, session};
        let jdk = tempfile::tempdir().unwrap();
        executable(&jdk.path().join("bin/jshell"), "#!/bin/sh\nexec cat\n");
        executable(&jdk.path().join("bin/java"), "#!/bin/sh\n");
        executable(&jdk.path().join("bin/javac"), "#!/bin/sh\n");
        let project = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(jdk.path(), project.path());

        let refused = registry.dispatch(&mut session, "java.jshell.stop", serde_json::json!({}));
        assert!(refused.is_err(), "nothing to stop: {refused:?}");
        let opened = registry.dispatch(&mut session, "java.jshell.open", serde_json::json!({})).unwrap();
        assert_eq!(opened["classpath"], false, "not a Java project: {opened}");
        assert_eq!(opened["command"], "jshell");
        let first = opened["session"].as_u64().unwrap();
        assert_eq!(tab_names(&session), ["JShell"]);
        assert_eq!(session.state::<Consoles>().unwrap().session(ConsoleKind::Jshell), Some(first));

        std::fs::create_dir_all(project.path().join("src/shop")).unwrap();
        std::fs::write(project.path().join("src/shop/App.java"), "package shop;\nclass App {}\n").unwrap();
        let restarted = registry
            .dispatch(&mut session, "java.jshell.open", serde_json::json!({ "classpath": true, "waitMs": 60_000 }))
            .unwrap();
        let command = restarted["command"].as_str().unwrap();
        assert!(command.starts_with("jshell --class-path ") && command.ends_with("/classes"), "{restarted}");
        let second = restarted["session"].as_u64().unwrap();
        let folder = session.state::<Consoles>().unwrap().running.last().and_then(|running| running.folder.clone());
        let folder = folder.expect("the console's own folder");
        assert!(folder.join("classpath.args").is_file());

        let again = registry.dispatch(&mut session, "java.jshell.restart", serde_json::json!({})).unwrap();
        assert_eq!(again["classpath"], true, "the same module and classpath: {again}");
        assert!(again["session"].is_null(), "the classpath is made again in the background: {again}");
        assert!(!folder.exists(), "the old console's folder is removed");
        let terminals = session.state::<Terminals>().unwrap();
        assert!(terminals.pane(second).is_none(), "the old console is closed");
        assert!(terminals.pane(first).is_some(), "only the newest console restarts");

        registry.dispatch(&mut session, "java.jshell.stop", serde_json::json!({})).unwrap();
        assert!(!session.state::<Consoles>().unwrap().preparing(ConsoleKind::Jshell));
        registry.dispatch(&mut session, "java.jshell.stop", serde_json::json!({})).unwrap();
        assert_eq!(tab_names(&session), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn the_kotlin_repl_needs_kotlinc_and_never_downloads_it() {
        use crate::java::testing::{executable, session};
        let jdk = tempfile::tempdir().unwrap();
        executable(&jdk.path().join("bin/java"), "#!/bin/sh\n");
        let tools = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(jdk.path(), project.path());
        session.state_mut::<Consoles>().set_tool_folders(Some(vec![tools.path().to_owned()]));

        let refused = registry.dispatch(&mut session, "kotlin.repl.open", serde_json::json!({})).unwrap_err();
        let refused = refused.to_string();
        assert!(refused.contains("kotlinc") && refused.contains("does not ship"), "{refused}");
        assert_eq!(tab_names(&session), Vec::<String>::new());

        executable(&tools.path().join("kotlinc"), "#!/bin/sh\nexec cat\n");
        let opened = registry.dispatch(&mut session, "kotlin.repl.open", serde_json::json!({})).unwrap();
        assert_eq!(opened["command"], "kotlinc");
        assert_eq!(tab_names(&session), ["Kotlin REPL"]);
        registry.dispatch(&mut session, "kotlin.repl.stop", serde_json::json!({})).unwrap();
    }

    #[test]
    #[ignore = "runs the real javac and jshell of the JDK named by ZEN_JDK_HOME"]
    fn jshell_sees_the_projects_classes_with_a_real_jdk() {
        let jdk = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("my shop");
        std::fs::create_dir_all(root.join("src/shop")).unwrap();
        std::fs::write(
            root.join("src/shop/App.java"),
            "package shop;\npublic class App { public static String hi() { return \"hi from App\"; } }\n",
        )
        .unwrap();
        let mut context =
            RunContext { root: root.clone(), scratch: dir.path().join("console"), ..RunContext::default() };
        context.env.env = vec![("JAVA_HOME".to_owned(), jdk.display().to_string())];
        let (home, lines, classpath) = JavaRun::default().project_classpath("", "", &context).unwrap();
        let jshell = home.join("bin/jshell");
        let base =
            Prepared { program: jshell.clone(), args: vec!["-q".to_owned()], cwd: root.clone(), env: Vec::new() };
        let prepared = prepare(lines, classpath, base, ConsoleKind::Jshell, None).unwrap();
        assert_eq!(prepared.args[0], "--class-path");
        let mut child = std::process::Command::new(&prepared.program)
            .args(&prepared.args)
            .current_dir(&prepared.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"System.out.println(shop.App.hi())\n/exit\n").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("hi from App"), "{output:?}");
    }
}
