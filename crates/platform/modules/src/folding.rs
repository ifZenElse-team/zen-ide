pub const FILE_HEADER: &str = "editor.folding.fileHeader";
pub const IMPORTS: &str = "editor.folding.imports";
pub const DOC_COMMENTS: &str = "editor.folding.docComments";
pub const METHOD_BODIES: &str = "editor.folding.methodBodies";
pub const CUSTOM_REGIONS: &str = "editor.folding.customRegions";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldOption {
    pub key: &'static str,
    pub label: &'static str,
    pub default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldingOptions {
    pub title: &'static str,
    pub options: &'static [FoldOption],
}

#[derive(Clone, Debug, Default)]
pub struct FoldingGroups(pub Vec<FoldingOptions>);

impl FoldingGroups {
    pub fn option(&self, key: &str) -> Option<FoldOption> {
        self.0.iter().flat_map(|group| group.options.iter()).find(|option| option.key == key).copied()
    }
}
