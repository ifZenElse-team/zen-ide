use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use super::BuildEnv;

/// Where a build tool's program was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    /// The project's own wrapper script (`mvnw`, `gradlew`).
    ProjectWrapper,
    /// The distribution a wrapper left on this computer (`~/.m2/wrapper/dists`, `~/.gradle/wrapper/dists`).
    WrapperDistribution,
    /// A folder on `PATH`.
    Path,
    /// SDKMAN!.
    Sdkman,
    /// Downloaded by the IDE.
    Downloaded,
    /// The home folder the settings name.
    Custom,
}

/// The program a build tool runs with, as the commands show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Executable {
    /// The program.
    pub path: PathBuf,
    /// Whether it is the project's wrapper.
    pub wrapper: bool,
    /// Where it was found.
    pub source: ToolSource,
    /// Its version, when the files say it; a wrapper script does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    pub candidate: &'static str,
    wrapper: &'static str,
    program: &'static str,
    program_on_windows: &'static str,
    folder_prefix: &'static str,
    version_jar: &'static str,
    wrapper_properties: &'static str,
}

pub const MAVEN: Tool = Tool {
    candidate: "maven",
    wrapper: "mvnw",
    program: "mvn",
    program_on_windows: "mvn.cmd",
    folder_prefix: "apache-maven-",
    version_jar: "maven-core-",
    wrapper_properties: ".mvn/wrapper/maven-wrapper.properties",
};

pub const GRADLE: Tool = Tool {
    candidate: "gradle",
    wrapper: "gradlew",
    program: "gradle",
    program_on_windows: "gradle.bat",
    folder_prefix: "gradle-",
    version_jar: "gradle-core-",
    wrapper_properties: "gradle/wrapper/gradle-wrapper.properties",
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distribution {
    pub program: PathBuf,
    pub version: String,
}

impl Distribution {
    fn executable(self) -> Executable {
        Executable {
            path: self.program,
            wrapper: false,
            source: ToolSource::WrapperDistribution,
            version: Some(self.version),
        }
    }
}

impl Tool {
    fn program_name(&self) -> &'static str {
        if cfg!(windows) { self.program_on_windows } else { self.program }
    }

    pub fn project_wrapper(&self, folder: &Path, root: &Path) -> Option<Executable> {
        super::wrapper(folder, root, self.wrapper).map(|path| Executable {
            path,
            wrapper: true,
            source: ToolSource::ProjectWrapper,
            version: None,
        })
    }

    pub fn custom(&self, home_path: &str) -> Option<Executable> {
        let home = home_path.trim();
        let program = PathBuf::from(home).join("bin").join(self.program_name());
        (!home.is_empty() && program.is_file()).then(|| Executable {
            version: self.version_of(&program),
            path: program,
            wrapper: false,
            source: ToolSource::Custom,
        })
    }

    fn on_a_path(&self, env: &BuildEnv, folders: &[PathBuf]) -> Option<Executable> {
        let path = zen_lsp::servers::find_executable(self.program, folders)?;
        Some(Executable { version: self.version_of(&path), source: source_of(env, &path), path, wrapper: false })
    }

    pub fn installed(
        &self,
        env: &BuildEnv,
        folders: &[PathBuf],
        distributions: Vec<Distribution>,
    ) -> Option<Executable> {
        self.on_a_path(env, folders).or_else(|| distributions.into_iter().next().map(Distribution::executable))
    }

    pub fn found(
        &self,
        env: &BuildEnv,
        folder: &Path,
        root: &Path,
        folders: &[PathBuf],
        distributions: Vec<Distribution>,
    ) -> Option<Executable> {
        if let Some(wrapper) = self.project_wrapper(folder, root) {
            return Some(wrapper);
        }
        let pinned = self.wrapper_version(root);
        let chosen = pinned.and_then(|version| distributions.iter().find(|found| found.version == version).cloned());
        chosen.map(Distribution::executable).or_else(|| self.installed(env, folders, distributions))
    }

    pub fn wrapper_version(&self, root: &Path) -> Option<String> {
        let text = std::fs::read_to_string(root.join(self.wrapper_properties)).ok()?;
        let url = text.lines().find_map(|line| line.trim().strip_prefix("distributionUrl")?.split_once('='))?.1;
        let file = url.trim().rsplit('/').next()?;
        self.version_in(file.strip_suffix(".zip").unwrap_or(file))
    }

    fn version_in(&self, name: &str) -> Option<String> {
        let rest = name.strip_prefix(self.folder_prefix)?;
        let version = rest.strip_suffix("-bin").or_else(|| rest.strip_suffix("-all")).unwrap_or(rest);
        version.starts_with(|c: char| c.is_ascii_digit()).then(|| version.to_owned())
    }

    fn version_of(&self, program: &Path) -> Option<String> {
        let home = std::fs::canonicalize(program).ok()?.parent()?.parent()?.to_owned();
        std::fs::read_dir(home.join("lib")).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name.strip_prefix(self.version_jar)?.strip_suffix(".jar")?;
            version.starts_with(|c: char| c.is_ascii_digit()).then(|| version.to_owned())
        })
    }

    fn complete(&self, home: &Path, unpacked_in: &Path) -> bool {
        let finished = match std::fs::read_dir(unpacked_in) {
            Ok(entries) => entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect(),
            Err(_) => Vec::new(),
        };
        let marked = self != &GRADLE || finished.iter().any(|name| name.ends_with(".ok"));
        marked
            && !finished.iter().any(|name| name.ends_with(".part"))
            && runnable(&home.join("bin").join(self.program_name()))
            && self.has_its_jars(home)
    }

    fn has_its_jars(&self, home: &Path) -> bool {
        let has = |folder: &str, start: &str| {
            std::fs::read_dir(home.join(folder)).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with(start) && name.ends_with(".jar")
                })
            })
        };
        has("lib", self.version_jar) && (self != &MAVEN || has("boot", "plexus-classworlds"))
    }

    pub fn distributions(&self, dists: &Path) -> Vec<Distribution> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dists).into_iter().flatten().flatten() {
            let Some(version) = self.version_in(&entry.file_name().to_string_lossy()) else { continue };
            for unpacked in std::fs::read_dir(entry.path()).into_iter().flatten().flatten() {
                let unpacked = unpacked.path();
                let nested = std::fs::read_dir(&unpacked).into_iter().flatten().flatten().map(|child| child.path());
                let home = std::iter::once(unpacked.clone())
                    .chain(nested)
                    .find(|home| home.join("bin").join(self.program_name()).is_file());
                if let Some(home) = home.filter(|home| self.complete(home, &unpacked)) {
                    found.push(Distribution {
                        program: home.join("bin").join(self.program_name()),
                        version: version.clone(),
                    });
                }
            }
        }
        found.sort_by_key(|found| std::cmp::Reverse(version_key(&found.version)));
        found
    }
}

fn version_key(version: &str) -> (bool, Vec<u32>) {
    let release = version.chars().all(|c| c.is_ascii_digit() || c == '.');
    (release, version.split(['.', '-']).filter_map(|part| part.parse().ok()).collect())
}

fn source_of(env: &BuildEnv, path: &Path) -> ToolSource {
    if env.data_dir().is_some_and(|data| path.starts_with(data.join("tools"))) {
        ToolSource::Downloaded
    } else if path.components().any(|part| part.as_os_str() == ".sdkman") {
        ToolSource::Sdkman
    } else {
        ToolSource::Path
    }
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn maven_home(home: &Path, version: &str) -> PathBuf {
        let program = home.join("bin/mvn");
        write(&program, "#!/bin/sh\n");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        write(&home.join(format!("lib/maven-core-{version}.jar")), "");
        write(&home.join("boot/plexus-classworlds-2.9.0.jar"), "");
        program
    }

    fn gradle_home(unpacked: &Path, version: &str) -> PathBuf {
        let home = unpacked.join(format!("gradle-{version}"));
        let program = home.join("bin/gradle");
        write(&program, "#!/bin/sh\n");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        write(&home.join(format!("lib/gradle-core-{version}.jar")), "");
        write(&unpacked.join(format!("gradle-{version}-bin.zip.ok")), "");
        program
    }

    #[test]
    fn maven_distributions_of_both_wrapper_layouts_are_found_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let dists = dir.path();
        let script = maven_home(&dists.join("apache-maven-3.9.11/a2d47e15"), "3.9.11");
        let binary = maven_home(&dists.join("apache-maven-3.8.8-bin/6mqf5/apache-maven-3.8.8"), "3.8.8");
        let found = MAVEN.distributions(dists);
        assert_eq!(
            found,
            [
                Distribution { program: script, version: "3.9.11".to_owned() },
                Distribution { program: binary, version: "3.8.8".to_owned() },
            ]
        );
    }

    #[test]
    fn a_release_comes_before_a_newer_pre_release() {
        let dir = tempfile::tempdir().unwrap();
        maven_home(&dir.path().join("apache-maven-4.0.0-rc-3/a"), "4.0.0-rc-3");
        maven_home(&dir.path().join("apache-maven-3.10.0/b"), "3.10.0");
        maven_home(&dir.path().join("apache-maven-3.9.11/c"), "3.9.11");
        let versions: Vec<String> = MAVEN.distributions(dir.path()).into_iter().map(|found| found.version).collect();
        assert_eq!(versions, ["3.10.0", "3.9.11", "4.0.0-rc-3"]);
    }

    #[test]
    fn broken_distributions_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let dists = dir.path();
        let good = maven_home(&dists.join("apache-maven-3.8.8/good"), "3.8.8");
        maven_home(&dists.join("apache-maven-3.9.9/no-core"), "3.9.9");
        std::fs::remove_file(dists.join("apache-maven-3.9.9/no-core/lib/maven-core-3.9.9.jar")).unwrap();
        let locked = maven_home(&dists.join("apache-maven-3.9.10/not-runnable"), "3.9.10");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        maven_home(&dists.join("apache-maven-3.9.11/partial"), "3.9.11");
        write(&dists.join("apache-maven-3.9.11/partial/apache-maven-3.9.11-bin.zip.part"), "");
        std::fs::create_dir_all(dists.join("apache-maven-3.9.12/empty")).unwrap();
        std::fs::write(dists.join("apache-maven-3.9.13"), "a file").unwrap();
        write(&dists.join("CACHEDIR.TAG"), "");
        assert_eq!(MAVEN.distributions(dists), [Distribution { program: good, version: "3.8.8".to_owned() }]);
    }

    #[test]
    fn a_gradle_distribution_needs_the_mark_its_wrapper_writes_when_it_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let dists = dir.path();
        let done = gradle_home(&dists.join("gradle-8.10.2-bin/abc"), "8.10.2");
        let unfinished = dists.join("gradle-8.14-all/def");
        gradle_home(&unfinished, "8.14");
        std::fs::remove_file(unfinished.join("gradle-8.14-bin.zip.ok")).unwrap();
        assert_eq!(GRADLE.distributions(dists), [Distribution { program: done, version: "8.10.2".to_owned() }]);
    }

    #[test]
    fn the_wrapper_properties_name_the_version_for_maven_and_gradle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(MAVEN.wrapper_version(root), None);
        write(
            &root.join(".mvn/wrapper/maven-wrapper.properties"),
            "wrapperVersion=3.3.2\ndistributionType=only-script\ndistributionUrl=https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.9.11/apache-maven-3.9.11-bin.zip\n",
        );
        write(
            &root.join("gradle/wrapper/gradle-wrapper.properties"),
            "distributionBase=GRADLE_USER_HOME\ndistributionUrl=https\\://services.gradle.org/distributions/gradle-9.1.0-all.zip\n",
        );
        assert_eq!(MAVEN.wrapper_version(root).as_deref(), Some("3.9.11"));
        assert_eq!(GRADLE.wrapper_version(root).as_deref(), Some("9.1.0"));
    }

    #[test]
    fn the_wrapper_of_the_project_comes_first_then_the_version_it_names_then_path_then_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let dists = dir.path().join("dists");
        let newest = maven_home(&dists.join("apache-maven-3.9.11/a"), "3.9.11");
        let named = maven_home(&dists.join("apache-maven-3.8.8/b"), "3.8.8");
        let path = maven_home(&dir.path().join("installed"), "3.6.3");
        let env = BuildEnv::default();
        let found =
            |folders: &[PathBuf]| MAVEN.found(&env, &root, &root, folders, MAVEN.distributions(&dists)).unwrap();
        let folders = [dir.path().join("installed/bin")];
        let from_dists = found(&[]);
        assert_eq!(
            (from_dists.path, from_dists.source, from_dists.version.as_deref(), from_dists.wrapper),
            (newest, ToolSource::WrapperDistribution, Some("3.9.11"), false)
        );
        let installed = found(&folders);
        assert_eq!(
            (installed.path, installed.source, installed.version.as_deref()),
            (path, ToolSource::Path, Some("3.6.3"))
        );
        write(
            &root.join(".mvn/wrapper/maven-wrapper.properties"),
            "distributionUrl=https://x/apache-maven-3.8.8-bin.zip\n",
        );
        assert_eq!(found(&folders).path, named);
        write(&root.join("mvnw"), "");
        let wrapper = found(&folders);
        assert_eq!(
            (wrapper.path, wrapper.source, wrapper.wrapper),
            (root.join("mvnw"), ToolSource::ProjectWrapper, true)
        );
    }

    #[test]
    fn what_the_ide_downloaded_and_sdkman_are_told_apart_from_path() {
        let dir = tempfile::tempdir().unwrap();
        let env = BuildEnv::default();
        env.set_data_dir(Some(dir.path().join("data")));
        let source = |path: &str| source_of(&env, Path::new(path));
        assert_eq!(source("/opt/bin/mvn"), ToolSource::Path);
        assert_eq!(source("/home/me/.sdkman/candidates/maven/current/bin/mvn"), ToolSource::Sdkman);
        assert_eq!(source_of(&env, &dir.path().join("data/tools/maven/3.9.16/bin/mvn")), ToolSource::Downloaded);
    }
}
