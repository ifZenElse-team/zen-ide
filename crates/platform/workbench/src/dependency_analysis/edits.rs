use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use roxmltree::{Document, Node};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exclusion {
    NotDeclared,
    Present,
    Edited(String),
}

pub fn valid_name(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn indent_at(text: &str, at: usize) -> &str {
    let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let line = &text[start..at];
    &line[..line.len() - line.trim_start().len()]
}

fn child_text<'a>(node: Node<'a, '_>, name: &str) -> &'a str {
    node.children().find(|child| child.has_tag_name(name)).and_then(|child| child.text()).unwrap_or_default().trim()
}

fn names(node: Node<'_, '_>, group: &str, artifact: &str) -> bool {
    let found = child_text(node, "groupId");
    child_text(node, "artifactId") == artifact && (found == group || found.contains("${"))
}

fn declared<'a, 'i>(document: &'a Document<'i>, group: &str, artifact: &str) -> Option<Node<'a, 'i>> {
    let project = document.root_element();
    let own = project.children().filter(|child| child.has_tag_name("dependencies"));
    let profiles = project
        .children()
        .filter(|child| child.has_tag_name("profiles"))
        .flat_map(|profiles| profiles.children().filter(|child| child.has_tag_name("profile")))
        .flat_map(|profile| profile.children().filter(|child| child.has_tag_name("dependencies")));
    own.chain(profiles)
        .flat_map(|list| list.children().filter(|child| child.has_tag_name("dependency")))
        .find(|dependency| names(*dependency, group, artifact))
}

pub fn pom_declaration(text: &str, group: &str, artifact: &str) -> Option<usize> {
    let document = Document::parse(text).ok()?;
    Some(declared(&document, group, artifact)?.range().start)
}

pub fn pom_parent(file: &Path, text: &str) -> Option<PathBuf> {
    let document = Document::parse(text).ok()?;
    let parent = document.root_element().children().find(|child| child.has_tag_name("parent"))?;
    let relative = parent.children().find(|child| child.has_tag_name("relativePath"));
    let relative = match relative {
        Some(node) => node.text().unwrap_or_default().trim().to_owned(),
        None => "../pom.xml".to_owned(),
    };
    if relative.is_empty() {
        return None;
    }
    let mut path = PathBuf::new();
    for part in file.parent()?.join(relative).components() {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            part => path.push(part),
        }
    }
    Some(if path.is_dir() || path.extension().is_none() { path.join("pom.xml") } else { path })
}

fn exclusion_block(base: &str, unit: &str, newline: &str, group: &str, artifact: &str) -> String {
    let inner = format!("{base}{unit}");
    format!(
        "{base}<exclusion>{newline}{inner}<groupId>{group}</groupId>{newline}{inner}<artifactId>{artifact}</artifactId>{newline}{base}</exclusion>"
    )
}

fn inline_exclusion(group: &str, artifact: &str) -> String {
    format!("<exclusion><groupId>{group}</groupId><artifactId>{artifact}</artifactId></exclusion>")
}

fn spliced(text: &str, range: Range<usize>, new: &str) -> String {
    let mut edited = String::with_capacity(text.len() + new.len());
    edited.push_str(&text[..range.start]);
    edited.push_str(new);
    edited.push_str(&text[range.end..]);
    edited
}

fn last_kept<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children().rfind(|child| child.is_element() || child.is_comment())
}

fn unit_between<'t>(outer: &str, inner: &'t str) -> &'t str {
    match inner.strip_prefix(outer) {
        Some(unit) if !unit.is_empty() => unit,
        _ => "    ",
    }
}

pub fn pom_exclusion(
    text: &str,
    via_group: &str,
    via_artifact: &str,
    group: &str,
    artifact: &str,
) -> Result<Exclusion, String> {
    let document = Document::parse(text).map_err(|error| error.to_string())?;
    let Some(dependency) = declared(&document, via_group, via_artifact) else { return Ok(Exclusion::NotDeclared) };
    let newline = line_ending(text);
    let outer = indent_at(text, dependency.range().start);
    let first = dependency.children().find(Node::is_element).ok_or("an empty <dependency>")?;
    let spread = text[dependency.range().start..first.range().start].contains('\n');
    let inner = if spread { indent_at(text, first.range().start) } else { outer };
    let unit = unit_between(outer, inner);
    if let Some(list) = dependency.children().find(|child| child.has_tag_name("exclusions")) {
        if list.children().filter(|child| child.has_tag_name("exclusion")).any(|node| names(node, group, artifact)) {
            return Ok(Exclusion::Present);
        }
        let Some(last) = last_kept(list) else {
            let base = indent_at(text, list.range().start).to_owned();
            let body = if spread {
                let block = exclusion_block(&format!("{base}{unit}"), unit, newline, group, artifact);
                format!("<exclusions>{newline}{block}{newline}{base}</exclusions>")
            } else {
                format!("<exclusions>{}</exclusions>", inline_exclusion(group, artifact))
            };
            return Ok(Exclusion::Edited(spliced(text, list.range(), &body)));
        };
        let end = last.range().end;
        let on_lines = text[list.range().start..last.range().start].contains('\n');
        let new = if on_lines {
            let base = indent_at(text, last.range().start);
            let unit = unit_between(indent_at(text, list.range().start), base);
            format!("{newline}{}", exclusion_block(base, unit, newline, group, artifact))
        } else {
            inline_exclusion(group, artifact)
        };
        return Ok(Exclusion::Edited(spliced(text, end..end, &new)));
    }
    let last = last_kept(dependency).ok_or("an empty <dependency>")?;
    let end = last.range().end;
    let new = if spread {
        let block = exclusion_block(&format!("{inner}{unit}"), unit, newline, group, artifact);
        format!("{newline}{inner}<exclusions>{newline}{block}{newline}{inner}</exclusions>")
    } else {
        format!("<exclusions>{}</exclusions>", inline_exclusion(group, artifact))
    };
    Ok(Exclusion::Edited(spliced(text, end..end, &new)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Byte {
    Code,
    Comment,
    Text,
}

fn classify(text: &str) -> Vec<Byte> {
    let bytes = text.as_bytes();
    let mut kinds = vec![Byte::Code; bytes.len()];
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        let (kind, end) = if rest.starts_with(b"//") {
            (Byte::Comment, at + rest.iter().position(|byte| *byte == b'\n').unwrap_or(rest.len()))
        } else if rest.starts_with(b"/*") {
            let close = rest.windows(2).position(|pair| pair == b"*/");
            (Byte::Comment, close.map_or(bytes.len(), |close| at + close + 2))
        } else if rest.starts_with(b"\"\"\"") || rest.starts_with(b"'''") {
            let close = rest[3..].windows(3).position(|triple| triple == &rest[..3]);
            (Byte::Text, close.map_or(bytes.len(), |close| at + close + 6))
        } else if matches!(rest[0], b'"' | b'\'') {
            let quote = rest[0];
            let mut end = at + 1;
            while end < bytes.len() && bytes[end] != quote && bytes[end] != b'\n' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            (Byte::Text, (end + 1).min(bytes.len()))
        } else {
            at += 1;
            continue;
        };
        kinds[at..end].fill(kind);
        at = end;
    }
    kinds
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Statement {
    start: usize,
    arguments: Range<usize>,
    closure: Option<Range<usize>>,
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn matching_close(text: &str, kinds: &[Byte], open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for at in open..bytes.len() {
        if kinds[at] != Byte::Code {
            continue;
        }
        match bytes[at] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

fn blocks(text: &str, kinds: &[Byte]) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let name = "dependencies";
    let mut found = Vec::new();
    for (at, _) in text.match_indices(name) {
        let end = at + name.len();
        let bounded = kinds[at] == Byte::Code
            && (at == 0 || !is_word(bytes[at - 1]))
            && !is_word(*bytes.get(end).unwrap_or(&b' '));
        if !bounded {
            continue;
        }
        let Some(open) = (end..bytes.len()).find(|index| !bytes[*index].is_ascii_whitespace()) else { continue };
        if bytes[open] != b'{' || kinds[open] != Byte::Code {
            continue;
        }
        if let Some(close) = matching_close(text, kinds, open) {
            found.push(open + 1..close);
        }
    }
    found
}

fn statement_at(text: &str, kinds: &[Byte], start: usize, end: usize) -> Option<(Statement, usize)> {
    let bytes = text.as_bytes();
    let mut word = start;
    if kinds[start] == Byte::Text {
        while word < end && kinds[word] == Byte::Text {
            word += 1;
        }
    } else {
        while word < end && kinds[word] == Byte::Code && (is_word(bytes[word]) || bytes[word] == b'.') {
            word += 1;
        }
    }
    if word == start {
        return None;
    }
    let mut at = word;
    let mut depth = 0usize;
    let mut closure = None;
    let mut last = word;
    let mut stop = end;
    while at < end {
        if kinds[at] == Byte::Comment {
            at += 1;
            continue;
        }
        let byte = bytes[at];
        if kinds[at] == Byte::Code {
            match byte {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b'{' => {
                    let close = matching_close(text, kinds, at)?;
                    if depth == 0 {
                        closure = Some(at..close + 1);
                        stop = close + 1;
                        break;
                    }
                    at = close + 1;
                    last = at;
                    continue;
                }
                b'\n' | b';' if depth == 0 && text[..at].trim_end().as_bytes().last() != Some(&b',') => {
                    stop = at;
                    break;
                }
                _ => {}
            }
        }
        if !byte.is_ascii_whitespace() {
            last = at + 1;
        }
        at += 1;
    }
    Some((Statement { start, arguments: word..last, closure }, stop))
}

fn statements(text: &str, kinds: &[Byte]) -> Vec<Statement> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for block in blocks(text, kinds) {
        let mut at = block.start;
        while at < block.end {
            if bytes[at].is_ascii_whitespace() || bytes[at] == b';' || kinds[at] == Byte::Comment {
                at += 1;
                continue;
            }
            match statement_at(text, kinds, at, block.end) {
                Some((statement, stop)) => {
                    found.push(statement);
                    at = stop.max(at + 1);
                }
                None => at += 1,
            }
        }
    }
    found
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GradleDependency {
    pub group: String,
    pub artifact: String,
    pub project: Option<String>,
    pub accessors: Vec<String>,
}

fn literals(text: &str, kinds: &[Byte], range: Range<usize>) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = range.start;
    while at < range.end {
        if kinds[at] != Byte::Text {
            at += 1;
            continue;
        }
        let start = at;
        while at < range.end && kinds[at] == Byte::Text {
            at += 1;
        }
        found.push(text[start..at].trim_matches(['"', '\'']).to_owned());
    }
    found
}

fn accessor_used(arguments: &str, accessor: &str) -> bool {
    arguments.match_indices(accessor).any(|(at, _)| {
        let bytes = arguments.as_bytes();
        let before = at == 0 || !(is_word(bytes[at - 1]) || bytes[at - 1] == b'.');
        let after = bytes.get(at + accessor.len()).copied().unwrap_or(b' ');
        let dotted = arguments[at + accessor.len()..].starts_with(".get()");
        before && (!is_word(after) && after != b'.' || dotted)
    })
}

fn statement_names(text: &str, kinds: &[Byte], statement: &Statement, wanted: &GradleDependency) -> bool {
    let keyword = text[statement.start..statement.arguments.start].trim_matches(['"', '\'']);
    if matches!(keyword, "dependency" | "exclude" | "classpath" | "constraints" | "components" | "modules") {
        return false;
    }
    let arguments = &text[statement.arguments.clone()];
    let quoted = literals(text, kinds, statement.arguments.clone());
    if let Some(project) = &wanted.project {
        return arguments.contains("project") && quoted.iter().any(|literal| literal == project);
    }
    let module = format!("{}:{}", wanted.group, wanted.artifact);
    let in_one = quoted.iter().any(|literal| *literal == module || literal.starts_with(&format!("{module}:")));
    let in_a_map = quoted.contains(&wanted.group) && quoted.contains(&wanted.artifact) && arguments.contains("name");
    in_one || in_a_map || wanted.accessors.iter().any(|accessor| accessor_used(arguments, accessor))
}

pub fn gradle_declaration(text: &str, wanted: &GradleDependency) -> Option<usize> {
    let kinds = classify(text);
    statements(text, &kinds)
        .iter()
        .find(|statement| statement_names(text, &kinds, statement, wanted))
        .map(|statement| statement.start)
}

fn exclude_line(kotlin: bool, quote: char, group: &str, artifact: &str) -> String {
    if kotlin {
        format!("exclude(group = \"{group}\", module = \"{artifact}\")")
    } else {
        format!("exclude group: {quote}{group}{quote}, module: {quote}{artifact}{quote}")
    }
}

fn parenthesized(text: &str, kinds: &[Byte], arguments: &Range<usize>) -> bool {
    let open = arguments.start + text[arguments.clone()].len() - text[arguments.clone()].trim_start().len();
    text.as_bytes().get(open) == Some(&b'(')
        && matching_close(text, kinds, open).is_some_and(|close| close + 1 == arguments.end)
}

fn excluded(closure: &str, group: &str, artifact: &str) -> bool {
    closure.lines().any(|line| line.contains("exclude") && line.contains(group) && line.contains(artifact))
}

fn edit_statement(
    text: &str,
    kinds: &[Byte],
    statement: &Statement,
    kotlin: bool,
    group: &str,
    artifact: &str,
) -> Option<(Range<usize>, String)> {
    let newline = line_ending(text);
    let indent = indent_at(text, statement.start);
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    let arguments = &statement.arguments;
    let quote = if text[arguments.clone()].contains('"') { '"' } else { '\'' };
    let line = exclude_line(kotlin, quote, group, artifact);
    if let Some(closure) = &statement.closure {
        if excluded(&text[closure.clone()], group, artifact) {
            return None;
        }
        let close = closure.end - 1;
        let start = text[..close].rfind('\n').map_or(0, |newline| newline + 1);
        if start > closure.start && text[start..close].trim().is_empty() {
            let at = start..start;
            return Some((at, format!("{indent}{unit}{line}{newline}")));
        }
        let body = text[closure.start + 1..close].trim_end();
        let end = closure.start + 1 + body.len();
        return Some(if body.trim().is_empty() {
            (closure.start + 1..close, format!(" {line} "))
        } else {
            (end..end, format!("; {line}"))
        });
    }
    let block = format!(" {{{newline}{indent}{unit}{line}{newline}{indent}}}");
    if parenthesized(text, kinds, arguments) {
        return Some((arguments.end..arguments.end, block));
    }
    let shown = text[arguments.clone()].trim_start();
    Some((arguments.clone(), format!("({shown}){block}")))
}

pub fn gradle_exclusion(text: &str, kotlin: bool, wanted: &GradleDependency, group: &str, artifact: &str) -> Exclusion {
    let kinds = classify(text);
    let found: Vec<Statement> = statements(text, &kinds)
        .into_iter()
        .filter(|statement| statement_names(text, &kinds, statement, wanted))
        .collect();
    if found.is_empty() {
        return Exclusion::NotDeclared;
    }
    let mut edits: Vec<(Range<usize>, String)> =
        found.iter().filter_map(|statement| edit_statement(text, &kinds, statement, kotlin, group, artifact)).collect();
    if edits.is_empty() {
        return Exclusion::Present;
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut edited = text.to_owned();
    for (range, new) in edits {
        edited.replace_range(range, &new);
    }
    Exclusion::Edited(edited)
}

pub fn catalog_accessors(catalog: &str, group: &str, artifact: &str) -> Vec<String> {
    let module = format!("{group}:{artifact}");
    let mut section = false;
    let mut found = Vec::new();
    for line in catalog.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line == "[libraries]";
            continue;
        }
        let Some((alias, value)) = line.split_once('=').filter(|_| section) else { continue };
        let quoted: Vec<&str> = value.split(['"', '\'']).skip(1).step_by(2).collect();
        let single = quoted.iter().any(|text| *text == module || text.starts_with(&format!("{module}:")));
        let apart = quoted.contains(&group) && quoted.contains(&artifact);
        if single || apart {
            found.push(format!("libs.{}", alias.trim().trim_matches('"').replace(['-', '_'], ".")));
        }
    }
    found
}

pub fn catalog_declaration(catalog: &str, group: &str, artifact: &str) -> Option<usize> {
    let module = format!("{group}:{artifact}");
    let found = catalog.find(&module);
    found.or_else(|| {
        let mut at = 0;
        catalog.split_inclusive('\n').find_map(|line| {
            let here = at;
            at += line.len();
            (line.contains(group) && line.contains(artifact) && line.contains('=')).then_some(here)
        })
    })
}
