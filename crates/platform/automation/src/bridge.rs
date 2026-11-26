use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::instances::{self, Lease};

const START_TIMEOUT: Duration = Duration::from_secs(30);

const RESTART_GRACE: Duration = Duration::from_secs(15);

const EXIT_ALREADY_RUNNING: i32 = 3;

pub fn recorded_url(file: &Path) -> Option<String> {
    let bytes = std::fs::read(file).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("url").and_then(Value::as_str).map(str::to_owned)
}

fn alive(url: &str) -> bool {
    let ping = json!({ "jsonrpc": "2.0", "id": "bridge-ping", "method": "ping" });
    ureq::post(url)
        .header("Content-Type", "application/json")
        .config()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .send(ping.to_string())
        .is_ok()
}

fn back_within(file: &Path, grace: Duration) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(url) = recorded_url(file).filter(|url| alive(url)) {
            return Some(url);
        }
        if started.elapsed() >= grace {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn connect(file: &Path, lost: bool, launch: &mut dyn FnMut() -> Result<(), String>) -> Result<String, String> {
    let grace = if lost { RESTART_GRACE } else { Duration::ZERO };
    if let Some(url) = back_within(file, grace) {
        return Ok(url);
    }
    launch()?;
    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if let Some(url) = recorded_url(file).filter(|url| alive(url)) {
            return Ok(url);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("the IDE did not start its MCP server; turn it on in Settings → Tools → MCP Server".to_owned())
}

pub fn run(file: &Path, mut launch: impl FnMut() -> Result<(), String>) -> Result<(), String> {
    serve(|lost| connect(file, lost, &mut launch))
}

struct Held {
    instance: instances::Instance,
    _lease: Lease,
}

pub fn run_headless(
    data: &Path,
    project: &Path,
    mut start: impl FnMut(&Path) -> Result<Child, String>,
) -> Result<(), String> {
    let project = std::fs::canonicalize(project).map_err(|error| format!("{}: {error}", project.display()))?;
    exit_with_parent();
    let mut held: Option<Held> = None;
    serve(move |failed| {
        if let Some(current) = &held
            && !failed
            && instances::file(data, current.instance.pid).exists()
        {
            return Ok(current.instance.url.clone());
        }
        held = None;
        let found = attach(data, &project, &mut start)?;
        let url = found.instance.url.clone();
        held = Some(found);
        Ok(url)
    })
}

fn attach(data: &Path, project: &Path, start: &mut dyn FnMut(&Path) -> Result<Child, String>) -> Result<Held, String> {
    std::fs::create_dir_all(instances::folder(data)).map_err(|error| error.to_string())?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(instances::folder(data).join("start.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let held = find_or_start(data, project, start);
    let _ = lock.unlock();
    held
}

fn find_or_start(
    data: &Path,
    project: &Path,
    start: &mut dyn FnMut(&Path) -> Result<Child, String>,
) -> Result<Held, String> {
    for _ in 0..3 {
        if let Some(instance) = instances::with_project(data, project) {
            let lease = Lease::take(data, instance.pid).map_err(|error| error.to_string())?;
            if instances::file(data, instance.pid).exists() {
                return Ok(Held { instance, _lease: lease });
            }
            continue;
        }
        let mut child = start(project)?;
        let pid = child.id();
        let started = Instant::now();
        loop {
            if let Some(instance) = instances::list(data).into_iter().find(|instance| instance.pid == pid) {
                let lease = Lease::take(data, pid).map_err(|error| error.to_string())?;
                drop(child);
                return Ok(Held { instance, _lease: lease });
            }
            match child.try_wait() {
                Ok(Some(status)) if status.code() == Some(EXIT_ALREADY_RUNNING) => break,
                Ok(Some(status)) => return Err(format!("the headless IDE ended at start ({status})")),
                _ => {}
            }
            if started.elapsed() > START_TIMEOUT {
                let _ = child.kill();
                return Err("the headless IDE did not start its MCP server".to_owned());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Err(format!("cannot start a headless IDE for {}", project.display()))
}

fn exit_with_parent() {
    #[cfg(unix)]
    {
        let parent = std::os::unix::process::parent_id();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if std::os::unix::process::parent_id() != parent {
                    std::process::exit(0);
                }
            }
        });
    }
}

fn serve(connect: impl FnMut(bool) -> Result<String, String>) -> Result<(), String> {
    serve_lines(std::io::stdin().lock(), std::io::stdout(), connect)
}

fn serve_lines(
    input: impl BufRead,
    mut stdout: impl Write,
    mut connect: impl FnMut(bool) -> Result<String, String>,
) -> Result<(), String> {
    let mut url: Option<String> = None;
    for line in input.lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let id = serde_json::from_str::<Value>(&line).ok().and_then(|message| message.get("id").cloned());
        let mut answer = Err(String::new());
        for attempt in 0..2 {
            let target = match &url {
                Some(url) if attempt == 0 => Ok(url.clone()),
                _ => connect(attempt > 0).inspect(|found| url = Some(found.clone())),
            };
            answer = target.and_then(|target| send(&target, &line));
            if answer.is_ok() {
                break;
            }
            url = None;
        }
        let reply = match answer {
            Ok(Some(body)) => Some(body),
            Ok(None) => None,
            Err(message) => id.map(|id| {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": message } }).to_string()
            }),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{}", reply.trim()).and_then(|()| stdout.flush()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn send(target: &str, line: &str) -> Result<Option<String>, String> {
    let response = ureq::post(target)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .config()
        .http_status_as_error(false)
        .build()
        .send(line);
    match response {
        Ok(response) if response.status().as_u16() == 202 => Ok(None),
        Ok(mut response) => response.body_mut().read_to_string().map(Some).map_err(|error| error.to_string()),
        Err(error) => Err(format!("cannot reach the IDE: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};

    use super::*;
    use crate::{Server, ServerInfo, Tool};

    fn ide(name: &str) -> Server {
        let tools = vec![Tool {
            name: "who".to_owned(),
            description: "Says which IDE answers.".to_owned(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
        }];
        let info = ServerInfo { name: "Test".to_owned(), version: "1.0".to_owned(), instructions: String::new() };
        let (server, calls) = Server::start(0, info, tools, || {}).unwrap();
        let name = name.to_owned();
        std::thread::spawn(move || {
            for call in calls {
                call.reply(Ok(json!({ "ide": name })));
            }
        });
        server
    }

    fn call(id: u64) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "who", "arguments": {} } })
            .to_string()
    }

    fn answered(output: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(output).lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    #[test]
    fn the_same_agent_keeps_its_tools_across_an_ide_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("automation.json");
        let first = ide("first");
        std::fs::write(&file, json!({ "url": first.url() }).to_string()).unwrap();
        let (reader, mut writer) = std::io::pipe().unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let (bridge_file, bridge_output) = (file.clone(), Arc::clone(&output));
        let bridge = std::thread::spawn(move || {
            struct Shared(Arc<Mutex<Vec<u8>>>);
            impl Write for Shared {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().write(bytes)
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let launched = |_: bool| connect(&bridge_file, true, &mut || Err("no IDE to start".to_owned()));
            serve_lines(std::io::BufReader::new(reader), Shared(bridge_output), launched)
        });
        let wait_for = |count: usize| {
            let started = Instant::now();
            while answered(&output.lock().unwrap()).len() < count {
                assert!(started.elapsed() < Duration::from_secs(30), "no answer");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        writeln!(writer, "{}", call(1)).unwrap();
        wait_for(1);
        drop(first);
        let restarted = std::thread::spawn({
            let file = file.clone();
            move || {
                std::thread::sleep(Duration::from_secs(1));
                let second = ide("second");
                std::fs::write(&file, json!({ "url": second.url() }).to_string()).unwrap();
                second
            }
        });
        writeln!(writer, "{}", call(2)).unwrap();
        wait_for(2);
        let second = restarted.join().unwrap();
        let answers = answered(&output.lock().unwrap());
        assert_eq!(answers[0]["result"]["content"][0]["text"], "{\"ide\":\"first\"}", "{answers:?}");
        assert_eq!(
            answers[1]["result"]["content"][0]["text"], "{\"ide\":\"second\"}",
            "the call waited for the new IDE"
        );
        drop(second);
        drop(writer);
        bridge.join().unwrap().unwrap();
    }

    #[test]
    fn with_no_ide_a_call_fails_after_the_wait_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("automation.json");
        let input = format!("{}\n", call(7));
        let mut output = Vec::new();
        let started = Instant::now();
        serve_lines(input.as_bytes(), &mut output, |lost| {
            connect(&file, lost, &mut || Err("the IDE is not installed".to_owned()))
        })
        .unwrap();
        let answers = answered(&output);
        assert_eq!(answers[0]["id"], 7);
        assert_eq!(answers[0]["error"]["message"], "the IDE is not installed");
        assert!(started.elapsed() < RESTART_GRACE + Duration::from_secs(5), "never forever");
    }
}
