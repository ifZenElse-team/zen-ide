use std::io::{self, Write};

use clap::Command;
use serde_json::{Map, Value};
use zen_core::Registry;
use zen_i18n::t;
use zen_toolbox::launchers::CompletionShell;

pub(crate) const HIDDEN: &str = "__complete";

pub(crate) const SHELLS: [&str; 3] = ["zsh", "bash", "fish"];

pub(crate) fn script(shell: &str) -> &'static str {
    CompletionShell::parse(shell).map_or("", CompletionShell::script)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    value: String,
    about: String,
}

impl Candidate {
    fn new(value: impl Into<String>, about: impl ToString) -> Self {
        Self { value: value.into(), about: about.to_string().replace(['\t', '\n'], " ") }
    }
}

pub(crate) fn print(registry: &Registry, shell: &str, words: &[String], stdout: &mut impl Write) -> io::Result<()> {
    for candidate in candidates(&crate::cli(), registry, words) {
        if shell == "bash" {
            writeln!(stdout, "{}", quoted(&candidate.value))?;
        } else {
            writeln!(stdout, "{}\t{}", candidate.value, candidate.about)?;
        }
    }
    Ok(())
}

fn quoted(value: &str) -> String {
    if value.chars().all(|character| character.is_alphanumeric() || "-_./".contains(character)) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

fn candidates(root: &Command, registry: &Registry, words: &[String]) -> Vec<Candidate> {
    let Some((current, before)) = words.split_last() else {
        return in_command(root, registry, "", 0);
    };
    let mut command = root;
    let mut positionals = 0;
    let mut awaited: Option<&clap::Arg> = None;
    for word in before {
        if awaited.take().is_some() {
            continue;
        }
        if let Some(name) = word.strip_prefix("--") {
            awaited = command.get_arguments().find(|arg| arg.get_long() == Some(name) && takes_value(arg));
        } else if !word.starts_with('-') {
            match command.find_subcommand(word) {
                Some(subcommand) if !subcommand.is_hide_set() => {
                    command = subcommand;
                    positionals = 0;
                }
                _ => positionals += 1,
            }
        }
    }
    let found = match awaited {
        Some(arg) => option_values(command, arg, registry, before),
        None if current.starts_with('-') => options(command),
        None => in_command(command, registry, current, positionals),
    };
    found.into_iter().filter(|candidate| candidate.value.starts_with(current.as_str())).collect()
}

fn takes_value(arg: &clap::Arg) -> bool {
    arg.get_action().takes_values()
}

fn options(command: &Command) -> Vec<Candidate> {
    command
        .get_arguments()
        .filter_map(|arg| {
            let long = arg.get_long()?;
            Some(Candidate::new(format!("--{long}"), arg.get_help().map(ToString::to_string).unwrap_or_default()))
        })
        .collect()
}

fn in_command(command: &Command, registry: &Registry, current: &str, positionals: usize) -> Vec<Candidate> {
    if command.get_name() == "run" && positionals == 0 {
        return registry
            .commands()
            .filter(|info| info.id.as_str().starts_with(current))
            .map(|info| Candidate::new(info.id.as_str(), &info.title))
            .collect();
    }
    let subcommands: Vec<Candidate> = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(|subcommand| {
            Candidate::new(subcommand.get_name(), subcommand.get_about().map(ToString::to_string).unwrap_or_default())
        })
        .collect();
    if !subcommands.is_empty() {
        return subcommands;
    }
    let positional = command.get_positionals().nth(positionals);
    positional.map_or_else(Vec::new, possible_values)
}

fn possible_values(arg: &clap::Arg) -> Vec<Candidate> {
    arg.get_possible_values().iter().map(|value| Candidate::new(value.get_name(), String::new())).collect()
}

fn option_values(command: &Command, arg: &clap::Arg, registry: &Registry, before: &[String]) -> Vec<Candidate> {
    match (command.get_name(), arg.get_id().as_str()) {
        ("run", "params") => params_of(registry, before),
        ("start", "ide") => {
            zen_toolbox::catalog().into_iter().map(|tool| Candidate::new(tool.launcher, tool.name)).collect()
        }
        _ => possible_values(arg),
    }
}

fn params_of(registry: &Registry, before: &[String]) -> Vec<Candidate> {
    let schema = before.iter().find_map(|word| registry.commands().find(|info| info.id.as_str() == word));
    let Some(properties) = schema.and_then(|info| info.params_schema.get("properties")?.as_object()) else {
        return vec![Candidate::new("{}", t!("cli-complete-required"))];
    };
    let required: Vec<&str> = schema
        .and_then(|info| info.params_schema.get("required")?.as_array())
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let skeleton = |keep: &dyn Fn(&str) -> bool| {
        let fields: Map<String, Value> = properties
            .iter()
            .filter(|(name, _)| keep(name))
            .map(|(name, schema)| (name.clone(), blank(schema)))
            .collect();
        Value::Object(fields).to_string()
    };
    let mut found = vec![Candidate::new(skeleton(&|name| required.contains(&name)), t!("cli-complete-required"))];
    let everything = skeleton(&|_| true);
    if everything != found[0].value {
        found.push(Candidate::new(everything, t!("cli-complete-all")));
    }
    found
}

fn blank(schema: &Value) -> Value {
    let kind = match schema.get("type") {
        Some(Value::Array(kinds)) => kinds.iter().find_map(Value::as_str).filter(|kind| *kind != "null"),
        Some(kind) => kind.as_str(),
        None => None,
    };
    match kind {
        Some("string") => Value::String(String::new()),
        Some("boolean") => Value::Bool(false),
        Some("integer" | "number") => Value::from(0),
        Some("array") => Value::Array(Vec::new()),
        Some("object") => Value::Object(Map::new()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(words: &[&str]) -> Vec<String> {
        crate::set_language(Some("en"));
        let registry = crate::registry().unwrap();
        let words: Vec<String> = words.iter().map(ToString::to_string).collect();
        candidates(&crate::cli(), &registry, &words).into_iter().map(|candidate| candidate.value).collect()
    }

    #[test]
    fn subcommands_come_from_the_command_line_definition_without_the_hidden_one() {
        assert_eq!(values(&[""]), ["commands", "run", "ide", "completions"]);
        assert_eq!(values(&["co"]), ["commands", "completions"]);
        assert_eq!(values(&["ide", "s"]), ["start", "stop"]);
    }

    #[test]
    fn options_are_offered_for_the_command_before_them() {
        assert_eq!(values(&["commands", "--"]), ["--json", "--help"]);
        assert_eq!(values(&["ide", "start", "x", "--mcp"]), ["--mcp-port"]);
    }

    #[test]
    fn command_ids_come_from_the_registry() {
        let registry = crate::registry().unwrap();
        let every: Vec<String> = registry.commands().map(|info| info.id.as_str().to_owned()).collect();
        assert_eq!(values(&["run", ""]), every);
        assert!(values(&["run", "toolbox.tool."]).contains(&"toolbox.tool.install".to_owned()));
    }

    #[test]
    fn the_params_of_a_command_are_offered_as_a_json_skeleton() {
        assert_eq!(values(&["run", "toolbox.tool.launch", "--params", ""]), [r#"{"id":""}"#, r#"{"id":"","path":""}"#]);
        assert_eq!(values(&["run", "toolbox.tools.list", "--params", ""]), ["{}"]);
    }

    #[test]
    fn the_shell_and_the_ide_are_chosen_from_their_lists() {
        assert_eq!(values(&["completions", ""]), SHELLS);
        assert_eq!(values(&["ide", "start", "x", "--ide", "go"]), ["gogarden"]);
    }

    #[test]
    fn a_free_argument_has_no_candidates_so_the_shell_offers_files() {
        assert!(values(&["ide", "start", ""]).is_empty());
    }

    #[test]
    fn bash_gets_quoted_values_without_descriptions() {
        let registry = crate::registry().unwrap();
        let mut out = Vec::new();
        let words: Vec<String> = ["run", "toolbox.tools.list", "--params", ""].map(str::to_owned).to_vec();
        print(&registry, "bash", &words, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "'{}'\n");
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
