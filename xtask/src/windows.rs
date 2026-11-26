use std::path::PathBuf;
use std::process::Command;

use crate::cross::{Builder, product, publish, upload};
use crate::release::{bundle_plugins, bundled_plugins, check_source, config_dir, notes, release_settings, run};

const PLATFORM: &str = "windows-x86_64";

const BUILDER: Builder = Builder {
    image: "zen-windows-builder",
    folder: "windows",
    docker_platform: None,
    target: Some("x86_64-pc-windows-gnu"),
};

pub(crate) fn release(args: &[String]) -> Result<(), String> {
    let [id, version] = args else {
        return Err(
            "usage: cargo xtask release-windows <product> <version>   (toolbox or an IDE: gogarden, rustronin, webwave, jvmjizo, sqlscroll, devopsdaruma)"
                .to_owned(),
        );
    };
    let product = product(id)?;
    let (id, name, version) = (product.id, product.name, version.as_str());
    if id != "toolbox" {
        bundled_plugins(id)?;
    }
    let notes = notes(None, &crate::changelog::product_path(id), version)?;
    check_source()?;
    let (server, token) = release_settings()?;
    let key = config_dir()?.join("publisher.key");

    println!("==> Building {name} {version} for Windows (in Docker)");
    let built = BUILDER.build(version, &[product.package])?;

    println!("==> Laying out the release");
    let dist = PathBuf::from("target/dist").join(format!("{id}-{version}-windows"));
    let _ = std::fs::remove_dir_all(&dist);
    let folder = dist.join(name);
    std::fs::create_dir_all(folder.join("bin")).map_err(|error| error.to_string())?;
    let exe = built.join(format!("{}.exe", product.binary));
    std::fs::copy(&exe, folder.join("bin").join(format!("{id}.exe")))
        .map_err(|error| format!("{}: {error}", exe.display()))?;
    if id != "toolbox" {
        bundle_plugins(id, &folder.join("Resources/plugins"))?;
    }
    crate::notices::lay_out(
        &folder.join(crate::notices::LICENCES_FOLDER),
        name,
        &crate::notices::product_shipped(id, Some(crate::notices::WINDOWS))?,
        true,
    )?;
    let archive = dist.join(format!("{id}-{version}-{PLATFORM}.tar.gz"));
    run(
        Command::new("tar").env("COPYFILE_DISABLE", "1").arg("-czf").arg(&archive).arg("-C").arg(&folder).arg("."),
        "tar",
    )?;
    let zip_name = format!("{}-{version}-{PLATFORM}.zip", name.replace(' ', "-"));
    let zip = dist.join(&zip_name);
    run(Command::new("zip").args(["-qr", "-X"]).arg(&zip_name).arg(name).current_dir(&dist), "zip")?;

    println!("==> Publishing the {PLATFORM} build to {server}");
    publish(&server, &token, &key, &product, version, PLATFORM, &notes, &archive)?;
    let url = upload(&zip, &server, &token, id, version)?;
    println!("\n{name} {version} for Windows is published (preview: built and checked on macOS, not run on Windows).");
    println!("  Download: {url}");
    Ok(())
}
