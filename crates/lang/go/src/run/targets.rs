use std::path::{Path, PathBuf};

use zen_run::{Configuration, Module, RunContext, RunTarget, TargetKind};

use super::{BUILD, BuildOptions, GENERATE, GenerateOptions, TEST, TestOptions};
use crate::{Token, tokenize};

#[derive(Clone, Copy, Debug)]
struct Tok<'a> {
    token: Token,
    word: &'a str,
    start: usize,
    end: usize,
    line_start: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Func {
    name: String,
    kind: TargetKind,
    line: u32,
    start: usize,
    end: usize,
}

fn significant(text: &str) -> Vec<Tok<'_>> {
    let mut toks = Vec::new();
    let mut line_start = true;
    for (token, start, end) in tokenize(text) {
        match token {
            Token::Whitespace => line_start |= text[start..end].contains('\n'),
            Token::Comment => line_start |= text[start..end].contains('\n') || text[start..end].starts_with("//"),
            _ => {
                toks.push(Tok { token, word: &text[start..end], start, end, line_start });
                line_start = false;
            }
        }
    }
    toks
}

fn is(toks: &[Tok<'_>], at: usize, punct: char) -> bool {
    toks.get(at).is_some_and(|tok| tok.token == Token::Punct(punct))
}

fn skip_balanced(toks: &[Tok<'_>], open: usize) -> usize {
    let mut depth = 0usize;
    let mut at = open;
    while at < toks.len() {
        match toks[at].token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    at
}

fn exported_suffix(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| !rest.starts_with(|c: char| c.is_lowercase()))
}

fn takes_one(params: &[Tok<'_>], type_name: &str) -> bool {
    let tail = |from: usize| {
        params.len() == from + 4
            && params[from].token == Token::Punct('*')
            && params[from + 1].token == Token::Ident
            && params[from + 2].token == Token::Punct('.')
            && params[from + 3].word == type_name
    };
    tail(0) || (params.first().is_some_and(|tok| tok.token == Token::Ident) && tail(1))
}

fn classify(name: &str, params: &[Tok<'_>], test_file: bool, main_package: bool) -> Option<TargetKind> {
    if test_file {
        if exported_suffix(name, "Test") && takes_one(params, "T") {
            Some(TargetKind::Test)
        } else if exported_suffix(name, "Benchmark") && takes_one(params, "B") {
            Some(TargetKind::Benchmark)
        } else if exported_suffix(name, "Fuzz") && takes_one(params, "F") {
            Some(TargetKind::Fuzz)
        } else if exported_suffix(name, "Example") && params.is_empty() {
            Some(TargetKind::Example)
        } else {
            None
        }
    } else {
        (main_package && name == "main" && params.is_empty()).then_some(TargetKind::Main)
    }
}

fn functions(text: &str, test_file: bool) -> Vec<Func> {
    let toks = significant(text);
    let main_package =
        toks.iter().position(|tok| tok.word == "package").and_then(|at| toks.get(at + 1)).map(|tok| tok.word)
            == Some("main");
    let mut found = Vec::new();
    let mut at = 0;
    while at < toks.len() {
        let tok = toks[at];
        if tok.word == "func"
            && tok.token == Token::Ident
            && toks.get(at + 1).is_some_and(|next| next.token == Token::Ident)
        {
            let name = toks[at + 1].word;
            let mut next = at + 2;
            let generic = is(&toks, next, '[');
            if generic {
                next = skip_balanced(&toks, next);
            }
            if !is(&toks, next, '(') {
                at = next;
                continue;
            }
            let close = skip_balanced(&toks, next);
            let params = &toks[next + 1..close.saturating_sub(1).max(next + 1)];
            let mut scan = close;
            let mut body = None;
            while scan < toks.len() && !toks[scan].line_start {
                match toks[scan].token {
                    Token::Punct('{') if !matches!(toks[scan - 1].word, "struct" | "interface") => {
                        body = Some(scan);
                        break;
                    }
                    Token::Punct('(' | '[' | '{') => scan = skip_balanced(&toks, scan),
                    _ => scan += 1,
                }
            }
            let Some(open) = body else {
                at = scan.max(at + 1);
                continue;
            };
            let after = skip_balanced(&toks, open);
            let results = open > close;
            if !generic
                && !results
                && let Some(kind) = classify(name, params, test_file, main_package)
            {
                let line = text[..tok.start].matches('\n').count() as u32;
                let end = toks.get(after.saturating_sub(1)).map_or(text.len(), |last| last.end);
                found.push(Func { name: name.to_owned(), kind, line, start: tok.start, end });
            }
            at = after;
        } else if matches!(tok.token, Token::Punct('(' | '[' | '{')) {
            at = skip_balanced(&toks, at);
        } else {
            at += 1;
        }
    }
    found
}

struct Directive<'a> {
    line: u32,
    start: usize,
    end: usize,
    source: &'a str,
}

fn directives(text: &str) -> Vec<Directive<'_>> {
    tokenize(text)
        .into_iter()
        .filter_map(|(token, start, end)| {
            let source = text[start..end].trim_end();
            let at_line_start = start == 0 || text[..start].ends_with('\n');
            let spaced = source.strip_prefix("//go:generate").is_some_and(|rest| rest.starts_with([' ', '\t']));
            (token == Token::Comment && at_line_start && spaced).then(|| Directive {
                line: text[..start].matches('\n').count() as u32,
                start,
                end: start + source.len(),
                source,
            })
        })
        .collect()
}

fn generate_configuration(root: &Path, path: &Path, directive: &Directive<'_>) -> Configuration {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let options =
        GenerateOptions { file: relative.to_string_lossy().into_owned(), directive: directive.source.to_owned() };
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let name = format!("go generate {file}:{}", directive.line + 1);
    configuration(name, GENERATE, serde_json::to_value(options).unwrap_or_default())
}

fn is_test_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with("_test.go"))
}

fn is_go_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "go")
}

fn folder_name(folder: &Path) -> String {
    folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "main".to_owned())
}

pub(super) fn import_path(modules: &[Module], folder: &Path) -> String {
    modules
        .iter()
        .filter(|module| !module.name.is_empty() && folder.starts_with(&module.root))
        .max_by_key(|module| module.root.components().count())
        .map(|module| {
            let rest: Vec<String> = folder
                .strip_prefix(&module.root)
                .map(|relative| {
                    relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect()
                })
                .unwrap_or_default();
            std::iter::once(module.name.clone()).chain(rest).collect::<Vec<_>>().join("/")
        })
        .unwrap_or_default()
}

fn relative_package(root: &Path, path: &Path) -> (String, String) {
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = folder_name(&folder);
    let package = match folder.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => {
            let parts: Vec<String> =
                relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
            format!("./{}", parts.join("/"))
        }
        Err(_) => folder.to_string_lossy().into_owned(),
    };
    (package, name)
}

fn configuration(name: String, kind: &str, options: serde_json::Value) -> Configuration {
    Configuration { name, kind: kind.to_owned(), options, ..Configuration::default() }
}

fn build_configuration(package: String, name: &str) -> Configuration {
    let options = BuildOptions { package, ..BuildOptions::default() };
    configuration(format!("go build {name}"), BUILD, serde_json::to_value(options).unwrap_or_default())
}

fn package_tests(package: String, name: &str) -> Configuration {
    let options = TestOptions { package, ..TestOptions::default() };
    configuration(format!("go test {name}"), TEST, serde_json::to_value(options).unwrap_or_default())
}

fn function_tests(package: String, name: &str, func: &Func) -> Configuration {
    let exact = format!("^{}$", func.name);
    let options = if func.kind == TargetKind::Benchmark {
        TestOptions { package, pattern: "^$".to_owned(), bench: exact, ..TestOptions::default() }
    } else {
        TestOptions { package, pattern: exact, ..TestOptions::default() }
    };
    configuration(format!("{} ({name})", func.name), TEST, serde_json::to_value(options).unwrap_or_default())
}

fn target_configuration(package: String, name: &str, func: &Func) -> Configuration {
    match func.kind {
        TargetKind::Main => build_configuration(package, name),
        _ => function_tests(package, name, func),
    }
}

pub(super) fn targets(context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
    if !is_go_file(path) {
        return Vec::new();
    }
    let (package, name) = relative_package(&context.root, path);
    let reported = path.parent().map(|folder| import_path(&context.modules, folder)).unwrap_or_default();
    functions(text, is_test_file(path))
        .into_iter()
        .map(|func| RunTarget {
            line: func.line,
            kind: func.kind,
            name: func.name.clone(),
            package: reported.clone(),
            configuration: target_configuration(package.clone(), &name, &func),
        })
        .chain(directives(text).iter().map(|directive| RunTarget {
            line: directive.line,
            kind: TargetKind::Generate,
            name: directive.source.trim_start_matches("//go:generate").trim().to_owned(),
            package: reported.clone(),
            configuration: generate_configuration(&context.root, path, directive),
        }))
        .collect()
}

pub(super) fn single_file(path: &Path, text: &str) -> Vec<RunTarget> {
    if !is_go_file(path) || is_test_file(path) {
        return Vec::new();
    }
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let options = BuildOptions {
        run_kind: "file".to_owned(),
        files: zen_run::external_tool::quoted(&path.to_string_lossy()),
        run_after_build: true,
        ..BuildOptions::default()
    };
    let configuration =
        configuration(format!("go build {file}"), BUILD, serde_json::to_value(options).unwrap_or_default());
    functions(text, false)
        .into_iter()
        .filter(|func| func.kind == TargetKind::Main)
        .map(|func| RunTarget {
            line: func.line,
            kind: TargetKind::Main,
            name: func.name,
            package: String::new(),
            configuration: configuration.clone(),
        })
        .collect()
}

pub(super) fn context(root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
    if !is_go_file(path) {
        return None;
    }
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    if let Some(directive) = directives(text).iter().find(|directive| directive.start <= byte && byte <= directive.end)
    {
        return Some(generate_configuration(root, path, directive));
    }
    let (package, name) = relative_package(root, path);
    let test_file = is_test_file(path);
    let funcs = functions(text, test_file);
    if test_file {
        return Some(match funcs.iter().find(|func| func.start <= byte && byte <= func.end) {
            Some(func) => function_tests(package, &name, func),
            None => package_tests(package, &name),
        });
    }
    let main_package = significant(text).windows(2).any(|pair| pair[0].word == "package" && pair[1].word == "main");
    main_package.then(|| build_configuration(package, &name))
}

pub(super) fn folder(root: &Path, text: &str) -> PathBuf {
    normalize(&root.join(text))
}

pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, test_file: bool) -> Vec<(String, TargetKind, u32)> {
        functions(text, test_file).into_iter().map(|func| (func.name, func.kind, func.line)).collect()
    }

    #[test]
    fn test_functions_follow_the_naming_rules() {
        let text = "package x\n\nimport \"testing\"\n\nfunc TestA(t *testing.T) {}\nfunc Testing(t *testing.T) {}\nfunc Test(t *testing.T) {}\nfunc Test_x(t *testing.T) {}\nfunc TestG[T any](t *testing.T) {}\nfunc TestR(t *testing.T) error { return nil }\nfunc TestB(b *testing.B) {}\nfunc BenchmarkB(b *testing.B) {}\nfunc Benchmarkb(b *testing.B) {}\nfunc FuzzF(f *testing.F) {}\nfunc Example() {}\nfunc ExampleX_y() {}\nfunc Exampled() {}\nfunc ExampleArg(x int) {}\nfunc (s *S) TestM(t *testing.T) {}\nfunc TestU(*testing.T) {}\n";
        let found: Vec<String> = kinds(text, true).into_iter().map(|(name, _, _)| name).collect();
        assert_eq!(found, ["TestA", "Test", "Test_x", "BenchmarkB", "FuzzF", "Example", "ExampleX_y", "TestU"]);
        assert!(kinds(text, false).is_empty());
    }

    #[test]
    fn main_needs_package_main_and_no_signature() {
        assert_eq!(kinds("package main\n\nfunc main() {\n}\n", false), [("main".to_owned(), TargetKind::Main, 2)]);
        assert!(kinds("package lib\n\nfunc main() {}\n", false).is_empty());
        assert!(kinds("package main\n\nfunc main(x int) {}\n", false).is_empty());
        assert!(kinds("package main\n\nfunc main() {}\n", true).is_empty());
        assert_eq!(
            kinds(
                "package main\n\nvar x = struct{}{}\n\nfunc f() interface{} { return nil }\n\nfunc main() {}\n",
                false
            )
            .len(),
            1
        );
    }

    #[test]
    fn folders_are_normalized() {
        assert_eq!(folder(Path::new("/p"), "./a/../b"), PathBuf::from("/p/b"));
        assert_eq!(folder(Path::new("/p"), ""), PathBuf::from("/p"));
        assert_eq!(folder(Path::new("/p"), "/abs/x"), PathBuf::from("/abs/x"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn go_generate_lines_get_their_own_targets_and_nothing_that_only_looks_like_one() {
        let text = "package x\n\n//go:generate stringer -type=Color  \n// go:generate no\n  //go:generate indented\n//go:generated no\nvar s = `\n//go:generate in a string\n`\n//go:generate go run gen.go\n";
        let found = directives(text);
        let lines: Vec<(u32, &str)> = found.iter().map(|found| (found.line, found.source)).collect();
        assert_eq!(lines, [(2, "//go:generate stringer -type=Color"), (9, "//go:generate go run gen.go")]);
    }

    #[test]
    fn a_go_generate_line_runs_only_that_directive_in_its_files_folder() {
        let root = Path::new("/work/demo");
        let path = root.join("api/gen.go");
        let text = "package api\n\n//go:generate echo (one)\n";
        let context = RunContext { root: root.to_owned(), ..RunContext::default() };
        let found = targets(&context, &path, text);
        assert_eq!(found[0].kind, TargetKind::Generate);
        assert_eq!(found[0].name, "echo (one)");
        assert_eq!(found[0].line, 2);
        let offset = text.find("echo").unwrap();
        let at_caret = super::context(root, &path, text, offset).unwrap();
        assert_eq!(at_caret, found[0].configuration, "the context at the line runs the same");
        assert_eq!(at_caret.name, "go generate gen.go:3");
        assert_eq!(at_caret.kind, GENERATE);
    }
}
