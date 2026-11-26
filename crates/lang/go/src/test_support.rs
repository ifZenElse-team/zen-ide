use std::path::{Path, PathBuf};

use zen_i18n::t;
use zen_run::TestSource;
use zen_run::test_support::{
    GeneratedTest, NewTest, TestDefaults, TestFramework, TestMember, TestSupport, line_count, line_of,
};

use crate::{Token, tokenize};

pub const TESTING: &str = "go";

pub const TESTIFY: &str = "testify";

const TESTIFY_MODULE: &str = "github.com/stretchr/testify";

const PREFIXES: [&str; 4] = ["Test", "Benchmark", "Example", "Fuzz"];

pub struct GoTests;

fn is_go(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "go")
}

fn is_test_name(name: &str) -> bool {
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn siblings(path: &Path, test: bool) -> Vec<PathBuf> {
    let Some(folder) = path.parent() else { return Vec::new() };
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|other| is_go(other) && other != path && GoTests.is_test_file(other) == test)
        .collect();
    found.sort();
    found
}

pub fn source_of(folder: &Path, name: &str) -> Option<TestSource> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| GoTests.is_test_file(file))
        .collect();
    files.sort();
    files.into_iter().find_map(|path| {
        let text = std::fs::read_to_string(&path).ok()?;
        let line = GoTests.members(&path, &text).into_iter().find(|member| member.test && member.name == name)?.line;
        Some(TestSource { path, line: u32::try_from(line + 1).ok()? })
    })
}

fn key(member: &TestMember) -> String {
    match &member.owner {
        Some(owner) => format!("{owner}_{}", member.name),
        None => member.name.clone(),
    }
}

fn test_name(member: &TestMember) -> String {
    let key = key(member);
    if key.starts_with(|c: char| c.is_uppercase()) { format!("Test{key}") } else { format!("Test_{key}") }
}

pub fn package_clause(text: &str) -> Option<String> {
    let words: Vec<&str> = tokenize(text)
        .into_iter()
        .filter(|(token, _, _)| !matches!(token, Token::Whitespace | Token::Comment))
        .map(|(_, start, end)| &text[start..end])
        .take(2)
        .collect();
    match words.as_slice() {
        ["package", name] => Some((*name).to_owned()),
        _ => None,
    }
}

fn module_file(root: &Path, subject: &Path) -> Option<String> {
    let mut folder = subject.parent();
    while let Some(here) = folder {
        if let Ok(text) = std::fs::read_to_string(here.join("go.mod")) {
            return Some(text);
        }
        if here == root {
            break;
        }
        folder = here.parent();
    }
    None
}

fn stub(member: &TestMember, testify: bool) -> (String, String) {
    let name = test_name(member);
    let check = if testify { "\t\t\tassert.NotEmpty(t, tt.name)\n" } else { "" };
    let text = format!(
        "\nfunc {name}(t *testing.T) {{\n\ttests := []struct {{\n\t\tname string\n\t}}{{}}\n\tfor _, tt := range tests {{\n\t\tt.Run(tt.name, func(t *testing.T) {{\n{check}\t\t}})\n\t}}\n}}\n"
    );
    (name, text)
}

fn with_import(text: &str, import: &str) -> String {
    if text.contains(&format!("\"{import}\"")) {
        return text.to_owned();
    }
    let package_end =
        text.find("package ").map_or(0, |at| text[at..].find('\n').map_or(text.len(), |end| at + end + 1));
    let mut out = text.to_owned();
    out.insert_str(package_end, &format!("\nimport \"{import}\"\n"));
    out
}

impl TestSupport for GoTests {
    fn id(&self) -> &str {
        "go"
    }

    fn handles(&self, path: &Path) -> bool {
        is_go(path)
    }

    fn is_test_file(&self, path: &Path) -> bool {
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with("_test.go"))
    }

    fn members(&self, path: &Path, text: &str) -> Vec<TestMember> {
        let test_file = self.is_test_file(path);
        let toks: Vec<(Token, usize, usize)> = tokenize(text)
            .into_iter()
            .filter(|(token, _, _)| !matches!(token, Token::Whitespace | Token::Comment))
            .collect();
        let word = |at: usize| toks.get(at).map_or("", |(_, start, end)| &text[*start..*end]);
        let ident = |at: usize| toks.get(at).is_some_and(|(token, _, _)| *token == Token::Ident);
        let mut found = Vec::new();
        let mut depth = 0usize;
        let mut at = 0;
        while at < toks.len() {
            let (token, start, _) = toks[at];
            match token {
                Token::Punct('(' | '[' | '{') => depth += 1,
                Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
                Token::Ident if depth == 0 && word(at) == "func" => {
                    let mut next = at + 1;
                    let mut owner = None;
                    if word(next) == "(" {
                        let mut names = Vec::new();
                        let mut scan = next + 1;
                        while scan < toks.len() && word(scan) != ")" {
                            if word(scan) == "[" {
                                while scan < toks.len() && word(scan) != "]" {
                                    scan += 1;
                                }
                            } else if ident(scan) {
                                names.push(word(scan).to_owned());
                            }
                            scan += 1;
                        }
                        owner = if names.len() >= 2 { names.get(1).cloned() } else { names.first().cloned() };
                        next = scan + 1;
                    }
                    if ident(next) {
                        let name = word(next).to_owned();
                        let test = test_file && owner.is_none() && is_test_name(&name);
                        let line = line_of(text, start);
                        found.push(TestMember { name, owner, line, end_line: line + 1, test });
                        at = next + 1;
                        continue;
                    }
                }
                Token::Ident if depth == 0 && word(at) == "type" && ident(at + 1) => {
                    let line = line_of(text, start);
                    found.push(TestMember {
                        name: word(at + 1).to_owned(),
                        owner: None,
                        line,
                        end_line: line + 1,
                        test: false,
                    });
                }
                _ => {}
            }
            at += 1;
        }
        let total = line_count(text);
        for index in 0..found.len() {
            found[index].end_line = found.get(index + 1).map_or(total, |next| next.line).max(found[index].line + 1);
        }
        found
    }

    fn test_files(&self, _root: &Path, subject: &Path) -> Vec<PathBuf> {
        let stem = subject.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        vec![subject.with_file_name(format!("{stem}_test.go"))]
    }

    fn subject_files(&self, _root: &Path, test: &Path) -> Vec<PathBuf> {
        let name = test.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        vec![test.with_file_name(name.trim_end_matches("_test.go").to_owned() + ".go")]
    }

    fn searched_files(&self, path: &Path, tests: bool) -> Vec<PathBuf> {
        siblings(path, tests)
    }

    fn tests(&self, subject: &TestMember, test: &TestMember) -> bool {
        if subject.test || !test.test {
            return false;
        }
        let Some(rest) = PREFIXES.iter().find_map(|prefix| test.name.strip_prefix(prefix)) else { return false };
        let rest = rest.trim_start_matches('_');
        let key = key(subject);
        rest == key || rest.starts_with(&format!("{key}_"))
    }

    fn frameworks(&self, root: &Path, subject: &Path) -> Vec<TestFramework> {
        let mut frameworks = vec![TestFramework { id: TESTING.to_owned(), name: t!("go-tests-framework-testing") }];
        if module_file(root, subject).is_some_and(|text| text.contains(TESTIFY_MODULE)) {
            frameworks.push(TestFramework { id: TESTIFY.to_owned(), name: "Testify".to_owned() });
        }
        frameworks
    }

    fn defaults(&self, _root: &Path, subject: &Path) -> TestDefaults {
        let stem = subject.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        TestDefaults {
            name: if self.is_test_file(subject) { stem } else { format!("{stem}_test") },
            destinations: subject.parent().map(Path::to_owned).into_iter().collect(),
        }
    }

    fn generate(
        &self,
        request: &NewTest,
        subject: &Path,
        subject_text: &str,
        existing: Option<&str>,
    ) -> Result<GeneratedTest, String> {
        let testify = request.framework == TESTIFY;
        let test_path = self.test_file(subject, request);
        let present = existing.map(|text| self.members(&test_path, text)).unwrap_or_default();
        let made: Vec<(String, String)> = request
            .members
            .iter()
            .filter(|member| !present.iter().any(|test| self.tests(member, test)))
            .map(|member| stub(member, testify))
            .collect();
        let functions: String = made.iter().map(|(_, text)| text.as_str()).collect();
        let mut text = match existing {
            Some(existing) => existing.to_owned(),
            None => format!("package {}\n", package_clause(subject_text).unwrap_or_else(|| "main".to_owned())),
        };
        if !made.is_empty() || existing.is_none() {
            text = with_import(&text, "testing");
        }
        if testify {
            text = with_import(&text, "github.com/stretchr/testify/assert");
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let at = text.len();
        text.push_str(&functions);
        let first = made.first().and_then(|(name, body)| body.find(name.as_str())).unwrap_or(0);
        Ok(GeneratedTest { line: line_of(&text, at + first), text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CART: &str = "package shop\n\ntype Cart struct{}\n\nfunc (c *Cart) Total() int {\n\treturn 0\n}\n\nfunc price(name string) int {\n\treturn 1\n}\n\nfunc New[T any](v T) {}\n";
    const CART_TEST: &str = "package shop\n\nimport \"testing\"\n\nfunc TestCart_Total(t *testing.T) {}\n\nfunc Test_price_free(t *testing.T) {}\n\nfunc helper() {}\n";

    fn names(members: &[TestMember]) -> Vec<(String, usize, usize, bool)> {
        members.iter().map(|member| (member.label(), member.line, member.end_line, member.test)).collect()
    }

    #[test]
    fn go_members_are_functions_methods_and_types_and_tests_only_in_test_files() {
        let subject = GoTests.members(Path::new("/p/cart.go"), CART);
        assert_eq!(
            names(&subject),
            [
                ("Cart".to_owned(), 2, 4, false),
                ("Cart.Total".to_owned(), 4, 8, false),
                ("price".to_owned(), 8, 12, false),
                ("New".to_owned(), 12, 13, false)
            ]
        );
        let tests = GoTests.members(Path::new("/p/cart_test.go"), CART_TEST);
        assert_eq!(
            names(&tests).into_iter().map(|(name, _, _, test)| (name, test)).collect::<Vec<_>>(),
            [("TestCart_Total".to_owned(), true), ("Test_price_free".to_owned(), true), ("helper".to_owned(), false)]
        );
    }

    #[test]
    fn go_tests_are_found_by_test_benchmark_example_and_fuzz_names() {
        let subject = GoTests.members(Path::new("/p/cart.go"), CART);
        let test = |name: &str| TestMember { name: name.into(), owner: None, line: 0, end_line: 1, test: true };
        let (cart, total, price) = (&subject[0], &subject[1], &subject[2]);
        assert!(GoTests.tests(total, &test("TestCart_Total")));
        assert!(GoTests.tests(total, &test("BenchmarkCart_Total")));
        assert!(GoTests.tests(total, &test("TestCart_Total_empty")));
        assert!(GoTests.tests(price, &test("Test_price")) && !GoTests.tests(price, &test("FuzzPrice")));
        assert!(GoTests.tests(price, &test("Example_price")));
        assert!(GoTests.tests(cart, &test("TestCart")) && GoTests.tests(cart, &test("TestCart_Total")));
        assert!(!GoTests.tests(cart, &test("TestCartesian")));
        assert!(!GoTests.tests(&test("TestCart"), total), "a test is no subject");
    }

    #[test]
    fn a_test_is_found_by_its_function_in_the_folders_test_files_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cart.go"), "package shop\n\nfunc TestCart_Total() {}\n").unwrap();
        std::fs::write(dir.path().join("cart_test.go"), CART_TEST).unwrap();
        let found = source_of(dir.path(), "Test_price_free").unwrap();
        assert_eq!((found.path, found.line), (dir.path().join("cart_test.go"), 7));
        assert_eq!(source_of(dir.path(), "TestCart_Total").unwrap().line, 5);
        assert!(source_of(dir.path(), "helper").is_none(), "not a test");
        assert!(source_of(dir.path(), "TestNowhere").is_none());
    }

    #[test]
    fn go_test_files_pair_foo_go_with_foo_test_go_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["cart.go", "cart_test.go", "price.go", "shop_test.go"] {
            std::fs::write(dir.path().join(name), "package shop\n").unwrap();
        }
        let cart = dir.path().join("cart.go");
        assert_eq!(GoTests.test_files(dir.path(), &cart), [dir.path().join("cart_test.go")]);
        assert_eq!(
            GoTests.searched_files(&cart, true),
            [dir.path().join("cart_test.go"), dir.path().join("shop_test.go")]
        );
        let shop_test = dir.path().join("shop_test.go");
        assert_eq!(GoTests.subject_files(dir.path(), &shop_test), [dir.path().join("shop.go")]);
        assert_eq!(GoTests.searched_files(&shop_test, false), [cart.clone(), dir.path().join("price.go")]);
        assert_eq!(
            GoTests.defaults(dir.path(), &cart),
            TestDefaults { name: "cart_test".into(), destinations: vec![dir.path().to_owned()] }
        );
    }

    #[test]
    fn testify_is_offered_when_go_mod_requires_it() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("shop")).unwrap();
        let cart = dir.path().join("shop/cart.go");
        let ids = |root: &Path| GoTests.frameworks(root, &cart).into_iter().map(|f| f.id).collect::<Vec<_>>();
        std::fs::write(dir.path().join("go.mod"), "module x\n").unwrap();
        assert_eq!(ids(dir.path()), [TESTING]);
        std::fs::write(dir.path().join("go.mod"), "module x\n\nrequire github.com/stretchr/testify v1.9.0\n").unwrap();
        assert_eq!(ids(dir.path()), [TESTING, TESTIFY]);
    }

    #[test]
    fn a_new_go_test_is_a_table_driven_test_per_member_in_the_subjects_package() {
        let subject = GoTests.members(Path::new("/p/cart.go"), CART);
        let request = NewTest {
            framework: TESTING.into(),
            name: "cart_test".into(),
            destination: PathBuf::from("/p"),
            members: vec![subject[1].clone(), subject[2].clone()],
        };
        assert_eq!(GoTests.test_file(Path::new("/p/cart.go"), &request), Path::new("/p/cart_test.go"));
        let made = GoTests.generate(&request, Path::new("/p/cart.go"), CART, None).unwrap();
        assert!(made.text.starts_with(
            "package shop\n\nimport \"testing\"\n\nfunc TestCart_Total(t *testing.T) {\n\ttests := []struct {\n"
        ));
        assert!(made.text.contains("\nfunc Test_price(t *testing.T) {\n"));
        assert_eq!(made.line, 4);
        let again = GoTests.generate(&request, Path::new("/p/cart.go"), CART, Some(&made.text)).unwrap();
        assert_eq!(again.text, made.text, "members with tests get no second one");
        let testify =
            NewTest { framework: TESTIFY.into(), members: vec![subject[0].clone(), subject[3].clone()], ..request };
        let added = GoTests.generate(&testify, Path::new("/p/cart.go"), CART, Some(CART_TEST)).unwrap();
        assert!(added.text.contains("import \"github.com/stretchr/testify/assert\"\n"));
        assert!(added.text.ends_with("\t\t\tassert.NotEmpty(t, tt.name)\n\t\t})\n\t}\n}\n"));
        assert!(
            added.text.contains("func TestNew(t *testing.T)") && !added.text.contains("func TestCart(t"),
            "Cart has its tests"
        );
    }
}
