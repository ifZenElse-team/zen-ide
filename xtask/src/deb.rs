use std::io::Write;
use std::path::Path;

pub(crate) struct Package<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
    pub(crate) display: &'a str,
    pub(crate) binary: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) depends: &'a str,
    pub(crate) categories: &'a str,
    pub(crate) icon: &'a [u8],
}

pub(crate) fn build(package: &Package<'_>, files: &Path) -> Result<Vec<u8>, String> {
    let name = package.name;
    let mut data = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    let mut installed = 0u64;
    for folder in ["./", "./opt/", &format!("./opt/{name}/")] {
        directory(&mut data, folder)?;
    }
    add_tree(&mut data, files, &format!("./opt/{name}"), package.binary, &mut installed)?;
    for folder in ["./usr/", "./usr/bin/", "./usr/share/", "./usr/share/applications/", "./usr/share/icons/"] {
        directory(&mut data, folder)?;
    }
    for folder in
        ["./usr/share/icons/hicolor/", "./usr/share/icons/hicolor/256x256/", "./usr/share/icons/hicolor/256x256/apps/"]
    {
        directory(&mut data, folder)?;
    }
    let executable = format!("/opt/{name}/bin/{}", package.binary);
    let mut link = header(tar::EntryType::Symlink, 0o777, 0);
    link.set_link_name(&executable).map_err(|error| error.to_string())?;
    append(&mut data, link, &format!("./usr/bin/{name}"), &[][..])?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={executable} %F\nIcon={name}\nCategories={}\nTerminal=false\nStartupWMClass={name}\n",
        package.display, package.categories
    );
    file(&mut data, &format!("./usr/share/applications/{name}.desktop"), desktop.as_bytes(), 0o644)?;
    file(&mut data, &format!("./usr/share/icons/hicolor/256x256/apps/{name}.png"), package.icon, 0o644)?;
    let copyright = copyright(files)?;
    for folder in ["./usr/share/doc/".to_owned(), format!("./usr/share/doc/{name}/")] {
        directory(&mut data, &folder)?;
    }
    file(&mut data, &format!("./usr/share/doc/{name}/copyright"), copyright.as_bytes(), 0o644)?;
    installed += (desktop.len() + package.icon.len() + copyright.len()) as u64;
    let data = data.into_inner().and_then(|gz| gz.finish()).map_err(|error| error.to_string())?;

    let control = format!(
        "Package: {name}\nVersion: {}\nArchitecture: amd64\nMaintainer: ifzenelse\nInstalled-Size: {}\n\
         Depends: {}\nSection: devel\nPriority: optional\nHomepage: https://ide.ifzenelse.space\n\
         Description: {}\n Part of Zen IDE, the open-source family of IDEs in pure Rust (preview).\n",
        package.version,
        installed.div_ceil(1024),
        package.depends,
        package.summary
    );
    let mut meta = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    directory(&mut meta, "./")?;
    file(&mut meta, "./control", control.as_bytes(), 0o644)?;
    let meta = meta.into_inner().and_then(|gz| gz.finish()).map_err(|error| error.to_string())?;

    let mut deb = b"!<arch>\n".to_vec();
    member(&mut deb, "debian-binary", b"2.0\n");
    member(&mut deb, "control.tar.gz", &meta);
    member(&mut deb, "data.tar.gz", &data);
    Ok(deb)
}

pub(crate) fn copyright(files: &Path) -> Result<String, String> {
    let licences = files.join(crate::notices::LICENCES_FOLDER);
    let mut text = String::new();
    for file in ["NOTICE", "LICENSE-MIT", "LICENSE-APACHE", crate::notices::NOTICES_FILE] {
        let path = licences.join(file);
        let body = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        text.push_str(&format!("{}\n{file}\n\n{}\n\n", "=".repeat(78), body.trim_end()));
    }
    Ok(text)
}

fn header(kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(size);
    header.set_uid(0);
    header.set_gid(0);
    let _ = header.set_username("root");
    let _ = header.set_groupname("root");
    header.set_mtime(0);
    header
}

fn append<W: Write>(tar: &mut tar::Builder<W>, mut header: tar::Header, path: &str, data: &[u8]) -> Result<(), String> {
    if path.len() >= 100 {
        return tar.append_data(&mut header, path, data).map_err(|error| error.to_string());
    }
    let name = &mut header.as_gnu_mut().ok_or("not a GNU header")?.name;
    name.fill(0);
    name[..path.len()].copy_from_slice(path.as_bytes());
    header.set_cksum();
    tar.append(&header, data).map_err(|error| error.to_string())
}

fn directory<W: Write>(tar: &mut tar::Builder<W>, path: &str) -> Result<(), String> {
    append(tar, header(tar::EntryType::Directory, 0o755, 0), path, &[])
}

fn file<W: Write>(tar: &mut tar::Builder<W>, path: &str, bytes: &[u8], mode: u32) -> Result<(), String> {
    append(tar, header(tar::EntryType::Regular, mode, bytes.len() as u64), path, bytes)
}

fn add_tree<W: Write>(
    tar: &mut tar::Builder<W>,
    from: &Path,
    to: &str,
    binary: &str,
    installed: &mut u64,
) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|error| format!("{}: {error}", from.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let inside = format!("{to}/{name}");
        if path.is_dir() {
            directory(tar, &format!("{inside}/"))?;
            add_tree(tar, &path, &inside, binary, installed)?;
        } else {
            let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let executable = inside.ends_with(&format!("/bin/{binary}")) || is_executable(&path);
            *installed += bytes.len() as u64;
            file(tar, &inside, &bytes, if executable { 0o755 } else { 0o644 })?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}

fn member(archive: &mut Vec<u8>, name: &str, data: &[u8]) {
    archive.extend_from_slice(
        format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, "100644", data.len()).as_bytes(),
    );
    archive.extend_from_slice(data);
    if data.len() % 2 == 1 {
        archive.push(b'\n');
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Read;

    use super::*;

    pub(crate) fn members(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert!(archive.starts_with(b"!<arch>\n"));
        let mut at = 8;
        let mut members = Vec::new();
        while at < archive.len() {
            let header = &archive[at..at + 60];
            assert_eq!(&header[58..60], b"`\n");
            let name = String::from_utf8_lossy(&header[..16]).trim_end().to_owned();
            let size: usize = String::from_utf8_lossy(&header[48..58]).trim().parse().unwrap();
            members.push((name, archive[at + 60..at + 60 + size].to_vec()));
            at += 60 + size + size % 2;
        }
        members
    }

    type Entry = (String, tar::EntryType, u32, u64, Option<String>, Vec<u8>);

    fn entries(archive: &[u8]) -> Vec<Entry> {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        tar.entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let header = entry.header().clone();
                let path = entry.path().unwrap().display().to_string();
                let link = entry.link_name().unwrap().map(|link| link.display().to_string());
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                (path, header.entry_type(), header.mode().unwrap(), header.uid().unwrap(), link, bytes)
            })
            .collect()
    }

    #[test]
    fn a_tar_entry_carries_no_date_and_no_owner_of_the_build() {
        let header = header(tar::EntryType::Regular, 0o644, 1);
        assert_eq!((header.mtime().unwrap(), header.uid().unwrap(), header.gid().unwrap()), (0, 0, 0));
        assert_eq!((header.username().unwrap(), header.groupname().unwrap()), (Some("root"), Some("root")));
    }

    #[test]
    fn a_package_has_the_layout_dpkg_expects() {
        let dir = tempfile::tempdir().unwrap();
        let files = dir.path().join("files");
        std::fs::create_dir_all(files.join("bin")).unwrap();
        std::fs::create_dir_all(files.join("Resources/plugins/json")).unwrap();
        std::fs::write(files.join("bin/rustronin"), b"\x7fELF").unwrap();
        std::fs::write(files.join("Resources/plugins/json/plugin.wasm"), b"\0asm").unwrap();
        std::fs::create_dir_all(files.join("Licenses")).unwrap();
        for file in ["NOTICE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES"] {
            std::fs::write(files.join("Licenses").join(file), format!("text of {file}\n")).unwrap();
        }
        let package = Package {
            name: "rustronin",
            version: "26.10.1",
            display: "RustRonin",
            binary: "rustronin",
            summary: "RustRonin, a Zen IDE",
            depends: "libc6 (>= 2.35), libx11-6, libxkbcommon0",
            categories: "Development;IDE;",
            icon: b"\x89PNG",
        };
        let deb = build(&package, &files).unwrap();
        let members = members(&deb);
        let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["debian-binary", "control.tar.gz", "data.tar.gz"]);
        assert_eq!(members[0].1, b"2.0\n");

        let control = entries(&members[1].1);
        let text = String::from_utf8(control.iter().find(|entry| entry.0 == "./control").unwrap().5.clone()).unwrap();
        assert!(text.starts_with("Package: rustronin\nVersion: 26.10.1\nArchitecture: amd64\n"), "{text}");
        assert!(text.contains("Depends: libc6 (>= 2.35), libx11-6, libxkbcommon0\n"), "{text}");
        assert!(text.ends_with("\n") && text.contains("Description: RustRonin, a Zen IDE\n "), "{text}");

        let data = entries(&members[2].1);
        let find = |path: &str| data.iter().find(|entry| entry.0 == path).unwrap_or_else(|| panic!("no {path}"));
        assert_eq!(find("./opt/rustronin/bin/rustronin").2, 0o755);
        assert_eq!(find("./opt/rustronin/Resources/plugins/json/plugin.wasm").2, 0o644);
        let link = find("./usr/bin/rustronin");
        assert_eq!((link.1, link.4.as_deref()), (tar::EntryType::Symlink, Some("/opt/rustronin/bin/rustronin")));
        let desktop = String::from_utf8(find("./usr/share/applications/rustronin.desktop").5.clone()).unwrap();
        assert!(desktop.contains("Exec=/opt/rustronin/bin/rustronin %F\n") && desktop.contains("Icon=rustronin\n"));
        assert_eq!(find("./usr/share/icons/hicolor/256x256/apps/rustronin.png").5, b"\x89PNG");
        let copyright = String::from_utf8(find("./usr/share/doc/rustronin/copyright").5.clone()).unwrap();
        assert!(copyright.contains("text of NOTICE") && copyright.contains("text of THIRD-PARTY-NOTICES"));
        assert!(data.iter().all(|entry| entry.3 == 0), "everything is root's");
        assert!(
            data.iter().position(|entry| entry.0 == "./opt/")
                < data.iter().position(|entry| entry.0 == "./opt/rustronin/bin/rustronin")
        );
        assert_eq!(build(&package, &files).unwrap(), deb, "the same files make the same package");
    }
}
