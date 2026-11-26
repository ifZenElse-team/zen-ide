mod complete;
mod ide;

use std::ffi::OsString;
use std::io::{self, Read, Write};

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::Value;
use zen_core::{DispatchError, Registry, RegistryError, Session};
use zen_i18n::t;

pub const LOCALES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en/zen-cli.ftl")), ("ru", include_str!("../locales/ru/zen-cli.ftl"))];

pub const LANGUAGE_ENV: &str = "ZEN_LANGUAGE";

pub const EXIT_OK: u8 = 0;

pub const EXIT_FAILURE: u8 = 1;

pub const EXIT_USAGE: u8 = 2;

pub fn set_language(value: Option<&str>) {
    zen_i18n::set_language(value.filter(|value| !value.is_empty()));
}

pub fn registry() -> Result<Registry, RegistryError> {
    zen_i18n::register(LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry)?;
    zen_toolbox::register(&mut registry)?;
    Ok(registry)
}

pub const VERSION: &str = match option_env!("ZEN_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn cli() -> Command {
    zen_i18n::register(LOCALES);
    let options = t!("cli-heading-options");
    Command::new("zen-cli")
        .version(VERSION)
        .about(t!("cli-about"))
        .override_usage(t!("cli-usage"))
        .help_template(template())
        .subcommand_required(true)
        .arg_required_else_help(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .subcommand_help_heading(t!("cli-heading-commands"))
        .subcommand_value_name(t!("cli-value-command"))
        .arg(help_arg(&options))
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(t!("cli-version"))
                .help_heading(&options),
        )
        .subcommand(
            subcommand("commands", t!("cli-commands-about"), t!("cli-commands-usage"))
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help(t!("cli-commands-json"))
                        .help_heading(&options),
                )
                .arg(help_arg(&options)),
        )
        .subcommand(
            subcommand("run", t!("cli-run-about"), t!("cli-run-usage"))
                .arg(
                    Arg::new("id")
                        .required(true)
                        .value_name(t!("cli-value-id"))
                        .help(t!("cli-run-id"))
                        .help_heading(t!("cli-heading-arguments")),
                )
                .arg(
                    Arg::new("params")
                        .long("params")
                        .value_name(t!("cli-value-json"))
                        .default_value("{}")
                        .hide_default_value(true)
                        .allow_hyphen_values(true)
                        .help(t!("cli-run-params"))
                        .help_heading(&options),
                )
                .arg(help_arg(&options)),
        )
        .subcommand(ide_command(&options))
        .subcommand(
            subcommand("completions", t!("cli-completions-about"), t!("cli-completions-usage"))
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_name(t!("cli-value-shell"))
                        .value_parser(complete::SHELLS)
                        .help(t!("cli-completions-shell"))
                        .help_heading(t!("cli-heading-arguments")),
                )
                .arg(help_arg(&options)),
        )
        .subcommand(
            Command::new(complete::HIDDEN)
                .hide(true)
                .arg(Arg::new("shell").required(true).value_parser(complete::SHELLS))
                .arg(Arg::new("words").num_args(0..).last(true).allow_hyphen_values(true)),
        )
}

fn ide_command(options: &str) -> Command {
    let arguments = t!("cli-heading-arguments");
    let instance = || {
        Arg::new("instance")
            .long("instance")
            .value_name(t!("cli-value-instance"))
            .help(t!("cli-ide-instance"))
            .help_heading(options.to_owned())
    };
    subcommand("ide", t!("cli-ide-about"), t!("cli-ide-usage"))
        .subcommand_required(true)
        .subcommand_help_heading(t!("cli-heading-commands"))
        .subcommand_value_name(t!("cli-value-command"))
        .arg(help_arg(options))
        .subcommand(
            subcommand("start", t!("cli-ide-start-about"), t!("cli-ide-start-usage"))
                .arg(
                    Arg::new("project")
                        .required(true)
                        .value_name(t!("cli-value-project"))
                        .help(t!("cli-ide-start-project"))
                        .help_heading(&arguments),
                )
                .arg(
                    Arg::new("ide")
                        .long("ide")
                        .value_name(t!("cli-value-program"))
                        .help(t!("cli-ide-start-ide"))
                        .help_heading(options.to_owned()),
                )
                .arg(
                    Arg::new("mcp-port")
                        .long("mcp-port")
                        .value_name(t!("cli-value-port"))
                        .help(t!("cli-ide-start-port"))
                        .help_heading(options.to_owned()),
                )
                .arg(help_arg(options)),
        )
        .subcommand(
            subcommand("list", t!("cli-ide-list-about"), t!("cli-ide-list-usage"))
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help(t!("cli-ide-list-json"))
                        .help_heading(options.to_owned()),
                )
                .arg(help_arg(options)),
        )
        .subcommand(
            subcommand("stop", t!("cli-ide-stop-about"), t!("cli-ide-stop-usage"))
                .arg(instance())
                .arg(help_arg(options)),
        )
        .subcommand(
            subcommand("call", t!("cli-ide-call-about"), t!("cli-ide-call-usage"))
                .arg(
                    Arg::new("tool")
                        .required(true)
                        .value_name(t!("cli-value-tool"))
                        .help(t!("cli-ide-call-tool"))
                        .help_heading(&arguments),
                )
                .arg(
                    Arg::new("args")
                        .long("args")
                        .value_name(t!("cli-value-json"))
                        .default_value("{}")
                        .hide_default_value(true)
                        .allow_hyphen_values(true)
                        .help(t!("cli-ide-call-args"))
                        .help_heading(options.to_owned()),
                )
                .arg(instance())
                .arg(help_arg(options)),
        )
}

fn template() -> String {
    format!("{{about-with-newline}}\n{}: {{usage}}\n\n{{all-args}}", t!("cli-heading-usage"))
}

fn subcommand(name: &'static str, about: String, usage: String) -> Command {
    Command::new(name).about(about).override_usage(usage).help_template(template()).disable_help_flag(true)
}

fn help_arg(heading: &str) -> Arg {
    Arg::new("help")
        .short('h')
        .long("help")
        .action(ArgAction::Help)
        .help(t!("cli-help"))
        .help_heading(heading.to_owned())
}

pub fn run<I, A>(args: I, stdin: &mut impl Read, stdout: &mut impl Write, stderr: &mut impl Write) -> u8
where
    I: IntoIterator<Item = A>,
    A: Into<OsString> + Clone,
{
    let matches = match cli().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) => return clap_error(&error, stdout, stderr),
    };
    let registry = match registry() {
        Ok(registry) => registry,
        Err(error) => return fail(stderr, &t!("cli-error-registry", error = error.to_string()), EXIT_FAILURE),
    };
    match matches.subcommand() {
        Some(("commands", matches)) => match list(&registry, matches.get_flag("json"), stdout) {
            Ok(()) => EXIT_OK,
            Err(_) => EXIT_FAILURE,
        },
        Some(("run", matches)) => match dispatch(&registry, matches, stdin) {
            Ok(output) => match print_json(stdout, &output) {
                Ok(()) => EXIT_OK,
                Err(_) => EXIT_FAILURE,
            },
            Err((message, code)) => fail(stderr, &message, code),
        },
        Some(("completions", matches)) => {
            let shell = matches.get_one::<String>("shell").map_or("", String::as_str);
            match write!(stdout, "{}", complete::script(shell)) {
                Ok(()) => EXIT_OK,
                Err(_) => EXIT_FAILURE,
            }
        }
        Some((complete::HIDDEN, matches)) => {
            let shell = matches.get_one::<String>("shell").map_or("", String::as_str);
            let words: Vec<String> = matches.get_many::<String>("words").into_iter().flatten().cloned().collect();
            match complete::print(&registry, shell, &words, stdout) {
                Ok(()) => EXIT_OK,
                Err(_) => EXIT_FAILURE,
            }
        }
        Some(("ide", matches)) => match ide::run(matches, stdin, stdout) {
            Ok(Value::Null) => EXIT_OK,
            Ok(output) => match print_json(stdout, &output) {
                Ok(()) => EXIT_OK,
                Err(_) => EXIT_FAILURE,
            },
            Err((message, code)) => fail(stderr, &message, code),
        },
        _ => fail(stderr, &t!("cli-error-usage"), EXIT_USAGE),
    }
}

fn list(registry: &Registry, json: bool, stdout: &mut impl Write) -> io::Result<()> {
    if json {
        let commands: Vec<_> = registry.commands().collect();
        return print_json(stdout, &serde_json::json!({ "commands": commands }));
    }
    let width = registry.commands().map(|info| info.id.as_str().len()).max().unwrap_or(0);
    for info in registry.commands() {
        let line = format!("{:<width$}  {}", info.id.as_str(), info.title);
        writeln!(stdout, "{}", line.trim_end())?;
    }
    Ok(())
}

fn dispatch(registry: &Registry, matches: &ArgMatches, stdin: &mut impl Read) -> Result<Value, (String, u8)> {
    let id = matches.get_one::<String>("id").map_or("", String::as_str);
    let mut text = matches.get_one::<String>("params").cloned().unwrap_or_default().into_bytes();
    if text == b"-" {
        text.clear();
        stdin
            .read_to_end(&mut text)
            .map_err(|error| (t!("cli-error-stdin", error = error.to_string()), EXIT_FAILURE))?;
    }
    let params: Value = serde_json::from_slice(&text)
        .map_err(|error| (t!("cli-error-invalid-json", error = error.to_string()), EXIT_USAGE))?;
    let mut session = Session::new();
    registry.dispatch(&mut session, id, params).map_err(|error| (dispatch_message(&error), exit_code(&error)))
}

pub fn dispatch_message(error: &DispatchError) -> String {
    match error {
        DispatchError::UnknownCommand { id } => t!("cli-error-unknown-command", id = id.as_str()),
        DispatchError::Unavailable { id } => t!("cli-error-unavailable", id = id.as_str()),
        DispatchError::InvalidParams { id, message } => {
            t!("cli-error-invalid-params", id = id.as_str(), error = message.as_str())
        }
        DispatchError::Failed { id, error } => t!("cli-error-failed", id = id.as_str(), error = error.message.as_str()),
        DispatchError::InvalidOutput { id, message } => {
            t!("cli-error-invalid-output", id = id.as_str(), error = message.as_str())
        }
    }
}

pub fn exit_code(error: &DispatchError) -> u8 {
    match error {
        DispatchError::UnknownCommand { .. } | DispatchError::InvalidParams { .. } => EXIT_USAGE,
        DispatchError::Unavailable { .. } | DispatchError::Failed { .. } | DispatchError::InvalidOutput { .. } => {
            EXIT_FAILURE
        }
    }
}

fn clap_error(error: &clap::Error, stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => match write!(stdout, "{}", error.render()) {
            Ok(()) => EXIT_OK,
            Err(_) => EXIT_FAILURE,
        },
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let _ = write!(stderr, "{}", error.render());
            EXIT_USAGE
        }
        kind => {
            let message = match kind {
                ErrorKind::InvalidSubcommand => context(error, ContextKind::InvalidSubcommand)
                    .map(|name| t!("cli-error-unknown-subcommand", name = name)),
                ErrorKind::UnknownArgument => {
                    context(error, ContextKind::InvalidArg).map(|name| t!("cli-error-unknown-argument", name = name))
                }
                ErrorKind::MissingRequiredArgument => {
                    context(error, ContextKind::InvalidArg).map(|name| t!("cli-error-missing-argument", name = name))
                }
                _ => context(error, ContextKind::InvalidArg).map(|name| t!("cli-error-invalid-argument", name = name)),
            };
            fail(stderr, &message.unwrap_or_else(|| t!("cli-error-usage")), EXIT_USAGE)
        }
    }
}

fn context(error: &clap::Error, kind: ContextKind) -> Option<String> {
    match error.get(kind)? {
        ContextValue::String(value) => Some(value.clone()),
        ContextValue::Strings(values) => Some(values.join(", ")),
        _ => None,
    }
}

fn print_json(stdout: &mut impl Write, value: &Value) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    writeln!(stdout, "{text}")
}

fn fail(stderr: &mut impl Write, message: &str, code: u8) -> u8 {
    let _ = writeln!(stderr, "{}", t!("cli-error", message = message));
    code
}

#[cfg(test)]
mod tests {
    use zen_core::{CommandError, CommandId};

    use super::*;

    fn run_with(args: &[&str], stdin: &str) -> (u8, String, String) {
        set_language(Some("en"));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(args.iter().copied(), &mut stdin.as_bytes(), &mut stdout, &mut stderr);
        (code, String::from_utf8(stdout).unwrap(), String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn registry_has_workbench_and_toolbox_commands() {
        let registry = registry().unwrap();
        assert!(registry.info("file.project.open").is_some());
        assert!(registry.info("toolbox.tools.list").is_some());
    }

    #[test]
    fn commands_are_aligned_and_sorted() {
        let (code, stdout, stderr) = run_with(&["zen-cli", "commands"], "");
        assert_eq!((code, stderr.as_str()), (EXIT_OK, ""));
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), registry().unwrap().commands().count());
        let ids: Vec<&str> = lines.iter().map(|line| line.split_whitespace().next().unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        let width = ids.iter().map(|id| id.len()).max().unwrap();
        assert!(lines.iter().all(|line| line.get(width..width + 2) == Some("  ")), "{stdout}");
    }

    #[test]
    fn toolbox_commands_fail_without_tools() {
        let (code, stdout, stderr) = run_with(&["zen-cli", "run", "toolbox.tools.list"], "");
        assert_eq!(code, EXIT_FAILURE);
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("zen-cli: `toolbox.tools.list` failed: "), "{stderr}");
    }

    #[test]
    fn params_come_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let params = serde_json::json!({ "path": dir.path() }).to_string();
        let (code, stdout, _) = run_with(&["zen-cli", "run", "file.project.open", "--params", "-"], &params);
        assert_eq!((code, stdout.as_str()), (EXIT_OK, "null\n"));
    }

    #[test]
    fn params_that_are_not_utf8_are_invalid_json() {
        set_language(Some("en"));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let args = ["zen-cli", "run", "file.project.open", "--params", "-"];
        let code = run(args, &mut &b"\xff"[..], &mut stdout, &mut stderr);
        assert_eq!((code, stdout.as_slice()), (EXIT_USAGE, &b""[..]));
        assert!(String::from_utf8(stderr).unwrap().starts_with("zen-cli: the parameters are not valid JSON: "));
    }

    #[test]
    fn dispatch_errors_map_to_exit_codes() {
        let id = CommandId::new("file.project.open").unwrap();
        assert_eq!(exit_code(&DispatchError::UnknownCommand { id: "x.y.z".into() }), EXIT_USAGE);
        assert_eq!(exit_code(&DispatchError::InvalidParams { id: id.clone(), message: String::new() }), EXIT_USAGE);
        assert_eq!(exit_code(&DispatchError::Unavailable { id: id.clone() }), EXIT_FAILURE);
        assert_eq!(exit_code(&DispatchError::Failed { id: id.clone(), error: CommandError::new("") }), EXIT_FAILURE);
        assert_eq!(exit_code(&DispatchError::InvalidOutput { id, message: String::new() }), EXIT_FAILURE);
    }

    #[test]
    fn usage_errors_are_one_line() {
        let (code, _, stderr) = run_with(&["zen-cli", "nope"], "");
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(stderr, "zen-cli: unknown subcommand `nope`; see zen-cli --help\n");
        let (code, _, stderr) = run_with(&["zen-cli", "run"], "");
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        assert!(stderr.starts_with("zen-cli: missing "), "{stderr}");
    }
}
