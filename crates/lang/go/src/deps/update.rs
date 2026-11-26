use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use zen_i18n::t;
use zen_project::inspect::{FixJob, FixNotice, FixState};
use zen_run::Programs;

use super::releases;
use crate::gomod::{GoMod, parse_go_mod};

const OUTPUT_TAIL: usize = 4000;

pub struct UpdateJob {
    state: Arc<Mutex<FixState>>,
    output: Arc<Mutex<Vec<String>>>,
    title: String,
    programs: Programs,
    notice: Arc<Mutex<Option<FixNotice>>>,
}

impl FixJob for UpdateJob {
    fn poll(&mut self) -> FixState {
        self.state.lock().map(|state| state.clone()).unwrap_or(FixState::Failed { output: String::new() })
    }

    fn title(&self) -> Option<String> {
        Some(self.title.clone())
    }

    fn drain_output(&mut self) -> Vec<String> {
        self.output.lock().map(|mut output| std::mem::take(&mut *output)).unwrap_or_default()
    }

    fn programs(&self) -> Option<Arc<dyn zen_project::inspect::Programs>> {
        Some(Arc::new(self.programs.clone()))
    }

    fn notice(&mut self) -> Option<FixNotice> {
        self.notice.lock().ok().and_then(|notice| notice.clone())
    }
}

pub fn command_lines(targets: &[(String, String)], tidy: bool) -> Vec<Vec<String>> {
    edit_lines(&[], targets, tidy)
}

pub fn edit_lines(edits: &[String], targets: &[(String, String)], tidy: bool) -> Vec<Vec<String>> {
    let mut lines = Vec::new();
    if !edits.is_empty() {
        let mut edit = vec!["mod".to_owned(), "edit".to_owned()];
        edit.extend(edits.iter().cloned());
        lines.push(edit);
    }
    if !targets.is_empty() {
        let mut get = vec!["get".to_owned()];
        get.extend(targets.iter().map(|(module, version)| format!("{module}@{version}")));
        lines.push(get);
    }
    if tidy {
        lines.push(vec!["mod".to_owned(), "tidy".to_owned()]);
    }
    lines
}

pub fn title_of(lines: &[Vec<String>]) -> String {
    let arg = |line: &Vec<String>, at: usize| line.get(at).map(String::as_str).unwrap_or_default().to_owned();
    if let Some(get) = lines.iter().find(|line| arg(line, 0) == "get") {
        return t!("go-deps-task-update", count = get.len() - 1);
    }
    match lines.first() {
        Some(line) if arg(line, 1) == "edit" => t!("go-deps-task-edit", file = format!("{}.{}", "go", arg(line, 0))),
        Some(line) => t!("go-deps-task-run", command = format!("go {}", line.join(" "))),
        None => String::new(),
    }
}

pub fn start(
    tool: &Path,
    env: &[(String, String)],
    flags: &str,
    go_mod: &Path,
    lines: Vec<Vec<String>>,
    wake: Option<zen_project::inspect::Wake>,
) -> Result<UpdateJob, String> {
    if tool.as_os_str().is_empty() {
        return Err(t!("go-deps-no-go"));
    }
    if lines.is_empty() {
        return Err(t!("go-deps-fix-nothing"));
    }
    let title = title_of(&lines);
    let programs = Programs::default();
    let state = Arc::new(Mutex::new(FixState::Running { line: None }));
    let output = Arc::new(Mutex::new(Vec::new()));
    let (shared, printed) = (Arc::clone(&state), Arc::clone(&output));
    let go = Go { tool: tool.to_owned(), env: env.to_vec(), flags: flags.to_owned(), programs: programs.clone() };
    let dir = go_mod.parent().map(Path::to_owned).unwrap_or_default();
    let changed: Vec<PathBuf> = vec![go_mod.to_owned(), dir.join("go.sum")];
    let notice = Arc::new(Mutex::new(None));
    let told = Arc::clone(&notice);
    let module = go_mod.file_name().is_some_and(|name| name == "go.mod").then(|| go_mod.to_owned());
    std::thread::Builder::new()
        .name("go-get".to_owned())
        .spawn(move || {
            let saved: Vec<(PathBuf, Option<Vec<u8>>)> =
                changed.iter().map(|path| (path.clone(), std::fs::read(path).ok())).collect();
            let mut result = FixState::Done { changed };
            let mut before = None;
            for args in lines {
                if before.is_none() && args.get(1).is_none_or(|arg| arg != "edit") {
                    before = module.as_deref().and_then(read_go_mod);
                }
                if let Err(output) = run(&go, &dir, &args, &shared, &printed) {
                    result = FixState::Failed { output };
                    break;
                }
            }
            if go.programs.stopping() {
                restore(&saved);
            }
            if matches!(result, FixState::Done { .. })
                && let (Some(before), Some(after)) = (before, module.as_deref().and_then(read_go_mod))
                && let Some(raised) = raise_notice(&before, &after, || module_cache(&go, &dir))
            {
                if let Ok(mut printed) = printed.lock() {
                    printed.push(raised.text.clone());
                }
                if let Ok(mut notice) = told.lock() {
                    *notice = Some(raised);
                }
            }
            if let Ok(mut state) = shared.lock() {
                *state = result;
            }
            if let Some(wake) = wake {
                wake();
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(UpdateJob { state, output, title, programs, notice })
}

fn restore(saved: &[(PathBuf, Option<Vec<u8>>)]) {
    for (path, bytes) in saved {
        match bytes {
            Some(bytes) if std::fs::read(path).is_ok_and(|now| now == *bytes) => {}
            Some(bytes) => {
                let beside = path.with_extension("zen-restore");
                if std::fs::write(&beside, bytes).is_err() || std::fs::rename(&beside, path).is_err() {
                    let _ = std::fs::remove_file(&beside);
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn read_go_mod(path: &Path) -> Option<GoMod> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(parse_go_mod(path, &text))
}

fn module_cache(go: &Go, dir: &Path) -> Option<PathBuf> {
    if let Some((_, cache)) = go.env.iter().find(|(key, value)| key == "GOMODCACHE" && !value.is_empty()) {
        return Some(PathBuf::from(cache));
    }
    let mut command = Command::new(&go.tool);
    command.args(["env", "GOMODCACHE"]).current_dir(dir).envs(go.env.iter().map(|(key, value)| (key, value)));
    command.env("GOTOOLCHAIN", "local");
    let output = zen_run::capture(&mut command, std::time::Duration::from_secs(60)).ok()?;
    let cache = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !cache.is_empty()).then(|| PathBuf::from(cache))
}

fn escape(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push(c.to_ascii_lowercase());
        } else {
            escaped.push(c);
        }
    }
    escaped
}

fn required_go(cache: &Path, module: &str, version: &str) -> Option<String> {
    let path =
        cache.join("cache").join("download").join(escape(module)).join("@v").join(format!("{}.mod", escape(version)));
    read_go_mod(&path)?.go
}

pub fn raise_notice(before: &GoMod, after: &GoMod, cache: impl FnOnce() -> Option<PathBuf>) -> Option<FixNotice> {
    let (Some(from), Some(to)) = (before.go.as_deref(), after.go.as_deref()) else { return None };
    if releases::compare(to, from).is_le() {
        return None;
    }
    let cache = cache();
    let requiring: Vec<String> = after
        .requires
        .iter()
        .filter(|requirement| {
            !before.requires.iter().any(|old| old.path == requirement.path && old.version == requirement.version)
        })
        .filter(|requirement| {
            cache
                .as_deref()
                .and_then(|cache| required_go(cache, &requirement.path, &requirement.version))
                .is_some_and(|go| releases::compare(&go, to).is_ge())
        })
        .map(|requirement| format!("{} {}", requirement.path, requirement.version))
        .collect();
    let text = match requiring.split_first() {
        Some((first, more)) => {
            t!("go-deps-go-raised-by", from = from, to = to, module = first.as_str(), more = more.len())
        }
        None => t!("go-deps-go-raised", from = from, to = to),
    };
    Some(FixNotice { title: t!("go-deps-go-raised-title", to = to), text })
}

struct Go {
    tool: PathBuf,
    env: Vec<(String, String)>,
    flags: String,
    programs: Programs,
}

fn run(
    go: &Go,
    dir: &Path,
    args: &[String],
    state: &Arc<Mutex<FixState>>,
    printed: &Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    let print = |line: String| {
        if let Ok(mut printed) = printed.lock() {
            printed.push(line);
        }
    };
    print(format!("$ go {}", args.join(" ")));
    let Go { tool, env, flags, programs } = go;
    let mut command = Command::new(tool);
    command.args(args).current_dir(dir).envs(env.iter().map(|(key, value)| (key, value)));
    if !flags.is_empty() {
        command.env("GOFLAGS", flags);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = programs.spawn(&mut command).map_err(|error| format!("go {}: {error}", args.join(" ")))?;
    let output = Arc::new(Mutex::new(String::new()));
    let follow = |stream: Option<Box<dyn Read + Send>>| {
        let (state, printed, output) = (Arc::clone(state), Arc::clone(printed), Arc::clone(&output));
        std::thread::spawn(move || {
            let Some(stream) = stream else { return };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if let Ok(mut state) = state.lock() {
                    *state = FixState::Running { line: Some(line.clone()) };
                }
                if let Ok(mut output) = output.lock() {
                    output.push_str(&line);
                    output.push('\n');
                }
                if let Ok(mut printed) = printed.lock() {
                    printed.push(line);
                }
            }
        })
    };
    let readers = [
        follow(child.stdout.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>)),
        follow(child.stderr.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>)),
    ];
    let status = child.wait().map_err(|error| error.to_string());
    zen_run::join_within(readers, std::time::Instant::now() + zen_run::PIPE_GRACE);
    let output = output.lock().map(|output| output.clone()).unwrap_or_default();
    programs.remove(child.id());
    let status = status?;
    if status.success() {
        return Ok(());
    }
    let mut tail = format!("go {}\n{}", args.join(" "), output.trim_end());
    if tail.len() > OUTPUT_TAIL {
        let mut cut = tail.len() - OUTPUT_TAIL;
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail = format!("…{}", &tail[cut..]);
    }
    Err(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_get_names_every_module_and_tidy_comes_after() {
        let targets = vec![
            ("golang.org/x/crypto".to_owned(), "v0.31.0".to_owned()),
            ("a.example/b".to_owned(), "v1.2.0".to_owned()),
        ];
        assert_eq!(command_lines(&targets, false), [vec!["get", "golang.org/x/crypto@v0.31.0", "a.example/b@v1.2.0"]]);
        assert_eq!(
            command_lines(&targets[..1], true),
            [vec!["get", "golang.org/x/crypto@v0.31.0"], vec!["mod", "tidy"]]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_cancelled_update_puts_go_mod_and_go_sum_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let started = dir.path().join("started");
        std::fs::write(
            &go,
            format!(
                "#!/bin/sh\nif [ \"$1\" = mod ]; then echo edited >> go.mod; exit 0; fi\n\
                 echo half > go.sum\ntouch '{}'\nsleep 60\n",
                started.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        let go_mod = dir.path().join("go.mod");
        std::fs::write(&go_mod, "module m\n").unwrap();
        let targets = [("a.example/b".to_owned(), "v1.2.0".to_owned())];
        let lines = edit_lines(&["-go=1.24".to_owned()], &targets, false);
        let mut job = start(&go, &[], "", &go_mod, lines, None).unwrap();
        while !started.exists() {
            std::thread::yield_now();
        }
        job.programs().unwrap().kill();
        while matches!(job.poll(), FixState::Running { .. }) {
            std::thread::yield_now();
        }
        assert_eq!(std::fs::read_to_string(&go_mod).unwrap(), "module m\n");
        assert!(!dir.path().join("go.sum").exists(), "the file it created is gone");
    }

    #[cfg(unix)]
    #[test]
    fn the_job_runs_the_tool_and_reports_its_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let log = dir.path().join("log");
        std::fs::write(
            &go,
            format!(
                "#!/bin/sh\necho \"$@ $GOFLAGS\" >> '{}'\nif [ \"$1\" = mod ]; then echo 'go: tidy broke' >&2; exit 1; fi\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        let go_mod = dir.path().join("go.mod");
        std::fs::write(&go_mod, "module m\n").unwrap();
        let wait = |mut job: UpdateJob| loop {
            match job.poll() {
                FixState::Running { .. } => std::thread::sleep(std::time::Duration::from_millis(10)),
                done => return done,
            }
        };
        let lines = command_lines(&[("a.example/b".to_owned(), "v1.2.0".to_owned())], false);
        let done = wait(start(&go, &[], "-mod=mod", &go_mod, lines, None).unwrap());
        assert_eq!(done, FixState::Done { changed: vec![go_mod.clone(), dir.path().join("go.sum")] });
        let lines = command_lines(&[("a.example/b".to_owned(), "v1.2.0".to_owned())], true);
        let FixState::Failed { output } = wait(start(&go, &[], "", &go_mod, lines, None).unwrap()) else { panic!() };
        assert!(output.contains("go mod tidy") && output.contains("tidy broke"), "{output}");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "get a.example/b@v1.2.0 -mod=mod\nget a.example/b@v1.2.0 \nmod tidy \n"
        );
        assert!(start(Path::new(""), &[], "", &go_mod, Vec::new(), None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn stopping_the_programs_ends_a_hanging_update() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let pids = dir.path().join("pids");
        let script = format!("#!/bin/sh\nsleep 60 &\necho \"$$ $!\" >> '{}'\nwait\n", pids.display());
        std::fs::write(&go, script).unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        let go_mod = dir.path().join("go.mod");
        std::fs::write(&go_mod, "module m\n").unwrap();
        let lines = command_lines(&[("a.example/b".to_owned(), "v1.2.0".to_owned())], true);
        let mut job = start(&go, &[], "", &go_mod, lines, None).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let started = loop {
            if let Some(line) = std::fs::read_to_string(&pids).ok().filter(|line| line.ends_with('\n')) {
                break line.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
            }
            assert!(std::time::Instant::now() < deadline, "go did not start");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let programs = job.programs().expect("an update has programs");
        assert!(programs.running());
        programs.interrupt();
        std::thread::sleep(std::time::Duration::from_millis(300));
        programs.kill();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while programs.running() || matches!(job.poll(), FixState::Running { .. }) {
            assert!(std::time::Instant::now() < deadline, "the update outlives its stop");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(job.poll(), FixState::Failed { .. }));
        for pid in &started {
            let alive = Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status();
            assert!(!alive.unwrap().success(), "{pid} of {started:?} still runs");
        }
        assert_eq!(std::fs::read_to_string(&pids).unwrap().lines().count(), 1, "go mod tidy did not start");
    }
}
