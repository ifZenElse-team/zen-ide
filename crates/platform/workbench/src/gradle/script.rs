use std::ops::Range;

use zen_i18n::t;

pub const CONFIGURATIONS: [&str; 8] = [
    "implementation",
    "api",
    "compileOnly",
    "runtimeOnly",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "annotationProcessor",
];

const WRAPPERS: [&str; 3] = ["platform", "enforcedPlatform", "testFixtures"];
const NOT_RESOLVED_FROM: [&str; 3] = ["buildscript", "pluginManagement", "publishing"];
const CENTRAL_WORD: &str = "mavenCentral";
const GOOGLE_WORD: &str = "google";
const PORTAL_WORD: &str = "gradlePluginPortal";
const DEPENDENCY_MANAGEMENT_PLUGIN: &str = "io.spring.dependency-management";
pub const GOOGLE: &str = "https://dl.google.com/dl/android/maven2";
pub const PORTAL: &str = "https://plugins.gradle.org/m2";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Statement {
    pub configuration: String,
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub version_range: Option<Range<usize>>,
    pub variable: Option<String>,
    pub accessor: Option<String>,
    pub platform: bool,
    pub range: Range<usize>,
}

fn skip_string(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let triple = bytes.len() >= at + 3 && bytes[at + 1] == quote && bytes[at + 2] == quote;
    let mut index = at + if triple { 3 } else { 1 };
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == quote {
            if !triple {
                return index + 1;
            }
            if bytes.len() >= index + 3 && bytes[index + 1] == quote && bytes[index + 2] == quote {
                return index + 3;
            }
        }
        if !triple && bytes[index] == b'\n' {
            return index;
        }
        index += 1;
    }
    bytes.len()
}

pub fn masked(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'\'' => index = skip_string(&bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    bytes[index] = b' ';
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut end = index + 2;
                while end < bytes.len() && !(bytes[end] == b'*' && bytes.get(end + 1) == Some(&b'/')) {
                    end += 1;
                }
                let end = (end + 2).min(bytes.len());
                for byte in &mut bytes[index..end] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
                index = end;
            }
            _ => index += 1,
        }
    }
    String::from_utf8(bytes).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    pub parents: Vec<String>,
    pub body: Range<usize>,
}

fn word_before(bytes: &[u8], mut end: usize) -> String {
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    String::from_utf8_lossy(&bytes[start..end]).into_owned()
}

pub fn blocks(masked: &str) -> Vec<Block> {
    let bytes = masked.as_bytes();
    let mut found = Vec::new();
    let mut open: Vec<(String, usize)> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'\'' => {
                index = skip_string(bytes, index);
                continue;
            }
            b'{' => open.push((word_before(bytes, index), index + 1)),
            b'}' => {
                if let Some((name, start)) = open.pop() {
                    let parents = open.iter().map(|(name, _)| name.clone()).collect();
                    found.push(Block { name, parents, body: start..index });
                }
            }
            _ => {}
        }
        index += 1;
    }
    found
}

fn top_block(masked: &str, name: &str) -> Option<Block> {
    blocks(masked).into_iter().find(|block| block.name == name && block.parents.is_empty())
}

fn statement_ranges(masked: &str, body: &Range<usize>) -> Vec<Range<usize>> {
    let bytes = masked.as_bytes();
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let mut last = body.start;
    let mut index = body.start;
    while index < body.end {
        let byte = bytes[index];
        match byte {
            b'"' | b'\'' => {
                start.get_or_insert(index);
                index = skip_string(bytes, index).min(body.end);
                last = index;
                continue;
            }
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if byte == b'\n' && depth == 0 {
            if let Some(from) = start
                && bytes[last.saturating_sub(1)] != b','
            {
                found.push(from..last);
                start = None;
            }
        } else if !byte.is_ascii_whitespace() {
            start.get_or_insert(index);
            last = index + 1;
        }
        index += 1;
    }
    if let Some(from) = start {
        found.push(from..last);
    }
    found
}

fn string_at(text: &str, at: usize) -> Option<Range<usize>> {
    let quote = text[at..].chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let end = text[at + 1..].find([quote, '\n'])? + at + 1;
    Some(at + 1..end)
}

fn variable_of(version: &str) -> Option<String> {
    let name = version.strip_prefix('$')?;
    let name = name.strip_prefix('{').and_then(|name| name.strip_suffix('}')).unwrap_or(name);
    let name = name.rsplit('.').next()?;
    name.chars().all(|c| c.is_alphanumeric() || c == '_').then(|| name.to_owned())
}

fn value_of(masked: &str, range: &Range<usize>, key: &str) -> Option<Range<usize>> {
    let bytes = masked.as_bytes();
    let part = &masked[range.clone()];
    let mut from = 0;
    while let Some(found) = part[from..].find(key) {
        let start = from + found;
        from = start + key.len();
        let boundary = start == 0 || !part.as_bytes()[start - 1].is_ascii_alphanumeric();
        let rest = part[from..].trim_start();
        let after = from + (part.len() - from - rest.len());
        if boundary && (rest.starts_with(':') || rest.starts_with('=')) {
            let value = part[after + 1..].trim_start();
            let at = range.start + part.len() - value.len();
            if matches!(bytes.get(at), Some(b'"' | b'\'')) {
                return string_at(masked, at);
            }
        }
    }
    None
}

fn parse_statement(masked: &str, range: Range<usize>) -> Option<Statement> {
    let line = &masked[range.clone()];
    let name_end = line.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '"' || c == '\''))?;
    let configuration = line[..name_end].trim_matches(['"', '\'']).to_owned();
    let mut at = range.start + name_end;
    let mut platform = false;
    loop {
        let rest = &masked[at..range.end];
        let trimmed = rest.trim_start_matches([' ', '\t', '(', '\n', '\r']);
        at = range.end - trimmed.len();
        let Some(wrapper) = WRAPPERS.iter().find(|wrapper| {
            trimmed.strip_prefix(**wrapper).is_some_and(|after| after.trim_start().starts_with(['(', ' ']))
        }) else {
            break;
        };
        platform |= *wrapper != "testFixtures";
        at += wrapper.len();
    }
    let rest = &masked[at..range.end];
    if rest.starts_with(['"', '\'']) {
        let inside = string_at(masked, at)?;
        let whole = &masked[inside.clone()];
        let whole = whole.split('@').next().unwrap_or(whole);
        let mut parts = whole.splitn(4, ':');
        let group = parts.next()?.to_owned();
        let artifact = parts.next()?.to_owned();
        let version = parts.next().filter(|version| !version.is_empty());
        let version_range = version.map(|version| {
            let start = inside.start + group.len() + 1 + artifact.len() + 1;
            start..start + version.len()
        });
        let variable = version.and_then(variable_of);
        return Some(Statement {
            configuration,
            group,
            artifact,
            version: version.map(str::to_owned),
            version_range,
            variable,
            platform,
            range,
            ..Statement::default()
        });
    }
    if let Some(after) = rest.strip_prefix("libs.") {
        let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(after.len());
        let accessor = &after[..end];
        if accessor.starts_with("bundles.") || accessor.starts_with("plugins.") {
            return None;
        }
        return Some(Statement {
            configuration,
            accessor: Some(accessor.to_owned()),
            platform,
            range,
            ..Statement::default()
        });
    }
    if rest.starts_with("group") {
        let value = |key: &str| value_of(masked, &range, key);
        let group = masked[value("group")?].to_owned();
        let artifact = masked[value("name")?].to_owned();
        let version_range = value("version");
        let version = version_range.clone().map(|range| masked[range].to_owned());
        let variable = version.as_deref().and_then(variable_of);
        return Some(Statement {
            configuration,
            group,
            artifact,
            version,
            version_range,
            variable,
            platform,
            range,
            ..Statement::default()
        });
    }
    None
}

pub fn statements(text: &str) -> Vec<Statement> {
    let masked = masked(text);
    let Some(block) = top_block(&masked, "dependencies") else { return Vec::new() };
    statement_ranges(&masked, &block.body)
        .into_iter()
        .filter_map(|range| parse_statement(&masked, range))
        .filter(|statement| statement.accessor.is_some() || !statement.group.contains('$'))
        .collect()
}

pub fn variable_in(text: &str, name: &str) -> Option<(String, Range<usize>)> {
    let masked = masked(text);
    let bytes = masked.as_bytes();
    let mut from = 0;
    while let Some(found) = masked[from..].find(name) {
        let start = from + found;
        from = start + name.len();
        let boundary = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let rest = masked[from..].trim_start_matches(['"', '\'', ']']).trim_start();
        let after_name = masked.len() - rest.len();
        if !boundary {
            continue;
        }
        let value = match rest.strip_prefix('=') {
            Some(after) if !after.starts_with('=') => after,
            _ => match rest.strip_prefix(',') {
                Some(after) => after,
                None => continue,
            },
        };
        let value = value.trim_start();
        let at = masked.len() - value.len();
        debug_assert!(at >= after_name);
        if value.starts_with(['"', '\''])
            && let Some(range) = string_at(&masked, at)
        {
            return Some((masked[range.clone()].to_owned(), range));
        }
    }
    None
}

pub fn repository_urls(text: &str) -> Vec<(String, String)> {
    let masked = masked(text);
    let mut found = Vec::new();
    for block in blocks(&masked) {
        if block.name != "repositories"
            || block.parents.iter().any(|parent| NOT_RESOLVED_FROM.contains(&parent.as_str()))
        {
            continue;
        }
        let body = &masked[block.body.clone()];
        for (word, id, url) in [
            (CENTRAL_WORD, "central", super::super::dependencies::sources::CENTRAL),
            (GOOGLE_WORD, "google", GOOGLE),
            (PORTAL_WORD, "gradlePluginPortal", PORTAL),
        ] {
            if body.contains(word) {
                found.push((id.to_owned(), url.to_owned()));
            }
        }
        let bytes = body.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if matches!(bytes[index], b'"' | b'\'') {
                let next = skip_string(bytes, index);
                let text = body[index + 1..next.saturating_sub(1).max(index + 1)].to_owned();
                if text.starts_with("http://") || text.starts_with("https://") {
                    found.push((text.clone(), text));
                }
                index = next;
            } else {
                index += 1;
            }
        }
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    pub kotlin: bool,
    pub quote: char,
    pub parens: bool,
    pub indent: String,
}

fn indent_of(text: &str, at: usize) -> String {
    let start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    text[start..at].to_owned()
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

pub fn style_of(text: &str, kotlin: bool) -> Style {
    let masked = masked(text);
    let found =
        top_block(&masked, "dependencies").map(|block| statement_ranges(&masked, &block.body)).unwrap_or_default();
    let literal = found.iter().find_map(|range| masked[range.clone()].find(['"', '\'']).map(|at| range.start + at));
    let sample = found.first();
    Style {
        kotlin,
        quote: if kotlin { '"' } else { literal.map_or('\'', |at| masked[at..].chars().next().unwrap_or('\'')) },
        parens: kotlin
            || sample.is_some_and(|range| {
                let line = &masked[range.clone()];
                let name = line.find(|c: char| !c.is_alphanumeric()).unwrap_or(line.len());
                line[name..].starts_with('(')
            }),
        indent: sample.map_or_else(|| "    ".to_owned(), |range| indent_of(text, range.start)),
    }
}

pub fn coordinates_argument(group: &str, artifact: &str, version: &str, style: &Style) -> String {
    let quote = style.quote;
    if version.is_empty() {
        format!("{quote}{group}:{artifact}{quote}")
    } else {
        format!("{quote}{group}:{artifact}:{version}{quote}")
    }
}

pub fn line_of(configuration: &str, argument: &str, style: &Style) -> String {
    if style.parens { format!("{configuration}({argument})") } else { format!("{configuration} {argument}") }
}

pub fn add_line(text: &str, configuration: &str, line: &str) -> String {
    let masked = masked(text);
    let style = style_of(text, false);
    let separator = newline(text);
    let Some(block) = top_block(&masked, "dependencies") else {
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(separator);
        }
        if !out.is_empty() {
            out.push_str(separator);
        }
        out.push_str(&format!("dependencies {{{separator}    {line}{separator}}}{separator}"));
        return out;
    };
    let ranges = statement_ranges(&masked, &block.body);
    let parsed: Vec<Statement> = ranges.iter().filter_map(|range| parse_statement(&masked, range.clone())).collect();
    let same = parsed.iter().rfind(|statement| statement.configuration == configuration);
    let after = same.map(|statement| statement.range.end).or_else(|| ranges.last().map(|range| range.end));
    let mut out = String::with_capacity(text.len() + line.len() + 8);
    match after {
        Some(at) => {
            let end = text[at..].find('\n').map_or(text.len(), |found| at + found);
            let end = if text[..end].ends_with('\r') { end - 1 } else { end };
            out.push_str(&text[..end]);
            out.push_str(&format!("{separator}{}{line}", style.indent));
            out.push_str(&text[end..]);
        }
        None => {
            let open = block.body.start;
            let close = block.body.end;
            out.push_str(&text[..open]);
            out.push_str(&format!("{separator}{}{line}{separator}", style.indent));
            out.push_str(&text[close..]);
        }
    }
    out
}

fn inserted_in(text: &str, block: &Block, lines: &[String]) -> String {
    let separator = newline(text);
    let body = &text[block.body.clone()];
    let trimmed = body.trim_end();
    let at = block.body.start + trimmed.len();
    let added: String = lines.iter().map(|line| format!("{separator}{line}")).collect();
    let mut out = String::with_capacity(text.len() + added.len());
    out.push_str(&text[..at]);
    out.push_str(&added);
    if body.trim().is_empty() {
        out.push_str(separator);
        out.push_str(&indent_of(text, block.body.end));
        out.push_str(&text[block.body.end..]);
    } else {
        out.push_str(&text[at..]);
    }
    out
}

fn appended_block(text: &str, block: &str) -> String {
    let separator = newline(text);
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(separator);
    }
    if !out.is_empty() {
        out.push_str(separator);
    }
    out.push_str(&block.replace('\n', separator));
    out
}

pub fn add_bom(text: &str, group: &str, artifact: &str, version: &str, kotlin: bool) -> String {
    let name = format!("{group}:{artifact}");
    if text.contains(&name) {
        return text.to_owned();
    }
    let style = style_of(text, kotlin);
    let quoted = format!("{0}{name}:{version}{0}", style.quote);
    if !text.contains(DEPENDENCY_MANAGEMENT_PLUGIN) {
        let line = line_of(CONFIGURATIONS[0], &format!("platform({quoted})"), &style);
        return add_line(text, CONFIGURATIONS[0], &line);
    }
    let import = if kotlin { format!("mavenBom({quoted})") } else { format!("mavenBom {quoted}") };
    let masked = masked(text);
    let found = blocks(&masked);
    let imports = found.iter().find(|block| block.name == "imports" && block.parents == ["dependencyManagement"]);
    if let Some(imports) = imports {
        let indent = format!("{}    ", indent_of(text, imports.body.end));
        return inserted_in(text, imports, &[format!("{indent}{import}")]);
    }
    if let Some(management) = top_block(&masked, "dependencyManagement") {
        let lines = ["    imports {".to_owned(), format!("        {import}"), "    }".to_owned()];
        return inserted_in(text, &management, &lines);
    }
    appended_block(text, &format!("dependencyManagement {{\n    imports {{\n        {import}\n    }}\n}}\n"))
}

pub fn add_repository(text: &str, url: &str, kotlin: bool) -> String {
    if repository_urls(text).iter().any(|(_, known)| known.trim_end_matches('/') == url.trim_end_matches('/')) {
        return text.to_owned();
    }
    let style = style_of(text, kotlin);
    let quote = style.quote;
    let line = if kotlin {
        format!("maven {{ url = uri({quote}{url}{quote}) }}")
    } else {
        format!("maven {{ url = {quote}{url}{quote} }}")
    };
    let masked = masked(text);
    match top_block(&masked, "repositories") {
        Some(block) => inserted_in(text, &block, &[format!("{}{line}", style.indent)]),
        None => match top_block(&masked, "dependencies") {
            Some(block) => {
                let at = text[..block.body.start].rfind('\n').map_or(0, |found| found + 1);
                let separator = newline(text);
                let new = format!("repositories {{{separator}    {line}{separator}}}{separator}{separator}");
                let mut out = text.to_owned();
                out.insert_str(at, &new);
                out
            }
            None => appended_block(text, &format!("repositories {{\n    {line}\n}}\n")),
        },
    }
}

pub fn remove_statement(text: &str, statement: &Statement) -> String {
    let start = text[..statement.range.start].rfind('\n').map_or(0, |found| found + 1);
    let end = text[statement.range.end..].find('\n').map_or(text.len(), |found| statement.range.end + found + 1);
    let whole_line = text[start..statement.range.start].trim().is_empty();
    let mut out = text.to_owned();
    out.replace_range(if whole_line { start..end } else { statement.range.clone() }, "");
    out
}

pub fn not_declared(group: &str, artifact: &str) -> String {
    t!("workbench-deps-not-declared", dependency = format!("{group}:{artifact}"))
}

pub fn caret_in_block(text: &str, caret: usize) -> bool {
    let masked = masked(text);
    top_block(&masked, "dependencies").is_some_and(|block| block.body.contains(&caret))
}

pub fn string_prefix(text: &str, caret: usize) -> Option<usize> {
    let start = text[..caret].rfind('\n').map_or(0, |found| found + 1);
    let line = &text[start..caret];
    let mut open: Option<(usize, char)> = None;
    for (at, c) in line.char_indices() {
        match open {
            Some((_, quote)) if c == quote => open = None,
            None if c == '"' || c == '\'' => open = Some((at, c)),
            _ => {}
        }
    }
    open.map(|(at, _)| start + at + 1)
}
