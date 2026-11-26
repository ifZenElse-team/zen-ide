use std::path::{Path, PathBuf};
use std::sync::Arc;

pub trait FileReferences: Send + Sync {
    fn handles(&self, file_name: &str) -> bool;

    fn files_at(&self, path: &Path, text: &str, offset: usize) -> Option<Vec<PathBuf>>;
}

#[derive(Clone, Default)]
pub struct FileReferenceSet(Vec<Arc<dyn FileReferences>>);

impl FileReferenceSet {
    pub fn add(&mut self, references: Arc<dyn FileReferences>) {
        self.0.push(references);
    }

    pub fn files_at(&self, path: &Path, text: &str, offset: usize) -> Option<Vec<PathBuf>> {
        let name = path.file_name()?.to_string_lossy();
        self.0
            .iter()
            .filter(|references| references.handles(&name))
            .find_map(|references| references.files_at(path, text, offset))
    }
}
