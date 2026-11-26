use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    Configuration, CoverageCounts, CoverageReport, LineCoverage, LineStatus, RunProvider, RunState, provider_for,
};

use crate::run::{MAX_RUN_WAIT, RunSummary, Runs, StartParams, configuration_to_start, project_root, run_context};
use crate::{Workbench, display};

#[derive(Clone, Debug)]
pub struct Suite {
    pub run_id: u64,
    pub name: String,
    pub root: PathBuf,
    pub report: Arc<CoverageReport>,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    revision: u64,
    suite: Option<Suite>,
    loading: Option<u64>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub struct Coverage {
    shared: Arc<Shared>,
}

impl Coverage {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    pub fn suite(&self) -> Option<Suite> {
        self.lock().suite.clone()
    }

    pub fn loading(&self) -> Option<u64> {
        self.lock().loading
    }

    pub fn hide(&self) -> bool {
        let mut state = self.lock();
        let shown = state.suite.take().is_some();
        state.revision += 1;
        drop(state);
        self.shared.changed.notify_all();
        shown
    }

    fn begin(&self, run_id: u64) {
        let mut state = self.lock();
        state.loading = Some(run_id);
        state.revision += 1;
    }

    fn finish(&self, run_id: u64, suite: Option<Suite>) {
        let mut state = self.lock();
        if state.loading == Some(run_id) {
            state.loading = None;
        }
        if let Some(suite) = suite
            && state.suite.as_ref().is_none_or(|shown| shown.run_id <= run_id)
        {
            state.suite = Some(suite);
        }
        state.revision += 1;
        drop(state);
        self.shared.changed.notify_all();
    }

    pub fn wait_loaded(&self, run_id: u64, timeout: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, timeout, |state| state.loading == Some(run_id))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.loading != Some(run_id)
    }
}

fn report_file() -> PathBuf {
    crate::run::scratch_folder().join("coverage.out")
}

fn percent(counts: &CoverageCounts) -> f64 {
    (counts.percent() * 10.0).round() / 10.0
}

fn unsupported(configuration: &Configuration) -> CommandError {
    CommandError::new(t!("workbench-coverage-unsupported", kind = configuration.kind.as_str()))
}

fn provider_of(session: &Session, configuration: &Configuration) -> Option<Arc<dyn RunProvider>> {
    let runs = session.state::<Runs>()?;
    provider_for(runs.providers(), &configuration.kind).cloned()
}

fn collect(
    coverage: Coverage,
    provider: Arc<dyn RunProvider>,
    configuration: Configuration,
    context: zen_run::RunContext,
    output: zen_run::SharedOutput,
    run_id: u64,
    report: PathBuf,
) {
    while !output.wait_end(MAX_RUN_WAIT) {}
    let stopped = output.read(|output| output.state() == RunState::Stopped);
    let suite = (!stopped).then(|| {
        let read = provider.coverage(&configuration).map_or_else(
            || Err(unsupported(&configuration).message),
            |reader| reader.read(&configuration, &context, &report),
        );
        let (report, error) = match read {
            Ok(report) => (report, None),
            Err(error) => (CoverageReport::default(), Some(error)),
        };
        Suite { run_id, name: configuration.name.clone(), root: context.root.clone(), report: Arc::new(report), error }
    });
    if let Some(folder) = report.parent() {
        let _ = std::fs::remove_dir_all(folder);
    }
    let line = suite.as_ref().map(|suite| match &suite.error {
        Some(error) => t!("workbench-coverage-console-error", error = error.as_str()),
        None => t!(
            "workbench-coverage-console",
            percent = format!("{:.1}", percent(&suite.report.counts())),
            files = suite.report.files.len()
        ),
    });
    if let Some(line) = line {
        output.update(|output| {
            output.system(&line);
        });
    }
    coverage.finish(run_id, suite);
}

pub(crate) fn start(
    session: &mut Session,
    configuration: Configuration,
    launch: crate::run::Launch,
) -> Result<u64, CommandError> {
    let root = project_root(session)?;
    let provider = provider_of(session, &configuration).ok_or_else(|| unsupported(&configuration))?;
    if provider.coverage(&configuration).is_none() || crate::compound::is_compound(&configuration) {
        return Err(unsupported(&configuration));
    }
    let context = run_context(session, &root);
    let report = report_file();
    if let Some(folder) = report.parent() {
        zen_run::create_private_folder(folder).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let launch = crate::run::Launch { recording: Some(crate::run::Recording::Coverage(report.clone())), ..launch };
    let started = crate::run::start_with(session, configuration.clone(), launch, &[]);
    let run_id = match started {
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
        .ok_or_else(|| CommandError::new(t!("workbench-coverage-no-run")))?;
    let coverage = session.state_mut::<Coverage>().clone();
    coverage.begin(run_id);
    let spawned = coverage.clone();
    std::thread::Builder::new()
        .name("zen-coverage".to_owned())
        .spawn(move || collect(spawned, provider, configuration, context, output, run_id, report))
        .map_err(|error| CommandError::new(error.to_string()))?;
    Ok(run_id)
}

/// A run with coverage once it ended: what it measured in all.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuiteSummary {
    /// The run that measured it.
    pub run_id: u64,
    /// The configuration's name.
    pub name: String,
    /// Lines covered, partly covered and not covered in all files.
    pub counts: CoverageCounts,
    /// The share of measured lines that ran, covered or partly, in percent.
    pub percent: f64,
    /// How many files were measured.
    pub files: usize,
    /// Why the coverage data could not be read, if it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SuiteSummary {
    fn of(suite: &Suite) -> Self {
        let counts = suite.report.counts();
        Self {
            run_id: suite.run_id,
            name: suite.name.clone(),
            percent: percent(&counts),
            counts,
            files: suite.report.files.len(),
            error: suite.error.clone(),
        }
    }
}

/// What `coverage.run.start` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoverageStarted {
    /// The run's id, for `run.output.get` and `run.tests.get`.
    pub run_id: u64,
    /// With `waitMs`: where the run is and its test counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RunSummary>,
    /// With `waitMs`, once the run ended and its data was read: the coverage in all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<SuiteSummary>,
}

pub struct StartCoverage;

impl Command for StartCoverage {
    type Params = StartParams;
    type Output = CoverageStarted;
    const ID: &'static str = "coverage.run.start";
    const TITLE: &'static str = "Run with Coverage";
    const DESCRIPTION: &'static str = "Saves modified files and runs a configuration with coverage (the same \
         parameters as run.process.start); once it ends, the lines it ran show in the editor's gutter and the \
         Coverage tool window. Go tests use go test -coverprofile, Cargo tests cargo-llvm-cov when it is installed. \
         With waitMs it answers once the run ended and its coverage was read, with both summaries; then \
         coverage.report.get lists the files and coverage.file.get one file's lines.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<CoverageStarted, CommandError> {
        let wait = params.wait_ms.map(Duration::from_millis);
        let launch = crate::run::Launch {
            prompts: crate::run::Prompts::of(&params),
            confirm_rerun: params.confirm_rerun,
            ..crate::run::Launch::default()
        };
        let configuration = configuration_to_start(session, params)?;
        let run_id = start(session, configuration, launch)?;
        let Some(wait) = wait else { return Ok(CoverageStarted { run_id, summary: None, coverage: None }) };
        let started = Instant::now();
        let summary = crate::run::wait_for_run(session, run_id, wait)?;
        let coverage = session.state_mut::<Coverage>().clone();
        let left = wait.min(MAX_RUN_WAIT).saturating_sub(started.elapsed());
        let loaded = summary.done && coverage.wait_loaded(run_id, left);
        let suite = coverage.suite().filter(|suite| loaded && suite.run_id == run_id);
        Ok(CoverageStarted { run_id, summary: Some(summary), coverage: suite.as_ref().map(SuiteSummary::of) })
    }
}

/// The parameters of `coverage.run.supports`: `configuration`, else `configurationId`, else the selected one.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SupportsParams {
    /// A saved configuration.
    pub configuration_id: Option<String>,
    /// A configuration as given.
    pub configuration: Option<Configuration>,
}

/// What `coverage.run.supports` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SupportsAnswer {
    /// Whether the configuration's provider runs it with coverage.
    pub supported: bool,
}

pub struct SupportsCoverage;

impl Command for SupportsCoverage {
    type Params = SupportsParams;
    type Output = SupportsAnswer;
    const ID: &'static str = "coverage.run.supports";
    const TITLE: &'static str = "Can Run with Coverage";
    const DESCRIPTION: &'static str = "Whether a configuration (given, saved, or the selected one) can run with \
         coverage. A tool it needs that is missing is reported only when the run starts.";

    fn run(&self, session: &mut Session, params: SupportsParams) -> Result<SupportsAnswer, CommandError> {
        let configuration = match (params.configuration, params.configuration_id) {
            (Some(configuration), _) => Some(configuration),
            (None, id) => {
                let root = project_root(session)?;
                let file = crate::run::read_file(session, &root)?;
                match id {
                    Some(id) => file.get(&id).cloned(),
                    None => file.selected_configuration().cloned(),
                }
            }
        };
        let supported = configuration.is_some_and(|configuration| {
            provider_of(session, &configuration).is_some_and(|provider| provider.coverage(&configuration).is_some())
        });
        Ok(SupportsAnswer { supported })
    }
}

/// The parameters of `coverage.report.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportParams {
    /// Only the files in this folder (or this file), absolute or relative to the project.
    pub path: Option<PathBuf>,
}

/// One measured file and its coverage.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub path: PathBuf,
    pub counts: CoverageCounts,
    /// The share of its measured lines that ran, in percent.
    pub percent: f64,
}

/// What `coverage.report.get` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportAnswer {
    /// Whether coverage is shown in the editor and the Coverage tool window.
    pub shown: bool,
    /// The run with coverage whose data is still being read, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<u64>,
    /// Grows whenever the coverage shown changes.
    pub revision: u64,
    /// The coverage shown, in all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SuiteSummary>,
    /// The measured files, by path.
    pub files: Vec<FileSummary>,
}

pub struct GetReport;

impl Command for GetReport {
    type Params = ReportParams;
    type Output = ReportAnswer;
    const ID: &'static str = "coverage.report.get";
    const TITLE: &'static str = "Coverage Report";
    const DESCRIPTION: &'static str = "The coverage shown from the last run with coverage: the summary and every \
         measured file with its covered, partly covered and not covered lines and its percentage, as the Coverage \
         tool window lists them; path narrows it to a folder or a file.";

    fn run(&self, session: &mut Session, params: ReportParams) -> Result<ReportAnswer, CommandError> {
        let coverage = session.state_mut::<Coverage>().clone();
        let (revision, loading, suite) = (coverage.revision(), coverage.loading(), coverage.suite());
        let within = params.path.map(|path| resolve(session, &path));
        let files = suite
            .iter()
            .flat_map(|suite| suite.report.files.iter())
            .filter(|file| within.as_ref().is_none_or(|within| file.path.starts_with(within)))
            .map(|file| {
                let counts = file.counts();
                FileSummary { path: file.path.clone(), percent: percent(&counts), counts }
            })
            .collect();
        Ok(ReportAnswer {
            shown: suite.is_some(),
            loading,
            revision,
            summary: suite.as_ref().map(SuiteSummary::of),
            files,
        })
    }
}

/// The parameters of `coverage.file.get`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileParams {
    /// The file, absolute or relative to the project.
    pub path: PathBuf,
    /// The revision the caller has; when it is still the current one the lines are left out.
    #[serde(default)]
    pub known: Option<u64>,
}

/// What `coverage.file.get` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileAnswer {
    /// Grows whenever the coverage shown changes.
    pub revision: u64,
    /// Whether the revision is the one given as `known`, so the lines were left out.
    pub unchanged: bool,
    /// The file's measured lines; empty when it was not measured or coverage is hidden.
    pub lines: Vec<LineCoverage>,
    /// The file's counts, when it was measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<CoverageCounts>,
}

pub struct GetFile;

impl Command for GetFile {
    type Params = FileParams;
    type Output = FileAnswer;
    const ID: &'static str = "coverage.file.get";
    const TITLE: &'static str = "File Coverage";
    const DESCRIPTION: &'static str = "One file's measured lines from the coverage shown, as the gutter marks them: \
         each line (from 1) with how many times it ran and whether it is covered, partly covered (some parts or \
         branches did not run) or not covered.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<FileAnswer, CommandError> {
        let coverage = session.state_mut::<Coverage>().clone();
        let revision = coverage.revision();
        if params.known == Some(revision) {
            return Ok(FileAnswer { revision, unchanged: true, lines: Vec::new(), counts: None });
        }
        let path = resolve(session, &params.path);
        let suite = coverage.suite();
        let file = suite.as_ref().and_then(|suite| suite.report.file(&path));
        Ok(FileAnswer {
            revision,
            unchanged: false,
            lines: file.map(|file| file.lines.clone()).unwrap_or_default(),
            counts: file.map(zen_run::FileCoverage::counts),
        })
    }
}

/// What `coverage.report.hide` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HideAnswer {
    /// Whether coverage was shown before.
    pub hidden: bool,
}

pub struct HideCoverage;

impl Command for HideCoverage {
    type Params = NoParams;
    type Output = HideAnswer;
    const ID: &'static str = "coverage.report.hide";
    const TITLE: &'static str = "Hide Coverage";
    const DESCRIPTION: &'static str = "Hides the coverage shown: the gutter marks go and the Coverage tool window empties until the next run with coverage.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HideAnswer, CommandError> {
        Ok(HideAnswer { hidden: session.state_mut::<Coverage>().hide() })
    }
}

/// The parameters of `coverage.report.export`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// The folder the report is written to (created when missing), absolute or relative to the project.
    pub folder: PathBuf,
}

/// What `coverage.report.export` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportAnswer {
    /// The report's page, to open in a browser.
    pub index: PathBuf,
}

pub struct ExportReport;

impl Command for ExportReport {
    type Params = ExportParams;
    type Output = ExportAnswer;
    const ID: &'static str = "coverage.report.export";
    const TITLE: &'static str = "Generate Coverage Report";
    const DESCRIPTION: &'static str = "Writes the coverage shown as an HTML report into a folder: index.html with \
         every file's percentage and its source with the lines that ran, ran partly and did not run marked.";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<ExportAnswer, CommandError> {
        let suite =
            session.state_mut::<Coverage>().suite().ok_or_else(|| CommandError::new(t!("workbench-coverage-none")))?;
        let folder = resolve(session, &params.folder);
        let index = folder.join("index.html");
        let mut written: Vec<PathBuf> = vec![folder.clone(), index.clone()];
        written.extend(suite.report.files.iter().map(|file| file.path.clone()));
        crate::hidden_files::guard_change(session, &suite.root, &written)?;
        let failed = |error: std::io::Error| {
            CommandError::new(t!(
                "workbench-coverage-export-failed",
                path = display(&folder),
                error = error.to_string()
            ))
        };
        std::fs::create_dir_all(&folder).map_err(failed)?;
        std::fs::write(&index, html(&suite)).map_err(failed)?;
        Ok(ExportAnswer { index })
    }
}

fn resolve(session: &Session, path: &Path) -> PathBuf {
    session.state::<Workbench>().map_or_else(|| path.to_owned(), |workbench| workbench.resolve(path))
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn css(color: zen_theme::Rgba) -> String {
    format!("rgba({}, {}, {}, {:.3})", color.r(), color.g(), color.b(), f32::from(color.a()) / 255.0)
}

fn shown_path(suite: &Suite, path: &Path) -> String {
    path.strip_prefix(&suite.root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn html(suite: &Suite) -> String {
    use zen_theme::{EditorKey, UiKey};
    let theme = zen_theme::ResolvedTheme::zen_light();
    let class = |status: LineStatus| match status {
        LineStatus::Covered => "covered",
        LineStatus::Partly => "partly",
        LineStatus::NotCovered => "not-covered",
    };
    let counts = suite.report.counts();
    let mut page = String::new();
    page.push_str("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    page.push_str(&format!(
        "<title>{}</title>\n",
        escape(&t!("workbench-coverage-html-title", name = suite.name.as_str()))
    ));
    page.push_str(&format!(
        "<style>\nbody {{ font-family: sans-serif; color: {text}; background: {background}; }}\n\
         table {{ border-collapse: collapse; }}\ntd, th {{ padding: 2px 12px; text-align: left; }}\n\
         th {{ color: {secondary}; font-weight: normal; }}\npre {{ margin: 0; }}\n\
         .line {{ display: block; padding-left: 8px; border-left: 4px solid transparent; }}\n\
         .number {{ color: {secondary}; display: inline-block; width: 4em; }}\n\
         .covered {{ border-left-color: {covered}; }}\n.partly {{ border-left-color: {partly}; }}\n\
         .not-covered {{ border-left-color: {uncovered}; }}\n</style>\n</head>\n<body>\n",
        text = css(theme.ui(UiKey::TextPrimary)),
        background = css(theme.ui(UiKey::IslandBackground)),
        secondary = css(theme.ui(UiKey::TextSecondary)),
        covered = css(theme.editor(EditorKey::CoverageCovered)),
        partly = css(theme.editor(EditorKey::CoveragePartly)),
        uncovered = css(theme.editor(EditorKey::CoverageNotCovered)),
    ));
    page.push_str(&format!("<h1>{}</h1>\n", escape(&t!("workbench-coverage-html-title", name = suite.name.as_str()))));
    page.push_str(&format!(
        "<p>{}</p>\n",
        escape(&t!(
            "workbench-coverage-html-summary",
            percent = format!("{:.1}", percent(&counts)),
            hit = counts.hit(),
            total = counts.total()
        ))
    ));
    page.push_str(&format!(
        "<table>\n<tr><th>{}</th><th>{}</th><th>{}</th></tr>\n",
        escape(&t!("workbench-coverage-html-file")),
        escape(&t!("workbench-coverage-html-percent")),
        escape(&t!("workbench-coverage-html-lines"))
    ));
    for (index, file) in suite.report.files.iter().enumerate() {
        let counts = file.counts();
        page.push_str(&format!(
            "<tr><td><a href=\"#file-{index}\">{}</a></td><td>{:.1}%</td><td>{}/{}</td></tr>\n",
            escape(&shown_path(suite, &file.path)),
            percent(&counts),
            counts.hit(),
            counts.total()
        ));
    }
    page.push_str("</table>\n");
    for (index, file) in suite.report.files.iter().enumerate() {
        page.push_str(&format!("<h2 id=\"file-{index}\">{}</h2>\n<pre>", escape(&shown_path(suite, &file.path))));
        let text = std::fs::read_to_string(&file.path).unwrap_or_default();
        for (number, source) in text.lines().enumerate() {
            let number = u32::try_from(number + 1).unwrap_or(u32::MAX);
            let measured = file.line(number);
            let class = measured.map_or("", |line| class(line.status));
            let title = measured.map_or(String::new(), |line| {
                format!(" title=\"{}\"", escape(&t!("workbench-coverage-html-hits", hits = line.hits)))
            });
            page.push_str(&format!(
                "<span class=\"line {class}\"{title}><span class=\"number\">{number}</span>{}</span>",
                escape(source)
            ));
        }
        page.push_str("</pre>\n");
    }
    page.push_str("</body>\n</html>\n");
    page
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(StartCoverage)?;
    registry.register(SupportsCoverage)?;
    registry.register(GetReport)?;
    registry.register(GetFile)?;
    registry.register(HideCoverage)?;
    registry.register(ExportReport)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_run::fake::{FakeProvider, ShellProvider};
    use zen_run::{RunEnv, Stream};

    use super::*;

    const PROFILE: &str = "mode: count\ncalc/calc.go:3.2,4.10 1 2\ncalc/calc.go:5.2,5.9 1 0\nmain.go:1.1,1.9 1 0\n";

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

    #[test]
    fn coverage_commands_are_registered_with_three_segments() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        for id in [
            "coverage.run.start",
            "coverage.run.supports",
            "coverage.report.get",
            "coverage.file.get",
            "coverage.report.hide",
            "coverage.report.export",
        ] {
            assert!(registry.commands().any(|info| info.id.as_str() == id), "{id}");
            assert_eq!(id.split('.').count(), 3, "{id}");
        }
    }

    fn refusal(answer: Result<Value, zen_core::DispatchError>) -> String {
        match answer {
            Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
            other => panic!("not refused: {other:?}"),
        }
    }

    #[test]
    fn coverage_runs_through_the_run_path_with_trust_and_before_launch_tasks() {
        let (registry, mut session, dir, fake) = setup();
        session.set_state(crate::build_tools::BuildEnv::default());
        fake.set_script(&[(Stream::Stdout, "ok")], 0);
        fake.set_coverage(Some(PROFILE));
        let created =
            registry.dispatch(&mut session, "run.configuration.create", json!({ "configuration": configuration("T") }));
        let id = created.unwrap()["id"].clone();
        let saved = registry.dispatch(&mut session, "coverage.run.start", json!({ "configurationId": id }));
        assert_eq!(refusal(saved), crate::trust::NOT_TRUSTED, "a saved configuration waits for trust");
        let mut tool = configuration("U");
        tool["common"]["beforeLaunch"] = json!([{ "type": crate::before_launch::EXTERNAL_TOOL, "tool": "x" }]);
        let before = registry.dispatch(&mut session, "coverage.run.start", json!({ "configuration": tool }));
        assert_eq!(refusal(before), crate::trust::NOT_TRUSTED, "a program before launch waits for trust");
        assert!(session.state::<Runs>().is_none_or(|runs| runs.runs().is_empty()), "nothing ran");

        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let started =
            registry.dispatch(&mut session, "coverage.run.start", json!({ "configurationId": id, "waitMs": 5000 }));
        assert_eq!(started.unwrap()["coverage"]["files"], 2);
    }

    #[test]
    fn an_agent_exports_no_coverage_report_into_or_out_of_hidden_files() {
        let (registry, mut session, dir, fake) = setup();
        fake.set_script(&[(Stream::Stdout, "ok")], 0);
        fake.set_coverage(Some(PROFILE));
        registry
            .dispatch(
                &mut session,
                "coverage.run.start",
                json!({ "configuration": configuration("T"), "waitMs": 5000 }),
            )
            .unwrap();
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret/**".to_owned()]);
        let hidden = dir.path().join("secret/report");
        let export = |session: &mut Session, folder: &Path| {
            let params = json!({ "folder": folder });
            zen_core::macros::as_agent(session, |session| registry.dispatch(session, "coverage.report.export", params))
        };
        assert!(export(&mut session, &hidden).is_err());
        assert!(!hidden.exists());
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["calc/**".to_owned()]);
        let open = dir.path().join("report");
        assert!(export(&mut session, &open).is_err(), "a measured hidden file would be copied into the report");
        assert!(!open.join("index.html").exists());
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[]);
        assert!(export(&mut session, &open).is_ok());
        assert!(open.join("index.html").is_file());
    }

    #[test]
    fn a_run_with_coverage_shows_its_files_and_lines_until_hidden() {
        let (registry, mut session, dir, fake) = setup();
        fake.set_script(&[(Stream::Stdout, "ok")], 1);
        let supports = |session: &mut Session, configuration: Value| {
            registry.dispatch(session, "coverage.run.supports", json!({ "configuration": configuration })).unwrap()
        };
        assert_eq!(supports(&mut session, configuration("T")), json!({ "supported": false }));
        let refused =
            registry.dispatch(&mut session, "coverage.run.start", json!({ "configuration": configuration("T") }));
        assert!(refused.is_err(), "a provider without coverage refuses");
        fake.set_coverage(Some(PROFILE));
        assert_eq!(supports(&mut session, configuration("T")), json!({ "supported": true }));

        let started = registry
            .dispatch(
                &mut session,
                "coverage.run.start",
                json!({ "configuration": configuration("T"), "waitMs": 5000 }),
            )
            .unwrap();
        assert_eq!(started["summary"]["exitCode"], 1, "failed tests still give coverage");
        assert_eq!(started["coverage"]["files"], 2);
        assert_eq!(started["coverage"]["counts"], json!({ "covered": 2, "partly": 0, "notCovered": 2 }));
        assert_eq!(started["coverage"]["percent"], 50.0);
        let run_id = started["runId"].as_u64().unwrap();
        let console = registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id })).unwrap();
        let texts: Vec<&str> =
            console["lines"].as_array().unwrap().iter().filter_map(|line| line["text"].as_str()).collect();
        assert_eq!(texts.first(), Some(&"fake T -cover"));
        assert!(texts.last().is_some_and(|line| line.contains("50.0")), "{texts:?}");

        let report = registry.dispatch(&mut session, "coverage.report.get", json!({})).unwrap();
        assert_eq!(report["shown"], true);
        let paths: Vec<PathBuf> = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
            .collect();
        assert_eq!(paths, [dir.path().join("calc/calc.go"), dir.path().join("main.go")]);
        let narrowed = registry.dispatch(&mut session, "coverage.report.get", json!({ "path": "calc" })).unwrap();
        assert_eq!(narrowed["files"].as_array().map(Vec::len), Some(1));

        let file = registry.dispatch(&mut session, "coverage.file.get", json!({ "path": "calc/calc.go" })).unwrap();
        assert_eq!(
            file["lines"],
            json!([
                { "line": 3, "hits": 2, "status": "covered" },
                { "line": 4, "hits": 2, "status": "covered" },
                { "line": 5, "hits": 0, "status": "notCovered" },
            ])
        );
        let revision = file["revision"].clone();
        let again = registry
            .dispatch(&mut session, "coverage.file.get", json!({ "path": "calc/calc.go", "known": revision }))
            .unwrap();
        assert_eq!((again["unchanged"].clone(), again["lines"].clone()), (json!(true), json!([])));

        let rerun = registry.dispatch(&mut session, "run.process.rerun", json!({ "runId": run_id })).unwrap();
        let rerun_id = rerun["runId"].as_u64().unwrap();
        assert!(session.state_mut::<Coverage>().clone().wait_loaded(rerun_id, Duration::from_secs(10)));
        assert_eq!(
            session.state_mut::<Coverage>().suite().map(|suite| suite.run_id),
            Some(rerun_id),
            "a rerun of a run with coverage measures again"
        );

        assert_eq!(
            registry.dispatch(&mut session, "coverage.report.hide", json!({})).unwrap(),
            json!({ "hidden": true })
        );
        let hidden = registry.dispatch(&mut session, "coverage.report.get", json!({})).unwrap();
        assert_eq!((hidden["shown"].clone(), hidden["files"].clone()), (json!(false), json!([])));
        let file = registry
            .dispatch(&mut session, "coverage.file.get", json!({ "path": "calc/calc.go", "known": revision }))
            .unwrap();
        assert_eq!((file["unchanged"].clone(), file["lines"].clone()), (json!(false), json!([])));
        assert_eq!(
            registry.dispatch(&mut session, "coverage.report.hide", json!({})).unwrap(),
            json!({ "hidden": false })
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_process_writes_the_report_the_provider_reads_and_the_file_is_removed() {
        let (registry, mut session, dir, _fake) = setup();
        let script = format!(
            "printf 'mode: set\\nlib.sh:1.1,2.1 1 1\\n' > \"${}\"; echo \"$ZEN_COVERAGE_REPORT\" > reported",
            zen_run::fake::COVERAGE_REPORT_VARIABLE
        );
        let shell = serde_json::to_value(ShellProvider::configuration("sh", &script)).unwrap();
        let started = registry
            .dispatch(&mut session, "coverage.run.start", json!({ "configuration": shell, "waitMs": 10000 }))
            .unwrap();
        assert_eq!(started["coverage"]["counts"]["covered"], 1, "{started}");
        let reported = std::fs::read_to_string(dir.path().join("reported")).unwrap();
        assert!(!Path::new(reported.trim()).exists(), "the report is removed once read");
    }

    #[test]
    fn a_report_that_cannot_be_read_is_shown_with_its_error() {
        let (registry, mut session, _dir, fake) = setup();
        fake.set_coverage(Some("not a profile"));
        let started = registry
            .dispatch(
                &mut session,
                "coverage.run.start",
                json!({ "configuration": configuration("T"), "waitMs": 5000 }),
            )
            .unwrap();
        assert!(
            started["coverage"]["error"].as_str().is_some_and(|error| error.contains("not a profile")),
            "{started}"
        );
        let run_id = started["runId"].as_u64().unwrap();
        let console = registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id })).unwrap();
        let last = console["lines"].as_array().unwrap().last().unwrap()["text"].as_str().unwrap().to_owned();
        assert!(last.contains("not a profile"), "{last}");
    }

    #[test]
    fn the_console_line_is_written_before_a_waiter_hears_the_coverage_was_read() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeProvider::new();
        fake.set_coverage(Some("not a profile"));
        let coverage = Coverage::default();
        coverage.begin(7);
        let watched = coverage.clone();
        let loading_at_each_line = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&loading_at_each_line);
        let waker: zen_run::Waker = Arc::new(move || seen.lock().unwrap().push(watched.lock().loading));
        let output = zen_run::SharedOutput::new(zen_run::RunOutput::new(Box::new(zen_run::PlainParser)), Some(waker));
        output.update(|output| output.end(RunState::Finished, Some(0)));
        let configuration = FakeProvider::configuration("T");
        let report = dir.path().join("run/coverage.out");
        collect(coverage, Arc::new(fake), configuration, zen_run::RunContext::default(), output, 7, report);
        assert_eq!(loading_at_each_line.lock().unwrap().last(), Some(&Some(7)));
    }

    #[test]
    fn the_report_exports_as_html_with_every_file_and_its_marked_lines() {
        let (registry, mut session, dir, fake) = setup();
        assert!(registry.dispatch(&mut session, "coverage.report.export", json!({ "folder": "out" })).is_err());
        std::fs::create_dir_all(dir.path().join("calc")).unwrap();
        std::fs::write(dir.path().join("calc/calc.go"), "package calc\n\nfunc A() {\n\tb()\n\tc(\"<x>\")\n}\n")
            .unwrap();
        fake.set_coverage(Some(PROFILE));
        registry
            .dispatch(
                &mut session,
                "coverage.run.start",
                json!({ "configuration": configuration("T"), "waitMs": 5000 }),
            )
            .unwrap();
        let exported = registry.dispatch(&mut session, "coverage.report.export", json!({ "folder": "out" })).unwrap();
        let index = dir.path().join("out/index.html");
        assert_eq!(exported["index"], json!(index));
        let page = std::fs::read_to_string(index).unwrap();
        assert!(page.contains("<a href=\"#file-0\">calc/calc.go</a></td><td>66.7%</td><td>2/3</td>"), "{page}");
        assert!(page.contains("<span class=\"line covered\""), "{page}");
        assert!(page.contains("<span class=\"line not-covered\""), "{page}");
        assert!(page.contains("c(&quot;&lt;x&gt;&quot;)"), "{page}");
        let theme = zen_theme::ResolvedTheme::zen_light();
        assert!(page.contains(&css(theme.editor(zen_theme::EditorKey::CoverageCovered))), "theme colours only");
    }

    #[test]
    fn a_newer_suite_replaces_the_shown_one_and_an_older_one_does_not() {
        let coverage = Coverage::default();
        let suite = |run_id: u64| Suite {
            run_id,
            name: format!("run {run_id}"),
            root: PathBuf::new(),
            report: Arc::default(),
            error: None,
        };
        coverage.begin(2);
        coverage.finish(2, Some(suite(2)));
        coverage.finish(1, Some(suite(1)));
        assert_eq!(coverage.suite().map(|shown| shown.run_id), Some(2));
        coverage.begin(3);
        assert_eq!(coverage.loading(), Some(3));
        assert!(!coverage.wait_loaded(3, Duration::ZERO));
        coverage.finish(3, None);
        assert!(coverage.wait_loaded(3, Duration::ZERO));
        assert_eq!(coverage.suite().map(|shown| shown.run_id), Some(2), "a stopped run keeps what was shown");
    }
}
