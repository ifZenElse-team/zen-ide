use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

const PROGRAM: &str = env!("CARGO_BIN_EXE_devopsdaruma");

fn devopsdaruma(data: &Path) -> Command {
    let mut command = Command::new(PROGRAM);
    command
        .env("ZEN_DATA_DIR", data)
        .env("ZEN_HANDOFF_ONLY", "1")
        .env("ZEN_PLUGINS_CATALOG", "none")
        .env("ZEN_RELEASES", "none")
        .env_remove("ZEN_AUTOMATION");
    command
}

struct Ide(Child);

impl Drop for Ide {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(unix)]
#[test]
fn a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide() {
    zen_testkit::handoff::a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide(
        &zen_testkit::handoff::Case {
            program: PROGRAM,
            id: "devopsdaruma",
            file: ("Dockerfile", "FROM scratch\nCOPY app /app\nENTRYPOINT [\"/app\"]\n"),
        },
    );
}

#[cfg(unix)]
#[test]
fn it_answers_agents_as_devopsdaruma_with_databases_on_and_go_and_rust_off() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("infra");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("compose.yaml"), "services:\n  web:\n    image: nginx\n").unwrap();
    let mut ide = Ide(devopsdaruma(&root.join("data"))
        .args(["--headless", "--mcp-port", "0"])
        .arg(&project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap());
    let mut line = String::new();
    BufReader::new(ide.0.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let url = serde_json::from_str::<Value>(&line).unwrap()["url"].as_str().unwrap().to_owned();
    let call = |command: &str, params: Value| {
        zen_automation::instances::call(&url, "dispatch_command", json!({ "command": command, "params": params }))
            .unwrap()
    };
    let modules = call("plugins.modules.list", json!({}));
    let text = modules.to_string();
    assert_eq!(modules["product"], "DevopsDaruma", "{text}");
    for (id, enabled) in [("go", false), ("rust", false), ("database", true)] {
        let on = modules["modules"].as_array().and_then(|all| all.iter().find(|module| module["id"] == id));
        assert_eq!(on.map(|module| module["enabled"].clone()), Some(json!(enabled)), "{id}: {text}");
    }
    let listed = call("db.dataSource.list", json!({}));
    assert!(listed["connections"].is_array(), "agents reach the Database workspace: {listed}");
    let _ = zen_automation::instances::call(&url, "exit_ide", json!({}));
}
