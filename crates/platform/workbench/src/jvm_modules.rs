use std::path::{Path, PathBuf};

use zen_project::model::{Dependency, FolderKind, Module, ModuleFolder};

use crate::java_run::{BuildTool, build_tool};
use crate::maven::pom::{self, PomProject};

const GRADLE_9_NEEDS_JAVA: u32 = 17;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildLevel {
    pub level: u32,
    pub file: PathBuf,
    pub toolchain: bool,
}

fn feature_release(level: &str) -> Option<u32> {
    crate::jdk::detect::major_of(level.trim()).filter(|major| (1..=99).contains(major))
}

fn highest(found: impl Iterator<Item = BuildLevel>) -> Option<BuildLevel> {
    found.max_by_key(|found| found.level)
}

fn maven_levels(project: &PomProject) -> Vec<BuildLevel> {
    let own = feature_release(&project.build.language_level).map(|level| BuildLevel {
        level,
        file: project.pom.clone(),
        toolchain: false,
    });
    own.into_iter().chain(project.modules.iter().flat_map(maven_levels)).collect()
}

fn gradle_level(file: &Path, text: &str) -> Option<BuildLevel> {
    let toolchain = ["JavaLanguageVersion.of", "jvmToolchain"].iter().any(|marker| text.contains(marker));
    let level = feature_release(&gradle_language_level(text)?)?;
    Some(BuildLevel { level, file: file.to_owned(), toolchain })
}

fn gradle_runs_on(root: &Path, level: &BuildLevel) -> bool {
    let gradle = crate::build_tools::found::GRADLE.wrapper_version(root);
    let major = gradle.and_then(|version| version.split('.').next()?.parse::<u32>().ok());
    !level.toolchain || level.level >= GRADLE_9_NEEDS_JAVA || major.is_none_or(|major| major < 9)
}

pub fn build_level(root: &Path) -> Option<BuildLevel> {
    match build_tool(root) {
        BuildTool::Maven => highest(maven_levels(&pom::read(&root.join("pom.xml"))).into_iter()),
        BuildTool::Gradle => highest(crate::gradle::model::projects(root).into_iter().filter_map(|project| {
            let file = project.build_file?;
            gradle_level(&file, &std::fs::read_to_string(&file).ok()?)
        }))
        .filter(|level| gradle_runs_on(root, level)),
        BuildTool::Javac => None,
    }
}

pub fn modules(root: &Path) -> Vec<Module> {
    match build_tool(root) {
        BuildTool::Maven => {
            let mut found = Vec::new();
            maven_modules(&pom::read(&root.join("pom.xml")), &mut found);
            found
        }
        BuildTool::Gradle => gradle_modules(root),
        BuildTool::Javac => Vec::new(),
    }
}

fn maven_modules(project: &PomProject, found: &mut Vec<Module>) {
    let folder = project.pom.parent().map(Path::to_owned).unwrap_or_default();
    let name = if project.artifact_id.is_empty() {
        folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        project.artifact_id.clone()
    };
    if let Some(error) = &project.error {
        found.push(Module {
            name,
            root: folder,
            file: project.pom.clone(),
            details: serde_json::json!({ "build": "Maven", "error": error }),
            ..Module::default()
        });
        return;
    }
    let build = &project.build;
    let target = place(&folder, &build.directory, "target", None);
    let path = |written: &str, default: &str| place(&folder, written, default, Some(&target));
    let mut folders = Vec::new();
    let mut add = |written: &str, default: &str, kind: FolderKind| {
        let path = path(written, default);
        if !written.is_empty() || path.is_dir() {
            folders.push(ModuleFolder { path, kind });
        }
    };
    add(&build.source_directory, "src/main/java", FolderKind::Sources);
    add("", "src/main/kotlin", FolderKind::Sources);
    add(&build.test_source_directory, "src/test/java", FolderKind::TestSources);
    add("", "src/test/kotlin", FolderKind::TestSources);
    for (written, default, kind) in [
        (&build.resources, "src/main/resources", FolderKind::Resources),
        (&build.test_resources, "src/test/resources", FolderKind::TestResources),
    ] {
        if written.is_empty() {
            add("", default, kind);
        }
        for resource in written {
            add(resource, default, kind);
        }
    }
    folders.push(ModuleFolder { path: target.clone(), kind: FolderKind::Excluded });
    let dependencies = project
        .dependencies
        .iter()
        .map(|dependency| Dependency {
            name: format!("{}:{}", dependency.group_id, dependency.artifact_id),
            version: dependency.version.clone(),
            scope: Some(dependency.scope.clone()),
            ..Dependency::default()
        })
        .collect();
    found.push(Module {
        name,
        file: project.pom.clone(),
        language_version: (!build.language_level.is_empty()).then(|| build.language_level.clone()),
        dependencies,
        details: serde_json::json!({ "build": "Maven" }),
        folders,
        output: Some(path(&build.output_directory, "target/classes")),
        test_output: Some(path(&build.test_output_directory, "target/test-classes")),
        assumed: false,
        root: folder,
        ..Module::default()
    });
    for module in &project.modules {
        maven_modules(module, found);
    }
}

fn place(folder: &Path, written: &str, default: &str, target: Option<&Path>) -> PathBuf {
    if written.is_empty() {
        return match (target, default.strip_prefix("target/")) {
            (Some(target), Some(rest)) => target.join(rest),
            _ => folder.join(default),
        };
    }
    let mut text = written.replace("${project.basedir}", &folder.to_string_lossy());
    text = text.replace("${basedir}", &folder.to_string_lossy());
    if let Some(target) = target {
        text = text.replace("${project.build.directory}", &target.to_string_lossy());
    }
    let path = PathBuf::from(text);
    if path.is_absolute() { path } else { folder.join(path) }
}

fn gradle_modules(root: &Path) -> Vec<Module> {
    crate::gradle::model::projects(root)
        .into_iter()
        .map(|project| {
            let folder = project.folder;
            let text = project.build_file.as_deref().and_then(|file| std::fs::read_to_string(file).ok());
            let mut folders: Vec<ModuleFolder> = [
                ("src/main/java", FolderKind::Sources),
                ("src/main/kotlin", FolderKind::Sources),
                ("src/main/groovy", FolderKind::Sources),
                ("src/main/resources", FolderKind::Resources),
                ("src/test/java", FolderKind::TestSources),
                ("src/test/kotlin", FolderKind::TestSources),
                ("src/test/groovy", FolderKind::TestSources),
                ("src/test/resources", FolderKind::TestResources),
            ]
            .into_iter()
            .map(|(relative, kind)| ModuleFolder { path: folder.join(relative), kind })
            .filter(|found| found.path.is_dir())
            .collect();
            folders.push(ModuleFolder { path: folder.join("build"), kind: FolderKind::Excluded });
            if folder == root {
                folders.push(ModuleFolder { path: folder.join(".gradle"), kind: FolderKind::Excluded });
            }
            Module {
                name: project.name,
                file: project
                    .build_file
                    .clone()
                    .or_else(|| crate::gradle::model::settings_file(&folder))
                    .unwrap_or_default(),
                language_version: text.as_deref().and_then(gradle_language_level),
                dependencies: text.as_deref().map(gradle_dependencies).unwrap_or_default(),
                details: serde_json::json!({ "build": "Gradle", "path": project.path }),
                folders,
                output: Some(folder.join("build/classes/java/main")),
                test_output: Some(folder.join("build/classes/java/test")),
                assumed: true,
                root: folder,
                ..Module::default()
            }
        })
        .collect()
}

pub fn gradle_dependencies(text: &str) -> Vec<Dependency> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let scope: String = trimmed.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if scope.is_empty() {
            continue;
        }
        for coordinates in crate::jvm_libraries::gradle_coordinates(line) {
            found.push(Dependency {
                name: format!("{}:{}", coordinates.group_id, coordinates.artifact_id),
                version: coordinates.version,
                scope: Some(scope.clone()),
                ..Dependency::default()
            });
        }
    }
    found
}

pub fn gradle_language_level(text: &str) -> Option<String> {
    let after = |marker: &str| -> Option<String> {
        let at = text.find(marker)? + marker.len();
        let digits: String = text[at..]
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '=' | '(' | '"' | '\''))
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '_'))
            .collect();
        let level = digits.trim_matches(|c| c == '.' || c == '_').replace('_', ".");
        (!level.is_empty()).then_some(level)
    };
    [
        "JavaLanguageVersion.of",
        "jvmToolchain",
        "JavaVersion.VERSION_",
        "sourceCompatibility",
        "release.set",
        "options.release =",
    ]
    .iter()
    .find_map(|marker| after(marker))
}
