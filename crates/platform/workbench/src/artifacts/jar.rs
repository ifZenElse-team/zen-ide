use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use zen_i18n::t;

use crate::ide_settings::zip::{Archive, Limits, ZipWriter};

const LIBRARY_LIMITS: Limits =
    Limits { archive: 512 * 1024 * 1024, entries: 65_535, entry: 256 * 1024 * 1024, total: 2 * 1024 * 1024 * 1024 };

pub const MANIFEST: &str = "META-INF/MANIFEST.MF";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contents {
    pub folders: Vec<PathBuf>,
    pub libraries: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub jar: PathBuf,
    pub copied: Vec<PathBuf>,
    pub entries: usize,
}

pub fn manifest(main_class: &str, class_path: &[String]) -> String {
    let mut text = String::new();
    let mut attribute = |name: &str, value: &str| {
        let line = format!("{name}: {value}");
        let mut rest = line.as_str();
        let mut first = true;
        while !rest.is_empty() {
            let room = if first { 72 } else { 71 };
            let mut cut = rest.len().min(room);
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            if !first {
                text.push(' ');
            }
            text.push_str(&rest[..cut]);
            text.push_str("\r\n");
            rest = &rest[cut..];
            first = false;
        }
    };
    attribute("Manifest-Version", "1.0");
    attribute("Created-By", "Zen IDE");
    if !main_class.trim().is_empty() {
        attribute("Main-Class", main_class.trim());
    }
    if !class_path.is_empty() {
        let joined: Vec<String> = class_path.iter().map(|name| name.replace('%', "%25").replace(' ', "%20")).collect();
        attribute("Class-Path", &joined.join(" "));
    }
    text.push_str("\r\n");
    text
}

pub fn class_path_of(manifest: &str) -> Vec<String> {
    let mut joined = String::new();
    let mut inside = false;
    for line in manifest.lines().map(|line| line.trim_end_matches('\r')) {
        if let Some(rest) = line.strip_prefix(' ') {
            if inside {
                joined.push_str(rest);
            }
            continue;
        }
        inside = false;
        if let Some(value) = line.strip_prefix("Class-Path:") {
            inside = true;
            joined.push_str(value.trim_start());
        }
    }
    joined.split_whitespace().map(|name| name.replace("%20", " ").replace("%25", "%")).collect()
}

pub fn read_manifest(jar: &Path) -> Option<String> {
    let archive = Archive::parse(std::fs::read(jar).ok()?, LIBRARY_LIMITS).ok()?;
    let entry = archive.entries().iter().find(|entry| entry.name == MANIFEST)?;
    archive.read(entry).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn skipped(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == MANIFEST
        || upper == "META-INF/INDEX.LIST"
        || name == "module-info.class"
        || upper.starts_with("META-INF/")
            && !upper[9..].contains('/')
            && [".SF", ".RSA", ".DSA", ".EC"].iter().any(|suffix| upper.ends_with(suffix))
}

fn files_of(folder: &Path, prefix: &str, found: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if kind.is_dir() {
            files_of(&entry.path(), &format!("{name}/"), found)?;
        } else {
            found.push((name, entry.path()));
        }
    }
    Ok(())
}

fn library_names(libraries: &[PathBuf]) -> Vec<String> {
    let mut taken = BTreeSet::new();
    libraries
        .iter()
        .map(|library| {
            let name = library.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let mut chosen = name.clone();
            let mut count = 1;
            while !taken.insert(chosen.clone()) {
                count += 1;
                chosen = format!("{count}-{name}");
            }
            chosen
        })
        .collect()
}

struct Packer {
    writer: ZipWriter,
    names: BTreeSet<String>,
    services: BTreeMap<String, Vec<u8>>,
    entries: usize,
}

impl Packer {
    fn add(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        if skipped(name) {
            return Ok(());
        }
        if name.starts_with("META-INF/services/") {
            let merged = self.services.entry(name.to_owned()).or_default();
            if !merged.is_empty() && !merged.ends_with(b"\n") {
                merged.push(b'\n');
            }
            merged.extend_from_slice(data);
            return Ok(());
        }
        if !self.names.insert(name.to_owned()) {
            return Ok(());
        }
        self.entries += 1;
        self.writer.add(name, data).map_err(|error| t!("artifacts-error-entry", name = name, error = error.to_string()))
    }
}

pub fn write(
    jar: &Path,
    main_class: &str,
    contents: &Contents,
    extract: bool,
    seconds: u64,
) -> Result<Written, String> {
    let folder = jar.parent().map(Path::to_owned).unwrap_or_default();
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let libraries: Vec<PathBuf> = contents.libraries.iter().filter(|library| library.is_file()).cloned().collect();
    let names = if extract { Vec::new() } else { library_names(&libraries) };
    let mut packer =
        Packer { writer: ZipWriter::new(seconds), names: BTreeSet::new(), services: BTreeMap::new(), entries: 1 };
    packer
        .writer
        .add(MANIFEST, manifest(main_class, &names).as_bytes())
        .map_err(|error| t!("artifacts-error-entry", name = MANIFEST, error = error.to_string()))?;
    for output in contents.folders.iter().filter(|folder| folder.is_dir()) {
        let mut files = Vec::new();
        files_of(output, "", &mut files)?;
        for (name, path) in files {
            let data = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            packer.add(&name, &data)?;
        }
    }
    if extract {
        for library in &libraries {
            let failed = |error: &dyn std::fmt::Display| {
                t!("artifacts-error-library", path = library.display().to_string(), error = error.to_string())
            };
            let bytes = std::fs::read(library).map_err(|error| failed(&error))?;
            let archive = Archive::parse(bytes, LIBRARY_LIMITS).map_err(|error| failed(&error))?;
            for entry in archive.entries().iter().filter(|entry| !entry.folder && !entry.link) {
                let data = archive.read(entry).map_err(|error| failed(&error))?;
                packer.add(&entry.name, &data)?;
            }
        }
    }
    let services = std::mem::take(&mut packer.services);
    for (name, data) in services {
        packer.entries += 1;
        packer
            .writer
            .add(&name, &data)
            .map_err(|error| t!("artifacts-error-entry", name = name.as_str(), error = error.to_string()))?;
    }
    let entries = packer.entries;
    let bytes = packer.writer.finish();
    zen_vfs::write_atomic(jar, &bytes).map_err(|error| error.to_string())?;
    let mut copied = Vec::new();
    for (library, name) in libraries.iter().zip(&names) {
        let target = folder.join(name);
        if target != *library {
            std::fs::copy(library, &target).map_err(|error| format!("{}: {error}", target.display()))?;
        }
        copied.push(target);
    }
    Ok(Written { jar: jar.to_owned(), copied, entries })
}

pub fn remove(jar: &Path) -> Vec<PathBuf> {
    let folder = jar.parent().map(Path::to_owned).unwrap_or_default();
    let mut removed = Vec::new();
    let next_to = read_manifest(jar).map(|text| class_path_of(&text)).unwrap_or_default();
    for name in next_to {
        let path = folder.join(&name);
        let plain = Path::new(&name).components().count() == 1 && !name.contains(['/', '\\']);
        if plain && path.is_file() && std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    if jar.is_file() && std::fs::remove_file(jar).is_ok() {
        removed.push(jar.to_owned());
    }
    removed
}

pub fn classpath_file(text: &str) -> Vec<PathBuf> {
    let text = text.trim();
    let Some(text) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) else {
        return std::env::split_paths(text).filter(|path| !path.as_os_str().is_empty()).collect();
    };
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                unescaped.push(next);
            }
        } else {
            unescaped.push(c);
        }
    }
    std::env::split_paths(&unescaped).filter(|path| !path.as_os_str().is_empty()).collect()
}

pub fn split(paths: Vec<PathBuf>) -> Contents {
    let (folders, libraries) = paths.into_iter().partition(|path| path.is_dir());
    Contents { folders, libraries }
}
