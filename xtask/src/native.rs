use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cross::Product;
use crate::release::{bundle_plugins, bundled_plugins, run};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Platform {
    Macos,
    Linux,
    Windows,
}

impl Platform {
    pub(crate) fn id(self) -> String {
        match self {
            Self::Macos => format!("macos-{}", std::env::consts::ARCH),
            Self::Linux => "linux-x86_64".to_owned(),
            Self::Windows => "windows-x86_64".to_owned(),
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }

    pub(crate) fn target(self) -> Option<&'static str> {
        match self {
            Self::Macos => None,
            Self::Linux => Some("x86_64-unknown-linux-gnu"),
            Self::Windows => Some("x86_64-pc-windows-gnu"),
        }
    }
}

pub(crate) fn platforms(text: &str) -> Result<Vec<Platform>, String> {
    let mut chosen = Vec::new();
    for word in text.split(',').map(str::trim) {
        match word {
            "macos" => chosen.push(Platform::Macos),
            "linux" => chosen.push(Platform::Linux),
            "windows" => chosen.push(Platform::Windows),
            "all" => chosen.extend([Platform::Macos, Platform::Linux, Platform::Windows]),
            other => return Err(format!("unknown platform `{other}`; use macos, linux, windows or all")),
        }
    }
    chosen.sort();
    chosen.dedup();
    Ok(chosen)
}

pub(crate) fn is_macos(platform: &str) -> bool {
    platform.starts_with("macos-")
}

pub(crate) fn tools_problem(platform: Platform) -> Option<String> {
    let has =
        |program: &str, arg: &str| Command::new(program).arg(arg).output().is_ok_and(|output| output.status.success());
    match platform {
        Platform::Macos => None,
        Platform::Linux if !has("zig", "version") || !has("cargo-zigbuild", "--version") => Some(
            "the Linux build needs zig and cargo-zigbuild (brew install zig; cargo install --locked cargo-zigbuild; \
             docs/development.md)"
                .to_owned(),
        ),
        Platform::Windows if !has(&windows_linker(), "--version") => Some(
            "the Windows build needs MinGW-w64's x86_64-w64-mingw32-gcc (brew install mingw-w64; docs/development.md)"
                .to_owned(),
        ),
        _ => None,
    }
}

pub(crate) fn windows_linker() -> String {
    std::env::var("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER").unwrap_or_else(|_| "x86_64-w64-mingw32-gcc".to_owned())
}

pub(crate) fn cargo_build(platform: Platform, package: &str) -> Vec<String> {
    let target = platform.target().unwrap_or_default();
    let mut args: Vec<String> = match platform {
        Platform::Linux => vec!["zigbuild".into(), "--target".into(), format!("{target}.2.35")],
        _ => vec!["build".into(), "--target".into(), target.to_owned()],
    };
    args.extend(["--release", "--locked", "--package", package].map(str::to_owned));
    args
}

pub(crate) fn build(
    product: &Product,
    version: &str,
    platform: Platform,
    out: &Path,
    gate: crate::release::Gate,
) -> Result<(), String> {
    if platform == Platform::Macos {
        return Err("the macOS build is the application bundle, not this one".to_owned());
    }
    let given = (gate == crate::release::Gate::Trial).then_some(crate::release::TRIAL_NOTES);
    let notes = crate::release::notes(given, &crate::changelog::product_path(product.id), version)?;
    if product.id != "toolbox" {
        bundled_plugins(product.id)?;
    }
    if let Some(problem) = tools_problem(platform) {
        return Err(problem);
    }
    gate.pass()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target_dir = crate::bench::target_dir(&cargo)?;
    println!("==> Building {} {version} for {} (natively)", product.name, platform.id());
    let mut build = Command::new(&cargo);
    build.args(cargo_build(platform, product.package)).env("ZEN_BUILD_VERSION", version);
    if platform == Platform::Windows {
        build.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", windows_linker());
    }
    run(&mut build, "cargo build (cross)")?;
    let built = target_dir.join(platform.target().unwrap_or_default()).join("release");

    crate::release_flow::clean_out(out)?;
    let dist = target_dir.join("dist").join(format!("{}-{version}-{}", product.id, platform.word()));
    let _ = std::fs::remove_dir_all(&dist);
    let folder = match platform {
        Platform::Windows => dist.join(product.name),
        _ => dist.join("files"),
    };
    let exe = if platform == Platform::Windows { format!("{}.exe", product.binary) } else { product.binary.to_owned() };
    std::fs::create_dir_all(folder.join("bin")).map_err(|error| error.to_string())?;
    let from = built.join(&exe);
    let installed =
        if platform == Platform::Windows { format!("{}.exe", product.id) } else { product.binary.to_owned() };
    std::fs::copy(&from, folder.join("bin").join(&installed))
        .map_err(|error| format!("{}: {error}", from.display()))?;
    if product.id != "toolbox" {
        bundle_plugins(product.id, &folder.join("Resources/plugins"))?;
    }
    crate::notices::lay_out(
        &folder.join(crate::notices::LICENCES_FOLDER),
        product.name,
        &crate::notices::product_shipped(product.id, platform.target())?,
        true,
    )?;
    let archive = out.join(format!("{}-{version}-{}.tar.gz", product.id, platform.id()));
    run(
        Command::new("tar").env("COPYFILE_DISABLE", "1").arg("-czf").arg(&archive).arg("-C").arg(&folder).arg("."),
        "tar",
    )?;
    let download = match platform {
        Platform::Linux => {
            let package = package_name(product);
            let icon = crate::icon::product_png(product.id, 256)?;
            let summary = if product.id == "toolbox" {
                "Installs, updates and launches the Zen IDEs".to_owned()
            } else {
                format!("{}, a Zen IDE", product.name)
            };
            let deb = crate::deb::build(
                &crate::deb::Package {
                    name: package,
                    version,
                    display: product.name,
                    binary: product.binary,
                    summary: &summary,
                    depends: "libc6 (>= 2.35), libx11-6, libxkbcommon0",
                    categories: if product.id == "toolbox" { "Development;" } else { "Development;IDE;" },
                    icon: &icon,
                },
                &folder,
            )?;
            let path = out.join(format!("{package}_{version}_amd64.deb"));
            std::fs::write(&path, deb).map_err(|error| error.to_string())?;
            (path, "package")
        }
        _ => {
            let name = format!("{}-{version}-{}.zip", product.name.replace(' ', "-"), platform.id());
            run(Command::new("zip").args(["-qr", "-X"]).arg(&name).arg(product.name).current_dir(&dist), "zip")?;
            let path = out.join(&name);
            std::fs::rename(dist.join(&name), &path).map_err(|error| error.to_string())?;
            (path, "download")
        }
    };
    let kind = if product.id == "toolbox" { "toolbox" } else { "ide" };
    let mut record = crate::release_flow::BuildRecord::new(kind, product.id, product.name, version, notes);
    record.platform = platform.id();
    record.gate = gate.recorded().to_owned();
    crate::release_flow::finish(out, record, &[(archive, "update"), download])
}

pub(crate) fn package_name(product: &Product) -> &'static str {
    if product.id == "toolbox" { "zen-toolbox" } else { product.id }
}

pub(crate) fn folder(parent: &Path, id: &str, version: &str, platform: Platform) -> PathBuf {
    match platform {
        Platform::Macos => parent.join(format!("{id}-{version}")),
        other => parent.join(format!("{id}-{version}-{}", other.word())),
    }
}

pub(crate) fn version_problem(
    product: &str,
    version: &str,
    platforms: &[Platform],
    published: &[(String, String, Vec<String>)],
) -> Option<String> {
    if platforms.contains(&Platform::Macos) || platforms.is_empty() {
        return None;
    }
    let on_macos = published.iter().any(|(id, published, platforms)| {
        id == product && published == version && platforms.iter().any(|platform| is_macos(platform))
    });
    (!on_macos).then(|| {
        format!(
            "{product} {version} is not published for macOS: a Linux or Windows build is of a version released for \
             macOS, or built together with it (--platform all)"
        )
    })
}

pub(crate) fn published() -> Result<Vec<(String, String, Vec<String>)>, String> {
    let manifest = crate::release::public_manifest()?;
    Ok(manifest["products"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|product| {
            let id = product["id"].as_str().unwrap_or_default().to_owned();
            product["releases"].as_array().into_iter().flatten().map(move |release| {
                let platforms = release["artifacts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|artifact| artifact["platform"].as_str().map(str::to_owned))
                    .collect();
                (id.clone(), release["version"].as_str().unwrap_or_default().to_owned(), platforms)
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_are_chosen_by_name() {
        assert_eq!(platforms("macos").unwrap(), [Platform::Macos]);
        assert_eq!(platforms("all").unwrap(), [Platform::Macos, Platform::Linux, Platform::Windows]);
        assert_eq!(platforms("windows,linux").unwrap(), [Platform::Linux, Platform::Windows], "macOS-first order");
        assert!(platforms("freebsd").unwrap_err().contains("unknown platform"));
        assert_eq!(Platform::Linux.id(), "linux-x86_64");
        assert_eq!(Platform::Windows.id(), "windows-x86_64");
        assert!(Platform::Macos.id().starts_with("macos-"));
        assert_eq!(
            cargo_build(Platform::Linux, "rust-ronin").join(" "),
            "zigbuild --target x86_64-unknown-linux-gnu.2.35 --release --locked --package rust-ronin"
        );
        assert_eq!(
            cargo_build(Platform::Windows, "zen-toolbox-app").join(" "),
            "build --target x86_64-pc-windows-gnu --release --locked --package zen-toolbox-app"
        );
        let parent = Path::new("/out");
        assert_eq!(folder(parent, "webwave", "26.10.1", Platform::Macos), Path::new("/out/webwave-26.10.1"));
        assert_eq!(folder(parent, "webwave", "26.10.1", Platform::Linux), Path::new("/out/webwave-26.10.1-linux"));
    }

    #[test]
    fn another_platform_follows_a_macos_release() {
        let published = vec![
            ("rustronin".to_owned(), "26.10.1".to_owned(), vec!["macos-aarch64".to_owned()]),
            ("toolbox".to_owned(), "0.3.15".to_owned(), vec!["macos-aarch64".to_owned(), "linux-x86_64".to_owned()]),
        ];
        let linux = [Platform::Linux];
        assert_eq!(version_problem("rustronin", "26.10.1", &linux, &published), None);
        assert_eq!(version_problem("toolbox", "0.3.15", &[Platform::Windows], &published), None);
        assert!(
            version_problem("rustronin", "26.10.2", &linux, &published).unwrap().contains("not published for macOS")
        );
        assert_eq!(version_problem("rustronin", "26.10.2", &[Platform::Macos, Platform::Linux], &published), None);
    }
}
