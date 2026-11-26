use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use zen_syntax::{Outliner, SymbolKind};

use crate::toolchain::go::{Environment, find_sdks};
use crate::{GoOutliner, Token, tokenize};

const MAX_FILES: usize = 20_000;

const FRESH: Duration = Duration::from_secs(5);

const BUILTIN: &str = "builtin";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Method {
    pub(crate) name: String,
    pub(crate) signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interface {
    pub(crate) import: String,
    pub(crate) package: String,
    pub(crate) folder: PathBuf,
    pub(crate) name: String,
    pub(crate) methods: Vec<Method>,
    embeds: Vec<String>,
    imports: BTreeMap<String, String>,
}

impl Interface {
    pub(crate) fn exported(&self) -> bool {
        self.name.chars().next().is_some_and(char::is_uppercase)
    }

    pub(crate) fn is_builtin(&self) -> bool {
        self.import == BUILTIN
    }
}

pub(crate) struct Target<'a> {
    pub(crate) folder: &'a Path,
    pub(crate) imports: BTreeMap<String, String>,
}

fn package_clause(text: &str) -> Option<String> {
    let mut tokens =
        tokenize(text).into_iter().filter(|(token, ..)| !matches!(token, Token::Whitespace | Token::Comment));
    let (_, start, end) = tokens.next()?;
    let (token, name_start, name_end) = tokens.next()?;
    (&text[start..end] == "package" && token == Token::Ident).then(|| text[name_start..name_end].to_owned())
}

fn default_name(path: &str) -> String {
    let mut parts = path.rsplit('/');
    let last = parts.next().unwrap_or_default();
    let versioned =
        last.strip_prefix('v').is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()));
    let last = if versioned { parts.next().unwrap_or(last) } else { last };
    last.split('.').next().unwrap_or(last).to_owned()
}

pub(crate) fn imports_of(text: &str) -> BTreeMap<String, String> {
    let tokens: Vec<(Token, usize, usize)> =
        tokenize(text).into_iter().filter(|(token, ..)| !matches!(token, Token::Whitespace | Token::Comment)).collect();
    let word = |index: usize| tokens.get(index).map(|&(_, start, end)| &text[start..end]);
    let mut imports = BTreeMap::new();
    let mut alias = None;
    let mut open = false;
    let mut index = 2;
    while let Some(&(token, start, end)) = tokens.get(index) {
        match token {
            Token::Punct('(') if word(index - 1) == Some("import") => open = true,
            Token::Punct(')') if open => open = false,
            Token::String => {
                let path = text[start..end].trim_matches(|c| c == '"' || c == '`').to_owned();
                let name = alias.take().unwrap_or_else(|| default_name(&path));
                imports.insert(path, name);
            }
            Token::Ident if word(index) == Some("import") => {}
            Token::Ident | Token::Punct('.') if open || word(index - 1) == Some("import") => {
                alias = Some(text[start..end].to_owned());
            }
            Token::Punct(';') => {}
            _ if !open => break,
            _ => {}
        }
        index += 1;
    }
    imports
}

fn embedded(body: &str) -> Option<Vec<String>> {
    let mut lines = vec![String::new()];
    for (token, start, end) in tokenize(body) {
        match token {
            Token::Comment => {}
            Token::Whitespace if body[start..end].contains('\n') => lines.push(String::new()),
            Token::Whitespace => {}
            Token::Punct(';') => lines.push(String::new()),
            _ => lines.last_mut()?.push_str(&body[start..end]),
        }
    }
    let lines: Vec<String> = lines.into_iter().filter(|line| !line.is_empty()).collect();
    let plain = |line: &String| line.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    lines.iter().all(plain).then_some(lines)
}

pub(crate) fn declared(text: &str, import: &str, folder: &Path) -> Vec<Interface> {
    let Some(package) = package_clause(text) else { return Vec::new() };
    let imports: BTreeMap<String, String> = imports_of(text).into_iter().map(|(path, name)| (name, path)).collect();
    let chars: Vec<char> = text.chars().collect();
    GoOutliner
        .outline(text)
        .into_iter()
        .filter(|item| item.kind == SymbolKind::Interface)
        .filter_map(|item| {
            let after_name = item.selection_start + item.name.chars().count();
            let source = &chars[after_name.min(chars.len())..item.end.min(chars.len())];
            let open = after_name + source.iter().position(|c| *c == '{')?;
            if source.iter().position(|c| *c == '[').is_some_and(|at| after_name + at < open) {
                return None;
            }
            let mut body = String::new();
            let mut at = open + 1;
            for method in &item.children {
                body.extend(&chars[at..method.start]);
                body.push('\n');
                at = method.end;
            }
            body.extend(&chars[at..item.end.saturating_sub(1).max(at)]);
            let embeds = embedded(&body)?;
            let methods = item
                .children
                .iter()
                .map(|method| Method {
                    name: method.name.clone(),
                    signature: chars[method.selection_start + method.name.chars().count()..method.end]
                        .iter()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                })
                .collect::<Vec<_>>();
            (!methods.is_empty() || !embeds.is_empty()).then(|| Interface {
                import: import.to_owned(),
                package: package.clone(),
                folder: folder.to_owned(),
                name: item.name,
                methods,
                embeds,
                imports: imports.clone(),
            })
        })
        .collect()
}

fn walk(folder: &Path, top: &Path, skip_internal: bool, budget: &mut usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            let hidden = name.starts_with(['.', '_']);
            let skipped = matches!(name.as_str(), "vendor" | "testdata" | "node_modules")
                || (skip_internal && matches!(name.as_str(), "internal" | "cmd"));
            if !hidden && !skipped && !(path != top && path.join("go.mod").is_file()) {
                walk(&path, top, skip_internal, budget, found);
            }
        } else if name.ends_with(".go") && !name.ends_with("_test.go") && *budget > 0 {
            *budget -= 1;
            found.push(path);
        }
    }
}

fn read_tree(top: &Path, prefix: &str, skip_internal: bool) -> Vec<Interface> {
    let mut files = Vec::new();
    walk(top, top, skip_internal, &mut MAX_FILES.clone(), &mut files);
    files.sort();
    let mut interfaces = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        if !text.contains("interface") {
            continue;
        }
        let folder = file.parent().unwrap_or(top);
        let relative = folder.strip_prefix(top).unwrap_or(folder).to_string_lossy().replace('\\', "/");
        let import = match (prefix.is_empty(), relative.is_empty()) {
            (_, true) => prefix.to_owned(),
            (true, false) => relative,
            (false, false) => format!("{prefix}/{relative}"),
        };
        interfaces.extend(declared(&text, &import, folder));
    }
    interfaces
}

fn builtin_error() -> Interface {
    Interface {
        import: BUILTIN.to_owned(),
        package: BUILTIN.to_owned(),
        folder: PathBuf::new(),
        name: "error".to_owned(),
        methods: vec![Method { name: "Error".to_owned(), signature: "() string".to_owned() }],
        embeds: Vec::new(),
        imports: BTreeMap::new(),
    }
}

pub(crate) fn read_standard_library(goroot: &Path) -> Vec<Interface> {
    let mut found = read_tree(&goroot.join("src"), "", true);
    found.retain(Interface::exported);
    found.push(builtin_error());
    found
}

pub(crate) fn module_root(file: &Path) -> PathBuf {
    let folder = file.parent().unwrap_or(file);
    folder.ancestors().find(|at| at.join("go.mod").is_file()).unwrap_or(folder).to_owned()
}

pub(crate) fn read_module(root: &Path) -> Vec<Interface> {
    let file = root.join("go.mod");
    let module = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| crate::gomod::parse_go_mod(&file, &text).module)
        .map(|module| module.path)
        .unwrap_or_default();
    read_tree(root, &module, false)
}

fn resolve<'a>(all: &'a [Interface], from: &Interface, embed: &str) -> Option<&'a Interface> {
    let (import, name) = match embed.split_once('.') {
        Some((package, name)) => (from.imports.get(package)?.as_str(), name),
        None if embed == "error" => (BUILTIN, embed),
        None => (from.import.as_str(), embed),
    };
    all.iter().find(|known| {
        known.name == name && if import.is_empty() { known.folder == from.folder } else { known.import == import }
    })
}

pub(crate) fn method_set<'a>(
    all: &'a [Interface],
    interface: &'a Interface,
) -> Option<Vec<(&'a Interface, &'a Method)>> {
    fn add<'a>(
        all: &'a [Interface],
        interface: &'a Interface,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<(&'a Interface, &'a Method)>,
        depth: usize,
    ) -> Option<()> {
        if depth > 32 {
            return None;
        }
        for method in &interface.methods {
            if seen.insert(method.name.clone()) {
                out.push((interface, method));
            }
        }
        for embed in &interface.embeds {
            add(all, resolve(all, interface, embed)?, seen, out, depth + 1)?;
        }
        Some(())
    }
    let mut out = Vec::new();
    add(all, interface, &mut BTreeSet::new(), &mut out, 0)?;
    Some(out)
}

pub(crate) fn qualified(
    owner: &Interface,
    signature: &str,
    target: &Target<'_>,
    needed: &mut BTreeSet<String>,
) -> String {
    let same_package = owner.folder == target.folder;
    let local = |path: &str, fallback: &str, needed: &mut BTreeSet<String>| match target.imports.get(path) {
        Some(name) => name.clone(),
        None => {
            needed.insert(path.to_owned());
            fallback.to_owned()
        }
    };
    let tokens = tokenize(signature);
    let mut out = String::new();
    let mut after_dot = false;
    for (index, &(token, start, end)) in tokens.iter().enumerate() {
        let word = &signature[start..end];
        if token != Token::Ident {
            out.push_str(word);
            after_dot = token == Token::Punct('.');
            continue;
        }
        let selector = tokens.get(index + 1).is_some_and(|next| next.0 == Token::Punct('.'));
        if after_dot {
            out.push_str(word);
        } else if selector {
            match owner.imports.get(word) {
                Some(path) => out.push_str(&local(path, word, needed)),
                None => out.push_str(word),
            }
        } else if !same_package && word.chars().next().is_some_and(char::is_uppercase) {
            out.push_str(&local(&owner.import, &owner.package, needed));
            out.push('.');
            out.push_str(word);
        } else {
            out.push_str(word);
        }
        after_dot = false;
    }
    out
}

#[derive(Debug, Default)]
struct State {
    root: Option<PathBuf>,
    started: Option<Instant>,
    running: bool,
    standard: Option<Arc<Vec<Interface>>>,
    module: Vec<Interface>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Catalog(Arc<Mutex<State>>);

impl Catalog {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn refresh(&self, file: &Path) {
        let root = module_root(file);
        let standard = {
            let mut state = self.state();
            let same = state.root.as_ref() == Some(&root);
            let recent = state.started.is_some_and(|started| started.elapsed() < FRESH);
            if state.running || (same && recent) {
                return;
            }
            if !same {
                state.module.clear();
            }
            state.root = Some(root.clone());
            state.started = Some(Instant::now());
            state.running = true;
            state.standard.clone()
        };
        let catalog = self.clone();
        std::thread::spawn(move || {
            let standard = standard.unwrap_or_else(|| {
                let sdks = find_sdks(&Environment::of_process());
                Arc::new(sdks.first().map_or_else(|| vec![builtin_error()], |sdk| read_standard_library(&sdk.root)))
            });
            let module = read_module(&root);
            let mut state = catalog.state();
            state.standard = Some(standard);
            if state.root.as_ref() == Some(&root) {
                state.module = module;
            }
            state.running = false;
        });
    }

    pub(crate) fn pending(&self, file: &Path) -> bool {
        self.refresh(file);
        let state = self.state();
        state.running && (state.standard.is_none() || state.module.is_empty())
    }

    pub(crate) fn interfaces(&self) -> Vec<Interface> {
        let state = self.state();
        state.module.iter().cloned().chain(state.standard.iter().flat_map(|all| all.iter().cloned())).collect()
    }

    #[cfg(test)]
    pub(crate) fn filled(interfaces: Vec<Interface>, file: &Path) -> Self {
        let catalog = Self::default();
        {
            let mut state = catalog.state();
            state.root = Some(module_root(file));
            state.started = Some(Instant::now());
            state.standard = Some(Arc::new(interfaces));
        }
        catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IO: &str = "package io\n\nimport \"time\"\n\n// Reader reads.\ntype Reader interface {\n\t// Read reads.\n\tRead(p []byte) (n int, err error)\n}\n\ntype Closer interface {\n\tClose() error\n}\n\ntype ReadCloser interface {\n\tReader\n\tCloser // and closes\n\tDeadline() time.Time\n}\n\ntype Number interface {\n\t~int | ~float64\n}\n\ntype Box[T any] interface {\n\tGet() T\n}\n\ntype Empty interface{}\n\ntype Sink interface {\n\tDrain(from Reader,\n\t\tlimit int)\n}\n";

    fn io() -> Vec<Interface> {
        declared(IO, "io", Path::new("/go/src/io"))
    }

    fn names(all: &[Interface]) -> Vec<&str> {
        all.iter().map(|interface| interface.name.as_str()).collect()
    }

    #[test]
    fn a_file_gives_its_interfaces_without_constraints_generic_ones_and_empty_ones() {
        let all = io();
        assert_eq!(names(&all), ["Reader", "Closer", "ReadCloser", "Sink"]);
        assert_eq!(all[0].methods[0].signature, "(p []byte) (n int, err error)");
        assert_eq!(all[2].embeds, ["Reader", "Closer"]);
        assert_eq!(all[3].methods[0].signature, "(from Reader, limit int)");
    }

    #[test]
    fn embedded_interfaces_give_their_methods_once() {
        let all = io();
        let methods: Vec<&str> =
            method_set(&all, &all[2]).unwrap().iter().map(|(_, method)| method.name.as_str()).collect();
        assert_eq!(methods, ["Deadline", "Read", "Close"]);
        let outside =
            declared("package x\n\nimport \"y\"\n\ntype A interface {\n\ty.Missing\n}\n", "x", Path::new("/x"));
        assert_eq!(method_set(&all, &outside[0]), None, "a method set that cannot be known is not offered");
    }

    #[test]
    fn a_signature_names_the_types_as_the_target_file_sees_them() {
        let all = io();
        let target =
            Target { folder: Path::new("/p"), imports: [("time".to_owned(), "tm".to_owned())].into_iter().collect() };
        let mut needed = BTreeSet::new();
        assert_eq!(qualified(&all[3], "(from Reader, limit int)", &target, &mut needed), "(from io.Reader, limit int)");
        assert_eq!(qualified(&all[2], "() time.Time", &target, &mut needed), "() tm.Time");
        assert_eq!(needed.into_iter().collect::<Vec<_>>(), ["io"]);
        let same = Target { folder: Path::new("/go/src/io"), imports: BTreeMap::new() };
        let mut needed = BTreeSet::new();
        assert_eq!(qualified(&all[3], "(from Reader)", &same, &mut needed), "(from Reader)");
        assert!(needed.is_empty());
    }

    #[test]
    fn imports_are_read_with_their_names() {
        let text = "package a\n\nimport (\n\t\"fmt\"\n\tstr \"strings\"\n\t\"math/rand/v2\"\n\t\"gopkg.in/yaml.v3\"\n)\nimport \"os\"\n\nvar x = \"not an import\"\n";
        let imports: Vec<(String, String)> = imports_of(text).into_iter().collect();
        let expected =
            [("fmt", "fmt"), ("gopkg.in/yaml.v3", "yaml"), ("math/rand/v2", "rand"), ("os", "os"), ("strings", "str")]
                .map(|(path, name)| (path.to_owned(), name.to_owned()));
        assert_eq!(imports, expected);
    }

    #[test]
    fn a_module_is_read_without_its_tests_vendor_and_nested_modules() {
        let dir = tempfile::tempdir().unwrap();
        let write = |file: &str, text: &str| {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let declaring = |name: &str| format!("package p\n\ntype {name} interface {{\n\tRun()\n}}\n");
        write("go.mod", "module example.com/app\n\ngo 1.26\n");
        write("root.go", &declaring("Top"));
        write("inner/store/store.go", &declaring("Store"));
        write("inner/store/store_test.go", &declaring("Fake"));
        write("vendor/v/v.go", &declaring("Vendored"));
        write("tool/go.mod", "module example.com/tool\n");
        write("tool/tool.go", &declaring("Nested"));
        let all = read_module(dir.path());
        assert_eq!(names(&all), ["Store", "Top"]);
        assert_eq!(all[0].import, "example.com/app/inner/store");
        assert_eq!(all[1].import, "example.com/app");
    }

    #[test]
    fn the_standard_library_is_read_without_internal_packages_and_with_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("internal/x")).unwrap();
        std::fs::create_dir_all(src.join("net/http")).unwrap();
        std::fs::write(src.join("internal/x/x.go"), "package x\ntype Hidden interface { Run() }\n").unwrap();
        std::fs::write(
            src.join("net/http/h.go"),
            "package http\ntype Handler interface { ServeHTTP() }\ntype private interface { Run() }\n",
        )
        .unwrap();
        let all = read_standard_library(dir.path());
        assert_eq!(names(&all), ["Handler", "error"]);
        assert_eq!(all[0].import, "net/http");
        assert!(all[1].is_builtin());
    }

    #[test]
    fn the_real_standard_library_gives_its_common_interfaces() {
        let Some(sdk) = find_sdks(&Environment::of_process()).into_iter().next() else { return };
        let all = read_standard_library(&sdk.root);
        let find = |import: &str, name: &str| {
            all.iter().find(|known| known.import == import && known.name == name).expect(name)
        };
        let methods = |interface| -> Vec<String> {
            let mut names: Vec<String> =
                method_set(&all, interface).unwrap().iter().map(|(_, method)| method.name.clone()).collect();
            names.sort();
            names
        };
        assert_eq!(methods(find("io", "ReadWriteCloser")), ["Close", "Read", "Write"]);
        assert_eq!(methods(find("net/http", "Handler")), ["ServeHTTP"]);
        assert_eq!(methods(find("sort", "Interface")), ["Len", "Less", "Swap"]);
        assert_eq!(methods(find("fmt", "Stringer")), ["String"]);
        assert_eq!(methods(find("context", "Context")), ["Deadline", "Done", "Err", "Value"]);
    }
}
