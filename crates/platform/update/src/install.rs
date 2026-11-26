use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct InstallError(pub String);

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InstallError {}

fn io_error(what: &str, path: &Path, error: io::Error) -> InstallError {
    InstallError(format!("cannot {what} {}: {error}", path.display()))
}

pub fn executable_name(id: &str) -> String {
    format!("{id}{}", std::env::consts::EXE_SUFFIX)
}

#[derive(Clone, Debug)]
pub struct Installation {
    root: PathBuf,
    applications: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    Bundle(PathBuf),
    Binary(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl Launch {
    pub fn command_line(&self, args: &[OsString]) -> LaunchCommand {
        match self {
            Self::Bundle(bundle) if args.is_empty() => {
                LaunchCommand { program: PathBuf::from("open"), args: vec![bundle.clone().into_os_string()] }
            }
            Self::Bundle(bundle) => {
                let mut line = vec![OsString::from("-n"), OsString::from("-a"), bundle.clone().into_os_string()];
                line.push(OsString::from("--args"));
                line.extend(args.iter().cloned());
                LaunchCommand { program: PathBuf::from("open"), args: line }
            }
            Self::Binary(binary) => LaunchCommand { program: binary.clone(), args: args.to_vec() },
        }
    }
}

impl LaunchCommand {
    pub fn to_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

impl Installation {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), applications: None }
    }

    pub fn with_applications(mut self, folder: impl Into<PathBuf>) -> Self {
        self.applications = Some(folder.into());
        self
    }

    pub fn default_applications() -> Option<PathBuf> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let system = PathBuf::from("/Applications");
        let probe = system.join(format!(".zen-toolbox-probe-{}", std::process::id()));
        if fs::write(&probe, b"").is_ok() {
            let _ = fs::remove_file(&probe);
            return Some(system);
        }
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Applications"))
    }

    pub fn default_root() -> Option<PathBuf> {
        let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        if let Some(base) = env("ZEN_DATA_DIR") {
            Some(base.join("space.ifzenelse.toolbox").join("tools"))
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|home| home.join("Library/Application Support/space.ifzenelse.toolbox/tools"))
        } else if cfg!(windows) {
            env("LOCALAPPDATA").map(|base| base.join("Zen Toolbox").join("tools"))
        } else {
            env("XDG_DATA_HOME")
                .or_else(|| env("HOME").map(|home| home.join(".local/share")))
                .map(|base| base.join("zen-toolbox").join("tools"))
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn pointer(&self, id: &str, name: &str) -> Option<String> {
        let text = fs::read_to_string(self.root.join(id).join(name)).ok()?;
        let version = text.trim().to_owned();
        (!version.is_empty() && self.root.join(id).join(&version).is_dir()).then_some(version)
    }

    fn set_pointer(&self, id: &str, name: &str, version: Option<&str>) -> Result<(), InstallError> {
        let path = self.root.join(id).join(name);
        match version {
            None => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(io_error("remove", &path, error)),
            },
            Some(version) => {
                let temporary = self.root.join(id).join(format!(".{name}.partial"));
                fs::write(&temporary, format!("{version}\n")).map_err(|error| io_error("write", &temporary, error))?;
                fs::rename(&temporary, &path).map_err(|error| io_error("update", &path, error))
            }
        }
    }

    pub fn current(&self, id: &str) -> Option<String> {
        self.pointer(id, "current")
    }

    pub fn executable(&self, id: &str) -> Option<PathBuf> {
        let version = self.current(id)?;
        let path = self.root.join(id).join(version).join("bin").join(executable_name(id));
        path.is_file().then_some(path)
    }

    pub fn launch(&self, id: &str) -> Option<Launch> {
        if let Some(exported) = self.exported(id).filter(|path| path.is_dir()) {
            return Some(Launch::Bundle(exported));
        }
        let version = self.current(id)?;
        if let Some(bundle) = bundle_in(&self.root.join(id).join(&version)) {
            return Some(Launch::Bundle(bundle));
        }
        self.executable(id).map(Launch::Binary)
    }

    pub fn launch_with(&self, id: &str, args: &[OsString]) -> Option<LaunchCommand> {
        self.launch(id).map(|launch| launch.command_line(args))
    }

    pub fn exported(&self, id: &str) -> Option<PathBuf> {
        let text = fs::read_to_string(self.root.join(id).join("exported")).ok()?;
        Some(PathBuf::from(text.trim())).filter(|path| !path.as_os_str().is_empty())
    }

    fn export(&self, id: &str) -> Result<(), InstallError> {
        let Some(applications) = &self.applications else { return Ok(()) };
        let bundle = self.current(id).and_then(|version| bundle_in(&self.root.join(id).join(version)));
        let previous = self.exported(id);
        let Some(bundle) = bundle else {
            if let Some(previous) = previous {
                let _ = fs::remove_dir_all(previous);
                let _ = fs::remove_file(self.root.join(id).join("exported"));
            }
            return Ok(());
        };
        fs::create_dir_all(applications).map_err(|error| io_error("create", applications, error))?;
        let name = bundle.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let target = applications.join(&name);
        let staging = applications.join(format!(".{name}.zen-partial"));
        let _ = fs::remove_dir_all(&staging);
        copy_tree(&bundle, &staging)?;
        let replaced = applications.join(format!(".{name}.zen-old"));
        let _ = fs::remove_dir_all(&replaced);
        if target.exists() {
            fs::rename(&target, &replaced).map_err(|error| io_error("replace", &target, error))?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            let _ = fs::rename(&replaced, &target);
            return Err(io_error("install", &target, error));
        }
        let _ = fs::remove_dir_all(&replaced);
        if let Some(previous) = previous.filter(|previous| *previous != target) {
            let _ = fs::remove_dir_all(previous);
        }
        refresh_system_icon(&target);
        let record = self.root.join(id).join("exported");
        fs::write(&record, format!("{}\n", target.display())).map_err(|error| io_error("write", &record, error))
    }

    pub fn install(&self, id: &str, version: &str, archive: &[u8]) -> Result<(), InstallError> {
        let tool = self.root.join(id);
        fs::create_dir_all(&tool).map_err(|error| io_error("create", &tool, error))?;
        let staging = tool.join(format!(".{version}.partial"));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|error| io_error("clean", &staging, error))?;
        }
        unpack_tar_gz(archive, &staging)?;
        if !staging.join("bin").join(executable_name(id)).is_file() && bundle_in(&staging).is_none() {
            let _ = fs::remove_dir_all(&staging);
            return Err(InstallError(format!(
                "the archive has no bin/{} or application bundle to launch",
                executable_name(id)
            )));
        }
        let target = tool.join(version);
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|error| io_error("replace", &target, error))?;
        }
        fs::rename(&staging, &target).map_err(|error| io_error("install", &target, error))?;

        self.set_pointer(id, "current", Some(version))?;
        self.prune(id);
        self.export(id)
    }

    pub fn uninstall(&self, id: &str) -> Result<(), InstallError> {
        if let Some(exported) = self.exported(id) {
            let _ = fs::remove_dir_all(exported);
        }
        let tool = self.root.join(id);
        match fs::remove_dir_all(&tool) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error("remove", &tool, error)),
        }
    }

    pub fn prune(&self, id: &str) {
        let _ = fs::remove_file(self.root.join(id).join("previous"));
        let keep = [self.current(id)];
        let Ok(entries) = fs::read_dir(self.root.join(id)) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_version = entry.path().is_dir() && !name.starts_with('.');
            let is_leftover = entry.path().is_dir() && name.ends_with(".partial");
            if (is_version && !keep.iter().flatten().any(|kept| *kept == name)) || is_leftover {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn bundle_in(directory: &Path) -> Option<PathBuf> {
    fs::read_dir(directory).ok()?.flatten().map(|entry| entry.path()).find(|path| {
        path.is_dir() && path.extension().is_some_and(|extension| extension == "app") && path.join("Contents").is_dir()
    })
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(to).map_err(|error| io_error("create", to, error))?;
    for entry in fs::read_dir(from).map_err(|error| io_error("read", from, error))?.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| io_error("read", &source, error))?;
        if kind.is_symlink() {
            let link = fs::read_link(&source).map_err(|error| io_error("read", &source, error))?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link, &target).map_err(|error| io_error("link", &target, error))?;
            #[cfg(not(unix))]
            let _ = link;
        } else if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            fs::copy(&source, &target).map_err(|error| io_error("copy", &source, error))?;
        }
    }
    Ok(())
}

pub fn unpack_tar_gz(archive: &[u8], destination: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(destination).map_err(|error| io_error("create", destination, error))?;
    let mut reader = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    reader.set_preserve_permissions(true);
    for entry in
        reader.entries().map_err(|error| InstallError(format!("the archive is not a valid .tar.gz: {error}")))?
    {
        let mut entry = entry.map_err(|error| InstallError(format!("the archive is damaged: {error}")))?;
        let metadata =
            entry.path().ok().and_then(|path| path.file_name().map(|name| name.to_string_lossy().starts_with("._")));
        if metadata == Some(true) {
            continue;
        }
        let unpacked = entry.unpack_in(destination).map_err(|error| io_error("unpack into", destination, error))?;
        if !unpacked {
            return Err(InstallError("the archive contains a path outside its own directory".to_owned()));
        }
    }
    Ok(())
}

pub fn running_programs() -> Option<Vec<PathBuf>> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "comm="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).lines().map(|line| PathBuf::from(line.trim())).collect())
}

pub fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && bundle.extension()? == "app")
        .then(|| bundle.to_owned())
}

pub fn bundle_in_release(directory: &Path) -> Option<PathBuf> {
    bundle_in(directory)
}

pub fn replace_bundle(current: &Path, new: &Path) -> Result<(), InstallError> {
    let name = current.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = current.parent().ok_or_else(|| InstallError(format!("{} has no parent folder", current.display())))?;
    let staging = parent.join(format!(".{name}.zen-partial"));
    let _ = fs::remove_dir_all(&staging);
    copy_tree(new, &staging)?;
    let backup = parent.join(format!(".{name}.zen-old"));
    let _ = fs::remove_dir_all(&backup);
    fs::rename(current, &backup).map_err(|error| io_error("move aside", current, error))?;
    if let Err(error) = fs::rename(&staging, current) {
        let _ = fs::rename(&backup, current);
        return Err(io_error("install", current, error));
    }
    let _ = fs::remove_dir_all(&backup);
    refresh_system_icon(current);
    Ok(())
}

pub fn refresh_system_icon(bundle: &Path) {
    if let Ok(directory) = fs::File::open(bundle) {
        let _ = directory.set_modified(std::time::SystemTime::now());
    }
    #[cfg(target_os = "macos")]
    {
        const LSREGISTER: &str =
            "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        let _ = std::process::Command::new(LSREGISTER)
            .arg("-f")
            .arg(bundle)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

pub fn replace_executable(current: &Path, new_executable: &[u8]) -> Result<(), InstallError> {
    let staged = current.with_extension("new");
    fs::write(&staged, new_executable).map_err(|error| io_error("write", &staged, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map_err(|error| io_error("mark executable", &staged, error))?;
    }
    let backup = current.with_extension("zen-old");
    let _ = fs::remove_file(&backup);
    fs::rename(current, &backup).map_err(|error| io_error("move aside", current, error))?;
    if let Err(error) = fs::rename(&staged, current) {
        let _ = fs::rename(&backup, current);
        return Err(io_error("install", current, error));
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

pub fn remove_leftovers(executable: &Path) {
    for leftover in [executable.with_extension("previous"), executable.with_extension("zen-old")] {
        let _ = fs::remove_file(leftover);
    }
    if let Some(bundle) = bundle_of(executable)
        && let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name())
    {
        let name = name.to_string_lossy();
        for leftover in [
            parent.join(format!("{name}.previous")),
            parent.join(format!(".{name}.zen-old")),
            parent.join(format!(".{name}.zen-partial")),
        ] {
            let _ = fs::remove_dir_all(leftover);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn archive(id: &str, content: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, format!("bin/{}", executable_name(id)), content.as_bytes()).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn install_update_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Installation::new(dir.path());
        assert_eq!(tools.current("rustronin"), None);

        tools.install("rustronin", "0.1.0", &archive("rustronin", "one")).unwrap();
        assert_eq!(tools.current("rustronin").as_deref(), Some("0.1.0"));
        assert_eq!(fs::read_to_string(tools.executable("rustronin").unwrap()).unwrap(), "one");

        tools.install("rustronin", "0.2.0", &archive("rustronin", "two")).unwrap();
        assert_eq!(tools.current("rustronin").as_deref(), Some("0.2.0"));
        assert!(!dir.path().join("rustronin/0.1.0").exists(), "only the active version is kept");

        tools.uninstall("rustronin").unwrap();
        assert_eq!(tools.current("rustronin"), None);
    }

    #[test]
    fn pruning_removes_what_older_toolboxes_kept_for_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Installation::new(dir.path());
        tools.install("gogarden", "0.2.0", &archive("gogarden", "two")).unwrap();
        fs::create_dir_all(dir.path().join("gogarden/0.1.0/bin")).unwrap();
        fs::write(dir.path().join("gogarden/previous"), "0.1.0\n").unwrap();
        tools.prune("gogarden");
        assert!(!dir.path().join("gogarden/0.1.0").exists());
        assert!(!dir.path().join("gogarden/previous").exists());
        assert_eq!(tools.current("gogarden").as_deref(), Some("0.2.0"));
    }

    #[test]
    fn an_archive_without_the_executable_is_refused_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Installation::new(dir.path());
        tools.install("rustronin", "0.1.0", &archive("rustronin", "one")).unwrap();
        let error = tools.install("rustronin", "0.2.0", &archive("other", "x")).unwrap_err();
        assert!(error.to_string().contains("bin/rustronin"));
        assert_eq!(tools.current("rustronin").as_deref(), Some("0.1.0"));
        assert!(tools.install("rustronin", "0.2.0", b"not an archive").is_err());
        assert_eq!(tools.current("rustronin").as_deref(), Some("0.1.0"));
    }

    fn bundle_archive(name: &str, content: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in [
            (format!("{name}.app/Contents/MacOS/tool"), content),
            (format!("{name}.app/Contents/Info.plist"), "plist"),
            ("._junk".to_owned(), "x"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, data.as_bytes()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn bundles_are_exported_to_the_applications_folder() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("Applications");
        let tools = Installation::new(dir.path().join("tools")).with_applications(&applications);
        let app = applications.join("RustRonin.app");
        let content = || fs::read_to_string(app.join("Contents/MacOS/tool")).unwrap();

        tools.install("rustronin", "0.1.0", &bundle_archive("RustRonin", "one")).unwrap();
        assert_eq!(content(), "one");
        assert_eq!(tools.launch("rustronin"), Some(Launch::Bundle(app.clone())));
        assert!(!dir.path().join("tools/rustronin/0.1.0/._junk").exists(), "metadata files are skipped");

        tools.install("rustronin", "0.2.0", &bundle_archive("RustRonin", "two")).unwrap();
        assert_eq!(content(), "two");
        assert_eq!(fs::read_dir(&applications).unwrap().count(), 1, "no leftovers next to the app");

        tools.uninstall("rustronin").unwrap();
        assert!(!app.exists());
    }

    #[test]
    fn launch_command_lines_pass_arguments_to_the_tool() {
        let project = [OsString::from("/work/my project")];
        let bundle = Launch::Bundle(PathBuf::from("/Applications/GoGarden.app"));
        let open = |args: &[&str]| LaunchCommand {
            program: PathBuf::from("open"),
            args: args.iter().map(OsString::from).collect(),
        };
        assert_eq!(bundle.command_line(&[]), open(&["/Applications/GoGarden.app"]));
        assert_eq!(
            bundle.command_line(&project),
            open(&["-n", "-a", "/Applications/GoGarden.app", "--args", "/work/my project"])
        );

        let binary = Launch::Binary(PathBuf::from("/tools/gogarden/0.9.0/bin/gogarden"));
        assert_eq!(
            binary.command_line(&project),
            LaunchCommand { program: PathBuf::from("/tools/gogarden/0.9.0/bin/gogarden"), args: project.to_vec() }
        );
        assert!(binary.command_line(&[]).args.is_empty());

        let command = bundle.command_line(&project).to_command();
        assert_eq!(command.get_program(), "open");
        assert_eq!(command.get_args().count(), 5);
    }

    #[test]
    fn installed_tools_are_launched_with_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Installation::new(dir.path());
        let project = [OsString::from("/work")];
        assert_eq!(tools.launch_with("gogarden", &project), None);
        tools.install("gogarden", "0.1.0", &archive("gogarden", "x")).unwrap();
        let line = tools.launch_with("gogarden", &project).unwrap();
        assert_eq!(line.program, tools.executable("gogarden").unwrap());
        assert_eq!(line.args, project);
    }

    #[test]
    fn bundles_are_found_from_their_executable_and_replaced_whole() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Zen Toolbox.app");
        fs::create_dir_all(old.join("Contents/MacOS")).unwrap();
        fs::write(old.join("Contents/MacOS/zen-toolbox"), "old").unwrap();
        fs::write(old.join("Contents/Info.plist"), "0.1.3").unwrap();
        assert_eq!(bundle_of(&old.join("Contents/MacOS/zen-toolbox")), Some(old.clone()));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/zen-toolbox")), None);

        let release = dir.path().join("release");
        let new = release.join("Zen Toolbox.app");
        fs::create_dir_all(new.join("Contents/MacOS")).unwrap();
        fs::write(new.join("Contents/MacOS/zen-toolbox"), "new").unwrap();
        fs::write(new.join("Contents/Info.plist"), "0.1.4").unwrap();
        assert_eq!(bundle_in_release(&release), Some(new.clone()));

        replace_bundle(&old, &new).unwrap();
        assert_eq!(fs::read_to_string(old.join("Contents/Info.plist")).unwrap(), "0.1.4", "the whole bundle is new");
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(left.len(), 2, "only the new bundle and the release folder remain: {left:?}");
    }

    #[test]
    fn replacing_an_executable_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("zen-toolbox");
        fs::write(&exe, "old").unwrap();
        fs::write(dir.path().join("zen-toolbox.previous"), "older").unwrap();
        replace_executable(&exe, b"new").unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        remove_leftovers(&exe);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "only the executable remains");
    }
}
