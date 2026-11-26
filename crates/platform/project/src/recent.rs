use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const LIMIT: usize = 30;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: PathBuf,
    pub opened: u64,
    pub open: bool,
}

impl RecentProject {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.path.display().to_string(), |name| name.to_string_lossy().into_owned())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProjects {
    projects: Vec<RecentProject>,
}

impl RecentProjects {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), zen_vfs::VfsError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        zen_vfs::write_atomic(path, &bytes)
    }

    pub fn projects(&self) -> &[RecentProject] {
        &self.projects
    }

    pub fn to_reopen(&self) -> Vec<PathBuf> {
        self.projects
            .iter()
            .filter(|project| project.open && project.path.is_dir())
            .map(|project| project.path.clone())
            .collect()
    }

    pub fn opened(&mut self, path: &Path, now: u64) {
        self.projects.retain(|project| project.path != path);
        self.projects.insert(0, RecentProject { path: path.to_owned(), opened: now, open: true });
        self.projects.truncate(LIMIT);
    }

    pub fn closed(&mut self, path: &Path) {
        if let Some(project) = self.projects.iter_mut().find(|project| project.path == path) {
            project.open = false;
        }
    }

    pub fn remove(&mut self, path: &Path) {
        self.projects.retain(|project| project.path != path);
    }
}

pub fn default_file(app_id: &str) -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let base = if let Some(base) = env("ZEN_DATA_DIR") {
        Some(base)
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|home| home.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|home| home.join(".local/share")))
    };
    base.map(|base| file_in(&base, app_id))
}

pub fn file_in(data_dir: &Path, app_id: &str) -> PathBuf {
    data_dir.join(app_id).join("recent-projects.json")
}

pub fn read(app_id: &str) -> RecentProjects {
    default_file(app_id).map(|file| RecentProjects::load(&file)).unwrap_or_default()
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_order_and_open_projects() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut recent = RecentProjects::default();
        recent.opened(&a, 1);
        recent.opened(&b, 2);
        recent.opened(&a, 3);
        assert_eq!(recent.projects().iter().map(RecentProject::name).collect::<Vec<_>>(), ["a", "b"]);
        recent.closed(&b);
        assert_eq!(recent.to_reopen(), vec![a.clone()]);

        let file = dir.path().join("state/recent-projects.json");
        recent.save(&file).unwrap();
        assert_eq!(RecentProjects::load(&file), recent);
        assert_eq!(RecentProjects::load(&dir.path().join("missing.json")), RecentProjects::default());
    }

    #[test]
    fn corrupt_files_read_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("recent-projects.json");
        std::fs::write(&file, b"{\"projects\": [{\"path\": ").unwrap();
        assert_eq!(RecentProjects::load(&file), RecentProjects::default());
        std::fs::write(&file, b"").unwrap();
        assert_eq!(RecentProjects::load(&file), RecentProjects::default());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = br#"{"version": 2, "projects": [{"path": "/a", "opened": 7, "open": false, "ide": "x"}]}"#;
        let recent: RecentProjects = serde_json::from_slice(json).unwrap();
        assert_eq!(recent.projects(), [RecentProject { path: PathBuf::from("/a"), opened: 7, open: false }]);
    }

    #[test]
    fn the_documented_example_is_the_format() {
        let doc = include_str!("../../../../docs/recent-projects.md");
        let start = doc.find("```json\n").unwrap() + "```json\n".len();
        let example = &doc[start..start + doc[start..].find("```").unwrap()];
        let recent: RecentProjects = serde_json::from_str(example).unwrap();
        assert_eq!(
            recent.projects(),
            [
                RecentProject { path: PathBuf::from("/Users/ada/src/garden"), opened: 1_792_224_000, open: true },
                RecentProject { path: PathBuf::from("/Users/ada/src/notes"), opened: 1_792_137_600, open: false },
            ]
        );
        assert_eq!(serde_json::to_string_pretty(&recent).unwrap(), example.trim_end());
    }

    #[test]
    fn the_file_is_named_after_the_application_id() {
        assert_eq!(
            file_in(Path::new("/data"), "space.ifzenelse.gogarden"),
            Path::new("/data/space.ifzenelse.gogarden/recent-projects.json")
        );
    }

    #[test]
    fn missing_folders_are_not_reopened() {
        let mut recent = RecentProjects::default();
        recent.opened(Path::new("/no/such/project"), 1);
        assert!(recent.to_reopen().is_empty());
        assert_eq!(recent.projects().len(), 1);
    }
}
