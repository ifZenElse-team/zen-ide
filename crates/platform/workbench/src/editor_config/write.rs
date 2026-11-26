use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::Session;
use zen_project::settings::{CodeStyle, DEFAULT_LANGUAGE, FileCodeStyle};

use crate::{code_style, line_separators, project_settings};

pub const DEFAULT_RIGHT_MARGIN: u16 = 120;

/// The property stubs a new `.editorconfig` file is written with, as File → New → EditorConfig
/// File's check boxes choose them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Stubs {
    /// `root = true`: the folders above are not read.
    pub root: bool,
    /// The standard EditorConfig properties: `charset`, `end_of_line`, `indent_size`,
    /// `indent_style`, `insert_final_newline`, `max_line_length`, `tab_width`,
    /// `trim_trailing_whitespace`.
    pub standard: bool,
    /// The IntelliJ properties the IDE reads: `ij_continuation_indent_size`, `ij_visual_guides`.
    pub intellij: bool,
}

impl Default for Stubs {
    fn default() -> Self {
        Self { root: true, standard: true, intellij: false }
    }
}

fn indent_lines(style: &FileCodeStyle) -> Vec<String> {
    vec![
        format!("indent_size = {}", style.indent),
        format!("indent_style = {}", if style.use_tabs { "tab" } else { "space" }),
        format!("tab_width = {}", style.tab_size),
    ]
}

fn intellij_lines(style: &FileCodeStyle) -> Vec<String> {
    let guides = if style.visual_guides.is_empty() {
        "none".to_owned()
    } else {
        style.visual_guides.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
    };
    vec![
        format!(
            "ij_continuation_indent_size = {}",
            style.continuation_indent.unwrap_or(style.indent.saturating_mul(2))
        ),
        format!("ij_visual_guides = {guides}"),
    ]
}

fn finish(session: &Session, path: &Path, blocks: Vec<Vec<String>>) -> String {
    let text: String = blocks
        .into_iter()
        .filter(|block| !block.is_empty())
        .map(|block| block.join("\n") + "\n")
        .collect::<Vec<_>>()
        .join("\n");
    let separator = line_separators::for_new_file(session, path);
    zen_text::line_separator::convert(&text, separator).into_owned()
}

pub fn stub_text(session: &Session, folder: &Path, stubs: Stubs) -> String {
    let sample = folder.join("file");
    let style = project_settings::file_code_style(session, &sample);
    let save = super::save_properties(session, &sample);
    let mut section = Vec::new();
    if stubs.standard {
        let separator = line_separators::for_new_file(session, &sample);
        let charset = save.charset.map_or("utf-8", crate::charsets::Charset::id);
        section.push(format!("charset = {charset}"));
        section.push(format!("end_of_line = {}", separator.id()));
        section.extend(indent_lines(&style).into_iter().take(2));
        section.push(format!("insert_final_newline = {}", save.insert_final_newline.unwrap_or(false)));
        section.push(format!("max_line_length = {}", style.right_margin.unwrap_or(DEFAULT_RIGHT_MARGIN)));
        section.push(format!("tab_width = {}", style.tab_size));
        section.push(format!("trim_trailing_whitespace = {}", save.trim_trailing_whitespace.unwrap_or(false)));
    }
    if stubs.intellij {
        section.extend(intellij_lines(&style));
    }
    if !section.is_empty() {
        section.insert(0, "[*]".to_owned());
    }
    let preamble = if stubs.root { vec!["root = true".to_owned()] } else { Vec::new() };
    finish(session, &folder.join(zen_project::editorconfig::FILE_NAME), vec![preamble, section])
}

fn scheme_style(session: &Session, language: &str) -> FileCodeStyle {
    let user = code_style::explicit(session.settings(), language).unwrap_or_else(|| CodeStyle::default_for(language));
    let base = FileCodeStyle::from(user);
    match project_settings::code_style_override(session, language) {
        Some(project) => project.apply_to_file(base),
        None => base,
    }
}

fn section_glob(globs: &[String]) -> String {
    match globs {
        [one] => one.clone(),
        many => format!("{{{}}}", many.join(",")),
    }
}

pub fn scheme_text(session: &Session, path: &Path) -> String {
    let default = scheme_style(session, DEFAULT_LANGUAGE);
    let separator = line_separators::chosen(session);
    let mut all = vec!["[*]".to_owned()];
    if let Some(separator) = zen_text::LineSeparator::parse(&separator) {
        all.push(format!("end_of_line = {}", separator.id()));
    }
    let lines = indent_lines(&default);
    all.extend(lines[..2].iter().cloned());
    all.push(format!("max_line_length = {}", default.right_margin.unwrap_or(DEFAULT_RIGHT_MARGIN)));
    all.push(lines[2].clone());
    all.extend(intellij_lines(&default));
    let mut languages: Vec<String> = CodeStyle::BUILT_IN.iter().map(|(language, _)| (*language).to_owned()).collect();
    let user = session.settings().iter().filter_map(|(key, _)| {
        let rest = key.strip_prefix(code_style::PREFIX)?;
        Some(rest.rsplit_once('.')?.0.to_owned())
    });
    let project = session
        .state::<project_settings::ProjectSettingsState>()
        .map(|state| state.settings().code_style.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    languages.extend(user.chain(project).chain(crate::code_style_options::declared_languages(session)));
    languages.sort();
    languages.dedup();
    let mut blocks = vec![vec!["root = true".to_owned()], all];
    for language in languages.iter().filter(|language| *language != DEFAULT_LANGUAGE) {
        let globs = code_style::globs_of(language);
        let style = scheme_style(session, language);
        let declared = crate::code_style_options::editor_config_pairs(session, language);
        if globs.is_empty() || (style.code_style() == default.code_style() && declared.is_empty()) {
            continue;
        }
        let mut section = vec![format!("[{}]", section_glob(&globs))];
        section.extend(indent_lines(&style));
        section.extend(declared.into_iter().map(|(key, value)| format!("{key} = {value}")));
        blocks.push(section);
    }
    finish(session, path, blocks)
}
