use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use roxmltree::{Document, Node};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::pom::{self, child, children, coordinates_and_properties, interpolate, module_paths, text_of};
use super::pom_edit::{self, NewBom, NewDependency, NewRepository, VersionEdit};
use super::settings::MavenSettings;
use super::{KIND, ReloadParams, ReloadProject};
use crate::dependencies::sources::{CENTRAL, Repository, safe_coordinates};
use crate::dependencies::{
    BuildFiles, Completing, DeclaredDependency, DependencyChange, DependencyModule, DependencyProvider,
    DependencyStatus, FileChange, VersionPlace, VersionSite,
};

const MAX_LAYERS: usize = 12;
const MAX_MODULE_DEPTH: usize = 8;
const MAX_PASSES: usize = 4;
const SCOPES: [&str; 4] = ["compile", "provided", "runtime", "test"];
const POM: &str = "pom.xml";
const ANNOTATION_PROCESSOR: &str = "annotationProcessor";

pub struct MavenDependencies;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Coordinates {
    group: String,
    artifact: String,
    version: String,
}

impl Coordinates {
    fn text(&self) -> String {
        format!("{}:{}:{}", self.group, self.artifact, self.version)
    }
}

#[derive(Clone, Debug)]
struct Layer {
    file: Option<PathBuf>,
    text: String,
    coordinates: Coordinates,
}

type ParentPom = (Coordinates, Option<String>);

fn read_coordinates(text: &str) -> Option<(Coordinates, Option<ParentPom>)> {
    let document = Document::parse(text).ok()?;
    let root = document.root_element();
    let (group, artifact, version, _) = coordinates_and_properties(root);
    let parent = child(root, "parent").map(|parent| {
        let relative = child(parent, "relativePath").map(|_| text_of(parent, "relativePath"));
        let coordinates = Coordinates {
            group: text_of(parent, "groupId"),
            artifact: text_of(parent, "artifactId"),
            version: text_of(parent, "version"),
        };
        (coordinates, relative)
    });
    Some((Coordinates { group, artifact, version }, parent))
}

fn local_or_remote(files: &BuildFiles<'_>, repositories: &[Repository], wanted: &Coordinates) -> Option<String> {
    if !safe_coordinates(&[&wanted.group, &wanted.artifact, &wanted.version]) {
        return None;
    }
    let chosen = MavenSettings::read(files.env, files.root).local_repository;
    let repository = if chosen.is_empty() {
        crate::jvm_libraries::maven_repository(&files.env.home())
    } else {
        PathBuf::from(chosen)
    };
    let file = pom::local_pom(&repository, &wanted.group, &wanted.artifact, &wanted.version);
    files.read(&file).or_else(|| (files.remote_pom)(repositories, &wanted.group, &wanted.artifact, &wanted.version))
}

fn lexically_clean(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    clean
}

fn parent_on_disk(
    files: &BuildFiles<'_>,
    folder: &Path,
    parent: &Coordinates,
    relative: Option<&str>,
) -> Option<(PathBuf, String)> {
    let relative = relative.unwrap_or("../pom.xml");
    if relative.is_empty() {
        return None;
    }
    let path = lexically_clean(&folder.join(relative));
    let path = if path.is_dir() { path.join(POM) } else { path };
    let text = files.read(&path)?;
    let (found, _) = read_coordinates(&text)?;
    (found.group == parent.group && found.artifact == parent.artifact).then_some((path, text))
}

fn lineage(files: &BuildFiles<'_>, path: &Path, text: &str, repositories: &[Repository]) -> Vec<Layer> {
    let mut layers = Vec::new();
    let mut current = Some((Some(path.to_owned()), text.to_owned()));
    while let Some((file, text)) = current.take() {
        if layers.len() >= MAX_LAYERS {
            break;
        }
        let Some((coordinates, parent)) = read_coordinates(&text) else { break };
        if let Some((parent, relative)) = parent {
            let folder = file.as_ref().and_then(|file| file.parent().map(Path::to_owned));
            let near = folder.and_then(|folder| parent_on_disk(files, &folder, &parent, relative.as_deref()));
            current = match near {
                Some((path, text)) => Some((Some(path), text)),
                None => local_or_remote(files, repositories, &parent).map(|text| (None, text)),
            };
        }
        layers.push(Layer { file, text, coordinates });
    }
    layers
}

fn merged_properties(layers: &[Layer]) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    for layer in layers.iter().rev() {
        if let Ok(document) = Document::parse(&layer.text) {
            merged.extend(coordinates_and_properties(document.root_element()).3);
        }
    }
    for _ in 0..MAX_PASSES {
        let again: BTreeMap<String, String> =
            merged.iter().map(|(name, value)| (name.clone(), interpolate(value, &merged))).collect();
        if again == merged {
            break;
        }
        merged = again;
    }
    merged
}

#[derive(Clone, Debug)]
struct Managed {
    version: String,
    place: VersionPlace,
}

type ManagedVersions = HashMap<(String, String), Managed>;

fn inside_project(root: &Path, file: Option<&PathBuf>) -> Option<String> {
    let inside = file?.strip_prefix(root).ok()?;
    Some(inside.to_string_lossy().replace('\\', "/"))
}

fn managed_versions(
    files: &BuildFiles<'_>,
    layers: &[Layer],
    outer: Option<&BTreeMap<String, String>>,
    by: Option<&Coordinates>,
    repositories: &[Repository],
    depth: usize,
    found: &mut ManagedVersions,
) {
    let mut properties = merged_properties(layers);
    if let Some(outer) = outer {
        properties.extend(outer.clone());
    }
    let mut imports = Vec::new();
    for layer in layers {
        let Ok(document) = Document::parse(&layer.text) else { continue };
        let root = document.root_element();
        let list = child(root, "dependencyManagement").and_then(|management| child(management, "dependencies"));
        for node in list.into_iter().flat_map(|list| children(list, "dependency")) {
            let value = |name: &str| interpolate(&text_of(node, name), &properties);
            let (group, artifact, version) = (value("groupId"), value("artifactId"), value("version"));
            if text_of(node, "scope") == "import" {
                imports.push(Coordinates { group, artifact, version });
                continue;
            }
            let place = match (by, inside_project(files.root, layer.file.as_ref())) {
                (None, Some(file)) => VersionPlace::ManagedHere { by: file },
                (Some(by), _) => VersionPlace::Managed { by: by.text() },
                (None, None) => VersionPlace::Managed { by: layer.coordinates.text() },
            };
            found.entry((group, artifact)).or_insert(Managed { version, place });
        }
    }
    if depth >= MAX_LAYERS {
        return;
    }
    for bom in imports {
        let Some(text) = local_or_remote(files, repositories, &bom) else { continue };
        let bom_layers = lineage_of_text(files, &text, repositories);
        managed_versions(files, &bom_layers, None, Some(&bom), repositories, depth + 1, found);
    }
}

fn lineage_of_text(files: &BuildFiles<'_>, text: &str, repositories: &[Repository]) -> Vec<Layer> {
    let mut layers = lineage(files, Path::new(""), text, repositories);
    if let Some(first) = layers.first_mut() {
        first.file = None;
    }
    layers
}

fn central() -> Repository {
    Repository::new("central", CENTRAL)
}

#[derive(Default)]
struct UserSettings {
    servers: HashMap<String, (String, String)>,
    mirrors: Vec<(String, String, String)>,
    repositories: Vec<Repository>,
    offline: bool,
}

fn expanded(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("${env.") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "${env.".len()..];
        let end = after.find('}')?;
        out.push_str(&std::env::var(&after[..end]).ok()?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn repositories_in(node: Node<'_, '_>) -> Vec<Repository> {
    let list = child(node, "repositories");
    list.into_iter()
        .flat_map(|list| children(list, "repository"))
        .filter(|repository| {
            child(*repository, "releases").is_none_or(|releases| text_of(releases, "enabled") != "false")
        })
        .map(|repository| Repository::new(&text_of(repository, "id"), &text_of(repository, "url")))
        .filter(|repository| !repository.url.is_empty() && !repository.url.contains("${"))
        .collect()
}

fn read_user_settings(files: &BuildFiles<'_>) -> UserSettings {
    let chosen = MavenSettings::read(files.env, files.root).user_settings;
    let path = if chosen.is_empty() { files.env.home().join(".m2/settings.xml") } else { PathBuf::from(chosen) };
    let Some(text) = files.read(&path) else { return UserSettings::default() };
    let Ok(document) = Document::parse(&text) else { return UserSettings::default() };
    let root = document.root_element();
    let mut settings = UserSettings { offline: text_of(root, "offline") == "true", ..UserSettings::default() };
    for server in child(root, "servers").into_iter().flat_map(|list| children(list, "server")) {
        let user = expanded(&text_of(server, "username"));
        let password = expanded(&text_of(server, "password"));
        if let (Some(user), Some(password)) = (user, password)
            && !user.is_empty()
            && !password.starts_with('{')
        {
            settings.servers.insert(text_of(server, "id"), (user, password));
        }
    }
    for mirror in child(root, "mirrors").into_iter().flat_map(|list| children(list, "mirror")) {
        settings.mirrors.push((text_of(mirror, "id"), text_of(mirror, "url"), text_of(mirror, "mirrorOf")));
    }
    let active: Vec<String> = child(root, "activeProfiles")
        .into_iter()
        .flat_map(|list| children(list, "activeProfile"))
        .map(|node| node.text().unwrap_or_default().trim().to_owned())
        .collect();
    for profile in child(root, "profiles").into_iter().flat_map(|list| children(list, "profile")) {
        let by_default =
            child(profile, "activation").is_some_and(|activation| text_of(activation, "activeByDefault") == "true");
        if by_default || active.contains(&text_of(profile, "id")) {
            settings.repositories.extend(repositories_in(profile));
        }
    }
    settings
}

fn mirror_matches(of: &str, id: &str) -> bool {
    let mut matched = false;
    for pattern in of.split(',').map(str::trim) {
        if pattern.strip_prefix('!') == Some(id) {
            return false;
        }
        matched |= pattern == "*" || pattern == "external:*" || pattern == id;
    }
    matched
}

fn with_settings(mut repositories: Vec<Repository>, settings: &UserSettings) -> Vec<Repository> {
    repositories.extend(settings.repositories.iter().cloned());
    repositories.push(central());
    let mut seen = Vec::new();
    for repository in repositories {
        let mirrored = settings.mirrors.iter().find(|(_, _, of)| mirror_matches(of, &repository.id));
        let mut repository = match mirrored {
            Some((id, url, _)) => Repository::new(id, url),
            None => repository,
        };
        if let Some((user, password)) = settings.servers.get(&repository.id) {
            repository = repository.basic(user, password);
        }
        if !seen.iter().any(|known: &Repository| known.url == repository.url) {
            seen.push(repository);
        }
    }
    seen
}

fn repositories_of(files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<Repository> {
    repositories_with(files, path, text, &read_user_settings(files))
}

fn repositories_with(files: &BuildFiles<'_>, path: &Path, text: &str, settings: &UserSettings) -> Vec<Repository> {
    let base = with_settings(Vec::new(), settings);
    if !files.trusted() {
        return base;
    }
    let mut declared = Vec::new();
    for layer in lineage(files, path, text, &base) {
        if let Ok(document) = Document::parse(&layer.text) {
            declared.extend(repositories_in(document.root_element()));
        }
    }
    with_settings(declared, settings)
}

fn reactor(files: &BuildFiles<'_>, path: &Path, depth: usize, found: &mut Vec<(PathBuf, String)>) {
    let Some(text) = files.read(path) else {
        found.push((path.to_owned(), String::new()));
        return;
    };
    let modules = module_paths(&text);
    found.push((path.to_owned(), text));
    if depth >= MAX_MODULE_DEPTH {
        return;
    }
    let folder = path.parent().unwrap_or(Path::new(""));
    for module in modules {
        let file = folder.join(&module);
        let file = if file.extension().is_some_and(|ext| ext == "xml") { file } else { file.join(POM) };
        reactor(files, &file, depth + 1, found);
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn declared_in(
    files: &BuildFiles<'_>,
    path: &Path,
    text: &str,
    repositories: &[Repository],
    reactor: &[(String, String)],
) -> Result<Vec<DeclaredDependency>, String> {
    let layers = lineage(files, path, text, repositories);
    let properties = merged_properties(&layers);
    let mut managed = ManagedVersions::new();
    managed_versions(files, &layers, Some(&properties), None, repositories, 0, &mut managed);
    let document = Document::parse(text).map_err(|error| error.to_string())?;
    let root = document.root_element();
    let mut found = Vec::new();
    for node in child(root, "dependencies").into_iter().flat_map(|list| children(list, "dependency")) {
        let value = |name: &str| interpolate(&text_of(node, name), &properties);
        let (group, artifact) = (value("groupId"), value("artifactId"));
        let written = text_of(node, "version");
        let scope = match text_of(node, "scope") {
            scope if scope.is_empty() => "compile".to_owned(),
            scope => scope,
        };
        let (version, version_place) = if written.is_empty() {
            match managed.get(&(group.clone(), artifact.clone())) {
                Some(entry) => (entry.version.clone(), entry.place.clone()),
                None => (String::new(), VersionPlace::Unknown),
            }
        } else if written.starts_with("${project.") {
            (interpolate(&written, &properties), VersionPlace::Literal)
        } else if let Some(name) = written.strip_prefix("${").and_then(|rest| rest.strip_suffix('}')) {
            let holder = layers.iter().find(|layer| {
                Document::parse(&layer.text).is_ok_and(|document| {
                    child(document.root_element(), "properties").and_then(|list| child(list, name)).is_some()
                })
            });
            let place = match holder {
                Some(layer) if layer.file.is_none() => VersionPlace::Managed { by: layer.coordinates.text() },
                _ => VersionPlace::Property { name: name.to_owned() },
            };
            (interpolate(&written, &properties), place)
        } else {
            (written, VersionPlace::Literal)
        };
        let local = reactor.iter().any(|(g, a)| *g == group && *a == artifact);
        found.push(DeclaredDependency {
            group,
            artifact,
            version,
            scope,
            version_place,
            local,
            status: DependencyStatus::default(),
        });
    }
    Ok(found)
}

fn new_change(path: PathBuf, text: String) -> Vec<FileChange> {
    vec![FileChange { path, text }]
}

fn set_version(
    files: &BuildFiles<'_>,
    path: &Path,
    group: &str,
    artifact: &str,
    version: &str,
) -> Result<Vec<FileChange>, String> {
    let text = files.read(path).ok_or_else(|| t!("workbench-deps-no-module", module = relative(files.root, path)))?;
    let repositories = repositories_of(files, path, &text);
    let layers: Vec<Layer> = lineage(files, path, &text, &repositories);
    let mut wanted_property: Option<String> = None;
    for layer in layers.iter().filter(|layer| layer.file.as_ref().is_some_and(|file| file.starts_with(files.root))) {
        let Some(file) = &layer.file else { continue };
        if let Some(name) = &wanted_property {
            if let Some(changed) = pom_edit::set_property(&layer.text, name, version)? {
                return Ok(new_change(file.clone(), changed));
            }
            continue;
        }
        match pom_edit::set_version(&layer.text, group, artifact, version)? {
            VersionEdit::Changed(changed) => return Ok(new_change(file.clone(), changed)),
            VersionEdit::InProperty(name) => wanted_property = Some(name),
            VersionEdit::Absent => {}
        }
    }
    Err(t!("workbench-deps-managed-outside", dependency = format!("{group}:{artifact}")))
}

fn add(
    files: &BuildFiles<'_>,
    path: &Path,
    group: &str,
    artifact: &str,
    version: &str,
    scope: &str,
) -> Result<Vec<FileChange>, String> {
    let text = files.read(path).ok_or_else(|| t!("workbench-deps-no-module", module = relative(files.root, path)))?;
    let repositories = repositories_of(files, path, &text);
    let layers = lineage(files, path, &text, &repositories);
    let properties = merged_properties(&layers);
    let mut managed = ManagedVersions::new();
    managed_versions(files, &layers, Some(&properties), None, &repositories, 0, &mut managed);
    let is_managed = managed.contains_key(&(group.to_owned(), artifact.to_owned()));
    let new = NewDependency {
        group,
        artifact,
        version: (!is_managed && !version.is_empty()).then_some(version),
        scope: (!scope.is_empty() && scope != "compile" && scope != ANNOTATION_PROCESSOR).then_some(scope),
        optional: scope == ANNOTATION_PROCESSOR,
    };
    Ok(new_change(path.to_owned(), pom_edit::add(&text, &new)?))
}

fn in_dependency(text: &str, caret: usize) -> Option<(usize, usize)> {
    let before = &text[..caret];
    let open = before.rfind("<dependency>")?;
    if before.rfind("</dependency>").is_some_and(|close| close > open) {
        return None;
    }
    let end = text[caret..].find("</dependency>").map_or(text.len(), |at| caret + at);
    Some((open, end))
}

fn sibling(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find('<')? + start;
    Some(text[start..end].trim().to_owned()).filter(|value| !value.is_empty())
}

fn pom_completing(text: &str, caret: usize) -> Option<Completing> {
    let (open, end) = in_dependency(text, caret)?;
    let before = &text[open..caret];
    let tag_at = before.rfind('<')?;
    let tag = &before[tag_at..];
    let close = tag.find('>')?;
    let name = tag[1..close].trim();
    let prefix = tag[close + 1..].trim_start().to_owned();
    let from = caret - prefix.len();
    let block = &text[open..end];
    match name {
        "groupId" => Some(Completing::Group { prefix, from }),
        "artifactId" => Some(Completing::Artifact { group: sibling(block, "groupId")?, prefix, from }),
        "version" => Some(Completing::Version {
            group: sibling(block, "groupId")?,
            artifact: sibling(block, "artifactId")?,
            prefix,
            from,
        }),
        _ => None,
    }
}

impl DependencyProvider for MavenDependencies {
    fn id(&self) -> &str {
        KIND
    }

    fn applies(&self, root: &Path) -> bool {
        root.join(POM).is_file()
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name == POM
    }

    fn modules(&self, files: &BuildFiles<'_>) -> Vec<DependencyModule> {
        let top = files.root.join(POM);
        if files.read(&top).is_none() {
            return Vec::new();
        }
        let mut poms = Vec::new();
        reactor(files, &top, 0, &mut poms);
        let own: Vec<(String, String)> = poms
            .iter()
            .filter_map(|(_, text)| read_coordinates(text))
            .map(|(coordinates, _)| (coordinates.group, coordinates.artifact))
            .collect();
        let settings = read_user_settings(files);
        poms.into_iter()
            .map(|(path, text)| {
                let name = pom::parse(&text).map_or_else(
                    |_| {
                        path.parent()
                            .and_then(Path::file_name)
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    },
                    |project| project.name,
                );
                let repositories = repositories_with(files, &path, &text, &settings);
                let (dependencies, error) = match declared_in(files, &path, &text, &repositories, &own) {
                    Ok(dependencies) => (dependencies, None),
                    Err(error) => (Vec::new(), Some(error)),
                };
                DependencyModule {
                    id: relative(files.root, &path),
                    name,
                    file: path,
                    provider: KIND.to_owned(),
                    scopes: SCOPES.iter().map(|scope| (*scope).to_owned()).collect(),
                    repositories,
                    dependencies,
                    error,
                }
            })
            .collect()
    }

    fn change(&self, files: &BuildFiles<'_>, change: &DependencyChange) -> Result<Vec<FileChange>, String> {
        match change {
            DependencyChange::Add { module, group, artifact, version, scope } => {
                add(files, &files.root.join(module), group, artifact, version, scope)
            }
            DependencyChange::SetVersion { module, group, artifact, version } => {
                set_version(files, &files.root.join(module), group, artifact, version)
            }
            DependencyChange::Remove { module, group, artifact, scope } => {
                let path = files.root.join(module);
                let text = files.read(&path).ok_or_else(|| t!("workbench-deps-no-module", module = module.as_str()))?;
                Ok(new_change(path, pom_edit::remove(&text, group, artifact, scope.as_deref())?))
            }
            DependencyChange::AddBom { module, group, artifact, version } => {
                let path = files.root.join(module);
                let text = files.read(&path).ok_or_else(|| t!("workbench-deps-no-module", module = module.as_str()))?;
                Ok(new_change(path, pom_edit::add_bom(&text, &NewBom { group, artifact, version })?))
            }
            DependencyChange::AddRepository { module, id, name, url, snapshots } => {
                let path = files.root.join(module);
                let text = files.read(&path).ok_or_else(|| t!("workbench-deps-no-module", module = module.as_str()))?;
                let repository = NewRepository { id, name, url, snapshots: *snapshots };
                Ok(new_change(path, pom_edit::add_repository(&text, &repository)?))
            }
            DependencyChange::Exclude { .. } => Err(t!("workbench-deps-exclude-unsupported")),
        }
    }

    fn sites(&self, files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<VersionSite> {
        let Ok(document) = Document::parse(text) else { return Vec::new() };
        let root = document.root_element();
        let repositories = with_settings(Vec::new(), &read_user_settings(files));
        let layers = lineage(files, path, text, &repositories);
        let properties = merged_properties(&layers);
        let managed = child(root, "dependencyManagement").and_then(|management| child(management, "dependencies"));
        let mut sites = Vec::new();
        for list in child(root, "dependencies").into_iter().chain(managed) {
            for node in children(list, "dependency") {
                let Some(range) = pom_edit::version_range(node) else { continue };
                let written = &text[range.clone()];
                let version = interpolate(written, &properties);
                let value = |name: &str| interpolate(&text_of(node, name), &properties);
                if version.contains("${") || written.contains("project.") {
                    continue;
                }
                sites.push(VersionSite {
                    group: value("groupId"),
                    artifact: value("artifactId"),
                    version,
                    start: range.start,
                    end: range.end,
                });
            }
        }
        sites
    }

    fn completing(&self, _path: &Path, text: &str, caret: usize) -> Option<Completing> {
        pom_completing(text, caret)
    }

    fn repositories_for(&self, files: &BuildFiles<'_>, path: &Path) -> Vec<Repository> {
        let text = files.read(path).unwrap_or_default();
        repositories_of(files, path, &text)
    }

    fn offline(&self, files: &BuildFiles<'_>) -> bool {
        MavenSettings::read(files.env, files.root).offline || read_user_settings(files).offline
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        ReloadProject.run(session, ReloadParams::default()).map(|_| ())
    }
}
