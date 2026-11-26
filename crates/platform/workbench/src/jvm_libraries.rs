use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::model::{Library, LibraryKind, ModelEnv, ModelProvider, ProjectModel};

use crate::build_tools::{BuildEnv, LoadState};
use crate::ide_settings::zip::{Archive, Limits};
use crate::java::class_file;
use crate::java_run::{BuildTool, build_tool};
use crate::virtual_documents::{self, VirtualDocument, VirtualKind};

const LIMITS: Limits =
    Limits { archive: 1024 * 1024 * 1024, entries: 65_535, entry: 256 * 1024 * 1024, total: 4 * 1024 * 1024 * 1024 };

const WATCHED: &[&str] =
    &["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts", "libs.versions.toml"];

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Coordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub classifier: String,
}

impl Coordinates {
    pub fn new(group: &str, artifact: &str, version: &str) -> Self {
        Self {
            group_id: group.to_owned(),
            artifact_id: artifact.to_owned(),
            version: version.to_owned(),
            classifier: String::new(),
        }
    }

    pub fn text(&self) -> String {
        let mut text = format!("{}:{}:{}", self.group_id, self.artifact_id, self.version);
        if !self.classifier.is_empty() {
            text = format!("{text}:{}", self.classifier);
        }
        text
    }

    fn file_name(&self, classifier: &str) -> String {
        match classifier {
            "" => format!("{}-{}.jar", self.artifact_id, self.version),
            classifier => format!("{}-{}-{classifier}.jar", self.artifact_id, self.version),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub maven: PathBuf,
    pub gradle: PathBuf,
    pub cache: PathBuf,
}

impl Places {
    pub fn of(env: &BuildEnv) -> Self {
        let home = env.home();
        let cache = env.data_dir().map_or_else(virtual_documents::folder, |data| data.join("caches").join("libraries"));
        Self { maven: maven_repository(&home), gradle: gradle_files(&home), cache }
    }

    fn jars(&self) -> PathBuf {
        self.cache.join("jars")
    }
}

pub fn maven_repository(home: &Path) -> PathBuf {
    std::fs::read_to_string(home.join(".m2/settings.xml"))
        .ok()
        .and_then(|settings| {
            let document = roxmltree::Document::parse(&settings).ok()?;
            let node = document.descendants().find(|node| node.has_tag_name("localRepository"))?;
            let local = node.text().map(str::trim).unwrap_or_default();
            (!local.is_empty()).then(|| PathBuf::from(local.replace("${user.home}", &home.to_string_lossy())))
        })
        .unwrap_or_else(|| home.join(".m2/repository"))
}

pub fn gradle_files(home: &Path) -> PathBuf {
    std::env::var_os("GRADLE_USER_HOME")
        .filter(|value| !value.is_empty())
        .map_or_else(|| home.join(".gradle"), PathBuf::from)
        .join("caches/modules-2/files-2.1")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub coordinates: Coordinates,
    pub tool: BuildTool,
    pub jar: Option<PathBuf>,
}

pub trait LibrarySource: Send + Sync {
    fn list(&self, root: &Path, env: &BuildEnv, places: &Places) -> Option<Vec<Listed>>;
}

pub struct MavenTree;

impl LibrarySource for MavenTree {
    fn list(&self, root: &Path, env: &BuildEnv, _places: &Places) -> Option<Vec<Listed>> {
        let load = env.load(&crate::maven::tree_key(root)).filter(|load| load.state == LoadState::Loaded)?;
        let mut found = Vec::new();
        fn walk(node: &crate::maven::tree::TreeNode, found: &mut Vec<Listed>) {
            for child in &node.children {
                if child.kind != "pom" {
                    let mut coordinates = Coordinates::new(&child.group_id, &child.artifact_id, &child.version);
                    coordinates.classifier = child.classifier.clone();
                    found.push(Listed { coordinates, tool: BuildTool::Maven, jar: None });
                }
                walk(child, found);
            }
        }
        for project in crate::maven::tree::parse(&load.output) {
            walk(&project, &mut found);
        }
        Some(found)
    }
}

pub struct GradleResolved;

pub(crate) fn gradle_key(root: &Path) -> String {
    format!("gradle.libraries\n{}", root.display())
}

pub fn gradle_libraries(output: &str) -> Vec<Listed> {
    output
        .lines()
        .filter_map(|line| {
            let (coordinates, jar) = line.trim_end().strip_prefix("zen-library ")?.split_once('\t')?;
            let parts: Vec<&str> = coordinates.split(':').collect();
            let [group, artifact, version] = parts.as_slice() else { return None };
            let jar = PathBuf::from(jar);
            let name = jar.file_name()?.to_string_lossy().into_owned();
            let mut coordinates = Coordinates::new(group, artifact, version);
            let stem = name.strip_suffix(".jar")?;
            if let Some(classifier) = stem.strip_prefix(&format!("{artifact}-{version}-")) {
                coordinates.classifier = classifier.to_owned();
            }
            Some(Listed { coordinates, tool: BuildTool::Gradle, jar: Some(jar) })
        })
        .collect()
}

impl LibrarySource for GradleResolved {
    fn list(&self, root: &Path, env: &BuildEnv, _places: &Places) -> Option<Vec<Listed>> {
        let load = env.load(&gradle_key(root)).filter(|load| load.state == LoadState::Loaded)?;
        Some(gradle_libraries(&load.output))
    }
}

pub const GRADLE_LIBRARIES_INIT: &str = r#"allprojects { p ->
    p.tasks.register('zenLibraries') {
        doLast {
            ['compileClasspath', 'runtimeClasspath', 'testCompileClasspath', 'testRuntimeClasspath'].each { name ->
                def configuration = p.configurations.findByName(name)
                if (configuration != null && configuration.canBeResolved) {
                    configuration.incoming.artifactView { lenient = true }.artifacts.each { artifact ->
                        def id = artifact.id.componentIdentifier
                        if (id instanceof org.gradle.api.artifacts.component.ModuleComponentIdentifier) {
                            println 'zen-library ' + id.group + ':' + id.module + ':' + id.version + '\t' + artifact.file
                        }
                    }
                }
            }
        }
    }
}
"#;

pub(crate) fn load_gradle_libraries(session: &mut Session, env: &BuildEnv, root: &Path) -> Result<(), String> {
    let script = crate::gradle::init_script("zen-libraries", GRADLE_LIBRARIES_INIT)?;
    let tasks = format!("-q --init-script {} zenLibraries", script.display());
    let configuration = crate::gradle::GradleRun::tasks("libraries", ".", &tasks);
    let line = crate::gradle::GradleRun::new(env.clone()).command_line(&configuration, root)?;
    let task = session.state_mut::<crate::tasks::Tasks>().start(&t!("workbench-library-gradle-task"), "gradle", false);
    env.spawn(&gradle_key(root), line, Some(task), |output| Ok(output.to_owned()));
    Ok(())
}

pub struct Declared;

impl LibrarySource for Declared {
    fn list(&self, root: &Path, _env: &BuildEnv, places: &Places) -> Option<Vec<Listed>> {
        match build_tool(root) {
            BuildTool::Maven => Some(declared_maven(&crate::maven::pom::read(&root.join("pom.xml")), places)),
            BuildTool::Gradle => Some(declared_gradle(root)),
            BuildTool::Javac => None,
        }
    }
}

fn declared_maven(project: &crate::maven::pom::PomProject, places: &Places) -> Vec<Listed> {
    let own: BTreeSet<(String, String)> = std::iter::once(project)
        .chain(project.modules.iter())
        .map(|module| (module.group_id.clone(), module.artifact_id.clone()))
        .collect();
    let mut found = Vec::new();
    let mut projects = vec![project];
    while let Some(project) = projects.pop() {
        projects.extend(project.modules.iter());
        for dependency in &project.dependencies {
            if own.contains(&(dependency.group_id.clone(), dependency.artifact_id.clone())) {
                continue;
            }
            let version = if dependency.version.is_empty() || dependency.version.contains("${") {
                newest_version(&places.maven, &dependency.group_id, &dependency.artifact_id)
            } else {
                Some(dependency.version.clone())
            };
            if let Some(version) = version {
                let coordinates = Coordinates::new(&dependency.group_id, &dependency.artifact_id, &version);
                found.push(Listed { coordinates, tool: BuildTool::Maven, jar: None });
            }
        }
    }
    found
}

fn newest_version(repository: &Path, group: &str, artifact: &str) -> Option<String> {
    let folder = group.split('.').fold(repository.to_owned(), |path, part| path.join(part)).join(artifact);
    let mut versions: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    versions.sort_by(|a, b| compare_versions(a, b));
    versions.pop()
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |text: &str| -> Vec<(u64, String)> {
        text.split(['.', '-'])
            .map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                (digits.parse().unwrap_or(0), part[digits.len()..].to_owned())
            })
            .collect()
    };
    parts(a).cmp(&parts(b))
}

fn declared_gradle(root: &Path) -> Vec<Listed> {
    let mut files: Vec<PathBuf> = ["build.gradle", "build.gradle.kts"].iter().map(|name| root.join(name)).collect();
    for project in crate::gradle::model::projects(root) {
        files.extend(project.build_file);
    }
    files.sort();
    files.dedup();
    let mut found = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for coordinates in gradle_coordinates(&text) {
            found.push(Listed { coordinates, tool: BuildTool::Gradle, jar: None });
        }
    }
    found
}

pub fn gradle_coordinates(text: &str) -> Vec<Coordinates> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(['"', '\'']) {
        let quote = rest[at..].chars().next().unwrap_or('"');
        let after = &rest[at + 1..];
        let Some(end) = after.find(quote) else { break };
        let literal = &after[..end];
        rest = &after[end + 1..];
        let parts: Vec<&str> = literal.split(':').collect();
        let plain = |part: &&str| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        };
        if (3..=4).contains(&parts.len()) && parts.iter().all(plain) {
            let mut coordinates = Coordinates::new(parts[0], parts[1], parts[2]);
            coordinates.classifier = parts.get(3).map(|part| (*part).to_owned()).unwrap_or_default();
            found.push(coordinates);
        }
    }
    found
}

pub fn jar_of(places: &Places, coordinates: &Coordinates, classifier: &str) -> Option<PathBuf> {
    let name = coordinates.file_name(classifier);
    let maven = coordinates
        .group_id
        .split('.')
        .fold(places.maven.clone(), |path, part| path.join(part))
        .join(&coordinates.artifact_id)
        .join(&coordinates.version)
        .join(&name);
    if maven.is_file() {
        return Some(maven);
    }
    let gradle = places.gradle.join(&coordinates.group_id).join(&coordinates.artifact_id).join(&coordinates.version);
    std::fs::read_dir(gradle).ok()?.flatten().map(|entry| entry.path().join(&name)).find(|path| path.is_file())
}

pub fn sources_of(jar: &Path) -> Option<PathBuf> {
    let name = jar.file_name()?.to_string_lossy();
    let stem = name.strip_suffix(".jar")?;
    let wanted = format!("{stem}-sources.jar");
    let beside = jar.with_file_name(&wanted);
    if beside.is_file() {
        return Some(beside);
    }
    let version = jar.parent()?.parent()?;
    std::fs::read_dir(version).ok()?.flatten().map(|entry| entry.path().join(&wanted)).find(|path| path.is_file())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Origin {
    archive: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinates: Option<Coordinates>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(default)]
    sources: bool,
}

fn folder_name(archive: &Path, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    format!("{safe}-{}", &virtual_documents::hashed(&archive.to_string_lossy())[..8])
}

static UNPACKING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

pub fn unpack(archive: &Path, root: &Path, origin_json: &str, class_placeholders: bool) -> Result<(), String> {
    unpack_until(archive, root, origin_json, class_placeholders, &AtomicBool::new(false))
}

fn unpack_until(
    archive: &Path,
    root: &Path,
    origin_json: &str,
    class_placeholders: bool,
    stop: &AtomicBool,
) -> Result<(), String> {
    let bytes = std::fs::read(archive).map_err(|error| format!("{}: {error}", archive.display()))?;
    let zip = Archive::parse(bytes, LIMITS).map_err(|error| format!("{}: {error}", archive.display()))?;
    let part = beside(root, &format!(".part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(|error| error.to_string())?;
    for entry in zip.entries() {
        if stop.load(Ordering::Relaxed) {
            let _ = std::fs::remove_dir_all(&part);
            return Err(format!("{}: stopped", part.display()));
        }
        if !part.is_dir() {
            return Err(format!("{}: removed while unpacking", part.display()));
        }
        let path = part.join(&entry.name);
        if entry.folder {
            std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
            continue;
        }
        if entry.link {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let data = if class_placeholders && entry.name.ends_with(".class") {
            Vec::new()
        } else {
            zip.read(entry).map_err(|error| format!("{}: {error}", archive.display()))?
        };
        std::fs::write(&path, data).map_err(|error| error.to_string())?;
    }
    std::fs::write(origin_file(root), origin_json).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(root);
    std::fs::rename(&part, root).map_err(|error| error.to_string())
}

fn beside(root: &Path, suffix: &str) -> PathBuf {
    let mut name = root.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    root.with_file_name(name)
}

fn origin_file(root: &Path) -> PathBuf {
    beside(root, ".origin.json")
}

pub type Unpacking = (PathBuf, PathBuf, String, bool);

pub fn is_unpacking(root: &Path) -> bool {
    UNPACKING.lock().unwrap_or_else(PoisonError::into_inner).contains(root)
}

#[derive(Default)]
struct Unpacks {
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl Drop for Unpacks {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let threads = std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

fn unpack_later(work: Vec<Unpacking>, cache: &Path, unpacks: &Unpacks) {
    let owner = cache.parent().unwrap_or(cache).to_owned();
    let mut queued = Vec::new();
    {
        let mut unpacking = UNPACKING.lock().unwrap_or_else(PoisonError::into_inner);
        for item in work {
            if unpacking.insert(item.1.clone()) {
                queued.push(item);
            }
        }
    }
    if queued.is_empty() {
        return;
    }
    let stop = Arc::clone(&unpacks.stop);
    let thread = std::thread::spawn(move || {
        for (archive, root, origin, placeholders) in queued {
            if owner.is_dir() && !stop.load(Ordering::Relaxed) {
                if let Some(parent) = root.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = unpack_until(&archive, &root, &origin, placeholders, &stop);
            }
            UNPACKING.lock().unwrap_or_else(PoisonError::into_inner).remove(&root);
        }
    });
    let mut threads = unpacks.threads.lock().unwrap_or_else(PoisonError::into_inner);
    threads.retain(|thread| !thread.is_finished());
    threads.push(thread);
}

pub fn jdk_version(home: &Path) -> Option<String> {
    let release = std::fs::read_to_string(home.join("release")).ok()?;
    let value = release.lines().find_map(|line| line.strip_prefix("JAVA_VERSION="))?;
    Some(value.trim().trim_matches('"').to_owned())
}

fn jdk_sources(home: &Path) -> Option<PathBuf> {
    [home.join("lib/src.zip"), home.join("src.zip")].into_iter().find(|path| path.is_file())
}

pub struct JvmLibraries {
    env: BuildEnv,
    sources: Vec<Box<dyn LibrarySource>>,
    places: Option<Places>,
    unpacks: Unpacks,
}

impl JvmLibraries {
    pub fn new(env: BuildEnv) -> Self {
        Self {
            env,
            sources: vec![Box::new(MavenTree), Box::new(GradleResolved), Box::new(Declared)],
            places: None,
            unpacks: Unpacks::default(),
        }
    }

    pub fn with_places(env: BuildEnv, places: Places, sources: Vec<Box<dyn LibrarySource>>) -> Self {
        Self { env, sources, places: Some(places), unpacks: Unpacks::default() }
    }

    pub fn jars(&self, root: &Path) -> Vec<PathBuf> {
        if build_tool(root) == BuildTool::Javac {
            return Vec::new();
        }
        let places = self.places.clone().unwrap_or_else(|| Places::of(&self.env));
        let listed = self.sources.iter().find_map(|source| source.list(root, &self.env, &places)).unwrap_or_default();
        let mut seen = BTreeSet::new();
        listed
            .into_iter()
            .filter_map(|Listed { coordinates, jar, .. }| {
                jar.filter(|jar| jar.is_file()).or_else(|| jar_of(&places, &coordinates, &coordinates.classifier))
            })
            .filter(|jar| seen.insert(jar.clone()))
            .collect()
    }

    pub fn libraries(&self, root: &Path) -> (Vec<Library>, Vec<Unpacking>) {
        let places = self.places.clone().unwrap_or_else(|| Places::of(&self.env));
        let mut libraries = Vec::new();
        let mut work = Vec::new();
        if let Some(home) = self.env.java_home(root) {
            let version = jdk_version(&home).unwrap_or_default();
            let library_root = match jdk_sources(&home) {
                Some(zip) => {
                    let unpacked = places.cache.join("jdk").join(folder_name(&home, &format!("jdk-{version}")));
                    let origin = Origin { archive: zip.clone(), sources: true, ..Origin::default() };
                    work.push((zip, unpacked.clone(), json(&origin), false));
                    unpacked
                }
                None => home.clone(),
            };
            libraries.push(Library {
                kind: LibraryKind::Sdk,
                name: "JDK".to_owned(),
                version: Some(version.clone()),
                exists: library_root.is_dir(),
                root: library_root,
                group: None,
                label: Some(format!("< {version} >")),
            });
        }
        let listed = self.sources.iter().find_map(|source| source.list(root, &self.env, &places)).unwrap_or_default();
        let mut seen = BTreeSet::new();
        for Listed { coordinates, tool, jar } in listed {
            if !seen.insert(coordinates.clone()) {
                continue;
            }
            let tool_name = if tool == BuildTool::Gradle { "Gradle" } else { "Maven" };
            let label = format!("{tool_name}: {}", coordinates.text());
            let jar =
                jar.filter(|jar| jar.is_file()).or_else(|| jar_of(&places, &coordinates, &coordinates.classifier));
            let library_root = match &jar {
                Some(jar) => {
                    let unpacked = places.jars().join(folder_name(jar, &coordinates.text()));
                    let origin = Origin {
                        archive: jar.clone(),
                        coordinates: Some(coordinates.clone()),
                        tool: Some(tool_name.to_owned()),
                        sources: false,
                    };
                    if !unpacked.is_dir() {
                        work.push((jar.clone(), unpacked.clone(), json(&origin), true));
                    }
                    unpacked
                }
                None => PathBuf::new(),
            };
            libraries.push(Library {
                kind: LibraryKind::Dependency,
                name: format!("{}:{}", coordinates.group_id, coordinates.artifact_id),
                version: Some(coordinates.version.clone()),
                exists: library_root.is_dir(),
                root: library_root,
                group: Some(String::new()),
                label: Some(label),
            });
        }
        work.retain(|(_, root, _, _)| !root.is_dir());
        (libraries, work)
    }
}

fn json(origin: &Origin) -> String {
    serde_json::to_string(origin).unwrap_or_default()
}

impl ModelProvider for JvmLibraries {
    fn id(&self) -> &str {
        "jvm"
    }

    fn watches(&self, file_name: &str) -> bool {
        WATCHED.contains(&file_name)
    }

    fn generation(&self, root: &Path) -> u64 {
        [crate::maven::tree_key(root), gradle_key(root)]
            .iter()
            .filter_map(|key| self.env.load(key))
            .map(|load| {
                let state = match load.state {
                    LoadState::Loading => 1,
                    LoadState::Loaded => 2,
                    LoadState::Failed => 3,
                };
                ((load.output.len() as u64) << 2) | state
            })
            .fold(0, |sum, one| sum.wrapping_mul(31).wrapping_add(one))
    }

    fn load(&self, root: &Path, _env: &ModelEnv) -> Option<ProjectModel> {
        if build_tool(root) == BuildTool::Javac {
            return None;
        }
        let (libraries, work) = self.libraries(root);
        let cache = self.places.clone().unwrap_or_else(|| Places::of(&self.env)).cache;
        unpack_later(work, &cache, &self.unpacks);
        Some(ProjectModel { libraries, modules: crate::jvm_modules::modules(root), ..ProjectModel::default() })
    }
}

fn unpacked_origin(path: &Path) -> Option<(Origin, String)> {
    for root in path.ancestors().skip(1) {
        let file = origin_file(root);
        if let Ok(text) = std::fs::read_to_string(&file) {
            let origin: Origin = serde_json::from_str(&text).ok()?;
            let entry = path.strip_prefix(root).ok()?;
            let entry = entry.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            return Some((origin, entry));
        }
    }
    None
}

fn read_entry(archive: &Path, name: &str) -> Option<Vec<u8>> {
    let zip = Archive::parse(std::fs::read(archive).ok()?, LIMITS).ok()?;
    let entry = zip.entries().iter().find(|entry| entry.name == name)?;
    zip.read(entry).ok()
}

pub fn opened_as(path: &Path, cache: &Path) -> Option<PathBuf> {
    if path.extension().is_none_or(|extension| extension != "class") {
        return None;
    }
    let origin = unpacked_origin(path);
    let bytes = match &origin {
        Some((origin, entry)) if !origin.sources => read_entry(&origin.archive, entry)?,
        _ => std::fs::read(path).ok()?,
    };
    if !class_file::is_class(&bytes) {
        return None;
    }
    let class = class_file::parse(&bytes).ok()?;
    let jar = origin.map(|(origin, _)| origin.archive);
    let (package, name) = class.name.rsplit_once('/').unwrap_or(("", &class.name));
    let outer = name.split('$').next().unwrap_or(name);
    let source_name = class.source_file.clone().unwrap_or_else(|| format!("{outer}.java"));
    let source_entry = if package.is_empty() { source_name.clone() } else { format!("{package}/{source_name}") };
    if let Some(jar) = &jar
        && let Some(sources) = sources_of(jar)
        && let Some(text) = read_entry(&sources, &source_entry)
    {
        let folder = folder_name(&sources, "sources");
        let relative = Path::new("sources").join(folder).join(&source_entry);
        let document = VirtualDocument {
            kind: VirtualKind::Sources,
            origin: class.name.replace('/', "."),
            library: Some(jar.clone()),
            bytecode: Some(class.major),
            class_path: Some(path.to_owned()),
        };
        return virtual_documents::store(cache, &relative, &String::from_utf8_lossy(&text), document, None).ok();
    }
    let notice = t!("workbench-library-decompiled-notice", class = class.name.replace(['/', '$'], "."));
    let text = class_file::stub(&class, &notice);
    let key = virtual_documents::hashed(&jar.as_deref().unwrap_or(path).to_string_lossy());
    let file = if package.is_empty() { format!("{outer}.java") } else { format!("{package}/{outer}.java") };
    let file = if name.contains('$') { file.replace(&format!("{outer}.java"), &format!("{name}.java")) } else { file };
    let relative = Path::new("decompiled").join(key).join(file);
    let document = VirtualDocument {
        kind: VirtualKind::Decompiled,
        origin: class.name.replace('/', "."),
        library: jar,
        bytecode: Some(class.major),
        class_path: Some(path.to_owned()),
    };
    virtual_documents::store(cache, &relative, &text, document, None).ok()
}

pub fn coordinates_of(jar: &Path, places: &Places) -> Option<Coordinates> {
    if let Ok(rest) = jar.strip_prefix(&places.gradle) {
        let parts: Vec<String> =
            rest.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        if parts.len() == 5 {
            return Some(Coordinates::new(&parts[0], &parts[1], &parts[2]));
        }
    }
    let rest = jar.strip_prefix(&places.maven).ok()?;
    let parts: Vec<String> = rest.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    let [group @ .., artifact, version, _file] = parts.as_slice() else { return None };
    (!group.is_empty()).then(|| Coordinates::new(&group.join("."), artifact, version))
}

fn sources_key(root: &Path) -> String {
    format!("libraries.sources\n{}", root.display())
}

pub const GRADLE_SOURCES_INIT: &str = r#"allprojects { p ->
    p.tasks.register('zenDownloadSources') {
        doLast {
            def wanted = (p.findProperty('zenSourcesOf') ?: '').toString().split(',').findAll { it }
            def ids = []
            p.configurations.findAll { it.canBeResolved }.each { configuration ->
                try {
                    configuration.incoming.resolutionResult.allDependencies.each { dependency ->
                        if (dependency instanceof org.gradle.api.artifacts.result.ResolvedDependencyResult) {
                            def id = dependency.selected.id
                            if (id instanceof org.gradle.api.artifacts.component.ModuleComponentIdentifier) {
                                if (wanted.isEmpty() || wanted.contains(id.group + ':' + id.module)) { ids << id }
                            }
                        }
                    }
                } catch (Exception ignored) {
                }
            }
            def result = p.dependencies.createArtifactResolutionQuery()
                .forComponents(ids.unique())
                .withArtifacts(org.gradle.jvm.JvmLibrary, org.gradle.language.base.artifact.SourcesArtifact)
                .execute()
            result.resolvedComponents.each { component ->
                component.getArtifacts(org.gradle.language.base.artifact.SourcesArtifact).each { artifact ->
                    if (artifact instanceof org.gradle.api.artifacts.result.ResolvedArtifactResult) {
                        println 'zen-sources ' + artifact.file
                    }
                }
            }
        }
    }
}
"#;

/// The parameters of `java.libraries.downloadSources`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadParams {
    /// A library jar, a file under External Libraries, or a decompiled class's document: only that
    /// library's sources. Every library's when absent.
    pub path: Option<PathBuf>,
}

/// What `java.libraries.downloadSources` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadStarted {
    /// The build tool that downloads them: `maven` or `gradle`.
    pub tool: String,
    /// The library asked for; absent for every library.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,
    /// The background task that downloads them (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

fn library_of(path: &Path) -> Option<(PathBuf, Option<Coordinates>)> {
    if path.extension().is_some_and(|extension| extension == "jar") {
        return Some((path.to_owned(), None));
    }
    if let Some(document) = virtual_documents::get(path) {
        return document.library.map(|jar| (jar, None));
    }
    let inside = if path.is_dir() { path.join("_") } else { path.to_owned() };
    unpacked_origin(&inside)
        .filter(|(origin, _)| !origin.sources)
        .map(|(origin, _)| (origin.archive, origin.coordinates))
}

pub fn download_line(
    env: &BuildEnv,
    root: &Path,
    tool: BuildTool,
    only: Option<&Coordinates>,
    init_script: &Path,
) -> Result<zen_run::CommandLine, String> {
    match tool {
        BuildTool::Maven => {
            let configuration = crate::maven::MavenRun::goals("sources", "pom.xml", "dependency:sources", "");
            let mut line = crate::maven::MavenRun::new(env.clone()).command_line(&configuration, root)?;
            let mut extra = vec!["-B".to_owned()];
            if let Some(only) = only {
                extra.push(format!("-DincludeGroupIds={}", only.group_id));
                extra.push(format!("-DincludeArtifactIds={}", only.artifact_id));
            }
            line.display = format!("{} {}", line.display, extra.join(" "));
            line.args.extend(extra);
            Ok(line)
        }
        BuildTool::Gradle => {
            let mut tasks = format!("-q --init-script {} zenDownloadSources", init_script.display());
            if let Some(only) = only {
                tasks = format!("{tasks} -PzenSourcesOf={}:{}", only.group_id, only.artifact_id);
            }
            let configuration = crate::gradle::GradleRun::tasks("sources", ".", &tasks);
            crate::gradle::GradleRun::new(env.clone()).command_line(&configuration, root)
        }
        BuildTool::Javac => Err(t!("workbench-library-no-build-tool")),
    }
}

pub struct DownloadSources;

impl Command for DownloadSources {
    type Params = DownloadParams;
    type Output = DownloadStarted;
    const ID: &'static str = "java.libraries.downloadSources";
    const TITLE: &'static str = "Download Sources";
    const DESCRIPTION: &'static str = "Downloads the sources of the project's Maven or Gradle libraries through the \
         build tool, in a background task (it uses the network): mvn dependency:sources, or a Gradle task that \
         resolves the sources artifacts. With `path` (a jar, a file under External Libraries, or a decompiled \
         class's document) only that library's. Afterwards, opening a class shows its sources instead of the \
         decompiled view. Never runs on its own.";

    fn run(&self, session: &mut Session, params: DownloadParams) -> Result<DownloadStarted, CommandError> {
        let root = crate::run::project_root(session)?;
        crate::trust::guard_build_load(session, &root)?;
        let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
        let tool = build_tool(&root);
        let places = Places::of(&env);
        let only = match &params.path {
            Some(path) => {
                let (jar, known) = library_of(path).ok_or_else(|| {
                    CommandError::new(t!("workbench-library-not-a-library", path = path.display().to_string()))
                })?;
                Some(known.or_else(|| coordinates_of(&jar, &places)).ok_or_else(|| {
                    CommandError::new(t!("workbench-library-not-a-library", path = jar.display().to_string()))
                })?)
            }
            None => None,
        };
        let script = if tool == BuildTool::Gradle {
            crate::gradle::init_script("zen-library-sources", GRADLE_SOURCES_INIT).map_err(CommandError::new)?
        } else {
            PathBuf::new()
        };
        let line = download_line(&env, &root, tool, only.as_ref(), &script).map_err(CommandError::new)?;
        let title = match &only {
            Some(only) => t!("workbench-library-sources-task-one", library = only.text()),
            None => t!("workbench-library-sources-task"),
        };
        let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "libraries", false);
        let id = task.id();
        env.spawn(&sources_key(&root), line, Some(task), |output| Ok(output.to_owned()));
        let tool = if tool == BuildTool::Gradle { "gradle" } else { "maven" };
        Ok(DownloadStarted { tool: tool.to_owned(), coordinates: only.map(|only| only.text()), task: Some(id) })
    }
}

/// What `java.libraries.document` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDocument {
    /// Whether it shows a library's sources or a decompiled class.
    pub kind: VirtualKind,
    /// The class or the language server's URI it shows.
    pub origin: String,
    /// The library's jar, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<PathBuf>,
    /// The class file's version, e.g. 65 for Java 21.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytecode: Option<u16>,
    /// The Java release of that version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java: Option<u16>,
    /// The class file it was opened from, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_path: Option<PathBuf>,
    /// Whether a sources download for the project runs now.
    pub downloading: bool,
    /// Whether its library's sources are there now (opening `classPath` again shows them).
    pub sources_found: bool,
}

/// Which document.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DocumentParams {
    /// The document; the active one's when absent.
    pub path: Option<PathBuf>,
}

pub struct DocumentInfo;

impl Command for DocumentInfo {
    type Params = DocumentParams;
    type Output = Option<LibraryDocument>;
    const ID: &'static str = "java.libraries.document";
    const TITLE: &'static str = "Library Document";
    const DESCRIPTION: &'static str = "Whether a document (the active one when `path` is absent) is a library class \
         shown read-only: its library's sources or a decompiled view of its class file, the class, its library's \
         jar and bytecode version, and whether a sources download runs. Null for any other document.";

    fn run(&self, session: &mut Session, params: DocumentParams) -> Result<Option<LibraryDocument>, CommandError> {
        let path = match params.path {
            Some(path) => path,
            None => match session.state::<crate::Workbench>().and_then(crate::Workbench::active_document) {
                Some(document) => document.path().to_owned(),
                None => return Ok(None),
            },
        };
        let Some(document) = virtual_documents::get(&path) else { return Ok(None) };
        let downloading = crate::run::project_root(session).ok().is_some_and(|root| {
            let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
            env.load(&sources_key(&root)).is_some_and(|load| load.state == LoadState::Loading)
        });
        let sources_found = document.library.as_deref().and_then(sources_of).is_some();
        Ok(Some(LibraryDocument {
            kind: document.kind,
            origin: document.origin,
            library: document.library,
            java: document.bytecode.map(class_file::java_version),
            bytecode: document.bytecode,
            class_path: document.class_path,
            downloading,
            sources_found,
        }))
    }
}

pub fn set_cache(env: &BuildEnv) {
    virtual_documents::set_folder(Some(Places::of(env).cache));
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(DownloadSources)?;
    registry.register(DocumentInfo)?;
    Ok(())
}

#[cfg(test)]
mod tests;
