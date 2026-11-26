use std::path::{Path, PathBuf};
use std::sync::Arc;

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A testing framework a project can write its tests with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestFramework {
    /// The framework's id, e.g. `go`, `testify`, `rust`, `junit5`.
    pub id: String,
    /// The name shown in the Create Test dialog.
    pub name: String,
}

/// A function, method or type of a file, as Go to Test and Create Test see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestMember {
    pub name: String,
    /// The type a method belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The line it starts on, zero-based.
    pub line: usize,
    /// The line after its last, zero-based.
    pub end_line: usize,
    /// Whether it is a test (or the module that holds tests).
    #[serde(default)]
    pub test: bool,
}

impl TestMember {
    pub fn label(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn holds(&self, line: usize) -> bool {
        (self.line..self.end_line.max(self.line + 1)).contains(&line)
    }
}

/// What the Create Test dialog offers first for a subject file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestDefaults {
    /// The test's name: its file's name without the extension (Go, Rust) or its class (JVM languages).
    pub name: String,
    /// The folders the test can go to, the usual one first.
    pub destinations: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTest {
    pub framework: String,
    pub name: String,
    pub destination: PathBuf,
    pub members: Vec<TestMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedTest {
    pub text: String,
    pub line: usize,
}

pub trait TestSupport: Send + Sync {
    fn id(&self) -> &str;

    fn handles(&self, path: &Path) -> bool;

    fn is_test_file(&self, path: &Path) -> bool;

    fn members(&self, path: &Path, text: &str) -> Vec<TestMember>;

    fn test_files(&self, root: &Path, subject: &Path) -> Vec<PathBuf>;

    fn subject_files(&self, root: &Path, test: &Path) -> Vec<PathBuf>;

    fn searched_files(&self, _path: &Path, _tests: bool) -> Vec<PathBuf> {
        Vec::new()
    }

    fn tests(&self, subject: &TestMember, test: &TestMember) -> bool;

    fn frameworks(&self, root: &Path, subject: &Path) -> Vec<TestFramework>;

    fn defaults(&self, root: &Path, subject: &Path) -> TestDefaults;

    fn test_file(&self, subject: &Path, request: &NewTest) -> PathBuf {
        let extension = subject.extension().map(|extension| extension.to_string_lossy().into_owned());
        let name = match extension {
            Some(extension) => format!("{}.{extension}", request.name),
            None => request.name.clone(),
        };
        request.destination.join(name)
    }

    fn generate(
        &self,
        request: &NewTest,
        subject: &Path,
        subject_text: &str,
        existing: Option<&str>,
    ) -> Result<GeneratedTest, String>;
}

pub type PluginTestSupports = Arc<dyn Fn() -> Vec<Arc<dyn TestSupport>> + Send + Sync>;

#[derive(Default, Clone)]
pub struct TestSupports {
    builtin: Vec<Arc<dyn TestSupport>>,
    plugins: Option<PluginTestSupports>,
}

impl TestSupports {
    pub fn add(&mut self, support: Arc<dyn TestSupport>) {
        if !self.builtin.iter().any(|known| known.id() == support.id()) {
            self.builtin.push(support);
        }
    }

    pub fn set_plugins(&mut self, plugins: PluginTestSupports) {
        self.plugins = Some(plugins);
    }

    pub fn all(&self) -> Vec<Arc<dyn TestSupport>> {
        let mut all = self.builtin.clone();
        if let Some(plugins) = &self.plugins {
            all.extend(plugins());
        }
        all
    }

    pub fn for_file(&self, path: &Path) -> Option<Arc<dyn TestSupport>> {
        self.all().into_iter().find(|support| support.handles(path))
    }
}

pub fn line_count(text: &str) -> usize {
    text.matches('\n').count() + usize::from(!text.ends_with('\n') && !text.is_empty())
}

pub fn line_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].matches('\n').count()
}

pub fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// One way a subject file and its test file are named, relative to a module's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestPairDeclaration {
    /// The subject's path, e.g. `src/main/java/{path}{name}.java`: `{path}` is any number of folders, each with its `/`, `{name}` one file name part.
    pub subject: String,
    /// The test's path with the same parts, e.g. `src/test/java/{path}{name}Test.java`.
    pub test: String,
}

/// A file a framework is detected by: the module's or the project's file that contains a text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestDetectDeclaration {
    pub file: String,
    pub contains: String,
}

/// A framework a plug-in writes tests with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestFrameworkDeclaration {
    pub id: String,
    pub name: String,
    /// The files that show the project uses it: the frameworks found so come first, the others after.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detect: Vec<TestDetectDeclaration>,
    /// A new test file: `${PACKAGE_LINE}`, `${PACKAGE}`, `${NAME}` (the test's name), `${SUBJECT}` (the subject's file name part) and `${METHODS}`.
    pub file: String,
    /// One test method: `${TEST}` (its name), `${MEMBER}` (the member it tests) and `${NAME}`.
    pub method: String,
}

/// The tests of one language, as a plug-in declares them in its plugin.json (`"tests"`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestDeclaration {
    pub id: String,
    /// The extensions of the language's files, without the dot.
    pub extensions: Vec<String>,
    pub pairs: Vec<TestPairDeclaration>,
    /// A regular expression that finds a member of a file, with its name in the group `name`; a name in backticks is taken without them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<String>,
    /// Words that are never a member: a match with one of them as a whole named group is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// How a test of a member is named: `{name}` is the member's name, `{Name}` the same with a capital letter; a test whose name starts with one tests it.
    pub tests: Vec<String>,
    /// The line that names the package, with `${PACKAGE}`; nothing for a test in no package.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package_line: String,
    /// Where methods go in a test file that exists: `end`, or `beforeLastBrace`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub insert: String,
    pub frameworks: Vec<TestFrameworkDeclaration>,
}

struct Pair {
    subject: Regex,
    test: Regex,
    declared: TestPairDeclaration,
}

pub struct DeclaredTests {
    declared: TestDeclaration,
    pairs: Vec<Pair>,
    members: Option<Regex>,
}

fn pattern(declared: &str) -> Result<Regex, String> {
    let mut expression = String::from("^(?P<root>(?:.*/)?)");
    let mut rest = declared;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("{path}") {
            expression.push_str("(?P<path>(?:[^/]+/)*)");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("{name}") {
            expression.push_str("(?P<name>[^/]+?)");
            rest = after;
        } else {
            let next = rest.char_indices().skip(1).find(|(_, c)| *c == '{').map_or(rest.len(), |(at, _)| at);
            expression.push_str(&regex::escape(&rest[..next]));
            rest = &rest[next..];
        }
    }
    expression.push('$');
    Regex::new(&expression).map_err(|error| format!("{declared}: {error}"))
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(template.to_owned(), |text, (key, value)| text.replace(&format!("${{{key}}}"), value))
}

impl DeclaredTests {
    pub fn new(declared: TestDeclaration) -> Result<Self, String> {
        let pairs = declared
            .pairs
            .iter()
            .map(|pair| {
                Ok(Pair { subject: pattern(&pair.subject)?, test: pattern(&pair.test)?, declared: pair.clone() })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let members = declared
            .members
            .as_deref()
            .map(|members| Regex::new(&format!("(?m){members}")).map_err(|error| error.to_string()))
            .transpose()?;
        Ok(Self { declared, pairs, members })
    }

    pub fn parse_list(json: &serde_json::Value) -> Vec<Self> {
        serde_json::from_value::<Vec<TestDeclaration>>(json.clone())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|declared| Self::new(declared).ok())
            .collect()
    }

    fn subject_parts(&self, subject: &Path) -> Option<(&Pair, String, String, String)> {
        let path = slashed(subject);
        self.pairs.iter().find_map(|pair| {
            let found = pair.subject.captures(&path)?;
            let part = |name: &str| found.name(name).map_or(String::new(), |part| part.as_str().to_owned());
            Some((pair, part("root"), part("path"), part("name")))
        })
    }

    fn package_of(&self, destination: &Path) -> String {
        let folder = format!("{}/", slashed(destination).trim_end_matches('/'));
        self.pairs
            .iter()
            .find_map(|pair| {
                let prefix =
                    pair.declared.test.split("{path}").next().filter(|_| pair.declared.test.contains("{path}"))?;
                let at = folder.rfind(prefix)?;
                Some(folder[at + prefix.len()..].trim_end_matches('/').replace('/', "."))
            })
            .unwrap_or_default()
    }

    fn test_names(&self, member: &str) -> Vec<String> {
        let upper = capitalized(member);
        self.declared.tests.iter().map(|name| name.replace("{Name}", &upper).replace("{name}", member)).collect()
    }
}

impl TestSupport for DeclaredTests {
    fn id(&self) -> &str {
        &self.declared.id
    }

    fn handles(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| {
            self.declared.extensions.iter().any(|known| known.eq_ignore_ascii_case(&extension.to_string_lossy()))
        })
    }

    fn is_test_file(&self, path: &Path) -> bool {
        let path = slashed(path);
        self.pairs.iter().any(|pair| pair.test.is_match(&path))
    }

    fn members(&self, path: &Path, text: &str) -> Vec<TestMember> {
        let Some(members) = &self.members else { return Vec::new() };
        let test = self.is_test_file(path);
        let mut found: Vec<TestMember> = members
            .captures_iter(text)
            .filter(|captures| {
                members
                    .capture_names()
                    .flatten()
                    .filter_map(|group| captures.name(group))
                    .all(|part| !self.declared.keywords.iter().any(|keyword| keyword == part.as_str()))
            })
            .filter_map(|captures| {
                let name = captures.name("name")?;
                let line = line_of(text, name.start());
                let shown = name.as_str().strip_prefix('`').and_then(|inner| inner.strip_suffix('`'));
                let name = shown.unwrap_or(name.as_str()).to_owned();
                Some(TestMember { name, owner: None, line, end_line: line + 1, test })
            })
            .collect();
        let total = line_count(text);
        for index in 0..found.len() {
            found[index].end_line = found.get(index + 1).map_or(total, |next| next.line).max(found[index].line + 1);
        }
        found
    }

    fn test_files(&self, _root: &Path, subject: &Path) -> Vec<PathBuf> {
        let path = slashed(subject);
        self.pairs
            .iter()
            .filter_map(|pair| {
                let found = pair.subject.captures(&path)?;
                let part = |name: &str| found.name(name).map_or("", |part| part.as_str());
                let test = pair.declared.test.replace("{path}", part("path")).replace("{name}", part("name"));
                Some(PathBuf::from(format!("{}{test}", part("root"))))
            })
            .collect()
    }

    fn subject_files(&self, _root: &Path, test: &Path) -> Vec<PathBuf> {
        let path = slashed(test);
        self.pairs
            .iter()
            .filter_map(|pair| {
                let found = pair.test.captures(&path)?;
                let part = |name: &str| found.name(name).map_or("", |part| part.as_str());
                let subject = pair.declared.subject.replace("{path}", part("path")).replace("{name}", part("name"));
                Some(PathBuf::from(format!("{}{subject}", part("root"))))
            })
            .collect()
    }

    fn tests(&self, subject: &TestMember, test: &TestMember) -> bool {
        !subject.test
            && test.test
            && self.test_names(&subject.name).iter().any(|name| test.name.starts_with(name.as_str()))
    }

    fn frameworks(&self, root: &Path, subject: &Path) -> Vec<TestFramework> {
        let module = self.subject_parts(subject).map(|(_, module, _, _)| PathBuf::from(module));
        let detected = |framework: &TestFrameworkDeclaration| {
            framework.detect.iter().any(|detect| {
                module.iter().map(PathBuf::as_path).chain([root]).any(|folder| {
                    std::fs::read_to_string(folder.join(&detect.file)).is_ok_and(|text| text.contains(&detect.contains))
                })
            })
        };
        let shown = |framework: &TestFrameworkDeclaration| TestFramework {
            id: framework.id.clone(),
            name: framework.name.clone(),
        };
        let (found, others): (Vec<_>, Vec<_>) =
            self.declared.frameworks.iter().partition(|framework| detected(framework));
        found.into_iter().chain(others).map(shown).collect()
    }

    fn defaults(&self, root: &Path, subject: &Path) -> TestDefaults {
        let Some(test) = self.test_files(root, subject).into_iter().next() else { return TestDefaults::default() };
        let name = test.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        TestDefaults { name, destinations: test.parent().map(Path::to_owned).into_iter().collect() }
    }

    fn generate(
        &self,
        request: &NewTest,
        subject: &Path,
        _subject_text: &str,
        existing: Option<&str>,
    ) -> Result<GeneratedTest, String> {
        let framework = self
            .declared
            .frameworks
            .iter()
            .find(|framework| framework.id == request.framework)
            .ok_or_else(|| format!("unknown framework {}", request.framework))?;
        let present = existing.map(|text| self.members(&self.test_file(subject, request), text)).unwrap_or_default();
        let made: Vec<(String, String)> = request
            .members
            .iter()
            .filter(|member| !present.iter().any(|test| self.tests(member, &TestMember { test: true, ..test.clone() })))
            .map(|member| {
                let test = self.test_names(&member.name).into_iter().next().unwrap_or_else(|| member.name.clone());
                let text =
                    fill(&framework.method, &[("TEST", &test), ("MEMBER", &member.name), ("NAME", &request.name)]);
                (test, text)
            })
            .collect();
        let methods: String = made.iter().map(|(_, text)| text.as_str()).collect();
        let first = made.first().and_then(|(test, text)| text.find(test.as_str())).unwrap_or(0);
        let subject_name = subject.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        match existing {
            None => {
                let package = self.package_of(&request.destination);
                let package_line = if package.is_empty() {
                    String::new()
                } else {
                    fill(&self.declared.package_line, &[("PACKAGE", &package)])
                };
                let head = fill(
                    &framework.file,
                    &[
                        ("PACKAGE_LINE", &package_line),
                        ("PACKAGE", &package),
                        ("NAME", &request.name),
                        ("SUBJECT", &subject_name),
                    ],
                );
                let at = head.find("${METHODS}").unwrap_or(head.len());
                let text = head.replacen("${METHODS}", &methods, 1);
                Ok(GeneratedTest { line: line_of(&text, at + first), text })
            }
            Some(existing) => {
                let at = match self.declared.insert.as_str() {
                    "beforeLastBrace" => existing.rfind('}').unwrap_or(existing.len()),
                    _ => existing.len(),
                };
                let mut text = existing.to_owned();
                text.insert_str(at, &methods);
                Ok(GeneratedTest { line: line_of(&text, at + first), text })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java() -> DeclaredTests {
        DeclaredTests::new(
            serde_json::from_value(serde_json::json!({
                "id": "java",
                "extensions": ["java"],
                "pairs": [{ "subject": "src/main/java/{path}{name}.java", "test": "src/test/java/{path}{name}Test.java" }],
                "members": "^\\s*(?:public |protected |private |static |final )*(?:void|int|String|boolean) (?P<name>\\w+)\\(",
                "tests": ["test{Name}", "{name}"],
                "packageLine": "package ${PACKAGE};\n\n",
                "insert": "beforeLastBrace",
                "frameworks": [
                    { "id": "junit5", "name": "JUnit 5", "detect": [{ "file": "pom.xml", "contains": "junit-jupiter" }],
                      "file": "${PACKAGE_LINE}import org.junit.jupiter.api.Test;\n\nclass ${NAME} {\n${METHODS}}\n",
                      "method": "\n    @Test\n    void ${TEST}() {\n    }\n" },
                    { "id": "testng", "name": "TestNG", "detect": [{ "file": "pom.xml", "contains": "testng" }],
                      "file": "class ${NAME} {\n${METHODS}}\n", "method": "    void ${TEST}() {}\n" }
                ]
            }))
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_declared_pair_leads_from_the_subject_to_its_test_and_back() {
        let tests = java();
        let subject = Path::new("/p/app/src/main/java/com/acme/Cart.java");
        let test = PathBuf::from("/p/app/src/test/java/com/acme/CartTest.java");
        assert!(tests.handles(subject) && !tests.is_test_file(subject) && tests.is_test_file(&test));
        assert_eq!(tests.test_files(Path::new("/p"), subject), std::slice::from_ref(&test));
        assert_eq!(tests.subject_files(Path::new("/p"), &test), [subject.to_owned()]);
        let defaults = tests.defaults(Path::new("/p"), subject);
        assert_eq!(defaults.name, "CartTest");
        assert_eq!(defaults.destinations, [PathBuf::from("/p/app/src/test/java/com/acme")]);
        assert_eq!(tests.package_of(&defaults.destinations[0]), "com.acme");
    }

    #[test]
    fn declared_members_are_tested_by_the_names_the_plugin_gives() {
        let tests = java();
        let subject =
            tests.members(Path::new("/p/src/main/java/Cart.java"), "class Cart {\n  public int total() {\n  }\n}\n");
        assert_eq!(
            (subject[0].name.as_str(), subject[0].line, subject[0].end_line, subject[0].test),
            ("total", 1, 4, false)
        );
        let test = tests
            .members(Path::new("/p/src/test/java/CartTest.java"), "class CartTest {\n  void testTotalIsZero() {\n");
        assert!(tests.tests(&subject[0], &test[0]));
        assert!(!tests.tests(&test[0], &subject[0]));
    }

    #[test]
    fn a_declared_framework_writes_a_new_test_or_adds_its_methods_before_the_last_brace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<artifactId>junit-jupiter</artifactId>").unwrap();
        let tests = java();
        let subject = dir.path().join("src/main/java/com/acme/Cart.java");
        assert_eq!(
            tests.frameworks(dir.path(), &subject).iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            ["junit5", "testng"]
        );
        std::fs::write(dir.path().join("pom.xml"), "<artifactId>testng</artifactId>").unwrap();
        assert_eq!(
            tests.frameworks(dir.path(), &subject).iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            ["testng", "junit5"],
            "the frameworks the project depends on come first"
        );
        std::fs::write(dir.path().join("pom.xml"), "<artifactId>junit-jupiter</artifactId>").unwrap();
        let member = TestMember { name: "total".into(), owner: None, line: 1, end_line: 2, test: false };
        let request = NewTest {
            framework: "junit5".into(),
            name: "CartTest".into(),
            destination: dir.path().join("src/test/java/com/acme"),
            members: vec![member.clone()],
        };
        assert_eq!(tests.test_file(&subject, &request), dir.path().join("src/test/java/com/acme/CartTest.java"));
        let made = tests.generate(&request, &subject, "", None).unwrap();
        assert_eq!(
            made.text,
            "package com.acme;\n\nimport org.junit.jupiter.api.Test;\n\nclass CartTest {\n\n    @Test\n    void testTotal() {\n    }\n}\n"
        );
        assert_eq!(made.line, 7);
        let again = tests.generate(&request, &subject, "", Some(&made.text)).unwrap();
        assert_eq!(again.text, made.text, "a member with a test gets no second one");
        let other = NewTest { members: vec![TestMember { name: "clear".into(), ..member }], ..request };
        let added = tests.generate(&other, &subject, "", Some(&made.text)).unwrap();
        assert!(added.text.ends_with("    void testTotal() {\n    }\n\n    @Test\n    void testClear() {\n    }\n}\n"));
    }

    fn plugin(manifest: &str) -> DeclaredTests {
        let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
        let mut declared = DeclaredTests::parse_list(&manifest["tests"]);
        assert_eq!(declared.len(), 1, "the plug-in's declaration reads");
        declared.pop().unwrap()
    }

    fn plugin_java() -> DeclaredTests {
        plugin(include_str!("../../../../plugins/java/plugin.json"))
    }

    fn plugin_kotlin() -> DeclaredTests {
        plugin(include_str!("../../../../plugins/kotlin/plugin.json"))
    }

    fn names(tests: &DeclaredTests, path: &str, text: &str) -> Vec<(String, usize)> {
        tests.members(Path::new(path), text).into_iter().map(|member| (member.name, member.line)).collect()
    }

    fn ids(tests: &DeclaredTests, root: &Path, subject: &Path) -> Vec<String> {
        tests.frameworks(root, subject).into_iter().map(|framework| framework.id).collect()
    }

    #[test]
    fn the_java_declaration_finds_methods_of_every_common_shape_and_no_statements() {
        let text = "class Cart<T extends Item> {\n\
            \x20   public Cart(int size) {\n\
            \x20       this.items = new ArrayList<>(size);\n\
            \x20       return;\n\
            \x20   }\n\
            \x20   @Override public int total() {\n\
            \x20       return sum(items);\n\
            \x20   }\n\
            \x20   @SuppressWarnings(\"unchecked\")\n\
            \x20   public static <K, V extends List<K>> Map<K, List<V>> group(Map<K, V> m) {\n\
            \x20       new Thread(() -> {}).start();\n\
            \x20       throw new IllegalStateException(\"x\");\n\
            \x20   }\n\
            \x20   protected final String[] names() throws IOException {\n\
            \x20       else if (x) {}\n\
            \x20       yield compute(1);\n\
            \x20   }\n\
            \x20   void clear()\n\
            \x20   private record Line(String sku) {}\n\
            \x20   java.util.Optional<T> first() {}\n\
            }\n";
        assert_eq!(
            names(&plugin_java(), "/p/src/main/java/Cart.java", text),
            [
                ("total".into(), 5),
                ("group".into(), 9),
                ("names".into(), 13),
                ("clear".into(), 17),
                ("first".into(), 19)
            ]
        );
    }

    #[test]
    fn the_kotlin_declaration_finds_functions_with_receivers_generics_and_backticked_names() {
        let text = "class Cart {\n\
            \x20   fun total(): Int = 0\n\
            \x20   override suspend fun load(id: String) {}\n\
            \x20   fun <T : Comparable<T>> List<T>.largest(): T? = null\n\
            \x20   fun String?.orBlank() = this ?: \"\"\n\
            \x20   val size = items.count()\n\
            \x20   @Test fun `total of an empty cart is zero`() {}\n\
            \x20   private inline fun <reified T> find(): T? = null\n\
            }\n";
        assert_eq!(
            names(&plugin_kotlin(), "/p/src/main/kotlin/Cart.kt", text),
            [
                ("total".into(), 1),
                ("load".into(), 2),
                ("largest".into(), 3),
                ("orBlank".into(), 4),
                ("total of an empty cart is zero".into(), 6),
                ("find".into(), 7)
            ]
        );
        let kotlin = plugin_kotlin();
        let subject = &kotlin.members(Path::new("/p/src/main/kotlin/Cart.kt"), text)[0];
        let test = &kotlin.members(Path::new("/p/src/test/kotlin/CartTest.kt"), text)[4];
        assert!(kotlin.tests(subject, test), "a test named in backticks after a function tests it");
    }

    #[test]
    fn the_jvm_declarations_pair_code_with_test_test_tests_it_and_a_test_prefix() {
        for (tests, main, test, extension) in [
            (plugin_java(), "src/main/java", "src/test/java", "java"),
            (plugin_kotlin(), "src/main/kotlin", "src/test/kotlin", "kt"),
            (plugin_kotlin(), "src/main/java", "src/test/java", "kt"),
            (plugin_kotlin(), "src/commonMain/kotlin", "src/commonTest/kotlin", "kt"),
        ] {
            let subject = PathBuf::from(format!("/p/shop/{main}/com/acme/Cart.{extension}"));
            let paired: Vec<PathBuf> = ["CartTest", "CartTests", "CartIT", "TestCart"]
                .iter()
                .map(|name| PathBuf::from(format!("/p/shop/{test}/com/acme/{name}.{extension}")))
                .collect();
            assert_eq!(tests.test_files(Path::new("/p"), &subject), paired, "{main}");
            for file in &paired {
                assert!(tests.is_test_file(file) && !tests.is_test_file(&subject), "{}", file.display());
                assert_eq!(tests.subject_files(Path::new("/p"), file).first(), Some(&subject), "{}", file.display());
            }
            let defaults = tests.defaults(Path::new("/p"), &subject);
            assert_eq!(defaults.name, "CartTest");
            assert_eq!(tests.package_of(&defaults.destinations[0]), "com.acme");
        }
    }

    #[test]
    fn the_jvm_frameworks_the_build_depends_on_come_first() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("shop");
        std::fs::create_dir_all(dir.path().join("gradle")).unwrap();
        std::fs::create_dir_all(&module).unwrap();
        let java = plugin_java();
        let subject = module.join("src/main/java/com/acme/Cart.java");
        assert_eq!(ids(&java, dir.path(), &subject), ["junit5", "junit4", "testng"]);
        std::fs::write(module.join("pom.xml"), "<groupId>org.testng</groupId>").unwrap();
        assert_eq!(ids(&java, dir.path(), &subject), ["testng", "junit5", "junit4"]);
        std::fs::remove_file(module.join("pom.xml")).unwrap();
        std::fs::write(module.join("build.gradle.kts"), "testImplementation(\"junit:junit:4.13.2\")").unwrap();
        assert_eq!(ids(&java, dir.path(), &subject), ["junit4", "junit5", "testng"]);
        std::fs::write(dir.path().join("gradle/libs.versions.toml"), "junit = \"org.junit.jupiter:junit-jupiter\"")
            .unwrap();
        assert_eq!(ids(&java, dir.path(), &subject), ["junit5", "junit4", "testng"]);
        let kotlin = plugin_kotlin();
        let subject = module.join("src/main/kotlin/com/acme/Cart.kt");
        std::fs::remove_file(dir.path().join("gradle/libs.versions.toml")).unwrap();
        std::fs::write(module.join("build.gradle.kts"), "testImplementation(kotlin(\"test\"))").unwrap();
        assert_eq!(ids(&kotlin, dir.path(), &subject), ["kotlin.test", "junit5", "junit4", "testng"]);
    }

    #[test]
    fn every_jvm_framework_writes_set_up_tear_down_and_a_stub_per_member() {
        let member = TestMember { name: "total".into(), owner: None, line: 1, end_line: 2, test: false };
        for (tests, extension, expected) in [
            (
                plugin_java(),
                "java",
                [
                    (
                        "junit5",
                        [
                            "import org.junit.jupiter.api.Test;",
                            "@BeforeEach\n    void setUp()",
                            "@AfterEach\n    void tearDown()",
                            "@Test\n    void total()",
                        ],
                    ),
                    (
                        "junit4",
                        [
                            "import org.junit.Test;",
                            "@Before\n    public void setUp()",
                            "@After\n    public void tearDown()",
                            "@Test\n    public void total()",
                        ],
                    ),
                    (
                        "testng",
                        [
                            "import org.testng.annotations.Test;",
                            "@BeforeMethod\n    public void setUp()",
                            "@AfterMethod\n    public void tearDown()",
                            "@Test\n    public void total()",
                        ],
                    ),
                ]
                .to_vec(),
            ),
            (
                plugin_kotlin(),
                "kt",
                [
                    (
                        "junit5",
                        [
                            "import org.junit.jupiter.api.Test\n",
                            "@BeforeEach\n    fun setUp()",
                            "@AfterEach\n    fun tearDown()",
                            "@Test\n    fun total()",
                        ],
                    ),
                    (
                        "junit4",
                        [
                            "import org.junit.Test\n",
                            "@Before\n    fun setUp()",
                            "@After\n    fun tearDown()",
                            "@Test\n    fun total()",
                        ],
                    ),
                    (
                        "testng",
                        [
                            "import org.testng.annotations.Test\n",
                            "@BeforeMethod\n    fun setUp()",
                            "@AfterMethod\n    fun tearDown()",
                            "@Test\n    fun total()",
                        ],
                    ),
                    (
                        "kotlin.test",
                        [
                            "import kotlin.test.Test\n",
                            "@BeforeTest\n    fun setUp()",
                            "@AfterTest\n    fun tearDown()",
                            "@Test\n    fun total()",
                        ],
                    ),
                ]
                .to_vec(),
            ),
        ] {
            let folder = if extension == "kt" { "kotlin" } else { "java" };
            let subject = PathBuf::from(format!("/p/src/main/{folder}/com/acme/Cart.{extension}"));
            for (framework, parts) in expected {
                let request = NewTest {
                    framework: framework.into(),
                    name: "CartTest".into(),
                    destination: PathBuf::from(format!("/p/src/test/{folder}/com/acme")),
                    members: vec![member.clone()],
                };
                let made = tests.generate(&request, &subject, "", None).unwrap();
                assert!(made.text.starts_with("package com.acme"), "{framework}: {}", made.text);
                for part in parts {
                    assert!(made.text.contains(part), "{framework} {extension}: {part:?} in {}", made.text);
                }
                assert_eq!(
                    made.text.lines().nth(made.line),
                    Some(if extension == "kt" {
                        "    fun total() {"
                    } else if framework == "junit5" {
                        "    void total() {"
                    } else {
                        "    public void total() {"
                    })
                );
                let present = tests.members(&tests.test_file(&subject, &request), &made.text);
                assert_eq!(present.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["setUp", "tearDown", "total"]);
            }
        }
    }
}
