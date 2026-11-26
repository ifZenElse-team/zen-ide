use std::io::Write;
use std::process::{Command, Output, Stdio};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn zen_cli(args: &[&str], stdin: Option<&str>, language: &str) -> Run {
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zen-cli"))
        .args(args)
        .env("ZEN_DATA_DIR", data.path())
        .env("ZEN_LANGUAGE", language)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(text) = stdin {
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    }
    let Output { status, stdout, stderr } = child.wait_with_output().unwrap();
    Run {
        code: status.code().unwrap(),
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn en(args: &[&str]) -> Run {
    zen_cli(args, None, "en")
}

fn command_count() -> usize {
    zen_cli::registry().unwrap().commands().count()
}

#[test]
fn commands_lists_ids_and_titles() {
    let run = en(&["commands"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), command_count());
    let open = lines.iter().find(|line| line.starts_with("file.project.open ")).unwrap();
    assert!(open.ends_with("  Open…"), "{open}");
    assert!(lines.iter().any(|line| line.starts_with("toolbox.tools.list ")));
}

#[test]
fn commands_as_json() {
    let run = en(&["commands", "--json"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let json: serde_json::Value = serde_json::from_str(&run.stdout).unwrap();
    let commands = json["commands"].as_array().unwrap();
    assert_eq!(commands.len(), command_count());
    for command in commands {
        for field in ["id", "title", "description", "paramsSchema", "aliases"] {
            assert!(command.get(field).is_some(), "{field} missing in {command}");
        }
    }
    let open = commands.iter().find(|command| command["id"] == "file.project.open").unwrap();
    assert_eq!(open["paramsSchema"]["required"], serde_json::json!(["path"]));
}

#[test]
fn every_subcommand_has_help() {
    for args in [&["--help"][..], &["commands", "--help"], &["run", "--help"], &["run", "-h"]] {
        let run = en(args);
        assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{args:?}");
        assert!(run.stdout.contains("Usage: zen-cli"), "{args:?}: {}", run.stdout);
    }
    assert!(en(&["--help"]).stdout.contains("Lists and runs Zen Platform commands."));
    assert!(en(&["commands", "--help"]).stdout.contains("--json"));
    assert!(en(&["run", "--help"]).stdout.contains("--params <json>"));
}

#[test]
fn help_follows_the_language() {
    let run = zen_cli(&["run", "--help"], None, "ru");
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("Выполняет команду"), "{}", run.stdout);
}

#[test]
fn missing_subcommand_is_a_usage_error() {
    let run = en(&[]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("Usage: zen-cli"), "{}", run.stderr);
}

#[test]
fn run_opens_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let params = serde_json::json!({ "path": dir.path() }).to_string();
    let run = en(&["run", "file.project.open", "--params", &params]);
    assert_eq!((run.code, run.stdout.as_str(), run.stderr.as_str()), (0, "null\n", ""));
}

#[test]
fn run_prints_the_result_as_pretty_json() {
    let run = en(&["run", "editor.find.info"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let result: serde_json::Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(result["count"], 0);
    assert_eq!(run.stdout, format!("{}\n", serde_json::to_string_pretty(&result).unwrap()));
}

#[test]
fn version_is_printed() {
    let run = en(&["--version"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    assert_eq!(run.stdout, format!("zen-cli {}\n", zen_cli::VERSION));
    assert_eq!(zen_cli::VERSION, option_env!("ZEN_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")));
}

#[test]
fn run_reports_a_failed_command() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "text").unwrap();
    let params = serde_json::json!({ "path": file }).to_string();
    let run = en(&["run", "file.project.open", "--params", &params]);
    assert_eq!(run.code, 1);
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, format!("zen-cli: `file.project.open` failed: {} is not a folder.\n", file.display()));

    let run = zen_cli(&["run", "file.project.open", "--params", &params], None, "ru");
    assert_eq!(run.code, 1);
    assert_eq!(run.stderr, format!("zen-cli: `file.project.open` не выполнена: {} — не папка.\n", file.display()));
}

#[test]
fn run_rejects_an_unknown_command() {
    let run = en(&["run", "no.such.command"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, "zen-cli: unknown command `no.such.command`; zen-cli commands lists them\n");
}

#[test]
fn run_rejects_malformed_params() {
    let run = en(&["run", "file.project.open", "--params", "{\"path\":"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.starts_with("zen-cli: the parameters are not valid JSON: "), "{}", run.stderr);
    assert_eq!(run.stderr.lines().count(), 1);
}

#[test]
fn ide_lists_no_instances_and_says_none_runs() {
    let run = en(&["ide", "list", "--json"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let listed: serde_json::Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(listed, serde_json::json!({ "instances": [] }));
    let run = en(&["ide", "list"]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
    let run = en(&["ide", "call", "list_windows"]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.starts_with("zen-cli: no headless IDE is running"), "{}", run.stderr);
    let run = en(&["ide", "start", "/nonexistent/project"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    for args in [&["ide", "--help"][..], &["ide", "start", "--help"], &["ide", "call", "--help"]] {
        let run = en(args);
        assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{args:?}");
        assert!(run.stdout.contains("Usage: zen-cli ide"), "{args:?}: {}", run.stdout);
    }
}

#[test]
fn run_rejects_params_that_do_not_match_the_schema() {
    let run = en(&["run", "file.project.open", "--params", "{\"path\": 5}"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.starts_with("zen-cli: invalid parameters for `file.project.open`: "), "{}", run.stderr);
}

#[test]
fn run_reads_params_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let params = serde_json::json!({ "path": dir.path() }).to_string();
    let run = zen_cli(&["run", "file.project.open", "--params", "-"], Some(&params), "en");
    assert_eq!((run.code, run.stdout.as_str(), run.stderr.as_str()), (0, "null\n", ""));
}

#[test]
fn run_without_params_uses_an_empty_object() {
    let run = en(&["run", "file.project.open"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("missing field `path`"), "{}", run.stderr);
}

#[test]
fn bad_arguments_are_usage_errors() {
    let run = en(&["frobnicate"]);
    assert_eq!((run.code, run.stderr.as_str()), (2, "zen-cli: unknown subcommand `frobnicate`; see zen-cli --help\n"));
    let run = en(&["commands", "--yaml"]);
    assert_eq!((run.code, run.stderr.as_str()), (2, "zen-cli: unknown argument `--yaml`; see zen-cli --help\n"));
    let run = en(&["run", "file.project.open", "--params"]);
    assert_eq!((run.code, run.stderr.as_str()), (2, "zen-cli: invalid use of `--params <json>`; see zen-cli --help\n"));
    let run = en(&["commands", "--json", "--json"]);
    assert_eq!((run.code, run.stderr.as_str()), (2, "zen-cli: invalid use of `--json`; see zen-cli --help\n"));
}

fn shell_named(name: &str) -> Option<std::path::PathBuf> {
    ["/bin", "/usr/bin", "/opt/homebrew/bin"]
        .iter()
        .map(|dir| std::path::Path::new(dir).join(name))
        .find(|path| path.is_file())
}

fn shell_output(shell: &std::path::Path, script: &str) -> String {
    let data = tempfile::tempdir().unwrap();
    let output = Command::new(shell)
        .args(["-f", "-c", script].iter().skip(usize::from(!shell.ends_with("zsh"))))
        .env("ZEN_DATA_DIR", data.path())
        .env("ZEN_LANGUAGE", "en")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn the_bash_script_completes_through_the_cli() {
    let Some(bash) = shell_named("bash") else { return };
    let script = en(&["completions", "bash"]).stdout;
    let program = env!("CARGO_BIN_EXE_zen-cli");
    let check = format!(
        "{script}\nCOMP_WORDS=('{program}' run toolbox.shellScripts.st); COMP_CWORD=2; _zen; printf '%s\\n' \"${{COMPREPLY[@]}}\""
    );
    assert_eq!(shell_output(&bash, &check), "toolbox.shellScripts.status\n");
}

#[test]
fn the_zsh_script_completes_through_the_cli_and_falls_back_to_files() {
    let Some(zsh) = shell_named("zsh") else { return };
    let script = en(&["completions", "zsh"]).stdout;
    let program = env!("CARGO_BIN_EXE_zen-cli");
    let check = format!(
        "compdef() {{ :; }}\n_files() {{ print files; }}\n_describe() {{ print -l -- ${{(P)2}}; }}\n{script}\n\
         words=('{program}' run toolbox.shellScripts.st); CURRENT=3; _zen\n\
         words=('{program}' ide start ''); CURRENT=4; _zen"
    );
    assert_eq!(shell_output(&zsh, &check), "toolbox.shellScripts.status:Shell Scripts Status\nfiles\n");
}

#[test]
fn the_fish_script_is_printed_with_the_marker_toolbox_owns_it_by() {
    let run = en(&["completions", "fish"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("# Generated by Zen Toolbox. Do not edit."), "{}", run.stdout);
    assert!(run.stdout.contains("__complete fish"), "{}", run.stdout);
}

#[test]
fn an_unknown_shell_is_a_usage_error() {
    let run = en(&["completions", "tcsh"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.starts_with("zen-cli: "), "{}", run.stderr);
}
