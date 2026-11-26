use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_update::install::{Installation, Launch};
pub use zen_update::scripts::{
    CompletionShell, MARKER, Shell, ZEN_CLI, default_location, expand_home, on_path, script,
};
use zen_update::scripts::{Owner, owner};

use crate::Tool;

pub const LEGACY_NAMES: &[&str] =
    &["ronin", "garden", "petal", "wave", "crane", "shuriken", "pagoda", "ryokan", "scroll", "daruma", "totami"];

pub fn executable(installation: &Installation, id: &str) -> Option<PathBuf> {
    match installation.launch(id)? {
        Launch::Bundle(bundle) => Some(bundle.join("Contents").join("MacOS").join(id)).filter(|path| path.is_file()),
        Launch::Binary(binary) => Some(binary),
    }
}

pub fn zen_cli(installation: &Installation, id: &str) -> Option<PathBuf> {
    let name = zen_update::install::executable_name(ZEN_CLI);
    let path = match installation.launch(id)? {
        Launch::Bundle(bundle) => bundle.join("Contents").join("MacOS").join(name),
        Launch::Binary(binary) => binary.parent()?.join(name),
    };
    path.is_file().then_some(path)
}

/// What generating or removing launchers did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The directory that holds the launchers.
    pub location: PathBuf,
    /// The launchers written or rewritten.
    pub written: Vec<PathBuf>,
    /// The launchers deleted: of tools that are no longer installed, or all of them on removal.
    pub removed: Vec<PathBuf>,
    /// Files with a launcher's name that Zen Toolbox did not write, left untouched.
    pub skipped: Vec<PathBuf>,
    /// Whether the directory is on this process's `PATH`; `toolbox.shellScripts.status` asks a new
    /// terminal window too.
    pub on_path: bool,
}

impl Report {
    fn new(dir: &Path) -> Self {
        Self { location: dir.to_owned(), on_path: on_path(dir, std::env::var_os("PATH").as_deref()), ..Self::default() }
    }

    fn keep(&mut self, dir: &Path, name: &str, executable: Option<PathBuf>, shell: Shell) -> Result<(), String> {
        let path = dir.join(shell.file_name(name));
        let Some(executable) = executable else { return self.drop_ours(&path, false) };
        if matches!(owner(&path), Owner::Foreign) {
            self.skipped.push(path);
            return Ok(());
        }
        let executable = executable
            .to_str()
            .ok_or_else(|| t!("toolbox-error-launcher-path", path = executable.display().to_string()))?;
        fs::create_dir_all(dir).map_err(|error| failure("toolbox-error-launcher-directory", dir, &error))?;
        zen_update::scripts::write(&path, &script(executable, shell))
            .map_err(|error| failure("toolbox-error-launcher-write", &path, &error))?;
        self.written.push(path);
        Ok(())
    }

    fn drop_ours(&mut self, path: &Path, report_foreign: bool) -> Result<(), String> {
        match owner(path) {
            Owner::Absent => {}
            Owner::Ours => {
                zen_update::scripts::delete(path)
                    .map_err(|error| failure("toolbox-error-launcher-remove", path, &error))?;
                self.removed.push(path.to_owned());
            }
            Owner::Foreign if report_foreign => self.skipped.push(path.to_owned()),
            Owner::Foreign => {}
        }
        Ok(())
    }
}

pub fn generate(tools: &[Tool], installations: &[Installation], dir: &Path, shell: Shell) -> Result<Report, String> {
    let mut report = Report::new(dir);
    let mut zen = None;
    for tool in tools {
        let executable = installations.iter().find_map(|installation| executable(installation, &tool.id));
        if zen.is_none() && executable.is_some() {
            zen = installations.iter().find_map(|installation| zen_cli(installation, &tool.id));
        }
        report.keep(dir, &tool.launcher, executable, shell)?;
    }
    report.keep(dir, ZEN_CLI, zen, shell)?;
    for name in LEGACY_NAMES {
        report.drop_ours(&dir.join(shell.file_name(name)), false)?;
    }
    Ok(report)
}

pub fn follow_with_completions(report: &mut Report, home: &Path, shell: Shell) -> Result<(), String> {
    if shell != Shell::Posix {
        return Ok(());
    }
    let zen_is_there = owner(&report.location.join(ZEN_CLI)) == Owner::Ours;
    for completion in CompletionShell::ALL {
        let path = completion.file(home);
        if !zen_is_there || !completion.in_use(home) {
            report.drop_ours(&path, false)?;
        } else if owner(&path) == Owner::Foreign {
            report.skipped.push(path);
        } else {
            let folder = path.parent().unwrap_or(home);
            fs::create_dir_all(folder).map_err(|error| failure("toolbox-error-launcher-directory", folder, &error))?;
            zen_update::scripts::write(&path, completion.script())
                .map_err(|error| failure("toolbox-error-launcher-write", &path, &error))?;
            report.written.push(path);
        }
    }
    Ok(())
}

pub fn remove_completions(report: &mut Report, home: &Path) -> Result<(), String> {
    for completion in CompletionShell::ALL {
        report.drop_ours(&completion.file(home), true)?;
    }
    Ok(())
}

pub fn remove(tools: &[Tool], dir: &Path, shell: Shell) -> Result<Report, String> {
    let mut report = Report::new(dir);
    for name in tools.iter().map(|tool| tool.launcher.as_str()).chain([ZEN_CLI]) {
        report.drop_ours(&dir.join(shell.file_name(name)), true)?;
    }
    for name in LEGACY_NAMES {
        report.drop_ours(&dir.join(shell.file_name(name)), false)?;
    }
    Ok(report)
}

fn failure(id: &str, path: &Path, error: &std::io::Error) -> String {
    t!(id, path = path.display().to_string(), error = error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn archive(paths: &[String]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for path in paths {
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, &b"x"[..]).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn install(installation: &Installation, id: &str) {
        let paths = [format!("bin/{}", zen_update::install::executable_name(id))];
        installation.install(id, "0.1.0", &archive(&paths)).unwrap();
    }

    fn install_bundle(installation: &Installation, id: &str, name: &str) {
        let paths = [
            format!("{name}.app/Contents/MacOS/{id}"),
            format!("{name}.app/Contents/MacOS/zen"),
            format!("{name}.app/Contents/Info.plist"),
        ];
        installation.install(id, "0.1.0", &archive(&paths)).unwrap();
    }

    #[test]
    fn a_posix_launcher_passes_every_argument_on() {
        assert_eq!(
            script("/opt/tools/gogarden/0.1.0/bin/gogarden", Shell::Posix),
            "#!/bin/sh\n# Generated by Zen Toolbox. Do not edit.\nexec \"/opt/tools/gogarden/0.1.0/bin/gogarden\" \"$@\"\n"
        );
    }

    #[test]
    fn a_posix_launcher_quotes_spaces_quotes_and_dollars() {
        let text = script("/Users/a b/My \"IDE\"/$HOME/`x`\\y/gogarden", Shell::Posix);
        assert_eq!(text.lines().nth(2).unwrap(), r#"exec "/Users/a b/My \"IDE\"/\$HOME/\`x\`\\y/gogarden" "$@""#);
    }

    #[test]
    fn a_windows_launcher_is_a_batch_file() {
        assert_eq!(
            script(r"C:\Users\A B\Zen Toolbox\tools\gogarden\0.1.0\bin\gogarden.exe", Shell::Cmd),
            "@echo off\r\nREM Generated by Zen Toolbox. Do not edit.\r\n\"C:\\Users\\A B\\Zen Toolbox\\tools\\gogarden\\0.1.0\\bin\\gogarden.exe\" %*\r\n"
        );
        assert_eq!(script(r"C:\100%\x.exe", Shell::Cmd).lines().nth(2).unwrap(), "\"C:\\100%%\\x.exe\" %*");
        assert_eq!(Shell::Cmd.file_name("gogarden"), "gogarden.cmd");
        assert_eq!(Shell::Posix.file_name("gogarden"), "gogarden");
    }

    #[test]
    fn completions_follow_the_zen_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let installation = Installation::new(dir.path().join("tools")).with_applications(dir.path().join("Apps"));
        install_bundle(&installation, "gogarden", "GoGarden");
        let bin = dir.path().join("bin");
        let installations = std::slice::from_ref(&installation);
        let mut report = generate(&catalog(), installations, &bin, Shell::Posix).unwrap();
        follow_with_completions(&mut report, &home, Shell::Posix).unwrap();
        let (zsh, bash, fish) =
            (CompletionShell::Zsh.file(&home), CompletionShell::Bash.file(&home), CompletionShell::Fish.file(&home));
        assert!(report.written.contains(&zsh) && report.written.contains(&bash));
        assert_eq!(fs::read_to_string(&bash).unwrap(), CompletionShell::Bash.script());
        assert!(!fish.exists(), "no fish on this machine");

        installation.uninstall("gogarden").unwrap();
        let mut report = generate(&catalog(), installations, &bin, Shell::Posix).unwrap();
        follow_with_completions(&mut report, &home, Shell::Posix).unwrap();
        assert!(report.removed.contains(&zsh) && !zsh.exists(), "no zen, no completions: {report:?}");
    }

    #[test]
    fn a_completion_file_someone_else_wrote_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let installation = Installation::new(dir.path().join("tools")).with_applications(dir.path().join("Apps"));
        install_bundle(&installation, "gogarden", "GoGarden");
        let bin = dir.path().join("bin");
        let mut report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::Posix).unwrap();
        let zsh = CompletionShell::Zsh.file(&home);
        fs::create_dir_all(zsh.parent().unwrap()).unwrap();
        fs::write(&zsh, "#compdef zen\nmine\n").unwrap();
        follow_with_completions(&mut report, &home, Shell::Posix).unwrap();
        assert!(report.skipped.contains(&zsh));
        assert_eq!(fs::read_to_string(&zsh).unwrap(), "#compdef zen\nmine\n");

        let mut report = remove(&catalog(), &bin, Shell::Posix).unwrap();
        remove_completions(&mut report, &home).unwrap();
        assert!(report.skipped.contains(&zsh) && zsh.exists());
        assert!(report.removed.contains(&CompletionShell::Bash.file(&home)));
    }

    #[test]
    fn launchers_are_named_by_the_product_id() {
        for tool in catalog() {
            assert_eq!(tool.launcher, tool.id, "{} types as its id", tool.name);
        }
    }

    #[test]
    fn a_bundle_launcher_starts_the_executable_inside_the_bundle_and_zen_its_zen_cli() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("Applications");
        let installation = Installation::new(dir.path().join("tools")).with_applications(&applications);
        install_bundle(&installation, "gogarden", "GoGarden");
        let bundle = installation.launch("gogarden").map(|launch| match launch {
            Launch::Bundle(bundle) | Launch::Binary(bundle) => bundle,
        });
        let bundle = bundle.unwrap();
        assert_eq!(executable(&installation, "gogarden"), Some(bundle.join("Contents/MacOS/gogarden")));
        assert_eq!(zen_cli(&installation, "gogarden"), Some(bundle.join("Contents/MacOS/zen")));

        let bin = dir.path().join("bin");
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::Posix).unwrap();
        assert_eq!(report.written, [bin.join("gogarden"), bin.join("zen")]);
        let zen = fs::read_to_string(bin.join("zen")).unwrap();
        assert!(zen.contains(&format!("exec \"{}\" \"$@\"", bundle.join("Contents/MacOS/zen").display())), "{zen}");

        installation.uninstall("gogarden").unwrap();
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::Posix).unwrap();
        assert_eq!(report.removed, [bin.join("gogarden"), bin.join("zen")], "uninstalling the last IDE takes zen too");
    }

    #[test]
    fn an_ide_without_zen_cli_gets_no_zen_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        let bin = dir.path().join("bin");
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::current()).unwrap();
        assert_eq!(report.written, [bin.join(Shell::current().file_name("gogarden"))]);
    }

    #[test]
    fn launchers_of_the_old_names_are_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let shell = Shell::current();
        let garden = bin.join(shell.file_name("garden"));
        fs::write(&garden, script("/opt/old/gogarden", shell)).unwrap();
        let wave = bin.join(shell.file_name("wave"));
        fs::write(&wave, "#!/bin/sh\necho my own wave\n").unwrap();
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, shell).unwrap();
        assert_eq!(report.written, [bin.join(shell.file_name("gogarden"))]);
        assert_eq!(report.removed, std::slice::from_ref(&garden));
        assert!(!garden.exists());
        assert!(report.skipped.is_empty(), "someone else's `wave` is no business of a launcher named by id");
        assert!(wave.exists());
    }

    #[test]
    fn generate_and_remove_touch_only_marked_files() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        let bin = dir.path().join("my bin");
        let shell = Shell::current();
        install(&installation, "gogarden");
        install(&installation, "rustronin");
        fs::create_dir_all(&bin).unwrap();
        let foreign = bin.join(shell.file_name("rustronin"));
        fs::write(&foreign, "#!/bin/sh\necho mine\n").unwrap();
        let unrelated = bin.join("gogarden-notes");
        fs::write(&unrelated, "keep").unwrap();

        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, shell).unwrap();
        let gogarden = bin.join(shell.file_name("gogarden"));
        assert_eq!(report.written, std::slice::from_ref(&gogarden));
        assert_eq!(report.skipped, std::slice::from_ref(&foreign));
        assert!(!report.on_path);
        let expected = script(executable(&installation, "gogarden").unwrap().to_str().unwrap(), shell);
        assert_eq!(fs::read_to_string(&gogarden).unwrap(), expected);
        assert_eq!(
            fs::read_to_string(&foreign).unwrap(),
            "#!/bin/sh\necho mine\n",
            "a file without the marker is kept"
        );

        installation.uninstall("gogarden").unwrap();
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, shell).unwrap();
        assert_eq!(report.removed, std::slice::from_ref(&gogarden), "the launcher of an uninstalled tool goes");
        assert!(!gogarden.exists());

        install(&installation, "gogarden");
        generate(&catalog(), std::slice::from_ref(&installation), &bin, shell).unwrap();
        let report = remove(&catalog(), &bin, shell).unwrap();
        assert_eq!(report.removed, std::slice::from_ref(&gogarden));
        assert_eq!(report.skipped, std::slice::from_ref(&foreign));
        assert!(foreign.exists());
        assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
        let left: Vec<_> = fs::read_dir(&bin).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(left.len(), 2, "no staging files are left: {left:?}");
    }

    #[test]
    fn the_marker_must_be_a_comment_line_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let gogarden = bin.join(Shell::current().file_name("gogarden"));
        let mine = format!("#!/bin/sh\necho '{MARKER}'\n");
        fs::write(&gogarden, &mine).unwrap();
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::current()).unwrap();
        assert_eq!(report.skipped, std::slice::from_ref(&gogarden));
        assert_eq!(fs::read_to_string(&gogarden).unwrap(), mine);
        assert!(remove(&catalog(), &bin, Shell::current()).unwrap().removed.is_empty());
    }

    #[test]
    fn the_first_installation_that_has_a_tool_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (new, old) = (Installation::new(dir.path().join("new")), Installation::new(dir.path().join("old")));
        install(&old, "gogarden");
        install(&old, "rustronin");
        install(&new, "rustronin");
        let bin = dir.path().join("bin");
        let report = generate(&catalog(), &[new.clone(), old.clone()], &bin, Shell::current()).unwrap();
        assert_eq!(report.written.len(), 2);
        let rustronin = fs::read_to_string(bin.join(Shell::current().file_name("rustronin"))).unwrap();
        assert!(rustronin.contains(executable(&new, "rustronin").unwrap().to_str().unwrap()));
        let gogarden = fs::read_to_string(bin.join(Shell::current().file_name("gogarden"))).unwrap();
        assert!(gogarden.contains(executable(&old, "gogarden").unwrap().to_str().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        install(&installation, "rustronin");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, script("/opt/x", Shell::Posix)).unwrap();
        std::os::unix::fs::symlink(&victim, bin.join("rustronin")).unwrap();
        std::os::unix::fs::symlink(&victim, bin.join(".gogarden.zen-partial")).unwrap();

        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::Posix).unwrap();
        assert_eq!(report.skipped, [bin.join("rustronin")], "a link to a marked file is not ours");
        assert_eq!(report.written, [bin.join("gogarden")]);
        assert_eq!(
            fs::read_to_string(&victim).unwrap(),
            script("/opt/x", Shell::Posix),
            "nothing is written through a link"
        );
        assert!(fs::symlink_metadata(bin.join("gogarden")).unwrap().is_file());

        remove(&catalog(), &bin, Shell::Posix).unwrap();
        assert!(victim.exists());
        assert!(fs::symlink_metadata(bin.join("rustronin")).unwrap().file_type().is_symlink());
    }

    #[test]
    fn generate_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        let bin = dir.path().join("a/b/bin");
        let report = generate(&catalog(), std::slice::from_ref(&installation), &bin, Shell::current()).unwrap();
        assert_eq!(report.location, bin);
        assert_eq!(report.written.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn launchers_are_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path().join("tools"));
        install(&installation, "gogarden");
        let report =
            generate(&catalog(), std::slice::from_ref(&installation), &dir.path().join("bin"), Shell::Posix).unwrap();
        let mode = fs::metadata(&report.written[0]).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn the_path_check_finds_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([other.path(), dir.path()]).unwrap();
        assert!(on_path(dir.path(), Some(&path)));
        assert!(!on_path(&dir.path().join("missing"), Some(&path)));
        assert!(!on_path(dir.path(), None));
    }

    #[test]
    fn a_tilde_means_the_home_directory() {
        let Some(home) = zen_update::scripts::home() else { return };
        assert_eq!(expand_home(Path::new("~/.local/bin")), home.join(".local/bin"));
        assert_eq!(expand_home(Path::new("/opt/bin")), PathBuf::from("/opt/bin"));
        assert_eq!(default_location(), Some(home.join(".local").join("bin")));
    }
}
