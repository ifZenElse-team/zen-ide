use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::Session;
use zen_project::inspect::Programs;
use zen_run::RunOutput;

use crate::run::{Run, Runs};
use crate::tasks::{TaskState, Tasks};

pub const INDEX: &str = "index";
pub const LANGUAGE_SERVER: &str = "language-server";

/// What kind of work is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WorkKind {
    /// A quick-fix that updates dependencies (`go get`).
    DependencyUpdate,
    /// A Git operation: fetch, pull, push.
    Git,
    /// Any other background task.
    Task,
    /// A run configuration's program or tests.
    Run,
    /// A debug session.
    Debug,
}

/// Something still running that exiting would stop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunningWork {
    /// What kind of work it is.
    pub kind: WorkKind,
    /// Its name: the task's title or the run configuration's name.
    pub title: String,
}

pub fn running_work(session: &Session) -> Vec<RunningWork> {
    let mut work = Vec::new();
    if let Some(tasks) = session.state::<Tasks>() {
        let running = tasks.list().into_iter().filter(|task| task.state == TaskState::Running);
        for task in running.filter(|task| task.source != INDEX && task.source != LANGUAGE_SERVER) {
            let kind = match task.source.as_str() {
                "inspections" => WorkKind::DependencyUpdate,
                "vcs" => WorkKind::Git,
                _ => WorkKind::Task,
            };
            work.push(RunningWork { kind, title: task.title });
        }
    }
    if let Some(runs) = session.state::<Runs>() {
        for run in runs.runs().iter().filter(|run| run.output.read(RunOutput::is_running)) {
            let kind = if run.debug { WorkKind::Debug } else { WorkKind::Run };
            work.push(RunningWork { kind, title: run.configuration.name.clone() });
        }
    }
    work
}

pub fn running_runs(session: &Session) -> Vec<Run> {
    session
        .state::<Runs>()
        .map(|runs| runs.runs().iter().filter(|run| run.output.read(RunOutput::is_running)).cloned().collect())
        .unwrap_or_default()
}

pub fn running_programs(session: &Session) -> Vec<Arc<dyn Programs>> {
    session.state::<Tasks>().map(Tasks::running_programs).unwrap_or_default()
}

pub fn stop_work(runs: &[Run], programs: &[Arc<dyn Programs>], grace: Duration) {
    if runs.is_empty() && programs.is_empty() {
        return;
    }
    for run in runs {
        run.stop();
    }
    for program in programs {
        program.interrupt();
    }
    let running = || {
        runs.iter().any(|run| run.output.read(RunOutput::is_running))
            || programs.iter().any(|program| program.running())
    };
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline && running() {
        std::thread::sleep(Duration::from_millis(20));
    }
    for run in runs {
        run.kill();
    }
    for program in programs {
        program.kill();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zen_run::{Configuration, PlainParser, RunState, SharedOutput};

    use super::*;
    use crate::run::RunControl;

    struct Idle;

    impl RunControl for Idle {
        fn stop(&self) -> bool {
            false
        }

        fn kill(&self) {}
    }

    #[test]
    fn nothing_runs_in_a_new_window() {
        assert!(running_work(&Session::new()).is_empty());
    }

    #[test]
    fn the_running_tasks_are_named_by_their_kind() {
        let mut session = Session::new();
        let tasks = session.state_mut::<Tasks>();
        let _update = tasks.start("Updating 3 dependencies", "inspections", false);
        let fetch = tasks.start("Fetching origin", "vcs", false);
        let _other = tasks.start("Generating code", "build", false);
        let _index = tasks.start("Indexing the project", INDEX, false);
        let _server = tasks.start("gopls: Loading packages", LANGUAGE_SERVER, false);
        let done = tasks.start("Pushing main", "vcs", false);
        done.finish(Ok("Pushed".to_owned()));
        assert_eq!(
            running_work(&session),
            [
                RunningWork { kind: WorkKind::DependencyUpdate, title: "Updating 3 dependencies".to_owned() },
                RunningWork { kind: WorkKind::Git, title: "Fetching origin".to_owned() },
                RunningWork { kind: WorkKind::Task, title: "Generating code".to_owned() },
            ]
        );
        fetch.finish(Err("offline".to_owned()));
        assert_eq!(running_work(&session).len(), 2, "a failed task is over");
    }

    #[test]
    fn running_debug_sessions_are_named_by_their_configuration() {
        let mut session = Session::new();
        let configuration = Configuration { name: "server".to_owned(), ..Configuration::default() };
        let output = SharedOutput::new(RunOutput::new(Box::new(PlainParser)), None);
        let runs = session.state_mut::<Runs>();
        runs.insert_external(configuration.clone(), "go".to_owned(), Vec::new(), output.clone(), Arc::new(Idle));
        assert_eq!(running_work(&session), [RunningWork { kind: WorkKind::Debug, title: "server".to_owned() }]);
        output.update(|output| output.end(RunState::Stopped, None));
        assert!(running_work(&session).is_empty(), "an ended session is not running");
    }

    #[cfg(unix)]
    fn alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    fn start(session: &mut Session, folder: &std::path::Path, name: &str, first: &str, script: &str) -> String {
        let runs = session.state_mut::<Runs>();
        runs.set_providers(vec![Arc::new(zen_run::fake::ShellProvider)], zen_run::RunEnv::default());
        let pid_file = folder.join(name);
        let script = format!("{first}; echo $$ > '{}'; {script}", pid_file.display());
        let context = zen_run::RunContext { root: folder.to_owned(), ..zen_run::RunContext::default() };
        runs.start(zen_run::fake::ShellProvider::configuration(name, &script), &context).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file).ok().filter(|pid| pid.ends_with('\n')) {
                return pid.trim().to_owned();
            }
            assert!(Instant::now() < deadline, "{name} did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn exiting_stops_every_run_and_kills_the_ones_that_do_not_stop() {
        let folder = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        let polite = start(&mut session, folder.path(), "polite", ":", "sleep 60");
        let stubborn = start(&mut session, folder.path(), "stubborn", "trap '' INT", "while :; do sleep 0.05; done");
        let runs = running_runs(&session);
        assert_eq!(runs.len(), 2);
        let grace = Duration::from_millis(600);
        let at = Instant::now();
        stop_work(&runs, &[], grace);
        assert!(at.elapsed() >= grace, "the stubborn program had its grace: {:?}", at.elapsed());
        let deadline = Instant::now() + Duration::from_secs(60);
        while alive(&polite) {
            assert!(Instant::now() < deadline, "the interrupted program ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        while alive(&stubborn) {
            assert!(Instant::now() < deadline, "the program that ignores the interrupt is killed");
            std::thread::sleep(Duration::from_millis(20));
        }
        while !running_runs(&session).is_empty() {
            assert!(Instant::now() < deadline, "no run is left running");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
