use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Licenses";

pub const NOTICES: &str = "THIRD-PARTY-NOTICES";

const FIRST: [&str; 5] = [NOTICES, "NOTICE", "LICENSE-MIT", "LICENSE-APACHE", "RUST-STANDARD-LIBRARY-COPYRIGHT.html"];

pub fn folder_for(executable: &Path) -> Option<PathBuf> {
    let bin = executable.parent()?;
    let above = bin.parent()?;
    [above.join("Resources").join(FOLDER), above.join(FOLDER)].into_iter().find(|folder| folder.join(NOTICES).is_file())
}

pub fn bundled() -> Option<PathBuf> {
    folder_for(&std::env::current_exe().ok()?)
}

pub fn files(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect(folder, folder, &mut found);
    found.sort_by_key(|name| (FIRST.iter().position(|first| first == name).unwrap_or(FIRST.len()), name.clone()));
    found
}

fn collect(root: &Path, folder: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, found);
        } else if let Ok(relative) = path.strip_prefix(root) {
            found.push(
                relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
            );
        }
    }
}

pub fn read(folder: &Path, file: &str) -> Option<String> {
    let inside = files(folder).into_iter().find(|name| name == file)?;
    let bytes = std::fs::read(folder.join(inside)).ok()?;
    Some(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests {
    use super::{files, folder_for, read};

    #[test]
    fn the_licences_are_found_in_an_application_bundle_and_next_to_bin() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("GoGarden.app/Contents");
        std::fs::create_dir_all(bundle.join("MacOS")).unwrap();
        assert_eq!(folder_for(&bundle.join("MacOS/gogarden")), None, "no notices, no folder");
        std::fs::create_dir_all(bundle.join("Resources/Licenses/fonts")).unwrap();
        for file in ["LICENSE-MIT", "THIRD-PARTY-NOTICES", "NOTICE", "fonts/OFL-Inter.txt"] {
            std::fs::write(bundle.join("Resources/Licenses").join(file), format!("{file}\r\ntext")).unwrap();
        }
        let folder = folder_for(&bundle.join("MacOS/gogarden")).unwrap();
        assert_eq!(folder, bundle.join("Resources/Licenses"));
        assert_eq!(files(&folder), ["THIRD-PARTY-NOTICES", "NOTICE", "LICENSE-MIT", "fonts/OFL-Inter.txt"]);
        assert_eq!(read(&folder, "fonts/OFL-Inter.txt").unwrap(), "fonts/OFL-Inter.txt\ntext");
        assert_eq!(read(&folder, "../secret"), None, "only the listed files");

        let linux = dir.path().join("rustronin");
        std::fs::create_dir_all(linux.join("bin")).unwrap();
        std::fs::create_dir_all(linux.join("Licenses")).unwrap();
        std::fs::write(linux.join("Licenses/THIRD-PARTY-NOTICES"), "x").unwrap();
        assert_eq!(folder_for(&linux.join("bin/rustronin")).unwrap(), linux.join("Licenses"));
    }
}
