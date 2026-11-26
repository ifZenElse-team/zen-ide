use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub path: Vec<PathBuf>,
    pub goroot: Option<PathBuf>,
    pub gopath: Vec<PathBuf>,
    pub gobin: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub install_folders: Vec<PathBuf>,
    pub gomodcache: Option<PathBuf>,
    pub goenv: Option<PathBuf>,
}

impl Environment {
    pub fn of_process() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let list = |name: &str| var(name).map(|value| std::env::split_paths(&value).collect()).unwrap_or_default();
        Self {
            path: list("PATH"),
            goroot: var("GOROOT").map(PathBuf::from),
            gopath: list("GOPATH"),
            gobin: var("GOBIN").map(PathBuf::from),
            home: var("HOME").map(PathBuf::from),
            install_folders: [
                "/usr/local/go",
                "/opt/homebrew/opt/go/libexec",
                "/usr/local/opt/go/libexec",
                "/usr/lib/go",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
            gomodcache: var("GOMODCACHE").map(PathBuf::from),
            goenv: match var("GOENV") {
                Some(value) if value == "off" => None,
                Some(value) => Some(PathBuf::from(value)),
                None => go_env_file(
                    std::env::consts::OS,
                    var("HOME").map(PathBuf::from).as_deref(),
                    var("XDG_CONFIG_HOME").map(PathBuf::from).as_deref(),
                    var("APPDATA").map(PathBuf::from).as_deref(),
                ),
            },
        }
    }

    fn gopaths(&self) -> Vec<PathBuf> {
        if self.gopath.is_empty() {
            self.home.iter().map(|home| home.join("go")).collect()
        } else {
            self.gopath.clone()
        }
    }
}

/// A Go SDK.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct GoSdk {
    /// Its root (`GOROOT`), holding `bin/go`.
    pub root: PathBuf,
    /// Its version, e.g. `go1.27.2`, from its `VERSION` file.
    pub version: Option<String>,
}

impl GoSdk {
    pub fn go(&self) -> PathBuf {
        self.root.join("bin").join(executable("go"))
    }
}

pub fn find_sdks(env: &Environment) -> Vec<GoSdk> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.extend(env.goroot.iter().cloned());
    for folder in &env.path {
        let go = folder.join(executable("go"));
        if go.is_file() {
            let real = go.canonicalize().unwrap_or_else(|_| go.clone());
            match real.parent().and_then(Path::parent) {
                Some(root) if is_sdk(root) => candidates.push(root.to_owned()),
                root => {
                    let asked = goroot_of(&go);
                    candidates.extend(asked.or_else(|| root.map(Path::to_owned)));
                }
            }
        }
    }
    candidates.extend(env.install_folders.iter().cloned());
    if let Some(home) = &env.home {
        candidates.extend(children(&home.join("sdk"), "go"));
    }
    for gopath in env.gopaths() {
        candidates.extend(children(&gopath.join("pkg/mod/golang.org"), "toolchain@"));
    }
    let mut sdks: Vec<GoSdk> = Vec::new();
    let mut real: Vec<PathBuf> = Vec::new();
    for root in candidates {
        if !has_go(&root) {
            continue;
        }
        let root = stable_root(&root);
        let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
        if sdks.iter().any(|sdk| sdk.root == root) || real.contains(&canonical) {
            continue;
        }
        let version = sdk_version(&root);
        real.push(canonical);
        sdks.push(GoSdk { root, version });
    }
    sdks
}

pub fn stable_root(root: &Path) -> PathBuf {
    match homebrew_alias(root) {
        Some(alias) if same_folder(&alias, root) => alias,
        _ => root.to_owned(),
    }
}

pub fn homebrew_alias(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let at = parts.iter().position(|part| part.as_os_str() == "Cellar")?;
    let formula = parts.get(at + 1)?;
    parts.get(at + 2)?;
    let mut alias: PathBuf = parts[..at].iter().collect();
    alias.push("opt");
    alias.push(formula);
    alias.extend(&parts[at + 3..]);
    Some(alias)
}

pub fn same_folder(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn is_sdk(root: &Path) -> bool {
    has_go(root) && (root.join("VERSION").is_file() || root.join("src").join("runtime").is_dir())
}

const GOROOT_WAIT: Duration = Duration::from_secs(if cfg!(test) { 60 } else { 5 });

pub fn goroot_of(go: &Path) -> Option<PathBuf> {
    let output = command_output(go, &["env", "GOROOT"], GOROOT_WAIT)?;
    let root = PathBuf::from(output.lines().next()?.trim());
    (root.is_absolute() && has_go(&root)).then_some(root)
}

pub fn sdk_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("VERSION")).ok()?;
    text.lines().next().map(str::trim).filter(|line| line.starts_with("go")).map(str::to_owned)
}

pub fn find_gopls(env: &Environment) -> Option<PathBuf> {
    find_tool(env, "gopls")
}

pub fn find_dlv(env: &Environment) -> Option<PathBuf> {
    find_tool(env, "dlv")
}

pub fn find_tool(env: &Environment, tool: &str) -> Option<PathBuf> {
    let name = executable(tool);
    let mut folders: Vec<PathBuf> = env.gobin.iter().cloned().collect();
    folders.extend(env.gopaths().into_iter().map(|gopath| gopath.join("bin")));
    folders.extend(env.path.iter().cloned());
    folders.push(PathBuf::from("/opt/homebrew/bin"));
    folders.push(PathBuf::from("/usr/local/bin"));
    folders.into_iter().map(|folder| folder.join(&name)).find(|found| found.is_file())
}

pub fn gopls_version(gopls: &Path) -> Option<String> {
    parse_gopls_version(&version_output(gopls, PROBE_WAIT)?)
}

pub const PROBE_WAIT: Duration = Duration::from_secs(15);

pub const MIN_DLV: (u32, u32) = (1, 20);

pub fn dlv_version(dlv: &Path) -> Option<String> {
    parse_dlv_version(&version_output(dlv, PROBE_WAIT)?)
}

pub fn parse_dlv_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let version = line.trim().strip_prefix("Version:")?.trim();
        let version = version.strip_prefix('v').unwrap_or(version);
        version.starts_with(|first: char| first.is_ascii_digit()).then(|| version.to_owned())
    })
}

pub fn dlv_outdated(version: &str) -> bool {
    let mut parts = version.split(['.', '-', '+']).map(|part| part.parse::<u32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) < MIN_DLV,
        _ => false,
    }
}

pub fn go_version(go: &Path) -> Option<String> {
    parse_go_version(&version_output(go, PROBE_WAIT)?)
}

pub fn parse_go_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next()? == "go" && words.next()? == "version")
            .then(|| words.next())
            .flatten()
            .filter(|version| version.starts_with("go"))
            .map(str::to_owned)
    })
}

pub fn sdk_at(root: &Path) -> Option<GoSdk> {
    if !has_go(root) {
        return None;
    }
    let root = stable_root(root);
    let version = sdk_version(&root).or_else(|| go_version(&root.join("bin").join(executable("go"))))?;
    Some(GoSdk { root, version: Some(version) })
}

pub fn has_go(root: &Path) -> bool {
    root.join("bin").join(executable("go")).is_file()
}

fn version_output(program: &Path, timeout: Duration) -> Option<String> {
    command_output(program, &["version"], timeout)
}

fn command_output(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let child = spawn_when_free(|| {
        Command::new(program)
            .args(args)
            .env("GOTOOLCHAIN", "local")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    })
    .ok()?;
    let captured = zen_run::capture_child(child, timeout)?;
    Some(String::from_utf8_lossy(&captured.stdout).into_owned())
}

fn spawn_when_free(
    mut spawn: impl FnMut() -> std::io::Result<std::process::Child>,
) -> std::io::Result<std::process::Child> {
    const TEXT_FILE_BUSY: i32 = 26;
    let mut tries = 0;
    loop {
        match spawn() {
            Err(error) if cfg!(unix) && error.raw_os_error() == Some(TEXT_FILE_BUSY) && tries < 25 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

pub fn parse_gopls_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("golang.org/x/tools/gopls")?;
        rest.split_whitespace().next().filter(|version| version.starts_with('v')).map(str::to_owned)
    })
}

pub fn install_gopls_command(sdk: &GoSdk) -> Command {
    install_command(sdk, "golang.org/x/tools/gopls@latest")
}

pub fn install_dlv_command(sdk: &GoSdk) -> Command {
    install_command(sdk, "github.com/go-delve/delve/cmd/dlv@latest")
}

pub fn install_command(sdk: &GoSdk, package: &str) -> Command {
    let mut command = Command::new(sdk.go());
    command.args(["install", package]);
    let mut path = vec![sdk.root.join("bin")];
    path.extend(
        std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect::<Vec<_>>()).unwrap_or_default(),
    );
    path.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    if let Ok(joined) = std::env::join_paths(path) {
        command.env("PATH", joined);
    }
    command.env("GOROOT", &sdk.root);
    command
}

pub fn go_env_file(
    os: &str,
    home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    appdata: Option<&Path>,
) -> Option<PathBuf> {
    let config = match os {
        "windows" => appdata.map(Path::to_owned),
        "macos" | "ios" => home.map(|home| home.join("Library/Application Support")),
        _ => xdg_config_home
            .filter(|folder| folder.is_absolute())
            .map(Path::to_owned)
            .or_else(|| home.map(|home| home.join(".config"))),
    }?;
    Some(config.join("go").join("env"))
}

pub fn parse_go_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

pub fn module_cache_with(env: &Environment, env_file: Option<&str>) -> Option<PathBuf> {
    if let Some(cache) = &env.gomodcache {
        return Some(cache.clone());
    }
    let settings = env_file.map(parse_go_env).unwrap_or_default();
    let setting = |name: &str| {
        settings.iter().rev().find(|(key, value)| key == name && !value.is_empty()).map(|(_, value)| value.as_str())
    };
    if let Some(cache) = setting("GOMODCACHE") {
        return Some(PathBuf::from(cache));
    }
    let gopath = if env.gopath.is_empty() {
        match setting("GOPATH") {
            Some(list) => std::env::split_paths(list).next(),
            None => env.gopaths().into_iter().next(),
        }
    } else {
        env.gopath.first().cloned()
    }?;
    Some(gopath.join("pkg").join("mod"))
}

pub fn module_cache(env: &Environment) -> Option<PathBuf> {
    let text = env.goenv.as_ref().and_then(|file| std::fs::read_to_string(file).ok());
    module_cache_with(env, text.as_deref())
}

fn children(folder: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(prefix)))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn executable(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn a_program_still_being_written_is_started_once_it_is_free() {
        let mut busy = 3;
        let child = spawn_when_free(|| {
            if busy > 0 {
                busy -= 1;
                return Err(std::io::Error::from_raw_os_error(26));
            }
            Command::new("true").spawn()
        });
        assert!(child.unwrap().wait().unwrap().success());
        assert_eq!(busy, 0);
        let other = spawn_when_free(|| Err(std::io::Error::from_raw_os_error(2)));
        assert_eq!(other.unwrap_err().raw_os_error(), Some(2), "other errors are not tried again");
    }

    use super::*;

    #[cfg(unix)]
    #[test]
    fn asking_a_go_program_its_version_never_switches_the_toolchain() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("go");
        std::fs::write(&program, "#!/bin/sh\necho \"$GOTOOLCHAIN\"\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let answer = command_output(&program, &["version"], Duration::from_secs(60)).unwrap();
        assert_eq!(answer.trim(), "local");
    }

    fn sdk(root: &Path, version: &str) {
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join(executable("go")), "").unwrap();
        std::fs::write(root.join("VERSION"), format!("{version}\ntime 2026-10-01T00:00:00Z\n")).unwrap();
    }

    #[test]
    fn sdks_are_found_in_every_place_once() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        sdk(&temp.path().join("goroot"), "go1.27.2");
        sdk(&temp.path().join("usr-local-go"), "go1.26.0");
        sdk(&home.join("sdk/go1.25.4"), "go1.25.4");
        sdk(&home.join("go/pkg/mod/golang.org/toolchain@v0.0.1-go1.28.0.darwin-arm64"), "go1.28.0");
        let env = Environment {
            path: vec![temp.path().join("goroot/bin"), temp.path().join("empty")],
            goroot: Some(temp.path().join("goroot")),
            home: Some(home),
            install_folders: vec![temp.path().join("usr-local-go"), temp.path().join("missing")],
            ..Environment::default()
        };
        let versions: Vec<_> = find_sdks(&env).into_iter().map(|sdk| sdk.version.unwrap()).collect();
        assert_eq!(versions, ["go1.27.2", "go1.26.0", "go1.25.4", "go1.28.0"]);
    }

    #[test]
    fn homebrew_cellar_paths_have_a_stable_alias() {
        assert_eq!(
            homebrew_alias(Path::new("/opt/homebrew/Cellar/go/1.27.2/libexec")),
            Some(PathBuf::from("/opt/homebrew/opt/go/libexec"))
        );
        assert_eq!(
            homebrew_alias(Path::new("/usr/local/Cellar/go@1.26/1.26.5/libexec")),
            Some(PathBuf::from("/usr/local/opt/go@1.26/libexec"))
        );
        assert_eq!(homebrew_alias(Path::new("/opt/homebrew/Cellar/go")), None);
        assert_eq!(homebrew_alias(Path::new("/usr/local/go")), None);
        let temp = tempfile::tempdir().unwrap();
        let keg = temp.path().join("Cellar/go/1.27.2/libexec");
        sdk(&keg, "go1.27.2");
        assert_eq!(stable_root(&keg), keg);
    }

    #[cfg(unix)]
    #[test]
    fn a_shim_on_path_is_asked_for_its_goroot() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let installed = temp.path().join("asdf/installs/golang/1.27.3/go");
        sdk(&installed, "go1.27.3");
        let shim = temp.path().join("asdf/shims/go");
        std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::fs::write(&shim, format!("#!/bin/sh\n[ \"$1 $2\" = \"env GOROOT\" ] && echo '{}'\n", installed.display()))
            .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = Environment {
            path: vec![temp.path().join("asdf/shims")],
            home: Some(temp.path().join("home")),
            ..Environment::default()
        };
        let found = find_sdks(&env);
        assert_eq!(found, [GoSdk { root: installed, version: Some("go1.27.3".to_owned()) }]);
    }

    #[test]
    fn gopls_is_found_where_go_install_puts_it() {
        let temp = tempfile::tempdir().unwrap();
        let env = Environment { home: Some(temp.path().to_owned()), ..Environment::default() };
        let installed = temp.path().join("go/bin").join(executable("gopls"));
        if find_gopls(&env).is_some_and(|found| !found.starts_with(temp.path())) {
            return;
        }
        assert!(find_gopls(&env).is_none());
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        std::fs::write(&installed, "").unwrap();
        assert_eq!(find_gopls(&env), Some(installed));
        let gobin = temp.path().join("gobin");
        std::fs::create_dir_all(&gobin).unwrap();
        std::fs::write(gobin.join(executable("gopls")), "").unwrap();
        let env = Environment { gobin: Some(gobin.clone()), ..env };
        assert_eq!(find_gopls(&env), Some(gobin.join(executable("gopls"))), "GOBIN first");
    }

    #[test]
    fn dlv_is_found_in_gobin_then_gopath_then_path() {
        let temp = tempfile::tempdir().unwrap();
        let place = |folder: &str| {
            let dlv = temp.path().join(folder).join(executable("dlv"));
            std::fs::create_dir_all(dlv.parent().unwrap()).unwrap();
            std::fs::write(&dlv, "").unwrap();
            dlv
        };
        let env = Environment {
            path: vec![temp.path().join("empty"), temp.path().join("path")],
            gopath: vec![temp.path().join("gopath-a"), temp.path().join("gopath-b")],
            gobin: Some(temp.path().join("gobin")),
            home: Some(temp.path().join("home")),
            ..Environment::default()
        };
        assert!(find_dlv(&env).is_none_or(|found| !found.starts_with(temp.path())));
        let on_path = place("path");
        assert_eq!(find_dlv(&env), Some(on_path));
        let in_second = place("gopath-b/bin");
        assert_eq!(find_dlv(&env), Some(in_second));
        let in_first = place("gopath-a/bin");
        assert_eq!(find_dlv(&env), Some(in_first));
        let in_gobin = place("gobin");
        assert_eq!(find_dlv(&env), Some(in_gobin));
        let home = Environment { home: Some(temp.path().join("home")), ..Environment::default() };
        let in_home = place("home/go/bin");
        assert_eq!(find_dlv(&home), Some(in_home), "GOPATH defaults to ~/go");
    }

    #[test]
    fn dlv_versions_are_read_and_checked() {
        let output = "Delve Debugger\nVersion: 1.23.1\nBuild: $Id: 2eba762d75437d380e48fc42213853f13aa2904d $\n";
        assert_eq!(parse_dlv_version(output).as_deref(), Some("1.23.1"));
        assert_eq!(parse_dlv_version("Delve Debugger\nVersion: v1.9.0\n").as_deref(), Some("1.9.0"));
        assert_eq!(parse_dlv_version("Delve Debugger\nVersion: \n"), None);
        assert_eq!(parse_dlv_version("command not found"), None);
        assert!(!dlv_outdated("1.23.1"));
        assert!(!dlv_outdated("1.20.0"));
        assert!(!dlv_outdated("2.0.0"));
        assert!(dlv_outdated("1.19.2"));
        assert!(dlv_outdated("1.9.0"));
        assert!(dlv_outdated("0.12.2"));
        assert!(!dlv_outdated("devel"));
    }

    #[test]
    fn delve_is_installed_with_go_install() {
        let sdk = GoSdk { root: PathBuf::from("/sdk/go"), version: None };
        let command = install_dlv_command(&sdk);
        assert_eq!(command.get_program(), sdk.go().as_os_str());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["install", "github.com/go-delve/delve/cmd/dlv@latest"]);
        let goroot = command.get_envs().find(|(key, _)| *key == "GOROOT").and_then(|(_, value)| value);
        assert_eq!(goroot, Some(sdk.root.as_os_str()));
        let gopls: Vec<_> = install_gopls_command(&sdk).get_args().map(|arg| arg.to_owned()).collect();
        assert_eq!(gopls, ["install", "golang.org/x/tools/gopls@latest"]);
    }

    #[test]
    fn the_go_env_file_is_in_the_user_configuration_folder() {
        let home = Path::new("/home/u");
        assert_eq!(
            go_env_file("macos", Some(home), None, None),
            Some(PathBuf::from("/home/u/Library/Application Support/go/env"))
        );
        assert_eq!(go_env_file("linux", Some(home), None, None), Some(PathBuf::from("/home/u/.config/go/env")));
        assert_eq!(go_env_file("linux", Some(home), Some(Path::new("/xdg")), None), Some(PathBuf::from("/xdg/go/env")));
        assert_eq!(
            go_env_file("linux", Some(home), Some(Path::new("relative")), None),
            Some(PathBuf::from("/home/u/.config/go/env"))
        );
        assert_eq!(
            go_env_file("windows", Some(home), None, Some(Path::new("C:/Users/u/AppData/Roaming"))),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming/go/env"))
        );
        assert_eq!(go_env_file("windows", Some(home), None, None), None);
        assert_eq!(go_env_file("linux", None, None, None), None);
    }

    #[test]
    fn go_env_files_are_parsed() {
        let settings = parse_go_env(
            "# comment
GOPROXY=https://proxy.example.com,direct

  GOMODCACHE = /cache 
not a setting
=x
",
        );
        assert_eq!(
            settings,
            [
                ("GOPROXY".to_owned(), "https://proxy.example.com,direct".to_owned()),
                ("GOMODCACHE".to_owned(), "/cache".to_owned()),
            ]
        );
    }

    #[test]
    fn the_module_cache_comes_from_the_environment_the_file_or_gopath() {
        let home = Environment { home: Some(PathBuf::from("/home/u")), ..Environment::default() };
        assert_eq!(module_cache_with(&home, None), Some(PathBuf::from("/home/u/go/pkg/mod")));
        assert_eq!(
            module_cache_with(
                &home,
                Some(
                    "GOMODCACHE=/from/file
"
                )
            ),
            Some(PathBuf::from("/from/file"))
        );
        assert_eq!(
            module_cache_with(
                &home,
                Some(
                    "GOPATH=/gp
"
                )
            ),
            Some(PathBuf::from("/gp/pkg/mod"))
        );
        assert_eq!(
            module_cache_with(
                &home,
                Some(
                    "GOMODCACHE=
"
                )
            ),
            Some(PathBuf::from("/home/u/go/pkg/mod"))
        );
        let gopath = Environment { gopath: vec![PathBuf::from("/a"), PathBuf::from("/b")], ..home.clone() };
        assert_eq!(
            module_cache_with(
                &gopath,
                Some(
                    "GOPATH=/gp
"
                )
            ),
            Some(PathBuf::from("/a/pkg/mod"))
        );
        let set = Environment { gomodcache: Some(PathBuf::from("/env")), ..gopath };
        assert_eq!(
            module_cache_with(
                &set,
                Some(
                    "GOMODCACHE=/from/file
"
                )
            ),
            Some(PathBuf::from("/env"))
        );
        assert_eq!(module_cache_with(&Environment::default(), None), None);
    }

    #[test]
    fn the_module_cache_reads_the_env_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(
            &file,
            "GOMODCACHE=/written/by/go/env
",
        )
        .unwrap();
        let env = Environment { goenv: Some(file), home: Some(temp.path().to_owned()), ..Environment::default() };
        assert_eq!(module_cache(&env), Some(PathBuf::from("/written/by/go/env")));
        let missing = Environment { goenv: Some(temp.path().join("none")), ..env };
        assert_eq!(module_cache(&missing), Some(temp.path().join("go/pkg/mod")));
    }

    #[test]
    fn versions_are_read() {
        assert_eq!(
            parse_gopls_version("golang.org/x/tools/gopls v0.18.1\n    golang.org/x/tools/gopls@v0.18.1 h1:abc=\n")
                .as_deref(),
            Some("v0.18.1")
        );
        assert_eq!(parse_gopls_version("command not found"), None);
        assert_eq!(parse_go_version("go version go1.27.2 darwin/arm64\n").as_deref(), Some("go1.27.2"));
        assert_eq!(parse_go_version("go version devel +abc"), None);
        assert_eq!(parse_go_version("gopls version"), None);
        let temp = tempfile::tempdir().unwrap();
        sdk(temp.path(), "go1.27.2");
        assert_eq!(sdk_version(temp.path()).as_deref(), Some("go1.27.2"));
    }
}
