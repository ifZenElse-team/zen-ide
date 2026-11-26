use std::path::{Path, PathBuf};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::console::{ConsoleLine, Stream};
use crate::links::LinkSpan;
use crate::parser::{OutputParser, ParsedLine};
use crate::test_tree::TestEvent;
use crate::{CommandLine, Configuration, KindSpec, RunContext, RunProvider, RunTarget};

pub const KIND: &str = "externalTool";

pub const FILE_PATH: &str = "$FILE_PATH$";

pub const LINE: &str = "$LINE$";

pub const COLUMN: &str = "$COLUMN$";

pub const MESSAGE: &str = "$MESSAGE$";

/// What an external tool's run starts and shows, with every macro expanded: the options of its configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolLaunch {
    /// The id of the tool.
    pub tool_id: String,
    /// The program.
    pub program: PathBuf,
    /// Its arguments, one per item.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub cwd: PathBuf,
    /// The output filters: regular expressions with `$FILE_PATH$`, `$LINE$` and `$COLUMN$`.
    pub filters: Vec<String>,
    /// Whether open files are read again from disk once the run has ended.
    pub synchronize: bool,
    /// Whether the run gets a tab in the Run tool window.
    pub open_console: bool,
    /// Whether the Run tool window comes forward when the program writes to its standard output.
    pub activate_on_stdout: bool,
    /// Whether the Run tool window comes forward when the program writes to its standard error.
    pub activate_on_stderr: bool,
    /// Environment variables added to the IDE's, as `[[name, value]]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    /// Whether the run gets a tab in the Run tool window once it has failed, when it gets none from the start.
    pub console_on_error: bool,
}

impl ToolLaunch {
    pub fn of(configuration: &Configuration) -> Option<ToolLaunch> {
        (configuration.kind == KIND).then(|| serde_json::from_value(configuration.options.clone()).ok()).flatten()
    }

    pub fn command_line(&self) -> CommandLine {
        let display = std::iter::once(self.program.display().to_string())
            .chain(self.args.iter().cloned())
            .map(|word| quoted(&word))
            .collect::<Vec<_>>()
            .join(" ");
        CommandLine {
            program: self.program.clone(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            display,
        }
    }
}

pub fn joined(words: &[String]) -> String {
    words.iter().map(|word| quoted(word)).collect::<Vec<_>>().join(" ")
}

pub fn command_configuration(
    line: &str,
    program: PathBuf,
    args: Vec<String>,
    cwd: &Path,
    env: Vec<(String, String)>,
) -> Configuration {
    let launch = ToolLaunch { program, args, cwd: cwd.to_owned(), open_console: true, env, ..ToolLaunch::default() };
    Configuration {
        id: String::new(),
        name: line.trim().to_owned(),
        kind: KIND.to_owned(),
        temporary: true,
        common: Default::default(),
        options: serde_json::to_value(launch).unwrap_or_default(),
        ..Configuration::default()
    }
}

pub fn quoted(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | ',' | '+' | '@' | '%'));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

#[derive(Clone, Debug)]
pub struct OutputFilter {
    regex: Regex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterMatch {
    pub range: std::ops::Range<usize>,
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
    pub message: Option<String>,
}

impl OutputFilter {
    pub fn compile(filter: &str) -> Result<OutputFilter, String> {
        Self::build(filter, r"(?P<file>(?:[A-Za-z]:)?[\\/][^:\n]*)")
    }

    pub fn compile_relative(filter: &str) -> Result<OutputFilter, String> {
        Self::build(filter, r"(?P<file>(?:[A-Za-z]:[\\/])?[^:\n]+)")
    }

    fn build(filter: &str, file: &str) -> Result<OutputFilter, String> {
        if !filter.contains(FILE_PATH) {
            return Err(zen_i18n::t!("run-filter-no-file-path", filter = filter, placeholder = FILE_PATH));
        }
        let pattern = filter
            .replacen(FILE_PATH, file, 1)
            .replacen(LINE, r"(?P<line>[0-9]+)", 1)
            .replacen(COLUMN, r"(?P<column>[0-9]+)", 1)
            .replacen(MESSAGE, r"(?P<message>.+)", 1);
        Regex::new(&pattern).map(|regex| OutputFilter { regex }).map_err(|error| error.to_string())
    }

    pub fn matches(&self, text: &str) -> Vec<FilterMatch> {
        let chars = |byte: usize| text[..byte].chars().count();
        self.regex
            .captures_iter(text)
            .filter_map(|found| {
                let whole = found.get(0)?;
                let file = found.name("file")?.as_str().trim();
                if file.is_empty() || whole.is_empty() {
                    return None;
                }
                let number = |name: &str| found.name(name).and_then(|value| value.as_str().parse::<u32>().ok());
                let message = found.name("message").map(|value| value.as_str().trim().to_owned());
                Some(FilterMatch {
                    range: chars(whole.start())..chars(whole.end()),
                    file: PathBuf::from(file),
                    line: number("line").unwrap_or(1).max(1),
                    column: number("column"),
                    message: message.filter(|message| !message.is_empty()),
                })
            })
            .collect()
    }

    pub fn links(&self, text: &str) -> Vec<LinkSpan> {
        self.matches(text)
            .into_iter()
            .map(|found| LinkSpan { range: found.range, file: found.file, line: found.line, column: found.column })
            .collect()
    }
}

pub struct FilterParser {
    filters: Vec<OutputFilter>,
}

impl FilterParser {
    pub fn new(filters: &[String]) -> Self {
        Self { filters: filters.iter().filter_map(|filter| OutputFilter::compile(filter).ok()).collect() }
    }
}

impl OutputParser for FilterParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut line = ConsoleLine::new(stream, text);
        let mut links: Vec<LinkSpan> = self.filters.iter().flat_map(|filter| filter.links(&line.text)).collect();
        if !links.is_empty() {
            let overlaps = |found: &LinkSpan| {
                links.iter().any(|link| link.range.start < found.range.end && found.range.start < link.range.end)
            };
            let rest: Vec<LinkSpan> = line.links.drain(..).filter(|found| !overlaps(found)).collect();
            links.extend(rest);
            links.sort_by_key(|link| link.range.start);
            line.links = links;
        }
        ParsedLine { console: Some(line), events: Vec::new() }
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExternalToolProvider;

impl RunProvider for ExternalToolProvider {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        Vec::new()
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, configuration: &Configuration, _context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let launch = ToolLaunch::of(configuration)
            .ok_or_else(|| zen_i18n::t!("run-external-tool-unknown", kind = configuration.kind.as_str()))?;
        if launch.program.as_os_str().is_empty() {
            return Err(zen_i18n::t!("run-external-tool-no-program"));
        }
        Ok(vec![launch.command_line()])
    }

    fn parser(&self, configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        let filters = ToolLaunch::of(configuration).map(|launch| launch.filters).unwrap_or_default();
        Box::new(FilterParser::new(&filters))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configuration(launch: &ToolLaunch) -> Configuration {
        Configuration {
            id: "externalTool:t".to_owned(),
            name: "Tool".to_owned(),
            kind: KIND.to_owned(),
            temporary: false,
            common: Default::default(),
            options: serde_json::to_value(launch).unwrap(),
            ..Configuration::default()
        }
    }

    #[test]
    fn a_launch_becomes_one_command_line_with_its_arguments_kept_apart() {
        let launch = ToolLaunch {
            tool_id: "t".to_owned(),
            program: PathBuf::from("/bin/echo"),
            args: vec!["two words".to_owned(), "it's".to_owned(), "-n".to_owned(), String::new()],
            cwd: PathBuf::from("/tmp"),
            ..ToolLaunch::default()
        };
        let lines = ExternalToolProvider.command_lines(&configuration(&launch), &RunContext::default()).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].args, launch.args);
        assert_eq!(lines[0].cwd, PathBuf::from("/tmp"));
        assert_eq!(lines[0].display, r"/bin/echo 'two words' 'it'\''s' -n ''");
        assert!(ExternalToolProvider.kinds().is_empty());
        let empty = ToolLaunch::default();
        zen_i18n::register(crate::LOCALES);
        let error = ExternalToolProvider.command_lines(&configuration(&empty), &RunContext::default()).unwrap_err();
        assert_eq!(error, zen_i18n::t!("run-external-tool-no-program"));
    }

    #[test]
    fn a_typed_command_runs_its_program_in_its_folder_with_its_environment_in_a_console() {
        let env = vec![("GOFLAGS".to_owned(), "-mod=mod".to_owned())];
        let configuration = command_configuration(
            " go test ./... ",
            PathBuf::from("/usr/bin/go"),
            vec!["test".to_owned(), "./...".to_owned()],
            Path::new("/work"),
            env.clone(),
        );
        assert_eq!(configuration.name, "go test ./...");
        assert!(configuration.temporary);
        let launch = ToolLaunch::of(&configuration).unwrap();
        assert!(launch.open_console);
        let lines = ExternalToolProvider.command_lines(&configuration, &RunContext::default()).unwrap();
        assert_eq!(lines[0].program, PathBuf::from("/usr/bin/go"));
        assert_eq!(lines[0].cwd, PathBuf::from("/work"));
        assert_eq!(lines[0].env, env);
        assert_eq!(lines[0].display, "/usr/bin/go test ./...");
        let saved = serde_json::to_value(ToolLaunch::default()).unwrap();
        assert!(saved.get("env").is_none(), "{saved}");
    }

    #[test]
    fn output_filters_make_file_links() {
        let filter = OutputFilter::compile("^$FILE_PATH$:$LINE$:$COLUMN$: ").unwrap();
        let links = filter.links("/tmp/my dir/a.txt:3:7: warning");
        assert_eq!(
            links,
            [LinkSpan { range: 0..23, file: PathBuf::from("/tmp/my dir/a.txt"), line: 3, column: Some(7) }]
        );
        let without_column = OutputFilter::compile("at $FILE_PATH$ line $LINE$").unwrap();
        let found = without_column.links("файл at /x/y.rs line 9");
        assert_eq!(found[0].range, 5..22);
        assert_eq!((found[0].line, found[0].column), (9, None));
        zen_i18n::register(crate::LOCALES);
        let error = OutputFilter::compile("$LINE$ only").unwrap_err();
        assert_eq!(error, zen_i18n::t!("run-filter-no-file-path", filter = "$LINE$ only", placeholder = FILE_PATH));
        assert!(!error.starts_with("run-"), "{error}");
        assert!(OutputFilter::compile("$FILE_PATH$ (").is_err());
    }

    #[test]
    fn a_filter_reads_the_message_and_may_take_relative_paths() {
        let filter = OutputFilter::compile_relative("^$FILE_PATH$:$LINE$:$COLUMN$: $MESSAGE$").unwrap();
        let found = filter.matches("pkg/a b.go:12:3: unused variable x");
        assert_eq!(
            found,
            [FilterMatch {
                range: 0..34,
                file: PathBuf::from("pkg/a b.go"),
                line: 12,
                column: Some(3),
                message: Some("unused variable x".to_owned()),
            }]
        );
        assert!(OutputFilter::compile("^$FILE_PATH$:$LINE$").unwrap().matches("pkg/a.go:1").is_empty());
        let absolute = OutputFilter::compile_relative("$FILE_PATH$:$LINE$").unwrap().matches("/p/a.go:4");
        assert_eq!((absolute[0].file.clone(), absolute[0].message.clone()), (PathBuf::from("/p/a.go"), None));
    }

    #[test]
    fn the_filter_parser_puts_its_links_first_and_keeps_others_apart() {
        let mut parser = FilterParser::new(&["ERR $FILE_PATH$@$LINE$".to_owned(), "broken (".to_owned()]);
        let parsed = parser.line(Stream::Stdout, "ERR /p/x.go@4 then y.go:2");
        let line = parsed.console.unwrap();
        let files: Vec<(String, u32)> =
            line.links.iter().map(|link| (link.file.display().to_string(), link.line)).collect();
        assert_eq!(files, [("/p/x.go".to_owned(), 4), ("y.go".to_owned(), 2)]);
        let plain = parser.line(Stream::Stderr, "nothing here");
        assert!(plain.console.unwrap().links.is_empty());
        assert!(parser.finish().is_empty());
    }
}
