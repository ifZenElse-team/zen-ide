mod coverage;
pub mod libtest;
mod targets;

use std::path::Path;

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_run::{
    AnythingPrefix, CommandLine, Configuration, FieldKind, FieldSpec, KindSpec, OutputParser, RunContext, RunProvider,
    RunTarget, TestId, args,
};

use std::sync::Arc;

use crate::CargoModels;
use crate::toolchain::Toolchain;
pub use libtest::LibtestParser;

pub const RUN: &str = "cargo.run";

pub const TEST: &str = "cargo.test";

pub const BENCH: &str = "cargo.bench";

pub const FILE: &str = "rustc.file";

const EDITIONS: [&str; 4] = ["2024", "2021", "2018", "2015"];

const ANYTHING_COMMANDS: [&str; 7] =
    ["cargo build", "cargo test", "cargo run", "cargo check", "cargo clippy", "cargo fmt", "cargo doc"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CargoOptions {
    pub package: String,
    pub target_kind: String,
    pub target: String,
    pub filter: String,
    pub exact: bool,
    pub features: String,
    pub all_features: bool,
    pub no_default_features: bool,
    pub profile: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileOptions {
    pub file: String,
    pub edition: String,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self { file: String::new(), edition: EDITIONS[0].to_owned() }
    }
}

#[derive(Clone, Default)]
pub struct CargoRun {
    toolchain: Toolchain,
    models: Arc<CargoModels>,
}

impl CargoRun {
    pub fn new(toolchain: Toolchain, models: Arc<CargoModels>) -> Self {
        Self { toolchain, models }
    }
}

fn options(configuration: &Configuration) -> CargoOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn choice(values: &[&str]) -> FieldKind {
    FieldKind::Choice(values.iter().map(|value| (*value).to_owned()).collect())
}

fn kind(id: &str, name: &str, targets: &[&str], filter: bool) -> KindSpec {
    let mut fields = vec![
        field("package", "rust-run-field-package", FieldKind::Text),
        field("targetKind", "rust-run-field-target-kind", choice(targets)),
        field("target", "rust-run-field-target", FieldKind::Text),
    ];
    if filter {
        fields.push(field("filter", "rust-run-field-filter", FieldKind::Text));
        fields.push(field("exact", "rust-run-field-exact", FieldKind::Bool));
    }
    fields.extend([
        field("features", "rust-run-field-features", FieldKind::Text),
        field("allFeatures", "rust-run-field-all-features", FieldKind::Bool),
        field("noDefaultFeatures", "rust-run-field-no-default-features", FieldKind::Bool),
        field("profile", "rust-run-field-profile", FieldKind::Text),
    ]);
    let defaults = CargoOptions { target_kind: targets[0].to_owned(), ..CargoOptions::default() };
    KindSpec {
        id: id.to_owned(),
        name_message: name.to_owned(),
        fields,
        defaults: serde_json::to_value(defaults).unwrap_or_default(),
    }
}

pub fn crate_name(file: &Path) -> String {
    let stem = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) { format!("_{name}") } else { name }
}

pub fn file_lines(
    configuration: &Configuration,
    context: &RunContext,
    cargo: &Path,
    env: Vec<(String, String)>,
) -> Result<Vec<CommandLine>, String> {
    let options: FileOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
    let file = options.file.trim();
    if file.is_empty() {
        return Err(t!("rust-run-error-no-file"));
    }
    let file = Path::new(file);
    let edition = if options.edition.trim().is_empty() { EDITIONS[0] } else { options.edition.trim() };
    let folder = if context.scratch.as_os_str().is_empty() {
        std::env::temp_dir().join("zen-run")
    } else {
        context.scratch.clone()
    };
    zen_run::create_private_folder(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let name = crate_name(file);
    let program = folder.join(crate::toolchain::rust::executable(&name));
    let rustc = cargo.with_file_name(crate::toolchain::rust::executable("rustc"));
    let mut args = vec![
        "--edition".to_owned(),
        edition.to_owned(),
        "--crate-name".to_owned(),
        name,
        "-o".to_owned(),
        program.to_string_lossy().into_owned(),
    ];
    args.extend(args::split(&configuration.common.tool_arguments));
    args.push(file.to_string_lossy().into_owned());
    let mut env = env;
    env.extend(configuration.common.env.iter().cloned());
    let display = |program: &Path, args: &[String]| {
        std::iter::once(program.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|word| quote(&word))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let program_args = args::split(&configuration.common.program_arguments);
    Ok(vec![
        CommandLine {
            display: display(Path::new("rustc"), &args),
            program: rustc,
            args,
            cwd: context.root.clone(),
            env: env.clone(),
        },
        CommandLine {
            display: display(&program, &program_args),
            program,
            args: program_args,
            cwd: configuration.common.working_directory_in(&context.root),
            env,
        },
    ])
}

pub fn cargo_args(
    kind: &str,
    options: &CargoOptions,
    tool_arguments: &str,
    program_arguments: &str,
) -> Result<Vec<String>, String> {
    let command = match kind {
        RUN => "run",
        TEST => "test",
        BENCH => "bench",
        other => return Err(t!("rust-run-error-kind", kind = other)),
    };
    let mut line = vec![command.to_owned()];
    if kind == TEST {
        line.push("--no-fail-fast".to_owned());
    }
    let profile = options.profile.trim();
    if !profile.is_empty() {
        line.extend(["--profile".to_owned(), profile.to_owned()]);
    }
    let package = options.package.trim();
    if !package.is_empty() {
        line.extend(["-p".to_owned(), package.to_owned()]);
    }
    let target = options.target.trim();
    match options.target_kind.as_str() {
        "" | "all" => {}
        "lib" => line.push("--lib".to_owned()),
        "doc" => line.push("--doc".to_owned()),
        "bin" | "test" | "bench" | "example" if target.is_empty() && kind == RUN && options.target_kind == "bin" => {}
        "bin" | "test" | "bench" | "example" => {
            if target.is_empty() {
                return Err(t!("rust-run-error-no-target", kind = options.target_kind.as_str()));
            }
            line.extend([format!("--{}", options.target_kind), target.to_owned()]);
        }
        other => return Err(t!("rust-run-error-target-kind", kind = other)),
    }
    let features: Vec<&str> =
        options.features.split([' ', ',']).map(str::trim).filter(|feature| !feature.is_empty()).collect();
    if !features.is_empty() {
        line.extend(["--features".to_owned(), features.join(",")]);
    }
    if options.all_features {
        line.push("--all-features".to_owned());
    }
    if options.no_default_features {
        line.push("--no-default-features".to_owned());
    }
    line.extend(args::split(tool_arguments));
    let mut after: Vec<String> = Vec::new();
    if kind != RUN {
        after.extend(args::split(&options.filter).into_iter().map(|name| libtest::harness_name(&name)));
        if options.exact && !options.filter.trim().is_empty() {
            after.push("--exact".to_owned());
        }
    }
    after.extend(args::split(program_arguments));
    if !after.is_empty() {
        line.push("--".to_owned());
        line.extend(after);
    }
    Ok(line)
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '=' | ':' | ',' | '+' | '@'));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

impl RunProvider for CargoRun {
    fn id(&self) -> &str {
        "cargo"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![
            kind(RUN, "rust-run-kind-run", &["bin", "example"], false),
            kind(TEST, "rust-run-kind-test", &["all", "lib", "bin", "test", "bench", "example", "doc"], true),
            kind(BENCH, "rust-run-kind-bench", &["all", "lib", "bin", "bench"], true),
            KindSpec {
                id: FILE.to_owned(),
                name_message: "rust-run-kind-file".to_owned(),
                fields: vec![
                    field("file", "rust-run-field-file", FieldKind::Path),
                    field("edition", "rust-run-field-edition", choice(&EDITIONS)),
                ],
                defaults: serde_json::to_value(FileOptions::default()).unwrap_or_default(),
            },
        ]
    }

    fn single_file(&self, path: &Path, text: &str) -> Vec<RunTarget> {
        targets::single_file(path, text)
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        targets::targets(&context.modules, path, text)
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
        let model = self.models.known(root).unwrap_or_default();
        targets::context(&model.modules, path, text, byte)
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        if configuration.kind == FILE {
            let project = self.toolchain.project(&context.root);
            let program = project.cargo.ok_or_else(|| t!("rust-model-no-cargo"))?;
            return file_lines(configuration, context, &program, project.env);
        }
        let args = cargo_args(
            &configuration.kind,
            &options(configuration),
            &configuration.common.tool_arguments,
            &configuration.common.program_arguments,
        )?;
        Ok(vec![self.cargo_line(configuration, context, args, Vec::new())?])
    }

    fn coverage(&self, configuration: &Configuration) -> Option<&dyn zen_run::CoverageProvider> {
        (configuration.kind == TEST).then_some(self as &dyn zen_run::CoverageProvider)
    }

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        let cwd = configuration.common.working_directory_in(&context.root);
        match configuration.kind.as_str() {
            TEST | BENCH => Box::new(LibtestParser::new(&cwd)),
            _ => Box::new(LinksParser { cwd }),
        }
    }

    fn rerun_failed(&self, configuration: &Configuration, failed: &[TestId]) -> Option<Configuration> {
        if configuration.kind != TEST || failed.is_empty() {
            return None;
        }
        let mut names: Vec<String> = Vec::new();
        for id in failed {
            if !names.contains(&id.test) {
                names.push(id.test.clone());
            }
        }
        let filter = names.iter().map(|name| quote(&libtest::harness_name(name))).collect::<Vec<_>>().join(" ");
        let options = CargoOptions { filter, exact: true, ..options(configuration) };
        Some(Configuration {
            id: String::new(),
            temporary: true,
            options: serde_json::to_value(options).ok()?,
            ..configuration.clone()
        })
    }
    fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
        vec![AnythingPrefix {
            prefix: "cargo".to_owned(),
            help_message: "rust-run-anything-help".to_owned(),
            commands: ANYTHING_COMMANDS.iter().map(|command| (*command).to_owned()).collect(),
        }]
    }

    fn anything(&self, words: &[String], cwd: &Path, context: &RunContext) -> Option<Result<Configuration, String>> {
        let (first, args) = words.split_first()?;
        if first != "cargo" {
            return None;
        }
        let project = self.toolchain.project(&context.root);
        Some(project.cargo.ok_or_else(|| t!("rust-model-no-cargo")).map(|cargo| {
            zen_run::external_tool::command_configuration(
                &zen_run::external_tool::joined(words),
                cargo,
                args.to_vec(),
                cwd,
                project.env,
            )
        }))
    }
}

impl CargoRun {
    fn cargo_line(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        args: Vec<String>,
        extra_env: Vec<(String, String)>,
    ) -> Result<CommandLine, String> {
        let project = self.toolchain.project(&context.root);
        let program = project.cargo.ok_or_else(|| t!("rust-model-no-cargo"))?;
        let display = std::iter::once("cargo".to_owned())
            .chain(args.iter().cloned())
            .map(|word| quote(&word))
            .collect::<Vec<_>>()
            .join(" ");
        let mut env = project.env;
        env.extend(extra_env);
        env.extend(configuration.common.env.iter().cloned());
        Ok(CommandLine { program, args, cwd: configuration.common.working_directory_in(&context.root), env, display })
    }
}

struct LinksParser {
    cwd: std::path::PathBuf,
}

impl OutputParser for LinksParser {
    fn line(&mut self, stream: zen_run::Stream, text: &str) -> zen_run::ParsedLine {
        let mut parsed = zen_run::ParsedLine::console(stream, text);
        if let Some(console) = &mut parsed.console {
            for link in &mut console.links {
                if !link.file.is_absolute() {
                    link.file = self.cwd.join(&link.file);
                }
            }
        }
        parsed
    }

    fn finish(&mut self) -> Vec<zen_run::TestEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, options: CargoOptions) -> String {
        cargo_args(kind, &options, "", "").unwrap().join(" ")
    }

    #[test]
    fn run_anything_offers_cargo_commands_and_leaves_other_words_alone() {
        let run = CargoRun::default();
        let prefixes = run.anything_prefixes();
        assert_eq!(prefixes[0].prefix, "cargo");
        assert!(prefixes[0].commands.iter().any(|command| command == "cargo test"));
        let words = args::split("go test");
        assert!(run.anything(&words, Path::new("/p"), &RunContext::default()).is_none());
    }

    #[test]
    fn a_scratch_file_is_compiled_by_rustc_into_the_runs_folder_and_run() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("scratches/scratch-1.rs");
        let targets = CargoRun::default().single_file(&scratch, "use std::fmt;\n\nfn main() {\n}\n");
        assert_eq!(
            targets.iter().map(|target| (target.line, target.kind)).collect::<Vec<_>>(),
            [(2, zen_run::TargetKind::Main)]
        );
        assert_eq!(targets[0].configuration.name, "Run scratch-1.rs");
        assert!(CargoRun::default().single_file(&scratch, "fn helper() {}\n").is_empty());
        assert!(CargoRun::default().single_file(&scratch.with_extension("go"), "fn main() {}\n").is_empty());
        assert_eq!(crate_name(Path::new("1st.rs")), "_1st");

        let context = RunContext {
            root: dir.path().join("project"),
            scratch: dir.path().join("zen-run-ab"),
            ..RunContext::default()
        };
        let mut configuration = targets[0].configuration.clone();
        configuration.common.program_arguments = "a 'b c'".to_owned();
        configuration.common.env = vec![("MODE".to_owned(), "dev".to_owned())];
        let cargo = Path::new("/toolchain/bin/cargo");
        let env = vec![("PATH".to_owned(), "/toolchain/bin".to_owned())];
        let lines = file_lines(&configuration, &context, cargo, env).unwrap();
        let program = context.scratch.join(crate::toolchain::rust::executable("scratch_1"));
        assert_eq!(lines[0].program, Path::new("/toolchain/bin").join(crate::toolchain::rust::executable("rustc")));
        let file = scratch.to_string_lossy().into_owned();
        let output = program.to_string_lossy().into_owned();
        assert_eq!(
            lines[0].args,
            ["--edition", "2024", "--crate-name", "scratch_1", "-o", output.as_str(), file.as_str()],
        );
        assert_eq!(lines[0].cwd, context.root, "the project's rust-toolchain.toml chooses the toolchain");
        assert_eq!(
            (lines[1].program.clone(), lines[1].args.clone()),
            (program, vec!["a".to_owned(), "b c".to_owned()])
        );
        assert!(lines[1].env.contains(&("MODE".to_owned(), "dev".to_owned())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&context.scratch).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the run's folder is private");
        }
        let empty = Configuration { kind: FILE.to_owned(), ..Configuration::default() };
        assert!(file_lines(&empty, &context, cargo, Vec::new()).is_err());
    }

    #[test]
    #[ignore = "compiles and runs a scratch file with the real cargo and rustc on PATH"]
    fn a_scratch_file_runs_with_the_real_rustc() {
        let Some(cargo) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|folder| folder.join(crate::toolchain::rust::executable("cargo")))
            .find(|cargo| cargo.is_file())
        else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("Application Support/scratches/scratch.rs");
        std::fs::create_dir_all(scratch.parent().unwrap()).unwrap();
        let text = "fn main() {\n    println!(\"hi from {}\", std::env::args().nth(1).unwrap());\n}\n";
        std::fs::write(&scratch, text).unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let context = RunContext { root, scratch: dir.path().join("zen-run-real"), ..RunContext::default() };
        let mut configuration = CargoRun::default().single_file(&scratch, text)[0].configuration.clone();
        configuration.common.program_arguments = "rust".to_owned();
        let mut output = String::new();
        for line in file_lines(&configuration, &context, &cargo, Vec::new()).unwrap() {
            let result =
                std::process::Command::new(&line.program).args(&line.args).current_dir(&line.cwd).output().unwrap();
            output.push_str(&String::from_utf8_lossy(&result.stdout));
            assert!(result.status.success(), "{}: {}", line.display, String::from_utf8_lossy(&result.stderr));
        }
        assert_eq!(output, "hi from rust\n");
    }

    #[test]
    fn options_become_cargos_arguments() {
        let test = CargoOptions {
            package: "calc".to_owned(),
            target_kind: "lib".to_owned(),
            filter: "tests::adds".to_owned(),
            exact: true,
            features: "fast, simd".to_owned(),
            no_default_features: true,
            profile: "release".to_owned(),
            ..CargoOptions::default()
        };
        assert_eq!(
            line(TEST, test),
            "test --no-fail-fast --profile release -p calc --lib --features fast,simd --no-default-features -- tests::adds --exact"
        );
        let run = CargoOptions {
            package: "app".to_owned(),
            target_kind: "bin".to_owned(),
            target: "app".to_owned(),
            all_features: true,
            ..CargoOptions::default()
        };
        assert_eq!(
            cargo_args(RUN, &run, "--locked", "--port 8080").unwrap().join(" "),
            "run -p app --bin app --all-features --locked -- --port 8080"
        );
        assert_eq!(line(RUN, CargoOptions { target_kind: "bin".to_owned(), ..CargoOptions::default() }), "run");
        assert_eq!(line(TEST, CargoOptions::default()), "test --no-fail-fast");
        let doc = CargoOptions {
            target_kind: "test".to_owned(),
            target: "outside".to_owned(),
            filter: "a b".to_owned(),
            ..CargoOptions::default()
        };
        assert_eq!(line(TEST, doc), "test --no-fail-fast --test outside -- a b");
        assert!(
            cargo_args(TEST, &CargoOptions { target_kind: "test".to_owned(), ..CargoOptions::default() }, "", "")
                .is_err()
        );
        assert!(cargo_args("cargo.other", &CargoOptions::default(), "", "").is_err());
        let bench =
            CargoOptions { target_kind: "bench".to_owned(), target: "speed".to_owned(), ..CargoOptions::default() };
        assert_eq!(line(BENCH, bench), "bench --bench speed");
    }

    #[test]
    fn failed_tests_are_run_again_by_their_exact_names() {
        let configuration = Configuration {
            id: "c1".to_owned(),
            name: "Test calc".to_owned(),
            kind: TEST.to_owned(),
            options: serde_json::to_value(CargoOptions {
                package: "calc".to_owned(),
                filter: "tests::".to_owned(),
                ..CargoOptions::default()
            })
            .unwrap(),
            ..Configuration::default()
        };
        let failed = [
            TestId { package: "calc".to_owned(), test: "tests::b".to_owned() },
            TestId { package: "calc (doc-tests)".to_owned(), test: libtest::tree_name("src/lib.rs - add (line 5)") },
        ];
        let again = CargoRun::default().rerun_failed(&configuration, &failed).unwrap();
        assert!(again.temporary && again.id.is_empty());
        let options: CargoOptions = serde_json::from_value(again.options).unwrap();
        assert!(options.exact);
        assert_eq!(
            cargo_args(TEST, &options, "", "").unwrap(),
            ["test", "--no-fail-fast", "-p", "calc", "--", "tests::b", "src/lib.rs - add (line 5)", "--exact"]
        );
        assert!(CargoRun::default().rerun_failed(&configuration, &[]).is_none());
    }
}
