use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Default)]
struct Words {
    names: Vec<String>,
    neutral: Vec<String>,
    contexts: Vec<String>,
    issue: Option<Regex>,
}

const WORDS_ENV: &str = "ZEN_LEGAL_WORDS";

impl Words {
    fn parse(text: &str) -> Result<Words, String> {
        let mut words = Words::default();
        for (index, line) in text.lines().enumerate() {
            let entry = line.split("  #").next().unwrap_or_default().trim();
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            if let Some(token) = entry.strip_prefix("neutral:") {
                words.neutral.push(token.trim().to_lowercase());
            } else if let Some(context) = entry.strip_prefix("context:") {
                words.contexts.push(context.trim().to_lowercase());
            } else if let Some(pattern) = entry.strip_prefix("issue:") {
                let pattern = Regex::new(pattern.trim()).map_err(|error| format!("line {}: {error}", index + 1))?;
                words.issue = Some(pattern);
            } else {
                words.names.push(entry.to_lowercase());
            }
        }
        Ok(words)
    }

    fn from_environment() -> Result<Option<Words>, String> {
        let Some(path) = std::env::var_os(WORDS_ENV).filter(|path| !path.is_empty()) else { return Ok(None) };
        let path = Path::new(&path);
        let text =
            std::fs::read_to_string(path).map_err(|error| format!("{WORDS_ENV}: {}: {error}", path.display()))?;
        Words::parse(&text).map(Some).map_err(|error| format!("{WORDS_ENV}: {}: {error}", path.display()))
    }
}

const COMPATIBILITY: [(&str, &str); 26] = [
    ("import", "an import feature names the product whose files it reads"),
    ("export", "files another product exported, read by an import feature"),
    ("adopt", "run configurations adopted from another product's project files"),
    (".iml", "another product's module files, shared with it"),
    ("module file", "another product's module files, shared with it"),
    (".run/", "run configurations shared with another product"),
    ("dictionar", "project dictionaries shared with another product"),
    ("filetemplates", "file templates shared with another product"),
    (".idea", "another product's project folder, read for compatibility"),
    ("keymap", "keymaps imported from another product"),
    ("colour scheme", "colour schemes imported from another product"),
    ("color scheme", "colour schemes imported from another product"),
    (".icls", "another product's colour scheme files"),
    ("theme file", "theme files imported from other products"),
    ("colour theme", "colour themes imported from other products"),
    ("data-sources.json", "database connections imported from another product"),
    ("datasources.xml", "database connections imported from another product's project files"),
    ("credential store", "where another product keeps the passwords of imported connections"),
    ("ij_", "another product's `.editorconfig` properties"),
    ("kotlin-lsp", "a third-party language server, named with its vendor"),
    ("not affiliated", "the non-affiliation notice"),
    ("trademark", "the trademark notice"),
    ("theme-files", "the file filter of theme import"),
    ("импорт", "an import feature, in Russian"),
    ("цветов", "colour schemes and themes imported from other products, in Russian"),
    ("товарн", "the trademark notice, in Russian"),
];

const ALLOWED: [(&str, &str); 33] = [
    ("TRADEMARKS.md", "legal notice"),
    ("NOTICE", "legal notice"),
    ("ATTRIBUTION.md", "legal notice"),
    ("AGENTS.md", "golden rule 14 names what it forbids"),
    ("CONTRIBUTING.md", "the ground rules name what they forbid"),
    ("docs/ai-workflow.md", "the rules for agents name what they forbid"),
    ("xtask/src/legal.rs", "this guard's own wording patterns and tests"),
    ("crates/platform/jb-import/", "import of another product's settings formats"),
    ("crates/platform/theme/src/import", "import of other products' themes"),
    ("crates/platform/theme/tests/", "tests of theme import"),
    ("docs/jetbrains-compatibility.md", "the guide to importing another product's projects and settings"),
    ("docs/themes/import-", "the guides to importing other products' themes"),
    ("crates/platform/workbench/src/ide_settings", "import of another product's exported settings"),
    ("crates/platform/workbench/src/keymaps.rs", "import of another product's keymaps"),
    ("crates/platform/workbench/src/themes.rs", "import of other products' themes"),
    ("crates/platform/workbench/src/run.rs", "run configurations adopted from another product's project files"),
    ("crates/platform/workbench/src/project_settings.rs", "another product's project settings left unchanged"),
    (
        "crates/platform/workbench/src/code_style_scheme.rs",
        "code styles shared through another product's project files",
    ),
    ("crates/platform/workbench/src/editor_config/", "another product's `.editorconfig` properties"),
    ("crates/platform/workbench/src/database/imports.rs", "import of other products' database connections"),
    ("crates/platform/workbench/src/database/detect/", "detection of other products' database connections"),
    ("crates/platform/db/src/import.rs", "import of other products' database connections"),
    ("crates/platform/db/src/detect.rs", "detection of other products' database connections"),
    ("crates/platform/db/src/registry.rs", "where an imported database connection came from"),
    ("crates/platform/ui/src/ide/import_dialog.rs", "the database import dialog"),
    ("crates/platform/ui/src/ide/manage_settings/", "the settings import dialog"),
    ("crates/platform/ui/src/ide/app/manage_settings_tests.rs", "tests of the settings import dialog"),
    ("crates/platform/spell/", "proper nouns the dictionary knows, and another product's project dictionaries read"),
    ("crates/platform/workbench/src/spelling/", "project dictionaries kept in another product's format"),
    ("crates/platform/ui/src/ide/editor_config_view.rs", "another product's `.editorconfig` properties"),
    ("crates/platform/workbench/tests/kotlin.rs", "tests of a third-party language server, named with its vendor"),
    ("apps/go-garden/tests/idea_project.rs", "tests that another product's project files stay unchanged"),
    ("plugins/kotlin/", "a third-party language server, named with its vendor and terms"),
];

const SIGN_OFF_FROM: (i64, &str) = (
    1_794_441_600,
    "2026-11-12T00:00:00Z: commits written before the sign-off rule reached main (the licence wave) need none",
);

const ADR_HISTORY: (u32, &str) = (280, "an accepted ADR is never edited (golden rule 6); ADR-0280 states the rule");

const IMAGE_FOLDERS: [(&str, &str); 3] = [
    ("assets/", "our own icons and pictures"),
    ("deploy/releases/site/img/", "the website's own pictures"),
    ("docs/behaviour/ide/text-rendering/", "our own renders of the text-rendering spike"),
];

const IMAGE_EXTENSIONS: [&str; 11] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tif", "tiff", "svg"];

const SECRET_FIXTURES: [(&str, &str); 1] =
    [("crates/platform/net/tests/fixtures/", "throwaway keys and certificates of the TLS tests")];

static COPIED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[Cc]opied|[Pp]orted) from (?:the |an? )?(?:[A-Z]|upstream\b|reference\b)")
        .expect("a valid pattern")
});

static ADR_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([0-9]{4})-[a-z0-9-]+\.md\b").expect("a valid pattern"));

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        r"|glpat-[0-9A-Za-z_-]{20,}",
        r"|gh[pousr]_[0-9A-Za-z]{36,}",
        r"|AKIA[0-9A-Z]{16}",
        r"|xox[abprs]-[0-9A-Za-z-]{10,}",
        r"|sk-ant-[0-9A-Za-z_-]{20,}",
        r"|AIza[0-9A-Za-z_-]{35}",
    ))
    .expect("a valid pattern")
});

#[derive(Default)]
struct Change {
    added_lines: Vec<(String, usize, String)>,
    added_files: Vec<String>,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    match args.first().map(String::as_str) {
        Some("--all") => all(&root, &words()?),
        None => gate(&root),
        Some(other) => Err(format!("unknown argument `{other}`; use `cargo xtask legal [--all]`")),
    }
}

fn words() -> Result<Words, String> {
    Ok(Words::from_environment()?.unwrap_or_else(|| {
        println!("legal: names skipped: no word list ({WORDS_ENV} is not set); the other checks run");
        Words::default()
    }))
}

pub(crate) fn gate(root: &Path) -> Result<(), String> {
    let base = base(root)?;
    let problems = problems_since(root, &base, &words()?)?;
    if problems.is_empty() {
        return Ok(());
    }
    for problem in &problems {
        eprintln!("{problem}");
    }
    Err(format!(
        "{} problem(s) in this change break ADR-0280 and golden rule 14 (AGENTS.md), or the sign-off of CONTRIBUTING.md; \
         the allowlist of paths is in xtask/src/legal.rs",
        problems.len()
    ))
}

fn problems_since(root: &Path, base: &str, words: &Words) -> Result<Vec<String>, String> {
    let change = change(root, base)?;
    let mut problems = Vec::new();
    let mut files: BTreeMap<&str, Option<Vec<String>>> = BTreeMap::new();
    for (path, line, text) in &change.added_lines {
        let lines = files
            .entry(path.as_str())
            .or_insert_with(|| text_of(&root.join(path)).map(|text| text.lines().map(str::to_owned).collect()));
        let around = match lines {
            Some(lines) if lines.get(line.wrapping_sub(1)) == Some(text) => around(lines, line - 1),
            _ => text.clone(),
        };
        problems.extend(
            line_problems_in(words, path, text, &around).into_iter().map(|problem| format!("{path}:{line}: {problem}")),
        );
    }
    for path in &change.added_files {
        problems.extend(file_problems(words, path).into_iter().map(|problem| format!("{path}: {problem}")));
    }
    problems.extend(unsigned(root, base)?);
    Ok(problems)
}

fn all(root: &Path, words: &Words) -> Result<(), String> {
    let findings = tree(root, words)?;
    if findings.is_empty() {
        println!("legal: the tree has no findings");
        return Ok(());
    }
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, finding) in &findings {
        println!("{finding}");
        *per_file.entry(path).or_default() += 1;
    }
    Err(format!("legal: {} finding(s) in {} file(s)", findings.len(), per_file.len()))
}

fn tree(root: &Path, words: &Words) -> Result<Vec<(String, String)>, String> {
    let mut findings = Vec::new();
    for path in git_lines(root, &["ls-files", "--cached", "--others", "--exclude-standard"])? {
        if std::fs::symlink_metadata(root.join(&path)).is_ok_and(|meta| meta.file_type().is_symlink()) {
            continue;
        }
        findings.extend(
            file_problems(words, &path).into_iter().map(|problem| (path.clone(), format!("{path}: {problem}"))),
        );
        let Some(text) = text_of(&root.join(&path)) else { continue };
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        for (index, line) in lines.iter().enumerate() {
            findings.extend(
                line_problems_in(words, &path, line, &around(&lines, index))
                    .into_iter()
                    .map(|problem| (path.clone(), format!("{path}:{}: {problem}", index + 1))),
            );
        }
    }
    Ok(findings)
}

fn text_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn around(lines: &[String], index: usize) -> String {
    let first = index.saturating_sub(1);
    let last = (index + 1).min(lines.len().saturating_sub(1));
    lines[first..=last].join("\n")
}

fn line_problems_in(words: &Words, path: &str, line: &str, around: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if !allowed(path) {
        if let Some(name) = product_name(words, line)
            && !compatibility(words, around)
        {
            problems.push(format!("names another product (`{name}`)"));
        }
        if one_to_one(line) {
            problems.push("says \"one-to-one\" about our work".to_owned());
        }
        if let Some(found) = COPIED.find(line) {
            problems.push(format!("says where code came from (`{}`)", found.as_str()));
        }
        if let Some(found) = words.issue.as_ref().and_then(|issue| issue.find(line)) {
            problems.push(format!("cites another product's issue (`{}`)", found.as_str()));
        }
    }
    if !SECRET_FIXTURES.iter().any(|(prefix, _)| path.starts_with(prefix)) && SECRET.is_match(line) {
        problems.push("holds what looks like a secret".to_owned());
    }
    problems
}

fn file_problems(words: &Words, path: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let extension = path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()).unwrap_or_default();
    if IMAGE_EXTENSIONS.contains(&extension.as_str())
        && !IMAGE_FOLDERS.iter().any(|(prefix, _)| path.starts_with(prefix))
    {
        problems.push("an image outside the folders of our own pictures".to_owned());
    }
    if !allowed(path)
        && let Some(name) = product_name(words, path)
    {
        problems.push(format!("a file name with another product's name (`{name}`)"));
    }
    problems
}

fn compatibility(words: &Words, line: &str) -> bool {
    let lower = line.to_lowercase();
    COMPATIBILITY.iter().any(|(context, _)| lower.contains(context))
        || words.contexts.iter().any(|context| lower.contains(context.as_str()))
}

fn allowed(path: &str) -> bool {
    ALLOWED.iter().any(|(prefix, _)| path.starts_with(prefix)) || historical_adr(path)
}

fn historical_adr(path: &str) -> bool {
    path.strip_prefix("docs/adr/")
        .and_then(|name| name.get(..4))
        .and_then(|number| number.parse::<u32>().ok())
        .is_some_and(|number| number <= ADR_HISTORY.0)
}

fn product_name(words: &Words, line: &str) -> Option<String> {
    if words.names.is_empty() {
        return None;
    }
    let original = line.as_bytes();
    let mut lower = line.to_ascii_lowercase().into_bytes();
    for token in &words.neutral {
        blank(&mut lower, token.as_bytes());
    }
    for found in ADR_FILE.captures_iter(line) {
        let historical = found[1].parse::<u32>().is_ok_and(|number| number <= ADR_HISTORY.0);
        let whole = found.get(0).expect("the whole match");
        if historical {
            lower[whole.range()].fill(b' ');
        }
    }
    let lower = String::from_utf8_lossy(&lower).into_owned();
    words
        .names
        .iter()
        .find(|name| {
            lower
                .match_indices(name.as_str())
                .any(|(at, _)| starts_word(original, at) && ends_word(original, at + name.len()))
        })
        .cloned()
}

fn blank(text: &mut [u8], token: &[u8]) {
    let mut at = 0;
    while at + token.len() <= text.len() {
        if &text[at..at + token.len()] == token {
            text[at..at + token.len()].fill(b' ');
            at += token.len();
        } else {
            at += 1;
        }
    }
}

fn starts_word(text: &[u8], at: usize) -> bool {
    let Some(&before) = at.checked_sub(1).and_then(|index| text.get(index)) else { return true };
    let first = text.get(at).copied().unwrap_or(b' ');
    !before.is_ascii_alphanumeric()
        || ((before.is_ascii_lowercase() || before.is_ascii_digit()) && first.is_ascii_uppercase())
}

fn ends_word(text: &[u8], end: usize) -> bool {
    let Some(&after) = text.get(end) else { return true };
    let last = end.checked_sub(1).and_then(|index| text.get(index)).copied().unwrap_or(b' ');
    !after.is_ascii_alphanumeric()
        || after.is_ascii_digit()
        || (after.is_ascii_uppercase() && !last.is_ascii_uppercase())
}

fn one_to_one(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("one-to-one") && !["many-to-one", "cardinality", "relation"].iter().any(|word| lower.contains(word))
}

fn base(root: &Path) -> Result<String, String> {
    if let Ok(sha) = std::env::var("CI_MERGE_REQUEST_DIFF_BASE_SHA")
        && !sha.is_empty()
    {
        return Ok(sha);
    }
    Ok(git_lines(root, &["merge-base", "HEAD", "origin/main"])
        .ok()
        .and_then(|lines| lines.into_iter().next())
        .unwrap_or_else(|| {
            println!("legal: no origin/main to compare with, so only uncommitted changes are checked");
            "HEAD".to_owned()
        }))
}

fn change(root: &Path, base: &str) -> Result<Change, String> {
    let mut change = Change::default();
    let mut path = String::new();
    let mut line = 0usize;
    for text in git_lines(root, &["diff", "-M", "--unified=0", "--no-color", "--no-ext-diff", base])? {
        if let Some(target) = text.strip_prefix("+++ ") {
            path = target.trim_matches('"').strip_prefix("b/").unwrap_or(target).to_owned();
        } else if let Some(hunk) = text.strip_prefix("@@ ") {
            line = hunk
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse().ok())
                .unwrap_or(0);
        } else if let Some(added) = text.strip_prefix('+') {
            change.added_lines.push((path.clone(), line, added.to_owned()));
            line += 1;
        }
    }
    for status in git_lines(root, &["diff", "-M", "--name-status", "--diff-filter=ACR", base])? {
        if let Some(target) = status.rsplit('\t').next() {
            change.added_files.push(target.to_owned());
        }
    }
    for untracked in git_lines(root, &["ls-files", "--others", "--exclude-standard"])? {
        if let Some(text) = text_of(&root.join(&untracked)) {
            for (index, added) in text.lines().enumerate() {
                change.added_lines.push((untracked.clone(), index + 1, added.to_owned()));
            }
        }
        change.added_files.push(untracked);
    }
    Ok(change)
}

fn unsigned(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for commit in git_lines(root, &["rev-list", "--no-merges", &format!("{base}..HEAD")])? {
        let written = git_lines(root, &["log", "-1", "--format=%at", &commit])?;
        if written.first().and_then(|at| at.parse::<i64>().ok()).is_some_and(|at| at < SIGN_OFF_FROM.0) {
            continue;
        }
        let message = git_lines(root, &["log", "-1", "--format=%B", &commit])?;
        if !message.iter().any(|line| line.starts_with("Signed-off-by: ")) {
            let subject = message.first().cloned().unwrap_or_default();
            problems.push(format!(
                "commit {} ({subject}) has no Signed-off-by line; sign it off with `git commit -s`",
                &commit[..12.min(commit.len())]
            ));
        }
    }
    Ok(problems)
}

fn git_lines(root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{
        ALLOWED, COMPATIBILITY, IMAGE_FOLDERS, SECRET_FIXTURES, Words, file_problems, line_problems_in, problems_since,
    };

    const LIST: &str = "# made-up names: the real list is not in the repository\n\
        acme\n\
        orbitide\n\
        Orbit Code  # two words\n\
        orbitcode\n\
        \n\
        neutral: acme mono  # a font\n\
        neutral: org.acme\n\
        neutral: acme-json-language-server\n\
        context: acme project\n\
        issue: \\bORB-[0-9]+\n";

    fn words() -> Words {
        Words::parse(LIST).expect("a valid list")
    }

    fn line_problems(path: &str, line: &str) -> Vec<String> {
        line_problems_in(&words(), path, line, line)
    }

    #[test]
    fn the_word_list_holds_names_neutral_text_contexts_and_an_issue_pattern() {
        let words = words();
        assert_eq!(words.names, ["acme", "orbitide", "orbit code", "orbitcode"]);
        assert_eq!(words.neutral, ["acme mono", "org.acme", "acme-json-language-server"]);
        assert_eq!(words.contexts, ["acme project"]);
        assert!(words.issue.is_some());
        assert!(Words::parse("issue: (").is_err());
    }

    #[test]
    fn without_a_word_list_names_pass_and_the_other_checks_still_run() {
        let none = Words::default();
        for line in ["The popup filters as in Acme.", "Fixes ORB-12345."] {
            assert_eq!(line_problems_in(&none, "docs/guide/x.md", line, line), Vec::<String>::new(), "{line}");
        }
        assert_eq!(file_problems(&none, "docs/x/acme.png"), ["an image outside the folders of our own pictures"]);
        assert!(!line_problems_in(&none, "docs/guide/x.md", "The layout matches one-to-one.", "").is_empty());
        let key = format!("{}{}", "glpat-", "a".repeat(24));
        assert!(!line_problems_in(&none, "docs/guide/x.md", &key, &key).is_empty());
    }

    #[test]
    fn a_shared_project_file_named_on_the_next_line_makes_a_line_compatibility() {
        let (dir, base) = repository();
        let root = dir.path();
        let text = "Intro.\nThe mark is saved where Acme keeps it, in the project's module\nfile (`*.iml`).\n\nIt works like Acme does.\n";
        std::fs::write(root.join("guide.md"), text).expect("written");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-s", "-m", "guide"]);
        let problems = problems_since(root, &base, &words()).expect("checked");
        assert_eq!(problems, ["guide.md:5: names another product (`acme`)"], "only the comparison fails");
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git").args(args).current_dir(root).output().expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn repository() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("README.md"), "Zen IDE\nThe popup filters as in OrbitIDE.\n").expect("written");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-s", "-m", "start"]);
        let base = git(root, &["rev-parse", "HEAD"]);
        (dir, base)
    }

    #[test]
    fn a_commit_adding_a_screenshot_named_after_a_product_fails_and_old_lines_do_not() {
        let (dir, base) = repository();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs/x")).expect("created");
        std::fs::write(root.join("docs/x/orbitide.png"), [0x89, b'P', b'N', b'G', 0]).expect("written");
        std::fs::write(root.join("README.md"), "Zen IDE, our own\nThe popup filters as in OrbitIDE.\n")
            .expect("written");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-s", "-m", "add a screenshot"]);
        let problems = problems_since(root, &base, &words()).expect("checked");
        assert_eq!(
            problems,
            [
                "docs/x/orbitide.png: an image outside the folders of our own pictures",
                "docs/x/orbitide.png: a file name with another product's name (`orbitide`)",
            ]
        );
    }

    #[test]
    fn a_commit_without_a_sign_off_fails() {
        let (dir, base) = repository();
        let root = dir.path();
        std::fs::write(root.join("notes.md"), "ours\n").expect("written");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "unsigned"]);
        let problems = problems_since(root, &base, &Words::default()).expect("checked");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no Signed-off-by line"), "{problems:?}");
    }

    #[test]
    fn a_commit_written_before_the_sign_off_rule_needs_none() {
        let (dir, base) = repository();
        let root = dir.path();
        std::fs::write(root.join("notes.md"), "ours\n").expect("written");
        git(root, &["add", "."]);
        let output = Command::new("git")
            .args(["commit", "-q", "-m", "older"])
            .env("GIT_AUTHOR_DATE", "2026-11-11T22:51:25+08:00")
            .current_dir(root)
            .output()
            .expect("git runs");
        assert!(output.status.success());
        assert_eq!(problems_since(root, &base, &words()).expect("checked"), Vec::<String>::new());
    }

    #[test]
    fn a_new_screenshot_named_after_a_product_fails() {
        let problems = file_problems(&words(), "docs/x/orbitide.png");
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn product_names_fail_in_prose_and_in_identifiers() {
        for line in [
            "The popup filters as in OrbitIDE.",
            "let of_acme = 1;",
            "struct OrbitCodeLook;",
            "fn isAcmeLike() {}",
            "See acme-community for the details.",
            "use com.acme.openapi;",
            "Works like Orbit Code.",
        ] {
            assert!(!line_problems("crates/platform/ui/src/x.rs", line).is_empty(), "{line}");
        }
    }

    #[test]
    fn reference_wording_fails() {
        for line in [
            "The layout matches one-to-one.",
            "Ported from Upstream sources.",
            "copied from the Reference IDE",
            "Fixes ORB-12345.",
        ] {
            assert!(!line_problems("docs/guide/x.md", line).is_empty(), "{line}");
        }
    }

    #[test]
    fn everyday_words_and_compatibility_names_pass() {
        for line in [
            "Search Everywhere, Code Vision, Power Save Mode, Intentions, Scratches, Distraction Free Mode",
            "Tables copied from one connection into another.",
            "rows copied from a spreadsheet",
            "a relation is many-to-one, or one-to-one when its columns are unique",
            "The editor font is Acme Mono.",
            "id 'org.acme.kotlin.jvm'",
            "\"command\": \"acme-json-language-server\"",
            "the .idea folder of a project",
            "an acmefied module",
            "exported from Acme",
            "Acmes and Macme are words",
            "ide-settings-import-kind-acme = Settings exported from an Acme IDE: { $path }.",
            "Theme files (Acme, Orbit Code, Zen IDE): import one",
            "the Acme project stays as it was",
        ] {
            assert_eq!(line_problems("crates/platform/ui/src/x.rs", line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn import_code_legal_notices_and_accepted_adrs_may_name_products() {
        for path in [
            "crates/platform/jb-import/src/keymap/mod.rs",
            "TRADEMARKS.md",
            "docs/adr/0011-plugins-and-acme-compatibility.md",
            "docs/themes/import-orbit.md",
        ] {
            assert!(line_problems(path, "Acme and Orbit Code").is_empty(), "{path}");
        }
        assert!(!line_problems("docs/adr/0281-something.md", "as in OrbitIDE").is_empty());
    }

    #[test]
    fn a_link_to_an_accepted_adr_is_not_a_product_name_but_its_text_still_is() {
        for line in [
            "**Decision:** [ADR-0011](adr/0011-plugins-and-acme-compatibility.md)",
            "[ADR-0051](../../adr/0051-acme-keys-and-project-settings.md), specification:",
        ] {
            assert_eq!(line_problems("docs/behaviour.md", line), Vec::<String>::new(), "{line}");
        }
        assert!(
            !line_problems("docs/x.md", "[ADR-0011](adr/0011-plugins-and-acme-compatibility.md) as in OrbitIDE")
                .is_empty()
        );
        assert!(!line_problems("docs/x.md", "see 0300-follows-acme.md").is_empty(), "a later ADR's name is checked");
    }

    #[test]
    fn secrets_fail_everywhere_but_the_test_keys() {
        let key = format!("{}{}", "glpat-", "a".repeat(24));
        assert!(!line_problems("TRADEMARKS.md", &key).is_empty());
        let pem = format!("-----BEGIN {}-----", "PRIVATE KEY");
        assert!(line_problems("crates/platform/net/tests/fixtures/leaf.pk8.pem", &pem).is_empty());
        assert!(!line_problems("crates/platform/net/src/tls.rs", &pem).is_empty());
    }

    #[test]
    fn every_allowlist_entry_gives_its_reason() {
        let reasons = ALLOWED.iter().chain(&COMPATIBILITY).chain(&IMAGE_FOLDERS).chain(&SECRET_FIXTURES);
        for (entry, reason) in reasons {
            assert!(!entry.is_empty() && reason.split_whitespace().count() >= 2, "{entry}");
        }
    }
}
