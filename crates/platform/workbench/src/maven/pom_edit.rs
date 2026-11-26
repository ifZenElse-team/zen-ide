use std::collections::BTreeMap;
use std::ops::Range;

use roxmltree::{Document, Node};
use zen_i18n::t;

use super::pom::{child, children, coordinates_and_properties, interpolate, text_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewDependency<'a> {
    pub group: &'a str,
    pub artifact: &'a str,
    pub version: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewBom<'a> {
    pub group: &'a str,
    pub artifact: &'a str,
    pub version: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewRepository<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub url: &'a str,
    pub snapshots: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionEdit {
    Changed(String),
    InProperty(String),
    Absent,
}

fn parsed(text: &str) -> Result<Document<'_>, String> {
    Document::parse(text).map_err(|error| error.to_string())
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |found| found + 1)
}

fn indent_at(text: &str, at: usize) -> &str {
    let start = line_start(text, at);
    let line = &text[start..at];
    &line[..line.len() - line.trim_start().len()]
}

fn indent_unit(text: &str, root: Node<'_, '_>) -> String {
    root.children()
        .find(Node::is_element)
        .map(|first| indent_at(text, first.range().start))
        .filter(|indent| !indent.is_empty())
        .map_or_else(|| "    ".to_owned(), str::to_owned)
}

fn matches(node: Node<'_, '_>, group: &str, artifact: &str, properties: &BTreeMap<String, String>) -> bool {
    let value = |name: &str| interpolate(&text_of(node, name), properties);
    value("groupId") == group && value("artifactId") == artifact
}

fn scope_of(node: Node<'_, '_>) -> String {
    match text_of(node, "scope") {
        scope if scope.is_empty() => "compile".to_owned(),
        scope => scope,
    }
}

fn dependency_xml(new: &NewDependency<'_>, indent: &str, inner: &str, line: &str) -> String {
    let mut out = format!("{indent}<dependency>{line}");
    let mut element = |name: &str, value: &str| {
        out.push_str(&format!("{inner}<{name}>{value}</{name}>{line}"));
    };
    element("groupId", new.group);
    element("artifactId", new.artifact);
    if let Some(version) = new.version {
        element("version", version);
    }
    if let Some(scope) = new.scope {
        element("scope", scope);
    }
    if new.optional {
        element("optional", "true");
    }
    out.push_str(&format!("{indent}</dependency>"));
    out
}

fn inner_indent(text: &str, sample: Option<Node<'_, '_>>, indent: &str, unit: &str) -> String {
    sample
        .and_then(|node| node.children().find(Node::is_element))
        .map(|first| indent_at(text, first.range().start).to_owned())
        .filter(|inner| !inner.is_empty())
        .unwrap_or_else(|| format!("{indent}{unit}"))
}

fn splice(text: &str, range: Range<usize>, insert: &str) -> String {
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..range.start]);
    out.push_str(insert);
    out.push_str(&text[range.end..]);
    out
}

pub fn add(text: &str, new: &NewDependency<'_>) -> Result<String, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let (_, _, _, properties) = coordinates_and_properties(root);
    let line = newline(text);
    let unit = indent_unit(text, root);
    let Some(list) = child(root, "dependencies") else {
        return Ok(with_new_list(text, root, new, &unit, line));
    };
    let scope = new.scope.unwrap_or("compile");
    let declared: Vec<Node<'_, '_>> = children(list, "dependency").collect();
    if declared
        .iter()
        .copied()
        .any(|node| matches(node, new.group, new.artifact, &properties) && scope_of(node) == scope)
    {
        return Err(t!("workbench-deps-already", dependency = format!("{}:{}", new.group, new.artifact)));
    }
    Ok(appended(text, list, "dependency", &unit, line, |indent, inner| dependency_xml(new, indent, inner, line)))
}

fn appended(
    text: &str,
    list: Node<'_, '_>,
    element: &str,
    unit: &str,
    line: &str,
    xml: impl Fn(&str, &str) -> String,
) -> String {
    let name = list.tag_name().name();
    match children(list, element).last() {
        Some(last) => {
            let indent = indent_at(text, last.range().start);
            let inner = inner_indent(text, Some(last), indent, unit);
            let at = last.range().end;
            splice(text, at..at, &format!("{line}{}", xml(indent, &inner)))
        }
        None => {
            let outer = indent_at(text, list.range().start);
            let indent = format!("{outer}{unit}");
            let xml = xml(&indent, &format!("{indent}{unit}"));
            let start = list.range().start;
            let open = text[start..list.range().end].find('>').map_or(start, |at| start + at);
            let closed = text[..list.range().end].ends_with("/>");
            let range = if closed { start..list.range().end } else { open + 1..list.range().end };
            let replaced = if closed {
                format!("<{name}>{line}{xml}{line}{outer}</{name}>")
            } else {
                format!("{line}{xml}{line}{outer}</{name}>")
            };
            splice(text, range, &replaced)
        }
    }
}

fn with_block(text: &str, root: Node<'_, '_>, block: &str, later: &[&str], line: &str) -> String {
    let before = root
        .children()
        .filter(Node::is_element)
        .find(|node| later.contains(&node.tag_name().name()))
        .map(|node| line_start(text, node.range().start));
    match before {
        Some(at) => splice(text, at..at, &format!("{block}{line}")),
        None => {
            let close = root.range().end - "</project>".len();
            let at = line_start(text, close);
            let gap = if text[..at].trim_end_matches(['\r', '\n']).len() + 2 * line.len() <= at { "" } else { line };
            splice(text, at..at, &format!("{gap}{block}"))
        }
    }
}

fn bom_xml(bom: &NewBom<'_>, indent: &str, inner: &str, line: &str) -> String {
    let lines = [
        ("groupId", bom.group),
        ("artifactId", bom.artifact),
        ("version", bom.version),
        ("type", "pom"),
        ("scope", "import"),
    ];
    let mut out = format!("{indent}<dependency>{line}");
    for (name, value) in lines {
        out.push_str(&format!("{inner}<{name}>{value}</{name}>{line}"));
    }
    out.push_str(&format!("{indent}</dependency>"));
    out
}

pub fn add_bom(text: &str, bom: &NewBom<'_>) -> Result<String, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let (_, _, _, properties) = coordinates_and_properties(root);
    let (line, unit) = (newline(text), indent_unit(text, root));
    let management = child(root, "dependencyManagement");
    if let Some(list) = management.and_then(|management| child(management, "dependencies")) {
        if children(list, "dependency").any(|node| matches(node, bom.group, bom.artifact, &properties)) {
            return Ok(text.to_owned());
        }
        return Ok(appended(text, list, "dependency", &unit, line, |indent, inner| bom_xml(bom, indent, inner, line)));
    }
    if let Some(management) = management {
        let closing = management.range().end - "</dependencyManagement>".len();
        let start = line_start(text, closing);
        let (inner, deep) = (format!("{unit}{unit}"), format!("{unit}{unit}{unit}"));
        let xml = bom_xml(bom, &deep, &format!("{deep}{unit}"), line);
        return Ok(splice(
            text,
            start..start,
            &format!("{inner}<dependencies>{line}{xml}{line}{inner}</dependencies>{line}"),
        ));
    }
    let (inner, deep) = (format!("{unit}{unit}"), format!("{unit}{unit}{unit}"));
    let xml = bom_xml(bom, &deep, &format!("{deep}{unit}"), line);
    let block = format!(
        "{unit}<dependencyManagement>{line}{inner}<dependencies>{line}{xml}{line}{inner}</dependencies>{line}{unit}</dependencyManagement>{line}"
    );
    Ok(with_block(
        text,
        root,
        &block,
        &["dependencies", "repositories", "pluginRepositories", "build", "reporting", "profiles"],
        line,
    ))
}

fn repository_xml(repository: &NewRepository<'_>, indent: &str, inner: &str, line: &str) -> String {
    let unit = &inner[indent.len().min(inner.len())..];
    let mut out = format!("{indent}<repository>{line}");
    for (name, value) in [("id", repository.id), ("name", repository.name), ("url", repository.url)] {
        if !value.is_empty() {
            out.push_str(&format!("{inner}<{name}>{value}</{name}>{line}"));
        }
    }
    if !repository.snapshots {
        out.push_str(&format!(
            "{inner}<snapshots>{line}{inner}{unit}<enabled>false</enabled>{line}{inner}</snapshots>{line}"
        ));
    }
    out.push_str(&format!("{indent}</repository>"));
    out
}

pub fn add_repository(text: &str, repository: &NewRepository<'_>) -> Result<String, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let (line, unit) = (newline(text), indent_unit(text, root));
    let xml = |indent: &str, inner: &str| repository_xml(repository, indent, inner, line);
    if let Some(list) = child(root, "repositories") {
        let same = |node: Node<'_, '_>| text_of(node, "url") == repository.url || text_of(node, "id") == repository.id;
        if children(list, "repository").any(same) {
            return Ok(text.to_owned());
        }
        return Ok(appended(text, list, "repository", &unit, line, xml));
    }
    let inner = format!("{unit}{unit}");
    let body = xml(&inner, &format!("{inner}{unit}"));
    let block = format!("{unit}<repositories>{line}{body}{line}{unit}</repositories>{line}");
    Ok(with_block(text, root, &block, &["pluginRepositories", "build", "reporting", "profiles"], line))
}

fn with_new_list(text: &str, root: Node<'_, '_>, new: &NewDependency<'_>, unit: &str, line: &str) -> String {
    let inner = format!("{unit}{unit}");
    let xml = dependency_xml(new, &inner, &format!("{inner}{unit}"), line);
    let block = format!("{unit}<dependencies>{line}{xml}{line}{unit}</dependencies>{line}");
    with_block(text, root, &block, &["repositories", "pluginRepositories", "build", "reporting", "profiles"], line)
}

fn found<'a, 'i>(
    list: Option<Node<'a, 'i>>,
    group: &str,
    artifact: &str,
    properties: &BTreeMap<String, String>,
) -> Vec<Node<'a, 'i>> {
    list.into_iter()
        .flat_map(|list| children(list, "dependency"))
        .filter(|node| matches(*node, group, artifact, properties))
        .collect()
}

pub fn remove(text: &str, group: &str, artifact: &str, scope: Option<&str>) -> Result<String, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let (_, _, _, properties) = coordinates_and_properties(root);
    let node = found(child(root, "dependencies"), group, artifact, &properties)
        .into_iter()
        .find(|node| scope.is_none_or(|scope| scope_of(*node) == scope))
        .ok_or_else(|| t!("workbench-deps-not-declared", dependency = format!("{group}:{artifact}")))?;
    let range = node.range();
    let start = line_start(text, range.start);
    if !text[start..range.start].trim().is_empty() {
        return Ok(splice(text, range, ""));
    }
    let rest = &text[range.end..];
    let trailing = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let end = range.end + trailing;
    let end = match &text[end..] {
        after if after.starts_with("\r\n") => end + 2,
        after if after.starts_with('\n') => end + 1,
        _ => end,
    };
    Ok(splice(text, start..end, ""))
}

fn element_text_range(node: Node<'_, '_>) -> Option<Range<usize>> {
    node.first_child().filter(Node::is_text).map(|text| {
        let range = text.range();
        let raw = text.text().unwrap_or_default();
        let lead = raw.len() - raw.trim_start().len();
        range.start + lead..range.start + lead + raw.trim().len()
    })
}

pub fn set_version(text: &str, group: &str, artifact: &str, version: &str) -> Result<VersionEdit, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let (_, _, _, properties) = coordinates_and_properties(root);
    let managed = child(root, "dependencyManagement").and_then(|management| child(management, "dependencies"));
    let mut nodes = found(child(root, "dependencies"), group, artifact, &properties);
    nodes.extend(found(managed, group, artifact, &properties));
    for node in nodes {
        let Some(place) = child(node, "version") else { continue };
        let Some(range) = element_text_range(place) else { continue };
        let written = text[range.clone()].to_owned();
        if let Some(name) = written.strip_prefix("${").and_then(|rest| rest.strip_suffix('}')) {
            return Ok(match set_property(text, name, version)? {
                Some(changed) => VersionEdit::Changed(changed),
                None => VersionEdit::InProperty(name.to_owned()),
            });
        }
        return Ok(VersionEdit::Changed(splice(text, range, version)));
    }
    Ok(VersionEdit::Absent)
}

pub fn set_property(text: &str, name: &str, value: &str) -> Result<Option<String>, String> {
    let document = parsed(text)?;
    let root = document.root_element();
    let property = child(root, "properties").and_then(|list| child(list, name));
    Ok(property.and_then(element_text_range).map(|range| splice(text, range, value)))
}

pub fn version_range(node: Node<'_, '_>) -> Option<Range<usize>> {
    child(node, "version").and_then(element_text_range)
}
