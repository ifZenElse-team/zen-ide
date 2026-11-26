use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};

use roxmltree::Node;
use zen_project::settings::{CodeStyleOptions, CodeStyleOverride, DEFAULT_LANGUAGE, SchemeFile};

use super::code_style::{
    COMPONENT, CONFIG, DO_NOT_FORMAT, FORMATTER_MARKERS, LANGUAGES, PROJECT, WRAP_ON_TYPING, language_id, scheme_node,
    uses_project_scheme,
};
use super::xml;

const VERSION: &str = "173";
const OTHER_INDENT: &str = "OTHER_INDENT_OPTIONS";
const MARGINS: [&str; 2] = ["RIGHT_MARGIN", "SOFT_MARGINS"];
const INDENTS: [&str; 4] = ["USE_TAB_CHARACTER", "TAB_SIZE", "INDENT_SIZE", "CONTINUATION_INDENT_SIZE"];
const SCHEME_OPTIONS: [&str; 6] =
    ["RIGHT_MARGIN", "SOFT_MARGINS", WRAP_ON_TYPING, FORMATTER_MARKERS, DO_NOT_FORMAT, OTHER_INDENT];

pub(super) fn write(root: &Path, scheme: &SchemeFile) -> Result<Vec<PathBuf>, String> {
    let project = root.join(PROJECT);
    let config = root.join(CONFIG);
    let current = read(&project)?;
    let text = merged(current.as_deref(), scheme)?;
    let mut written = Vec::new();
    if current.as_deref() != Some(text.as_str()) {
        save(&project, &text)?;
        written.push(project);
    }
    if !read(&config)?.is_some_and(|current| uses_project_scheme(&current) == Ok(true)) {
        save(&config, &config_text())?;
        written.push(config);
    }
    Ok(written)
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn save(path: &Path, text: &str) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
    }
    zen_vfs::write_atomic(path, text.as_bytes()).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn config_text() -> String {
    format!(
        "<component name=\"{COMPONENT}\">\n  <state>\n    <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" />\n  \
         </state>\n</component>\n"
    )
}

fn escaped(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn option(name: &str, value: impl std::fmt::Display, indent: &str) -> String {
    format!("\n{indent}<option name=\"{name}\" value=\"{}\" />", escaped(&value.to_string()))
}

fn margin_options(style: &CodeStyleOverride, indent: &str) -> String {
    let mut text = String::new();
    if let Some(margin) = style.right_margin {
        text += &option("RIGHT_MARGIN", margin, indent);
    }
    if let Some(guides) = &style.visual_guides {
        let columns: Vec<String> = guides.iter().map(u16::to_string).collect();
        text += &option("SOFT_MARGINS", columns.join(","), indent);
    }
    text
}

fn indent_options(style: &CodeStyleOverride, indent: &str) -> String {
    let mut text = String::new();
    if let Some(size) = style.continuation_indent {
        text += &option("CONTINUATION_INDENT_SIZE", size, indent);
    }
    if let Some(size) = style.indent {
        text += &option("INDENT_SIZE", size, indent);
    }
    if let Some(size) = style.tab_size {
        text += &option("TAB_SIZE", size, indent);
    }
    if let Some(tabs) = style.use_tabs {
        text += &option("USE_TAB_CHARACTER", tabs, indent);
    }
    text
}

fn has_indents(style: &CodeStyleOverride) -> bool {
    !indent_options(style, "").is_empty()
}

fn scheme_options(scheme: &SchemeFile) -> String {
    const INDENT: &str = "    ";
    let default = scheme.styles.get(DEFAULT_LANGUAGE).cloned().unwrap_or_default();
    let CodeStyleOptions { wrap_on_typing, do_not_format, formatter_markers } = &scheme.options;
    let mut text = margin_options(&default, INDENT);
    if let Some(wrap) = wrap_on_typing {
        text += &option(WRAP_ON_TYPING, wrap, INDENT);
    }
    if let Some(markers) = formatter_markers {
        text += &option(FORMATTER_MARKERS, markers, INDENT);
    }
    if let Some(patterns) = do_not_format.as_ref().filter(|patterns| !patterns.is_empty()) {
        let _ = write!(text, "\n{INDENT}<option name=\"{DO_NOT_FORMAT}\">\n{INDENT}  <list>");
        for pattern in patterns {
            let _ = write!(text, "\n{INDENT}    <fileSet type=\"globPattern\" pattern=\"{}\" />", escaped(pattern));
        }
        let _ = write!(text, "\n{INDENT}  </list>\n{INDENT}</option>");
    }
    if has_indents(&default) {
        let indents = indent_options(&default, "        ");
        let _ = write!(
            text,
            "\n{INDENT}<option name=\"{OTHER_INDENT}\">\n{INDENT}  <value>{indents}\n{INDENT}  </value>\n{INDENT}</option>"
        );
    }
    text
}

fn jetbrains_name(language: &str) -> &str {
    LANGUAGES.iter().find(|(_, id)| *id == language).map_or(language, |(name, _)| name)
}

fn language_block(name: &str, style: &CodeStyleOverride) -> String {
    let mut body = margin_options(style, "      ");
    if has_indents(style) {
        let indents = indent_options(style, "        ");
        let _ = write!(body, "\n      <indentOptions>{indents}\n      </indentOptions>");
    }
    format!("\n    <codeStyleSettings language=\"{}\">{body}\n    </codeStyleSettings>", escaped(name))
}

fn language_blocks(scheme: &SchemeFile, known: &[String]) -> String {
    scheme
        .styles
        .iter()
        .filter(|(language, _)| *language != DEFAULT_LANGUAGE && !known.contains(language))
        .filter(|(_, style)| **style != CodeStyleOverride::default())
        .map(|(language, style)| language_block(jetbrains_name(language), style))
        .collect()
}

pub(super) fn scheme_text(scheme: &SchemeFile) -> String {
    format!(
        "<code_scheme name=\"{}\" version=\"{VERSION}\">{}{}\n</code_scheme>\n",
        escaped(&scheme.name),
        scheme_options(scheme),
        language_blocks(scheme, &[]),
    )
}

fn project_text(scheme: &SchemeFile) -> String {
    format!(
        "<component name=\"{COMPONENT}\">\n  <code_scheme name=\"Project\" version=\"{VERSION}\">{}{}\n  </code_scheme>\n</component>\n",
        scheme_options(scheme),
        language_blocks(scheme, &[]),
    )
}

#[derive(Default)]
struct Splices(Vec<(Range<usize>, String)>);

impl Splices {
    fn insert(&mut self, at: usize, text: String) {
        self.0.push((at..at, text));
    }

    fn remove(&mut self, source: &str, node: Node<'_, '_>) {
        let range = node.range();
        let blank = source[..range.start].trim_end_matches([' ', '\t', '\r', '\n']).len();
        self.0.push((blank..range.end, String::new()));
    }

    fn applied(mut self, source: &str) -> String {
        self.0.sort_by_key(|(range, _)| (range.start, range.end));
        let mut text = String::with_capacity(source.len());
        let mut at = 0;
        for (range, replacement) in self.0 {
            text.push_str(&source[at..range.start.max(at)]);
            text.push_str(&replacement);
            at = at.max(range.end);
        }
        text.push_str(&source[at..]);
        text
    }
}

fn named<'a, 'input>(node: Node<'a, 'input>, names: &[&str]) -> Vec<Node<'a, 'input>> {
    node.children()
        .filter(|child| child.has_tag_name("option"))
        .filter(|child| child.attribute("name").is_some_and(|name| names.contains(&name)))
        .collect()
}

fn opening_end(node: Node<'_, '_>) -> Option<usize> {
    node.first_child().map(|child| child.range().start)
}

fn last_end(node: Node<'_, '_>) -> Option<usize> {
    node.children().rfind(Node::is_element).map(|child| child.range().end)
}

fn merge_language(source: &str, node: Node<'_, '_>, style: &CodeStyleOverride, splices: &mut Splices) {
    let indent_node = node.children().find(|child| child.has_tag_name("indentOptions"));
    for option in named(node, &MARGINS) {
        splices.remove(source, option);
    }
    let margins = margin_options(style, "      ");
    let Some(at) = opening_end(node) else {
        let block = language_block(node.attribute("language").unwrap_or_default(), style);
        splices.0.push((node.range(), block.trim_start().to_owned()));
        return;
    };
    let indents = indent_options(style, "        ");
    let Some(block) = indent_node else {
        let new = if indents.is_empty() {
            String::new()
        } else {
            format!("\n      <indentOptions>{indents}\n      </indentOptions>")
        };
        splices.insert(at, margins + &new);
        return;
    };
    splices.insert(at, margins);
    match opening_end(block) {
        Some(inner) => {
            for option in named(block, &INDENTS) {
                splices.remove(source, option);
            }
            splices.insert(inner, indents);
        }
        None if !indents.is_empty() => {
            splices.0.push((block.range(), format!("<indentOptions>{indents}\n      </indentOptions>")));
        }
        None => {}
    }
}

pub(super) fn merged(current: Option<&str>, scheme: &SchemeFile) -> Result<String, String> {
    let Some(source) = current else { return Ok(project_text(scheme)) };
    let document = xml::parse(source).map_err(|error| format!("cannot parse Project.xml: {error}"))?;
    let Some(node) = scheme_node(&document) else { return Ok(project_text(scheme)) };
    let Some(top) = opening_end(node) else { return Ok(project_text(scheme)) };
    let mut splices = Splices::default();
    for option in named(node, &SCHEME_OPTIONS) {
        splices.remove(source, option);
    }
    splices.insert(top, scheme_options(scheme));
    let mut known = Vec::new();
    for language in node.children().filter(|child| child.has_tag_name("codeStyleSettings")) {
        let Some(name) = language.attribute("language") else { continue };
        let id = language_id(name);
        let style = scheme.styles.get(&id).cloned().unwrap_or_default();
        merge_language(source, language, &style, &mut splices);
        known.push(id);
    }
    if let Some(end) = last_end(node).or(Some(top)) {
        splices.insert(end, language_blocks(scheme, &known));
    }
    Ok(splices.applied(source))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zen_project::settings::{CodeStyle, CodeStyleGeneral};

    use super::super::code_style::parse_file;
    use super::*;

    fn sample() -> SchemeFile {
        let languages = BTreeMap::from([
            ("go".to_owned(), CodeStyle { use_tabs: true, tab_size: 8, indent: 8 }),
            (DEFAULT_LANGUAGE.to_owned(), CodeStyle { use_tabs: false, tab_size: 3, indent: 3 }),
        ]);
        let general = CodeStyleGeneral {
            hard_wrap_at: 100,
            wrap_on_typing: true,
            visual_guides: vec![72, 80],
            do_not_format: vec!["*.min.js".to_owned(), "a&b*".to_owned()],
            formatter_markers: true,
        };
        SchemeFile::of("Team", &languages, &general)
    }

    fn read_back(text: &str, name: &str) -> SchemeFile {
        SchemeFile { name: name.to_owned(), ..parse_file(text).unwrap() }
    }

    #[test]
    fn a_new_project_scheme_reads_back_as_written() {
        let scheme = sample();
        let text = merged(None, &scheme).unwrap();
        assert!(
            text.starts_with("<component name=\"ProjectCodeStyleConfiguration\">\n  <code_scheme name=\"Project\"")
        );
        assert_eq!(read_back(&text, "Team"), scheme);
    }

    #[test]
    fn an_exported_scheme_reads_back_as_written() {
        let scheme = sample();
        let text = scheme_text(&scheme);
        assert!(text.starts_with("<code_scheme name=\"Team\" version=\"173\">"), "{text}");
        assert_eq!(parse_file(&text).unwrap(), scheme);
    }

    #[test]
    fn writing_keeps_what_the_ide_does_not_manage() {
        let current = r#"<component name="ProjectCodeStyleConfiguration">
  <code_scheme name="Project" version="173">
    <option name="RIGHT_MARGIN" value="140" />
    <JavaCodeStyleSettings>
      <option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="99" />
    </JavaCodeStyleSettings>
    <codeStyleSettings language="go">
      <option name="KEEP_LINE_BREAKS" value="false" />
      <indentOptions>
        <option name="TAB_SIZE" value="2" />
        <option name="SMART_TABS" value="true" />
      </indentOptions>
    </codeStyleSettings>
    <codeStyleSettings language="Groovy">
      <option name="RIGHT_MARGIN" value="90" />
      <option name="ALIGN_GROUP" value="true" />
    </codeStyleSettings>
  </code_scheme>
</component>
"#;
        let scheme = sample();
        let text = merged(Some(current), &scheme).unwrap();
        for kept in [
            "<JavaCodeStyleSettings>",
            "CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND",
            "KEEP_LINE_BREAKS",
            "SMART_TABS",
            "ALIGN_GROUP",
            "language=\"Groovy\"",
        ] {
            assert!(text.contains(kept), "{kept} in {text}");
        }
        assert!(!text.contains("value=\"140\""), "{text}");
        assert!(!text.contains("value=\"90\""), "a language the scheme no longer sets loses the margin: {text}");
        assert_eq!(text.matches("name=\"TAB_SIZE\"").count(), 2, "{text}");
        assert_eq!(read_back(&text, "Team"), scheme);
    }

    #[test]
    fn writing_twice_changes_nothing() {
        let current = r#"<component name="ProjectCodeStyleConfiguration"><code_scheme name="Project" version="173"><JetCodeStyleSettings /></code_scheme></component>"#;
        let scheme = sample();
        let once = merged(Some(current), &scheme).unwrap();
        assert_eq!(merged(Some(&once), &scheme).unwrap(), once);
        assert!(once.contains("<JetCodeStyleSettings />"), "{once}");
    }

    #[test]
    fn an_empty_scheme_element_is_filled_in() {
        let current = r#"<component name="ProjectCodeStyleConfiguration"><code_scheme name="Project" version="173" /></component>"#;
        let scheme = sample();
        assert_eq!(read_back(&merged(Some(current), &scheme).unwrap(), "Team"), scheme);
    }

    #[test]
    fn a_shared_file_keeps_its_format_and_what_the_ide_does_not_know() {
        let current = r#"<component name="ProjectCodeStyleConfiguration">
  <code_scheme name="Project" version="173">
    <!-- the team's note -->
    <option name="OTHER_OPTION" value="kept" />
    <FutureCodeStyleSettings unknown="1">
      <option name="NOT_KNOWN_YET" value="&lt;kept&gt;" />
    </FutureCodeStyleSettings>
    <codeStyleSettings language="kotlin" extra="yes">
      <option name="LINE_COMMENT_AT_FIRST_COLUMN" value="false" />
    </codeStyleSettings>
  </code_scheme>
</component>
"#;
        let text = merged(Some(current), &sample()).unwrap();
        for kept in [
            "<!-- the team's note -->",
            r#"<option name="OTHER_OPTION" value="kept" />"#,
            r#"<FutureCodeStyleSettings unknown="1">"#,
            r#"<option name="NOT_KNOWN_YET" value="&lt;kept&gt;" />"#,
            r#"<codeStyleSettings language="kotlin" extra="yes">"#,
            r#"<option name="LINE_COMMENT_AT_FIRST_COLUMN" value="false" />"#,
        ] {
            assert!(text.contains(kept), "{kept} in {text}");
        }
        assert!(
            text.starts_with(
                "<component name=\"ProjectCodeStyleConfiguration\">\n  <code_scheme name=\"Project\" version=\"173\">"
            ),
            "{text}"
        );
        assert!(text.contains("<option name=\"TAB_SIZE\" value=\""), "the reference's own option names: {text}");
        assert_eq!(read_back(&text, "Team"), sample());
    }

    #[test]
    fn the_config_file_is_the_one_the_format_names() {
        assert_eq!(
            config_text(),
            "<component name=\"ProjectCodeStyleConfiguration\">\n  <state>\n    <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" />\n  </state>\n</component>\n"
        );
    }

    #[test]
    fn a_broken_file_is_not_overwritten() {
        assert!(merged(Some("<component"), &sample()).is_err());
    }

    #[test]
    fn writing_a_project_creates_both_files_and_turns_the_scheme_on() {
        let dir = tempfile::tempdir().unwrap();
        let written = write(dir.path(), &sample()).unwrap();
        assert_eq!(written.len(), 2);
        let config = std::fs::read_to_string(dir.path().join(CONFIG)).unwrap();
        assert_eq!(uses_project_scheme(&config), Ok(true));
        assert!(write(dir.path(), &sample()).unwrap().is_empty(), "nothing changed, nothing written");
    }
}
