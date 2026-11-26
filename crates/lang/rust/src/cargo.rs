use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::toolchain::{ProjectRust, rust};

const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_default_members: Vec<String>,
    pub workspace_root: PathBuf,
    pub target_directory: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub id: String,
    pub source: Option<String>,
    pub manifest_path: PathBuf,
    pub edition: String,
    pub rust_version: Option<String>,
    pub dependencies: Vec<PackageDependency>,
    pub targets: Vec<Target>,
    pub features: BTreeMap<String, Vec<String>>,
}

impl Package {
    pub fn root(&self) -> PathBuf {
        self.manifest_path.parent().map(Path::to_owned).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PackageDependency {
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub optional: bool,
    pub rename: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: PathBuf,
    pub test: bool,
    pub doctest: bool,
    #[serde(rename = "required-features")]
    pub required_features: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataError {
    pub message: String,
    pub manifest: Option<PathBuf>,
    pub line: Option<u32>,
}

pub fn metadata(root: &Path, project: &ProjectRust, deps: bool) -> Result<Metadata, MetadataError> {
    let Some(cargo) = &project.cargo else {
        return Err(MetadataError { message: zen_i18n::t!("rust-model-no-cargo"), ..MetadataError::default() });
    };
    let mut command = std::process::Command::new(cargo);
    command.args(["metadata", "--format-version", "1", "--offline", "--color", "never"]);
    if !deps {
        command.arg("--no-deps");
    } else if let Some(host) = &project.host {
        command.args(["--filter-platform", host]);
    }
    command.current_dir(root).envs(project.env.iter().map(|(name, value)| (name, value)));
    let finished =
        rust::run(&mut command, TIMEOUT).map_err(|message| MetadataError { message, ..MetadataError::default() })?;
    if !finished.success {
        return Err(parse_error(root, &finished.stderr));
    }
    serde_json::from_str(&finished.stdout)
        .map_err(|error| MetadataError { message: error.to_string(), ..MetadataError::default() })
}

pub fn parse_error(root: &Path, stderr: &str) -> MetadataError {
    let lines: Vec<&str> = stderr.lines().map(str::trim).collect();
    let start = lines.iter().position(|line| line.starts_with("error")).unwrap_or(0);
    let mut parts: Vec<String> = Vec::new();
    for line in &lines[start..] {
        let snippet = line.starts_with("-->")
            || line.starts_with('|')
            || line.split_once(" |").is_some_and(|(number, _)| number.bytes().all(|byte| byte.is_ascii_digit()))
            || line.chars().all(|c| c == '^' || c == '-' || c.is_whitespace());
        if line.is_empty() || *line == "Caused by:" || snippet {
            continue;
        }
        let text = line.strip_prefix("error:").map_or(*line, str::trim);
        if !parts.iter().any(|part| part == text) {
            parts.push(text.to_owned());
        }
    }
    let message = if parts.is_empty() { stderr.trim().to_owned() } else { parts.join(": ") };
    let pointed = lines.iter().find_map(|line| {
        let place = line.strip_prefix("-->")?.trim();
        let mut pieces = place.rsplitn(3, ':');
        let _column = pieces.next()?;
        let line: u32 = pieces.next()?.parse().ok()?;
        Some((root.join(pieces.next()?), line))
    });
    if let Some((manifest, line)) = pointed {
        return MetadataError { message, manifest: Some(manifest), line: Some(line) };
    }
    let manifest = lines.iter().find_map(|line| {
        let (_, rest) = line.split_once('`')?;
        let (path, _) = rest.split_once('`')?;
        path.ends_with("Cargo.toml").then(|| root.join(path))
    });
    let line = lines.iter().find_map(|line| {
        let (_, rest) = line.split_once(" at line ")?;
        rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    });
    MetadataError { message, manifest, line }
}

pub fn toolchain_channel(root: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(root.join("rust-toolchain.toml")) {
        return text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "channel").then(|| value.trim().trim_matches(['"', '\'']).to_owned())
        });
    }
    let text = std::fs::read_to_string(root.join("rust-toolchain")).ok()?;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line.starts_with('[') {
        return text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "channel").then(|| value.trim().trim_matches(['"', '\'']).to_owned())
        });
    }
    Some(line.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargos_errors_name_the_manifest_and_the_line() {
        let root = Path::new("/p");
        let stderr = "error: string values must be quoted, expected literal string\n --> tool/Cargo.toml:3:11\n  |\n3 | version = \n  |           ^\nerror: failed to load manifest for workspace member `/p/tool`\nreferenced by workspace at `/p/Cargo.toml`\n";
        let error = parse_error(root, stderr);
        assert_eq!(error.manifest, Some(PathBuf::from("/p/tool/Cargo.toml")));
        assert_eq!(error.line, Some(3));
        assert_eq!(
            error.message,
            "string values must be quoted, expected literal string: failed to load manifest for workspace member \
             `/p/tool`: referenced by workspace at `/p/Cargo.toml`"
        );
        let older = "error: failed to parse manifest at `/p/crates/a/Cargo.toml`\n\nCaused by:\n  TOML parse error at line 3, column 9\n    |\n  3 | version = \n    |         ^\n  string values must be quoted\n";
        let error = parse_error(root, older);
        assert_eq!(error.manifest, Some(PathBuf::from("/p/crates/a/Cargo.toml")));
        assert_eq!(error.line, Some(3));
        assert!(error.message.starts_with("failed to parse manifest at `/p/crates/a/Cargo.toml`: TOML parse error"));
        let offline = "error: no matching package named `serde` found\nlocation searched: registry `crates-io`\nrequired by package `app v0.1.0 (/p)`\nAs a reminder, you're using offline mode (--offline) which can sometimes cause surprising resolution failures";
        let error = parse_error(root, offline);
        assert_eq!(error.manifest, None);
        assert_eq!(error.line, None);
        assert!(error.message.starts_with("no matching package named `serde` found"));
        assert_eq!(parse_error(root, "   Updating index\n").message, "Updating index");
    }

    #[test]
    fn the_toolchain_channel_is_read_from_either_file() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(toolchain_channel(temp.path()), None);
        std::fs::write(temp.path().join("rust-toolchain"), "nightly-2026-10-01\n").unwrap();
        assert_eq!(toolchain_channel(temp.path()).as_deref(), Some("nightly-2026-10-01"));
        std::fs::write(temp.path().join("rust-toolchain"), "[toolchain]\nchannel = \"beta\"\n").unwrap();
        assert_eq!(toolchain_channel(temp.path()).as_deref(), Some("beta"));
        std::fs::write(
            temp.path().join("rust-toolchain.toml"),
            "# Zen IDE builds with the latest stable Rust.\n[toolchain]\nchannel = \"stable\"\ncomponents = [\"rustfmt\"]\n",
        )
        .unwrap();
        assert_eq!(toolchain_channel(temp.path()).as_deref(), Some("stable"));
    }

    #[test]
    fn metadata_is_read_from_cargos_json() {
        let json = r#"{"packages":[{"name":"app","version":"0.1.0","id":"path+file:///p#app@0.1.0","source":null,
            "manifest_path":"/p/Cargo.toml","edition":"2024","rust_version":"1.85",
            "dependencies":[{"name":"serde","req":"^1","kind":null,"optional":false,"rename":null},
                            {"name":"lib","req":"*","kind":"dev","optional":false,"path":"/p/lib"}],
            "targets":[{"name":"app","kind":["bin"],"src_path":"/p/src/main.rs","test":true,"doctest":false,
                        "required-features":["cli"]}],
            "features":{"default":["cli"],"cli":[]}}],
            "workspace_members":["path+file:///p#app@0.1.0"],"workspace_default_members":["path+file:///p#app@0.1.0"],
            "resolve":null,"target_directory":"/p/target","workspace_root":"/p","metadata":null,"version":1}"#;
        let metadata: Metadata = serde_json::from_str(json).unwrap();
        let app = &metadata.packages[0];
        assert_eq!(app.root(), PathBuf::from("/p"));
        assert_eq!(app.rust_version.as_deref(), Some("1.85"));
        assert_eq!(app.dependencies[1].path, Some(PathBuf::from("/p/lib")));
        assert_eq!(app.dependencies[1].kind.as_deref(), Some("dev"));
        assert_eq!(app.targets[0].required_features, ["cli"]);
        assert_eq!(metadata.workspace_root, PathBuf::from("/p"));
    }
}
