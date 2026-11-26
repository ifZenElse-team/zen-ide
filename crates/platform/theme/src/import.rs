use std::fmt;

use crate::Theme;

mod textmate;
pub mod vscode;

pub trait ThemeImporter: Send + Sync {
    fn format_name(&self) -> &'static str;

    fn detects(&self, text: &str) -> bool;

    fn import(&self, text: &str) -> Result<ImportReport, ImportError>;

    fn detects_at(&self, _path: &str, text: &str) -> bool {
        self.detects(text)
    }

    fn import_at(
        &self,
        _path: &str,
        text: &str,
        _read: &dyn Fn(&str) -> Option<String>,
    ) -> Result<ImportReport, ImportError> {
        self.import(text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub theme: Theme,
    pub mapped: usize,
    pub unmapped: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError(pub String);

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

impl From<crate::FormatError> for ImportError {
    fn from(error: crate::FormatError) -> ImportError {
        ImportError(error.to_string())
    }
}
