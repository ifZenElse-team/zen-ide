use std::sync::Arc;

use zen_syntax::{
    ColorSpan, Conversion, Expansion, Feature, JvmFile, KeyReferences, LineFold, OutlineItem, Outliner, RunOffer,
    SchemaDeclaration, Suggestions, TextFeatures, TextInlay, TextProblem, ToolRow, ToolWindowDeclaration,
};

use crate::{
    convert_input, decode_colors, decode_conversion, decode_converts_from, decode_expansion, decode_folds,
    decode_inlays, decode_jvm, decode_key_references, decode_outline, decode_problems, decode_run_targets,
    decode_suggestions, decode_tool_rows, decode_triggers, resource_path, supports, with_caret, with_path, with_typed,
};

pub type Export = fn(&[u8]) -> Vec<u8>;

#[derive(Clone, Copy, Debug, Default)]
pub struct Exports {
    pub fold: Option<Export>,
    pub outline: Option<Export>,
    pub complete: Option<Export>,
    pub expand: Option<Export>,
    pub colors: Option<Export>,
    pub on_type: Option<Export>,
    pub triggers: Option<Export>,
    pub problems: Option<Export>,
    pub dialect: Option<Export>,
    pub jvm: Option<Export>,
    pub inlays: Option<Export>,
    pub key_references: Option<Export>,
    pub resource: Option<Export>,
    pub run_targets: Option<Export>,
    pub tool_rows: Option<Export>,
    pub index: Option<Export>,
    pub convert: Option<Export>,
    pub convert_from: Option<Export>,
}

#[derive(Clone, Debug)]
pub struct NativePlugin {
    extensions: Vec<String>,
    exports: Exports,
    dialect: String,
    id: String,
    schemas: Vec<SchemaDeclaration>,
    runs: Vec<String>,
    person_only: Vec<crate::PersonOnly>,
    target_files: Vec<String>,
    tool_windows: Vec<ToolWindowDeclaration>,
    index: Option<crate::IndexDeclaration>,
}

impl NativePlugin {
    pub fn new(extensions: &[&str], exports: Exports) -> Self {
        Self {
            extensions: extensions.iter().map(|extension| extension.to_ascii_lowercase()).collect(),
            exports,
            dialect: String::new(),
            id: String::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
        }
    }

    pub fn with_index(mut self, id: &str, index: crate::IndexDeclaration) -> Self {
        self.id = id.to_owned();
        self.index = Some(index);
        self
    }

    pub fn with_tool_windows(mut self, windows: Vec<ToolWindowDeclaration>) -> Self {
        self.tool_windows = windows;
        self
    }

    pub fn with_schemas(mut self, id: &str, schemas: Vec<SchemaDeclaration>) -> Self {
        self.id = id.to_owned();
        self.schemas = schemas;
        self
    }

    pub fn with_runs(mut self, id: &str, runs: &[&str]) -> Self {
        self.id = id.to_owned();
        self.runs = runs.iter().map(|program| (*program).to_owned()).collect();
        self
    }

    pub fn with_person_only(mut self, person_only: &[crate::PersonOnly]) -> Self {
        self.person_only = person_only.to_vec();
        self
    }

    pub fn with_target_files(mut self, patterns: &[&str]) -> Self {
        self.target_files = patterns.iter().map(|pattern| (*pattern).to_owned()).collect();
        self
    }
}

impl Outliner for NativePlugin {
    fn handles(&self, file_name: &str) -> bool {
        supports(&self.extensions, &[], file_name)
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        let Some(outline) = self.exports.outline else { return Vec::new() };
        self.tell();
        decode_outline(&outline(text.as_bytes()), text).expect("the plug-in's outline is valid")
    }
}

impl TextFeatures for NativePlugin {
    fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Folding => self.exports.fold.is_some(),
            Feature::Outline => self.exports.outline.is_some(),
            Feature::Completion => self.exports.complete.is_some(),
            Feature::Expansion => self.exports.expand.is_some(),
            Feature::Colors => self.exports.colors.is_some(),
            Feature::Typing => self.exports.on_type.is_some() && !self.triggers().is_empty(),
            Feature::Problems => self.exports.problems.is_some(),
            Feature::Jvm => self.exports.jvm.is_some(),
            Feature::Inlays => self.exports.inlays.is_some(),
            Feature::Resources => self.exports.resource.is_some(),
            Feature::RunTargets => self.exports.run_targets.is_some(),
            Feature::Conversion => self.exports.convert.is_some() && !self.converts_from().is_empty(),
        }
    }

    fn folds(&self, text: &str) -> Vec<LineFold> {
        let Some(fold) = self.exports.fold else { return Vec::new() };
        self.tell();
        decode_folds(&fold(text.as_bytes()), text.split('\n').count()).expect("the plug-in's folds are valid")
    }

    fn complete(&self, text: &str, caret: usize) -> Option<Suggestions> {
        let complete = self.exports.complete?;
        self.tell();
        let input = with_caret(text, caret)?;
        decode_suggestions(&complete(&input), text, caret).expect("the plug-in's suggestions are valid")
    }

    fn expand(&self, text: &str, caret: usize) -> Option<Expansion> {
        let expand = self.exports.expand?;
        self.tell();
        let input = with_caret(text, caret)?;
        decode_expansion(&expand(&input), text, caret).expect("the plug-in's expansion is valid")
    }

    fn colors(&self, text: &str) -> Vec<ColorSpan> {
        let Some(colors) = self.exports.colors else { return Vec::new() };
        self.tell();
        decode_colors(&colors(text.as_bytes()), text).expect("the plug-in's colours are valid")
    }

    fn problems(&self, text: &str) -> Vec<TextProblem> {
        let Some(problems) = self.exports.problems else { return Vec::new() };
        self.tell();
        decode_problems(&problems(text.as_bytes()), text).expect("the plug-in's problems are valid")
    }

    fn reacts_to(&self, typed: char) -> bool {
        self.exports.on_type.is_some() && self.triggers().contains(&typed)
    }

    fn on_type(&self, text: &str, caret: usize, typed: char) -> Option<Expansion> {
        let on_type = self.exports.on_type.filter(|_| self.reacts_to(typed))?;
        let input = with_typed(text, caret, typed)?;
        self.tell();
        decode_expansion(&on_type(&input), text, caret).expect("the plug-in's answer to typing is valid")
    }

    fn key_references(&self) -> Vec<KeyReferences> {
        self.exports
            .key_references
            .map(|declared| decode_key_references(&declared(&[])).expect("the plug-in's key references are valid"))
            .unwrap_or_default()
    }

    fn inlays(&self, text: &str) -> Vec<TextInlay> {
        let Some(inlays) = self.exports.inlays else { return Vec::new() };
        self.tell();
        decode_inlays(&inlays(text.as_bytes()), text).expect("the plug-in's inlays are valid")
    }

    fn plugin_id(&self) -> Option<&str> {
        Some(self.id.as_str()).filter(|id| !id.is_empty())
    }

    fn schemas(&self) -> Vec<SchemaDeclaration> {
        self.schemas.clone()
    }

    fn resource(&self, path: &str) -> Option<Vec<u8>> {
        let resource = self.exports.resource.filter(|_| resource_path(path))?;
        Some(resource(path.as_bytes())).filter(|bytes| !bytes.is_empty())
    }

    fn run_targets(&self, path: &str, text: &str) -> Vec<RunOffer> {
        let (Some(run_targets), Some(input)) = (self.exports.run_targets, with_path(path, text)) else {
            return Vec::new();
        };
        decode_run_targets(&run_targets(&input), text, &self.runs).expect("the plug-in's run targets are valid")
    }

    fn runs(&self) -> Vec<String> {
        self.runs.clone()
    }

    fn person_only(&self) -> Vec<crate::PersonOnly> {
        self.person_only.clone()
    }

    fn target_files(&self) -> Vec<String> {
        self.target_files.clone()
    }

    fn tool_windows(&self) -> Vec<ToolWindowDeclaration> {
        self.tool_windows.clone()
    }

    fn index_declaration(&self) -> Option<crate::IndexDeclaration> {
        self.index.clone()
    }

    fn index(&self, input: &[u8]) -> Option<crate::ProjectIndex> {
        Some(crate::decode_index(&(self.exports.index?)(input)).expect("the plug-in's index is valid"))
    }

    fn tool_rows(&self, section: &str, output: &str) -> Option<Vec<ToolRow>> {
        let (tool_rows, input) = (self.exports.tool_rows?, with_path(section, output)?);
        Some(decode_tool_rows(&tool_rows(&input)).expect("the plug-in's rows are valid"))
    }

    fn jvm(&self, text: &str) -> Option<JvmFile> {
        let jvm = self.exports.jvm?;
        decode_jvm(&jvm(text.as_bytes()), text).expect("the plug-in's JVM answer is valid")
    }

    fn converts_from(&self) -> Vec<String> {
        self.exports.convert_from.map(|declared| decode_converts_from(&declared(&[]))).unwrap_or_default()
    }

    fn convert(&self, from: &str, text: &str) -> Option<Conversion> {
        let convert = self.exports.convert?;
        if !self.converts_from().iter().any(|known| known == from) {
            return None;
        }
        decode_conversion(&convert(&convert_input(from, text)?), text).expect("the plug-in's conversion is valid")
    }

    fn in_dialect(&self, dialect: &str) -> Option<Arc<dyn TextFeatures>> {
        self.exports.dialect?;
        Some(Arc::new(Self { dialect: dialect.to_owned(), ..self.clone() }))
    }
}

impl NativePlugin {
    fn tell(&self) {
        if let Some(dialect) = self.exports.dialect {
            dialect(self.dialect.as_bytes());
        }
    }

    fn triggers(&self) -> Vec<char> {
        self.exports
            .triggers
            .map(|triggers| decode_triggers(&triggers(&[])).expect("the plug-in's triggers are valid"))
            .unwrap_or_default()
    }
}
