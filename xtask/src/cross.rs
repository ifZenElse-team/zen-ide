use std::path::{Path, PathBuf};
use std::process::Command;

use crate::release::{IDES, run};

pub(crate) struct Builder {
    pub(crate) image: &'static str,
    pub(crate) folder: &'static str,
    pub(crate) docker_platform: Option<&'static str>,
    pub(crate) target: Option<&'static str>,
}

impl Builder {
    pub(crate) fn build(&self, version: &str, packages: &[&str]) -> Result<PathBuf, String> {
        let root = std::env::current_dir().map_err(|error| error.to_string())?;
        println!("==> Preparing the build image ({})", self.image);
        let deploy = format!("deploy/{}", self.folder);
        run(Command::new("docker").args(["build", "-q", "-t", self.image, &deploy]), "docker build")?;
        let target_dir = root.join("target").join(self.folder);
        std::fs::create_dir_all(&target_dir).map_err(|error| error.to_string())?;
        let mut cargo = String::new();
        if let Some(target) = self.target {
            cargo.push_str(&format!("rustup target add {target} >/dev/null 2>&1 && "));
        }
        cargo.push_str("cargo build --release --locked");
        if let Some(target) = self.target {
            cargo.push_str(&format!(" --target {target}"));
        }
        for package in packages {
            cargo.push_str(&format!(" --package {package}"));
        }
        let mut docker = Command::new("docker");
        docker.args(["run", "--rm"]);
        if let Some(platform) = self.docker_platform {
            docker.args(["--platform", platform]);
        }
        docker.args([
            "-v",
            &format!("{}:/src:ro", root.display()),
            "-v",
            &format!("{}:/target", target_dir.display()),
            "-v",
            &format!("{}-cargo:/usr/local/cargo/registry", self.image),
            "-v",
            &format!("{}-rustup:/usr/local/rustup", self.image),
            "-e",
            "CARGO_TARGET_DIR=/target",
            "-e",
            &format!("ZEN_BUILD_VERSION={version}"),
            self.image,
            "sh",
            "-c",
            &cargo,
        ]);
        run(&mut docker, &format!("the {} build", self.folder))?;
        Ok(match self.target {
            Some(target) => target_dir.join(target).join("release"),
            None => target_dir.join("release"),
        })
    }

    pub(crate) fn run_in(&self, folder: &Path, script: &str, what: &str) -> Result<(), String> {
        let folder = std::fs::canonicalize(folder).map_err(|error| error.to_string())?;
        let mut docker = Command::new("docker");
        docker.args(["run", "--rm"]);
        if let Some(platform) = self.docker_platform {
            docker.args(["--platform", platform]);
        }
        docker.args(["-v", &format!("{}:/work", folder.display()), "-w", "/work", self.image, "sh", "-c", script]);
        run(&mut docker, what)
    }
}

pub(crate) struct Product {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) package: &'static str,
    pub(crate) binary: &'static str,
}

pub(crate) fn product(id: &str) -> Result<Product, String> {
    if id == "toolbox" {
        return Ok(Product { id: "toolbox", name: "Zen Toolbox", package: "zen-toolbox-app", binary: "zen-toolbox" });
    }
    IDES.iter()
        .find(|(product, _, _)| *product == id)
        .map(|&(id, name, package)| Product { id, name, package, binary: id })
        .ok_or_else(|| {
            format!(
                "unknown product `{id}`; known: toolbox, {}",
                IDES.iter().map(|ide| ide.0).collect::<Vec<_>>().join(", ")
            )
        })
}

#[allow(clippy::too_many_arguments, reason = "what one publish needs")]
pub(crate) fn publish(
    server: &str,
    token: &str,
    key: &Path,
    product: &Product,
    version: &str,
    platform: &str,
    notes: &str,
    archive: &Path,
) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(
        Command::new(&cargo).args(["build", "--release", "--locked", "--package", "zen-releases"]),
        "cargo build (zen-releases)",
    )?;
    run(
        Command::new("target/release/zen-releases")
            .args(["publish", "--server", server, "--key"])
            .arg(key)
            .args(["--product", product.id, "--name", product.name, "--version", version, "--platform", platform])
            .args(["--notes", notes])
            .arg(archive)
            .env("ZEN_RELEASES_TOKEN", token),
        "zen-releases publish",
    )
}

pub(crate) fn upload(file: &Path, server: &str, token: &str, product: &str, version: &str) -> Result<String, String> {
    let name = file.file_name().and_then(|name| name.to_str()).ok_or("bad file name")?;
    run(
        Command::new("curl")
            .args(["-fsS", "-o", "/dev/null", "-X", "PUT", "-H", &format!("Authorization: Bearer {token}")])
            .arg("--data-binary")
            .arg(format!("@{}", file.display()))
            .arg(format!("{server}/api/v1/artifacts/{product}/{version}/{name}")),
        "uploading the download",
    )?;
    Ok(format!("{server}/releases/artifacts/{product}/{version}/{name}"))
}
