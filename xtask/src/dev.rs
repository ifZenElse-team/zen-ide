use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const TOKEN: &str = "dev-token";
const ADDRESS: &str = "127.0.0.1:8787";

const PRODUCTS: &[(&str, &str)] = &[
    ("toolbox", "Zen Toolbox"),
    ("rustronin", "RustRonin"),
    ("gogarden", "GoGarden"),
    ("pypetal", "PyPetal"),
    ("jvmjizo", "JvmJizo"),
    ("webwave", "WebWave"),
    ("cppcrane", "CppCrane"),
    ("sharpshuriken", "SharpShuriken"),
    ("phppagoda", "PhpPagoda"),
    ("rubyryokan", "RubyRyokan"),
    ("sqlscroll", "SqlScroll"),
    ("devopsdaruma", "DevopsDaruma"),
    ("testtotami", "TestTotami"),
];

fn root() -> PathBuf {
    PathBuf::from("target/dev-releases")
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command.status().map_err(|error| format!("cannot run {what}: {error}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed")) }
}

fn releases_binary() -> PathBuf {
    PathBuf::from("target/debug").join(format!("zen-releases{}", std::env::consts::EXE_SUFFIX))
}

fn dev_key() -> Result<(PathBuf, String), String> {
    let key = root().join("dev-publisher.key");
    let public = root().join("dev-publisher.pub");
    if !key.exists() {
        std::fs::create_dir_all(root()).map_err(|error| error.to_string())?;
        let output = Command::new(releases_binary())
            .args(["keygen", "--out"])
            .arg(&key)
            .output()
            .map_err(|error| format!("cannot run zen-releases keygen: {error}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let hex =
            text.lines().find_map(|line| line.strip_prefix("Public key: ")).ok_or("keygen printed no public key")?;
        std::fs::write(&public, hex).map_err(|error| error.to_string())?;
    }
    let hex = std::fs::read_to_string(&public).map_err(|error| format!("cannot read {}: {error}", public.display()))?;
    Ok((key, hex.trim().to_owned()))
}

fn artifact(id: &str, version: &str) -> Result<PathBuf, String> {
    let staging = root().join("staging").join(format!("{id}-{version}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("bin")).map_err(|error| error.to_string())?;
    if id == "toolbox" {
        let target_dir = root().join("toolbox-builds").join(version);
        run(
            Command::new(cargo())
                .args(["build", "--quiet", "--package", "zen-toolbox-app", "--target-dir"])
                .arg(&target_dir)
                .env("ZEN_BUILD_VERSION", version),
            "cargo build (Zen Toolbox)",
        )?;
        let name = format!("zen-toolbox{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(target_dir.join("debug").join(&name), staging.join("bin").join(&name))
            .map_err(|error| error.to_string())?;
    } else {
        let name = PRODUCTS.iter().find(|(product, _)| *product == id).map_or(id, |(_, name)| name);
        let script = format!(
            "#!/bin/sh\n# A test build published by `cargo xtask dev-server`; not a real IDE.\n\
             message='{name} {version} is a test build from the local release server. The real IDE is not built yet.'\n\
             osascript -e \"display dialog \\\"$message\\\" buttons {{\\\"OK\\\"}} default button 1 with title \\\"{name}\\\"\" >/dev/null 2>&1 \
             || zenity --info --title=\"{name}\" --text=\"$message\" 2>/dev/null || echo \"$message\"\n"
        );
        let path = staging.join("bin").join(id);
        std::fs::write(&path, script).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| error.to_string())?;
        }
    }
    let archive = root().join("staging").join(format!("{id}-{version}-{}.tar.gz", platform()));
    run(
        Command::new("tar").env("COPYFILE_DISABLE", "1").arg("-czf").arg(&archive).arg("-C").arg(&staging).arg("bin"),
        "tar",
    )?;
    Ok(archive)
}

fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn publish(key: &Path, id: &str, version: &str) -> Result<(), String> {
    let name = PRODUCTS
        .iter()
        .find(|(product, _)| *product == id)
        .map(|(_, name)| *name)
        .ok_or_else(|| format!("unknown product `{id}`"))?;
    let file = artifact(id, version)?;
    run(
        Command::new(releases_binary())
            .args(["publish", "--server", &format!("http://{ADDRESS}"), "--key"])
            .arg(key)
            .args([
                "--product",
                id,
                "--name",
                name,
                "--version",
                version,
                "--notes",
                "### Test build\n\n- Published by `cargo xtask dev-server` or `dev-publish`.",
            ])
            .arg(&file)
            .env("ZEN_RELEASES_TOKEN", TOKEN),
        "zen-releases publish",
    )
}

fn build_tools() -> Result<(), String> {
    run(
        Command::new(cargo()).args(["build", "--quiet", "--package", "zen-releases", "--package", "zen-toolbox-app"]),
        "cargo build",
    )
}

fn wait_for_server(server: &mut Child) -> Result<(), String> {
    for _ in 0..100 {
        if let Ok(Some(status)) = server.try_wait() {
            return Err(format!("the release server exited ({status}); is port 8787 in use?"));
        }
        if std::net::TcpStream::connect(ADDRESS).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("the release server did not start".to_owned())
}

pub(crate) fn server() -> Result<(), String> {
    build_tools()?;
    let (key, public) = dev_key()?;
    let mut server = Command::new(releases_binary())
        .args(["serve", "--listen", ADDRESS, "--public-key", &public, "--data"])
        .arg(root().join("data"))
        .env("ZEN_RELEASES_TOKEN", TOKEN)
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start the release server: {error}"))?;
    wait_for_server(&mut server)?;

    for (id, version) in [("rustronin", "0.1.0"), ("gogarden", "0.1.0"), ("toolbox", "0.0.1")] {
        publish(&key, id, version)?;
    }

    let tools_dir = std::env::current_dir().map_err(|error| error.to_string())?.join(root()).join("tools");
    println!("\nThe local release server is running on http://{ADDRESS} (Ctrl-C to stop).");
    println!("Published: RustRonin 0.1.0, GoGarden 0.1.0, Zen Toolbox 0.0.1 (test builds).\n");
    println!("Run Zen Toolbox against it:\n");
    println!(
        "  ./target/debug/zen-toolbox --releases http://{ADDRESS}/releases/manifest.json \\\n      --trust-key {public} \\\n      --tools-dir {}\n",
        tools_dir.display()
    );
    println!("Publish a newer version while it runs, to try updates and rollback:\n");
    println!("  cargo xtask dev-publish rustronin 0.2.0\n");
    let status = server.wait().map_err(|error| error.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("the release server exited with {status}")) }
}

pub(crate) fn publish_command(args: &[String]) -> Result<(), String> {
    let [id, version] = args else {
        return Err("usage: cargo xtask dev-publish <product> <version>".to_owned());
    };
    build_tools()?;
    let (key, _) = dev_key()?;
    if std::net::TcpStream::connect(ADDRESS).is_err() {
        return Err("the release server is not running; start it with `cargo xtask dev-server`".to_owned());
    }
    publish(&key, id, version)?;
    println!("Published {id} {version}. In Zen Toolbox: Settings menu → Check for updates.");
    Ok(())
}
