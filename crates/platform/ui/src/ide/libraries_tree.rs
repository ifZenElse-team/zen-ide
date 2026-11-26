use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Ui, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::model::{Library, LibraryKind, Module};
use zen_workbench::project_model::ProjectModels;

use super::{IdeView, TreeFrame, metrics};
use crate::theme;

const RECHECK: f64 = 5.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RowKind {
    #[default]
    Disk,
    Group,
    Missing,
    Bundle,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RowMode<'a> {
    pub(super) excluded: bool,
    pub(super) read_only: bool,
    pub(super) label: Option<&'a str>,
    pub(super) detail: Option<&'a str>,
    pub(super) kind: RowKind,
    pub(super) module: bool,
    pub(super) scratch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LibraryRow {
    pub(super) label: String,
    pub(super) root: PathBuf,
    pub(super) missing: bool,
}

impl LibraryRow {
    pub(super) fn tree_path(&self, project: &Path) -> PathBuf {
        if self.root.as_os_str().is_empty() {
            group_path(project, &self.label.replace(['/', '\\'], " "))
        } else {
            self.root.clone()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Libraries {
    pub(super) revision: u64,
    pub(super) sdks: Vec<LibraryRow>,
    pub(super) modules_label: String,
    pub(super) module_name: Option<String>,
    pub(super) dependencies: Vec<LibraryRow>,
    pub(super) module_roots: Vec<(PathBuf, String)>,
    pub(super) flat: bool,
    pub(super) read_at: f64,
}

impl Libraries {
    pub(super) fn build(revision: u64, libraries: &[Library], modules: &[Module], project: &Path) -> Self {
        let row = |library: &Library, label: String| LibraryRow {
            label,
            root: library.root.clone(),
            missing: !library.exists,
        };
        let sdks: Vec<&Library> = libraries.iter().filter(|library| library.kind == LibraryKind::Sdk).collect();
        let language = sdks.first().map(|sdk| sdk.name.clone()).unwrap_or_default();
        let dependencies = || libraries.iter().filter(|library| library.kind == LibraryKind::Dependency);
        let group = dependencies()
            .find_map(|library| library.group.clone())
            .filter(|_| dependencies().all(|library| library.group.is_some()));
        let flat = group.as_deref() == Some("");
        let mut dependencies: Vec<LibraryRow> = libraries
            .iter()
            .filter(|library| library.kind == LibraryKind::Dependency)
            .map(|library| {
                let label = match (&library.label, &library.version) {
                    (Some(label), _) => label.clone(),
                    (None, Some(version)) => format!("{}@{version}", library.name),
                    (None, None) => library.name.clone(),
                };
                row(library, label)
            })
            .collect();
        dependencies.sort_by(|a, b| a.label.cmp(&b.label));
        dependencies.dedup_by(|a, b| a.label == b.label && a.root == b.root);
        let module_name = modules
            .iter()
            .find(|module| module.root == project)
            .or_else(|| modules.first())
            .map(|module| module.name.clone())
            .filter(|name| !name.is_empty() && group.is_none());
        Self {
            revision,
            sdks: sdks
                .into_iter()
                .map(|sdk| {
                    let version = sdk.version.clone().unwrap_or_default();
                    let label = sdk.label.clone().unwrap_or_else(|| {
                        t!("ide-libraries-sdk", name = sdk.name.as_str(), version = version).trim().to_owned()
                    });
                    row(sdk, label)
                })
                .collect(),
            modules_label: group.unwrap_or_else(|| t!("ide-libraries-modules", language = language).trim().to_owned()),
            module_name,
            dependencies,
            module_roots: modules
                .iter()
                .filter(|module| module.root != project && !module.name.is_empty())
                .map(|module| (module.root.clone(), module.name.clone()))
                .collect(),
            flat,
            read_at: 0.0,
        }
    }

    pub(super) fn module_at(&self, path: &Path) -> Option<&str> {
        self.module_roots.iter().find(|(root, _)| root == path).map(|(_, name)| name.as_str())
    }

    pub(super) fn has_missing(&self) -> bool {
        self.sdks.iter().chain(&self.dependencies).any(|row| row.missing)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.sdks.is_empty() && self.dependencies.is_empty()
    }

    pub(super) fn is_read_only(&self, project: &Path, path: &Path) -> bool {
        is_group(path) || self.library_of(project, path).is_some()
    }

    pub(super) fn library_of(&self, project: &Path, path: &Path) -> Option<&LibraryRow> {
        self.sdks.iter().chain(&self.dependencies).find(|row| {
            !row.root.as_os_str().is_empty() && !row.root.starts_with(project) && path.starts_with(&row.root)
        })
    }

    pub(super) fn shown_path(&self, project: &Path, path: &Path) -> Option<PathBuf> {
        if self.library_of(project, path).is_some() {
            return Some(path.to_owned());
        }
        let real = path.canonicalize().ok()?;
        self.sdks
            .iter()
            .chain(&self.dependencies)
            .filter(|row| !row.root.as_os_str().is_empty() && !row.root.starts_with(project))
            .find_map(|row| {
                let root = row.root.canonicalize().ok()?;
                real.strip_prefix(&root).ok().map(|rest| row.root.join(rest))
            })
    }
}

pub(super) fn group_path(project: &Path, group: &str) -> PathBuf {
    project.join(format!("\0{group}"))
}

pub(super) fn is_group(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('\0'))
}

fn fetch(registry: &Registry, session: &mut Session, project: &Path) -> Libraries {
    let answer = registry.dispatch(session, "project.libraries.list", serde_json::json!({})).unwrap_or_default();
    let revision = answer["revision"].as_u64().unwrap_or_default();
    let libraries: Vec<Library> = serde_json::from_value(answer["libraries"].clone()).unwrap_or_default();
    let info =
        registry.dispatch(session, "project.module.info", serde_json::json!({ "all": true })).unwrap_or_default();
    let modules: Vec<Module> = serde_json::from_value(info["modules"].clone()).unwrap_or_default();
    Libraries::build(revision, &libraries, &modules, project)
}

impl IdeView {
    pub(super) fn refresh_libraries(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        let now = ctx.input(|input| input.time);
        let revision = session.state::<ProjectModels>().map_or(0, ProjectModels::revision);
        let stale = self
            .libraries
            .as_ref()
            .is_none_or(|libraries| libraries.revision != revision || now - libraries.read_at > RECHECK);
        if stale {
            self.libraries = Some(Libraries { read_at: now, ..fetch(registry, session, project) });
        }
        if self.libraries.as_ref().is_some_and(Libraries::has_missing) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(RECHECK));
        }
    }

    pub(super) fn is_library_path(&self, project: &Path, path: &Path) -> bool {
        self.libraries.as_ref().is_some_and(|libraries| libraries.is_read_only(project, path))
    }

    pub(super) fn reveal_library_path(&mut self, project: &Path, path: &Path) -> bool {
        let Some(libraries) = &self.libraries else { return false };
        let Some(row) = libraries.library_of(project, path) else { return false };
        let in_modules = libraries.dependencies.contains(row) && !libraries.flat;
        let root = row.root.clone();
        self.expanded.insert(group_path(project, "libraries"));
        if in_modules {
            self.expanded.insert(group_path(project, "modules"));
        }
        self.expanded
            .extend(path.ancestors().skip(1).take_while(|folder| folder.starts_with(&root)).map(Path::to_path_buf));
        true
    }

    pub(super) fn libraries_rows(&mut self, ui: &mut Ui, project: &Path, active: Option<&Path>, frame: &mut TreeFrame) {
        let Some(libraries) = self.libraries.clone() else { return };
        if libraries.is_empty() {
            return;
        }
        let title = t!("ide-libraries-title");
        let top = group_path(project, "libraries");
        let group = RowMode { read_only: true, label: Some(&title), kind: RowKind::Group, ..RowMode::default() };
        self.tree_row(ui, &entry(&top, &title), 0, active, frame, group);
        if !self.expanded.contains(&top) {
            return;
        }
        for sdk in &libraries.sdks {
            self.library_row(ui, project, sdk, 1, active, frame);
        }
        if libraries.dependencies.is_empty() {
            return;
        }
        if libraries.flat {
            for dependency in &libraries.dependencies {
                self.library_row(ui, project, dependency, 1, active, frame);
            }
            return;
        }
        let modules = group_path(project, "modules");
        let group = RowMode {
            read_only: true,
            label: Some(&libraries.modules_label),
            detail: libraries.module_name.as_deref(),
            kind: RowKind::Group,
            ..RowMode::default()
        };
        self.tree_row(ui, &entry(&modules, &libraries.modules_label), 1, active, frame, group);
        if !self.expanded.contains(&modules) {
            return;
        }
        for dependency in &libraries.dependencies {
            self.library_row(ui, project, dependency, 2, active, frame);
        }
    }

    fn library_row(
        &mut self,
        ui: &mut Ui,
        project: &Path,
        row: &LibraryRow,
        depth: usize,
        active: Option<&Path>,
        frame: &mut TreeFrame,
    ) {
        let kind = if row.missing { RowKind::Missing } else { RowKind::Disk };
        let mode = RowMode { read_only: true, label: Some(&row.label), kind, ..RowMode::default() };
        self.tree_row(ui, &entry(&row.tree_path(project), &row.label), depth, active, frame, mode);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LibraryBanner {
    pub(super) text: String,
    pub(super) download: bool,
    pub(super) downloading: bool,
    pub(super) open: Option<PathBuf>,
}

impl LibraryBanner {
    pub(super) fn of(answer: &serde_json::Value) -> Option<Self> {
        if answer["kind"] != "decompiled" {
            return None;
        }
        let text = match (answer["bytecode"].as_u64(), answer["java"].as_u64()) {
            (Some(bytecode), Some(java)) => {
                t!("ide-library-decompiled-version", bytecode = format!("{bytecode}.0"), java = java)
            }
            _ => t!("ide-library-decompiled"),
        };
        let downloading = answer["downloading"].as_bool().unwrap_or_default();
        let has_library = answer["library"].is_string();
        let found = answer["sourcesFound"].as_bool().unwrap_or_default();
        let open = answer["classPath"].as_str().filter(|_| found && !downloading).map(PathBuf::from);
        Some(Self { text, download: has_library && !found && !downloading, downloading, open })
    }
}

impl IdeView {
    pub(super) fn library_banner(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, path: &Path) {
        if zen_workbench::virtual_documents::get(path).is_none() {
            return;
        }
        let answer = registry
            .dispatch(session, "java.libraries.document", serde_json::json!({ "path": path }))
            .unwrap_or_default();
        let Some(banner) = LibraryBanner::of(&answer) else { return };
        if banner.downloading {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::BANNER), Sense::hover());
        let band =
            Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom() - 4.0));
        ui.painter().rect_filled(band, 6, p.banner);
        let y = band.center().y;
        let text = ui.painter().with_clip_rect(band.shrink(4.0)).text(
            pos2(band.left() + 14.0, y),
            Align2::LEFT_CENTER,
            &banner.text,
            theme::regular(13.0),
            p.text,
        );
        crate::accessibility::announce(ui, ui.scope_id().with("library-banner"), text, || banner.text.clone());
        let mut x = text.right() + 20.0;
        if banner.downloading {
            ui.painter().text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                t!("ide-library-downloading-sources"),
                theme::regular(13.0),
                p.text_secondary,
            );
        }
        if banner.download && self.link(ui, &mut x, y, &t!("ide-library-download-sources"), "library-download") {
            let params = serde_json::json!({ "path": path });
            self.dispatch(ui, registry, session, "java.libraries.downloadSources", params);
        }
        if let Some(class) = &banner.open
            && self.link(ui, &mut x, y, &t!("ide-library-open-sources"), "library-open")
        {
            self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": class }));
        }
    }
}

fn entry(path: &Path, label: &str) -> zen_vfs::Entry {
    let name = path.file_name().map_or_else(|| label.to_owned(), |name| name.to_string_lossy().into_owned());
    zen_vfs::Entry { path: path.to_owned(), name, is_dir: true, is_symlink: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(kind: LibraryKind, name: &str, version: Option<&str>, root: &str, exists: bool) -> Library {
        Library {
            kind,
            name: name.to_owned(),
            version: version.map(str::to_owned),
            root: PathBuf::from(root),
            exists,
            group: None,
            label: None,
        }
    }

    fn sample() -> Libraries {
        let libraries = vec![
            library(
                LibraryKind::Dependency,
                "golang.org/x/text",
                Some("v0.14.0"),
                "/cache/golang.org/x/text@v0.14.0",
                true,
            ),
            library(LibraryKind::Sdk, "Go", Some("1.22.3"), "/sdk/go", true),
            library(
                LibraryKind::Dependency,
                "example.com/gone",
                Some("v1.0.0"),
                "/cache/example.com/gone@v1.0.0",
                false,
            ),
            library(
                LibraryKind::Dependency,
                "golang.org/x/text",
                Some("v0.14.0"),
                "/cache/golang.org/x/text@v0.14.0",
                true,
            ),
        ];
        let modules = vec![
            Module { name: "example.com/tool".to_owned(), root: PathBuf::from("/p/tool"), ..Module::default() },
            Module { name: "example.com/app".to_owned(), root: PathBuf::from("/p"), ..Module::default() },
        ];
        Libraries::build(7, &libraries, &modules, Path::new("/p"))
    }

    #[test]
    fn the_rows_have_their_labels() {
        let libraries = sample();
        assert_eq!(libraries.revision, 7);
        assert_eq!(libraries.sdks.len(), 1);
        assert_eq!(libraries.sdks[0].label, t!("ide-libraries-sdk", name = "Go", version = "1.22.3"));
        assert_eq!(libraries.sdks[0].root, PathBuf::from("/sdk/go"));
        assert_eq!(libraries.modules_label, t!("ide-libraries-modules", language = "Go"));
        assert_eq!(libraries.module_name.as_deref(), Some("example.com/app"));
        let labels: Vec<&str> = libraries.dependencies.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["example.com/gone@v1.0.0", "golang.org/x/text@v0.14.0"]);
    }

    #[test]
    fn modules_inside_the_project_are_named_and_dependencies_take_their_providers_group() {
        let libraries = sample();
        assert_eq!(libraries.module_at(Path::new("/p/tool")), Some("example.com/tool"));
        assert_eq!(libraries.module_at(Path::new("/p")), None, "the project's own row has its own look");
        assert_eq!(libraries.module_at(Path::new("/p/other")), None);

        let mut crate_row = library(LibraryKind::Dependency, "serde", Some("1.0.228"), "/cargo/serde-1.0.228", true);
        crate_row.group = Some("Crates".to_owned());
        let rust = library(LibraryKind::Sdk, "Rust", Some("1.99.1"), "/rust/library", true);
        let modules = vec![Module { name: "zen-core".to_owned(), root: PathBuf::from("/p/core"), ..Module::default() }];
        let libraries = Libraries::build(2, &[rust, crate_row], &modules, Path::new("/p"));
        assert_eq!(libraries.modules_label, "Crates");
        assert_eq!(libraries.module_name, None, "no member's name after Crates");
        assert_eq!(libraries.module_at(Path::new("/p/core")), Some("zen-core"));
        assert_eq!(libraries.dependencies[0].label, "serde@1.0.228");
    }

    #[test]
    fn a_library_not_downloaded_is_missing() {
        let libraries = sample();
        assert!(libraries.dependencies[0].missing);
        assert!(!libraries.dependencies[1].missing);
        assert!(!libraries.sdks[0].missing);
        assert!(libraries.has_missing());
        assert!(!Libraries::build(1, &[], &[], Path::new("/p")).has_missing());
    }

    #[test]
    fn everything_under_external_libraries_is_read_only() {
        let libraries = sample();
        let project = Path::new("/p");
        assert!(libraries.is_read_only(project, &group_path(project, "libraries")));
        assert!(libraries.is_read_only(project, &group_path(project, "modules")));
        assert!(libraries.is_read_only(project, Path::new("/sdk/go/src/fmt/print.go")));
        assert!(libraries.is_read_only(project, Path::new("/cache/golang.org/x/text@v0.14.0/go.mod")));
        assert!(!libraries.is_read_only(project, Path::new("/p/main.go")));
        assert!(!libraries.is_read_only(project, Path::new("/sdk/gopher")));
    }

    #[test]
    fn libraries_without_a_folder_or_inside_the_project_leave_the_project_writable() {
        let libraries = vec![
            library(LibraryKind::Dependency, "example.com/a", Some("v1.0.0"), "", false),
            library(LibraryKind::Dependency, "example.com/b", Some("v1.0.0"), "", false),
            library(LibraryKind::Dependency, "example.com/fork", None, "/p/fork", true),
        ];
        let libraries = Libraries::build(1, &libraries, &[], Path::new("/p"));
        assert_eq!(libraries.dependencies.len(), 3, "rows without a folder are not merged");
        let project = Path::new("/p");
        assert!(!libraries.is_read_only(project, Path::new("/p/main.go")));
        assert!(!libraries.is_read_only(project, Path::new("/p/fork/fork.go")));
        assert!(libraries.library_of(project, Path::new("/p/fork/fork.go")).is_none());
        assert!(is_group(&libraries.dependencies[0].tree_path(project)));
        assert_ne!(libraries.dependencies[0].tree_path(project), libraries.dependencies[1].tree_path(project));
        assert_eq!(libraries.dependencies[2].tree_path(project), PathBuf::from("/p/fork"));
    }

    #[cfg(unix)]
    #[test]
    fn a_library_file_reached_through_its_real_folder_is_shown_under_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap().join("Cellar/go/1.27.1/libexec/src");
        std::fs::create_dir_all(real.join("fmt")).unwrap();
        std::fs::write(real.join("fmt/print.go"), "package fmt\n").unwrap();
        let stable = dir.path().join("opt-go");
        std::os::unix::fs::symlink(real.parent().unwrap(), &stable).unwrap();
        let root = stable.join("src");
        let sdk = library(LibraryKind::Sdk, "Go", Some("1.27.1"), root.to_str().unwrap(), true);
        let project = Path::new("/p");
        let libraries = Libraries::build(1, &[sdk], &[], project);
        assert_eq!(libraries.shown_path(project, &root.join("fmt/print.go")), Some(root.join("fmt/print.go")));
        assert_eq!(libraries.shown_path(project, &real.join("fmt/print.go")), Some(root.join("fmt/print.go")));
        assert_eq!(libraries.shown_path(project, &dir.path().join("elsewhere.go")), None);
    }

    #[test]
    fn group_paths_are_not_files() {
        let group = group_path(Path::new("/p"), "libraries");
        assert!(is_group(&group));
        assert!(!group.exists());
        assert!(!is_group(Path::new("/p/libraries")));
    }

    #[test]
    fn nothing_to_show_without_libraries() {
        let libraries = Libraries::build(1, &[], &[], Path::new("/p"));
        assert!(libraries.is_empty());
        assert!(libraries.module_name.is_none());
        assert_eq!(libraries.modules_label, t!("ide-libraries-modules", language = "").trim());
    }

    #[test]
    fn libraries_with_their_own_labels_and_an_empty_group_sit_right_under_external_libraries() {
        let mut jdk = library(LibraryKind::Sdk, "JDK", Some("21.0.5"), "/cache/jdk", true);
        jdk.label = Some("< 21.0.5 >".to_owned());
        let mut lib = library(LibraryKind::Dependency, "org.slf4j:slf4j-api", Some("2.0.13"), "/cache/slf4j", true);
        lib.label = Some("Maven: org.slf4j:slf4j-api:2.0.13".to_owned());
        lib.group = Some(String::new());
        let libraries = Libraries::build(1, &[lib, jdk], &[], Path::new("/p"));
        assert!(libraries.flat);
        assert_eq!(libraries.sdks[0].label, "< 21.0.5 >");
        assert_eq!(libraries.dependencies[0].label, "Maven: org.slf4j:slf4j-api:2.0.13");
        assert!(libraries.is_read_only(Path::new("/p"), Path::new("/cache/slf4j/org/slf4j/Logger.class")));
        assert!(!sample().flat);
    }

    #[test]
    fn the_banner_of_a_decompiled_class_offers_its_sources_download_then_the_sources() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let answer = serde_json::json!({
            "kind": "decompiled", "origin": "org.demo.Box", "library": "/m2/lib.jar", "bytecode": 65, "java": 21,
            "classPath": "/cache/lib/org/demo/Box.class", "downloading": false, "sourcesFound": false
        });
        let banner = LibraryBanner::of(&answer).unwrap();
        assert_eq!(banner.text, t!("ide-library-decompiled-version", bytecode = "65.0", java = 21));
        assert!(banner.download && !banner.downloading && banner.open.is_none());
        let mut downloading = answer.clone();
        downloading["downloading"] = serde_json::json!(true);
        let banner = LibraryBanner::of(&downloading).unwrap();
        assert!(!banner.download && banner.downloading);
        let mut found = answer.clone();
        found["sourcesFound"] = serde_json::json!(true);
        let banner = LibraryBanner::of(&found).unwrap();
        assert_eq!(banner.open, Some(PathBuf::from("/cache/lib/org/demo/Box.class")));
        assert!(!banner.download);
        let server = serde_json::json!({ "kind": "decompiled", "origin": "jdt://x", "downloading": false });
        let banner = LibraryBanner::of(&server).unwrap();
        assert_eq!(banner.text, t!("ide-library-decompiled"));
        assert!(!banner.download, "no library known: nothing to download");
        assert!(LibraryBanner::of(&serde_json::json!({ "kind": "sources" })).is_none());
        assert!(LibraryBanner::of(&serde_json::Value::Null).is_none());
    }

    #[test]
    fn dependencies_of_two_kinds_share_the_group_only_when_all_have_it() {
        let grouped = |name: &str, group: Option<&str>| Library {
            group: group.map(str::to_owned),
            ..library(LibraryKind::Dependency, name, Some("1.0.0"), "/m", true)
        };
        let project = Path::new("/p");
        let npm = vec![grouped("vue", Some("node_modules")), grouped("lodash", Some("node_modules"))];
        assert_eq!(Libraries::build(1, &npm, &[], project).modules_label, "node_modules");
        let mixed = vec![grouped("vue", Some("node_modules")), grouped("example.com/a", None)];
        assert_ne!(Libraries::build(1, &mixed, &[], project).modules_label, "node_modules");
    }

    #[test]
    fn a_dependency_without_version_is_shown_by_name() {
        let libraries = vec![library(LibraryKind::Dependency, "example.com/local", None, "/p/local", true)];
        let libraries = Libraries::build(1, &libraries, &[], Path::new("/p"));
        assert_eq!(libraries.dependencies[0].label, "example.com/local");
        assert!(libraries.module_name.is_none());
    }
}
