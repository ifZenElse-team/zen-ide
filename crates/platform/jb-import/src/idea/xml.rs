use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};

pub const PROJECT_DIR: &str = "$PROJECT_DIR$";
pub const MODULE_DIR: &str = "$MODULE_DIR$";
pub const USER_HOME: &str = "$USER_HOME$";

const FILE_URL: &str = "file://";
const MAX_NESTING: usize = 256;

pub fn parse(text: &str) -> Result<Document<'_>, String> {
    if nesting(text) > MAX_NESTING {
        return Err(format!("the XML is nested deeper than {MAX_NESTING} elements"));
    }
    Document::parse(text).map_err(|error| error.to_string())
}

fn nesting(text: &str) -> usize {
    let bytes = text.as_bytes();
    let (mut depth, mut deepest, mut at) = (0usize, 0usize, 0usize);
    let skip_to = |from: usize, end: &str| text[from..].find(end).map_or(bytes.len(), |found| from + found + end.len());
    while let Some(found) = text[at..].find('<') {
        let start = at + found;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            at = skip_to(start, "-->");
        } else if rest.starts_with("<![CDATA[") {
            at = skip_to(start, "]]>");
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            at = skip_to(start, ">");
        } else if rest.starts_with("</") {
            depth = depth.saturating_sub(1);
            at = skip_to(start, ">");
        } else {
            let mut quote = None;
            let mut end = bytes.len();
            for (offset, byte) in bytes[start + 1..].iter().enumerate() {
                match (quote, *byte) {
                    (None, b'"' | b'\'') => quote = Some(*byte),
                    (Some(open), byte) if byte == open => quote = None,
                    (None, b'>') => {
                        end = start + 1 + offset;
                        break;
                    }
                    _ => {}
                }
            }
            if end == bytes.len() || bytes[end - 1] != b'/' {
                depth += 1;
                deepest = deepest.max(depth);
            }
            at = (end + 1).min(bytes.len());
        }
        if at >= bytes.len() {
            break;
        }
    }
    deepest
}

pub fn component<'a, 'input>(document: &'a Document<'input>, name: &str) -> Option<Node<'a, 'input>> {
    let root = document.root_element();
    let named = |node: &Node<'_, '_>| node.has_tag_name("component") && node.attribute("name") == Some(name);
    if named(&root) {
        return Some(root);
    }
    root.children().find(named)
}

pub fn option<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.has_tag_name("option") && child.attribute("name") == Some(name))
        .and_then(|child| child.attribute("value"))
}

pub fn options<'a>(node: Node<'a, '_>) -> Vec<(&'a str, &'a str)> {
    node.children()
        .filter(|child| child.has_tag_name("option"))
        .filter_map(|child| Some((child.attribute("name")?, child.attribute("value")?)))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macros {
    pub project_dir: PathBuf,
    pub module_dir: Option<PathBuf>,
    pub user_home: Option<PathBuf>,
}

impl Macros {
    pub fn new(project_dir: &Path, user_home: Option<&Path>) -> Self {
        Self { project_dir: project_dir.to_owned(), module_dir: None, user_home: user_home.map(Path::to_owned) }
    }

    pub fn for_module(&self, module_dir: &Path) -> Self {
        Self { module_dir: Some(module_dir.to_owned()), ..self.clone() }
    }

    pub fn expand(&self, text: &str) -> String {
        let mut expanded = text.to_owned();
        let known = [
            (MODULE_DIR, self.module_dir.as_deref()),
            (PROJECT_DIR, Some(self.project_dir.as_path())),
            (USER_HOME, self.user_home.as_deref()),
        ];
        for (name, value) in known {
            if let Some(value) = value {
                expanded = expanded.replace(name, value.to_string_lossy().trim_end_matches('/'));
            }
        }
        expanded
    }

    pub fn path(&self, value: &str) -> PathBuf {
        let expanded = PathBuf::from(self.expand(value));
        let absolute = if expanded.is_absolute() { expanded } else { self.project_dir.join(expanded) };
        let mut normal = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !normal.pop() {
                        normal.push(component);
                    }
                }
                other => normal.push(other),
            }
        }
        normal
    }

    pub fn url(&self, url: &str) -> Option<PathBuf> {
        url.strip_prefix(FILE_URL).map(|rest| self.path(rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="Other"><option name="A" value="1" /></component>
  <component name="ProjectModuleManager">
    <option name="FIRST" value="one" />
    <option name="SECOND" value="two" />
    <option name="NO_VALUE" />
    <modules />
  </component>
</project>"#;

    #[test]
    fn a_component_is_found_by_name_under_the_root_or_as_the_root() {
        let document = Document::parse(PROJECT).unwrap();
        let manager = component(&document, "ProjectModuleManager").unwrap();
        assert_eq!(option(manager, "FIRST"), Some("one"));
        assert_eq!(option(manager, "A"), None);
        assert_eq!(option(manager, "NO_VALUE"), None);
        assert_eq!(options(manager), vec![("FIRST", "one"), ("SECOND", "two")]);
        assert!(component(&document, "Missing").is_none());
        let single = Document::parse(r#"<component name="Root"><option name="X" value="y"/></component>"#).unwrap();
        assert_eq!(component(&single, "Root").and_then(|node| option(node, "X")), Some("y"));
    }

    #[test]
    fn macros_expand_and_file_urls_become_paths() {
        let macros = Macros::new(Path::new("/work/shop"), Some(Path::new("/home/ann/")));
        assert_eq!(macros.expand("$PROJECT_DIR$/build"), "/work/shop/build");
        assert_eq!(macros.expand("$USER_HOME$/go"), "/home/ann/go");
        assert_eq!(macros.expand("$MODULE_DIR$/out"), "$MODULE_DIR$/out", "no module, no expansion");
        assert_eq!(macros.expand("$UNKNOWN$/x"), "$UNKNOWN$/x");
        let module = macros.for_module(Path::new("/work/shop/app"));
        assert_eq!(module.expand("$MODULE_DIR$/out"), "/work/shop/app/out");
        assert_eq!(module.url("file://$MODULE_DIR$/dist"), Some(PathBuf::from("/work/shop/app/dist")));
        assert_eq!(module.url("file://$PROJECT_DIR$"), Some(PathBuf::from("/work/shop")));
        assert_eq!(module.url("file:///abs/path"), Some(PathBuf::from("/abs/path")));
        assert_eq!(module.url("jar://$PROJECT_DIR$/lib.jar!/"), None);
        assert_eq!(macros.path("relative/dir"), PathBuf::from("/work/shop/relative/dir"));
        assert_eq!(Macros::new(Path::new("/p"), None).expand("$USER_HOME$"), "$USER_HOME$");
    }

    #[test]
    fn paths_are_normalised() {
        let macros = Macros::new(Path::new("/p"), None).for_module(Path::new("/p/.idea/modules"));
        assert_eq!(macros.path("$MODULE_DIR$/../../gen"), PathBuf::from("/p/gen"));
        assert_eq!(macros.path("./a/./b"), PathBuf::from("/p/a/b"));
    }

    #[test]
    fn deep_nesting_is_refused_before_parsing() {
        assert_eq!(nesting("<?xml version=\"1.0\"?><!-- <a><a> --><a x=\"<b>\"><b/><c></c></a>"), 2);
        let deep = format!("{}{}", "<a>".repeat(20_000), "</a>".repeat(20_000));
        assert!(parse(&deep).unwrap_err().contains("nested"));
        assert!(parse("<a><b/></a>").is_ok());
        assert!(parse("<a").is_err());
    }
}
