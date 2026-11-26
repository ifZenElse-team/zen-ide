use std::path::{Path, PathBuf};

use crate::cross::{Builder, Product, product, publish, upload};
use crate::release::{bundle_plugins, bundled_plugins, config_dir, notes, release_settings, run};

const PLATFORM: &str = "linux-x86_64";

const BUILDER: Builder =
    Builder { image: "zen-linux-builder", folder: "linux", docker_platform: Some("linux/amd64"), target: None };

pub(crate) fn release_command(args: &[String]) -> Result<(), String> {
    let [id, version] = args else {
        return Err(
            "usage: cargo xtask release-linux <product> <version>   (toolbox or an IDE: gogarden, rustronin, webwave, jvmjizo, sqlscroll, devopsdaruma)"
                .to_owned(),
        );
    };
    crate::release::check_source()?;
    release(id, version)
}

pub(crate) fn release(id: &str, version: &str) -> Result<(), String> {
    let product = product(id)?;
    if product.id != "toolbox" {
        bundled_plugins(product.id)?;
    }
    let notes = notes(None, &crate::changelog::product_path(product.id), version)?;
    let (server, token) = release_settings()?;
    let key = config_dir()?.join("publisher.key");

    println!("==> Building {} {version} for Linux (in Docker)", product.name);
    let built = BUILDER.build(version, &[product.package])?;
    let binary = built.join(product.binary);

    println!("==> Laying out the Linux release");
    let dist = PathBuf::from("target/dist").join(format!("{}-{version}-linux", product.id));
    let _ = std::fs::remove_dir_all(&dist);
    let folder = dist.join("files");
    lay_out(&product, &binary, &folder)?;
    let archive = dist.join(format!("{}-{version}-{PLATFORM}.tar.gz", product.id));
    run(
        std::process::Command::new("tar")
            .env("COPYFILE_DISABLE", "1")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&folder)
            .arg("."),
        "tar",
    )?;

    println!("==> Building the .deb package");
    let deb = deb(&product, version, &folder, &dist)?;

    println!("==> Publishing the {PLATFORM} build to {server}");
    publish(&server, &token, &key, &product, version, PLATFORM, &notes, &archive)?;
    let url = upload(&deb, &server, &token, product.id, version)?;
    println!("\n{} {version} for Linux is published.", product.name);
    println!("  Package: {url}");
    Ok(())
}

fn lay_out(product: &Product, binary: &Path, folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder.join("bin")).map_err(|error| error.to_string())?;
    std::fs::copy(binary, folder.join("bin").join(product.binary))
        .map_err(|error| format!("{}: {error}", binary.display()))?;
    if product.id != "toolbox" {
        bundle_plugins(product.id, &folder.join("Resources/plugins"))?;
    }
    crate::notices::lay_out(
        &folder.join(crate::notices::LICENCES_FOLDER),
        product.name,
        &crate::notices::product_shipped(product.id, Some(crate::notices::LINUX))?,
        true,
    )
}

fn package_name(product: &Product) -> &'static str {
    if product.id == "toolbox" { "zen-toolbox" } else { product.id }
}

fn depends(_: &Product) -> &'static str {
    "libc6 (>= 2.35), libx11-6, libxkbcommon0"
}

fn deb(product: &Product, version: &str, files: &Path, dist: &Path) -> Result<PathBuf, String> {
    let package = package_name(product);
    let root = dist.join("deb").join(package);
    let opt = root.join("opt").join(package);
    std::fs::create_dir_all(&opt).map_err(|error| error.to_string())?;
    run(std::process::Command::new("cp").arg("-R").arg(format!("{}/.", files.display())).arg(&opt), "cp")?;
    let write = |path: PathBuf, text: String| -> Result<(), String> {
        std::fs::create_dir_all(path.parent().ok_or("no parent")?).map_err(|error| error.to_string())?;
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
    };
    let executable = format!("/opt/{package}/bin/{}", product.binary);
    let size_kb = folder_size(&opt) / 1024;
    write(
        root.join("DEBIAN/control"),
        format!(
            "Package: {package}\nVersion: {version}\nArchitecture: amd64\nMaintainer: ifzenelse\n\
             Installed-Size: {size_kb}\nDepends: {}\nSection: devel\nPriority: optional\nHomepage: https://ide.ifzenelse.space\n\
             Description: {}\n {}\n",
            depends(product),
            if product.id == "toolbox" {
                "Installs, updates and launches the Zen IDEs".to_owned()
            } else {
                format!("{}, a Zen IDE", product.name)
            },
            "Part of Zen IDE, the open-source family of IDEs in pure Rust (preview).",
        ),
    )?;
    write(root.join(format!("usr/share/doc/{package}/copyright")), crate::deb::copyright(files)?)?;
    let desktop_categories = if product.id == "toolbox" { "Development;" } else { "Development;IDE;" };
    write(
        root.join(format!("usr/share/applications/{package}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={executable} %F\nIcon={package}\nCategories={desktop_categories}\nTerminal=false\nStartupWMClass={package}\n",
            product.name
        ),
    )?;
    let icon = root.join(format!("usr/share/icons/hicolor/256x256/apps/{package}.png"));
    std::fs::create_dir_all(icon.parent().ok_or("no parent")?).map_err(|error| error.to_string())?;
    std::fs::write(&icon, crate::icon::product_png(product.id, 256)?).map_err(|error| error.to_string())?;
    let deb_name = format!("{package}_{version}_amd64.deb");
    BUILDER.run_in(
        &dist.join("deb"),
        &format!(
            "mkdir -p {package}/usr/bin && ln -sf {executable} {package}/usr/bin/{command} \
             && chmod 755 {package}/opt/{package}/bin/{binary} \
             && dpkg-deb --build --root-owner-group {package} {deb_name} >/dev/null",
            command = package,
            binary = product.binary,
        ),
        "dpkg-deb",
    )?;
    let built = dist.join("deb").join(&deb_name);
    let target = dist.join(&deb_name);
    std::fs::rename(&built, &target).map_err(|error| error.to_string())?;
    Ok(target)
}

fn folder_size(folder: &Path) -> u64 {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            _ => entry.metadata().map_or(0, |metadata| metadata.len()),
        })
        .sum()
}
