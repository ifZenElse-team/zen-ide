use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Form, Item, Tone};
use zen_project::inspect::{
    DeclaredSetting, Finding, FindingSeverity, FixJob, FixState, FixWrite, InspectContext, InspectionFix, Inspector,
    Wake,
};

use crate::toolchain::{Install, Toolchain, go};

pub const VET: &str = "go.inspections.vet";
pub const STATICCHECK: &str = "go.inspections.staticcheck";
pub const ON_SAVE: &str = "go.inspections.onSave";

const STATICCHECK_PACKAGE: &str = "honnef.co/go/tools/cmd/staticcheck@latest";
const TOOL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolEdit {
    start: usize,
    end: usize,
    new: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolFix {
    title: String,
    edits: Vec<ToolEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Issue {
    file: PathBuf,
    line: u32,
    column: u32,
    end: Option<(u32, u32)>,
    message: String,
    tool: &'static str,
    code: String,
    weak: bool,
    fixes: Vec<ToolFix>,
}

fn position(text: &str) -> Option<(PathBuf, u32, u32)> {
    let mut parts = text.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    Some((PathBuf::from(parts.next()?), line, column))
}

fn vet_fixes(found: &Value, file: &Path) -> Vec<ToolFix> {
    let fixes = found["suggested_fixes"].as_array().map(Vec::as_slice).unwrap_or_default();
    fixes
        .iter()
        .filter_map(|fix| {
            let edits = fix["edits"].as_array()?;
            let edits = edits
                .iter()
                .map(|edit| {
                    (Path::new(edit["filename"].as_str()?) == file).then_some(())?;
                    Some(ToolEdit {
                        start: edit["start"].as_u64()? as usize,
                        end: edit["end"].as_u64()? as usize,
                        new: edit["new"].as_str()?.to_owned(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ToolFix { title: fix["message"].as_str()?.to_owned(), edits })
        })
        .collect()
}

fn parse_vet(output: &[u8]) -> Vec<Issue> {
    let text = String::from_utf8_lossy(output);
    let json: String = text.lines().filter(|line| !line.starts_with('#')).collect::<Vec<_>>().join("\n");
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for packages in serde_json::Deserializer::from_str(&json).into_iter::<Value>().map_while(Result::ok) {
        let Some(packages) = packages.as_object() else { continue };
        for analyzers in packages.values().filter_map(Value::as_object) {
            for (analyzer, found) in analyzers {
                for found in found.as_array().map(Vec::as_slice).unwrap_or_default() {
                    let Some((file, line, column)) = found["posn"].as_str().and_then(position) else { continue };
                    let Some(message) = found["message"].as_str() else { continue };
                    if !seen.insert((file.clone(), line, column, message.to_owned())) {
                        continue;
                    }
                    let end = found["end"].as_str().and_then(position).filter(|end| end.0 == file);
                    issues.push(Issue {
                        fixes: vet_fixes(found, &file),
                        file,
                        line,
                        column,
                        end: end.map(|end| (end.1, end.2)),
                        message: message.to_owned(),
                        tool: "go vet",
                        code: analyzer.clone(),
                        weak: false,
                    });
                }
            }
        }
    }
    issues
}

fn parse_staticcheck(output: &[u8]) -> Vec<Issue> {
    let found = serde_json::Deserializer::from_slice(output).into_iter::<Value>().map_while(Result::ok);
    found
        .filter_map(|found| {
            let code = found["code"].as_str()?;
            if code == "compile" {
                return None;
            }
            let at = &found["location"];
            let end = &found["end"];
            let file = at["file"].as_str()?;
            let line = |at: &Value, key: &str| at[key].as_u64().map(|number| number as u32);
            Some(Issue {
                file: PathBuf::from(file),
                line: line(at, "line")?,
                column: line(at, "column")?,
                end: (end["file"].as_str() == Some(file))
                    .then(|| Some((line(end, "line")?, line(end, "column")?)))
                    .flatten(),
                message: found["message"].as_str()?.to_owned(),
                tool: "staticcheck",
                code: code.to_owned(),
                weak: ["ST", "S1", "QF"].iter().any(|prefix| code.starts_with(prefix)),
                fixes: Vec::new(),
            })
        })
        .collect()
}

fn line_range(text: &str, line: u32) -> Option<(usize, usize)> {
    let mut start = 0;
    for _ in 1..line {
        start += text[start..].find('\n')? + 1;
    }
    Some((start, text[start..].find('\n').map_or(text.len(), |at| start + at)))
}

fn offset(text: &str, line: u32, column: u32) -> Option<usize> {
    let (start, end) = line_range(text, line)?;
    let at = (start + column.saturating_sub(1) as usize).min(end);
    text.is_char_boundary(at).then_some(at)
}

fn word_end(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    let word = rest.find(|c: char| !(c == '_' || c.is_alphanumeric())).unwrap_or(rest.len());
    if word > 0 { at + word } else { at + rest.chars().next().map_or(0, char::len_utf8) }
}

fn hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn finding(issue: &Issue, text: &str, exact: Option<u64>) -> Option<Finding> {
    let start = offset(text, issue.line, issue.column)?;
    let end = issue.end.and_then(|(line, column)| offset(text, line, column)).filter(|end| *end > start);
    let fixes = exact.map_or_else(Vec::new, |vetted| {
        issue
            .fixes
            .iter()
            .enumerate()
            .map(|(index, fix)| {
                let edits: Vec<Value> = fix.edits.iter().map(|edit| json!([edit.start, edit.end, edit.new])).collect();
                InspectionFix {
                    id: format!("go.vet:{}:{}:{index}", issue.line, issue.column),
                    title: fix.title.clone(),
                    kind: Default::default(),
                    batch: None,
                    label: None,
                    inline: false,
                    args: json!({ "edits": edits, "vetted": vetted.to_string() }),
                }
            })
            .collect()
    });
    Some(Finding {
        start,
        end: end.unwrap_or_else(|| word_end(text, start)),
        severity: if issue.weak { FindingSeverity::WeakWarning } else { FindingSeverity::Warning },
        message: issue.message.clone(),
        note: None,
        rows: Vec::new(),
        details: None,
        fixes,
        source: issue.tool.to_owned(),
        code: issue.code.clone(),
        deprecated: false,
        unused: false,
    })
}

type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

fn go_files(dir: &Path) -> HashMap<PathBuf, (Stamp, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return HashMap::new() };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "go"))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path.clone(), (stamp(&path), text)))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tools {
    vet: bool,
    staticcheck: bool,
}

struct Finished {
    files: HashMap<PathBuf, (Stamp, String)>,
    issues: Vec<Issue>,
    tools: Tools,
    demand: u64,
}

#[derive(Default)]
struct State {
    finished: HashMap<PathBuf, Finished>,
    running: HashSet<PathBuf>,
    demand: u64,
    install: Option<Install>,
}

type Locate = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

struct Shared {
    locate: Locate,
    state: Mutex<State>,
    generation: AtomicU64,
    wake: Mutex<Option<Wake>>,
}

#[derive(Clone)]
pub struct GoVetInspector {
    shared: Arc<Shared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Run {
    tool: PathBuf,
    staticcheck: Option<PathBuf>,
    env: Vec<(String, String)>,
    dir: PathBuf,
    tools: Tools,
    demand: u64,
}

impl Run {
    fn process(&self, program: &Path, args: &[&str]) -> Process {
        let mut process = Process::new(program);
        process.args(args).current_dir(&self.dir).envs(self.env.iter().map(|(key, value)| (key, value)));
        process
            .env("GOTOOLCHAIN", "local")
            .env("GOFLAGS", crate::project_env::readonly_flags(&self.env))
            .env("GOPROXY", "off");
        process
    }

    fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        if self.tools.vet
            && let Ok(output) = zen_run::capture(&mut self.process(&self.tool, &["vet", "-json", "."]), TOOL_TIMEOUT)
        {
            issues.extend(parse_vet(&output.stdout));
            issues.extend(parse_vet(&output.stderr));
        }
        if self.tools.staticcheck
            && let Some(program) = &self.staticcheck
            && let Ok(output) = zen_run::capture(&mut self.process(program, &["-f", "json", "."]), TOOL_TIMEOUT)
        {
            issues.extend(parse_staticcheck(&output.stdout));
        }
        issues
    }
}

impl Shared {
    fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(wake) = lock(&self.wake).clone() {
            wake();
        }
    }

    fn run(self: &Arc<Self>, run: Run) {
        if !lock(&self.state).running.insert(run.dir.clone()) {
            return;
        }
        let shared = Arc::clone(self);
        let started = std::thread::Builder::new().name("go-vet".to_owned()).spawn(move || {
            let files = go_files(&run.dir);
            let issues = run.issues();
            let finished = Finished { files, issues, tools: run.tools, demand: run.demand };
            {
                let mut state = lock(&shared.state);
                state.running.remove(&run.dir);
                state.finished.insert(run.dir, finished);
            }
            shared.changed();
        });
        if started.is_err() {
            lock(&self.state).running.clear();
        }
    }
}

impl GoVetInspector {
    pub fn new(locate: Locate) -> Self {
        let state = Mutex::new(State::default());
        GoVetInspector {
            shared: Arc::new(Shared { locate, state, generation: AtomicU64::new(0), wake: Mutex::new(None) }),
        }
    }

    pub fn demand(&self) {
        lock(&self.shared.state).demand += 1;
        self.shared.changed();
    }

    pub fn running(&self) -> Vec<PathBuf> {
        let mut running: Vec<PathBuf> = lock(&self.shared.state).running.iter().cloned().collect();
        running.sort();
        running
    }

    pub fn staticcheck(&self) -> Option<PathBuf> {
        (self.shared.locate)()
    }

    fn install_state(&self) -> Install {
        lock(&self.shared.state).install.clone().unwrap_or(Install::Idle)
    }

    pub fn install(&self, sdk: go::GoSdk) {
        {
            let mut state = lock(&self.shared.state);
            if matches!(state.install, Some(Install::Running { .. })) {
                return;
            }
            state.install = Some(Install::Running { line: None });
        }
        let shared = Arc::clone(&self.shared);
        std::thread::spawn(move || {
            let outcome = crate::toolchain::run_install(&sdk, go::install_command(&sdk, STATICCHECK_PACKAGE), |line| {
                lock(&shared.state).install = Some(Install::Running { line: Some(line.to_owned()) });
            });
            lock(&shared.state).install = Some(match outcome {
                Ok(()) => Install::Done,
                Err(message) => Install::Failed { message },
            });
            lock(&shared.state).demand += 1;
            shared.changed();
        });
    }
}

impl Inspector for GoVetInspector {
    fn id(&self) -> &str {
        "go.vet"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    fn runs_programs(&self) -> bool {
        true
    }

    fn inputs(&self, path: &Path) -> Vec<PathBuf> {
        vec![path.to_owned()]
    }

    fn set_wake(&self, wake: Wake) {
        *lock(&self.shared.wake) = Some(wake);
    }

    fn refresh(&self) {
        self.demand();
    }

    fn settings(&self) -> Vec<DeclaredSetting> {
        let group = t!("go-inspections-group");
        let setting = |key: &str, title: String, description: String| DeclaredSetting {
            key: key.to_owned(),
            page: "go".to_owned(),
            group: group.clone(),
            title,
            description,
            default: true,
            network: Vec::new(),
        };
        vec![
            setting(VET, t!("go-inspections-vet"), t!("go-inspections-vet-description")),
            setting(STATICCHECK, t!("go-inspections-staticcheck"), t!("go-inspections-staticcheck-description")),
            setting(ON_SAVE, t!("go-inspections-on-save"), t!("go-inspections-on-save-description")),
        ]
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let enabled = |key: &str| (context.setting)(key).and_then(|value| value.as_bool()).unwrap_or(true);
        let tools = Tools { vet: enabled(VET), staticcheck: enabled(STATICCHECK) };
        if !(tools.vet || tools.staticcheck)
            || context.tool.as_os_str().is_empty()
            || !context.path.starts_with(context.root)
        {
            return Vec::new();
        }
        let dir = context.path.parent().map(Path::to_owned).unwrap_or_default();
        let now = stamp(context.path);
        let state = lock(&self.shared.state);
        let demand = state.demand;
        let kept = state.finished.get(&dir);
        let asked = kept.map_or(demand > 0, |kept| kept.demand < demand);
        let saved = kept.is_none_or(|kept| {
            kept.tools != tools || kept.files.get(context.path).is_none_or(|(vetted, _)| *vetted != now)
        });
        let findings = kept.map_or_else(Vec::new, |kept| findings_of(kept, context));
        drop(state);
        if asked || (saved && enabled(ON_SAVE)) {
            self.shared.run(Run {
                tool: context.tool.to_owned(),
                staticcheck: if tools.staticcheck { self.staticcheck() } else { None },
                env: context.env.to_vec(),
                dir,
                tools,
                demand,
            });
        }
        findings
    }

    fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        let stale = || t!("go-inspections-fix-stale");
        let fix = fixes.first().ok_or_else(stale)?;
        let vetted = std::fs::read_to_string(context.path).map_err(|error| error.to_string())?;
        let mut text = vetted.clone();
        if fix.args["vetted"].as_str() != Some(hash(&text).to_string().as_str()) {
            return Err(stale());
        }
        let mut edits: Vec<(usize, usize, &str)> = fix.args["edits"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|edit| Some((edit[0].as_u64()? as usize, edit[1].as_u64()? as usize, edit[2].as_str()?)))
            .collect();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
        for (start, end, new) in edits {
            if start > end || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                return Err(stale());
            }
            text.replace_range(start..end, new);
        }
        Ok(Box::new(Applied(vec![FixWrite { path: context.path.to_owned(), text, read: Some(vetted) }])))
    }
}

fn findings_of(kept: &Finished, context: &InspectContext<'_>) -> Vec<Finding> {
    let Some((_, vetted)) = kept.files.get(context.path) else { return Vec::new() };
    let same = vetted == context.text;
    let same_line = |line: u32| {
        let text_of = |text: &'_ str| line_range(text, line).map(|(start, end)| text[start..end].to_owned());
        text_of(vetted) == text_of(context.text)
    };
    kept.issues
        .iter()
        .filter(|issue| issue.file == context.path && (same || same_line(issue.line)))
        .filter_map(|issue| finding(issue, context.text, same.then(|| hash(vetted))))
        .collect()
}

struct Applied(Vec<FixWrite>);

impl FixJob for Applied {
    fn poll(&mut self) -> FixState {
        FixState::Done { changed: self.0.drain(..).map(|write| write.path).collect() }
    }

    fn writes(&self) -> Vec<FixWrite> {
        self.0.clone()
    }
}

/// What the Go inspections are doing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VetStatus {
    /// The staticcheck program in use, if one is installed.
    pub staticcheck: Option<PathBuf>,
    /// The package folders being checked now; empty when the results of problems.file.list are final.
    pub running: Vec<PathBuf>,
    /// What installing staticcheck is doing.
    pub install: Install,
}

fn status(session: &Session) -> Result<VetStatus, CommandError> {
    let inspector = session.state::<GoVetInspector>().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))?;
    Ok(VetStatus {
        staticcheck: inspector.staticcheck(),
        running: inspector.running(),
        install: inspector.install_state(),
    })
}

pub struct RunInspections;

impl Command for RunInspections {
    type Params = NoParams;
    type Output = VetStatus;
    const ID: &'static str = "go.inspections.run";
    const TITLE: &'static str = "Run go vet and staticcheck";
    const DESCRIPTION: &'static str = "Checks the packages of the open Go files with go vet and staticcheck now, \
         even with go.inspections.onSave off, in the background and without the network. Read the answers with \
         problems.file.list (source `go vet` or `staticcheck`), and their quick-fixes with problems.fixes.list; \
         go.inspections.status says when it is over.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<VetStatus, CommandError> {
        if let Some(inspector) = session.state::<GoVetInspector>() {
            inspector.demand();
        }
        status(session)
    }
}

pub struct InspectionsStatus;

impl Command for InspectionsStatus {
    type Params = NoParams;
    type Output = VetStatus;
    const ID: &'static str = "go.inspections.status";
    const TITLE: &'static str = "Go Inspections Status";
    const DESCRIPTION: &'static str = "Which staticcheck the Go inspections use, the package folders being checked \
         now and what installing staticcheck is doing. The settings are go.inspections.vet, \
         go.inspections.staticcheck and go.inspections.onSave (settings.declared.list).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<VetStatus, CommandError> {
        status(session)
    }
}

pub struct InstallStaticcheck;

impl Command for InstallStaticcheck {
    type Params = NoParams;
    type Output = VetStatus;
    const ID: &'static str = "go.staticcheck.install";
    const TITLE: &'static str = "Install staticcheck";
    const DESCRIPTION: &'static str = "Downloads and installs staticcheck with `go install \
         honnef.co/go/tools/cmd/staticcheck@latest` in the background, only when called; go.inspections.status \
         reports the progress as install.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<VetStatus, CommandError> {
        crate::toolchain::person_only(session)?;
        let toolchain = session.state::<Toolchain>().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))?;
        let sdk = toolchain.go().sdk.ok_or_else(|| CommandError::new(t!("toolchain-error-no-sdk-install")))?;
        if let Some(inspector) = session.state::<GoVetInspector>() {
            inspector.install(sdk);
        }
        status(session)
    }
}

pub fn add_settings(session: &Session, form: &mut Form) {
    let Some(inspector) = session.state::<GoVetInspector>() else { return };
    let install = inspector.install_state();
    let found = inspector.staticcheck();
    let items = &mut form.items;
    items.push(Item::Group { title: t!("settings-go-staticcheck") });
    if let Install::Running { line } = &install {
        let detail = line.clone().unwrap_or_default();
        items.push(Item::Text {
            text: t!("settings-go-staticcheck-installing", detail = detail),
            tone: Tone::Secondary,
            spinner: true,
        });
        form.busy = true;
        return;
    }
    let (text, tone) = match &found {
        Some(path) => (path.display().to_string(), Tone::Normal),
        None => (t!("settings-go-staticcheck-missing"), Tone::Secondary),
    };
    items.push(Item::Text { text, tone, spinner: false });
    if let Install::Failed { message } = &install {
        items.push(Item::Text {
            text: t!("settings-go-staticcheck-failed", detail = message.as_str()),
            tone: Tone::Error,
            spinner: false,
        });
    }
    let label = if found.is_some() { "settings-go-staticcheck-update" } else { "settings-go-staticcheck-install" };
    items.push(Item::Row {
        items: vec![Item::Button { label: t!(label), enabled: true, action: Action::command(InstallStaticcheck::ID) }],
    });
    items.push(Item::Text { text: t!("settings-go-staticcheck-note"), tone: Tone::Secondary, spinner: false });
}

#[cfg(test)]
mod tests;
