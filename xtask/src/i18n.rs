use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type Messages = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn parse(text: &str) -> Messages {
    let mut messages = Messages::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let starts_message = line.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
        if starts_message && let Some((id, _)) = line.split_once('=') {
            let id = id.trim().to_owned();
            messages.entry(id.clone()).or_default();
            current = Some(id);
        }
        if let Some(id) = &current {
            let variables = messages.entry(id.clone()).or_default();
            let mut rest = line;
            while let Some(index) = rest.find('$') {
                rest = &rest[index + 1..];
                let name: String =
                    rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
                if !name.is_empty() {
                    variables.insert(name);
                }
            }
        }
    }
    messages
}

pub(crate) fn compare(file: &str, english: &Messages, translation: &Messages) -> Vec<String> {
    let mut problems = Vec::new();
    for (id, variables) in english {
        match translation.get(id) {
            None => problems.push(format!("{file}: missing message `{id}`")),
            Some(translated) if translated != variables => problems
                .push(format!("{file}: message `{id}` uses {translated:?}, but the English source uses {variables:?}")),
            Some(_) => {}
        }
    }
    for id in translation.keys().filter(|id| !english.contains_key(*id)) {
        problems.push(format!("{file}: message `{id}` does not exist in English"));
    }
    problems
}

fn locale_dirs(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if !path.is_dir() || name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if name == "locales" {
            found.push(path);
        } else {
            locale_dirs(&path, found);
        }
    }
}

fn rust_files(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            let skipped = name.starts_with('.') || ["target", "tests", "benches", "examples"].contains(&&*name);
            if !skipped {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn blank_until_quote(code: &mut [u8], start: usize, hashes: Option<usize>) -> usize {
    let mut at = start;
    while at < code.len() {
        match hashes {
            None if code[at] == b'\\' => {
                code[at] = b' ';
                if at + 1 < code.len() {
                    code[at + 1] = b' ';
                }
                at += 2;
            }
            None if code[at] == b'"' => return at + 1,
            Some(count)
                if code[at] == b'"'
                    && code.len() - at > count
                    && code[at + 1..=at + count].iter().all(|b| *b == b'#') =>
            {
                return at + 1 + count;
            }
            _ => {
                code[at] = b' ';
                at += 1;
            }
        }
    }
    at
}

fn blank_comments_and_strings(text: &[u8]) -> Vec<u8> {
    let mut code = text.to_vec();
    let mut at = 0;
    while at < code.len() {
        let word_before = at > 0 && (code[at - 1].is_ascii_alphanumeric() || code[at - 1] == b'_');
        match code[at] {
            b'/' if code.get(at + 1) == Some(&b'/') => {
                while at < code.len() && code[at] != b'\n' {
                    code[at] = b' ';
                    at += 1;
                }
            }
            b'/' if code.get(at + 1) == Some(&b'*') => {
                let mut depth = 0;
                while at < code.len() {
                    if code[at..].starts_with(b"/*") {
                        depth += 1;
                        code[at..at + 2].fill(b' ');
                        at += 2;
                    } else if code[at..].starts_with(b"*/") {
                        depth -= 1;
                        code[at..at + 2].fill(b' ');
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        code[at] = b' ';
                        at += 1;
                    }
                }
            }
            b'"' => at = blank_until_quote(&mut code, at + 1, None),
            b'r' | b'b' | b'c' if !word_before => {
                let mut next = at + 1;
                if code[at] != b'r' && code.get(next) == Some(&b'r') {
                    next += 1;
                }
                let raw = code[at] == b'r' || code[at + 1] == b'r';
                let hashes = code[next..].iter().take_while(|b| **b == b'#').count();
                if raw && code.get(next + hashes) == Some(&b'"') {
                    at = blank_until_quote(&mut code, next + hashes + 1, Some(hashes));
                } else {
                    at += 1;
                }
            }
            b'\'' => {
                let end = if code.get(at + 1) == Some(&b'\\') {
                    code[at + 2..].iter().skip(1).position(|b| *b == b'\'').map(|index| at + 3 + index)
                } else {
                    let width = code.get(at + 1).map_or(1, |b| b.leading_ones().max(1) as usize);
                    (code.get(at + 1 + width) == Some(&b'\'')).then_some(at + 1 + width)
                };
                match end {
                    Some(end) => {
                        code[at..=end].fill(b' ');
                        at = end + 1;
                    }
                    None => at += 1,
                }
            }
            _ => at += 1,
        }
    }
    code
}

fn find(code: &[u8], what: &[u8], from: usize) -> Option<usize> {
    code[from..].windows(what.len()).position(|window| window == what).map(|index| from + index)
}

fn module_declared(item: &[u8]) -> Option<String> {
    let item = std::str::from_utf8(item).ok()?;
    let name = item.split_whitespace().skip_while(|word| *word != "mod").nth(1)?;
    Some(name.trim_end_matches(';').to_owned())
}

fn blank_test_items(code: &mut [u8]) -> Vec<String> {
    let mut modules = Vec::new();
    for attribute in [&b"#[cfg(test)]"[..], b"#[cfg(all(test,"] {
        let mut from = 0;
        while let Some(start) = find(code, attribute, from) {
            let mut at = start + attribute.len();
            let mut brackets = 0;
            let mut close_of_attribute = attribute.ends_with(b",");
            while at < code.len() {
                match code[at] {
                    b'[' | b'(' => brackets += 1,
                    b']' | b')' if close_of_attribute && brackets == 0 => close_of_attribute = code[at] != b']',
                    b']' | b')' => brackets -= 1,
                    b';' | b'{' if brackets == 0 => break,
                    _ => {}
                }
                at += 1;
            }
            if code.get(at) == Some(&b'{') {
                let mut depth = 0;
                while at < code.len() {
                    match code[at] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    at += 1;
                    if depth == 0 {
                        break;
                    }
                }
            } else {
                modules.extend(module_declared(&code[start..at.min(code.len())]));
            }
            let end = at.min(code.len());
            code[start..end].fill(b' ');
            from = end;
        }
    }
    modules
}

fn test_module_paths(file: &Path, module: &str) -> Vec<PathBuf> {
    let folder = file.parent().unwrap_or(Path::new("."));
    let own = match file.file_stem().and_then(|stem| stem.to_str()) {
        Some("lib" | "main" | "mod") | None => folder.to_owned(),
        Some(stem) => folder.join(stem),
    };
    vec![own.join(format!("{module}.rs")), own.join(module)]
}

#[derive(Debug, Default, PartialEq, Eq)]
struct IdsInCode {
    named: Vec<(usize, String)>,
    computed: usize,
}

fn ids_in_code(text: &[u8], code: &[u8]) -> IdsInCode {
    let mut ids = IdsInCode::default();
    let mut from = 0;
    while let Some(start) = find(code, b"t!", from) {
        from = start + 2;
        let word_before = start > 0 && (code[start - 1].is_ascii_alphanumeric() || code[start - 1] == b'_');
        let open = from + code[from..].iter().take_while(|b| b.is_ascii_whitespace()).count();
        if word_before || code.get(open) != Some(&b'(') {
            continue;
        }
        let first = open + 1 + code[open + 1..].iter().take_while(|b| b.is_ascii_whitespace()).count();
        let literal = (code.get(first) == Some(&b'"'))
            .then(|| code[first + 1..].iter().position(|b| *b == b'"').map(|length| first + 1 + length))
            .flatten();
        let after = literal.map(|end| end + 1 + code[end + 1..].iter().take_while(|b| b.is_ascii_whitespace()).count());
        match (literal, after.and_then(|at| code.get(at))) {
            (Some(end), Some(b',' | b')')) => {
                let line = text[..start].iter().filter(|b| **b == b'\n').count() + 1;
                ids.named.push((line, String::from_utf8_lossy(&text[first + 1..end]).into_owned()));
            }
            _ => ids.computed += 1,
        }
    }
    ids
}

fn check_code(root: &Path, known: &BTreeSet<String>) -> Result<(Vec<String>, usize, usize), String> {
    let mut files = Vec::new();
    rust_files(root, &mut files);
    let mut read = Vec::new();
    let mut test_paths = Vec::new();
    for file in files {
        let text = std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let mut code = blank_comments_and_strings(&text);
        for module in blank_test_items(&mut code) {
            test_paths.extend(test_module_paths(&file, &module));
        }
        read.push((file, text, code));
    }
    let mut problems = Vec::new();
    let (mut checked, mut computed) = (0, 0);
    for (file, text, code) in &read {
        if test_paths.iter().any(|test| file.starts_with(test)) {
            continue;
        }
        let ids = ids_in_code(text, code);
        computed += ids.computed;
        checked += ids.named.len();
        for (line, id) in ids.named.iter().filter(|(_, id)| !known.contains(id)) {
            problems.push(format!("{}:{line}: `{id}` has no English message", file.display()));
        }
    }
    Ok((problems, checked, computed))
}

pub(crate) fn run() -> Result<(), String> {
    run_in(Path::new("."))
}

fn run_in(root: &Path) -> Result<(), String> {
    let mut dirs = Vec::new();
    locale_dirs(root, &mut dirs);
    let mut problems = Vec::new();
    let mut known = BTreeSet::new();
    let mut files = 0;
    for dir in &dirs {
        let english_dir = dir.join("en");
        let Ok(english_files) = std::fs::read_dir(&english_dir) else {
            problems.push(format!("{}: no `en` directory", dir.display()));
            continue;
        };
        let languages: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && !path.ends_with("en"))
            .collect();
        for english_file in english_files
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "ftl"))
        {
            let english = parse(&std::fs::read_to_string(&english_file).map_err(|error| error.to_string())?);
            files += 1;
            known.extend(english.keys().cloned());
            for language in &languages {
                let translated_file = language.join(english_file.file_name().unwrap_or_default());
                match std::fs::read_to_string(&translated_file) {
                    Ok(text) => {
                        problems.extend(compare(&translated_file.display().to_string(), &english, &parse(&text)))
                    }
                    Err(_) => problems.push(format!("{}: missing translation file", translated_file.display())),
                }
            }
        }
    }
    let (missing, checked, computed) = check_code(root, &known)?;
    problems.extend(missing);
    if problems.is_empty() {
        println!(
            "i18n: ok ({files} source files in {} locale directories; {checked} ids in code found, {computed} computed \
             ids not checked)",
            dirs.len()
        );
        Ok(())
    } else {
        for problem in &problems {
            eprintln!("error: {problem}");
        }
        Err(format!("i18n: {} problem(s); see docs/adr/0024-localisation.md", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_variables() {
        let english = parse(
            "# comment\nhello = Hello, { $name }!\nupdates = { $count ->\n    [one] One\n   *[other] { $count } updates\n}\n",
        );
        assert_eq!(english.len(), 2);
        assert_eq!(english["hello"], BTreeSet::from(["name".to_owned()]));
        assert_eq!(english["updates"], BTreeSet::from(["count".to_owned()]));

        let good = parse("hello = Привет, { $name }!\nupdates = { $count ->\n   *[other] { $count } обновлений\n}\n");
        assert!(compare("ru", &english, &good).is_empty());

        let bad = parse("hello = Привет!\nextra = Лишнее\n");
        let problems = compare("ru", &english, &bad);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    fn write(root: &Path, file: &str, text: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn project(source: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "krate/locales/en/a.ftl", "known = Known\n");
        write(dir.path(), "krate/locales/ru/a.ftl", "known = Известно\n");
        write(dir.path(), "krate/src/lib.rs", source);
        dir
    }

    #[test]
    fn an_id_in_code_without_a_message_fails() {
        let dir = project("fn a() { zen_i18n::t!(\"known\"); }\nfn b() { t!(\n    \"missing-one\", count = 1); }\n");
        assert!(run_in(dir.path()).is_err());
        let known = BTreeSet::from(["known".to_owned()]);
        let (problems, checked, computed) = check_code(dir.path(), &known).unwrap();
        assert_eq!((checked, computed), (2, 0));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].ends_with("krate/src/lib.rs:2: `missing-one` has no English message"), "{problems:?}");
        write(dir.path(), "krate/locales/en/a.ftl", "known = Known\nmissing-one = One\n");
        write(dir.path(), "krate/locales/ru/a.ftl", "known = Известно\nmissing-one = Один\n");
        assert_eq!(run_in(dir.path()), Ok(()));
    }

    #[test]
    fn test_code_comments_and_strings_are_not_read_and_computed_ids_are_counted() {
        let source = r##"
//! t!("in-a-doc")
fn shown() {
    let text = "t!(\"in-a-string\")";
    let raw = r#"t!("in-a-raw-string")"#;
    let quote = '"';
    print!("{quote}");
    t!("known");
    t!(&format!("known-{text}"));
    t!(raw);
    t!("known-".to_owned() + raw);
    /* t!("in-a-comment") */
}
#[cfg(test)]
fn helper() { t!("in-a-test-function"); }
#[cfg(test)]
mod tests { fn a() { t!("in-a-test-module"); } }
#[cfg(test)]
mod more_tests;
#[cfg(test)]
#[path = "elsewhere.rs"]
mod path_tests;
"##;
        let dir = project(source);
        write(dir.path(), "krate/src/more_tests.rs", "fn a() { t!(\"in-a-test-file\"); }\n");
        write(dir.path(), "krate/tests/it.rs", "fn a() { t!(\"in-an-integration-test\"); }\n");
        let known = BTreeSet::from(["known".to_owned()]);
        let (problems, checked, computed) = check_code(dir.path(), &known).unwrap();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!((checked, computed), (1, 3));
    }

    #[test]
    fn a_test_module_beside_a_file_belongs_to_that_file() {
        let dir = project("mod feature;\n");
        write(dir.path(), "krate/src/feature.rs", "#[cfg(test)]\nmod tests;\n");
        write(dir.path(), "krate/src/feature/tests.rs", "fn a() { t!(\"in-a-test-file\"); }\n");
        write(dir.path(), "krate/src/feature/shown.rs", "fn a() { t!(\"is-shown\"); }\n");
        let known = BTreeSet::new();
        let (problems, ..) = check_code(dir.path(), &known).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("is-shown"), "{problems:?}");
    }
}
