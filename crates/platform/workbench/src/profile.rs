use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{CallNode, Configuration, Profile, ProfileKind, ProfileMetric, RunProvider, RunState, provider_for};

use crate::run::{
    Launch, MAX_RUN_WAIT, Recording, RunSummary, Runs, StartParams, configuration_to_start, project_root, run_context,
};
use crate::{Workbench, display};

const DEFAULT_FUNCTIONS: usize = 50;
const DEFAULT_DEPTH: usize = 12;
const MAX_DEPTH: usize = 64;
const DEFAULT_LEAST_PERCENT: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct Recorded {
    pub token: u64,
    pub run_id: Option<u64>,
    pub name: String,
    pub profile: Arc<Profile>,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    revision: u64,
    next: u64,
    shown: Option<Recorded>,
    loading: Option<u64>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub struct Profiles {
    shared: Arc<Shared>,
}

impl Profiles {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    pub fn shown(&self) -> Option<Recorded> {
        self.lock().shown.clone()
    }

    fn begin(&self) -> u64 {
        let mut state = self.lock();
        state.next += 1;
        let token = state.next;
        state.loading = Some(token);
        state.revision += 1;
        token
    }

    fn finish(&self, token: u64, recorded: Option<Recorded>) {
        let mut state = self.lock();
        if state.loading == Some(token) {
            state.loading = None;
        }
        if let Some(recorded) = recorded
            && state.shown.as_ref().is_none_or(|shown| shown.token <= token)
        {
            state.shown = Some(recorded);
        }
        state.revision += 1;
        drop(state);
        self.shared.changed.notify_all();
    }

    fn wait_loaded(&self, token: u64, timeout: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, timeout, |state| state.loading == Some(token))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.loading != Some(token)
    }
}

fn report_file() -> PathBuf {
    crate::run::scratch_folder().join("profile.pb.gz")
}

fn unsupported(configuration: &Configuration) -> CommandError {
    CommandError::new(t!("workbench-profile-unsupported", kind = configuration.kind.as_str()))
}

fn provider_of(session: &Session, configuration: &Configuration) -> Option<Arc<dyn RunProvider>> {
    let runs = session.state::<Runs>()?;
    provider_for(runs.providers(), &configuration.kind).cloned()
}

fn console_line(recorded: &Recorded) -> String {
    if let Some(error) = &recorded.error {
        return t!("workbench-profile-console-error", error = error.as_str());
    }
    let profile = &recorded.profile;
    let hottest = profile.functions(profile.default_metric).first().map(|cost| cost.frame);
    match hottest.and_then(|frame| profile.frames.get(frame as usize)) {
        Some(frame) => {
            t!("workbench-profile-console", samples = profile.samples.len(), function = frame.function.as_str())
        }
        None => t!("workbench-profile-console-empty"),
    }
}

struct Collection {
    profiles: Profiles,
    provider: Arc<dyn RunProvider>,
    configuration: Configuration,
    context: zen_run::RunContext,
    output: zen_run::SharedOutput,
    run_id: u64,
    token: u64,
    report: PathBuf,
}

fn collect(collection: Collection) {
    let Collection { profiles, provider, configuration, context, output, run_id, token, report } = collection;
    while !output.wait_end(MAX_RUN_WAIT) {}
    let stopped = output.read(|output| output.state() == RunState::Stopped);
    let recorded = (!stopped).then(|| {
        let read = provider.profiler(&configuration).map_or_else(
            || Err(unsupported(&configuration).message),
            |reader| reader.read(&configuration, &context, &report),
        );
        let (profile, error) = match read {
            Ok(profile) => (profile, None),
            Err(error) => (Profile::default(), Some(error)),
        };
        Recorded { token, run_id: Some(run_id), name: configuration.name.clone(), profile: Arc::new(profile), error }
    });
    if let Some(folder) = report.parent() {
        let _ = std::fs::remove_dir_all(folder);
    }
    let line = recorded.as_ref().map(console_line);
    profiles.finish(token, recorded);
    if let Some(line) = line {
        output.update(|output| {
            output.system(&line);
        });
    }
}

pub(crate) fn start(
    session: &mut Session,
    configuration: Configuration,
    kind: ProfileKind,
    launch: Launch,
) -> Result<(u64, u64), CommandError> {
    let root = project_root(session)?;
    let provider = provider_of(session, &configuration).ok_or_else(|| unsupported(&configuration))?;
    if provider.profiler(&configuration).is_none() || crate::compound::is_compound(&configuration) {
        return Err(unsupported(&configuration));
    }
    let context = run_context(session, &root);
    let report = report_file();
    if let Some(folder) = report.parent() {
        zen_run::create_private_folder(folder).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let launch = Launch { recording: Some(Recording::Profile(kind, report.clone())), ..launch };
    let run_id = match crate::run::start_with(session, configuration.clone(), launch, &[]) {
        Ok(started) => started.run_id,
        Err(error) => {
            if let Some(folder) = report.parent() {
                let _ = std::fs::remove_dir_all(folder);
            }
            return Err(error);
        }
    };
    let output = session
        .state::<Runs>()
        .and_then(|runs| runs.run(run_id))
        .map(|run| run.output.clone())
        .ok_or_else(|| CommandError::new(t!("workbench-profile-no-run")))?;
    let profiles = session.state_mut::<Profiles>().clone();
    let token = profiles.begin();
    let collection = Collection { profiles, provider, configuration, context, output, run_id, token, report };
    std::thread::Builder::new()
        .name("zen-profile".to_owned())
        .spawn(move || collect(collection))
        .map_err(|error| CommandError::new(error.to_string()))?;
    Ok((run_id, token))
}

/// A recorded profile in short.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    /// The run that recorded it; absent for a file that was opened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// The configuration's name, or the file's.
    pub name: String,
    /// What it measures.
    pub metrics: Vec<ProfileMetric>,
    /// The metric used when none is asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_metric: Option<String>,
    /// How many stacks it holds.
    pub samples: usize,
    /// How many functions appear in it.
    pub frames: usize,
    /// How long the program was recorded, in milliseconds.
    pub duration_ms: u64,
    /// Why the profile could not be read, if it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ProfileSummary {
    fn of(recorded: &Recorded) -> Self {
        let profile = &recorded.profile;
        Self {
            run_id: recorded.run_id,
            name: recorded.name.clone(),
            metrics: profile.metrics.clone(),
            default_metric: profile.metrics.get(profile.default_metric).map(|metric| metric.name.clone()),
            samples: profile.samples.len(),
            frames: profile.frames.len(),
            duration_ms: profile.duration_ns / 1_000_000,
            error: recorded.error.clone(),
        }
    }
}

/// The parameters of `profile.run.start`: those of `run.process.start` and what to record.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileStartParams {
    #[serde(flatten)]
    pub start: StartParams,
    /// What to record: `cpu` (the default) or `memory`.
    pub kind: ProfileKind,
}

/// What `profile.run.start` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStarted {
    /// The run's id, for `run.output.get` and `run.tests.get`.
    pub run_id: u64,
    /// With `waitMs`: where the run is and its test counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RunSummary>,
    /// With `waitMs`, once the run ended and its profile was read: the profile in short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileSummary>,
}

pub struct StartProfile;

impl Command for StartProfile {
    type Params = ProfileStartParams;
    type Output = ProfileStarted;
    const ID: &'static str = "profile.run.start";
    const TITLE: &'static str = "Run with Profiler";
    const DESCRIPTION: &'static str = "Saves modified files and runs a configuration while recording a cpu or memory \
         profile (the parameters of run.process.start and kind). Go tests and benchmarks use go test -cpuprofile or \
         -memprofile. With waitMs it answers once the run ended and its profile was read; then profile.report.get \
         lists the functions, profile.tree.get gives the call tree and profile.frame.open goes to a function's code.";

    fn run(&self, session: &mut Session, params: ProfileStartParams) -> Result<ProfileStarted, CommandError> {
        let ProfileStartParams { start: params, kind } = params;
        let wait = params.wait_ms.map(Duration::from_millis);
        let launch = Launch {
            prompts: crate::run::Prompts::of(&params),
            confirm_rerun: params.confirm_rerun,
            ..Launch::default()
        };
        let configuration = configuration_to_start(session, params)?;
        let (run_id, token) = start(session, configuration, kind, launch)?;
        let Some(wait) = wait else { return Ok(ProfileStarted { run_id, summary: None, profile: None }) };
        let started = Instant::now();
        let summary = crate::run::wait_for_run(session, run_id, wait)?;
        let profiles = session.state_mut::<Profiles>().clone();
        let left = wait.min(MAX_RUN_WAIT).saturating_sub(started.elapsed());
        let loaded = summary.done && profiles.wait_loaded(token, left);
        let recorded = profiles.shown().filter(|recorded| loaded && recorded.token == token);
        Ok(ProfileStarted { run_id, summary: Some(summary), profile: recorded.as_ref().map(ProfileSummary::of) })
    }
}

/// The parameters of `profile.file.open`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// The pprof file, gzipped or not.
    pub path: PathBuf,
    /// Waits up to this long for the file to be read and answers the profile in short.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `profile.file.open` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenAnswer {
    /// With `waitMs`, once the file was read: the profile in short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileSummary>,
}

fn read_file(path: &Path, root: &Path) -> Recorded {
    let (profile, error) = match zen_run::pprof::read(path, |file| root.join(file)) {
        Ok(profile) => (profile, None),
        Err(error) => (Profile::default(), Some(error)),
    };
    let name = path.file_name().map_or_else(|| display(path), |name| name.to_string_lossy().into_owned());
    Recorded { token: 0, run_id: None, name, profile: Arc::new(profile), error }
}

pub struct OpenFile;

impl Command for OpenFile {
    type Params = OpenParams;
    type Output = OpenAnswer;
    const ID: &'static str = "profile.file.open";
    const TITLE: &'static str = "Open Pprof Profile";
    const DESCRIPTION: &'static str = "Reads a pprof file, gzipped or not, such as one a program wrote with \
         runtime/pprof, on a thread of its own and makes it the profile shown; with waitMs it answers once the file \
         was read. It replaces the profile shown before.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<OpenAnswer, CommandError> {
        let root = project_root(session)?;
        let path = session
            .state::<Workbench>()
            .map_or_else(|| params.path.clone(), |workbench| workbench.resolve(&params.path));
        if zen_core::macros::by_agent(session) && !crate::inside_project(&root, &path) {
            let message = t!("workbench-profile-agent-outside", path = display(&path));
            return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, message));
        }
        crate::hidden_files::guard_read(session, std::slice::from_ref(&path))?;
        let profiles = session.state_mut::<Profiles>().clone();
        let token = profiles.begin();
        let spawned = profiles.clone();
        std::thread::Builder::new()
            .name("zen-profile".to_owned())
            .spawn(move || {
                let recorded = Recorded { token, ..read_file(&path, &root) };
                spawned.finish(token, Some(recorded));
            })
            .map_err(|error| CommandError::new(error.to_string()))?;
        let Some(wait) = params.wait_ms else { return Ok(OpenAnswer { profile: None }) };
        let loaded = profiles.wait_loaded(token, Duration::from_millis(wait).min(MAX_RUN_WAIT));
        let recorded = profiles.shown().filter(|recorded| loaded && recorded.token == token);
        Ok(OpenAnswer { profile: recorded.as_ref().map(ProfileSummary::of) })
    }
}

fn shown(session: &mut Session) -> Result<Recorded, CommandError> {
    session.state_mut::<Profiles>().shown().ok_or_else(|| CommandError::new(t!("workbench-profile-none")))
}

fn metric_of(profile: &Profile, metric: Option<&str>) -> Result<usize, CommandError> {
    match metric {
        Some(name) => profile
            .metric_index(name)
            .ok_or_else(|| CommandError::new(t!("workbench-profile-unknown-metric", metric = name))),
        None => Ok(profile.default_metric),
    }
}

fn percent(value: i64, total: i64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (value as f64 * 1000.0 / total as f64).round() / 10.0
}

/// The parameters of `profile.report.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportParams {
    /// The metric to rank by; the profile's default when absent.
    pub metric: Option<String>,
    /// How many functions to list; 50 when absent.
    pub limit: Option<usize>,
}

/// A function with what it cost.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionEntry {
    /// The frame, for `profile.frame.open`.
    pub frame: u32,
    /// The function's full name.
    pub function: String,
    /// Its source file, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// The line it starts at, from 1; 0 when unknown.
    pub line: u32,
    /// What the function itself cost.
    pub flat: i64,
    /// `flat` as a share of the total, in percent.
    pub flat_percent: f64,
    /// What it cost with everything it called.
    pub cumulative: i64,
    /// `cumulative` as a share of the total, in percent.
    pub cumulative_percent: f64,
}

/// What `profile.report.get` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportAnswer {
    /// Whether a profile is shown.
    pub shown: bool,
    /// Whether a profile is being read.
    pub loading: bool,
    /// Grows whenever what is shown changes.
    pub revision: u64,
    /// The profile in short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ProfileSummary>,
    /// The metric the costs are in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<ProfileMetric>,
    /// The whole profile's total in that metric.
    pub total: i64,
    /// The costliest functions by their own cost.
    pub functions: Vec<FunctionEntry>,
    /// How many more functions there are.
    pub omitted: usize,
}

pub struct GetReport;

impl Command for GetReport {
    type Params = ReportParams;
    type Output = ReportAnswer;
    const ID: &'static str = "profile.report.get";
    const TITLE: &'static str = "Profile Report";
    const DESCRIPTION: &'static str = "Answers the profile shown: a summary and its costliest functions, each with \
         its own cost, the cost with what it called, its file and line, in a metric of the profile.";

    fn run(&self, session: &mut Session, params: ReportParams) -> Result<ReportAnswer, CommandError> {
        let profiles = session.state_mut::<Profiles>().clone();
        let (revision, loading, recorded) = {
            let state = profiles.lock();
            (state.revision, state.loading.is_some(), state.shown.clone())
        };
        let Some(recorded) = recorded else {
            return Ok(ReportAnswer {
                shown: false,
                loading,
                revision,
                summary: None,
                metric: None,
                total: 0,
                functions: Vec::new(),
                omitted: 0,
            });
        };
        let profile = &recorded.profile;
        let summary = Some(ProfileSummary::of(&recorded));
        if profile.metrics.is_empty() {
            let functions = Vec::new();
            return Ok(ReportAnswer {
                shown: true,
                loading,
                revision,
                summary,
                metric: None,
                total: 0,
                functions,
                omitted: 0,
            });
        }
        let metric = metric_of(profile, params.metric.as_deref())?;
        let total = profile.total(metric);
        let costs = profile.functions(metric);
        let limit = params.limit.unwrap_or(DEFAULT_FUNCTIONS);
        let omitted = costs.len().saturating_sub(limit);
        let functions = costs
            .into_iter()
            .take(limit)
            .map(|cost| {
                let frame = &profile.frames[cost.frame as usize];
                FunctionEntry {
                    frame: cost.frame,
                    function: frame.function.clone(),
                    file: frame.file.clone(),
                    line: frame.line,
                    flat: cost.flat,
                    flat_percent: percent(cost.flat, total),
                    cumulative: cost.cumulative,
                    cumulative_percent: percent(cost.cumulative, total),
                }
            })
            .collect();
        Ok(ReportAnswer {
            shown: true,
            loading,
            revision,
            summary,
            metric: profile.metrics.get(metric).cloned(),
            total,
            functions,
            omitted,
        })
    }
}

/// The parameters of `profile.tree.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeParams {
    /// The metric of the costs; the profile's default when absent.
    pub metric: Option<String>,
    /// How many calls deep to go; 12 when absent, at most 64.
    pub depth: Option<usize>,
    /// Leaves out calls that cost less than this share of the total, in percent; 1 when absent.
    pub min_percent: Option<f64>,
}

/// A call in the call tree.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// The frame, for `profile.frame.open`; absent at the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    /// The function's full name; absent at the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// What the call cost with what it called.
    pub value: i64,
    /// `value` as a share of the total, in percent.
    pub percent: f64,
    /// The calls it made, the costliest first.
    pub children: Vec<TreeEntry>,
    /// How many calls the limits left out.
    pub omitted: usize,
}

fn entry(profile: &Profile, node: CallNode, total: i64) -> TreeEntry {
    TreeEntry {
        frame: node.frame,
        function: node.frame.map(|frame| profile.frames[frame as usize].function.clone()),
        value: node.value,
        percent: percent(node.value, total),
        omitted: node.omitted,
        children: node.children.into_iter().map(|child| entry(profile, child, total)).collect(),
    }
}

/// What `profile.tree.get` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeAnswer {
    /// Grows whenever what is shown changes.
    pub revision: u64,
    /// The metric of the costs.
    pub metric: ProfileMetric,
    /// The whole profile's total in that metric.
    pub total: i64,
    /// The call tree, from an unnamed root; a flame graph draws the same tree.
    pub root: TreeEntry,
}

pub struct GetTree;

impl Command for GetTree {
    type Params = TreeParams;
    type Output = TreeAnswer;
    const ID: &'static str = "profile.tree.get";
    const TITLE: &'static str = "Profile Call Tree";
    const DESCRIPTION: &'static str = "Answers the call tree of the profile shown, callers above what they called, \
         each call with its cost with what it called; a flame graph is the same tree. depth and minPercent bound the \
         answer, and each call says how many calls they left out.";

    fn run(&self, session: &mut Session, params: TreeParams) -> Result<TreeAnswer, CommandError> {
        let recorded = shown(session)?;
        let revision = session.state_mut::<Profiles>().revision();
        let profile = &recorded.profile;
        let index = metric_of(profile, params.metric.as_deref())?;
        let metric =
            profile.metrics.get(index).cloned().ok_or_else(|| CommandError::new(t!("workbench-profile-none")))?;
        let total = profile.total(index);
        let share = params.min_percent.unwrap_or(DEFAULT_LEAST_PERCENT).clamp(0.0, 100.0);
        let least = (total as f64 * share / 100.0).ceil() as i64;
        let depth = params.depth.unwrap_or(DEFAULT_DEPTH).min(MAX_DEPTH);
        let root = entry(profile, profile.call_tree(index, depth, least), total);
        Ok(TreeAnswer { revision, metric, total, root })
    }
}

/// The parameters of `profile.frame.open`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FrameParams {
    /// The frame, from `profile.report.get` or `profile.tree.get`.
    pub frame: u32,
}

/// What `profile.frame.open` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FrameAnswer {
    /// The file that was opened.
    pub path: PathBuf,
    /// The line the caret went to, from 1.
    pub line: u32,
}

pub struct OpenFrame;

impl Command for OpenFrame {
    type Params = FrameParams;
    type Output = FrameAnswer;
    const ID: &'static str = "profile.frame.open";
    const TITLE: &'static str = "Go to Profiled Function";
    const DESCRIPTION: &'static str = "Opens the source of a function of the profile shown at the line it starts at. \
         Refused when the profile names no file for it or the file does not exist here.";

    fn run(&self, session: &mut Session, params: FrameParams) -> Result<FrameAnswer, CommandError> {
        let recorded = shown(session)?;
        let frame = recorded
            .profile
            .frames
            .get(params.frame as usize)
            .ok_or_else(|| CommandError::new(t!("workbench-profile-unknown-frame", frame = params.frame)))?;
        let no_source = || CommandError::new(t!("workbench-profile-no-source", function = frame.function.as_str()));
        let path = frame.file.clone().filter(|path| path.is_file()).ok_or_else(no_source)?;
        crate::hidden_files::guard_read(session, std::slice::from_ref(&path))?;
        let line = frame.line.max(1);
        crate::history::go_to_line(session, &path, line - 1)?;
        Ok(FrameAnswer { path, line })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(StartProfile)?;
    registry.register(OpenFile)?;
    registry.register(GetReport)?;
    registry.register(GetTree)?;
    registry.register(OpenFrame)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_run::fake::{FakeProvider, ShellProvider, sample_pprof};
    use zen_run::{RunEnv, Stream};

    use super::*;

    const WAIT: u64 = 30_000;

    fn setup() -> (Registry, Session, tempfile::TempDir, FakeProvider) {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let fake = FakeProvider::new();
        session
            .state_mut::<Runs>()
            .set_providers(vec![Arc::new(fake.clone()), Arc::new(ShellProvider)], RunEnv::default());
        (registry, session, dir, fake)
    }

    fn configuration(name: &str) -> Value {
        serde_json::to_value(FakeProvider::configuration(name)).unwrap()
    }

    fn record(registry: &Registry, session: &mut Session) -> Value {
        registry
            .dispatch(session, "profile.run.start", json!({ "configuration": configuration("T"), "waitMs": WAIT }))
            .unwrap()
    }

    #[test]
    fn profile_commands_are_registered_with_three_segments() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        for id in
            ["profile.run.start", "profile.file.open", "profile.report.get", "profile.tree.get", "profile.frame.open"]
        {
            assert!(registry.commands().any(|info| info.id.as_str() == id), "{id}");
            assert_eq!(id.split('.').count(), 3, "{id}");
        }
    }

    #[test]
    fn a_run_with_the_profiler_shows_its_functions_and_tells_its_console() {
        let (registry, mut session, _dir, fake) = setup();
        fake.set_script(&[(Stream::Stdout, "ok")], 0);
        fake.set_profile(Some(sample_pprof()));
        let started = record(&registry, &mut session);
        let summary = &started["profile"];
        assert_eq!(summary["samples"], 2);
        assert_eq!(summary["defaultMetric"], "cpu");
        let run_id = started["runId"].as_u64().unwrap();
        let report = registry.dispatch(&mut session, "profile.report.get", json!({})).unwrap();
        assert_eq!(report["shown"], true);
        assert_eq!(report["total"], 4_000);
        assert_eq!(report["functions"][0]["function"], "main.work");
        assert_eq!(report["functions"][0]["flatPercent"], 75.0);
        let output = registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id })).unwrap();
        assert!(output.to_string().contains("main.work"), "{output}");
    }

    #[test]
    fn a_rerun_of_a_run_with_the_profiler_records_again() {
        let (registry, mut session, _dir, fake) = setup();
        fake.set_script(&[(Stream::Stdout, "ok")], 0);
        fake.set_profile(Some(sample_pprof()));
        let first = record(&registry, &mut session)["runId"].as_u64().unwrap();
        let before = session.state_mut::<Profiles>().shown().unwrap().token;
        let again = registry.dispatch(&mut session, "run.process.rerun", json!({ "runId": first })).unwrap();
        let run_id = again["runId"].as_u64().unwrap();
        crate::run::wait_for_run(&mut session, run_id, Duration::from_millis(WAIT)).unwrap();
        let profiles = session.state_mut::<Profiles>().clone();
        profiles.wait_loaded(before + 1, Duration::from_millis(WAIT));
        assert_eq!(profiles.shown().unwrap().run_id, Some(run_id));
    }

    #[test]
    fn the_call_tree_is_bounded_and_says_what_it_left_out() {
        let (registry, mut session, _dir, fake) = setup();
        fake.set_script(&[(Stream::Stdout, "ok")], 0);
        fake.set_profile(Some(sample_pprof()));
        record(&registry, &mut session);
        let tree = registry.dispatch(&mut session, "profile.tree.get", json!({})).unwrap();
        assert_eq!(tree["metric"]["name"], "cpu");
        let main = &tree["root"]["children"][0];
        assert_eq!(main["function"], "main.main");
        assert_eq!(main["children"][0]["function"], "main.work");
        let params = json!({ "depth": 1, "minPercent": 0 });
        let shallow = registry.dispatch(&mut session, "profile.tree.get", params).unwrap();
        assert_eq!(shallow["root"]["children"][0]["omitted"], 1);
        let unknown = registry.dispatch(&mut session, "profile.tree.get", json!({ "metric": "nope" }));
        assert!(unknown.is_err());
    }

    #[test]
    fn an_unsupported_configuration_is_refused_and_records_nothing() {
        let (registry, mut session, _dir, _fake) = setup();
        let refused =
            registry.dispatch(&mut session, "profile.run.start", json!({ "configuration": configuration("T") }));
        assert!(refused.is_err());
        assert!(session.state_mut::<Profiles>().shown().is_none());
    }

    #[test]
    fn a_pprof_file_opens_and_its_frame_goes_to_the_function_in_the_code() {
        let (registry, mut session, dir, _fake) = setup();
        let main = dir.path().join("main.go");
        std::fs::write(&main, "package main\n\nfunc a() {}\n\nfunc work() {}\n").unwrap();
        let file = dir.path().join("cpu.pprof");
        std::fs::write(&file, sample_pprof()).unwrap();
        let opened =
            registry.dispatch(&mut session, "profile.file.open", json!({ "path": file, "waitMs": WAIT })).unwrap();
        assert_eq!(opened["profile"]["name"], "cpu.pprof");
        let report = registry.dispatch(&mut session, "profile.report.get", json!({})).unwrap();
        let frame = report["functions"][0]["frame"].clone();
        let answer = registry.dispatch(&mut session, "profile.frame.open", json!({ "frame": frame })).unwrap();
        assert_eq!(answer["line"], 5);
        assert_eq!(PathBuf::from(answer["path"].as_str().unwrap()), main);
        let selections = session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(selections, "package main\n\nfunc a() {}\n\n".len());
    }

    #[test]
    fn a_frame_without_a_file_here_is_refused() {
        let (registry, mut session, dir, _fake) = setup();
        let file = dir.path().join("cpu.pprof");
        std::fs::write(&file, sample_pprof()).unwrap();
        registry.dispatch(&mut session, "profile.file.open", json!({ "path": file, "waitMs": WAIT })).unwrap();
        assert!(registry.dispatch(&mut session, "profile.frame.open", json!({ "frame": 0 })).is_err());
        assert!(registry.dispatch(&mut session, "profile.frame.open", json!({ "frame": 99 })).is_err());
    }

    #[test]
    fn a_file_that_is_no_profile_shows_why() {
        let (registry, mut session, dir, _fake) = setup();
        let file = dir.path().join("broken.pprof");
        std::fs::write(&file, "nonsense").unwrap();
        let opened =
            registry.dispatch(&mut session, "profile.file.open", json!({ "path": file, "waitMs": WAIT })).unwrap();
        assert!(opened["profile"]["error"].is_string(), "{opened}");
        let report = registry.dispatch(&mut session, "profile.report.get", json!({})).unwrap();
        assert_eq!(report["functions"], json!([]));
    }

    #[test]
    fn an_agent_opens_no_profile_outside_the_project_and_the_person_does() {
        let (registry, mut session, _dir, _fake) = setup();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("cpu.pprof");
        std::fs::write(&file, sample_pprof()).unwrap();
        let params = json!({ "path": file, "waitMs": WAIT });
        let refused = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "profile.file.open", params.clone())
        });
        match refused {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED), "{error:?}")
            }
            other => panic!("not refused: {other:?}"),
        }
        assert!(registry.dispatch(&mut session, "profile.file.open", params).is_ok());
    }

    #[test]
    fn an_agent_opens_no_profile_hidden_from_it() {
        let (registry, mut session, dir, _fake) = setup();
        std::fs::create_dir_all(dir.path().join("secret")).unwrap();
        let file = dir.path().join("secret/cpu.pprof");
        std::fs::write(&file, sample_pprof()).unwrap();
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret/**".to_owned()]);
        let opened = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "profile.file.open", json!({ "path": file, "waitMs": WAIT }))
        });
        assert!(opened.is_err());
        assert!(session.state_mut::<Profiles>().shown().is_none());
    }
}
