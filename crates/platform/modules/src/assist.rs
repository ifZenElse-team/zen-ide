use std::path::{Path, PathBuf};
use std::sync::Arc;

use zen_syntax::Suggestions;

pub struct AssistContext<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub offset: usize,
}

pub trait LanguageAssist: Send + Sync {
    fn handles(&self, file_name: &str) -> bool;

    fn complete(&self, _context: &AssistContext<'_>) -> Option<Suggestions> {
        None
    }

    fn declaration(&self, _context: &AssistContext<'_>) -> Option<Result<PathBuf, String>> {
        None
    }
}

#[derive(Clone, Default)]
pub struct LanguageAssists {
    assists: Vec<Arc<dyn LanguageAssist>>,
}

impl LanguageAssists {
    pub fn add(&mut self, assist: Arc<dyn LanguageAssist>) {
        self.assists.push(assist);
    }

    fn serving<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Arc<dyn LanguageAssist>> {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.assists.iter().filter(move |assist| assist.handles(&name))
    }

    pub fn serves(&self, path: &Path) -> bool {
        self.serving(path).next().is_some()
    }

    pub fn complete(&self, context: &AssistContext<'_>) -> Option<Suggestions> {
        self.serving(context.path).find_map(|assist| assist.complete(context))
    }

    pub fn declaration(&self, context: &AssistContext<'_>) -> Option<Result<PathBuf, String>> {
        self.serving(context.path).find_map(|assist| assist.declaration(context))
    }
}
