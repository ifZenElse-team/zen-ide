use std::path::{Path, PathBuf};

use zen_i18n::t;
use zen_run::test_support::{
    GeneratedTest, NewTest, TestDefaults, TestFramework, TestMember, TestSupport, line_count, line_of,
};

use crate::lexer::{Token, tokenize};

pub const BUILTIN: &str = "rust";

pub const TOKIO: &str = "tokio";

pub struct RustTests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Item,
    Body,
}

struct Scanned {
    members: Vec<TestMember>,
    test_module: Option<(usize, usize)>,
}

fn is_test_attribute(path: &str) -> bool {
    let last = path.rsplit("::").next().unwrap_or(path);
    matches!(last, "test" | "rstest" | "test_case")
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn key(member: &TestMember) -> String {
    match &member.owner {
        Some(owner) => format!("{}_{}", snake(owner), member.name),
        None => member.name.clone(),
    }
}

fn crate_folder(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).find(|folder| folder.join("Cargo.toml").is_file()).map(Path::to_owned)
}

fn scan(text: &str) -> Scanned {
    let toks: Vec<(Token, usize, usize)> = tokenize(text)
        .into_iter()
        .filter(|(token, _, _)| {
            !matches!(token, Token::Whitespace | Token::LineComment { .. } | Token::BlockComment { .. })
        })
        .collect();
    let word = |at: usize| toks.get(at).map_or("", |(_, start, end)| &text[*start..*end]);
    let mut members: Vec<TestMember> = Vec::new();
    let mut test_module = None;
    let mut scopes: Vec<(Scope, Option<String>, bool, Option<usize>)> = Vec::new();
    let mut attributes: Vec<String> = Vec::new();
    let mut pending: Option<(Scope, Option<String>, bool, Option<usize>)> = None;
    let mut at = 0;
    while at < toks.len() {
        let (token, start, _) = toks[at];
        let in_item_scope = scopes.last().is_none_or(|(scope, _, _, _)| *scope == Scope::Item);
        let in_tests = scopes.iter().any(|(_, _, tests, _)| *tests);
        let owner = scopes.last().and_then(|(_, owner, _, _)| owner.clone());
        match token {
            Token::Punct('#') if word(at + 1) == "[" || (word(at + 1) == "!" && word(at + 2) == "[") => {
                let open = if word(at + 1) == "!" { at + 2 } else { at + 1 };
                let mut depth = 0usize;
                let mut scan = open;
                let mut inner = String::new();
                while scan < toks.len() {
                    match word(scan) {
                        "[" | "(" => depth += 1,
                        "]" | ")" => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    if scan > open && depth > 0 {
                        inner.push_str(word(scan));
                    }
                    scan += 1;
                    if depth == 0 {
                        break;
                    }
                }
                if open == at + 1 {
                    attributes.push(inner);
                }
                at = scan;
                continue;
            }
            Token::Punct('{') => {
                let opened = pending.take().unwrap_or((Scope::Body, None, false, None));
                scopes.push(opened);
                attributes.clear();
            }
            Token::Punct('}') => {
                if let Some((_, _, tests, member)) = scopes.pop() {
                    let end = line_of(text, start) + 1;
                    if let Some(index) = member {
                        members[index].end_line = end;
                    }
                    if tests && !scopes.iter().any(|(_, _, tests, _)| *tests) {
                        test_module = test_module.map(|(line, _)| (line, end));
                    }
                }
                attributes.clear();
            }
            Token::Punct(';') => {
                attributes.clear();
                pending = None;
            }
            Token::Ident if in_item_scope && pending.is_none() && word(at) == "mod" && word(at + 2) == "{" => {
                let tests =
                    attributes.iter().any(|attribute| attribute.starts_with("cfg") && attribute.contains("test"));
                let line = line_of(text, start);
                let mut member = None;
                if tests && !in_tests {
                    test_module.get_or_insert((line, line + 1));
                    members.push(TestMember {
                        name: word(at + 1).to_owned(),
                        owner: None,
                        line,
                        end_line: line + 1,
                        test: true,
                    });
                    member = Some(members.len() - 1);
                }
                pending = Some((Scope::Item, None, tests, member));
                at += 2;
                continue;
            }
            Token::Ident if in_item_scope && pending.is_none() && matches!(word(at), "impl" | "trait") => {
                let trait_block = word(at) == "trait";
                let mut scan = at + 1;
                let mut angle = 0usize;
                let mut owner = None;
                let mut after_for = false;
                while scan < toks.len() && !(angle == 0 && matches!(word(scan), "{" | ";")) {
                    match word(scan) {
                        "<" => angle += 1,
                        ">" => angle = angle.saturating_sub(1),
                        "for" if angle == 0 => {
                            after_for = true;
                            owner = None;
                        }
                        "where" if angle == 0 => break,
                        name if angle == 0
                            && toks[scan].0 == Token::Ident
                            && (owner.is_none() || after_for)
                            && !matches!(name, "dyn" | "unsafe" | "const" | "mut") =>
                        {
                            owner = Some(name.to_owned());
                            after_for = false;
                        }
                        _ => {}
                    }
                    scan += 1;
                }
                if trait_block {
                    owner = toks.get(at + 1).map(|_| word(at + 1).to_owned());
                }
                pending = Some((Scope::Item, owner, false, None));
                at = scan;
                continue;
            }
            Token::Ident
                if in_item_scope
                    && word(at) == "fn"
                    && toks.get(at + 1).is_some_and(|(token, _, _)| *token == Token::Ident) =>
            {
                let line = line_of(text, start);
                let test = in_tests
                    || attributes
                        .iter()
                        .any(|attribute| is_test_attribute(attribute.split('(').next().unwrap_or_default()));
                members.push(TestMember { name: word(at + 1).to_owned(), owner, line, end_line: line + 1, test });
                pending = Some((Scope::Body, None, false, Some(members.len() - 1)));
                attributes.clear();
                at += 2;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    Scanned { members, test_module }
}

fn stub(member: &TestMember, tokio: bool, indent: &str) -> (String, String) {
    let name = format!("test_{}", key(member));
    let (attribute, asynchronous) = if tokio { ("#[tokio::test]", "async ") } else { ("#[test]", "") };
    (name.clone(), format!("\n{indent}{attribute}\n{indent}{asynchronous}fn {name}() {{\n{indent}}}\n"))
}

fn crate_name(folder: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(folder.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(value) = line.strip_prefix("name") {
            return Some(value.trim_start().strip_prefix('=')?.trim().trim_matches('"').replace('-', "_"));
        }
    }
    None
}

impl TestSupport for RustTests {
    fn id(&self) -> &str {
        "rust"
    }

    fn handles(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| extension == "rs")
    }

    fn is_test_file(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|name| name == "tests.rs")
            || path.ancestors().skip(1).any(|folder| {
                folder.file_name().is_some_and(|name| name == "tests")
                    && folder.parent().is_some_and(|parent| parent.join("Cargo.toml").is_file())
            })
    }

    fn members(&self, _path: &Path, text: &str) -> Vec<TestMember> {
        let scanned = scan(text);
        let total = line_count(text);
        scanned
            .members
            .into_iter()
            .map(|member| TestMember { end_line: member.end_line.min(total.max(1)), ..member })
            .collect()
    }

    fn test_files(&self, _root: &Path, subject: &Path) -> Vec<PathBuf> {
        let stem = subject.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = subject.parent().unwrap_or(Path::new(""));
        let mut files = vec![subject.to_owned()];
        if matches!(stem.as_str(), "mod" | "lib" | "main") {
            files.push(folder.join("tests.rs"));
        } else {
            files.push(folder.join(&stem).join("tests.rs"));
        }
        if let Some(krate) = crate_folder(subject) {
            let tests = krate.join("tests");
            let base = if matches!(stem.as_str(), "mod" | "lib" | "main") {
                folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
            } else {
                stem
            };
            for name in [format!("{base}.rs"), format!("{base}_test.rs"), format!("{base}_tests.rs")] {
                files.push(tests.join(name));
            }
        }
        files
    }

    fn subject_files(&self, _root: &Path, test: &Path) -> Vec<PathBuf> {
        let folder = test.parent().unwrap_or(Path::new(""));
        if test.file_name().is_some_and(|name| name == "tests.rs") {
            let mut files = Vec::new();
            if let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) {
                files.push(parent.join(format!("{}.rs", name.to_string_lossy())));
            }
            files.extend([folder.join("mod.rs"), folder.join("lib.rs"), folder.join("main.rs")]);
            return files;
        }
        let Some(krate) = crate_folder(test) else { return Vec::new() };
        let stem = test.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let base = stem.trim_end_matches("_tests").trim_end_matches("_test");
        let source = krate.join("src");
        vec![
            source.join(format!("{base}.rs")),
            source.join(base).join("mod.rs"),
            source.join("lib.rs"),
            source.join("main.rs"),
        ]
    }

    fn tests(&self, subject: &TestMember, test: &TestMember) -> bool {
        if subject.test || !test.test {
            return false;
        }
        let key = key(subject);
        [key.clone(), format!("test_{key}")]
            .iter()
            .any(|name| test.name == *name || test.name.starts_with(&format!("{name}_")))
    }

    fn frameworks(&self, root: &Path, subject: &Path) -> Vec<TestFramework> {
        let mut frameworks = vec![TestFramework { id: BUILTIN.to_owned(), name: t!("rust-tests-framework-builtin") }];
        let manifests = crate_folder(subject).into_iter().chain([root.to_owned()]);
        let tokio = manifests
            .filter_map(|folder| std::fs::read_to_string(folder.join("Cargo.toml")).ok())
            .any(|text| text.lines().any(|line| line.trim_start().starts_with("tokio")));
        if tokio {
            frameworks.push(TestFramework { id: TOKIO.to_owned(), name: "Tokio".to_owned() });
        }
        frameworks
    }

    fn defaults(&self, _root: &Path, subject: &Path) -> TestDefaults {
        let stem = subject.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let mut destinations: Vec<PathBuf> = subject.parent().map(Path::to_owned).into_iter().collect();
        if !self.is_test_file(subject) {
            destinations.extend(crate_folder(subject).map(|krate| krate.join("tests")));
        }
        TestDefaults { name: stem, destinations }
    }

    fn generate(
        &self,
        request: &NewTest,
        subject: &Path,
        subject_text: &str,
        existing: Option<&str>,
    ) -> Result<GeneratedTest, String> {
        let tokio = request.framework == TOKIO;
        let test_path = self.test_file(subject, request);
        let in_file = test_path == subject;
        let current = if in_file { Some(existing.unwrap_or(subject_text)) } else { existing };
        let present = current.map(|text| self.members(&test_path, text)).unwrap_or_default();
        let wanted: Vec<&TestMember> =
            request.members.iter().filter(|member| !present.iter().any(|test| self.tests(member, test))).collect();
        let indent = if in_file { "    " } else { "" };
        let made: Vec<(String, String)> = wanted.iter().map(|member| stub(member, tokio, indent)).collect();
        let functions: String = made.iter().map(|(_, text)| text.as_str()).collect();
        let first = made.first().and_then(|(name, body)| body.find(name.as_str())).unwrap_or(0);
        let mut text = current.map_or_else(String::new, str::to_owned);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        let at = if in_file {
            match scan(&text).test_module {
                Some((_, end)) => {
                    text.match_indices('\n').nth(end.saturating_sub(2)).map_or(text.len(), |(at, _)| at + 1)
                }
                None => {
                    text.push_str("\n#[cfg(test)]\nmod tests {\n    use super::*;\n");
                    let at = text.len();
                    text.push_str("}\n");
                    at
                }
            }
        } else {
            if current.is_none() {
                let library = crate_folder(subject).filter(|krate| krate.join("src/lib.rs").is_file());
                if let Some(name) = library.as_deref().and_then(crate_name) {
                    text.push_str(&format!("use {name}::*;\n"));
                }
            }
            text.len()
        };
        let functions = if !in_file && at == 0 { functions.trim_start_matches('\n').to_owned() } else { functions };
        let first = if !in_file && at == 0 { first.saturating_sub(1) } else { first };
        text.insert_str(at, &functions);
        Ok(GeneratedTest { line: line_of(&text, at + first), text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CART: &str = "pub struct Cart;\n\nimpl Cart {\n    pub fn total(&self) -> u32 {\n        0\n    }\n}\n\npub fn price() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_price_is_one() {\n        assert_eq!(price(), 1);\n    }\n}\n";

    fn names(members: &[TestMember]) -> Vec<(String, usize, usize, bool)> {
        members.iter().map(|member| (member.label(), member.line, member.end_line, member.test)).collect()
    }

    #[test]
    fn rust_members_are_functions_and_methods_and_the_tests_module_holds_tests() {
        assert_eq!(
            names(&RustTests.members(Path::new("/p/src/cart.rs"), CART)),
            [
                ("Cart.total".to_owned(), 3, 6, false),
                ("price".to_owned(), 8, 11, false),
                ("tests".to_owned(), 13, 21, true),
                ("test_price_is_one".to_owned(), 17, 20, true)
            ]
        );
        let traits = "impl<T: Clone> Shape for Square<T> where T: Copy {\n    fn area(&self) {}\n}\n";
        assert_eq!(RustTests.members(Path::new("/p/a.rs"), traits)[0].owner.as_deref(), Some("Square"));
        let attributed = "#[tokio::test]\nasync fn fetches() {}\n";
        assert!(RustTests.members(Path::new("/p/a.rs"), attributed)[0].test);
    }

    #[test]
    fn rust_tests_are_named_after_the_function_or_the_type_and_method() {
        let members = RustTests.members(Path::new("/p/src/cart.rs"), CART);
        let test = |name: &str| TestMember { name: name.into(), owner: None, line: 0, end_line: 1, test: true };
        assert!(RustTests.tests(&members[1], &members[3]));
        assert!(RustTests.tests(&members[0], &test("test_cart_total")));
        assert!(RustTests.tests(&members[0], &test("cart_total_of_nothing")));
        assert!(!RustTests.tests(&members[1], &test("prices")));
        assert!(!RustTests.tests(&members[3], &members[1]));
    }

    #[test]
    fn rust_tests_live_in_the_file_a_tests_rs_beside_it_or_the_crates_tests_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-shop\"\n\n[dependencies]\ntokio = \"1\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src/cart")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        let cart = dir.path().join("src/cart.rs");
        let files = RustTests.test_files(dir.path(), &cart);
        assert_eq!(files[..3], [cart.clone(), dir.path().join("src/cart/tests.rs"), dir.path().join("tests/cart.rs")]);
        assert!(RustTests.is_test_file(&dir.path().join("tests/cart.rs")) && RustTests.is_test_file(&files[1]));
        assert!(!RustTests.is_test_file(&cart));
        assert_eq!(RustTests.subject_files(dir.path(), &files[1])[0], cart);
        assert_eq!(RustTests.subject_files(dir.path(), &dir.path().join("tests/cart_tests.rs"))[0], cart);
        assert_eq!(
            RustTests.defaults(dir.path(), &cart),
            TestDefaults { name: "cart".into(), destinations: vec![dir.path().join("src"), dir.path().join("tests")] }
        );
        zen_i18n::register(crate::LOCALES);
        let ids: Vec<String> = RustTests.frameworks(dir.path(), &cart).into_iter().map(|f| f.id).collect();
        assert_eq!(ids, [BUILTIN, TOKIO]);
        assert_eq!(crate_name(dir.path()).as_deref(), Some("my_shop"));
    }

    #[test]
    fn a_new_rust_test_goes_into_the_tests_module_or_a_file_of_the_tests_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"shop\"\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub mod cart;\n").unwrap();
        let cart = dir.path().join("src/cart.rs");
        let members = RustTests.members(&cart, CART);
        let request = NewTest {
            framework: BUILTIN.into(),
            name: "cart".into(),
            destination: dir.path().join("src"),
            members: vec![members[0].clone(), members[1].clone()],
        };
        let made = RustTests.generate(&request, &cart, CART, None).unwrap();
        assert!(made.text.ends_with(
            "    fn test_price_is_one() {\n        assert_eq!(price(), 1);\n    }\n\n    #[test]\n    fn test_cart_total() {\n    }\n}\n"
        ));
        assert_eq!(made.text.lines().nth(made.line).unwrap(), "    fn test_cart_total() {");
        let bare = "pub fn price() -> u32 {\n    1\n}\n";
        let only = NewTest { members: RustTests.members(&cart, bare), ..request.clone() };
        let added = RustTests.generate(&only, &cart, bare, None).unwrap();
        assert_eq!(
            added.text,
            "pub fn price() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_price() {\n    }\n}\n"
        );
        let outside = NewTest { destination: dir.path().join("tests"), framework: TOKIO.into(), ..only };
        let file = RustTests.generate(&outside, &cart, bare, None).unwrap();
        assert_eq!(file.text, "use shop::*;\n\n#[tokio::test]\nasync fn test_price() {\n}\n");
        assert_eq!(file.line, 3);
    }
}
