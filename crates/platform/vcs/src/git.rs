use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::VcsError;

static ENVIRONMENT: Mutex<Vec<(OsString, OsString)>> = Mutex::new(Vec::new());
static TLS_ENVIRONMENT: Mutex<Vec<(OsString, OsString)>> = Mutex::new(Vec::new());

pub fn set_environment(variables: &[(&str, &str)]) {
    let mut environment = ENVIRONMENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *environment = variables.iter().map(|(name, value)| (OsString::from(name), OsString::from(value))).collect();
}

pub fn person_ca_file() -> Option<PathBuf> {
    let home = dirs_home();
    let output =
        start(&home, &["config", "--global", "--type=path", "--get", "http.sslCAInfo"], None, &[], &mut |_| {}).ok()?;
    let path = output.stdout.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from)
}

pub fn set_tls_environment(variables: &[(&str, &str)]) {
    let mut environment = TLS_ENVIRONMENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *environment = variables.iter().map(|(name, value)| (OsString::from(name), OsString::from(value))).collect();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spawned {
    Started(u32),
    Ended(u32),
}

pub type Watcher = Box<dyn Fn(Spawned)>;

std::thread_local! {
    static WATCHER: std::cell::RefCell<Option<Watcher>> = const { std::cell::RefCell::new(None) };
}

pub fn watched<T>(watcher: Watcher, work: impl FnOnce() -> T) -> T {
    let previous = WATCHER.with(|slot| slot.replace(Some(watcher)));
    let result = work();
    WATCHER.with(|slot| *slot.borrow_mut() = previous);
    result
}

fn tell(spawned: Spawned) {
    WATCHER.with(|slot| {
        if let Some(watcher) = slot.borrow().as_ref() {
            watcher(spawned);
        }
    });
}

#[cfg(unix)]
fn watching() -> bool {
    WATCHER.with(|slot| slot.borrow().is_some())
}

pub type Gate = std::sync::Arc<dyn Fn(&Path) -> bool + Send + Sync>;

static GATE: std::sync::RwLock<Option<Gate>> = std::sync::RwLock::new(None);

pub fn set_gate(gate: Option<Gate>) {
    *GATE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = gate;
}

pub fn allowed(path: &Path) -> bool {
    let gate = GATE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    gate.is_none_or(|gate| gate(path))
}

pub fn executable() -> PathBuf {
    static FOUND: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            let on_path = std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path).map(|folder| folder.join(program_name())).find(|file| file.is_file())
            });
            on_path
                .or_else(|| {
                    ["/opt/homebrew/bin/git", "/usr/local/bin/git", "/usr/bin/git"]
                        .into_iter()
                        .map(PathBuf::from)
                        .find(|file| file.is_file())
                })
                .unwrap_or_else(|| PathBuf::from(program_name()))
        })
        .clone()
}

fn program_name() -> &'static str {
    if cfg!(windows) { "git.exe" } else { "git" }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub phase: String,
    pub fraction: Option<f32>,
    pub line: String,
}

impl Progress {
    pub fn parse(line: &str) -> Self {
        let line = line.trim().trim_start_matches("remote:").trim();
        let mut progress = Progress { line: line.to_owned(), ..Progress::default() };
        if let Some((phase, rest)) = line.split_once(':') {
            let percent = rest.trim_start().split('%').next().and_then(|number| number.trim().parse::<f32>().ok());
            if let Some(percent) = percent {
                progress.phase = phase.trim().to_owned();
                progress.fraction = Some((percent / 100.0).clamp(0.0, 1.0));
            }
        }
        progress
    }
}

pub fn run(root: &Path, args: &[&str], input: Option<&str>) -> Result<Output, VcsError> {
    run_with_progress(root, args, input, &mut |_| {})
}

pub fn run_with_progress(
    root: &Path,
    args: &[&str],
    input: Option<&str>,
    progress: &mut dyn FnMut(Progress),
) -> Result<Output, VcsError> {
    run_with(root, args, input, &[], progress)
}

pub fn run_with_environment(
    root: &Path,
    args: &[&str],
    input: Option<&str>,
    environment: &[(&str, &str)],
) -> Result<Output, VcsError> {
    run_with(root, args, input, environment, &mut |_| {})
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloneRequest {
    pub url: String,
    pub folder: PathBuf,
    pub depth: Option<u32>,
}

fn clone_arguments(request: &CloneRequest) -> Vec<String> {
    let mut args: Vec<String> =
        ["-c", "protocol.ext.allow=never", "-c", "submodule.recurse=false", "clone", "--progress"]
            .map(str::to_owned)
            .to_vec();
    if let Some(depth) = request.depth {
        args.extend(["--depth".to_owned(), depth.to_string()]);
    }
    args.extend(["--".to_owned(), request.url.clone(), request.folder.to_string_lossy().into_owned()]);
    args
}

pub fn clone(request: &CloneRequest, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
    let root = request.folder.parent().unwrap_or(Path::new("."));
    let args = clone_arguments(request);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    start(root, &args, None, &[], progress).map(|_| ())
}

pub fn init(folder: &Path) -> Result<(), VcsError> {
    start(folder, &["init", "-q"], None, &[], &mut |_| {}).map(|_| ())
}

fn run_with(
    root: &Path,
    args: &[&str],
    input: Option<&str>,
    environment: &[(&str, &str)],
    progress: &mut dyn FnMut(Progress),
) -> Result<Output, VcsError> {
    if !allowed(root) {
        return Err(VcsError::NotTrusted);
    }
    start(root, args, input, environment, progress)
}

fn start(
    root: &Path,
    args: &[&str],
    input: Option<&str>,
    environment: &[(&str, &str)],
    progress: &mut dyn FnMut(Progress),
) -> Result<Output, VcsError> {
    let mut command = Command::new(executable());
    command.args(["-c", "core.fsmonitor=false"]);
    command
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in ENVIRONMENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter() {
        command.env(name, value);
    }
    for (name, value) in TLS_ENVIRONMENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter() {
        command.env(name, value);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    #[cfg(unix)]
    if watching() {
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
    }
    let child = command.spawn().map_err(|error| VcsError::NoGit(error.to_string()))?;
    let pid = child.id();
    tell(Spawned::Started(pid));
    let result = finish(child, input, progress);
    tell(Spawned::Ended(pid));
    result
}

fn finish(
    mut child: std::process::Child,
    input: Option<&str>,
    progress: &mut dyn FnMut(Progress),
) -> Result<Output, VcsError> {
    let writer = input.zip(child.stdin.take()).map(|(text, mut stdin)| {
        let text = text.to_owned();
        std::thread::spawn(move || stdin.write_all(text.as_bytes()))
    });
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let mut line = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            for &byte in &buffer[..read] {
                if byte == b'\r' || byte == b'\n' {
                    let text = String::from_utf8_lossy(&line).into_owned();
                    if !text.trim().is_empty() {
                        progress(Progress::parse(&text));
                        if byte == b'\n' {
                            stderr.push_str(&text);
                            stderr.push('\n');
                        }
                    }
                    line.clear();
                } else {
                    line.push(byte);
                }
            }
        }
        if !line.is_empty() {
            stderr.push_str(&String::from_utf8_lossy(&line));
        }
    }
    let stdout = reader.join().unwrap_or_default();
    let written = writer.map_or(Ok(()), |writer| writer.join().unwrap_or(Ok(())));
    let status = child.wait().map_err(|error| VcsError::Git(error.to_string()))?;
    if !status.success() {
        return Err(VcsError::Git(message(&stderr, &stdout)));
    }
    written.map_err(|error| VcsError::Git(error.to_string()))?;
    Ok(Output { stdout, stderr })
}

fn message(stderr: &str, stdout: &str) -> String {
    let rejected = stdout.lines().filter(|line| line.starts_with("!\t")).filter_map(|line| line.rsplit('\t').next());
    let important: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("error:") || line.starts_with("fatal:") || line.starts_with("! ["))
        .chain(rejected)
        .collect();
    let text = if important.is_empty() {
        let all = format!("{}\n{}", stderr.trim(), stdout.trim());
        all.trim().to_owned()
    } else {
        important.join("\n")
    };
    text.lines()
        .map(|line| line.trim_start_matches("error: ").trim_start_matches("fatal: "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(folder: &Path, args: &[&str]) {
        let output = Command::new(executable()).current_dir(folder).args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn remote(folder: &Path) {
        std::fs::create_dir_all(folder).unwrap();
        git_in(folder, &["init", "-q"]);
        git_in(folder, &["config", "user.email", "a@b.c"]);
        git_in(folder, &["config", "user.name", "A"]);
        for number in 0..3 {
            std::fs::write(folder.join("file.txt"), number.to_string()).unwrap();
            git_in(folder, &["add", "-A"]);
            git_in(folder, &["commit", "-q", "-m", "change"]);
        }
    }

    #[test]
    fn a_clone_leaves_submodules_for_later_and_truncates_the_history_on_request() {
        let plain = CloneRequest { url: "u".to_owned(), folder: PathBuf::from("/f"), depth: None };
        let args = clone_arguments(&plain);
        assert!(!args.iter().any(|arg| arg.contains("recurse-submodules") || arg.contains("shallow-submodules")));
        assert!(args.contains(&"submodule.recurse=false".to_owned()), "the person's own setting does not start it");
        assert!(!args.contains(&"--depth".to_owned()));
        let shallow = clone_arguments(&CloneRequest { depth: Some(7), ..plain });
        let depth = shallow.iter().position(|arg| arg == "--depth").unwrap();
        assert_eq!(shallow[depth + 1], "7");
    }

    #[test]
    fn a_shallow_clone_has_that_many_commits() {
        let folder = tempfile::tempdir().unwrap();
        let origin = folder.path().join("origin");
        remote(&origin);
        let target = folder.path().join("clone");
        let url = format!("file://{}", origin.display());
        clone(&CloneRequest { url, folder: target.clone(), depth: Some(1) }, &mut |_| {}).unwrap();
        let count = run(&target, &["rev-list", "--count", "HEAD"], None).unwrap();
        assert_eq!(count.stdout.trim(), "1");
    }

    #[test]
    fn a_url_cannot_run_a_program_or_pass_for_an_option() {
        let folder = tempfile::tempdir().unwrap();
        let mark = folder.path().join("ran");
        let url = format!("ext::sh -c touch% {}", mark.display());
        let target = folder.path().join("clone");
        assert!(clone(&CloneRequest { url, folder: target.clone(), depth: None }, &mut |_| {}).is_err());
        assert!(!mark.exists(), "the ext transport stays off");
        let option = CloneRequest { url: "--upload-pack=touch ran".to_owned(), folder: target, depth: None };
        assert!(clone(&option, &mut |_| {}).is_err());
        assert!(!folder.path().join("ran").exists());
    }

    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"], None).unwrap();
        dir
    }

    #[test]
    fn a_repository_config_runs_no_fsmonitor_program_when_the_ide_asks_git() {
        let dir = folder();
        let marker = dir.path().join("ran");
        let program = format!("touch {}; true", marker.display());
        run(dir.path(), &["config", "core.fsmonitor", &program], None).unwrap();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        run(dir.path(), &["status", "--porcelain"], None).unwrap();
        run(dir.path(), &["add", "a.txt"], None).unwrap();
        run(dir.path(), &["diff", "--name-only"], None).unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn output_that_is_not_utf8_is_kept_with_replacement_characters() {
        let dir = folder();
        std::fs::write(dir.path().join("latin1.txt"), b"caf\xe9\n").unwrap();
        let blob = run(dir.path(), &["hash-object", "-w", "latin1.txt"], None).unwrap().stdout;
        let shown = run(dir.path(), &["cat-file", "blob", blob.trim()], None).unwrap();
        assert_eq!(shown.stdout, "caf\u{fffd}\n");
    }

    #[test]
    fn a_program_that_stops_reading_its_input_still_reports_why_it_failed() {
        let dir = folder();
        let input = "x".repeat(1 << 20);
        let failed = run(dir.path(), &["no-such-command"], Some(&input)).unwrap_err();
        let VcsError::Git(message) = failed else { panic!("{failed:?}") };
        assert!(message.contains("no-such-command"), "{message}");
    }

    #[test]
    fn input_and_output_larger_than_a_pipe_do_not_block_each_other() {
        let dir = folder();
        let input = format!("{}\n", "0".repeat(40)).repeat(20_000);
        let output = run(dir.path(), &["cat-file", "--batch-check"], Some(&input)).unwrap();
        assert_eq!(output.stdout.lines().count(), 20_000);
    }

    #[test]
    fn the_trust_store_reaches_git_through_its_environment() {
        let folder = tempfile::tempdir().unwrap();
        set_tls_environment(&[("GIT_SSL_CAINFO", "/tmp/zen-bundle.pem")]);
        let output = run(folder.path(), &["-c", "alias.show-environment=!env", "show-environment"], None).unwrap();
        set_tls_environment(&[]);
        assert!(output.stdout.contains("GIT_SSL_CAINFO=/tmp/zen-bundle.pem"), "{}", output.stdout);
    }

    #[test]
    fn progress_lines_give_the_phase_and_how_far_it_is() {
        let progress = Progress::parse("Receiving objects:  45% (450/1000), 1.20 MiB | 2.00 MiB/s");
        assert_eq!(progress.phase, "Receiving objects");
        assert_eq!(progress.fraction, Some(0.45));
        let remote = Progress::parse("remote: Counting objects: 100% (3/3), done.");
        assert_eq!(remote.phase, "Counting objects");
        assert_eq!(remote.fraction, Some(1.0));
        let plain = Progress::parse("To github.com:owner/repo.git");
        assert_eq!(plain.fraction, None);
        assert_eq!(plain.line, "To github.com:owner/repo.git");
    }

    #[test]
    fn failures_keep_the_lines_that_explain_them() {
        let stderr = "hint: something\nerror: failed to push some refs to 'origin'\nhint: pull first\n";
        assert_eq!(message(stderr, ""), "failed to push some refs to 'origin'");
        assert_eq!(message("fatal: not a git repository\n", ""), "not a git repository");
        let porcelain =
            "To origin\n!\trefs/heads/a:refs/heads/a\t[rejected] (remote ref updated since checkout)\nDone\n";
        assert_eq!(
            message("error: failed to push some refs to 'origin'\n", porcelain),
            "failed to push some refs to 'origin'\n[rejected] (remote ref updated since checkout)"
        );
        assert_eq!(message("", "nothing to commit, working tree clean\n"), "nothing to commit, working tree clean");
    }

    #[test]
    fn git_never_waits_for_a_password_on_a_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let prompt = run(dir.path(), &["-c", "alias.prompt=!printenv GIT_TERMINAL_PROMPT", "prompt"], None).unwrap();
        assert_eq!(prompt.stdout.trim(), "0");
    }
}
