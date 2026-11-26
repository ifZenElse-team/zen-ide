use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub path: Vec<PathBuf>,
    pub home: Option<PathBuf>,
    pub cargo_home: Option<PathBuf>,
    pub install_folders: Vec<PathBuf>,
}

impl Environment {
    pub fn of_process() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Self {
            path: var("PATH").map(|value| std::env::split_paths(&value).collect()).unwrap_or_default(),
            home: var("HOME").or_else(|| var("USERPROFILE")).map(PathBuf::from),
            cargo_home: var("CARGO_HOME").map(PathBuf::from),
            install_folders: [
                "/opt/homebrew/opt/rust/bin",
                "/usr/local/opt/rust/bin",
                "/opt/homebrew/bin",
                "/usr/local/bin",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        }
    }

    pub fn cargo_bin(&self) -> Option<PathBuf> {
        self.cargo_home
            .clone()
            .or_else(|| self.home.as_ref().map(|home| home.join(".cargo")))
            .map(|home| home.join("bin"))
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in
            self.cargo_bin().into_iter().chain(self.path.iter().cloned()).chain(self.install_folders.iter().cloned())
        {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }
}

/// A Rust toolchain: a folder holding `cargo` and `rustc`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RustToolchain {
    /// The folder, e.g. `~/.cargo/bin`.
    pub bin: PathBuf,
    /// The version `rustc --version` prints, e.g. `1.99.1`, when it answered.
    pub version: Option<String>,
    /// Whether the folder holds rustup (its `cargo` and `rustc` are rustup's proxies).
    pub rustup: bool,
}

impl RustToolchain {
    pub fn cargo(&self) -> PathBuf {
        self.bin.join(executable("cargo"))
    }

    pub fn rustc(&self) -> PathBuf {
        self.bin.join(executable("rustc"))
    }

    pub fn rustup(&self) -> Option<PathBuf> {
        self.rustup.then(|| self.bin.join(executable("rustup")))
    }
}

pub fn executable(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

pub fn is_toolchain(folder: &Path) -> bool {
    folder.join(executable("cargo")).is_file() && folder.join(executable("rustc")).is_file()
}

pub fn toolchain_at(folder: &Path) -> Option<RustToolchain> {
    is_toolchain(folder).then(|| RustToolchain {
        bin: folder.to_owned(),
        version: rustc_version(&folder.join(executable("rustc")), None),
        rustup: folder.join(executable("rustup")).is_file(),
    })
}

pub fn find_toolchains(env: &Environment) -> Vec<RustToolchain> {
    let mut found: Vec<RustToolchain> = Vec::new();
    let mut real: Vec<PathBuf> = Vec::new();
    for folder in env.folders() {
        if !is_toolchain(&folder) {
            continue;
        }
        let cargo = folder.join(executable("cargo"));
        let canonical = cargo.canonicalize().ok().and_then(|cargo| Some(cargo.parent()?.to_owned()));
        let canonical = canonical.unwrap_or_else(|| folder.clone());
        if real.contains(&canonical) {
            continue;
        }
        real.push(canonical);
        found.extend(toolchain_at(&folder));
    }
    found
}

pub fn find_program(folders: &[PathBuf], name: &str) -> Option<PathBuf> {
    folders.iter().map(|folder| folder.join(executable(name))).find(|path| path.is_file())
}

pub fn rustc_version(rustc: &Path, cwd: Option<&Path>) -> Option<String> {
    parse_rustc_version(&output(rustc, &["--version"], cwd, Duration::from_secs(10))?)
}

pub fn parse_rustc_version(output: &str) -> Option<String> {
    let mut words = output.split_whitespace();
    (words.next()? == "rustc").then_some(())?;
    words.next().map(str::to_owned)
}

pub fn host(rustc: &Path, cwd: Option<&Path>) -> Option<String> {
    parse_host(&output(rustc, &["-vV"], cwd, Duration::from_secs(10))?)
}

pub fn parse_host(output: &str) -> Option<String> {
    output.lines().find_map(|line| line.strip_prefix("host: ")).map(|host| host.trim().to_owned())
}

pub fn sysroot(rustc: &Path, cwd: Option<&Path>) -> Option<PathBuf> {
    let printed = output(rustc, &["--print", "sysroot"], cwd, Duration::from_secs(10))?;
    let line = printed.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

pub fn library_sources(sysroot: &Path) -> PathBuf {
    sysroot.join("lib").join("rustlib").join("src").join("rust").join("library")
}

pub fn output(program: &Path, args: &[&str], cwd: Option<&Path>, timeout: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args).env("RUSTUP_AUTO_INSTALL", "0");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let finished = run(&mut command, timeout).ok()?;
    finished.success.then_some(finished.stdout)
}

/// rust-analyzer, found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RustAnalyzer {
    /// Its executable.
    pub path: PathBuf,
    /// Its version, e.g. `1.99.1`, when it answered.
    pub version: Option<String>,
}

pub fn find_rust_analyzer(
    toolchain: Option<&RustToolchain>,
    folders: &[PathBuf],
    home: Option<&Path>,
) -> Option<RustAnalyzer> {
    if let Some(rustup) = toolchain.and_then(RustToolchain::rustup) {
        let cwd = home.unwrap_or(Path::new("/"));
        let which = output(&rustup, &["which", "rust-analyzer"], Some(cwd), Duration::from_secs(10));
        if let Some(path) = which.and_then(|printed| printed.lines().next().map(|line| PathBuf::from(line.trim())))
            && path.is_file()
            && let Some(version) = rust_analyzer_version(&path)
        {
            return Some(RustAnalyzer { path, version: Some(version) });
        }
    }
    folders
        .iter()
        .map(|folder| folder.join(executable("rust-analyzer")))
        .filter(|path| path.is_file())
        .find_map(|path| rust_analyzer_version(&path).map(|version| RustAnalyzer { path, version: Some(version) }))
}

pub fn rust_analyzer_version(path: &Path) -> Option<String> {
    parse_rust_analyzer_version(&output(path, &["--version"], None, Duration::from_secs(10))?)
}

pub fn parse_rust_analyzer_version(output: &str) -> Option<String> {
    let mut words = output.split_whitespace();
    (words.next()? == "rust-analyzer").then_some(())?;
    words.next().map(str::to_owned)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finished {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(command: &mut Command, timeout: Duration) -> Result<Finished, String> {
    let captured = zen_run::capture(command, timeout)?;
    Ok(Finished {
        success: captured.status.success(),
        stdout: String::from_utf8_lossy(&captured.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&captured.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn versions_and_hosts_are_read() {
        assert_eq!(parse_rustc_version("rustc 1.99.1 (58cf8f9c3 2026-10-13)\n").as_deref(), Some("1.99.1"));
        assert_eq!(parse_rustc_version("rustc 1.100.0-nightly (abc 2026-11-01)").as_deref(), Some("1.100.0-nightly"));
        assert_eq!(parse_rustc_version("error: no such command"), None);
        let verbose =
            "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.99.1\n";
        assert_eq!(parse_host(verbose).as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(parse_host("rustc 1.0"), None);
        assert_eq!(
            parse_rust_analyzer_version("rust-analyzer 1.99.1 (58cf8f9 2026-10-13)\n").as_deref(),
            Some("1.99.1")
        );
        assert_eq!(parse_rust_analyzer_version("error: Unknown binary 'rust-analyzer'"), None);
    }

    #[cfg(unix)]
    #[test]
    fn rust_analyzer_is_found_through_rustup_or_when_it_answers() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let bin = home.join(".cargo/bin");
        script(&bin.join("rust-analyzer"), "echo \"error: Unknown binary 'rust-analyzer'\" >&2; exit 1");
        script(&bin.join("rustup"), "exit 1");
        let toolchain = RustToolchain { bin: bin.clone(), version: None, rustup: true };
        assert_eq!(find_rust_analyzer(Some(&toolchain), std::slice::from_ref(&bin), Some(&home)), None);
        let installed = home.join("rustup/toolchains/stable/bin/rust-analyzer");
        script(&installed, "echo 'rust-analyzer 1.99.1 (58cf8f9 2026-10-13)'");
        script(&bin.join("rustup"), &format!("echo '{}'", installed.display()));
        let found = find_rust_analyzer(Some(&toolchain), std::slice::from_ref(&bin), Some(&home)).unwrap();
        assert_eq!((found.path, found.version.as_deref()), (installed, Some("1.99.1")));
        let brew = home.join("brew/bin");
        script(&brew.join("rust-analyzer"), "echo 'rust-analyzer 0.3.2600-standalone'");
        let found = find_rust_analyzer(None, &[bin, brew.clone()], Some(&home)).unwrap();
        assert_eq!(found.path, brew.join("rust-analyzer"));
    }

    #[test]
    fn the_library_sources_are_in_the_sysroot() {
        assert_eq!(
            library_sources(Path::new("/rustup/toolchains/stable")),
            Path::new("/rustup/toolchains/stable/lib/rustlib/src/rust/library")
        );
    }

    #[cfg(unix)]
    fn script(path: &Path, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn toolchains_are_found_once_rustups_first() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let rustup_bin = home.join(".cargo/bin");
        script(&rustup_bin.join("cargo"), "echo cargo");
        script(&rustup_bin.join("rustc"), "echo 'rustc 1.99.1 (58cf8f9c3 2026-10-13)'");
        script(&rustup_bin.join("rustup"), "echo rustup");
        let brew = home.join("brew/opt/rust/bin");
        script(&brew.join("cargo"), "echo cargo");
        script(&brew.join("rustc"), "echo 'rustc 1.98.0 (aaaa 2026-09-01)'");
        let links = home.join("brew/bin");
        std::fs::create_dir_all(&links).unwrap();
        std::os::unix::fs::symlink(brew.join("cargo"), links.join("cargo")).unwrap();
        std::os::unix::fs::symlink(brew.join("rustc"), links.join("rustc")).unwrap();
        script(&home.join("half/cargo"), "echo cargo");
        let env = Environment {
            path: vec![links.clone(), home.join("half")],
            home: Some(home.clone()),
            cargo_home: None,
            install_folders: vec![brew.clone()],
        };
        let found = find_toolchains(&env);
        assert_eq!(
            found,
            [
                RustToolchain { bin: rustup_bin.clone(), version: Some("1.99.1".to_owned()), rustup: true },
                RustToolchain { bin: links, version: Some("1.98.0".to_owned()), rustup: false },
            ]
        );
        assert_eq!(found[0].rustup(), Some(rustup_bin.join("rustup")));
        assert_eq!(found[1].rustup(), None);
        let moved = Environment { cargo_home: Some(home.join("elsewhere")), ..env };
        assert_eq!(moved.cargo_bin(), Some(home.join("elsewhere/bin")));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_hangs_is_stopped_and_long_output_is_read() {
        let temp = tempfile::tempdir().unwrap();
        let slow = temp.path().join("slow");
        script(&slow, "sleep 600");
        let started = Instant::now();
        assert!(run(&mut Command::new(&slow), Duration::from_millis(200)).is_err());
        assert!(started.elapsed() < Duration::from_secs(60));
        let loud = temp.path().join("loud");
        script(
            &loud,
            "i=0; while [ $i -lt 4000 ]; do echo 0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789; i=$((i+1)); done; echo bad >&2; exit 3",
        );
        let finished = run(&mut Command::new(&loud), Duration::from_secs(20)).unwrap();
        assert!(!finished.success);
        assert_eq!(finished.stdout.lines().count(), 4000);
        assert_eq!(finished.stderr, "bad\n");
    }
}
