use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_update::scripts::{self, Owner, Shell, Written};

#[derive(Clone, Debug)]
pub struct LauncherTarget {
    pub name: String,
    pub executable: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl LauncherTarget {
    pub fn this_process(name: &str) -> Self {
        Self { name: name.to_owned(), executable: std::env::current_exe().ok(), home: scripts::home() }
    }

    fn folder(&self, folder: Option<&Path>) -> Result<PathBuf, CommandError> {
        let folder = match folder {
            Some(folder) => match (folder.strip_prefix("~"), &self.home) {
                (Ok(rest), Some(home)) => home.join(rest),
                _ => folder.to_owned(),
            },
            None => self
                .home
                .as_ref()
                .map(|home| home.join(".local").join("bin"))
                .ok_or_else(|| CommandError::new(t!("workbench-error-launcher-no-home")))?,
        };
        if folder.is_absolute() {
            Ok(folder)
        } else {
            Err(CommandError::new(t!("workbench-error-launcher-relative", path = folder.display().to_string())))
        }
    }

    fn name(&self, name: Option<String>) -> Result<String, CommandError> {
        let name = name.map_or_else(|| self.name.clone(), |name| name.trim().to_owned());
        let valid = !name.is_empty()
            && !name.starts_with('.')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if valid { Ok(name) } else { Err(CommandError::new(t!("workbench-error-launcher-name", name = name))) }
    }
}

/// Parameters of `tools.commandLineLauncher.status` and `tools.commandLineLauncher.create`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LauncherParams {
    /// The folder of the script, absolute or starting with `~`; `~/.local/bin` when omitted.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// The script's name; the product id (`gogarden`) when omitted.
    #[serde(default)]
    pub name: Option<String>,
    /// `create` only: also put the folder on `PATH` by adding one line to `~/.zprofile` (and
    /// `~/.bash_profile` if it exists). It edits the user's shell files: only when the user asked.
    #[serde(default)]
    pub add_to_path: bool,
}

/// What the launcher commands report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LauncherReport {
    /// The script.
    pub script: PathBuf,
    /// Whether the script is there, written by Zen.
    pub exists: bool,
    /// Whether the folder is on the `PATH` of a new terminal window (the login shell is asked).
    pub on_path: bool,
    /// Whether a shell profile has the line already.
    pub in_profile: bool,
    /// The line that puts the folder on `PATH`.
    pub line: String,
    /// The shell profiles the line goes to.
    pub profiles: Vec<PathBuf>,
    /// The shell profiles `create` changed.
    pub changed: Vec<PathBuf>,
}

fn report(target: &LauncherTarget, folder: &Path, name: &str, changed: Vec<PathBuf>) -> LauncherReport {
    let script = folder.join(Shell::current().file_name(name));
    let status = scripts::path_status(folder, target.home.as_deref());
    LauncherReport {
        exists: scripts::owner(&script) == Owner::Ours,
        script,
        on_path: status.on_path,
        in_profile: status.in_profile,
        line: status.line,
        profiles: status.profiles,
        changed,
    }
}

pub struct LauncherStatus(pub LauncherTarget);

impl Command for LauncherStatus {
    type Params = LauncherParams;
    type Output = LauncherReport;
    const ID: &'static str = "tools.commandLineLauncher.status";
    const TITLE: &'static str = "Command-line Launcher Status";
    const DESCRIPTION: &'static str = "Says where this IDE's command-line launcher (a shell script named by the \
        product id, e.g. gogarden) is or would be, whether it is there, and whether its folder is on the PATH of a \
        new terminal window, with the exact line that would put it there. Changes nothing.";

    fn run(&self, _: &mut Session, params: LauncherParams) -> Result<LauncherReport, CommandError> {
        let folder = self.0.folder(params.folder.as_deref())?;
        let name = self.0.name(params.name)?;
        Ok(report(&self.0, &folder, &name, Vec::new()))
    }
}

pub struct CreateLauncher(pub LauncherTarget);

impl Command for CreateLauncher {
    type Params = LauncherParams;
    type Output = LauncherReport;
    const ID: &'static str = "tools.commandLineLauncher.create";
    const TITLE: &'static str = "Create Command-line Launcher…";
    const DESCRIPTION: &'static str = "Writes a shell script that starts this IDE with every argument it is given \
        (gogarden ., gogarden --headless ., gogarden mcp --headless ., gogarden main.go:12) into a folder \
        (~/.local/bin by default). A file of that name that Zen did not write is never replaced. With addToPath, \
        also adds one line putting the folder on PATH to ~/.zprofile (and ~/.bash_profile if it exists): only when \
        the user asked for it.";

    fn run(&self, _: &mut Session, params: LauncherParams) -> Result<LauncherReport, CommandError> {
        let folder = self.0.folder(params.folder.as_deref())?;
        let name = self.0.name(params.name)?;
        let executable =
            self.0.executable.as_deref().ok_or_else(|| CommandError::new(t!("workbench-error-launcher-executable")))?;
        let script = folder.join(Shell::current().file_name(&name));
        match scripts::write_launcher(&folder, &name, executable, Shell::current()) {
            Ok(Written::Written) => {}
            Ok(Written::Skipped) => {
                return Err(CommandError::new(t!(
                    "workbench-error-launcher-foreign",
                    path = script.display().to_string()
                )));
            }
            Err(error) => {
                return Err(CommandError::new(t!(
                    "workbench-error-launcher-write",
                    path = script.display().to_string(),
                    error = error.to_string()
                )));
            }
        }
        let changed = match (params.add_to_path, &self.0.home) {
            (false, _) => Vec::new(),
            (true, None) => return Err(CommandError::new(t!("workbench-error-launcher-no-home"))),
            (true, Some(home)) => scripts::add_to_profiles(&folder, home).map_err(|error| {
                CommandError::new(t!("workbench-error-launcher-profile", error = error.to_string()))
            })?,
        };
        Ok(report(&self.0, &folder, &name, changed))
    }
}

pub fn register(registry: &mut Registry, target: LauncherTarget) -> Result<(), RegistryError> {
    registry.register(LauncherStatus(target.clone()))?;
    registry.register(CreateLauncher(target))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn setup() -> (tempfile::TempDir, Registry, Session, PathBuf) {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut registry = Registry::new();
        let target = LauncherTarget {
            name: "gogarden".into(),
            executable: Some(PathBuf::from("/Applications/GoGarden.app/Contents/MacOS/gogarden")),
            home: Some(home.clone()),
        };
        register(&mut registry, target).unwrap();
        (dir, registry, Session::new(), home)
    }

    #[test]
    fn the_launcher_goes_to_local_bin_and_starts_this_ide() {
        let (_dir, registry, mut session, home) = setup();
        let status = registry.dispatch(&mut session, "tools.commandLineLauncher.status", json!({})).unwrap();
        let script = home.join(".local/bin").join(Shell::current().file_name("gogarden"));
        assert_eq!(status["script"], json!(script));
        assert_eq!(status["exists"], false);
        assert_eq!(status["line"], r#"export PATH="$HOME/.local/bin:$PATH""#);
        assert!(!script.exists(), "asking writes nothing");

        let created = registry.dispatch(&mut session, "tools.commandLineLauncher.create", json!({})).unwrap();
        assert_eq!(created["exists"], true);
        assert_eq!(created["changed"], json!([]));
        assert!(
            std::fs::read_to_string(&script).unwrap().contains("/Applications/GoGarden.app/Contents/MacOS/gogarden")
        );
        assert!(!home.join(".zprofile").exists(), "no shell profile without addToPath");
    }

    #[test]
    fn the_path_line_is_added_only_when_asked() {
        let (_dir, registry, mut session, home) = setup();
        let params = json!({ "folder": "~/bin", "name": "gg", "addToPath": true });
        let created = registry.dispatch(&mut session, "tools.commandLineLauncher.create", params.clone()).unwrap();
        assert_eq!(created["script"], json!(home.join("bin").join(Shell::current().file_name("gg"))));
        assert_eq!(created["changed"], json!([home.join(".zprofile")]));
        let profile = std::fs::read_to_string(home.join(".zprofile")).unwrap();
        assert!(profile.contains(r#"export PATH="$HOME/bin:$PATH""#), "{profile}");
        let again = registry.dispatch(&mut session, "tools.commandLineLauncher.create", params).unwrap();
        assert_eq!(again["changed"], json!([]), "never twice");
        assert_eq!(again["inProfile"], true);
    }

    #[test]
    fn someone_elses_file_and_bad_names_are_refused() {
        let (_dir, registry, mut session, home) = setup();
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let mine = bin.join(Shell::current().file_name("gogarden"));
        std::fs::write(&mine, "#!/bin/sh\necho mine\n").unwrap();
        let error = registry.dispatch(&mut session, "tools.commandLineLauncher.create", json!({})).unwrap_err();
        assert!(error.to_string().contains("not written by Zen"), "{error}");
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "#!/bin/sh\necho mine\n");
        for params in [json!({ "name": "../x" }), json!({ "name": "" }), json!({ "folder": "bin" })] {
            assert!(registry.dispatch(&mut session, "tools.commandLineLauncher.create", params).is_err());
        }
    }
}
