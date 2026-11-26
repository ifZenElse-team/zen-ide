use std::path::PathBuf;

use serde::Deserialize;
use zen_run::{Measure, Module, OutputParser, ParsedLine, Stream, TestEvent, TestStatus};

use super::command::{module_folder, resolve_links};
use super::targets::normalize;
use super::verbose::VerboseParser;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Event {
    action: String,
    package: String,
    test: Option<String>,
    elapsed: Option<f64>,
    output: Option<String>,
    output_type: Option<String>,
    import_path: String,
}

fn benchmark_result(output: &str) -> Option<(u64, Vec<Measure>)> {
    let mut words: Vec<&str> = output.split_whitespace().collect();
    if words.first().is_some_and(|word| word.starts_with("Benchmark")) {
        words.remove(0);
    }
    let (iterations, numbers) = words.split_first()?;
    if numbers.is_empty() || !numbers.len().is_multiple_of(2) {
        return None;
    }
    let measures = numbers.chunks(2).map(|pair| {
        pair[0].parse::<f64>().ok()?;
        pair[1].contains('/').then(|| Measure { unit: pair[1].to_owned(), value: pair[0].to_owned() })
    });
    Some((iterations.parse().ok()?, measures.collect::<Option<Vec<Measure>>>()?))
}

pub struct Test2JsonParser {
    cwd: PathBuf,
    modules: Vec<Module>,
    open: Vec<(String, String)>,
    verbose: Option<VerboseParser>,
}

impl Test2JsonParser {
    pub fn new(cwd: PathBuf, modules: &[Module]) -> Self {
        Self { cwd, modules: modules.to_vec(), open: Vec::new(), verbose: None }
    }

    pub fn with_verbose(mut self, verbose: VerboseParser) -> Self {
        self.verbose = Some(verbose);
        self
    }

    fn package_folder(&self, package: &str) -> PathBuf {
        if package.starts_with('.') {
            return normalize(&self.cwd.join(package));
        }
        module_folder(package, &self.modules).map_or_else(|| self.cwd.clone(), |(_, folder)| folder)
    }

    fn console(&self, stream: Stream, output: &str, package: Option<&str>) -> ParsedLine {
        let text = output.strip_suffix('\n').unwrap_or(output);
        let mut parsed = ParsedLine::console(stream, text);
        let bare = package.map_or_else(|| self.cwd.clone(), |package| self.package_folder(package));
        resolve_links(&mut parsed, &self.cwd, &bare);
        parsed
    }

    fn finished(&mut self, event: Event, status: TestStatus) -> ParsedLine {
        let duration_ms = event.elapsed.map(|seconds| (seconds * 1000.0).round().max(0.0) as u64);
        let mut events = Vec::new();
        match event.test {
            Some(test) => {
                self.open.retain(|(package, open)| !(*package == event.package && *open == test));
                events.push(TestEvent::Finished { package: event.package, test: Some(test), status, duration_ms });
            }
            None => {
                let left = match status {
                    TestStatus::Skipped => TestStatus::Skipped,
                    TestStatus::Passed => TestStatus::Passed,
                    _ => TestStatus::Failed,
                };
                let (closed, open): (Vec<_>, Vec<_>) =
                    std::mem::take(&mut self.open).into_iter().partition(|(package, _)| *package == event.package);
                self.open = open;
                for (package, test) in closed.into_iter().rev() {
                    events.push(TestEvent::Finished { package, test: Some(test), status: left, duration_ms: None });
                }
                events.push(TestEvent::Finished { package: event.package, test: None, status, duration_ms });
            }
        }
        ParsedLine { console: None, events }
    }

    fn event(&mut self, event: Event) -> ParsedLine {
        match event.action.as_str() {
            "start" => {
                ParsedLine { console: None, events: vec![TestEvent::Started { package: event.package, test: None }] }
            }
            "run" => {
                let test = event.test.unwrap_or_default();
                if !self.open.iter().any(|(package, open)| *package == event.package && *open == test) {
                    self.open.push((event.package.clone(), test.clone()));
                }
                ParsedLine {
                    console: None,
                    events: vec![TestEvent::Started { package: event.package, test: Some(test) }],
                }
            }
            "output" => {
                let stream = match event.output_type.as_deref() {
                    Some("error" | "error-continue") => Stream::Stderr,
                    _ => Stream::Stdout,
                };
                let mut parsed =
                    self.console(stream, event.output.as_deref().unwrap_or_default(), Some(&event.package));
                let benchmark = event.test.as_ref().filter(|test| test.starts_with("Benchmark"));
                if let (Some(test), Some((iterations, measures))) =
                    (benchmark, event.output.as_deref().and_then(benchmark_result))
                {
                    parsed.events.push(TestEvent::Measured {
                        package: event.package.clone(),
                        test: test.clone(),
                        iterations,
                        measures,
                    });
                }
                parsed.events.push(TestEvent::Output { package: event.package, test: event.test });
                parsed
            }
            "pass" | "bench" => self.finished(event, TestStatus::Passed),
            "fail" => self.finished(event, TestStatus::Failed),
            "skip" => self.finished(event, TestStatus::Skipped),
            "build-output" => {
                let package = build_package(&event.import_path);
                let mut parsed = self.console(Stream::Stderr, event.output.as_deref().unwrap_or_default(), None);
                parsed.events.push(TestEvent::Output { package, test: None });
                parsed
            }
            "build-fail" => ParsedLine {
                console: None,
                events: vec![TestEvent::Finished {
                    package: build_package(&event.import_path),
                    test: None,
                    status: TestStatus::Failed,
                    duration_ms: None,
                }],
            },
            _ => ParsedLine::default(),
        }
    }
}

fn build_package(import_path: &str) -> String {
    match import_path.split_once(" [") {
        Some((path, variant)) => match variant.strip_suffix(']').and_then(|inner| inner.strip_suffix(".test")) {
            Some(tested) => tested.to_owned(),
            None => path.to_owned(),
        },
        None => import_path.to_owned(),
    }
}

impl OutputParser for Test2JsonParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let json = text.trim_start();
        if json.starts_with('{')
            && let Ok(event) = serde_json::from_str::<Event>(json)
            && !event.action.is_empty()
        {
            return self.event(event);
        }
        match &mut self.verbose {
            Some(verbose) => verbose.line(stream, text),
            None => self.console(stream, text, None),
        }
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        self.open.clear();
        self.verbose.as_mut().map(OutputParser::finish).unwrap_or_default()
    }
}

impl std::fmt::Debug for Test2JsonParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Test2JsonParser").field("cwd", &self.cwd).field("open", &self.open).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn build_import_paths_name_the_tested_package() {
        assert_eq!(build_package("example.com/a [example.com/a.test]"), "example.com/a");
        assert_eq!(build_package("example.com/a_test [example.com/a.test]"), "example.com/a");
        assert_eq!(build_package("example.com/dep"), "example.com/dep");
        assert_eq!(build_package("./nosuch"), "./nosuch");
    }

    #[test]
    fn package_folders_come_from_modules() {
        let module =
            Module { name: "example.com/demo".to_owned(), root: PathBuf::from("/w/demo"), ..Module::default() };
        let parser = Test2JsonParser::new(PathBuf::from("/w/demo/calc"), &[module]);
        assert_eq!(parser.package_folder("example.com/demo/calc"), Path::new("/w/demo/calc"));
        assert_eq!(parser.package_folder("example.com/demo"), Path::new("/w/demo"));
        assert_eq!(parser.package_folder("example.com/demox"), Path::new("/w/demo/calc"));
        assert_eq!(parser.package_folder("command-line-arguments"), Path::new("/w/demo/calc"));
        assert_eq!(parser.package_folder("../x"), Path::new("/w/demo/x"));
    }
}
