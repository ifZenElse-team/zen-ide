use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use zen_i18n::t;
use zen_project::generators::{ToolRun, ToolRunner};
use zen_project::inspect::Programs as _;

use crate::build_tools::BuildEnv;
use crate::maven::settings::{self, MavenSettings, Purpose};

const MAVEN: &str = "maven";

const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default)]
pub struct BuildToolRunner {
    env: BuildEnv,
}

impl BuildToolRunner {
    pub fn new(env: BuildEnv) -> Self {
        Self { env }
    }

    fn maven(&self, run: &ToolRun, folder: &Path) -> Result<Command, String> {
        let env = &self.env;
        let maven = MavenSettings::default();
        let folders = env.folders(MAVEN);
        let program = settings::executable(&maven, folder, folder, env, &folders)
            .ok_or_else(|| t!("workbench-new-project-no-maven"))?
            .path;
        let java = run.java_home.as_deref().map(PathBuf::from).or_else(|| env.java_home(folder));
        let mut command = Command::new(program);
        command.args(maven.invocation(Purpose::Goal).args).args(&run.args).current_dir(folder);
        command.envs(env.environment_with(java, &folders));
        Ok(command)
    }
}

fn pipe_lines(pipe: Option<impl Read + Send + 'static>, lines: mpsc::Sender<String>) {
    let Some(pipe) = pipe else { return };
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
}

fn run_program(mut command: Command, stop: &AtomicBool, output: &dyn Fn(&str)) -> Result<bool, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let programs = zen_run::Programs::default();
    let mut child = programs.spawn(&mut command).map_err(|error| error.to_string())?;
    let (sender, lines) = mpsc::channel();
    pipe_lines(child.stdout.take(), sender.clone());
    pipe_lines(child.stderr.take(), sender);
    loop {
        match lines.recv_timeout(POLL) {
            Ok(line) => output(&line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if stop.load(Ordering::Relaxed) {
            programs.kill();
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    Ok(status.success())
}

impl ToolRunner for BuildToolRunner {
    fn run(&self, run: &ToolRun, folder: &Path, stop: &AtomicBool, output: &dyn Fn(&str)) -> Result<(), String> {
        if run.tool != MAVEN {
            return Err(t!("workbench-new-project-tool-unknown", tool = run.tool.as_str()));
        }
        let worked = run_program(self.maven(run, folder)?, stop, output)?;
        if stop.load(Ordering::Relaxed) {
            return Err(t!("workbench-new-project-cancelled"));
        }
        if worked { Ok(()) } else { Err(t!("workbench-new-project-tool-failed", tool = "Maven")) }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn fake_maven(folder: &Path, script: &str) -> BuildToolRunner {
        let bin = folder.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join("mvn");
        std::fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = BuildEnv::default();
        env.set_folders(Some(vec![bin]));
        BuildToolRunner::new(env)
    }

    fn maven_run(args: &[&str]) -> ToolRun {
        ToolRun {
            tool: MAVEN.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..ToolRun::default()
        }
    }

    #[test]
    fn maven_runs_in_the_folder_with_the_chosen_jdk_and_its_output_is_shown() {
        let folder = tempfile::tempdir().unwrap();
        let runner = fake_maven(folder.path(), r#"echo "args $*"; echo "java $JAVA_HOME"; echo err >&2; touch made"#);
        let work = folder.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let mut run = maven_run(&["archetype:generate", "-DgroupId=a b"]);
        run.java_home = Some("/jdk/21".to_owned());
        let seen = std::sync::Mutex::new(Vec::new());
        runner.run(&run, &work, &AtomicBool::new(false), &|line| seen.lock().unwrap().push(line.to_owned())).unwrap();
        let seen = seen.into_inner().unwrap();
        assert!(seen.contains(&"args archetype:generate -DgroupId=a b".to_owned()), "{seen:?}");
        assert!(seen.contains(&"java /jdk/21".to_owned()), "{seen:?}");
        assert!(seen.contains(&"err".to_owned()), "{seen:?}");
        assert!(work.join("made").is_file());
    }

    #[test]
    fn a_failing_maven_is_an_error() {
        let folder = tempfile::tempdir().unwrap();
        let runner = fake_maven(folder.path(), "exit 1");
        assert!(runner.run(&maven_run(&[]), folder.path(), &AtomicBool::new(false), &|_| {}).is_err());
    }

    #[test]
    fn no_maven_says_so() {
        let folder = tempfile::tempdir().unwrap();
        let env = BuildEnv::default();
        env.set_folders(Some(Vec::new()));
        let error = BuildToolRunner::new(env).run(&maven_run(&[]), folder.path(), &AtomicBool::new(false), &|_| {});
        assert_eq!(error, Err(t!("workbench-new-project-no-maven")));
    }

    #[test]
    fn another_tool_is_refused() {
        let run = ToolRun { tool: "make".to_owned(), ..ToolRun::default() };
        let refused = BuildToolRunner::default().run(&run, Path::new("/"), &AtomicBool::new(false), &|_| {});
        assert!(refused.is_err());
    }
}
