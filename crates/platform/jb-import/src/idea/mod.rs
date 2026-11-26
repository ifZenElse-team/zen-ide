mod code_style;
mod code_style_write;
mod encodings;
mod exclusion;
pub mod inspections;
pub mod modules;
mod run;
pub mod scopes;
pub mod xml;

use std::path::{Component, Path, PathBuf};

use zen_project::settings::{
    ExclusionChange, ProjectSettings, ReadFiles, SchemeFile, SettingsProblem, SettingsProvider,
};

use self::xml::Macros;

const IDEA: &str = ".idea";
const RUN: &str = ".run";
const MODULES: &str = "modules.xml";
const PERSONAL: &[&str] = &[
    "workspace.xml",
    "shelf",
    "tasks.xml",
    "usage.statistics.xml",
    "dataSources",
    "dataSources.local.xml",
    "httpRequests",
    "sonarlint",
];
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdeaProject {
    user_home: Option<PathBuf>,
}

impl IdeaProject {
    pub fn new() -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        Self::with_user_home(home)
    }

    pub fn with_user_home(user_home: Option<PathBuf>) -> Self {
        Self { user_home }
    }
}

pub fn is_idea_file(relative: &Path) -> bool {
    let in_folder = names(relative).any(|name| name == IDEA || name == RUN);
    in_folder || relative.extension().is_some_and(|extension| extension == "iml")
}

impl SettingsProvider for IdeaProject {
    fn id(&self) -> &str {
        "idea"
    }

    fn watches(&self, relative: &Path) -> bool {
        let parts: Vec<&std::ffi::OsStr> = names(relative).collect();
        match parts.as_slice() {
            [first, second, ..] if *first == IDEA => {
                !PERSONAL.iter().any(|name| *second == *name)
                    && parts.len() <= MAX_DEPTH + 2
                    && (has_suffix(relative, ".xml") || relative.extension().is_none())
            }
            [first, _] if *first == RUN => has_suffix(relative, ".run.xml"),
            [first] if *first == IDEA || *first == RUN => true,
            _ => relative.extension().is_some_and(|extension| extension == "iml"),
        }
    }

    fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings> {
        let idea = files.list(Path::new(IDEA));
        let run = files.list(Path::new(RUN));
        let top: Vec<PathBuf> = files.list(Path::new("")).into_iter().filter(|path| is_module_file(path)).collect();
        if idea.is_empty() && run.is_empty() && top.is_empty() {
            return None;
        }
        let macros = Macros::new(root, self.user_home.as_deref());
        let mut settings = ProjectSettings::default();
        let mut sources: Vec<PathBuf> = Vec::new();
        collect_xml(files, idea, 0, &mut sources);
        sources.extend(run.into_iter().filter(|path| has_suffix(path, ".run.xml")));
        sources.extend(top);
        let modules = Path::new(IDEA).join(MODULES);
        if sources.contains(&modules) {
            sources.extend(module_files(root, files, &modules, &macros, &mut settings.problems));
        }
        sources.sort();
        sources.dedup();
        modules::read(root, files, &sources, &macros, &mut settings);
        scopes::read(root, files, &sources, &mut settings);
        inspections::read(root, files, &sources, &mut settings);
        let (run_configurations, problems) = run::read(root, files, &macros);
        settings.run_configurations = run_configurations;
        settings.problems.extend(problems);
        (settings.code_style, settings.code_style_options) = code_style::read(root, files, &mut settings.problems);
        settings.encodings = encodings::read(root, files, &macros, &mut settings.problems);
        for source in &sources {
            if files.read(source).is_none() {
                settings
                    .problems
                    .push(SettingsProblem { file: root.join(source), message: "cannot read the file".to_owned() });
            }
        }
        let mut sources: Vec<PathBuf> = sources.iter().map(|source| root.join(source)).collect();
        sources.sort();
        sources.dedup();
        settings.sources = sources;
        Some(settings)
    }

    fn set_excluded(&self, root: &Path, folder: &Path, excluded: bool) -> Option<Result<ExclusionChange, String>> {
        Some(exclusion::set_excluded(root, self.user_home.as_deref(), folder, excluded))
    }

    fn write_code_style(&self, root: &Path, scheme: &SchemeFile) -> Option<Result<Vec<PathBuf>, String>> {
        Some(code_style_write::write(root, scheme))
    }

    fn code_style_text(&self, scheme: &SchemeFile) -> Option<String> {
        Some(code_style_write::scheme_text(scheme))
    }

    fn parse_code_style(&self, text: &str) -> Option<Result<SchemeFile, String>> {
        Some(code_style::parse_file(text))
    }

    fn set_resource_root(&self, root: &Path, folder: &Path, marked: bool) -> Option<Result<ExclusionChange, String>> {
        Some(exclusion::set_resource_root(root, self.user_home.as_deref(), folder, marked))
    }

    fn set_exclude_patterns(&self, root: &Path, patterns: &[String]) -> Option<Result<ExclusionChange, String>> {
        Some(exclusion::set_exclude_patterns(root, self.user_home.as_deref(), patterns))
    }

    fn set_encodings(&self, root: &Path, encodings: &zen_project::settings::Encodings) -> Option<Result<(), String>> {
        Some(encodings::write(root, self.user_home.as_deref(), encodings))
    }
}

fn module_sources(
    root: &Path,
    files: &dyn ReadFiles,
    macros: &Macros,
    problems: &mut Vec<SettingsProblem>,
) -> Vec<PathBuf> {
    let mut sources: Vec<PathBuf> = files.list(Path::new("")).into_iter().filter(|path| is_module_file(path)).collect();
    sources.extend(module_files(root, files, &Path::new(IDEA).join(MODULES), macros, problems));
    sources.sort();
    sources.dedup();
    sources
}

fn names(path: &Path) -> impl Iterator<Item = &std::ffi::OsStr> {
    path.components().filter_map(|component| match component {
        Component::Normal(name) => Some(name),
        _ => None,
    })
}

fn collect_xml(files: &dyn ReadFiles, entries: Vec<PathBuf>, depth: usize, sources: &mut Vec<PathBuf>) {
    for entry in entries {
        let personal =
            depth == 0 && entry.file_name().is_some_and(|name| PERSONAL.iter().any(|personal| name == *personal));
        if personal {
            continue;
        }
        let children = if depth < MAX_DEPTH { files.list(&entry) } else { Vec::new() };
        if !children.is_empty() {
            collect_xml(files, children, depth + 1, sources);
        } else if has_suffix(&entry, ".xml") {
            sources.push(entry);
        }
    }
}

fn module_files(
    root: &Path,
    files: &dyn ReadFiles,
    modules: &Path,
    macros: &Macros,
    problems: &mut Vec<SettingsProblem>,
) -> Vec<PathBuf> {
    let Some(text) = files.read(modules) else { return Vec::new() };
    let document = match xml::parse(&text) {
        Ok(document) => document,
        Err(error) => {
            problems.push(SettingsProblem {
                file: root.join(modules),
                message: format!("cannot parse {MODULES}: {error}"),
            });
            return Vec::new();
        }
    };
    let Some(manager) = xml::component(&document, "ProjectModuleManager") else { return Vec::new() };
    manager
        .descendants()
        .filter(|node| node.has_tag_name("module"))
        .filter_map(|node| {
            node.attribute("filepath").map(|path| macros.path(path)).or_else(|| macros.url(node.attribute("fileurl")?))
        })
        .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_owned))
        .filter(|path| is_module_file(path))
        .collect()
}

fn parse_problem(root: &Path, source: &Path, error: &str) -> SettingsProblem {
    let name = source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    SettingsProblem { file: root.join(source), message: format!("cannot parse {name}: {error}") }
}

fn is_module_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "iml")
}

fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct Memory(BTreeMap<PathBuf, String>);

    impl Memory {
        fn with(files: &[(&str, &str)]) -> Self {
            Self(files.iter().map(|(path, text)| (PathBuf::from(path), (*text).to_owned())).collect())
        }
    }

    impl ReadFiles for Memory {
        fn read(&self, relative: &Path) -> Option<String> {
            self.0.get(relative).cloned()
        }

        fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
            let mut entries: Vec<PathBuf> = self
                .0
                .keys()
                .filter_map(|path| {
                    let rest = path.strip_prefix(relative_dir).ok()?;
                    let first = rest.components().next()?;
                    Some(relative_dir.join(first))
                })
                .collect();
            entries.dedup();
            entries
        }
    }

    #[test]
    fn idea_files_are_recognised_and_personal_ones_are_not_watched() {
        let provider = IdeaProject::with_user_home(None);
        for path in [".idea/misc.xml", ".idea/codeStyles/Project.xml", ".run/serve.run.xml", "app.iml", "sub/app.iml"] {
            assert!(is_idea_file(Path::new(path)), "{path}");
            assert!(provider.watches(Path::new(path)), "{path}");
        }
        for path in [
            ".idea/workspace.xml",
            ".idea/shelf/Changes.xml",
            ".idea/dataSources/a1b2.xml",
            ".idea/dataSources.local.xml",
            ".idea/copilot.data.migration.ask.lock",
            ".run/notes.txt",
        ] {
            assert!(is_idea_file(Path::new(path)), "{path}");
            assert!(!provider.watches(Path::new(path)), "{path}");
        }
        for path in ["main.go", "idea/misc.xml", "src/.idea.go", "app.iml.bak"] {
            assert!(!is_idea_file(Path::new(path)), "{path}");
            assert!(!provider.watches(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_project_without_idea_files_has_no_settings() {
        let files = Memory::with(&[("main.go", "package main\n"), ("go.mod", "module m\n")]);
        assert_eq!(IdeaProject::with_user_home(None).load(Path::new("/p"), &files), None);
    }

    #[test]
    fn modules_listed_in_modules_xml_are_sources_and_broken_files_are_problems() {
        let modules = r#"<project version="4"><component name="ProjectModuleManager"><modules>
            <module fileurl="file://$PROJECT_DIR$/svc/svc.iml" filepath="$PROJECT_DIR$/svc/svc.iml" />
            <module fileurl="file://$PROJECT_DIR$/web/web.iml" />
            <module filepath="/elsewhere/other.iml" />
        </modules></component></project>"#;
        let files = Memory::with(&[
            (".idea/modules.xml", modules),
            (".idea/workspace.xml", "<project/>"),
            ("svc/svc.iml", "<module/>"),
            ("web/web.iml", "<module/>"),
        ]);
        let settings = IdeaProject::with_user_home(None).load(Path::new("/p"), &files).unwrap();
        let expected: Vec<PathBuf> =
            ["/p/.idea/modules.xml", "/p/svc/svc.iml", "/p/web/web.iml"].iter().map(PathBuf::from).collect();
        assert_eq!(settings.sources, expected);
        assert!(settings.problems.is_empty());

        let broken = Memory::with(&[(".idea/modules.xml", "<project")]);
        let settings = IdeaProject::with_user_home(None).load(Path::new("/p"), &broken).unwrap();
        assert_eq!(settings.problems.len(), 1);
        assert_eq!(settings.problems[0].file, PathBuf::from("/p/.idea/modules.xml"));
    }
}
