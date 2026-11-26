use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use zen_i18n::t;

use super::headless::{EXIT_USAGE, HeadlessOptions, usage as headless_usage};

pub const HANDOFF_ONLY: &str = "ZEN_HANDOFF_ONLY";

const START_WAIT: Duration = Duration::from_secs(60);

const START_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
pub struct Program {
    pub name: &'static str,
    pub id: &'static str,
    pub app_id: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    Window(Vec<PathBuf>),
    Headless(HeadlessOptions),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Parsed {
    Start(Start),
    Wait(Vec<PathBuf>),
    Version,
    Help,
    Mcp { headless: bool, project: Option<PathBuf>, no_restore: bool },
    Usage(String),
}

pub fn read_command_line(program: &Program, args: impl IntoIterator<Item = OsString>) -> Result<Start, i32> {
    let args: Vec<OsString> = args.into_iter().collect();
    if let Some(code) = zen_db::ssh::askpass_main(&args) {
        return Err(code);
    }
    zen_i18n::register(crate::toolbox::LOCALES);
    let data = data_folder(program.app_id);
    zen_i18n::set_language(data.as_deref().and_then(language_setting).as_deref());
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let log = |data: Option<&Path>| {
        zen_workbench::ide_log::install(program.app_id, program.name, program.version, data);
        read_login_environment(data);
    };
    match parse(program, args) {
        Parsed::Start(Start::Window(paths)) => {
            let snapshot = std::env::var_os("ZEN_UI_SCREENSHOT").is_some();
            if !snapshot && data.as_deref().is_some_and(|data| zen_workbench::handoff::send(data, &paths)) {
                return Err(0);
            }
            if handoff_only() {
                let _ = writeln!(stderr, "{}: {}", program.id, t!("ide-cli-not-handed-over", product = program.name));
                return Err(1);
            }
            log(data.as_deref());
            Ok(Start::Window(paths))
        }
        Parsed::Wait(paths) => {
            let handed = data.as_deref().is_some_and(|data| zen_workbench::handoff::send_and_wait(data, &paths));
            if handed {
                return Err(0);
            }
            if handoff_only() {
                let _ = writeln!(stderr, "{}: {}", program.id, t!("ide-cli-not-handed-over", product = program.name));
                return Err(1);
            }
            match start_and_wait(data.as_deref(), &paths) {
                Ok(()) => Err(0),
                Err(error) => {
                    let _ = writeln!(stderr, "{}: {error}", program.id);
                    Err(1)
                }
            }
        }
        Parsed::Start(start) => {
            log(data.as_deref());
            Ok(start)
        }
        Parsed::Version => {
            let _ = writeln!(stdout, "{}", t!("ide-cli-version", product = program.name, version = program.version));
            Err(0)
        }
        Parsed::Help => {
            let _ = write!(stdout, "{}", help(program));
            Err(0)
        }
        Parsed::Usage(message) => {
            let _ = writeln!(stderr, "{}: {message}", program.id);
            Err(EXIT_USAGE)
        }
        Parsed::Mcp { headless, project, no_restore } => {
            if headless {
                log(data.as_deref());
            }
            let result =
                if headless { mcp_headless(program, data.as_deref(), project, no_restore) } else { mcp(program) };
            match result {
                Ok(()) => Err(0),
                Err(error) => {
                    let _ = writeln!(stderr, "{} mcp: {error}", program.id);
                    Err(1)
                }
            }
        }
    }
}

fn handoff_only() -> bool {
    std::env::var_os(HANDOFF_ONLY).is_some_and(|value| !value.is_empty() && value != "0")
}

fn start_and_wait(data: Option<&Path>, paths: &[PathBuf]) -> Result<(), String> {
    let data = data.ok_or_else(|| t!("ide-cli-no-data-folder"))?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command.spawn().map_err(|error| t!("ide-cli-cannot-start", error = error.to_string()))?;
    let started = Instant::now();
    while started.elapsed() < START_WAIT {
        if zen_workbench::handoff::send_and_wait(data, paths) {
            return Ok(());
        }
        std::thread::sleep(START_POLL);
    }
    Err(t!("ide-cli-wait-no-ide"))
}

fn parse(program: &Program, args: Vec<OsString>) -> Parsed {
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Parsed::Version;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Parsed::Help;
    }
    if args.first().is_some_and(|arg| arg == "mcp") {
        let mut headless = false;
        let mut no_restore = false;
        let mut projects = Vec::new();
        for arg in &args[1..] {
            match arg.to_str() {
                Some("--headless") => headless = true,
                Some("--no-restore") => no_restore = true,
                Some(text) if text.starts_with('-') => {
                    return Parsed::Usage(mcp_usage(program, &t!("ide-headless-unknown", option = text)));
                }
                _ => projects.push(PathBuf::from(arg)),
            }
        }
        if (!projects.is_empty() || no_restore) && !headless {
            return Parsed::Usage(mcp_usage(program, &t!("ide-cli-mcp-project-needs-headless")));
        }
        if projects.len() > 1 {
            return Parsed::Usage(mcp_usage(program, &t!("ide-cli-mcp-one-project")));
        }
        return Parsed::Mcp { headless, project: projects.pop(), no_restore };
    }
    match HeadlessOptions::from_args(args.clone()) {
        Ok(Some(options)) => Parsed::Start(Start::Headless(options)),
        Ok(None) => window(program, args),
        Err(error) => Parsed::Usage(headless_usage(program.id, &error)),
    }
}

fn window(program: &Program, args: Vec<OsString>) -> Parsed {
    let mut paths = Vec::new();
    let mut options_end = false;
    let mut wait = false;
    for arg in args {
        match arg.to_str() {
            Some("--") if !options_end => options_end = true,
            Some("--wait") if !options_end => wait = true,
            Some(text) if !options_end && text.starts_with("-psn_") => {}
            Some(text) if !options_end && text.starts_with('-') && text.len() > 1 => {
                let error = t!("ide-headless-unknown", option = text);
                return Parsed::Usage(t!("ide-cli-window-usage", error = error, program = program.id));
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    if wait && !paths.is_empty() { Parsed::Wait(paths) } else { Parsed::Start(Start::Window(paths)) }
}

fn mcp_usage(program: &Program, error: &str) -> String {
    t!("ide-cli-mcp-usage", error = error, program = program.id)
}

fn help(program: &Program) -> String {
    let about = t!("ide-cli-about", product = program.name, version = program.version);
    let mut text = format!("{about}\n\n{}\n", t!("ide-cli-usage"));
    for (syntax, about) in [
        (t!("ide-cli-help-window-syntax"), t!("ide-cli-help-window")),
        (t!("ide-cli-help-wait-syntax"), t!("ide-cli-help-wait", program = program.id)),
        (t!("ide-cli-help-headless-syntax"), t!("ide-cli-help-headless")),
        (t!("ide-cli-help-mcp-headless-syntax"), t!("ide-cli-help-mcp-headless")),
        ("mcp".to_owned(), t!("ide-cli-help-mcp")),
        ("--version, -V".to_owned(), t!("ide-cli-help-version")),
        ("--help, -h".to_owned(), t!("ide-cli-help-help")),
    ] {
        text.push_str(&format!("  {} {syntax}\n      {about}\n", program.id));
    }
    text
}

fn data_folder(app_id: &str) -> Option<PathBuf> {
    zen_workbench::recent::default_file(app_id).and_then(|file| Some(file.parent()?.to_owned()))
}

fn login_shell_setting(data: &Path) -> Option<bool> {
    let bytes = std::fs::read(data.join("settings.json")).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    settings.get("loginShellEnvironment")?.as_bool()
}

fn read_login_environment(data: Option<&Path>) {
    let on = data.and_then(login_shell_setting).unwrap_or(true);
    zen_env::set_enabled(on);
    let opted_out = std::env::var_os("ZEN_LOGIN_SHELL").is_some_and(|value| value == "none");
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from);
    if let Some(home) = home.filter(|_| on && !opted_out && cfg!(unix)) {
        zen_env::start(zen_env::user_shell(), home, zen_env::TIMEOUT);
    }
}

fn language_setting(data: &Path) -> Option<String> {
    let bytes = std::fs::read(data.join("settings.json")).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    settings.get("language")?.as_str().map(str::to_owned)
}

fn mcp(program: &Program) -> Result<(), String> {
    super::app::run_mcp_bridge(program.app_id)
}

fn mcp_headless(
    program: &Program,
    data: Option<&Path>,
    project: Option<PathBuf>,
    no_restore: bool,
) -> Result<(), String> {
    let data = data.ok_or_else(|| t!("ide-cli-no-data-folder"))?;
    let project = match project {
        Some(project) => project,
        None => std::env::current_dir().map_err(|error| error.to_string())?,
    };
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let id = program.id;
    zen_automation::bridge::run_headless(data, &project, move |project| {
        let mut command = std::process::Command::new(&exe);
        command.args(["--headless", "--exit-when-unused"]);
        if no_restore {
            command.arg("--no-restore");
        }
        command.arg(project);
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let log = std::env::temp_dir().join(format!("{id}-headless-{}.log", std::process::id()));
        let stderr = std::fs::File::create(&log).map(Stdio::from).unwrap_or(Stdio::null());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|error| t!("ide-cli-cannot-start", error = error.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: Program =
        Program { name: "GoGarden", id: "gogarden", app_id: "space.ifzenelse.test", version: "1.2.3" };

    fn parse_args(args: &[&str]) -> Parsed {
        parse(&PROGRAM, args.iter().map(OsString::from).collect())
    }

    #[test]
    fn version_and_help_come_before_everything_else() {
        assert_eq!(parse_args(&["--version"]), Parsed::Version);
        assert_eq!(parse_args(&["-V"]), Parsed::Version);
        assert_eq!(parse_args(&["--headless", "--version"]), Parsed::Version);
        assert_eq!(parse_args(&["mcp", "-V"]), Parsed::Version);
        assert_eq!(parse_args(&["--help"]), Parsed::Help);
        assert_eq!(parse_args(&["-h"]), Parsed::Help);
        assert_eq!(parse_args(&["/tmp/a", "--help"]), Parsed::Help);
    }

    #[test]
    fn wait_holds_a_start_only_when_it_opens_something() {
        assert_eq!(parse_args(&["--wait", "/tmp/a"]), Parsed::Wait(vec![PathBuf::from("/tmp/a")]));
        assert_eq!(parse_args(&["/tmp/a", "--wait"]), Parsed::Wait(vec![PathBuf::from("/tmp/a")]));
        assert_eq!(parse_args(&["--wait"]), Parsed::Start(Start::Window(Vec::new())));
        assert_eq!(
            parse_args(&["--", "--wait"]),
            Parsed::Start(Start::Window(vec![PathBuf::from("--wait")])),
            "after -- it is a file"
        );
    }

    #[test]
    fn the_command_line_picks_windows_headless_or_a_bridge() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        assert_eq!(parse_args(&[]), Parsed::Start(Start::Window(Vec::new())));
        assert_eq!(parse_args(&["/tmp/a"]), Parsed::Start(Start::Window(vec![PathBuf::from("/tmp/a")])));
        assert!(matches!(parse_args(&["--headless", "/tmp/a"]), Parsed::Start(Start::Headless(_))));
        assert_eq!(parse_args(&["mcp"]), Parsed::Mcp { headless: false, project: None, no_restore: false });
        assert_eq!(
            parse_args(&["mcp", "--headless"]),
            Parsed::Mcp { headless: true, project: None, no_restore: false }
        );
        assert_eq!(
            parse_args(&["mcp", "--headless", "."]),
            Parsed::Mcp { headless: true, project: Some(PathBuf::from(".")), no_restore: false }
        );
        assert_eq!(
            parse_args(&["mcp", "--headless", "--no-restore", "."]),
            Parsed::Mcp { headless: true, project: Some(PathBuf::from(".")), no_restore: true }
        );
        assert!(matches!(parse_args(&["mcp", "--no-restore"]), Parsed::Usage(_)), "only for a headless IDE");
        assert_eq!(
            parse_args(&["mcp", "--headless", "a", "b"]),
            Parsed::Usage(
                "only one project can be given; usage: gogarden mcp [--headless [--no-restore] [<project>]]".to_owned()
            )
        );
        assert!(matches!(parse_args(&["mcp", "."]), Parsed::Usage(_)));
        assert!(matches!(parse_args(&["mcp", "--nope"]), Parsed::Usage(_)));
        assert!(matches!(parse_args(&["--headless", "--nope"]), Parsed::Usage(_)));
    }

    #[test]
    fn an_unknown_option_starts_nothing() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        assert_eq!(
            parse_args(&["--bogus-flag"]),
            Parsed::Usage(
                "unknown option `--bogus-flag`; usage: gogarden [<folder or file>...]; gogarden --help lists every way to start"
                    .to_owned()
            )
        );
        assert!(matches!(parse_args(&["/tmp/a", "-x"]), Parsed::Usage(_)));
        assert_eq!(
            parse_args(&["--", "--bogus-flag", "-"]),
            Parsed::Start(Start::Window(vec![PathBuf::from("--bogus-flag"), PathBuf::from("-")])),
            "after -- everything is a path"
        );
        assert_eq!(
            parse_args(&["-psn_0_12345", "/tmp/a"]),
            Parsed::Start(Start::Window(vec![PathBuf::from("/tmp/a")])),
            "the process serial number of older macOS versions is ignored"
        );
    }

    #[test]
    fn the_help_names_every_way_to_start() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let text = help(&PROGRAM);
        assert!(text.starts_with("GoGarden 1.2.3"), "{text}");
        for line in [
            "gogarden --headless",
            "gogarden mcp --headless [--no-restore] [<project>]",
            "gogarden mcp\n",
            "gogarden --version, -V",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
    }

    #[test]
    fn messages_follow_the_language_setting_of_the_data_folder() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(language_setting(data.path()), None, "no settings: the system's language");
        std::fs::write(data.path().join("settings.json"), r#"{"mcpEnabled": true}"#).unwrap();
        assert_eq!(language_setting(data.path()), None);
        std::fs::write(data.path().join("settings.json"), r#"{"language": "ru"}"#).unwrap();
        assert_eq!(language_setting(data.path()).as_deref(), Some("ru"));
    }
}
