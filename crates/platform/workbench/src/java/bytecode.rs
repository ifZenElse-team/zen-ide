use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::CommandLine;

use crate::Workbench;
use crate::java_run::{BuildTool, build_tool, class_at, module_of};
use crate::notifications::{Action, Group, Kind, NewNotification};
use crate::virtual_documents::{self, VirtualDocument, VirtualKind};

pub const JAVAP_FLAGS: [&str; 3] = ["-c", "-p", "-v"];

pub const MAX_TEXT: usize = 256 * 1024;

const MAIN_FOLDERS: [&str; 3] = ["target/classes", "out/production/classes", "bin/main"];

const TEST_FOLDERS: [&str; 3] = ["target/test-classes", "out/test/classes", "bin/test"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Compiled(PathBuf),
    CompileAlone { source_root: Option<PathBuf> },
    Build { tool: BuildTool, module: String, test: bool },
}

pub fn class_relative(binary: &str) -> PathBuf {
    PathBuf::from(format!("{}.class", binary.replace('.', "/")))
}

pub fn output_folders(module: &Path, test: bool) -> Vec<PathBuf> {
    let set = if test { "test" } else { "main" };
    let mut folders: Vec<PathBuf> =
        (if test { TEST_FOLDERS } else { MAIN_FOLDERS }).iter().map(|folder| module.join(folder)).collect();
    let mut gradle: Vec<PathBuf> = std::fs::read_dir(module.join("build/classes"))
        .map(|entries| entries.flatten().map(|entry| entry.path().join(set)).filter(|path| path.is_dir()).collect())
        .unwrap_or_default();
    gradle.sort_by_key(|path| !path.parent().is_some_and(|language| language.ends_with("java")));
    folders.splice(1..1, gradle);
    folders
}

fn package_of(text: &str) -> String {
    let Some(start) = text.find("package ") else { return String::new() };
    let before = &text[..start];
    if before.lines().any(|line| {
        let line = line.trim();
        !line.is_empty()
            && !line.starts_with("//")
            && !line.starts_with('*')
            && !line.starts_with("/*")
            && !line.starts_with('@')
    }) {
        return String::new();
    }
    text[start + "package ".len()..].split(';').next().unwrap_or_default().split_whitespace().collect()
}

pub fn source_root(file: &Path, text: &str) -> Option<PathBuf> {
    let package = package_of(text);
    let mut folder = file.parent()?.to_owned();
    for part in package.split('.').filter(|part| !part.is_empty()).rev() {
        if folder.file_name()?.to_str()? != part {
            return None;
        }
        folder = folder.parent()?.to_owned();
    }
    Some(folder)
}

pub fn locate(root: Option<&Path>, file: &Path, text: &str, binary: &str, alone: bool) -> Location {
    locate_in(root, file, text, binary, alone, &[])
}

pub fn locate_in(
    root: Option<&Path>,
    file: &Path,
    text: &str,
    binary: &str,
    alone: bool,
    modules: &[zen_project::model::Module],
) -> Location {
    let test = file
        .components()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].as_os_str() == "src" && pair[1].as_os_str() == "test");
    let inside = root.filter(|root| !alone && file.starts_with(root));
    let Some(root) = inside else { return Location::CompileAlone { source_root: None } };
    let module = module_of(root, file);
    let folder = if module.is_empty() { root.to_owned() } else { root.join(&module) };
    let relative = class_relative(binary);
    let declared = modules.iter().filter(|declared| declared.root == folder);
    let declared: Vec<PathBuf> = declared
        .flat_map(|declared| {
            let outputs = [declared.test_output.clone().filter(|_| test), declared.output.clone()];
            outputs.into_iter().flatten()
        })
        .map(|output| if output.is_absolute() { output } else { folder.join(output) })
        .collect();
    let found = declared
        .into_iter()
        .chain(output_folders(&folder, test))
        .chain(if test { output_folders(&folder, false) } else { Vec::new() })
        .map(|output| output.join(&relative))
        .find(|candidate| candidate.is_file());
    if let Some(found) = found {
        return Location::Compiled(found);
    }
    match build_tool(&folder) {
        BuildTool::Javac => Location::CompileAlone { source_root: source_root(file, text) },
        tool => Location::Build { tool, module, test },
    }
}

pub fn build_action(root: &Path, tool: BuildTool, module: &str, test: bool) -> Option<(&'static str, Value)> {
    match tool {
        BuildTool::Maven => {
            let pom = if module.is_empty() { root.join("pom.xml") } else { root.join(module).join("pom.xml") };
            Some(("maven.goal.run", json!({ "goals": if test { "test-compile" } else { "compile" }, "pom": pom })))
        }
        BuildTool::Gradle => {
            let task = if test { "testClasses" } else { "classes" };
            let tasks =
                if module.is_empty() { task.to_owned() } else { format!(":{}:{task}", module.replace('/', ":")) };
            Some(("gradle.task.run", json!({ "tasks": tasks })))
        }
        BuildTool::Javac => None,
    }
}

pub fn javac_line(home: &Path, file: &Path, source_root: Option<&Path>, output: &Path) -> CommandLine {
    let mut args = vec![
        "-d".to_owned(),
        output.display().to_string(),
        "-encoding".to_owned(),
        "UTF-8".to_owned(),
        "-g".to_owned(),
        "-proc:none".to_owned(),
        "-implicit:none".to_owned(),
    ];
    if let Some(source_root) = source_root {
        args.extend(["-sourcepath".to_owned(), source_root.display().to_string()]);
    }
    args.push(file.display().to_string());
    tool_line(home, "javac", args, file.parent().unwrap_or(Path::new(".")))
}

pub fn javap_line(home: &Path, class_file: &Path) -> CommandLine {
    let mut args: Vec<String> = JAVAP_FLAGS.iter().map(|flag| (*flag).to_owned()).collect();
    args.push(class_file.display().to_string());
    tool_line(home, "javap", args, class_file.parent().unwrap_or(Path::new(".")))
}

fn tool_line(home: &Path, name: &str, args: Vec<String>, cwd: &Path) -> CommandLine {
    let program = home.join("bin").join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() });
    let display = std::iter::once(name.to_owned()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
    let env = vec![("JAVA_HOME".to_owned(), home.display().to_string())];
    CommandLine { program, args, cwd: cwd.to_owned(), env, display }
}

fn disassemble(compile: Option<(CommandLine, PathBuf)>, class_file: PathBuf, home: PathBuf) -> Result<String, String> {
    let cancel = AtomicBool::new(false);
    let folder = compile.as_ref().map(|(_, folder)| folder.clone());
    let result = (|| {
        if let Some((line, _)) = &compile {
            crate::build_tools::capture(line, None, &cancel)?;
        }
        crate::build_tools::capture(&javap_line(&home, &class_file), None, &cancel)
    })();
    if let Some(folder) = folder {
        let _ = std::fs::remove_dir_all(folder);
    }
    result
}

struct Job {
    source: PathBuf,
    class: String,
    receiver: Receiver<Result<String, String>>,
}

#[derive(Default)]
pub struct Bytecode {
    job: Option<Job>,
    view: Option<PathBuf>,
    shown: Option<(PathBuf, String)>,
    probe: Option<(PathBuf, u64, u64)>,
}

impl Bytecode {
    pub fn view(&self) -> Option<&Path> {
        self.view.as_deref()
    }

    pub fn shown_class(&self) -> Option<&str> {
        self.shown.as_ref().map(|(_, class)| class.as_str())
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// The parameters of `java.bytecode.show`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowParams {
    /// The Java source file, absolute or relative to the project; the active editor's file by default.
    pub path: Option<PathBuf>,
    /// The one-based line whose class is shown; the caret's line in the active editor by default.
    pub line: Option<u32>,
    /// Waits up to this long (at most 1 minute) for javap and answers its output in `text`.
    pub wait_ms: Option<u64>,
}

/// A command that makes a class's bytecode available.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildOffer {
    /// The command to dispatch.
    pub command: String,
    /// Its parameters.
    pub params: Value,
}

/// What `java.bytecode.show` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    /// The binary name of the class at the place, such as `shop.App$Item`.
    pub class: String,
    /// Whether its compiled class was found (or compiled on the spot for a file outside any build).
    pub compiled: bool,
    /// The `.class` file javap reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_file: Option<PathBuf>,
    /// The read-only view that shows the bytecode, once it is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<PathBuf>,
    /// javap's output, with waitMs (cut at 256 KiB).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What to run to compile the class when it is not compiled yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildOffer>,
}

fn is_kotlin(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("kt")
}

fn is_jvm_source(path: &Path) -> bool {
    is_kotlin(path) || path.extension().and_then(|extension| extension.to_str()) == Some("java")
}

fn class_in(session: &Session, path: &Path, text: &str, line: u32) -> Option<String> {
    if !is_kotlin(path) {
        return class_at(path, text, line);
    }
    let env = session.state::<crate::build_tools::BuildEnv>().cloned().unwrap_or_default();
    let file = env.jvm_file(path, text)?.named_after(&path.file_stem()?.to_string_lossy());
    let innermost = file
        .classes
        .iter()
        .filter(|class| class.first_line <= line && line <= class.last_line)
        .min_by_key(|class| class.last_line - class.first_line)
        .map(|class| class.name.clone());
    innermost.or_else(|| (!file.file_class.is_empty()).then(|| file.file_class.clone()))
}

fn caret_line(session: &Session, path: &Path, text: &str) -> u32 {
    let head = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.selections().primary().head);
    let Some(head) = head else { return 0 };
    let count = text.chars().take(head).filter(|c| *c == '\n').count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn view_path(root: Option<&Path>) -> PathBuf {
    let key = virtual_documents::hashed(&root.map(|root| root.display().to_string()).unwrap_or_default());
    virtual_documents::folder().join("bytecode").join(key).join("Bytecode.txt")
}

fn project(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn is_scratch(session: &Session, path: &Path) -> bool {
    session.state::<crate::scratch::Scratches>().is_some_and(|scratches| scratches.contains(path))
}

enum Started {
    Running,
    Build(String, Option<(&'static str, Value)>),
}

fn start(
    session: &mut Session,
    source: &Path,
    text: &str,
    class: &str,
) -> Result<(Started, Option<PathBuf>), CommandError> {
    let root = project(session);
    let alone = is_scratch(session, source);
    let context = match &root {
        Some(root) => crate::run::run_context(session, root),
        None => zen_run::RunContext::default(),
    };
    let location = locate_in(root.as_deref(), source, text, class, alone, &context.modules);
    let home = crate::java_run::jdk_home("", &context).map_err(CommandError::new)?;
    if !home.join("bin").join(if cfg!(windows) { "javap.exe" } else { "javap" }).is_file() {
        return Err(CommandError::new(t!("workbench-bytecode-no-javap", path = home.display().to_string())));
    }
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class).to_owned();
    let (compile, class_file) = match location {
        Location::Compiled(class_file) => (None, class_file),
        Location::CompileAlone { .. } if is_kotlin(source) => {
            return Err(CommandError::new(t!("workbench-bytecode-kotlin-alone", path = crate::display(source))));
        }
        Location::CompileAlone { source_root } => {
            let folder = crate::run::scratch_folder();
            zen_run::create_private_folder(&folder).map_err(|error| CommandError::new(error.to_string()))?;
            let line = javac_line(&home, source, source_root.as_deref(), &folder);
            let class_file = folder.join(class_relative(class));
            (Some((line, folder)), class_file)
        }
        Location::Build { tool, module, test } => {
            let action = root.as_deref().and_then(|root| build_action(root, tool, &module, test));
            return Ok((Started::Build(simple, action), None));
        }
    };
    if let Some(patterns) = crate::hidden_files::for_caller(session)
        && crate::hidden_files::is_hidden(&patterns, root.as_deref(), &class_file)
    {
        return Err(crate::hidden_files::refused(&crate::display(&class_file)));
    }
    let (sender, receiver) = channel();
    let job_class_file = class_file.clone();
    std::thread::Builder::new()
        .name("zen-javap".to_owned())
        .spawn(move || {
            let _ = sender.send(disassemble(compile, job_class_file, home));
        })
        .map_err(|error| CommandError::new(error.to_string()))?;
    let state = session.state_mut::<Bytecode>();
    state.job = Some(Job { source: source.to_owned(), class: class.to_owned(), receiver });
    Ok((Started::Running, Some(class_file)))
}

fn apply(session: &mut Session, source: PathBuf, class: String, result: Result<String, String>) -> Option<PathBuf> {
    let text = match result {
        Ok(text) => text,
        Err(error) => {
            let body = t!("workbench-bytecode-failed", class = class.as_str(), error = error);
            crate::notifications::post(session, NewNotification::message(Group::General, Kind::Error, body));
            return None;
        }
    };
    let root = project(session);
    let target = view_path(root.as_deref());
    let folder = virtual_documents::folder();
    let relative = target.strip_prefix(&folder).map(Path::to_owned).unwrap_or_else(|_| target.clone());
    let document = VirtualDocument {
        kind: VirtualKind::Bytecode,
        origin: class.clone(),
        library: None,
        bytecode: None,
        class_path: None,
    };
    let path = match virtual_documents::store(&folder, &relative, &text, document, None) {
        Ok(path) => path,
        Err(error) => {
            let body = t!("workbench-bytecode-failed", class = class.as_str(), error = error.to_string());
            crate::notifications::post(session, NewNotification::message(Group::General, Kind::Error, body));
            return None;
        }
    };
    let workbench = session.state_mut::<Workbench>();
    let open = workbench.documents.iter_mut().find(|document| document.path == path);
    match open {
        Some(document) => {
            document.reload_from_disk();
        }
        None => {
            let focused = workbench.groups.focused();
            let opened = crate::groups::OpenInRightSplit.run(session, crate::PathParams { path: path.clone() });
            if opened.is_ok() {
                let _ = crate::groups::FocusGroup.run(session, crate::groups::GroupParams { group: focused });
            }
        }
    }
    let state = session.state_mut::<Bytecode>();
    state.view = Some(path.clone());
    state.shown = Some((source, class));
    Some(path)
}

pub fn poll(session: &mut Session) {
    if session.state::<Bytecode>().is_none() {
        return;
    }
    let state = session.state_mut::<Bytecode>();
    let Some(job) = &state.job else {
        follow(session);
        return;
    };
    match job.receiver.try_recv() {
        Ok(result) => {
            if let Some(job) = state.job.take() {
                apply(session, job.source, job.class, result);
            }
        }
        Err(TryRecvError::Disconnected) => state.job = None,
        Err(TryRecvError::Empty) => {}
    }
}

fn follow(session: &mut Session) {
    let Some(view) = session.state::<Bytecode>().and_then(|state| state.view.clone()) else { return };
    let Some(workbench) = session.state::<Workbench>() else { return };
    if !workbench.documents().iter().any(|document| document.path() == view) {
        let state = session.state_mut::<Bytecode>();
        state.view = None;
        state.shown = None;
        state.probe = None;
        return;
    }
    let Some(active) = workbench.active_document() else { return };
    let path = active.path().to_owned();
    if path == view || !is_jvm_source(&path) {
        return;
    }
    let probe = (path.clone(), active.version(), active.selection_version());
    let text = active.text().to_owned();
    let head = active.selections().primary().head;
    let state = session.state_mut::<Bytecode>();
    if state.probe.as_ref() == Some(&probe) {
        return;
    }
    state.probe = Some(probe);
    let line = u32::try_from(text.chars().take(head).filter(|c| *c == '\n').count()).unwrap_or(u32::MAX);
    let Some(class) = class_in(session, &path, &text, line) else { return };
    let state = session.state_mut::<Bytecode>();
    if state.shown.as_ref().is_some_and(|(source, shown)| *source == path && *shown == class) {
        return;
    }
    let root = project(session);
    let alone = is_scratch(session, &path);
    if matches!(locate(root.as_deref(), &path, &text, &class, alone), Location::Build { .. }) {
        return;
    }
    let _ = start(session, &path, &text, &class);
}

pub struct ShowBytecode;

impl Command for ShowBytecode {
    type Params = ShowParams;
    type Output = Shown;
    const ID: &'static str = "java.bytecode.show";
    const TITLE: &'static str = "Show Bytecode";
    const DESCRIPTION: &'static str = "Shows the bytecode of the Java or Kotlin class at a place (the active editor's \
         caret by default; for Kotlin code outside a class, the file's class such as `CartKt`): finds its compiled .class in the module's build output (Maven target/classes or \
         target/test-classes, Gradle build/classes/<language>/main or test) and runs the project JDK's javap -c -p -v \
         on it in the background; the result opens in a read-only view in the editor's right split, which then \
         follows the caret to the class it is in. A file outside any build (a scratch file, a project without Maven or \
         Gradle) is compiled alone with javac first; a Kotlin file has to be built by Maven or Gradle. A class of a Maven or Gradle module that is not compiled yet \
         answers compiled false with `build`, the command that compiles it (the window offers it as Build in a \
         notification). With waitMs it waits for javap and answers its output in `text`.";

    fn run(&self, session: &mut Session, params: ShowParams) -> Result<Shown, CommandError> {
        poll(session);
        let path = match params.path {
            Some(path) => session.state::<Workbench>().map_or(path.clone(), |workbench| workbench.resolve(&path)),
            None => session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-bytecode-no-file")))?,
        };
        if !is_jvm_source(&path) {
            return Err(CommandError::new(t!("workbench-bytecode-not-java", path = crate::display(&path))));
        }
        let (text, _) = crate::run::file_text(session, &path)?;
        let line = params.line.map_or_else(|| caret_line(session, &path, &text), |line| line.saturating_sub(1));
        let class = class_in(session, &path, &text, line)
            .ok_or_else(|| CommandError::new(t!("workbench-bytecode-no-class", path = crate::display(&path))))?;
        let (started, class_file) = start(session, &path, &text, &class)?;
        let mut shown =
            Shown { class: class.clone(), compiled: true, class_file, document: None, text: None, build: None };
        if let Started::Build(simple, action) = started {
            shown.compiled = false;
            let mut notification = NewNotification::worded(
                Group::General,
                Kind::Warning,
                zen_i18n::Message::new("workbench-bytecode-not-compiled").with("class", simple.as_str()),
            );
            if let Some((command, params)) = action {
                notification =
                    notification.action(Action::new("build", t!("workbench-bytecode-build"), command, params.clone()));
                shown.build = Some(BuildOffer { command: command.to_owned(), params });
            }
            crate::notifications::post(session, notification);
            return Ok(shown);
        }
        let Some(wait) = params.wait_ms.map(|ms| Duration::from_millis(ms.min(60_000))) else { return Ok(shown) };
        let Some(job) = session.state_mut::<Bytecode>().job.take() else { return Ok(shown) };
        match job.receiver.recv_timeout(wait) {
            Ok(result) => {
                let failed = result.as_ref().err().cloned();
                let text = result.as_ref().ok().map(|text| cut(text, MAX_TEXT));
                shown.document = apply(session, job.source, job.class, result);
                if let Some(error) = failed {
                    return Err(CommandError::new(t!(
                        "workbench-bytecode-failed",
                        class = class.as_str(),
                        error = error
                    )));
                }
                shown.text = text;
                Ok(shown)
            }
            Err(RecvTimeoutError::Timeout) => {
                session.state_mut::<Bytecode>().job = Some(job);
                Ok(shown)
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(CommandError::new(t!("workbench-bytecode-failed", class = class.as_str(), error = String::new())))
            }
        }
    }
}

fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowBytecode)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use zen_core::{Registry, Session};

    use super::*;

    const APP: &str = "package shop;\n\npublic class App {\n    void run() {}\n\n    static class Item {\n        int price;\n    }\n}\n";

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn a_class_is_found_in_the_build_output_of_its_module() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(root.join("api")).unwrap();
        std::fs::write(root.join("api/build.gradle.kts"), "plugins { java }").unwrap();
        let app = root.join("src/main/java/shop/App.java");
        let api = root.join("api/src/test/java/shop/ApiTest.java");

        assert_eq!(
            locate(Some(root), &app, APP, "shop.App$Item", false),
            Location::Build { tool: BuildTool::Maven, module: String::new(), test: false }
        );
        assert_eq!(
            build_action(root, BuildTool::Maven, "", false),
            Some(("maven.goal.run", json!({ "goals": "compile", "pom": root.join("pom.xml") })))
        );
        touch(&root.join("target/classes/shop/App$Item.class"));
        assert_eq!(
            locate(Some(root), &app, APP, "shop.App$Item", false),
            Location::Compiled(root.join("target/classes/shop/App$Item.class"))
        );

        assert_eq!(
            locate(Some(root), &api, "package shop;", "shop.ApiTest", false),
            Location::Build { tool: BuildTool::Gradle, module: "api".to_owned(), test: true }
        );
        assert_eq!(
            build_action(root, BuildTool::Gradle, "api", true),
            Some(("gradle.task.run", json!({ "tasks": ":api:testClasses" })))
        );
        touch(&root.join("api/build/classes/kotlin/test/shop/ApiTest.class"));
        touch(&root.join("api/build/classes/java/test/shop/ApiTest.class"));
        assert_eq!(
            locate(Some(root), &api, "package shop;", "shop.ApiTest", false),
            Location::Compiled(root.join("api/build/classes/java/test/shop/ApiTest.class")),
            "Java's output first"
        );

        let plain = tempfile::tempdir().unwrap();
        let file = plain.path().join("src/shop/Tool.java");
        assert_eq!(
            locate(Some(plain.path()), &file, "package shop;\nclass Tool {}", "shop.Tool", false),
            Location::CompileAlone { source_root: Some(plain.path().join("src")) },
            "no build tool: javac compiles the file alone against its source root"
        );
        assert_eq!(
            locate(Some(root), &app, APP, "shop.App", true),
            Location::CompileAlone { source_root: None },
            "a scratch file is compiled alone"
        );
        assert_eq!(source_root(&file, "/* c */\npackage other;\n"), None, "a file outside its package's folders");

        touch(&root.join("custom/out/shop/App.class"));
        let declared = zen_project::model::Module {
            root: root.to_owned(),
            output: Some(PathBuf::from("custom/out")),
            ..zen_project::model::Module::default()
        };
        assert_eq!(
            locate_in(Some(root), &app, APP, "shop.App", false, &[declared]),
            Location::Compiled(root.join("custom/out/shop/App.class")),
            "the output folder the build declares comes first"
        );
    }

    #[test]
    fn javap_prints_code_private_members_and_the_constant_pool() {
        let home = Path::new("/jdk");
        let line = javap_line(home, Path::new("/p/target/classes/shop/App.class"));
        assert_eq!(line.program, home.join("bin/javap"));
        assert_eq!(line.args, ["-c", "-p", "-v", "/p/target/classes/shop/App.class"]);
        let javac = javac_line(home, Path::new("/s/Tool.java"), Some(Path::new("/s")), Path::new("/tmp/out"));
        assert_eq!(javac.program, home.join("bin/javac"));
        assert!(javac.args.windows(2).any(|pair| pair == ["-sourcepath", "/s"]), "{:?}", javac.args);
        assert_eq!(javac.args.last().map(String::as_str), Some("/s/Tool.java"));
    }

    #[cfg(unix)]
    fn setup(jdk: &Path) -> (Registry, Session, tempfile::TempDir) {
        crate::java::testing::executable(&jdk.join("bin/javap"), "#!/bin/sh\necho \"javap $*\"\n");
        let dir = tempfile::tempdir().unwrap();
        let (registry, session) = crate::java::testing::session(jdk, dir.path());
        (registry, session, dir)
    }

    #[cfg(unix)]
    fn wait(session: &mut Session) {
        let job = session.state_mut::<Bytecode>().job.take().expect("javap runs");
        let result = job.receiver.recv().unwrap();
        apply(session, job.source, job.class, result);
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_sees_no_bytecode_of_a_hidden_source_or_a_hidden_class() {
        let jdk_dir = tempfile::tempdir().unwrap();
        let (registry, mut session, dir) = setup(jdk_dir.path());
        let root = dir.path();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        let app = root.join("src/main/java/shop/App.java");
        std::fs::create_dir_all(app.parent().unwrap()).unwrap();
        std::fs::write(&app, APP).unwrap();
        touch(&root.join("target/classes/shop/App.class"));
        let params = json!({ "path": app, "line": 1 });
        for hidden in ["target/**", "src/**"] {
            session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[hidden.to_owned()]);
            let refused = zen_core::macros::as_agent(&mut session, |session| {
                registry.dispatch(session, "java.bytecode.show", params.clone())
            })
            .unwrap_err();
            assert!(format!("{refused:?}").contains(crate::hidden_files::REDACTED_CODE), "{hidden}: {refused:?}");
            assert!(session.state::<Bytecode>().is_none_or(|bytecode| bytecode.job.is_none()), "{hidden}: javap ran");
        }
        registry.dispatch(&mut session, "java.bytecode.show", params).expect("a person sees it");
    }

    #[cfg(unix)]
    #[test]
    fn show_bytecode_opens_a_read_only_view_beside_the_editor_that_follows_the_caret() {
        let jdk_dir = tempfile::tempdir().unwrap();
        let (registry, mut session, dir) = setup(jdk_dir.path());
        let root = dir.path();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        let app = root.join("src/main/java/shop/App.java");
        std::fs::create_dir_all(app.parent().unwrap()).unwrap();
        std::fs::write(&app, APP).unwrap();
        touch(&root.join("target/classes/shop/App$Item.class"));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": app })).unwrap();

        let shown = registry.dispatch(&mut session, "java.bytecode.show", json!({})).unwrap();
        assert_eq!(shown["class"], "shop.App");
        assert_eq!(shown["compiled"], false, "{shown}");
        assert_eq!(shown["build"]["command"], "maven.goal.run");
        let notifications = registry.dispatch(&mut session, "ui.notifications.list", json!({})).unwrap();
        assert!(notifications.to_string().contains("App is not compiled yet"), "{notifications}");
        assert!(notifications.to_string().contains("maven.goal.run"), "Build is offered: {notifications}");

        let shown =
            registry.dispatch(&mut session, "java.bytecode.show", json!({ "line": 7, "waitMs": 20_000 })).unwrap();
        assert_eq!(shown["class"], "shop.App$Item");
        let class_file = root.join("target/classes/shop/App$Item.class");
        assert_eq!(shown["classFile"], json!(class_file));
        assert_eq!(shown["text"], format!("javap -c -p -v {}\n", class_file.display()));
        let view = PathBuf::from(shown["document"].as_str().unwrap());
        assert!(virtual_documents::reason(&view).is_some(), "the view is read-only");
        let workbench = session.state::<Workbench>().unwrap();
        assert!(workbench.documents().iter().any(|document| document.path() == view));
        assert_eq!(workbench.active_document().map(|document| document.path().to_owned()), Some(app.clone()));
        assert_eq!(workbench.groups().groups().len(), 2, "the view opens in a split beside the editor");

        touch(&root.join("target/classes/shop/App.class"));
        registry.dispatch(&mut session, "editor.caret.goTo", json!({ "line": 4, "column": 1 })).unwrap();
        poll(&mut session);
        assert!(session.state::<Bytecode>().unwrap().busy(), "moving into another class shows that class");
        wait(&mut session);
        assert_eq!(session.state::<Bytecode>().unwrap().shown_class(), Some("shop.App"));
        let text = std::fs::read_to_string(&view).unwrap();
        assert!(text.contains("shop/App.class"), "{text}");
        let open = session.state::<Workbench>().unwrap().documents().iter().find(|document| document.path() == view);
        assert_eq!(open.map(|document| document.text().to_owned()), Some(text), "the open view is reloaded");
        poll(&mut session);
        assert!(!session.state::<Bytecode>().unwrap().busy(), "the same class is not shown again");

        let refused = registry.dispatch(&mut session, "java.bytecode.show", json!({ "path": root.join("pom.xml") }));
        assert!(refused.is_err(), "{refused:?}");
    }

    const KOTLIN: &str = "package shop\n\nclass Cart {\n    fun total() = 1\n\n    class Line {\n        val price = 2\n    }\n}\n\nfun main() {}\n";

    #[cfg(unix)]
    fn kotlin_classes(session: &mut Session) {
        let env = crate::build_tools::BuildEnv::default();
        env.set_jvm_reader(Some(std::sync::Arc::new(|path: &Path, _: &str| {
            let class =
                |name: &str, first_line, last_line| zen_syntax::JvmClass { name: name.into(), first_line, last_line };
            let classes = vec![class("shop.Cart", 2, 8), class("shop.Cart$Line", 5, 7)];
            let file = zen_syntax::JvmFile { package: "shop".into(), classes, ..Default::default() };
            Some(file.named_after(&path.file_stem()?.to_string_lossy()))
        })));
        session.set_state(env);
    }

    #[cfg(unix)]
    #[test]
    fn kotlin_classes_show_their_bytecode_from_the_kotlin_build_output() {
        let jdk_dir = tempfile::tempdir().unwrap();
        let (registry, mut session, dir) = setup(jdk_dir.path());
        kotlin_classes(&mut session);
        let root = dir.path();
        std::fs::write(root.join("build.gradle.kts"), "plugins { kotlin(\"jvm\") }").unwrap();
        let cart = root.join("src/main/kotlin/shop/Cart.kt");
        std::fs::create_dir_all(cart.parent().unwrap()).unwrap();
        std::fs::write(&cart, KOTLIN).unwrap();
        let output = root.join("build/classes/kotlin/main/shop");
        for class in ["Cart", "Cart$Line", "CartKt"] {
            touch(&output.join(format!("{class}.class")));
        }
        let show = |session: &mut Session, line: u32| {
            registry
                .dispatch(session, "java.bytecode.show", json!({ "path": cart, "line": line, "waitMs": 20_000 }))
                .unwrap()
        };
        let outer = show(&mut session, 4);
        assert_eq!(outer["class"], "shop.Cart", "{outer}");
        assert_eq!(outer["classFile"], json!(output.join("Cart.class")));
        assert!(outer["text"].as_str().unwrap().contains("Cart.class"), "{outer}");
        assert_eq!(show(&mut session, 7)["class"], "shop.Cart$Line", "the innermost class");
        assert_eq!(show(&mut session, 11)["class"], "shop.CartKt", "top-level functions are in the file's class");

        std::fs::remove_file(output.join("CartKt.class")).unwrap();
        let unbuilt = show(&mut session, 11);
        assert_eq!(unbuilt["compiled"], false);
        assert_eq!(unbuilt["build"]["params"]["tasks"], "classes", "Gradle compiles Kotlin with its classes task");

        let scratch = tempfile::tempdir().unwrap();
        let alone = scratch.path().join("Tool.kt");
        std::fs::write(&alone, "fun tool() {}\n").unwrap();
        let refused = registry.dispatch(&mut session, "java.bytecode.show", json!({ "path": alone, "line": 1 }));
        assert!(format!("{refused:?}").contains("Kotlin file outside Maven and Gradle"), "{refused:?}");
    }

    #[test]
    #[ignore = "runs the real javac and javap of the JDK named by ZEN_JDK_HOME"]
    fn a_file_outside_any_build_is_compiled_alone_and_disassembled_with_a_real_jdk() {
        let jdk = crate::build_tools::real_tool_in_tests("ZEN_JDK_HOME");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("src/shop/App.java");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(dir.path().join("src/shop/Price.java"), "package shop;\nclass Price { int cents; }\n").unwrap();
        let text = "package shop;\n\nclass App {\n    static class Item { Price price = new Price(); }\n}\n";
        std::fs::write(&file, text).unwrap();
        let Location::CompileAlone { source_root } = locate(Some(dir.path()), &file, text, "shop.App$Item", false)
        else {
            panic!("no build tool")
        };
        let output = dir.path().join("out");
        let line = javac_line(&jdk, &file, source_root.as_deref(), &output);
        let printed =
            disassemble(Some((line, output.clone())), output.join(class_relative("shop.App$Item")), jdk).unwrap();
        assert!(printed.contains("class shop.App$Item"), "{printed}");
        assert!(printed.contains("Constant pool:") && printed.contains("invokespecial"), "{printed}");
        assert!(!output.exists(), "the compiled classes are removed");
    }
}
