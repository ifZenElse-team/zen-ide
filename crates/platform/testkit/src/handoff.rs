use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const CEILING: Duration = Duration::from_secs(60);

pub struct Case<'a> {
    pub program: &'a str,
    pub id: &'a str,
    pub file: (&'a str, &'a str),
}

fn program(case: &Case<'_>, data: &Path) -> Command {
    let mut command = Command::new(case.program);
    command
        .env("ZEN_DATA_DIR", data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .env("ZEN_PLUGINS_CATALOG", "none")
        .env("ZEN_RELEASES", "none")
        .env_remove("ZEN_AUTOMATION");
    command
}

struct Ide {
    child: Child,
    stderr: PathBuf,
}

impl Drop for Ide {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Ide {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

pub fn a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide(case: &Case<'_>) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("data");
    for folder in ["first", "app/src", "other"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    std::fs::write(root.join("app/src").join(case.file.0), case.file.1).unwrap();
    let stderr = root.join("ide.log");
    let mut ide = Ide {
        child: program(case, &data)
            .args(["--headless", "--accept-open", "--mcp-port", "0"])
            .arg(root.join("first"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
        stderr,
    };
    let mut stdout = BufReader::new(ide.child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    std::thread::spawn(move || {
        let mut rest = String::new();
        while stdout.read_line(&mut rest).is_ok_and(|read| read > 0) {
            rest.clear();
        }
    });
    let url = serde_json::from_str::<Value>(&line).unwrap_or_else(|_| panic!("{line}\n{}", ide.log()))["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let call = |tool: &str, arguments: Value| zen_automation::instances::call(&url, tool, arguments);

    let bin = root.join("bin");
    zen_update::scripts::write_launcher(&bin, case.id, Path::new(case.program), zen_update::scripts::Shell::Posix)
        .unwrap();
    let second = Command::new(bin.join(case.id))
        .args([format!("src/{}:2:5", case.file.0).as_str(), "../other"])
        .current_dir(root.join("app"))
        .env("ZEN_DATA_DIR", &data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "the second start was not taken: {}\n{}\nthe running IDE: alive {}\n{}",
        second.status,
        String::from_utf8_lossy(&second.stderr),
        ide.child.try_wait().ok().flatten().is_none(),
        ide.log()
    );

    let started = Instant::now();
    let windows = loop {
        let windows = call("list_windows", json!({})).unwrap();
        if windows["windows"].as_array().is_some_and(|windows| windows.len() == 3) {
            break windows;
        }
        assert!(started.elapsed() < CEILING, "{windows}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let projects: Vec<PathBuf> = windows["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| PathBuf::from(window["project"].as_str().unwrap()))
        .collect();
    assert_eq!(projects, vec![root.join("first"), root.join("app/src"), root.join("other")], "{windows}");
    let carets = call(
        "dispatch_command",
        json!({ "command": "editor.selection.get", "project": root.join("app/src"), "params": {} }),
    )
    .unwrap();
    assert_eq!((carets[0]["line"].clone(), carets[0]["column"].clone()), (json!(1), json!(4)), "{carets}");

    let mut waiting = Command::new(bin.join(case.id))
        .args(["--wait", format!("src/{}:1", case.file.0).as_str()])
        .current_dir(root.join("app"))
        .env("ZEN_DATA_DIR", &data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .spawn()
        .unwrap();
    let started = Instant::now();
    while call(
        "dispatch_command",
        json!({ "command": "editor.selection.get", "project": root.join("app/src"), "params": {} }),
    )
    .unwrap()[0]["line"]
        != json!(0)
    {
        assert!(started.elapsed() < CEILING);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(waiting.try_wait().unwrap(), None, "--wait holds the start while the tab is open");
    call(
        "dispatch_command",
        json!({ "command": "file.document.close", "project": root.join("app/src"), "params": {} }),
    )
    .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = waiting.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < CEILING, "--wait ends with the tab\n{}", ide.log());
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success());

    let _ = call("exit_ide", json!({}));
    let started = Instant::now();
    while ide.child.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < CEILING, "the headless IDE exits");
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = program(case, &data).arg(root.join("other")).output().unwrap();
    assert_eq!(output.status.code(), Some(1), "nobody takes it now, and no window opens");
}
