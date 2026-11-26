use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::{Conversion, Feature, TextFeatures};

use crate::Workbench;

/// The parameters of `file.convert.toLanguage`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertFileParams {
    /// The file to convert, absolute or relative to the project; the file shown in the editor by default.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The extension of the language to convert into, such as `kt`.
    pub to: String,
    /// Keep the original file; by default it is moved to the Trash once the new file is written.
    #[serde(default)]
    pub keep: bool,
}

/// The parameters of `editor.convert.text`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertTextParams {
    /// The extension of the text's language, such as `java`.
    pub from: String,
    /// The extension of the language to convert into, such as `kt`.
    pub to: String,
    /// The code to convert: a whole file, members of a class or statements.
    pub text: String,
}

/// A place in the original text that the conversion could not carry over exactly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversionProblem {
    /// The line in the original text, from 1.
    pub line: usize,
    /// The column in the original text, from 1, in characters.
    pub column: usize,
    /// What to check.
    pub message: String,
}

/// What `editor.convert.text` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertedText {
    /// The converted code.
    pub text: String,
    /// The places to check, in the original text.
    pub problems: Vec<ConversionProblem>,
}

/// What `file.convert.toLanguage` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertedFile {
    /// The new file, opened in the editor.
    pub path: PathBuf,
    /// Whether the original file was moved to the Trash.
    pub removed: bool,
    /// The places to check, in the original file.
    pub problems: Vec<ConversionProblem>,
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase())
}

pub fn converter(session: &Session, from: &str, to: &str) -> Option<Arc<dyn TextFeatures>> {
    let target = PathBuf::from(format!("converted.{to}"));
    let plugin = super::plugin_for(session, &target, Feature::Conversion)?;
    plugin.converts_from().iter().any(|known| known.eq_ignore_ascii_case(from)).then_some(plugin)
}

pub fn convertible(session: &Session, path: &Path) -> bool {
    let Some(from) = extension_of(path) else { return false };
    session.state::<super::PluginFeatures>().is_some_and(|features| {
        features
            .all()
            .iter()
            .any(|plugin| plugin.supports(Feature::Conversion) && plugin.converts_from().contains(&from))
    })
}

fn problems(source: &str, conversion: &Conversion) -> Vec<ConversionProblem> {
    conversion
        .problems
        .iter()
        .map(|problem| {
            let before = &source[..problem.range.start.min(source.len())];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().map_or(0, |part| part.chars().count()) + 1;
            ConversionProblem { line, column, message: problem.message.clone() }
        })
        .collect()
}

pub fn convert_text(session: &Session, from: &str, to: &str, text: &str) -> Result<ConvertedText, CommandError> {
    let plugin = converter(session, from, to).ok_or_else(|| {
        CommandError::new(t!("workbench-convert-unsupported", from = from.to_owned(), to = to.to_owned()))
    })?;
    let conversion = plugin
        .convert(&from.to_ascii_lowercase(), text)
        .ok_or_else(|| CommandError::new(t!("workbench-convert-failed")))?;
    Ok(ConvertedText { problems: problems(text, &conversion), text: conversion.text })
}

fn source_text(session: &Session, path: &Path) -> Result<String, CommandError> {
    if let Some(document) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
    {
        return Ok(document.text().to_owned());
    }
    std::fs::read_to_string(path).map_err(|error| {
        CommandError::new(t!(
            "workbench-convert-read-failed",
            path = path.display().to_string(),
            error = error.to_string()
        ))
    })
}

pub fn convert_file(session: &mut Session, params: &ConvertFileParams) -> Result<ConvertedFile, CommandError> {
    let path = match &params.path {
        Some(path) => session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(path)),
        None => session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    let from = extension_of(&path).ok_or_else(|| {
        CommandError::new(t!("workbench-convert-unsupported", from = String::new(), to = params.to.clone()))
    })?;
    let to = params.to.trim_start_matches('.').to_ascii_lowercase();
    let target = path.with_extension(&to);
    if target.exists() {
        return Err(CommandError::new(t!("workbench-convert-exists", path = target.display().to_string())));
    }
    crate::trust::guard_agent_data_folder(session, &[path.clone(), target.clone()])?;
    let source = source_text(session, &path)?;
    let converted = convert_text(session, &from, &to, &source)?;
    std::fs::write(&target, converted.text.as_bytes()).map_err(|error| {
        CommandError::new(t!(
            "workbench-convert-write-failed",
            path = target.display().to_string(),
            error = error.to_string()
        ))
    })?;
    let removed = !params.keep
        && crate::delete_entries(session, std::slice::from_ref(&path)).is_ok_and(|gone| gone.contains(&path));
    crate::open_document(session, &target)?;
    Ok(ConvertedFile { path: target, removed, problems: converted.problems })
}

pub struct ConvertFile;

impl Command for ConvertFile {
    type Params = ConvertFileParams;
    type Output = ConvertedFile;
    const ID: &'static str = "file.convert.toLanguage";
    const TITLE: &'static str = "Convert File to Another Language";
    const DESCRIPTION: &'static str = "Converts a file (the one in the editor by default) into the language of the \
         extension `to` with the plug-in that converts into it, such as Java into Kotlin (`to: \"kt\"`): writes the new \
         file beside it with the same name, opens it, and moves the original to the Trash unless `keep` (Local History \
         keeps it). `problems` are the places of the original that were kept as they were or need a look, with lines \
         and columns from 1. Refused when the new file exists.";

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<super::PluginFeatures>()
            .is_some_and(|features| features.all().iter().any(|plugin| plugin.supports(Feature::Conversion)))
    }

    fn run(&self, session: &mut Session, params: ConvertFileParams) -> Result<ConvertedFile, CommandError> {
        convert_file(session, &params)
    }
}

pub struct ConvertText;

impl Command for ConvertText {
    type Params = ConvertTextParams;
    type Output = ConvertedText;
    const ID: &'static str = "editor.convert.text";
    const TITLE: &'static str = "Convert Code";
    const DESCRIPTION: &'static str = "Converts code from the language of the extension `from` into the language of `to` \
         (such as `java` into `kt`) and answers the converted `text` with the `problems` to check (lines and columns \
         of the original text, from 1). It takes a whole file, the members of a class or statements, and changes \
         nothing in the project.";

    fn run(&self, session: &mut Session, params: ConvertTextParams) -> Result<ConvertedText, CommandError> {
        convert_text(session, &params.from, &params.to, &params.text)
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ConvertFile)?;
    registry.register(ConvertText)
}
