pub mod excluded_folders;
pub mod file_types;
#[cfg(not(target_arch = "wasm32"))]
mod watch;
pub mod zip;
#[cfg(not(target_arch = "wasm32"))]
pub use watch::{WatchRetry, Watcher};

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

pub use file_types::is_ignored;

pub const HIDDEN: &[&str] = &[".git", ".hg", ".svn", ".DS_Store"];

pub const TOOL_FOLDERS: &[&str] = &[".idea", ".gradle"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

#[derive(Debug)]
pub struct VfsError {
    pub action: &'static str,
    pub path: PathBuf,
    pub source: io::Error,
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot {} {}: {}", self.action, self.path.display(), self.source)
    }
}

impl std::error::Error for VfsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl VfsError {
    pub fn is_changed_on_disk(&self) -> bool {
        self.source.get_ref().is_some_and(|source| source.is::<ChangedOnDisk>())
    }
}

#[derive(Debug)]
struct ChangedOnDisk;

impl fmt::Display for ChangedOnDisk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the file changed on disk")
    }
}

impl std::error::Error for ChangedOnDisk {}

fn error<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> VfsError + 'a {
    move |source| VfsError { action, path: path.to_owned(), source }
}

pub fn list(folder: &Path) -> Result<Vec<Entry>, VfsError> {
    let mut entries: Vec<Entry> = fs::read_dir(folder)
        .map_err(error("list", folder))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_ignored(&name) {
                return None;
            }
            let is_symlink = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            let is_dir =
                if is_symlink { entry.path().is_dir() } else { entry.file_type().is_ok_and(|kind| kind.is_dir()) };
            Some(Entry { path: entry.path(), name, is_dir, is_symlink })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

pub fn read_text(path: &Path) -> Result<String, VfsError> {
    let bytes = fs::read(path).map_err(error("read", path))?;
    String::from_utf8(bytes).map_err(|_| VfsError {
        action: "read",
        path: path.to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidData, "the file is not UTF-8 text"),
    })
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), VfsError> {
    write_replacing(path, contents, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    file: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    #[cfg(unix)]
    let file = std::os::unix::fs::MetadataExt::ino(&metadata);
    #[cfg(not(unix))]
    let file = 0;
    Some(Stamp { modified: metadata.modified().ok(), len: metadata.len(), file })
}

pub fn write_unless_changed(path: &Path, contents: &[u8], expected: Option<Stamp>) -> Result<(), VfsError> {
    write_replacing(
        path,
        contents,
        |target| {
            if stamp(target) == expected { Ok(()) } else { Err(changed_on_disk(target)) }
        },
    )
}

pub fn replace_unchanged(path: &Path, before: Option<&[u8]>, after: &[u8]) -> Result<(), VfsError> {
    let expected = stamp(path);
    let found = match fs::read(path) {
        Ok(found) => Some(found),
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(VfsError { action: "read", path: path.to_owned(), source }),
    };
    if found.as_deref() != before {
        return Err(changed_on_disk(path));
    }
    write_unless_changed(path, after, expected)
}

fn changed_on_disk(path: &Path) -> VfsError {
    VfsError { action: "save", path: path.to_owned(), source: io::Error::other(ChangedOnDisk) }
}

fn write_replacing(
    path: &Path,
    contents: &[u8],
    still_expected: impl FnOnce(&Path) -> Result<(), VfsError>,
) -> Result<(), VfsError> {
    use std::io::Write;
    let linked = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink());
    let target = if linked { fs::canonicalize(path).map_err(error("write", path))? } else { path.to_owned() };
    let path = target.as_path();
    if fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().readonly()) {
        return Err(VfsError { action: "save", path: path.to_owned(), source: io::ErrorKind::PermissionDenied.into() });
    }
    let (mut file, temporary) = create_temporary(path)?;
    let written = file.write_all(contents).and_then(|()| file.sync_all()).map_err(error("write", &temporary));
    drop(file);
    let replaced = written.and_then(|()| {
        if let Ok(metadata) = fs::metadata(path) {
            let _ = fs::set_permissions(&temporary, metadata.permissions());
        }
        still_expected(path)?;
        fs::rename(&temporary, path).map_err(error("save", path))
    });
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    replaced
}

fn create_temporary(path: &Path) -> Result<(fs::File, PathBuf), VfsError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = mode_of(path) {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    }
    #[cfg(target_arch = "wasm32")]
    let process = 0;
    #[cfg(not(target_arch = "wasm32"))]
    let process = std::process::id();
    loop {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_file_name(format!(".{process}-{number}.zen-save"));
        match options.open(&temporary) {
            Ok(file) => return Ok((file, temporary)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(VfsError { action: "write", path: temporary, source }),
        }
    }
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).ok().map(|metadata| metadata.permissions().mode() & 0o777)
}

pub fn create_file(path: &Path) -> Result<(), VfsError> {
    fs::OpenOptions::new().write(true).create_new(true).open(path).map(|_| ()).map_err(error("create", path))
}

pub fn create_dir(path: &Path) -> Result<(), VfsError> {
    if path.exists() {
        return Err(VfsError { action: "create", path: path.to_owned(), source: io::ErrorKind::AlreadyExists.into() });
    }
    fs::create_dir_all(path).map_err(error("create", path))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn move_to_trash(path: &Path) -> Result<(), VfsError> {
    if fs::symlink_metadata(path).is_err() {
        return Err(VfsError { action: "delete", path: path.to_owned(), source: io::ErrorKind::NotFound.into() });
    }
    #[allow(unused_mut, reason = "configured on macOS only")]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete(path).map_err(|error| VfsError {
        action: "delete",
        path: path.to_owned(),
        source: io::Error::other(error.to_string()),
    })
}

pub fn copy_into(source: &Path, folder: &Path) -> Result<PathBuf, VfsError> {
    let name = source.file_name().ok_or_else(|| VfsError {
        action: "copy",
        path: source.to_owned(),
        source: io::ErrorKind::InvalidInput.into(),
    })?;
    if source.is_dir() && folder.starts_with(source) {
        return Err(VfsError { action: "copy", path: source.to_owned(), source: io::ErrorKind::InvalidInput.into() });
    }
    let target = free_name(folder, &name.to_string_lossy());
    copy_recursive(source, &target)?;
    Ok(target)
}

fn free_name(folder: &Path, name: &str) -> PathBuf {
    let candidate = folder.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    let copy = folder.join(format!("{stem} copy{extension}"));
    if !copy.exists() {
        return copy;
    }
    (2..).map(|n| folder.join(format!("{stem} copy {n}{extension}"))).find(|path| !path.exists()).unwrap_or(candidate)
}

pub fn move_into(source: &Path, folder: &Path) -> Result<PathBuf, VfsError> {
    let name = source.file_name().ok_or_else(|| VfsError {
        action: "move",
        path: source.to_owned(),
        source: io::ErrorKind::InvalidInput.into(),
    })?;
    if fs::symlink_metadata(source).is_err() {
        return Err(VfsError { action: "move", path: source.to_owned(), source: io::ErrorKind::NotFound.into() });
    }
    if source.parent() == Some(folder) {
        return Ok(source.to_owned());
    }
    if source.is_dir() && folder.starts_with(source) {
        return Err(VfsError { action: "move", path: source.to_owned(), source: io::ErrorKind::InvalidInput.into() });
    }
    let target = free_name(folder, &name.to_string_lossy());
    if fs::rename(source, &target).is_err() {
        copy_recursive(source, &target)?;
        let removed = if source.is_dir() { fs::remove_dir_all(source) } else { fs::remove_file(source) };
        removed.map_err(error("move", source))?;
    }
    Ok(target)
}

pub fn rename(path: &Path, name: &str) -> Result<PathBuf, VfsError> {
    let invalid =
        |path: &Path| VfsError { action: "rename", path: path.to_owned(), source: io::ErrorKind::InvalidInput.into() };
    let plain = !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(Path::new(name).components().next(), Some(std::path::Component::Normal(_)));
    if !plain {
        return Err(invalid(path));
    }
    let parent = path.parent().ok_or_else(|| invalid(path))?;
    let target = parent.join(name);
    if target == path {
        return Ok(target);
    }
    let same_entry = path.file_name().is_some_and(|old| old.to_string_lossy().to_lowercase() == name.to_lowercase())
        && !is_listed(parent, name);
    if fs::symlink_metadata(&target).is_ok() && !same_entry {
        return Err(VfsError { action: "rename", path: target, source: io::ErrorKind::AlreadyExists.into() });
    }
    fs::rename(path, &target).map_err(error("rename", path))?;
    Ok(target)
}

fn is_listed(folder: &Path, name: &str) -> bool {
    fs::read_dir(folder).is_ok_and(|entries| entries.flatten().any(|entry| entry.file_name() == name))
}

pub fn is_writable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| !metadata.permissions().readonly())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteAccess(u32);

pub fn make_read_only(path: &Path) -> Result<WriteAccess, VfsError> {
    let mut permissions = fs::metadata(path).map_err(error("make read-only", path))?.permissions();
    #[cfg(unix)]
    let access = {
        use std::os::unix::fs::PermissionsExt;
        let mode = permissions.mode();
        permissions.set_mode(mode & !0o222);
        WriteAccess(mode & 0o222)
    };
    #[cfg(not(unix))]
    let access = {
        permissions.set_readonly(true);
        WriteAccess(0o200)
    };
    fs::set_permissions(path, permissions).map_err(error("make read-only", path))?;
    Ok(access)
}

pub fn make_writable(path: &Path, access: Option<WriteAccess>) -> Result<(), VfsError> {
    let mut permissions = fs::metadata(path).map_err(error("make writable", path))?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bits = access.map_or(0o200, |access| access.0) & 0o222;
        permissions.set_mode(permissions.mode() | if bits == 0 { 0o200 } else { bits });
    }
    #[cfg(not(unix))]
    {
        let _ = access;
        permissions.set_readonly(false);
    }
    fs::set_permissions(path, permissions).map_err(error("make writable", path))
}

pub fn set_writable(path: &Path, writable: bool) -> Result<(), VfsError> {
    if writable { make_writable(path, None) } else { make_read_only(path).map(|_| ()) }
}

pub fn spelled_from(root: &Path, path: &Path) -> PathBuf {
    if path.starts_with(root) || !path.is_absolute() {
        return path.to_owned();
    }
    let Ok(real_root) = fs::canonicalize(root) else { return path.to_owned() };
    let mut deepest: Option<(&Path, PathBuf)> = None;
    for folder in path.ancestors() {
        let Ok(real) = fs::canonicalize(folder) else { continue };
        if real == real_root {
            return root.join(path.strip_prefix(folder).unwrap_or(Path::new("")));
        }
        if deepest.is_none() {
            deepest = Some((folder, real));
        }
    }
    match deepest {
        Some((folder, real)) if real.starts_with(&real_root) => {
            let inside = real.strip_prefix(&real_root).unwrap_or(Path::new(""));
            root.join(inside).join(path.strip_prefix(folder).unwrap_or(Path::new("")))
        }
        _ => path.to_owned(),
    }
}

fn copy_recursive(source: &Path, target: &Path) -> Result<(), VfsError> {
    if source.is_dir() {
        fs::create_dir(target).map_err(error("create", target))?;
        for entry in fs::read_dir(source).map_err(error("read", source))? {
            let entry = entry.map_err(error("read", source))?;
            let inside = target.join(entry.file_name());
            if entry.file_type().map_err(error("read", source))?.is_symlink() {
                copy_link(&entry.path(), &inside)?;
            } else {
                copy_recursive(&entry.path(), &inside)?;
            }
        }
        Ok(())
    } else {
        fs::copy(source, target).map(|_| ()).map_err(error("copy", source))
    }
}

fn copy_link(source: &Path, target: &Path) -> Result<(), VfsError> {
    let link = fs::read_link(source).map_err(error("read", source))?;
    make_link(&link, source, target).map_err(error("copy", source))
}

#[cfg(unix)]
fn make_link(link: &Path, _: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(link, target)
}

#[cfg(windows)]
fn make_link(link: &Path, source: &Path, target: &Path) -> io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(link, target)
    } else {
        std::os::windows::fs::symlink_file(link, target)
    }
}

#[cfg(not(any(unix, windows)))]
fn make_link(_: &Path, _: &Path, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn paths_through_a_link_are_spelled_from_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let root = base.join("real/project");
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg/a.go"), "package pkg").unwrap();
        std::os::unix::fs::symlink("a.go", root.join("pkg/b.go")).unwrap();
        std::os::unix::fs::symlink("real", base.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("pkg"), base.join("pkg-link")).unwrap();
        let linked = base.join("link/project");

        assert_eq!(spelled_from(&root, &linked.join("pkg/a.go")), root.join("pkg/a.go"));
        assert_eq!(spelled_from(&root, &linked), root);
        assert_eq!(spelled_from(&root, &linked.join("pkg/new/c.go")), root.join("pkg/new/c.go"));
        assert_eq!(spelled_from(&root, &linked.join("pkg/b.go")), root.join("pkg/b.go"));
        assert_eq!(spelled_from(&root, &base.join("pkg-link/a.go")), root.join("pkg/a.go"));
        assert_eq!(spelled_from(&root, &root.join("pkg/b.go")), root.join("pkg/b.go"));
        assert_eq!(spelled_from(&root, &base.join("real")), base.join("real"));
        assert_eq!(spelled_from(&root, Path::new("pkg/a.go")), Path::new("pkg/a.go"));
        assert_eq!(spelled_from(&linked, &root.join("pkg/a.go")), linked.join("pkg/a.go"));
    }

    #[cfg(unix)]
    #[test]
    fn marks_symbolic_links_and_follows_them_for_the_kind() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "x").unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        std::os::unix::fs::symlink("AGENTS.md", dir.path().join("CLAUDE.md")).unwrap();
        std::os::unix::fs::symlink("real", dir.path().join("linked")).unwrap();
        let entries = list(dir.path()).unwrap();
        let get = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
        assert!(get("CLAUDE.md").is_symlink && !get("CLAUDE.md").is_dir);
        assert!(get("linked").is_symlink && get("linked").is_dir);
        assert!(!get("AGENTS.md").is_symlink && !get("real").is_symlink);
    }

    #[test]
    fn creates_and_copies_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        create_file(&file).unwrap();
        assert!(create_file(&file).is_err(), "never overwrites");
        let folder = dir.path().join("pkg/sub");
        create_dir(&folder).unwrap();
        assert!(create_dir(&folder).is_err());
        fs::write(folder.join("a.txt"), "a").unwrap();

        let target = dir.path().join("target");
        create_dir(&target).unwrap();
        assert_eq!(copy_into(&file, &target).unwrap(), target.join("main.go"));
        assert_eq!(copy_into(&file, &target).unwrap(), target.join("main copy.go"));
        assert_eq!(copy_into(&file, &target).unwrap(), target.join("main copy 2.go"));
        let copied = copy_into(&dir.path().join("pkg"), &target).unwrap();
        assert_eq!(fs::read_to_string(copied.join("sub/a.txt")).unwrap(), "a");
        assert!(copy_into(&dir.path().join("pkg"), &folder).is_err(), "not into itself");
    }

    #[test]
    fn moves_keep_the_name_or_take_a_copy_name_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        create_dir(&a).unwrap();
        create_dir(&b).unwrap();
        fs::write(a.join("x.go"), "new").unwrap();
        fs::write(b.join("x.go"), "old").unwrap();
        fs::create_dir_all(a.join("pkg/sub")).unwrap();

        let moved = move_into(&a.join("x.go"), &b).unwrap();
        assert_eq!(moved, b.join("x copy.go"));
        assert_eq!(fs::read_to_string(&moved).unwrap(), "new");
        assert_eq!(fs::read_to_string(b.join("x.go")).unwrap(), "old", "never overwritten");
        assert!(!a.join("x.go").exists());
        assert_eq!(move_into(&moved, &b).unwrap(), moved, "already there");
        assert!(move_into(&a.join("pkg"), &a.join("pkg/sub")).is_err(), "not into itself");
        assert_eq!(move_into(&a.join("pkg"), &b).unwrap(), b.join("pkg"));
        assert!(b.join("pkg/sub").is_dir() && !a.join("pkg").exists());
        assert!(move_into(&a.join("missing"), &b).is_err());
    }

    #[test]
    fn rename_takes_a_plain_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.go");
        fs::write(&file, "a").unwrap();
        fs::write(dir.path().join("b.go"), "b").unwrap();
        assert!(rename(&file, "b.go").is_err(), "taken");
        assert!(rename(&file, "../c.go").is_err());
        assert!(rename(&file, "x/c.go").is_err());
        assert!(rename(&file, "").is_err());
        let renamed = rename(&file, "c.go").unwrap();
        assert_eq!(renamed, dir.path().join("c.go"));
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "a");
        assert_eq!(rename(&renamed, "C.go").unwrap(), dir.path().join("C.go"), "a change of case");
    }

    #[test]
    fn a_taken_name_is_the_listed_entry_of_exactly_that_spelling() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Readme.md"), "mine").unwrap();
        assert!(!is_listed(dir.path(), "readme.md"), "another spelling is not this entry");
        assert!(is_listed(dir.path(), "Readme.md"));
        let other = dir.path().join("readme.md");
        if fs::write(&other, "other").is_ok() && is_listed(dir.path(), "readme.md") {
            assert!(rename(&dir.path().join("Readme.md"), "readme.md").is_err(), "a case-sensitive volume");
            assert_eq!(fs::read_to_string(&other).unwrap(), "other");
            assert_eq!(fs::read_to_string(dir.path().join("Readme.md")).unwrap(), "mine");
        } else {
            fs::write(dir.path().join("Readme.md"), "mine").unwrap();
            let renamed = rename(&dir.path().join("Readme.md"), "readme.md").unwrap();
            assert!(is_listed(dir.path(), "readme.md") && !is_listed(dir.path(), "Readme.md"));
            assert_eq!(fs::read_to_string(renamed).unwrap(), "mine");
        }
    }

    #[cfg(unix)]
    #[test]
    fn copying_a_folder_keeps_its_links_and_survives_a_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("pkg");
        fs::create_dir_all(folder.join("sub")).unwrap();
        fs::write(folder.join("a.txt"), "a").unwrap();
        std::os::unix::fs::symlink("..", folder.join("sub/up")).unwrap();
        std::os::unix::fs::symlink("a.txt", folder.join("b.txt")).unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();

        let copied = copy_into(&folder, &target).unwrap();
        assert_eq!(fs::read_link(copied.join("sub/up")).unwrap(), Path::new(".."));
        assert_eq!(fs::read_link(copied.join("b.txt")).unwrap(), Path::new("a.txt"));
        assert_eq!(fs::read_to_string(copied.join("a.txt")).unwrap(), "a");
    }

    #[test]
    fn folders_first_sorted_and_hidden_names_left_out() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.rs", "A.rs", ".git", ".DS_Store", "src"] {
            if name.contains('.') && name != ".git" && name != ".DS_Store" {
                fs::write(dir.path().join(name), "").unwrap();
            } else {
                fs::create_dir(dir.path().join(name)).unwrap();
            }
        }
        let names: Vec<String> = list(dir.path()).unwrap().into_iter().map(|entry| entry.name).collect();
        assert_eq!(names, ["src", "A.rs", "b.rs"]);
    }

    #[test]
    fn the_write_permission_turns_off_and_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        fs::write(&file, "package main\n").unwrap();
        assert!(is_writable(&file));
        set_writable(&file, false).unwrap();
        assert!(!is_writable(&file));
        assert!(fs::OpenOptions::new().write(true).open(&file).is_err() || cfg!(not(unix)));
        set_writable(&file, true).unwrap();
        assert!(is_writable(&file));
        fs::write(&file, "package other\n").unwrap();
        assert!(!is_writable(&dir.path().join("missing.go")));
        assert!(set_writable(&dir.path().join("missing.go"), true).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            set_writable(&file, false).unwrap();
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o444);
            set_writable(&file, true).unwrap();
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o644);
        }
    }

    #[cfg(unix)]
    #[test]
    fn making_a_file_writable_again_gives_back_the_write_bits_it_had() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("shared.go");
        fs::write(&file, "package shared\n").unwrap();
        let mode = |file: &Path| fs::metadata(file).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o664)).unwrap();
        let access = make_read_only(&file).unwrap();
        assert_eq!(mode(&file), 0o444);
        make_writable(&file, Some(access)).unwrap();
        assert_eq!(mode(&file), 0o664, "the group keeps its write permission");
        make_read_only(&file).unwrap();
        make_writable(&file, None).unwrap();
        assert_eq!(mode(&file), 0o644, "without a remembered state only the owner gets it");

        let link = dir.path().join("link.go");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let access = make_read_only(&link).unwrap();
        assert!(!is_writable(&file), "the link's target is changed");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");
        make_writable(&link, Some(access)).unwrap();
        assert_eq!(mode(&file), 0o644);
    }

    #[test]
    fn an_atomic_write_never_replaces_a_read_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        fs::write(&file, "package main\n").unwrap();
        set_writable(&file, false).unwrap();
        let error = write_atomic(&file, b"package other\n").unwrap_err();
        assert_eq!(error.source.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&file).unwrap(), "package main\n");
        assert!(!dir.path().join(".main.go.zen-save").exists());
        set_writable(&file, true).unwrap();
        write_atomic(&file, b"package other\n").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "package other\n");
    }

    #[test]
    fn processes_saving_one_file_never_share_a_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.txt");
        fs::write(path.with_file_name(".shared.txt.zen-save"), "another process is writing").unwrap();
        write_atomic(&path, b"mine").unwrap();
        assert_eq!(read_text(&path).unwrap(), "mine");
        assert_eq!(
            read_text(&path.with_file_name(".shared.txt.zen-save")).unwrap(),
            "another process is writing",
            "the fixed name of an earlier version is left alone"
        );

        let writers: Vec<_> = (0..8)
            .map(|writer| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let text = writer.to_string().repeat(10_000);
                    for _ in 0..20 {
                        write_atomic(&path, text.as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        writers.into_iter().for_each(|writer| writer.join().unwrap());
        let text = read_text(&path).unwrap();
        assert_eq!(text.len(), 10_000);
        assert!(text.chars().all(|digit| text.starts_with(digit)), "one writer's text, whole");
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(left.len(), 2, "no temporary file is left behind: {left:?}");
    }

    #[test]
    fn a_name_as_long_as_the_file_system_allows_still_saves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n".repeat(250));
        fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(read_text(&path).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn a_private_file_is_never_readable_by_others_while_it_is_saved() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.env");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(mode_of(&path), Some(0o600));
        assert_eq!(mode_of(&dir.path().join("missing")), None);
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_stamp_tells_a_replaced_file_from_the_one_it_was_taken_of() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        assert_eq!(stamp(&path), None);
        fs::write(&path, "one").unwrap();
        let first = stamp(&path).unwrap();
        assert_eq!(stamp(&path), Some(first));
        write_atomic(&path, b"two").unwrap();
        assert_ne!(stamp(&path), Some(first), "the same size, another file");
    }

    #[test]
    fn a_guarded_write_refuses_a_file_that_changed_since_its_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "one").unwrap();
        let taken = stamp(&path);
        fs::write(&path, "theirs, longer").unwrap();
        let refused = write_unless_changed(&path, b"mine", taken).unwrap_err();
        assert!(refused.is_changed_on_disk());
        assert_eq!(read_text(&path).unwrap(), "theirs, longer");
        assert_eq!(list(dir.path()).unwrap().len(), 1, "no temporary file is left behind");
        write_unless_changed(&path, b"mine", stamp(&path)).unwrap();
        assert_eq!(read_text(&path).unwrap(), "mine");
    }

    #[test]
    fn replacing_compares_the_contents_on_disk_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pom.xml");
        fs::write(&path, "<theirs/>").unwrap();
        let refused = replace_unchanged(&path, Some(b"<mine/>"), b"<new/>").unwrap_err();
        assert!(refused.is_changed_on_disk());
        assert_eq!(read_text(&path).unwrap(), "<theirs/>");

        replace_unchanged(&path, Some(b"<theirs/>"), b"<new/>").unwrap();
        assert_eq!(read_text(&path).unwrap(), "<new/>");

        let new = dir.path().join("build.gradle");
        replace_unchanged(&new, None, b"plugins {}").unwrap();
        assert_eq!(read_text(&new).unwrap(), "plugins {}");
        let taken = replace_unchanged(&new, None, b"other").unwrap_err();
        assert!(taken.is_changed_on_disk(), "a file that appeared is not replaced");
        let gone = dir.path().join("gone.txt");
        assert!(replace_unchanged(&gone, Some(b"x"), b"y").unwrap_err().is_changed_on_disk(), "it was deleted");
        assert!(!gone.exists());
    }

    #[test]
    fn atomic_writes_replace_contents_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        write_atomic(&path, b"fn main() {}\n").unwrap();
        write_atomic(&path, "// привет\n".as_bytes()).unwrap();
        assert_eq!(read_text(&path).unwrap(), "// привет\n");
        assert_eq!(list(dir.path()).unwrap().len(), 1, "no temporary file is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_writes_through_a_link_keep_the_link() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dotfiles")).unwrap();
        let real = dir.path().join("dotfiles/config.toml");
        fs::write(&real, "old = 1\n").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink("dotfiles/config.toml", &link).unwrap();
        write_atomic(&link, b"new = 2\n").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_text(&real).unwrap(), "new = 2\n");
        assert_eq!(list(&dir.path().join("dotfiles")).unwrap().len(), 1, "no temporary file is left behind");
        fs::remove_file(&real).unwrap();
        assert!(write_atomic(&link, b"x").is_err(), "a dangling link is not replaced by a file");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(read_text(&path).is_err());
    }
}
