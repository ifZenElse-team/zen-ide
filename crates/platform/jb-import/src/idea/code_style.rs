use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use roxmltree::Node;
use zen_project::settings::{
    CodeStyleOptions, CodeStyleOverride, DEFAULT_LANGUAGE, PLAIN_TEXT, ReadFiles, SchemeFile, SettingsProblem,
};

use super::xml;

pub(super) const CONFIG: &str = ".idea/codeStyles/codeStyleConfig.xml";
pub(super) const PROJECT: &str = ".idea/codeStyles/Project.xml";
pub(super) const COMPONENT: &str = "ProjectCodeStyleConfiguration";
pub(super) const WRAP_ON_TYPING: &str = "WRAP_WHEN_TYPING_REACHES_RIGHT_MARGIN";
pub(super) const FORMATTER_MARKERS: &str = "FORMATTER_TAGS_ENABLED";
pub(super) const DO_NOT_FORMAT: &str = "DO_NOT_FORMAT";

pub(super) const LANGUAGES: &[(&str, &str)] = &[
    ("go", "go"),
    ("Rust", "rust"),
    ("JSON", "json"),
    ("JSON5", "json5"),
    ("yaml", "yaml"),
    ("TOML", "toml"),
    ("Markdown", "markdown"),
    ("XML", "xml"),
    ("HTML", "html"),
    ("CSS", "css"),
    ("SCSS", "scss"),
    ("JavaScript", "javascript"),
    ("TypeScript", "typescript"),
    ("Shell Script", "shellscript"),
    ("Python", "python"),
    ("SQL", "sql"),
    ("Dockerfile", "dockerfile"),
    ("Makefile", "makefile"),
    ("Properties", "properties"),
    ("protobuf", "proto"),
    ("JAVA", "java"),
    ("kotlin", "kotlin"),
    ("TEXT", PLAIN_TEXT),
];

pub(super) fn language_id(name: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(jetbrains, _)| *jetbrains == name)
        .map_or_else(|| name.to_lowercase(), |(_, id)| (*id).to_owned())
}

pub(super) fn read(
    root: &Path,
    files: &dyn ReadFiles,
    problems: &mut Vec<SettingsProblem>,
) -> (BTreeMap<String, CodeStyleOverride>, CodeStyleOptions) {
    let nothing = || (BTreeMap::new(), CodeStyleOptions::default());
    let Some(config) = files.read(Path::new(CONFIG)) else { return nothing() };
    let mut report = |file: &str, message: String| problems.push(SettingsProblem { file: root.join(file), message });
    match uses_project_scheme(&config) {
        Ok(true) => {}
        Ok(false) => return nothing(),
        Err(message) => {
            report(CONFIG, message);
            return nothing();
        }
    }
    let Some(scheme) = files.read(Path::new(PROJECT)) else { return nothing() };
    match parse_scheme(&scheme) {
        Ok((styles, messages)) => {
            for message in messages {
                report(PROJECT, message);
            }
            let options = xml::parse(&scheme).ok().map_or_else(CodeStyleOptions::default, |document| {
                scheme_node(&document).map_or_else(CodeStyleOptions::default, options_of)
            });
            (styles, options)
        }
        Err(message) => {
            report(PROJECT, message);
            nothing()
        }
    }
}

pub(super) fn scheme_node<'a, 'input>(document: &'a roxmltree::Document<'input>) -> Option<Node<'a, 'input>> {
    xml::component(document, COMPONENT)
        .and_then(|component| component.children().find(|child| child.has_tag_name("code_scheme")))
        .or_else(|| Some(document.root_element()).filter(|root| root.has_tag_name("code_scheme")))
}

fn options_of(scheme: Node<'_, '_>) -> CodeStyleOptions {
    let flag = |name: &str| xml::option(scheme, name).map(|value| value == "true");
    let do_not_format = scheme
        .children()
        .find(|child| child.has_tag_name("option") && child.attribute("name") == Some(DO_NOT_FORMAT))
        .map(|option| {
            option
                .descendants()
                .filter(|node| node.has_tag_name("fileSet"))
                .filter_map(|node| node.attribute("pattern"))
                .map(str::to_owned)
                .collect()
        });
    CodeStyleOptions { wrap_on_typing: flag(WRAP_ON_TYPING), do_not_format, formatter_markers: flag(FORMATTER_MARKERS) }
}

pub(super) fn parse_file(text: &str) -> Result<SchemeFile, String> {
    let (styles, _) = parse_scheme(text)?;
    let document = xml::parse(text).map_err(|error| format!("cannot parse the scheme: {error}"))?;
    let scheme = scheme_node(&document).ok_or_else(|| "the file holds no code_scheme".to_owned())?;
    let name = scheme.attribute("name").unwrap_or_default().to_owned();
    Ok(SchemeFile { name, styles, options: options_of(scheme) })
}

pub(super) fn uses_project_scheme(text: &str) -> Result<bool, String> {
    let document = xml::parse(text).map_err(|error| format!("cannot parse codeStyleConfig.xml: {error}"))?;
    let Some(component) = xml::component(&document, COMPONENT) else { return Ok(false) };
    let state = component.children().find(|child| child.has_tag_name("state")).unwrap_or(component);
    Ok(xml::option(state, "USE_PER_PROJECT_SETTINGS") == Some("true"))
}

fn parse_scheme(text: &str) -> Result<(BTreeMap<String, CodeStyleOverride>, Vec<String>), String> {
    let document = xml::parse(text).map_err(|error| format!("cannot parse Project.xml: {error}"))?;
    let mut styles = BTreeMap::new();
    let mut messages = Vec::new();
    let Some(scheme) = scheme_node(&document) else { return Ok((styles, messages)) };
    let mut other = CodeStyleOverride::default();
    right_margin(scheme, &mut other, &mut messages);
    let other_indent = scheme
        .children()
        .find(|child| child.has_tag_name("option") && child.attribute("name") == Some("OTHER_INDENT_OPTIONS"))
        .and_then(|option| option.children().find(|child| child.has_tag_name("value")));
    if let Some(value) = other_indent {
        indent_options(value, &mut other, &mut messages);
    }
    if other != CodeStyleOverride::default() {
        styles.insert(DEFAULT_LANGUAGE.to_owned(), other);
    }
    for settings in scheme.children().filter(|child| child.has_tag_name("codeStyleSettings")) {
        let Some(name) = settings.attribute("language") else { continue };
        let mut style = CodeStyleOverride::default();
        right_margin(settings, &mut style, &mut messages);
        if let Some(indent) = settings.children().find(|child| child.has_tag_name("indentOptions")) {
            indent_options(indent, &mut style, &mut messages);
        }
        if style != CodeStyleOverride::default() {
            styles.insert(language_id(name), style);
        }
    }
    Ok((styles, messages))
}

fn right_margin(node: Node<'_, '_>, style: &mut CodeStyleOverride, messages: &mut Vec<String>) {
    if let Some(value) = xml::option(node, "RIGHT_MARGIN") {
        style.right_margin = positive(node, "RIGHT_MARGIN", value, messages);
    }
    if let Some(value) = xml::option(node, "SOFT_MARGINS") {
        style.visual_guides = visual_guides(node, value, messages);
    }
}

fn visual_guides(node: Node<'_, '_>, value: &str, messages: &mut Vec<String>) -> Option<Vec<u16>> {
    let mut columns = Vec::new();
    for part in value.split(',').map(str::trim).filter(|part| !part.is_empty()) {
        match part.parse::<u16>() {
            Ok(column) if column > 0 => columns.push(column),
            _ => {
                messages.push(invalid(node, "SOFT_MARGINS", value));
                return None;
            }
        }
    }
    columns.sort_unstable();
    columns.dedup();
    Some(columns)
}

fn indent_options(node: Node<'_, '_>, style: &mut CodeStyleOverride, messages: &mut Vec<String>) {
    for (name, value) in xml::options(node) {
        match name {
            "USE_TAB_CHARACTER" => match value {
                "true" => style.use_tabs = Some(true),
                "false" => style.use_tabs = Some(false),
                _ => messages.push(invalid(node, name, value)),
            },
            "TAB_SIZE" => style.tab_size = positive(node, name, value, messages),
            "INDENT_SIZE" => style.indent = positive(node, name, value, messages),
            "CONTINUATION_INDENT_SIZE" => style.continuation_indent = number(node, name, value, messages),
            _ => {}
        }
    }
}

fn number<T: FromStr>(node: Node<'_, '_>, name: &str, value: &str, messages: &mut Vec<String>) -> Option<T> {
    let parsed = value.trim().parse().ok();
    if parsed.is_none() {
        messages.push(invalid(node, name, value));
    }
    parsed
}

fn positive<T: FromStr + Default + PartialEq>(
    node: Node<'_, '_>,
    name: &str,
    value: &str,
    messages: &mut Vec<String>,
) -> Option<T> {
    let parsed = value.trim().parse().ok().filter(|parsed: &T| *parsed != T::default());
    if parsed.is_none() {
        messages.push(invalid(node, name, value));
    }
    parsed
}

fn invalid(node: Node<'_, '_>, name: &str, value: &str) -> String {
    let language = node
        .ancestors()
        .find_map(|ancestor| ancestor.attribute("language"))
        .map_or_else(String::new, |language| format!(" of {language}"));
    format!("{name}{language} has the invalid value \"{value}\"; it is ignored")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(use_tabs: Option<bool>, tab_size: Option<u8>, indent: Option<u8>) -> CodeStyleOverride {
        CodeStyleOverride { use_tabs, tab_size, indent, ..Default::default() }
    }

    #[test]
    fn per_project_settings_must_be_on() {
        let on = r#"<component name="ProjectCodeStyleConfiguration"><state>
            <option name="USE_PER_PROJECT_SETTINGS" value="true" /></state></component>"#;
        assert_eq!(uses_project_scheme(on), Ok(true));
        let preferred = r#"<component name="ProjectCodeStyleConfiguration"><state>
            <option name="PREFERRED_PROJECT_CODE_STYLE" value="Team" /></state></component>"#;
        assert_eq!(uses_project_scheme(preferred), Ok(false));
        let off = on.replace("true", "false");
        assert_eq!(uses_project_scheme(&off), Ok(false));
        let wrapped = format!(r#"<project version="4">{on}</project>"#);
        assert_eq!(uses_project_scheme(&wrapped), Ok(true));
        assert_eq!(uses_project_scheme("<project/>"), Ok(false));
        assert!(uses_project_scheme("<component").is_err());
    }

    #[test]
    fn several_languages_and_the_other_options_are_read() {
        let scheme = r#"<component name="ProjectCodeStyleConfiguration">
          <code_scheme name="Project" version="173">
            <option name="RIGHT_MARGIN" value="100" />
            <option name="OTHER_INDENT_OPTIONS"><value>
              <option name="INDENT_SIZE" value="3" />
              <option name="USE_TAB_CHARACTER" value="false" />
            </value></option>
            <codeStyleSettings language="go">
              <option name="RIGHT_MARGIN" value="140" />
              <indentOptions>
                <option name="USE_TAB_CHARACTER" value="true" />
                <option name="TAB_SIZE" value="8" />
                <option name="CONTINUATION_INDENT_SIZE" value="0" />
                <option name="SMART_TABS" value="true" />
              </indentOptions>
            </codeStyleSettings>
            <codeStyleSettings language="yaml">
              <indentOptions><option name="INDENT_SIZE" value="2" /></indentOptions>
            </codeStyleSettings>
            <codeStyleSettings language="Shell Script">
              <indentOptions><option name="TAB_SIZE" value="2" /></indentOptions>
            </codeStyleSettings>
            <codeStyleSettings language="Groovy">
              <indentOptions><option name="INDENT_SIZE" value="4" /></indentOptions>
            </codeStyleSettings>
            <codeStyleSettings language="XML">
              <option name="WRAP_LONG_LINES" value="true" />
            </codeStyleSettings>
          </code_scheme>
        </component>"#;
        let (styles, messages) = parse_scheme(scheme).unwrap();
        assert!(messages.is_empty(), "{messages:?}");
        assert_eq!(
            styles[DEFAULT_LANGUAGE],
            CodeStyleOverride { right_margin: Some(100), ..style(Some(false), None, Some(3)) }
        );
        assert_eq!(
            styles["go"],
            CodeStyleOverride {
                continuation_indent: Some(0),
                right_margin: Some(140),
                ..style(Some(true), Some(8), None)
            }
        );
        assert_eq!(styles["yaml"], style(None, None, Some(2)));
        assert_eq!(styles["shellscript"], style(None, Some(2), None));
        assert_eq!(styles["groovy"], style(None, None, Some(4)), "an unknown language keeps its name, lowercased");
        assert!(!styles.contains_key("xml"), "nothing we read, no override");
    }

    #[test]
    fn missing_options_set_nothing_and_invalid_ones_are_reported() {
        let empty = r#"<component name="ProjectCodeStyleConfiguration"><code_scheme name="Project">
            <codeStyleSettings language="JSON"><indentOptions /></codeStyleSettings>
        </code_scheme></component>"#;
        assert_eq!(parse_scheme(empty).unwrap(), (BTreeMap::new(), Vec::new()));
        assert_eq!(parse_scheme("<component name=\"Other\" />").unwrap(), (BTreeMap::new(), Vec::new()));
        let invalid = r#"<code_scheme name="Project">
            <option name="RIGHT_MARGIN" value="wide" />
            <codeStyleSettings language="go"><indentOptions>
              <option name="TAB_SIZE" value="0" />
              <option name="INDENT_SIZE" value="300" />
              <option name="USE_TAB_CHARACTER" value="yes" />
              <option name="CONTINUATION_INDENT_SIZE" value="4" />
            </indentOptions></codeStyleSettings>
        </code_scheme>"#;
        let (styles, messages) = parse_scheme(invalid).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles["go"], CodeStyleOverride { continuation_indent: Some(4), ..Default::default() });
        assert_eq!(messages.len(), 4, "{messages:?}");
        assert!(messages[0].starts_with("RIGHT_MARGIN has the invalid value \"wide\""), "{}", messages[0]);
        assert!(messages[1].starts_with("TAB_SIZE of go has the invalid value \"0\""), "{}", messages[1]);
        assert!(parse_scheme("<code_scheme").is_err());
    }

    #[test]
    fn visual_guides_are_read_with_the_right_margin() {
        let scheme = r#"<code_scheme name="Project">
            <option name="SOFT_MARGINS" value="100, 80,100" />
            <codeStyleSettings language="go">
              <option name="RIGHT_MARGIN" value="120" />
              <option name="SOFT_MARGINS" value="" />
            </codeStyleSettings>
            <codeStyleSettings language="JSON"><option name="SOFT_MARGINS" value="80,x" /></codeStyleSettings>
        </code_scheme>"#;
        let (styles, messages) = parse_scheme(scheme).unwrap();
        assert_eq!(styles[DEFAULT_LANGUAGE].visual_guides, Some(vec![80, 100]), "sorted, each once");
        assert_eq!(styles["go"].visual_guides, Some(Vec::new()), "an empty list clears the project's");
        assert_eq!(styles["go"].right_margin, Some(120));
        assert!(!styles.contains_key("json"));
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].starts_with("SOFT_MARGINS of JSON has the invalid value \"80,x\""), "{}", messages[0]);
    }

    #[test]
    fn the_options_of_no_language_are_read() {
        let text = r#"<code_scheme name="Team" version="173">
            <option name="WRAP_WHEN_TYPING_REACHES_RIGHT_MARGIN" value="true" />
            <option name="FORMATTER_TAGS_ENABLED" value="false" />
            <option name="DO_NOT_FORMAT"><list>
              <fileSet type="globPattern" pattern="*.min.js" />
              <fileSet type="globPattern" pattern="gen/*" />
            </list></option>
        </code_scheme>"#;
        let file = parse_file(text).unwrap();
        assert_eq!(file.name, "Team");
        assert_eq!(file.options.wrap_on_typing, Some(true));
        assert_eq!(file.options.formatter_markers, Some(false));
        assert_eq!(file.options.do_not_format, Some(vec!["*.min.js".to_owned(), "gen/*".to_owned()]));
        assert!(parse_file("<project/>").is_err());
    }

    #[test]
    fn the_language_table_maps_names_once() {
        assert_eq!(language_id("go"), "go");
        assert_eq!(language_id("JSON"), "json");
        assert_eq!(language_id("TOML"), "toml");
        assert_eq!(language_id("Markdown"), "markdown");
        assert_eq!(language_id("TEXT"), "plaintext");
        assert_eq!(language_id("CoffeeScript"), "coffeescript");
        let mut names: Vec<&str> = LANGUAGES.iter().map(|(name, _)| *name).collect();
        let mut ids: Vec<&str> = LANGUAGES.iter().map(|(_, id)| *id).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!((names.len(), ids.len()), (count, count));
        assert!(ids.iter().all(|id| *id == id.to_lowercase() && *id != DEFAULT_LANGUAGE));
    }
}
