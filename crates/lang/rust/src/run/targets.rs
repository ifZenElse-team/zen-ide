use std::path::{Path, PathBuf};

use zen_run::{Configuration, Module, RunTarget, TargetKind};

use super::libtest::{binary_label, tree_name};
use super::{BENCH, CargoOptions, RUN, TEST};
use crate::lexer::{Token, tokenize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Item {
    pub(super) kind: TargetKind,
    pub(super) path: Vec<String>,
    pub(super) module: bool,
    pub(super) line: u32,
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Copy)]
struct Tok<'a> {
    token: Token,
    word: &'a str,
    start: usize,
    end: usize,
}

fn significant(text: &str) -> Vec<Tok<'_>> {
    tokenize(text)
        .into_iter()
        .filter(|(token, _, _)| {
            !matches!(token, Token::Whitespace | Token::LineComment { .. } | Token::BlockComment { .. })
        })
        .map(|(token, start, end)| Tok { token, word: &text[start..end], start, end })
        .collect()
}

fn after_balanced(toks: &[Tok<'_>], open: usize) -> usize {
    let mut depth = 0usize;
    for (at, tok) in toks.iter().enumerate().skip(open) {
        match tok.token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

fn attribute_path(toks: &[Tok<'_>]) -> String {
    toks.iter()
        .take_while(|tok| !matches!(tok.token, Token::Punct('(' | '=' | '[')))
        .map(|tok| tok.word)
        .collect::<String>()
}

fn is_test_attribute(path: &str) -> bool {
    let last = path.rsplit("::").next().unwrap_or(path);
    matches!(last, "test" | "rstest" | "test_case")
}

pub(super) fn items(text: &str) -> Vec<Item> {
    let toks = significant(text);
    let line_of = |byte: usize| text[..byte].matches('\n').count() as u32;
    let mut found = Vec::new();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let mut attributes_start: Option<usize> = None;
    let mut modules: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut at = 0;
    while at < toks.len() {
        let tok = toks[at];
        match tok.token {
            Token::Punct('#') if matches!(toks.get(at + 1).map(|next| next.token), Some(Token::Punct('['))) => {
                let close = after_balanced(&toks, at + 1);
                let inner = &toks[(at + 2).min(close)..close.saturating_sub(1).max(at + 2)];
                attributes.push((attribute_path(inner), inner.iter().map(|tok| tok.word).collect()));
                attributes_start.get_or_insert(tok.start);
                at = close;
                continue;
            }
            Token::Punct('#') if matches!(toks.get(at + 1).map(|next| next.token), Some(Token::Punct('!'))) => {
                at = after_balanced(&toks, at + 2);
                continue;
            }
            Token::Punct('{') => {
                depth += 1;
                attributes.clear();
                attributes_start = None;
            }
            Token::Punct('}') => {
                depth = depth.saturating_sub(1);
                while modules.last().is_some_and(|(_, body)| *body > depth) {
                    modules.pop();
                }
                attributes.clear();
                attributes_start = None;
            }
            Token::Punct(';') => {
                attributes.clear();
                attributes_start = None;
            }
            Token::Ident if tok.word == "mod" => {
                let name = toks.get(at + 1).filter(|next| next.token == Token::Ident).map(|next| next.word);
                if let Some(name) = name
                    && matches!(toks.get(at + 2).map(|next| next.token), Some(Token::Punct('{')))
                {
                    let tests = attributes.iter().any(|(path, inner)| path == "cfg" && inner.contains("test"));
                    let close = after_balanced(&toks, at + 2);
                    if tests {
                        let mut path: Vec<String> = modules.iter().map(|(name, _)| name.clone()).collect();
                        path.push(name.to_owned());
                        found.push(Item {
                            kind: TargetKind::Test,
                            path,
                            module: true,
                            line: line_of(tok.start),
                            start: attributes_start.unwrap_or(tok.start),
                            end: toks.get(close.saturating_sub(1)).map_or(text.len(), |last| last.end),
                        });
                    }
                    modules.push((name.to_owned(), depth + 1));
                    depth += 1;
                    attributes.clear();
                    attributes_start = None;
                    at += 3;
                    continue;
                }
            }
            Token::Ident if tok.word == "fn" => {
                let name = toks.get(at + 1).filter(|next| next.token == Token::Ident).map(|next| next.word);
                let mut scan = at + 2;
                let mut body = None;
                while scan < toks.len() {
                    match toks[scan].token {
                        Token::Punct('{') => {
                            body = Some(scan);
                            break;
                        }
                        Token::Punct(';') => break,
                        Token::Punct('(' | '[') => scan = after_balanced(&toks, scan),
                        _ => scan += 1,
                    }
                }
                if let (Some(name), Some(open)) = (name, body) {
                    let test = attributes.iter().any(|(path, _)| is_test_attribute(path));
                    let bench = attributes.iter().any(|(path, _)| path == "bench");
                    let kind = if test {
                        Some(TargetKind::Test)
                    } else if bench {
                        Some(TargetKind::Benchmark)
                    } else if name == "main" && modules.is_empty() && depth == 0 {
                        Some(TargetKind::Main)
                    } else {
                        None
                    };
                    let close = after_balanced(&toks, open);
                    if let Some(kind) = kind {
                        let mut path: Vec<String> = modules.iter().map(|(name, _)| name.clone()).collect();
                        path.push(name.to_owned());
                        found.push(Item {
                            kind,
                            path,
                            module: false,
                            line: line_of(tok.start),
                            start: attributes_start.unwrap_or(tok.start),
                            end: toks.get(close.saturating_sub(1)).map_or(text.len(), |last| last.end),
                        });
                    }
                    attributes.clear();
                    attributes_start = None;
                    at = close;
                    continue;
                }
            }
            _ => {}
        }
        at += 1;
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) package: String,
    pub(super) kind: String,
    pub(super) target: String,
    pub(super) modules: Vec<String>,
    pub(super) root: bool,
    pub(super) label: String,
}

fn module_targets(module: &Module) -> Vec<(String, String, PathBuf)> {
    module.details["targets"]
        .as_array()
        .map(|targets| {
            targets
                .iter()
                .filter_map(|target| {
                    let kind = target["kind"].as_array()?.first()?.as_str()?;
                    let kind = if matches!(kind, "rlib" | "dylib" | "staticlib" | "cdylib" | "proc-macro") {
                        "lib"
                    } else {
                        kind
                    };
                    Some((
                        target["name"].as_str()?.to_owned(),
                        kind.to_owned(),
                        PathBuf::from(target["srcPath"].as_str()?),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn discovered(root: &Path, path: &Path) -> Option<(String, String, PathBuf)> {
    let parts: Vec<String> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let (kind, rest) = match parts.as_slice() {
        ["tests", rest @ ..] => ("test", rest),
        ["benches", rest @ ..] => ("bench", rest),
        ["examples", rest @ ..] => ("example", rest),
        ["src", "bin", rest @ ..] => ("bin", rest),
        _ => return None,
    };
    let name = match rest {
        [file] => file.strip_suffix(".rs")?,
        [folder, "main.rs"] => folder,
        _ => return None,
    };
    Some((name.to_owned(), kind.to_owned(), path.to_owned()))
}

pub(super) fn place(modules: &[Module], path: &Path) -> Option<Place> {
    let module = modules
        .iter()
        .filter(|module| path.starts_with(&module.root))
        .max_by_key(|module| module.root.components().count())?;
    let mut targets: Vec<(String, String, PathBuf)> = module_targets(module)
        .into_iter()
        .filter(|(_, kind, _)| matches!(kind.as_str(), "lib" | "bin" | "test" | "bench" | "example"))
        .collect();
    if !targets.iter().any(|(_, _, source)| source == path)
        && let Some(target) = discovered(&module.root, path)
    {
        targets.push(target);
    }
    let (name, kind, source) = targets
        .iter()
        .find(|(_, _, source)| source == path)
        .or_else(|| {
            targets
                .iter()
                .filter(|(_, _, source)| source.parent().is_some_and(|folder| path.starts_with(folder)))
                .max_by_key(|(_, kind, source)| (source.components().count(), kind == "lib"))
        })?
        .clone();
    let root = source == path;
    let folder = source.parent()?;
    let mut modules: Vec<String> = Vec::new();
    if !root {
        let relative = path.strip_prefix(folder).ok()?.with_extension("");
        modules = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        if modules.last().is_some_and(|last| last == "mod") {
            modules.pop();
        }
    }
    let krate = name.replace('-', "_");
    let relative_source = source.strip_prefix(&module.root).unwrap_or(&source).to_string_lossy().replace('\\', "/");
    let label = binary_label(&krate, &relative_source);
    Some(Place { package: module.name.clone(), kind, target: name, modules, root, label })
}

fn configuration(name: String, kind: &str, options: &CargoOptions) -> Configuration {
    Configuration {
        name,
        kind: kind.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

pub(super) fn item_configuration(place: &Place, item: &Item) -> Configuration {
    let full = place.modules.iter().chain(&item.path).cloned().collect::<Vec<_>>().join("::");
    let base = CargoOptions {
        package: place.package.clone(),
        target_kind: place.kind.clone(),
        target: place.target.clone(),
        ..CargoOptions::default()
    };
    match item.kind {
        TargetKind::Main => configuration(format!("Run {}", place.target), RUN, &base),
        TargetKind::Benchmark => {
            let options = CargoOptions { filter: full.clone(), exact: true, ..base };
            configuration(format!("Bench {full}"), BENCH, &options)
        }
        _ if item.module => {
            let options = CargoOptions { filter: format!("{full}::"), ..base };
            configuration(format!("Test {full}"), TEST, &options)
        }
        _ => {
            let options = CargoOptions { filter: full.clone(), exact: true, ..base };
            configuration(format!("Test {full}"), TEST, &options)
        }
    }
}

fn target_configuration(place: &Place) -> Option<Configuration> {
    let options = CargoOptions {
        package: place.package.clone(),
        target_kind: place.kind.clone(),
        target: place.target.clone(),
        ..CargoOptions::default()
    };
    match place.kind.as_str() {
        "test" => Some(configuration(format!("Test {}", place.target), TEST, &options)),
        "bench" => Some(configuration(format!("Bench {}", place.target), BENCH, &options)),
        _ => None,
    }
}

pub(super) fn targets(modules: &[Module], path: &Path, text: &str) -> Vec<RunTarget> {
    if path.extension().is_none_or(|extension| extension != "rs") {
        return Vec::new();
    }
    let Some(place) = place(modules, path) else { return Vec::new() };
    items(text)
        .into_iter()
        .filter(|item| {
            item.kind != TargetKind::Main || (place.root && matches!(place.kind.as_str(), "bin" | "example"))
        })
        .map(|item| {
            let name = place.modules.iter().chain(&item.path).cloned().collect::<Vec<_>>().join("::");
            RunTarget {
                line: item.line,
                kind: item.kind,
                name: if item.kind == TargetKind::Main { place.target.clone() } else { tree_name(&name) },
                package: place.label.clone(),
                configuration: item_configuration(&place, &item),
            }
        })
        .collect()
}

pub(super) fn single_file(path: &Path, text: &str) -> Vec<RunTarget> {
    if path.extension().is_none_or(|extension| extension != "rs") {
        return Vec::new();
    }
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let options = super::FileOptions { file: path.to_string_lossy().into_owned(), ..super::FileOptions::default() };
    let configuration = Configuration {
        name: format!("Run {file}"),
        kind: super::FILE.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    };
    items(text)
        .into_iter()
        .filter(|item| item.kind == TargetKind::Main)
        .map(|item| RunTarget {
            line: item.line,
            kind: TargetKind::Main,
            name: "main".to_owned(),
            package: String::new(),
            configuration: configuration.clone(),
        })
        .collect()
}

pub(super) fn context(modules: &[Module], path: &Path, text: &str, byte: usize) -> Option<Configuration> {
    if path.extension().is_none_or(|extension| extension != "rs") {
        return None;
    }
    let place = place(modules, path)?;
    let items = items(text);
    let around = |module: bool| {
        items
            .iter()
            .filter(|item| item.module == module && item.kind != TargetKind::Main)
            .filter(|item| item.start <= byte && byte <= item.end)
            .max_by_key(|item| item.start)
    };
    if let Some(item) = around(false).or_else(|| around(true)) {
        return Some(item_configuration(&place, item));
    }
    if place.root
        && matches!(place.kind.as_str(), "bin" | "example")
        && let Some(main) = items.iter().find(|item| item.kind == TargetKind::Main)
    {
        return Some(item_configuration(&place, main));
    }
    target_configuration(&place)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const LIB: &str = "//! Arithmetic.\n\n/// Adds.\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds() {\n        assert_eq!(add(2, 2), 4);\n    }\n\n    #[tokio::test(flavor = \"multi_thread\")]\n    async fn later() {}\n\n    #[test]\n    #[ignore]\n    fn slow() {}\n\n    mod deep {\n        #[test]\n        fn inner() {}\n    }\n\n    fn helper() {}\n}\n\n#[bench]\nfn speed(b: &mut Bencher) {}\n";

    fn module() -> Module {
        Module {
            name: "calc".to_owned(),
            root: PathBuf::from("/w/calc"),
            details: json!({ "targets": [
                { "name": "calc", "kind": ["lib"], "srcPath": "/w/calc/src/lib.rs" },
                { "name": "calc-cli", "kind": ["bin"], "srcPath": "/w/calc/src/main.rs" },
                { "name": "tool", "kind": ["bin"], "srcPath": "/w/calc/src/bin/tool.rs" },
                { "name": "outside", "kind": ["test"], "srcPath": "/w/calc/tests/outside.rs" },
                { "name": "build-script-build", "kind": ["custom-build"], "srcPath": "/w/calc/build.rs" }
            ] }),
            ..Module::default()
        }
    }

    #[test]
    fn a_file_added_after_the_model_is_the_target_cargo_discovers() {
        let modules = [module()];
        let text = "#[test]\nfn added() {}\n";
        let found = targets(&modules, Path::new("/w/calc/tests/added.rs"), text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "added", "a test of its own, not a module of tests/outside.rs: {found:?}");
        let options = &found[0].configuration.options;
        assert_eq!(
            (options["target"].as_str(), options["targetKind"].as_str()),
            (Some("added"), Some("test")),
            "{options}"
        );
        let main = targets(&modules, Path::new("/w/calc/src/bin/new/main.rs"), "fn main() {}\n");
        assert_eq!(main.first().map(|target| target.name.as_str()), Some("new"), "{main:?}");
        assert!(discovered(Path::new("/w/calc"), Path::new("/w/calc/src/util.rs")).is_none());
    }

    #[test]
    fn tests_modules_benchmarks_and_main_are_found() {
        let found: Vec<(TargetKind, String, bool, u32)> =
            items(LIB).into_iter().map(|item| (item.kind, item.path.join("::"), item.module, item.line)).collect();
        assert_eq!(
            found,
            [
                (TargetKind::Test, "tests".to_owned(), true, 8),
                (TargetKind::Test, "tests::adds".to_owned(), false, 12),
                (TargetKind::Test, "tests::later".to_owned(), false, 17),
                (TargetKind::Test, "tests::slow".to_owned(), false, 21),
                (TargetKind::Test, "tests::deep::inner".to_owned(), false, 25),
                (TargetKind::Benchmark, "speed".to_owned(), false, 32),
            ]
        );
        let main = items("fn main() {\n    let f = |x: i32| x;\n}\nmod m { fn main() {} }\n");
        assert_eq!(main.iter().map(|item| (item.kind, item.line)).collect::<Vec<_>>(), [(TargetKind::Main, 0)]);
        assert!(items("#![allow(dead_code)]\nfn helper() {}\n#[test]\nfn t();\n").is_empty());
    }

    #[test]
    fn files_find_their_package_target_and_module_path() {
        let modules = [module()];
        let at = |path: &str| place(&modules, Path::new(path)).unwrap();
        let lib = at("/w/calc/src/lib.rs");
        assert_eq!(
            (lib.kind.as_str(), lib.target.as_str(), lib.root, lib.label.as_str()),
            ("lib", "calc", true, "calc")
        );
        let nested = at("/w/calc/src/ops/mul.rs");
        assert_eq!((nested.kind.as_str(), nested.modules.join("::")), ("lib", "ops::mul".to_owned()));
        assert_eq!(at("/w/calc/src/ops/mod.rs").modules, ["ops"]);
        let main = at("/w/calc/src/main.rs");
        assert_eq!(
            (main.kind.as_str(), main.target.as_str(), main.label.as_str()),
            ("bin", "calc-cli", "calc_cli (src/main.rs)")
        );
        let tool = at("/w/calc/src/bin/tool.rs");
        assert_eq!((tool.kind.as_str(), tool.target.as_str(), tool.root), ("bin", "tool", true));
        let outside = at("/w/calc/tests/outside.rs");
        assert_eq!((outside.kind.as_str(), outside.label.as_str()), ("test", "outside (tests/outside.rs)"));
        assert!(place(&modules, Path::new("/elsewhere/x.rs")).is_none());
    }

    #[test]
    fn targets_carry_the_configurations_that_run_them() {
        let modules = [module()];
        let found = targets(&modules, Path::new("/w/calc/src/lib.rs"), LIB);
        let adds = found.iter().find(|target| target.name == "tests::adds").unwrap();
        assert_eq!(adds.package, "calc");
        assert_eq!(adds.configuration.kind, TEST);
        assert_eq!(adds.configuration.name, "Test tests::adds");
        assert_eq!(
            adds.configuration.options,
            json!({ "package": "calc", "targetKind": "lib", "target": "calc", "filter": "tests::adds", "exact": true,
                    "features": "", "allFeatures": false, "noDefaultFeatures": false, "profile": "" })
        );
        let module = found.iter().find(|target| target.name == "tests").unwrap();
        assert_eq!(module.configuration.options["filter"], "tests::");
        assert_eq!(module.configuration.options["exact"], false);
        let speed = found.iter().find(|target| target.name == "speed").unwrap();
        assert_eq!((speed.kind, speed.configuration.kind.as_str()), (TargetKind::Benchmark, BENCH));
        assert!(!found.iter().any(|target| target.kind == TargetKind::Main), "no main in a library");

        let main = targets(&modules, Path::new("/w/calc/src/main.rs"), "fn main() {}\n");
        assert_eq!(main.len(), 1);
        assert_eq!((main[0].kind, main[0].name.as_str()), (TargetKind::Main, "calc-cli"));
        assert_eq!(main[0].configuration.kind, RUN);
        assert_eq!(main[0].configuration.options["targetKind"], "bin");
        let nested = targets(&modules, Path::new("/w/calc/src/ops/mul.rs"), "#[test]\nfn twice() {}\n");
        assert_eq!(nested[0].name, "ops::mul::twice");
        assert!(targets(&modules, Path::new("/w/calc/Cargo.toml"), "").is_empty());
    }

    #[test]
    fn the_context_is_the_innermost_runnable_item() {
        let modules = [module()];
        let lib = Path::new("/w/calc/src/lib.rs");
        let byte = |needle: &str| LIB.find(needle).unwrap();
        assert_eq!(context(&modules, lib, LIB, byte("add(2, 2)")).unwrap().name, "Test tests::adds");
        assert_eq!(context(&modules, lib, LIB, byte("fn inner")).unwrap().name, "Test tests::deep::inner");
        assert_eq!(context(&modules, lib, LIB, byte("fn helper")).unwrap().name, "Test tests");
        assert_eq!(context(&modules, lib, LIB, byte("a + b")), None, "nothing to run in a library function");
        let main = context(&modules, Path::new("/w/calc/src/main.rs"), "fn main() {}\n", 0).unwrap();
        assert_eq!(main.name, "Run calc-cli");
        let outside = context(&modules, Path::new("/w/calc/tests/outside.rs"), "use calc::add;\n", 0).unwrap();
        assert_eq!((outside.name.as_str(), outside.options["targetKind"].as_str()), ("Test outside", Some("test")));
    }
}
