use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a JDK was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum JdkSource {
    /// The `JAVA_HOME` environment variable.
    JavaHome,
    /// The system's JDKs: `/usr/libexec/java_home -V`, `/Library/Java/JavaVirtualMachines`, the
    /// user's `~/Library/Java/JavaVirtualMachines`, `/usr/lib/jvm`.
    System,
    /// Homebrew (`openjdk`, `openjdk@21`...).
    Homebrew,
    /// SDKMAN! (`~/.sdkman/candidates/java`).
    Sdkman,
    /// asdf (`~/.asdf/installs/java`).
    Asdf,
    /// mise (`~/.local/share/mise/installs/java`).
    Mise,
    /// Gradle's toolchains: the JDKs it downloaded (`~/.gradle/jdks`) and
    /// `org.gradle.java.installations.paths` in `~/.gradle/gradle.properties`.
    Gradle,
    /// Maven's toolchains (`~/.m2/toolchains.xml`).
    Maven,
    /// Downloaded by the IDE into its own folder.
    Downloaded,
    /// Added by hand.
    Added,
}

/// A JDK: its home folder and what its `release` file says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Jdk {
    /// The home folder, the one `JAVA_HOME` names (with `bin/java`).
    pub home: PathBuf,
    /// The Java version, e.g. `21.0.5` or `1.8.0_412`.
    pub version: String,
    /// The feature release, e.g. `21` (`8` for `1.8.0_412`).
    pub major: u32,
    /// Who made it, e.g. `Eclipse Adoptium`; empty when the `release` file does not say.
    pub vendor: String,
    /// Where it was found.
    pub source: JdkSource,
    /// Whether it has the compiler (`bin/javac`): a JDK rather than a JRE.
    pub compiler: bool,
}

impl Jdk {
    pub fn label(&self) -> String {
        if self.vendor.is_empty() { self.version.clone() } else { format!("{} ({})", self.version, self.vendor) }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub java_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub java_home_listing: Option<String>,
    pub system_folders: Vec<PathBuf>,
    pub homebrew: Vec<PathBuf>,
    pub downloaded: Option<PathBuf>,
    pub added: Vec<PathBuf>,
}

impl Sources {
    pub fn of_machine(downloaded: Option<PathBuf>, added: Vec<PathBuf>) -> Self {
        let java_home_listing = Path::new("/usr/libexec/java_home")
            .is_file()
            .then(|| std::process::Command::new("/usr/libexec/java_home").arg("-V").output().ok())
            .flatten()
            .map(|output| {
                format!("{}{}", String::from_utf8_lossy(&output.stderr), String::from_utf8_lossy(&output.stdout))
            });
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut system_folders =
            vec![PathBuf::from("/Library/Java/JavaVirtualMachines"), PathBuf::from("/usr/lib/jvm")];
        if let Some(home) = &home {
            system_folders.push(home.join("Library/Java/JavaVirtualMachines"));
        }
        Self {
            java_home: std::env::var_os("JAVA_HOME").filter(|value| !value.is_empty()).map(PathBuf::from),
            home,
            java_home_listing,
            system_folders,
            homebrew: ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"].map(PathBuf::from).to_vec(),
            downloaded,
            added,
        }
    }
}

pub fn detect(sources: &Sources) -> Vec<Jdk> {
    let mut found: Vec<(Vec<PathBuf>, JdkSource)> = Vec::new();
    let one = |found: &mut Vec<(Vec<PathBuf>, JdkSource)>, homes: Vec<PathBuf>, source: JdkSource| {
        found.extend(homes.into_iter().map(|home| (vec![home], source)));
    };
    if let Some(java_home) = &sources.java_home {
        one(&mut found, vec![java_home.clone()], JdkSource::JavaHome);
    }
    if let Some(listing) = &sources.java_home_listing {
        one(&mut found, java_home_listing(listing), JdkSource::System);
    }
    for folder in &sources.system_folders {
        one(&mut found, children(folder), JdkSource::System);
    }
    for prefix in &sources.homebrew {
        for opt in list(&prefix.join("opt")) {
            let name = opt.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if name == "openjdk" || name.starts_with("openjdk@") {
                found.push((
                    vec![opt.join("libexec/openjdk.jdk/Contents/Home"), opt.join("libexec"), opt],
                    JdkSource::Homebrew,
                ));
            }
        }
    }
    if let Some(home) = &sources.home {
        let managers = [
            (".sdkman/candidates/java", JdkSource::Sdkman),
            (".asdf/installs/java", JdkSource::Asdf),
            (".local/share/mise/installs/java", JdkSource::Mise),
            (".gradle/jdks", JdkSource::Gradle),
        ];
        for (folder, source) in managers {
            one(&mut found, children(&home.join(folder)), source);
        }
        if let Ok(text) = std::fs::read_to_string(home.join(".gradle/gradle.properties")) {
            one(&mut found, gradle_installations(&text), JdkSource::Gradle);
        }
        if let Ok(text) = std::fs::read_to_string(home.join(".m2/toolchains.xml")) {
            one(&mut found, maven_toolchains(&text), JdkSource::Maven);
        }
    }
    if let Some(downloaded) = &sources.downloaded {
        one(&mut found, children(downloaded), JdkSource::Downloaded);
    }
    one(&mut found, sources.added.clone(), JdkSource::Added);

    let mut seen = BTreeSet::new();
    let mut jdks: Vec<Jdk> = Vec::new();
    for (homes, source) in found {
        let Some(jdk) = homes.iter().find_map(|home| read(home, source)) else { continue };
        let real = std::fs::canonicalize(&jdk.home).unwrap_or_else(|_| jdk.home.clone());
        if seen.insert(real) {
            jdks.push(jdk);
        }
    }
    jdks.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| version_key(&b.version).cmp(&version_key(&a.version))));
    jdks
}

pub fn read(home: &Path, source: JdkSource) -> Option<Jdk> {
    let home = if home.join("Contents/Home/bin").is_dir() { home.join("Contents/Home") } else { home.to_owned() };
    let java = if cfg!(windows) { "bin/java.exe" } else { "bin/java" };
    if !home.join(java).is_file() {
        return None;
    }
    let release = std::fs::read_to_string(home.join("release")).unwrap_or_default();
    let field = |key: &str| release_field(&release, key);
    let version = field("JAVA_VERSION").or_else(|| field("JAVA_RUNTIME_VERSION"))?;
    let compiler = home.join(if cfg!(windows) { "bin/javac.exe" } else { "bin/javac" }).is_file();
    Some(Jdk {
        major: major_of(&version)?,
        version,
        vendor: field("IMPLEMENTOR").unwrap_or_default(),
        source,
        compiler,
        home,
    })
}

pub fn release_field(release: &str, key: &str) -> Option<String> {
    release.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned()).filter(|value| !value.is_empty())
    })
}

pub fn major_of(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 { parts.next()?.parse().ok() } else { Some(first) }
}

fn version_key(version: &str) -> Vec<u32> {
    let mut key: Vec<u32> = version.split(['.', '_', '-', '+']).filter_map(|part| part.parse().ok()).collect();
    if key.first() == Some(&1) {
        key.remove(0);
    }
    key
}

fn java_home_listing(listing: &str) -> Vec<PathBuf> {
    listing.lines().filter_map(|line| line.find(" /").map(|at| PathBuf::from(line[at + 1..].trim()))).collect()
}

fn children(folder: &Path) -> Vec<PathBuf> {
    list(folder).into_iter().filter(|child| child.file_name().is_some_and(|name| name != "current")).collect()
}

fn list(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    children.sort();
    children
}

fn gradle_installations(properties: &str) -> Vec<PathBuf> {
    properties
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "org.gradle.java.installations.paths")
        .flat_map(|(_, value)| {
            value.split(',').map(str::trim).filter(|path| !path.is_empty()).map(PathBuf::from).collect::<Vec<_>>()
        })
        .collect()
}

fn maven_toolchains(xml: &str) -> Vec<PathBuf> {
    let Ok(document) = roxmltree::Document::parse(xml) else { return Vec::new() };
    document
        .descendants()
        .filter(|node| node.has_tag_name("jdkHome"))
        .filter_map(|node| node.text())
        .map(|text| PathBuf::from(text.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn fake_jdk(home: &Path, version: &str, vendor: &str) {
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("bin/java"), "").unwrap();
        std::fs::write(home.join("bin/javac"), "").unwrap();
        std::fs::write(home.join("release"), format!("IMPLEMENTOR=\"{vendor}\"\nJAVA_VERSION=\"{version}\"\n"))
            .unwrap();
    }

    #[test]
    fn versions_and_release_files_are_read() {
        assert_eq!(major_of("21.0.5"), Some(21));
        assert_eq!(major_of("1.8.0_412"), Some(8));
        assert_eq!(major_of("25"), Some(25));
        assert_eq!(major_of("x"), None);
        let release = "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.5\"\nOS_ARCH=\"aarch64\"";
        assert_eq!(release_field(release, "IMPLEMENTOR").as_deref(), Some("Eclipse Adoptium"));
        assert_eq!(release_field(release, "MISSING"), None);
        let listing = "Matching Java Virtual Machines (2):\n    21.0.5 (arm64) \"Eclipse Adoptium\" - \"OpenJDK 21.0.5\" /Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home\n    17.0.2 (x86_64) \"Oracle Corporation\" - \"Java SE 17\" /Library/Java/JavaVirtualMachines/jdk-17.jdk/Contents/Home\n/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home";
        let homes = java_home_listing(listing);
        assert_eq!(homes[0], Path::new("/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home"));
        assert_eq!(homes.len(), 2, "the last line, the default, is a path alone");
        assert_eq!(
            gradle_installations("a=b\norg.gradle.java.installations.paths=/j/17, /j/21\n"),
            [PathBuf::from("/j/17"), PathBuf::from("/j/21")]
        );
        let toolchains = "<toolchains><toolchain><type>jdk</type><configuration><jdkHome>/j/11</jdkHome></configuration></toolchain></toolchains>";
        assert_eq!(maven_toolchains(toolchains), [PathBuf::from("/j/11")]);
    }

    #[cfg(unix)]
    #[test]
    fn every_source_is_found_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let home = root.join("home");
        fake_jdk(&root.join("jvm/temurin-21.jdk/Contents/Home"), "21.0.5", "Eclipse Adoptium");
        fake_jdk(&root.join("jvm/zulu-17.jdk/Contents/Home"), "17.0.12", "Azul Systems, Inc.");
        fake_jdk(&home.join(".sdkman/candidates/java/11.0.24-tem"), "11.0.24", "Eclipse Adoptium");
        std::os::unix::fs::symlink(
            home.join(".sdkman/candidates/java/11.0.24-tem"),
            home.join(".sdkman/candidates/java/current"),
        )
        .unwrap();
        fake_jdk(&home.join(".asdf/installs/java/openjdk-22"), "22", "Oracle Corporation");
        fake_jdk(&home.join(".local/share/mise/installs/java/23"), "23.0.1", "Oracle Corporation");
        fake_jdk(
            &home.join(".gradle/jdks/eclipse_adoptium-24-aarch64-os_x.2/Contents/Home"),
            "24.0.1",
            "Eclipse Adoptium",
        );
        fake_jdk(&root.join("brew/opt/openjdk@8/libexec/openjdk.jdk/Contents/Home"), "1.8.0_412", "Homebrew");
        fake_jdk(&root.join("maven-jdk"), "16.0.2", "Oracle Corporation");
        std::fs::create_dir_all(home.join(".m2")).unwrap();
        std::fs::write(
            home.join(".m2/toolchains.xml"),
            format!(
                "<toolchains><toolchain><configuration><jdkHome>{}</jdkHome></configuration></toolchain></toolchains>",
                root.join("maven-jdk").display()
            ),
        )
        .unwrap();
        fake_jdk(&root.join("ide/jdks/jdk-25+36/Contents/Home"), "25", "Eclipse Adoptium");
        std::fs::create_dir_all(root.join("jvm/broken.jdk")).unwrap();
        let sources = Sources {
            java_home: Some(root.join("jvm/temurin-21.jdk/Contents/Home")),
            home: Some(home.clone()),
            java_home_listing: Some(format!(
                "    17.0.12 (arm64) \"Azul\" - \"Zulu 17\" {}",
                root.join("jvm/zulu-17.jdk/Contents/Home").display()
            )),
            system_folders: vec![root.join("jvm")],
            homebrew: vec![root.join("brew")],
            downloaded: Some(root.join("ide/jdks")),
            added: vec![root.join("jvm/temurin-21.jdk")],
        };
        let jdks = detect(&sources);
        let found: Vec<(u32, JdkSource)> = jdks.iter().map(|jdk| (jdk.major, jdk.source)).collect();
        assert_eq!(
            found,
            [
                (21, JdkSource::JavaHome),
                (17, JdkSource::System),
                (8, JdkSource::Homebrew),
                (11, JdkSource::Sdkman),
                (22, JdkSource::Asdf),
                (23, JdkSource::Mise),
                (24, JdkSource::Gradle),
                (16, JdkSource::Maven),
                (25, JdkSource::Downloaded),
            ],
            "the added one is JAVA_HOME's, the `current` link SDKMAN's 11, and a folder without bin/java is none"
        );
        assert_eq!(jdks[0].vendor, "Eclipse Adoptium");
        assert_eq!(jdks[0].home, root.join("jvm/temurin-21.jdk/Contents/Home"));
        assert!(jdks[0].compiler);
        assert_eq!(jdks[0].label(), "21.0.5 (Eclipse Adoptium)");
    }
}
