use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::ArgMatches;
use serde_json::{Value, json};
use zen_automation::instances::{self, Instance};
use zen_i18n::t;

use crate::{EXIT_FAILURE, EXIT_USAGE};

const WAIT: Duration = Duration::from_secs(60);

fn data_root() -> Option<PathBuf> {
    zen_workbench::recent::default_file("zen").and_then(|file| Some(file.parent()?.parent()?.to_owned()))
}

fn all_instances() -> Vec<(PathBuf, Instance)> {
    let Some(root) = data_root() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut found: Vec<(PathBuf, Instance)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|folder| instances::folder(folder).is_dir())
        .flat_map(|folder| instances::list(&folder).into_iter().map(move |instance| (folder.clone(), instance)))
        .collect();
    found.sort_by_key(|(_, instance)| (instance.started, instance.pid));
    found
}

fn find(target: Option<&str>) -> Result<Instance, (String, u8)> {
    let all: Vec<Instance> = all_instances().into_iter().map(|(_, instance)| instance).collect();
    let found = match target {
        None if all.len() == 1 => all.into_iter().next(),
        None if all.is_empty() => return Err((t!("cli-ide-none"), EXIT_FAILURE)),
        None => return Err((t!("cli-ide-several"), EXIT_USAGE)),
        Some(target) => match target.parse::<u32>() {
            Ok(pid) => all.into_iter().find(|instance| instance.pid == pid),
            Err(_) => {
                let path = std::fs::canonicalize(target).unwrap_or_else(|_| PathBuf::from(target));
                all.into_iter().find(|instance| instance.projects.iter().any(|project| path.starts_with(project)))
            }
        },
    };
    found.ok_or_else(|| (t!("cli-ide-not-found", target = target.unwrap_or_default()), EXIT_FAILURE))
}

fn describe(instance: &Instance) -> Value {
    serde_json::to_value(instance).unwrap_or(Value::Null)
}

pub(crate) fn run(matches: &ArgMatches, stdin: &mut impl Read, stdout: &mut impl Write) -> Result<Value, (String, u8)> {
    match matches.subcommand() {
        Some(("list", matches)) => {
            let list: Vec<Value> = all_instances().iter().map(|(_, instance)| describe(instance)).collect();
            if matches.get_flag("json") {
                return Ok(json!({ "instances": list }));
            }
            for (_, instance) in all_instances() {
                let projects: Vec<String> =
                    instance.projects.iter().map(|project| project.display().to_string()).collect();
                let line = format!("{}  {}  {}  {}", instance.pid, instance.product, instance.url, projects.join(", "));
                let _ = writeln!(stdout, "{}", line.trim_end());
            }
            Ok(Value::Null)
        }
        Some(("start", matches)) => {
            let project = matches.get_one::<String>("project").map_or("", String::as_str);
            let program = matches.get_one::<String>("ide").map_or("gogarden", String::as_str);
            let port = matches.get_one::<String>("mcp-port").cloned();
            start(Path::new(project), program, port.as_deref())
        }
        Some(("stop", matches)) => {
            let instance = find(matches.get_one::<String>("instance").map(String::as_str))?;
            instances::call(&instance.url, "exit_ide", json!({ "confirmed": true }))
                .map_err(|error| (t!("cli-ide-call-failed", error = error), EXIT_FAILURE))?;
            let started = Instant::now();
            while instances::alive(&instance.url) {
                if started.elapsed() > WAIT {
                    return Err((t!("cli-ide-did-not-stop", pid = instance.pid.to_string()), EXIT_FAILURE));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(json!({ "stopped": instance.pid }))
        }
        Some(("call", matches)) => {
            let instance = find(matches.get_one::<String>("instance").map(String::as_str))?;
            let tool = matches.get_one::<String>("tool").map_or("", String::as_str);
            let mut text = matches.get_one::<String>("args").cloned().unwrap_or_default().into_bytes();
            if text == b"-" {
                text.clear();
                stdin
                    .read_to_end(&mut text)
                    .map_err(|error| (t!("cli-error-stdin", error = error.to_string()), EXIT_FAILURE))?;
            }
            let arguments: Value = serde_json::from_slice(&text)
                .map_err(|error| (t!("cli-error-invalid-json", error = error.to_string()), EXIT_USAGE))?;
            instances::call(&instance.url, tool, arguments)
                .map_err(|error| (t!("cli-ide-call-failed", error = error), EXIT_FAILURE))
        }
        _ => Err((t!("cli-error-usage"), EXIT_USAGE)),
    }
}

fn start(project: &Path, program: &str, port: Option<&str>) -> Result<Value, (String, u8)> {
    let real = std::fs::canonicalize(project).map_err(|error| {
        (t!("cli-ide-no-project", path = project.display().to_string(), error = error.to_string()), EXIT_USAGE)
    })?;
    if let Some((_, running)) = all_instances().into_iter().find(|(_, instance)| instance.projects.contains(&real)) {
        let mut answer = describe(&running);
        answer["reused"] = json!(true);
        return Ok(answer);
    }
    let executable = executable(program).ok_or_else(|| (t!("cli-ide-no-program", program = program), EXIT_FAILURE))?;
    let mut command = Command::new(&executable);
    command.arg("--headless").arg(&real);
    if let Some(port) = port {
        command.args(["--mcp-port", port]);
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let log = std::env::temp_dir().join(format!("zen-ide-{}-{stamp}.log", std::process::id()));
    let log_file = std::fs::File::create(&log)
        .map_err(|error| (t!("cli-ide-cannot-start", program = program, error = error.to_string()), EXIT_FAILURE))?;
    let mut child =
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log_file).spawn().map_err(|error| {
            (t!("cli-ide-cannot-start", program = program, error = error.to_string()), EXIT_FAILURE)
        })?;
    let pid = child.id();
    let started = Instant::now();
    loop {
        if let Some((_, instance)) = all_instances().into_iter().find(|(_, instance)| instance.pid == pid) {
            let mut answer = describe(&instance);
            answer["reused"] = json!(false);
            answer["log"] = json!(log);
            return Ok(answer);
        }
        if let Ok(Some(status)) = child.try_wait() {
            let error = std::fs::read_to_string(&log).unwrap_or_default().trim().to_owned();
            let _ = std::fs::remove_file(&log);
            let error = if error.is_empty() { status.to_string() } else { error };
            return Err((t!("cli-ide-cannot-start", program = program, error = error), EXIT_FAILURE));
        }
        if started.elapsed() > WAIT {
            let _ = child.kill();
            return Err((t!("cli-ide-start-timeout", program = program), EXIT_FAILURE));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn executable(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_owned());
    }
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_file());
    if on_path.is_some() {
        return on_path;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [Some(PathBuf::from("/Applications")), home.map(|home| home.join("Applications"))]
        .into_iter()
        .flatten()
        .filter_map(|folder| std::fs::read_dir(folder).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>())
        .filter(|app| app.extension().is_some_and(|extension| extension == "app"))
        .map(|app| app.join("Contents/MacOS").join(program))
        .find(|candidate| candidate.is_file())
}
