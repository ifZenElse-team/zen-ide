use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

const PROGRAM: &str = env!("CARGO_BIN_EXE_jvmjizo");

fn jvmjizo(data: &Path) -> Command {
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
            id: "jvmjizo",
            file: ("App.java", "class App {\n    void run() {}\n}\n"),
        },
    );
}

fn fake_jdk(home: &Path) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/java"), "").unwrap();
    std::fs::write(home.join("bin/javac"), "").unwrap();
    std::fs::write(home.join("release"), "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.5\"\n").unwrap();
}

#[cfg(unix)]
#[test]
fn agents_see_and_choose_the_projects_jdk() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("pom.xml"), "<project/>").unwrap();
    let jdk = root.join("jdk-21");
    fake_jdk(&jdk);
    let mut ide = Ide(jvmjizo(&root.join("data"))
        .args(["--headless", "--mcp-port", "0"])
        .arg(&project)
        .env("JAVA_HOME", &jdk)
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
    let current = call("toolchain.jdk.project", json!({}));
    assert_eq!(current["jdk"]["home"], jdk.display().to_string(), "{current}");
    assert_eq!((current["jdk"]["source"].clone(), current["languageLevel"].clone()), (json!("javaHome"), json!(21)));
    let set = call("toolchain.jdk.languageLevel", json!({ "level": "17" }));
    assert_eq!(set["project"]["languageLevel"], 17, "{set}");
    let form = call("toolchain.jdk.settings", json!({}));
    assert!(form.to_string().contains("Project SDK"), "{form}");
    let _ = zen_automation::instances::call(&url, "exit_ide", json!({}));
}
