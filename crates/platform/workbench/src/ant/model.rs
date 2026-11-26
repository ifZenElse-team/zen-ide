use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

/// An Ant build file read from its XML.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildFile {
    pub path: PathBuf,
    /// The project's `name`, else the file's name: what the tool window shows.
    pub name: String,
    /// The project's `default` target; empty when it has none.
    pub default: String,
    pub description: String,
    /// The targets and extension points, in the file's order.
    pub targets: Vec<Target>,
    /// Why the file did not read, when it did not; the rest is then empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A target of a build file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub name: String,
    /// Its `description`; the tool window's filter keeps only targets that have one.
    pub description: String,
    /// The targets it depends on, in order.
    pub depends: Vec<String>,
    /// Whether it is an `extension-point`, not a `target`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub extension_point: bool,
    /// The character offset of its name in the file.
    pub offset: usize,
    /// The line of its name, from 1.
    pub line: usize,
}

fn chars(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

pub fn parse(path: &Path, text: &str) -> BuildFile {
    let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut file = BuildFile { path: path.to_owned(), name: file_name, ..BuildFile::default() };
    let document = match roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() },
    ) {
        Ok(document) => document,
        Err(error) => {
            file.error = Some(error.to_string());
            return file;
        }
    };
    let root = document.root_element();
    if root.tag_name().name() != "project" {
        file.error = Some(format!("<{}> is not an Ant <project>", root.tag_name().name()));
        return file;
    }
    if let Some(name) = root.attribute("name").filter(|name| !name.trim().is_empty()) {
        file.name = name.trim().to_owned();
    }
    file.default = root.attribute("default").unwrap_or_default().trim().to_owned();
    file.description = root
        .children()
        .find(|node| node.has_tag_name("description"))
        .and_then(|node| node.text())
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    for node in root.children().filter(roxmltree::Node::is_element) {
        let extension_point = node.has_tag_name("extension-point");
        if !extension_point && !node.has_tag_name("target") {
            continue;
        }
        let Some(name) = node.attributes().find(|attribute| attribute.name() == "name") else { continue };
        let start = name.range_value().start;
        file.targets.push(Target {
            name: name.value().trim().to_owned(),
            description: node.attribute("description").unwrap_or_default().trim().to_owned(),
            depends: node
                .attribute("depends")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            extension_point,
            offset: chars(text, start),
            line: document.text_pos_at(start).row as usize,
        });
    }
    file
}

pub fn read(path: &Path) -> BuildFile {
    match zen_vfs::read_text(path) {
        Ok(text) => parse(path, &text),
        Err(error) => BuildFile {
            path: path.to_owned(),
            name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            error: Some(error.to_string()),
            ..BuildFile::default()
        },
    }
}

pub fn is_build_file(text: &str) -> bool {
    let body = text.trim_start_matches('\u{feff}');
    let mut rest = body;
    loop {
        rest = rest.trim_start();
        let skipped = if rest.starts_with("<?") {
            rest.find("?>").map(|end| end + 2)
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|end| end + 3)
        } else if rest.starts_with("<!") {
            rest.find('>').map(|end| end + 1)
        } else {
            break;
        };
        match skipped {
            Some(end) => rest = &rest[end..],
            None => return false,
        }
    }
    let Some(after) = rest.strip_prefix("<project") else { return false };
    let tag = &after[..after.find('>').unwrap_or(after.len())];
    after.starts_with(|c: char| c.is_whitespace() || matches!(c, '>' | '/')) && !tag.contains("xmlns=")
}

const REFERENCES: &[(&str, &str)] = &[
    ("target", "depends"),
    ("target", "extensionOf"),
    ("target", "name"),
    ("extension-point", "depends"),
    ("extension-point", "extensionOf"),
    ("extension-point", "name"),
    ("project", "default"),
    ("antcall", "target"),
    ("ant", "target"),
    ("runtarget", "target"),
];

pub fn target_at(text: &str, byte: usize) -> Option<String> {
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() },
    )
    .ok()?;
    let node = document.descendants().rfind(|node| node.is_element() && node.range().contains(&byte))?;
    let attribute = node.attributes().find(|attribute| {
        let range = attribute.range_value();
        range.start <= byte && byte <= range.end
    })?;
    let element = node.tag_name().name();
    if !REFERENCES.contains(&(element, attribute.name())) {
        return None;
    }
    if element == "ant" && node.attribute("antfile").is_some() {
        return None;
    }
    let range = attribute.range_value();
    let raw = &text[range.clone()];
    let at = byte - range.start;
    let start = raw[..at].rfind(',').map_or(0, |comma| comma + 1);
    let end = raw[at..].find(',').map_or(raw.len(), |comma| at + comma);
    let name = raw[start..end].trim();
    (!name.is_empty()).then(|| name.to_owned())
}
