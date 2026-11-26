use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::settings::GradleSettings;
use super::{KIND, ReloadBuild, ReloadParams, catalog, model, script};
use crate::dependencies::sources::{CENTRAL, Repository};
use crate::dependencies::{
    BuildFiles, Completing, DeclaredDependency, DependencyChange, DependencyModule, DependencyProvider,
    DependencyStatus, FileChange, VersionPlace, VersionSite,
};

const CATALOG: &str = "gradle/libs.versions.toml";
const CATALOG_NAME: &str = "libs.versions.toml";
const PROPERTIES: &str = "gradle.properties";

pub struct GradleDependencies;

fn is_kotlin(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "kts")
}

fn catalog_path(root: &Path) -> PathBuf {
    root.join(CATALOG)
}

fn read_catalog(files: &BuildFiles<'_>) -> Option<(PathBuf, String, catalog::Catalog)> {
    let path = catalog_path(files.root);
    let text = files.read(&path)?;
    let parsed = catalog::parse(&text);
    Some((path, text, parsed))
}

fn property_in(text: &str, name: &str) -> Option<(String, std::ops::Range<usize>)> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != name {
            continue;
        }
        let trimmed = value.trim();
        let offset = start + key.len() + 1 + (value.len() - value.trim_start().len());
        return Some((trimmed.to_owned(), offset..offset + trimmed.len()));
    }
    None
}

fn variable(
    files: &BuildFiles<'_>,
    script_path: &Path,
    text: &str,
    name: &str,
) -> Option<(PathBuf, String, std::ops::Range<usize>)> {
    if let Some((value, range)) = script::variable_in(text, name) {
        return Some((script_path.to_owned(), value, range));
    }
    let folder = script_path.parent().unwrap_or(files.root);
    let mut places = vec![folder.join(PROPERTIES), files.root.join(PROPERTIES)];
    places.dedup();
    for place in places {
        if let Some(found) = files.read(&place).and_then(|text| property_in(&text, name)) {
            return Some((place, found.0, found.1));
        }
    }
    let top = model::build_file(files.root).filter(|top| top != script_path)?;
    let top_text = files.read(&top)?;
    let (value, range) = script::variable_in(&top_text, name)?;
    Some((top, value, range))
}

fn repositories_of(files: &BuildFiles<'_>, path: Option<&Path>) -> Vec<Repository> {
    if !files.trusted() {
        return vec![Repository::new("central", CENTRAL)];
    }
    let mut places: Vec<PathBuf> = model::settings_file(files.root).into_iter().collect();
    places.extend(model::build_file(files.root));
    places.extend(path.map(Path::to_owned));
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut order = Vec::new();
    for place in places {
        let Some(text) = files.read(&place) else { continue };
        for (id, url) in script::repository_urls(&text) {
            if found.insert(url.clone(), id).is_none() {
                order.push(url);
            }
        }
    }
    if order.is_empty() {
        return vec![Repository::new("central", CENTRAL)];
    }
    order.into_iter().filter_map(|url| found.get(&url).map(|id| Repository::new(id, &url))).collect()
}

fn platform_of(statements: &[script::Statement]) -> Option<String> {
    statements
        .iter()
        .find(|statement| statement.platform)
        .map(|statement| format!("{}:{}", statement.group, statement.artifact))
}

fn declared(files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<DeclaredDependency> {
    let statements = script::statements(text);
    let catalog = read_catalog(files);
    let platform = platform_of(&statements);
    let mut found = Vec::new();
    for statement in &statements {
        let (group, artifact, version, version_place) = match &statement.accessor {
            Some(accessor) => {
                let Some((_, _, parsed)) = &catalog else { continue };
                let Some(library) =
                    parsed.libraries.iter().find(|library| catalog::accessor(&library.alias) == *accessor)
                else {
                    continue;
                };
                (
                    library.group.clone(),
                    library.artifact.clone(),
                    library.resolved(parsed).unwrap_or_default(),
                    VersionPlace::Catalog { alias: library.alias.clone() },
                )
            }
            None => {
                let (version, place) = match (&statement.version, &statement.variable) {
                    (_, Some(name)) => (
                        variable(files, path, text, name).map(|found| found.1).unwrap_or_default(),
                        VersionPlace::Property { name: name.clone() },
                    ),
                    (Some(version), None) => (version.clone(), VersionPlace::Literal),
                    (None, None) => match &platform {
                        Some(by) => (String::new(), VersionPlace::Managed { by: by.clone() }),
                        None => (String::new(), VersionPlace::Unknown),
                    },
                };
                (statement.group.clone(), statement.artifact.clone(), version, place)
            }
        };
        found.push(DeclaredDependency {
            group,
            artifact,
            version,
            scope: statement.configuration.clone(),
            version_place,
            local: false,
            status: DependencyStatus::default(),
        });
    }
    found
}

fn script_of(files: &BuildFiles<'_>, module: &str) -> Result<(PathBuf, String), String> {
    let path = files.root.join(module);
    let text = files.read(&path).ok_or_else(|| t!("workbench-deps-no-module", module = module))?;
    Ok((path, text))
}

fn add(
    files: &BuildFiles<'_>,
    module: &str,
    group: &str,
    artifact: &str,
    version: &str,
    scope: &str,
) -> Result<Vec<FileChange>, String> {
    let (path, text) = script_of(files, module)?;
    let configuration = match scope {
        "" | "compile" => script::CONFIGURATIONS[0],
        "runtime" => "runtimeOnly",
        "provided" => "compileOnly",
        "test" => "testImplementation",
        other => other,
    };
    let existing = declared(files, &path, &text);
    if existing.iter().any(|known| known.group == group && known.artifact == artifact && known.scope == configuration) {
        return Err(t!("workbench-deps-already", dependency = format!("{group}:{artifact}")));
    }
    let style = script::style_of(&text, is_kotlin(&path));
    let mut changes = Vec::new();
    let argument = match read_catalog(files) {
        Some((catalog_file, catalog_text, parsed)) => {
            let known = parsed.libraries.iter().find(|library| library.group == group && library.artifact == artifact);
            let alias = match known {
                Some(library) => library.alias.clone(),
                None => {
                    let (changed, alias) = catalog::add(&catalog_text, group, artifact, version);
                    changes.push(FileChange { path: catalog_file, text: changed });
                    alias
                }
            };
            format!("libs.{}", catalog::accessor(&alias))
        }
        None => script::coordinates_argument(group, artifact, version, &style),
    };
    let line = script::line_of(configuration, &argument, &style);
    changes.push(FileChange { path, text: script::add_line(&text, configuration, &line) });
    Ok(changes)
}

fn set_version(
    files: &BuildFiles<'_>,
    module: &str,
    group: &str,
    artifact: &str,
    version: &str,
) -> Result<Vec<FileChange>, String> {
    let (path, text) = script_of(files, module)?;
    let managed = || t!("workbench-deps-managed-outside", dependency = format!("{group}:{artifact}"));
    if path.file_name().is_some_and(|name| name == CATALOG_NAME) {
        let changed = catalog::set_version(&text, group, artifact, version).ok_or_else(managed)?;
        return Ok(vec![FileChange { path, text: changed }]);
    }
    let statements = script::statements(&text);
    let literal = statements.iter().find(|statement| statement.group == group && statement.artifact == artifact);
    if let Some(statement) = literal {
        if let Some(name) = &statement.variable {
            let (place, _, range) = variable(files, &path, &text, name).ok_or_else(managed)?;
            let mut changed = files.read(&place).ok_or_else(managed)?;
            changed.replace_range(range, version);
            return Ok(vec![FileChange { path: place, text: changed }]);
        }
        let range = statement.version_range.clone().ok_or_else(managed)?;
        let mut changed = text;
        changed.replace_range(range, version);
        return Ok(vec![FileChange { path, text: changed }]);
    }
    let (catalog_file, catalog_text, _) = read_catalog(files).ok_or_else(|| script::not_declared(group, artifact))?;
    let changed = catalog::set_version(&catalog_text, group, artifact, version)
        .ok_or_else(|| script::not_declared(group, artifact))?;
    Ok(vec![FileChange { path: catalog_file, text: changed }])
}

fn catalog_user_left(files: &BuildFiles<'_>, skip: &Path, accessor: &str) -> bool {
    model::projects(files.root).into_iter().filter_map(|project| project.build_file).filter(|file| file != skip).any(
        |file| {
            files.read(&file).is_some_and(|text| {
                script::statements(&text).iter().any(|statement| statement.accessor.as_deref() == Some(accessor))
            })
        },
    )
}

fn remove(
    files: &BuildFiles<'_>,
    module: &str,
    group: &str,
    artifact: &str,
    scope: Option<&str>,
) -> Result<Vec<FileChange>, String> {
    let (path, text) = script_of(files, module)?;
    let statements = script::statements(&text);
    let catalog = read_catalog(files);
    let library = catalog.as_ref().and_then(|(_, _, parsed)| {
        parsed.libraries.iter().find(|library| library.group == group && library.artifact == artifact).cloned()
    });
    let accessor = library.as_ref().map(|library| catalog::accessor(&library.alias));
    let statement = statements
        .iter()
        .filter(|statement| scope.is_none_or(|scope| scope == statement.configuration))
        .find(|statement| {
            (statement.group == group && statement.artifact == artifact)
                || (statement.accessor.is_some() && statement.accessor == accessor)
        })
        .ok_or_else(|| script::not_declared(group, artifact))?;
    let changed = script::remove_statement(&text, statement);
    let mut changes = Vec::new();
    if let (Some(accessor), Some(library), Some((file, catalog_text, _))) = (&statement.accessor, &library, &catalog) {
        let used_here = script::statements(&changed).iter().any(|other| other.accessor.as_ref() == Some(accessor));
        if !used_here
            && !catalog_user_left(files, &path, accessor)
            && let Some(without) = catalog::remove(catalog_text, &library.alias)
        {
            changes.push(FileChange { path: file.clone(), text: without });
        }
    }
    changes.push(FileChange { path, text: changed });
    Ok(changes)
}

fn coordinates_at(text: &str, caret: usize) -> Option<Completing> {
    let from = script::string_prefix(text, caret)?;
    let typed = &text[from..caret];
    let parts: Vec<&str> = typed.split(':').collect();
    match parts.as_slice() {
        [prefix] => Some(Completing::Group { prefix: (*prefix).to_owned(), from }),
        [group, prefix] => Some(Completing::Artifact {
            group: (*group).to_owned(),
            prefix: (*prefix).to_owned(),
            from: from + group.len() + 1,
        }),
        [group, artifact, prefix] => Some(Completing::Version {
            group: (*group).to_owned(),
            artifact: (*artifact).to_owned(),
            prefix: (*prefix).to_owned(),
            from: from + group.len() + artifact.len() + 2,
        }),
        _ => None,
    }
}

fn in_libraries_table(text: &str, caret: usize) -> bool {
    text[..caret]
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('['))
        .is_some_and(|line| line.trim() == "[libraries]")
}

impl DependencyProvider for GradleDependencies {
    fn id(&self) -> &str {
        KIND
    }

    fn applies(&self, root: &Path) -> bool {
        model::is_build(root)
    }

    fn handles(&self, file_name: &str) -> bool {
        model::BUILD.contains(&file_name) || file_name == CATALOG_NAME
    }

    fn modules(&self, files: &BuildFiles<'_>) -> Vec<DependencyModule> {
        if !model::is_build(files.root) {
            return Vec::new();
        }
        model::projects(files.root)
            .into_iter()
            .filter_map(|project| {
                let file = project.build_file?;
                let text = files.read(&file)?;
                let id = file.strip_prefix(files.root).unwrap_or(&file).to_string_lossy().replace('\\', "/");
                Some(DependencyModule {
                    id,
                    name: project.name,
                    provider: KIND.to_owned(),
                    scopes: script::CONFIGURATIONS.iter().map(|name| (*name).to_owned()).collect(),
                    repositories: repositories_of(files, Some(&file)),
                    dependencies: declared(files, &file, &text),
                    file,
                    error: None,
                })
            })
            .collect()
    }

    fn change(&self, files: &BuildFiles<'_>, change: &DependencyChange) -> Result<Vec<FileChange>, String> {
        match change {
            DependencyChange::Add { module, group, artifact, version, scope } => {
                add(files, module, group, artifact, version, scope)
            }
            DependencyChange::SetVersion { module, group, artifact, version } => {
                set_version(files, module, group, artifact, version)
            }
            DependencyChange::Remove { module, group, artifact, scope } => {
                remove(files, module, group, artifact, scope.as_deref())
            }
            DependencyChange::AddBom { module, group, artifact, version } => {
                let (path, text) = script_of(files, module)?;
                let changed = script::add_bom(&text, group, artifact, version, is_kotlin(&path));
                Ok(vec![FileChange { path, text: changed }])
            }
            DependencyChange::AddRepository { module, url, .. } => {
                let (path, text) = script_of(files, module)?;
                let changed = script::add_repository(&text, url, is_kotlin(&path));
                Ok(vec![FileChange { path, text: changed }])
            }
            DependencyChange::Exclude { .. } => Err(t!("workbench-deps-exclude-unsupported")),
        }
    }

    fn sites(&self, files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<VersionSite> {
        if path.file_name().is_some_and(|name| name == CATALOG_NAME) {
            let parsed = catalog::parse(text);
            return parsed
                .libraries
                .iter()
                .filter_map(|library| {
                    let range = library.version_range.clone()?;
                    Some(VersionSite {
                        group: library.group.clone(),
                        artifact: library.artifact.clone(),
                        version: library.resolved(&parsed)?,
                        start: range.start,
                        end: range.end,
                    })
                })
                .collect();
        }
        script::statements(text)
            .iter()
            .filter_map(|statement| {
                let range = statement.version_range.clone()?;
                let version = match &statement.variable {
                    Some(name) => variable(files, path, text, name)?.1,
                    None => statement.version.clone()?,
                };
                Some(VersionSite {
                    group: statement.group.clone(),
                    artifact: statement.artifact.clone(),
                    version,
                    start: range.start,
                    end: range.end,
                })
            })
            .collect()
    }

    fn completing(&self, path: &Path, text: &str, caret: usize) -> Option<Completing> {
        let wanted = if path.file_name().is_some_and(|name| name == CATALOG_NAME) {
            in_libraries_table(text, caret)
        } else {
            script::caret_in_block(text, caret)
        };
        wanted.then(|| coordinates_at(text, caret)).flatten()
    }

    fn repositories_for(&self, files: &BuildFiles<'_>, path: &Path) -> Vec<Repository> {
        repositories_of(files, Some(path))
    }

    fn offline(&self, files: &BuildFiles<'_>) -> bool {
        GradleSettings::read(files.env, files.root).offline
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        ReloadBuild.run(session, ReloadParams::default()).map(|_| ())
    }
}
