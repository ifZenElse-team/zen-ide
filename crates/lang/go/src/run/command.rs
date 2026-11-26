use std::path::{Path, PathBuf};

use zen_i18n::t;
use zen_run::{CommandLine, Configuration, Module, OutputParser, ParsedLine, RunContext, Stream, TestEvent, args};

use super::targets::folder;
use super::{BuildOptions, GenerateOptions, TestOptions};

pub(super) struct Target {
    pub(super) cwd: PathBuf,
    pub(super) args: Vec<String>,
    pub(super) name: String,
}

const OVERLAY_FOLDER: &str = "zen_scratch";

pub(super) fn is_folder_form(package: &str) -> bool {
    package.starts_with('.') || Path::new(package).is_absolute()
}

pub(super) fn module_folder(import_path: &str, modules: &[Module]) -> Option<(PathBuf, PathBuf)> {
    modules
        .iter()
        .filter(|module| {
            !module.name.is_empty()
                && (import_path == module.name || import_path.starts_with(&format!("{}/", module.name)))
        })
        .max_by_key(|module| module.name.len())
        .map(|module| {
            let rest = import_path[module.name.len()..].trim_start_matches('/');
            (module.root.clone(), module.root.join(rest))
        })
}

fn package_target(package: &str, context: &RunContext) -> Result<Target, String> {
    let package = package.trim();
    if package.is_empty() {
        return Err(t!("go-run-error-no-package"));
    }
    if is_folder_form(package) {
        let cwd = folder(&context.root, package);
        let name = cwd.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        return Ok(Target { cwd, args: vec![".".to_owned()], name });
    }
    let cwd = module_folder(package, &context.modules).map_or_else(|| context.root.clone(), |(module, _)| module);
    let name = package.rsplit('/').next().unwrap_or(package).to_owned();
    Ok(Target { cwd, args: vec![package.to_owned()], name })
}

fn borrowed_module<'a>(text: &str, context: &'a RunContext) -> Option<&'a Module> {
    let local = |path: &str, module: &Module| path == module.name || path.starts_with(&format!("{}/", module.name));
    let inside: Vec<&Module> = context
        .modules
        .iter()
        .filter(|module| !module.name.is_empty() && module.root.starts_with(&context.root))
        .collect();
    let imports = crate::imports::imported_paths(text);
    let outside_std = |path: &String| path.split('/').next().is_some_and(|first| first.contains('.'));
    let owner = imports.iter().find_map(|path| inside.iter().find(|module| local(path, module)));
    let first = inside.iter().min_by_key(|module| module.root.components().count());
    owner.or(first.filter(|_| imports.iter().any(outside_std))).copied()
}

fn real_root(module: &Module) -> PathBuf {
    module.root.canonicalize().unwrap_or_else(|_| module.root.clone())
}

fn scratch_package(stem: &str) -> String {
    let safe = |c: char| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' };
    stem.chars().map(safe).collect::<String>().trim_start_matches('_').to_owned()
}

pub(super) fn scratch_home<'a>(file: &Path, text: &str, context: &'a RunContext) -> Option<(&'a Module, PathBuf)> {
    if file.extension()? != "go" || context.modules.iter().any(|module| file.starts_with(&module.root)) {
        return None;
    }
    let module = borrowed_module(text, context)?;
    let folder = scratch_package(&file.file_stem()?.to_string_lossy());
    Some((module, real_root(module).join(OVERLAY_FOLDER).join(folder).join(file.file_name()?)))
}

fn overlay_target(file: &Path, context: &RunContext) -> Option<Target> {
    let (module, home) = scratch_home(file, &std::fs::read_to_string(file).ok()?, context)?;
    let overlay = scratch(context).join("overlay.json");
    zen_run::create_private_folder(&scratch(context)).ok()?;
    let replace = serde_json::json!({ "Replace": { home.to_string_lossy(): file.to_string_lossy() } });
    std::fs::write(&overlay, replace.to_string()).ok()?;
    let name = file.file_stem()?.to_string_lossy().into_owned();
    let package = home.parent()?.strip_prefix(real_root(module)).ok()?;
    let args = vec![format!("-overlay={}", overlay.display()), format!("./{}", package.display())];
    Some(Target { cwd: module.root.clone(), args, name })
}

fn files_target(files: &str, context: &RunContext) -> Result<Target, String> {
    let files: Vec<PathBuf> = args::split(files).iter().map(|file| folder(&context.root, file)).collect();
    if let [file] = files.as_slice()
        && let Some(target) = overlay_target(file, context)
    {
        return Ok(target);
    }
    let first = files.first().ok_or_else(|| t!("go-run-error-no-files"))?;
    let cwd = first.parent().map(Path::to_path_buf).unwrap_or_else(|| context.root.clone());
    let args = files
        .iter()
        .map(|file| match file.strip_prefix(&cwd) {
            Ok(relative) => relative.to_string_lossy().into_owned(),
            Err(_) => file.to_string_lossy().into_owned(),
        })
        .collect();
    let name = first.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Target { cwd, args, name })
}

fn folder_target(directory: &str, recursive: bool, context: &RunContext) -> Target {
    let cwd = folder(&context.root, directory.trim());
    let name = cwd.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    Target { cwd, args: vec![if recursive { "./..." } else { "." }.to_owned()], name }
}

pub(super) fn build_target(options: &BuildOptions, context: &RunContext) -> Result<Target, String> {
    match options.run_kind.as_str() {
        "package" | "" => package_target(&options.package, context),
        "file" => files_target(&options.files, context),
        "directory" => Ok(folder_target(&options.directory, false, context)),
        other => Err(t!("go-run-error-target", target = other)),
    }
}

pub(super) fn test_target(options: &TestOptions, context: &RunContext) -> Result<Target, String> {
    match options.test_kind.as_str() {
        "package" | "" => package_target(&options.package, context),
        "file" => files_target(&options.files, context),
        "directory" => Ok(folder_target(&options.directory, true, context)),
        other => Err(t!("go-run-error-target", target = other)),
    }
}

pub(super) fn test_folder(options: &TestOptions, context: &RunContext) -> Result<PathBuf, String> {
    test_target(options, context).map(|target| target.cwd)
}

pub(super) fn package_folder(options: &TestOptions, context: &RunContext) -> PathBuf {
    let package = options.package.trim();
    match options.test_kind.as_str() {
        "package" | "" if !package.is_empty() && !is_folder_form(package) => {
            module_folder(package, &context.modules).map_or_else(|| context.root.clone(), |(_, folder)| folder)
        }
        _ => test_folder(options, context).unwrap_or_else(|_| context.root.clone()),
    }
}

pub(super) fn test_package(options: &TestOptions, context: &RunContext) -> String {
    let package = options.package.trim();
    match options.test_kind.as_str() {
        "file" => "command-line-arguments".to_owned(),
        "package" | "" if !package.is_empty() && !is_folder_form(package) => package.to_owned(),
        _ => {
            let folder = package_folder(options, context);
            let path = super::targets::import_path(&context.modules, &folder);
            if path.is_empty() { folder.to_string_lossy().into_owned() } else { path }
        }
    }
}

pub(super) fn tool(context: &RunContext) -> Result<PathBuf, String> {
    if context.env.tool.as_os_str().is_empty() { Err(t!("go-run-error-no-go")) } else { Ok(context.env.tool.clone()) }
}

pub(super) fn environment(configuration: &Configuration, context: &RunContext) -> Vec<(String, String)> {
    context.env.env.iter().chain(&configuration.common.env).cloned().collect()
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '=' | ':' | ',' | '+' | '@'));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

pub(super) fn display(program: &Path, args: &[String]) -> String {
    std::iter::once(program.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|word| quote(&word))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn go_line(program: PathBuf, args: Vec<String>, cwd: PathBuf, env: Vec<(String, String)>) -> CommandLine {
    let display = format!("{} #gosetup", display(&program, &args));
    CommandLine { program, args, cwd, env, display }
}

pub(super) fn go_command(command: &str, directory: &str, context: &RunContext) -> Result<Vec<CommandLine>, String> {
    let args = args::split(command);
    if args.is_empty() {
        return Err(t!("go-run-error-no-command"));
    }
    let go = tool(context)?;
    let cwd = folder(&context.root, directory.trim());
    Ok(vec![go_line(go, args, cwd, context.env.env.clone())])
}

pub(super) fn generate_folder(options: &GenerateOptions, context: &RunContext) -> (PathBuf, String) {
    let file = folder(&context.root, options.file.trim());
    let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    (file.parent().map_or_else(|| context.root.clone(), Path::to_owned), name)
}

pub(super) fn generate(
    configuration: &Configuration,
    options: &GenerateOptions,
    context: &RunContext,
) -> Result<Vec<CommandLine>, String> {
    let go = tool(context)?;
    if options.file.trim().is_empty() {
        return Err(t!("go-run-error-no-generate-file"));
    }
    let (cwd, name) = generate_folder(options, context);
    let mut go_args = vec!["generate".to_owned()];
    if !options.directive.is_empty() {
        go_args.extend(["-run".to_owned(), format!("^{}$", super::quote_meta(&options.directive))]);
    }
    go_args.extend(args::split(&configuration.common.tool_arguments));
    go_args.push(name);
    Ok(vec![go_line(go, go_args, cwd, environment(configuration, context))])
}

pub(super) fn scratch(context: &RunContext) -> PathBuf {
    if context.scratch.as_os_str().is_empty() { std::env::temp_dir().join("zen-run") } else { context.scratch.clone() }
}

pub(super) fn binary(options: &BuildOptions, name: &str, context: &RunContext) -> Option<PathBuf> {
    let file = format!("{}{}", if name.is_empty() { "main" } else { name }, std::env::consts::EXE_SUFFIX);
    if !options.output_directory.trim().is_empty() {
        Some(folder(&context.root, options.output_directory.trim()).join(file))
    } else if options.run_after_build {
        Some(scratch(context).join(file))
    } else {
        None
    }
}

pub(super) fn temporary_binary(options: &BuildOptions, context: &RunContext) -> Option<PathBuf> {
    if !options.run_after_build || !options.output_directory.trim().is_empty() {
        return None;
    }
    let target = build_target(options, context).ok()?;
    binary(options, &target.name, context)
}

pub(super) fn build(
    configuration: &Configuration,
    options: &BuildOptions,
    context: &RunContext,
) -> Result<Vec<CommandLine>, String> {
    let go = tool(context)?;
    let target = build_target(options, context)?;
    let env = environment(configuration, context);
    let output = binary(options, &target.name, context);
    let mut build_args = vec!["build".to_owned()];
    if let Some(output) = &output {
        build_args.extend(["-o".to_owned(), output.to_string_lossy().into_owned()]);
    }
    build_args.extend(args::split(&configuration.common.tool_arguments));
    build_args.extend(target.args);
    let mut lines = vec![go_line(go, build_args, target.cwd, env.clone())];
    if options.run_after_build
        && let Some(program) = output
    {
        let program_args = args::split(&configuration.common.program_arguments);
        let display = display(&program, &program_args);
        let cwd = configuration.common.working_directory_in(&context.root);
        lines.push(CommandLine { program, args: program_args, cwd, env, display });
    }
    Ok(lines)
}

pub(super) fn test(
    configuration: &Configuration,
    options: &TestOptions,
    context: &RunContext,
) -> Result<Vec<CommandLine>, String> {
    test_with(configuration, options, context, Vec::new())
}

pub(super) fn test_with(
    configuration: &Configuration,
    options: &TestOptions,
    context: &RunContext,
    extra: Vec<String>,
) -> Result<Vec<CommandLine>, String> {
    let go = tool(context)?;
    let target = test_target(options, context)?;
    let mut test_args = vec!["test".to_owned(), "-json".to_owned()];
    test_args.extend(extra);
    if !options.pattern.is_empty() {
        test_args.extend(["-run".to_owned(), options.pattern.clone()]);
    }
    if !options.bench.is_empty() {
        test_args.extend(["-bench".to_owned(), options.bench.clone()]);
    }
    let tool_arguments = args::split(&configuration.common.tool_arguments);
    if !options.bench.is_empty() && !tool_arguments.iter().any(|word| word.trim_start_matches('-') == "benchmem") {
        test_args.push("-benchmem".to_owned());
    }
    test_args.extend(tool_arguments);
    test_args.extend(target.args);
    let program_args = args::split(&configuration.common.program_arguments);
    if !program_args.is_empty() {
        test_args.push("-args".to_owned());
        test_args.extend(program_args);
    }
    Ok(vec![go_line(go, test_args, target.cwd, environment(configuration, context))])
}

pub(super) fn resolve_links(parsed: &mut ParsedLine, cwd: &Path, bare: &Path) {
    if let Some(console) = &mut parsed.console {
        for link in &mut console.links {
            if link.file.is_absolute() {
                continue;
            }
            let base = if link.is_bare() { bare } else { cwd };
            link.file = super::targets::normalize(&base.join(&link.file));
        }
    }
}

pub(super) struct BuildParser {
    cwd: PathBuf,
    binary: Option<PathBuf>,
}

pub(super) fn generate_parser(cwd: PathBuf) -> BuildParser {
    BuildParser { cwd, binary: None }
}

pub(super) fn build_parser(options: &BuildOptions, context: &RunContext) -> BuildParser {
    let cwd = build_target(options, context).map_or_else(|_| context.root.clone(), |target| target.cwd);
    BuildParser { cwd, binary: temporary_binary(options, context) }
}

impl OutputParser for BuildParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut parsed = ParsedLine::console(stream, text);
        resolve_links(&mut parsed, &self.cwd, &self.cwd);
        parsed
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        if let Some(binary) = self.binary.take() {
            let _ = std::fs::remove_file(binary);
        }
        Vec::new()
    }
}
