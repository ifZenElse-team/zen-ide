use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command.status().map_err(|error| format!("cannot run {what}: {error}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed")) }
}

pub(crate) fn config_dir() -> Result<PathBuf, String> {
    if let Some(problem) = crate::ci_host::Ci::from_environment().secrets_problem() {
        return Err(problem);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("cannot find the home directory")?;
    Ok(base.join("zen-ide"))
}

pub(crate) struct Secrets {
    pub(crate) server: String,
    pub(crate) token: String,
    pub(crate) key: Option<PathBuf>,
}

pub(crate) fn secrets() -> Result<Secrets, String> {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.trim().is_empty());
    let ci = crate::ci_host::Ci::from_environment();
    if let Some(problem) = ci.secrets_problem() {
        return Err(problem);
    }
    if ci.secrets_come_from_variables() || env("ZEN_PUBLISHER_KEY").is_some() {
        let missing = |name: &str| format!("{name} is not set: {}", ci.where_variables_live());
        env("ZEN_PUBLISHER_KEY").ok_or_else(|| missing("ZEN_PUBLISHER_KEY"))?;
        return Ok(Secrets {
            server: env("ZEN_RELEASES_SERVER").ok_or_else(|| missing("ZEN_RELEASES_SERVER"))?,
            token: env("ZEN_RELEASES_TOKEN").ok_or_else(|| missing("ZEN_RELEASES_TOKEN"))?,
            key: None,
        });
    }
    let (server, token) = release_settings()?;
    Ok(Secrets { server, token, key: Some(config_dir()?.join("publisher.key")) })
}

pub(crate) fn release_settings() -> Result<(String, String), String> {
    let path = config_dir()?.join("releases.env");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
            .ok_or(format!("{key} is missing in {}", path.display()))
    };
    Ok((value("ZEN_RELEASES_SERVER")?, value("ZEN_RELEASES_TOKEN")?))
}

pub(crate) fn gate() -> Result<(), String> {
    check_source()?;
    println!("==> Quality gate (cargo xtask ci)");
    crate::gate_memo::run(false).map_err(|error| format!("{error}; nothing was released"))
}

pub(crate) fn check_source() -> Result<(), String> {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .map(|output| (output.status.success(), String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    };
    let (_, branch) =
        git(&["rev-parse", "--abbrev-ref", "HEAD"]).map_err(|error| format!("cannot run git: {error}"))?;
    let (_, changes) = git(&["status", "--porcelain", "--untracked-files=no"]).map_err(|error| error.to_string())?;
    let _ = git(&["fetch", "--quiet", "origin", "main"]);
    let pushed = git(&["merge-base", "--is-ancestor", "HEAD", "origin/main"]).is_ok_and(|(ok, _)| ok);
    let any_branch = std::env::var("ZEN_RELEASE_ANY_BRANCH").is_ok_and(|value| value == "1");
    match source_problem(&branch, !changes.is_empty(), pushed, any_branch) {
        Some(problem) => Err(format!("{problem}; nothing was released")),
        None => Ok(()),
    }
}

fn source_problem(branch: &str, dirty: bool, pushed: bool, any_branch: bool) -> Option<String> {
    if dirty {
        return Some("there are uncommitted changes; commit them first".to_owned());
    }
    if any_branch {
        return None;
    }
    if branch != "main" && branch != "HEAD" {
        return Some(format!(
            "releases are built from main, not `{branch}` (ZEN_RELEASE_ANY_BRANCH=1 in an emergency)"
        ));
    }
    if !pushed {
        return Some("this commit is not on origin/main yet; push or merge it first".to_owned());
    }
    None
}

struct BundleSpec<'a> {
    name: &'a str,
    identifier: String,
    executable: &'a str,
    tray_only: bool,
    colors: ([u8; 3], [u8; 3]),
}

const COPYRIGHT: &str = "The ifzenelse team and the Zen IDE contributors. MIT OR Apache-2.0. \
Zen IDE is an independent project and is not affiliated with, sponsored by or endorsed by any other IDE vendor. \
Oracle, Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates. \
Other names may be trademarks of their respective owners. \
Kotlin is a registered trademark of the Kotlin Foundation.";

fn info_plist(spec: &BundleSpec<'_>, version: &str) -> String {
    let BundleSpec { name, identifier, executable, tray_only, .. } = spec;
    let ui_element = if *tray_only { "\n\t<key>LSUIElement</key><true/>" } else { "" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDisplayName</key><string>{name}</string>
	<key>CFBundleName</key><string>{name}</string>
	<key>CFBundleIdentifier</key><string>{identifier}</string>
	<key>CFBundleExecutable</key><string>{executable}</string>
	<key>CFBundleIconFile</key><string>AppIcon</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>{version}</string>
	<key>CFBundleVersion</key><string>{version}</string>
	<key>LSMinimumSystemVersion</key><string>12.0</string>{ui_element}
	<key>NSHighResolutionCapable</key><true/>
	<key>NSHumanReadableCopyright</key><string>{COPYRIGHT}</string>
</dict>
</plist>
"#
    )
}

fn build_app(spec: &BundleSpec<'_>, version: &str, binary: &Path, dist: &Path) -> Result<PathBuf, String> {
    let app = dist.join(format!("{}.app", spec.name));
    let _ = std::fs::remove_dir_all(&app);
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(contents.join("Resources")).map_err(|error| error.to_string())?;
    std::fs::write(contents.join("Info.plist"), info_plist(spec, version)).map_err(|error| error.to_string())?;
    std::fs::copy(binary, contents.join("MacOS").join(spec.executable)).map_err(|error| error.to_string())?;

    let iconset = dist.join("AppIcon.iconset");
    let _ = std::fs::remove_dir_all(&iconset);
    let id = spec.executable.strip_prefix("zen-").unwrap_or(spec.executable);
    let svg = PathBuf::from("assets/icons/products").join(format!("{id}.svg"));
    match std::fs::read(&svg) {
        Ok(bytes) => crate::icon::write_iconset_from_svg(&iconset, &bytes)?,
        Err(_) => crate::icon::write_iconset(&iconset, spec.colors)?,
    }
    run(
        Command::new("iconutil")
            .args(["-c", "icns", "-o"])
            .arg(contents.join("Resources").join("AppIcon.icns"))
            .arg(&iconset),
        "iconutil",
    )?;
    let _ = std::fs::remove_dir_all(&iconset);
    // WHY: all of this comes before codesign, whose seal covers only what is in the bundle by then.
    if !spec.tray_only {
        bundle_plugins(spec.executable, &contents.join("Resources").join("plugins"))?;
        let zen_cli = binary.with_file_name(ZEN_CLI_BINARY);
        std::fs::copy(&zen_cli, contents.join("MacOS").join(ZEN_CLI_IN_BUNDLE))
            .map_err(|error| format!("cannot copy {} into the bundle: {error}", zen_cli.display()))?;
        let host = binary.with_file_name(PTY_HOST_BINARY);
        std::fs::copy(&host, contents.join("MacOS").join(PTY_HOST_BINARY))
            .map_err(|error| format!("cannot copy {} into the bundle: {error}", host.display()))?;
    }
    let product = if spec.tray_only { "toolbox" } else { spec.executable };
    crate::notices::lay_out(
        &contents.join("Resources").join(crate::notices::LICENCES_FOLDER),
        spec.name,
        &crate::notices::product_shipped(product, None)?,
        true,
    )?;
    run(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app), "codesign")?;
    Ok(app)
}

const ZEN_CLI_BINARY: &str = "zen-cli";

const PTY_HOST_BINARY: &str = "zen-pty-host";

const ZEN_CLI_IN_BUNDLE: &str = "zen";

pub(crate) const BASE_PLUGINS: &[&str] = &[
    "markdown",
    "yaml",
    "json",
    "toml",
    "dockerfile",
    "gitlab-ci",
    "ignore",
    "env",
    "shell",
    "html",
    "css",
    "javascript",
    "typescript",
    "http",
    "xml",
    "ai-agents",
];

pub(crate) const PRODUCT_PLUGINS: &[(&str, &[&str])] = &[
    ("gogarden", &["sql"]),
    ("rustronin", &["sql"]),
    ("webwave", &["scss", "sql", "npm", "nodejs"]),
    ("jvmjizo", &["java", "kotlin", "maven", "gradle", "ant", "properties", "spring-boot"]),
    ("sqlscroll", &["sql"]),
    ("devopsdaruma", &["sql", "terraform", "kubernetes", "gitops"]),
];

pub(crate) const PENDING_PLUGINS: &[&str] = &[];

fn in_repository(id: &str) -> bool {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins").join(id).join("plugin.json").is_file()
}

pub(crate) fn bundled_plugins(product: &str) -> Result<Vec<&'static str>, String> {
    let (_, own) = PRODUCT_PLUGINS
        .iter()
        .find(|(id, _)| *id == product)
        .ok_or_else(|| format!("no plug-in bundle for `{product}` in PRODUCT_PLUGINS (xtask/src/release.rs)"))?;
    let mut plugins = BASE_PLUGINS.to_vec();
    for id in own.iter().filter(|id| !BASE_PLUGINS.contains(id)) {
        if PENDING_PLUGINS.contains(id) && !in_repository(id) {
            println!("note: `{id}` is not in the repository yet; {product} is built without it");
            continue;
        }
        plugins.push(id);
    }
    Ok(plugins)
}

pub(crate) fn bundle_command(args: &[String]) -> Result<(), String> {
    let [product, folder] = args else {
        return Err("usage: cargo xtask bundle-plugins <product> <folder>   (gogarden, rustronin, webwave, jvmjizo, sqlscroll or devopsdaruma)"
            .to_owned());
    };
    bundled_plugins(product)?;
    let folder = Path::new(folder);
    if folder.exists() {
        std::fs::remove_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    bundle_plugins(product, folder)?;
    println!("\nRun the IDE with ZEN_BUNDLED_PLUGINS={} to load them.", folder.display());
    Ok(())
}

pub(crate) fn bundle_plugins(product: &str, folder: &Path) -> Result<(), String> {
    let bundled = bundled_plugins(product)?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut args = vec!["build", "--release", "--locked", "--target", "wasm32-unknown-unknown"];
    let packages: Vec<String> = bundled.iter().map(|id| format!("zen-plugin-{id}")).collect();
    for package in &packages {
        args.extend(["--package", package.as_str()]);
    }
    println!("==> Building the bundled plug-ins");
    run(Command::new(&cargo).args(&args), "cargo build (plug-ins)")?;
    for id in bundled {
        let installed = bundled_manifest(Path::new("."), id)?;
        let target = folder.join(id);
        std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        let module = crate::bench::target_dir(&cargo)?
            .join("wasm32-unknown-unknown/release")
            .join(format!("zen_plugin_{}.wasm", id.replace('-', "_")));
        std::fs::copy(&module, target.join("plugin.wasm")).map_err(|error| format!("{}: {error}", module.display()))?;
        crate::notices::add_to_module(&target.join("plugin.wasm"), id)?;
        std::fs::write(target.join("plugin.json"), serde_json::to_vec_pretty(&installed).unwrap_or_default())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn bundled_manifest(root: &Path, id: &str) -> Result<serde_json::Value, String> {
    let source = root.join("plugins").join(id);
    let info: PluginInfo = serde_json::from_slice(
        &std::fs::read(source.join("plugin.json")).map_err(|error| format!("plugins/{id}/plugin.json: {error}"))?,
    )
    .map_err(|error| format!("plugins/{id}/plugin.json: {error}"))?;
    let manifest = std::fs::read_to_string(source.join("Cargo.toml")).map_err(|error| error.to_string())?;
    let version = manifest
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = \"")?.strip_suffix('"').map(str::to_owned))
        .ok_or_else(|| format!("plugins/{id}/Cargo.toml has no version"))?;
    let notes = crate::changelog::notes(&root.join(crate::changelog::plugin_path(id)), &version)?;
    Ok(serde_json::json!({
        "id": id,
        "name": info.name,
        "description": info.description,
        "version": version,
        "extensions": info.extensions,
        "names": info.names,
        "hashbangs": info.hashbangs,
        "contents": info.contents,
        "languageServers": info.language_servers,
        "fileTemplates": info.file_templates,
        "liveTemplates": info.live_templates,
        "serverPlugins": info.server_plugins,
        "requires": info.requires,
        "project": info.project,
        "databaseDrivers": info.database_drivers,
        "projectGenerators": info.project_generators,
        "tests": info.tests,
        "codeStyles": info.code_styles,
        "terminalAgents": info.terminal_agents,
        "runs": info.runs,
        "personOnly": info.person_only,
        "targetFiles": info.target_files,
        "toolWindows": info.tool_windows,
        "index": info.index,
        "schemas": info.schemas,
        "notes": notes.trim(),
    }))
}

fn build_dmg(version: &str, app: &Path, dist: &Path, arch: &str) -> Result<PathBuf, String> {
    let staging = dist.join("dmg");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    run(Command::new("cp").arg("-R").arg(app).arg(&staging), "cp")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", staging.join("Applications")).map_err(|error| error.to_string())?;
    #[cfg(not(unix))]
    run(Command::new("ln").arg("-s").arg("/Applications").arg(staging.join("Applications")), "ln")?;
    let dmg = dist.join(format!("Zen-Toolbox-{version}-macos-{arch}.dmg"));
    let _ = std::fs::remove_file(&dmg);
    run(
        Command::new("hdiutil")
            .args(["create", "-quiet", "-volname", "Zen Toolbox", "-fs", "HFS+", "-format", "UDZO", "-srcfolder"])
            .arg(&staging)
            .arg(&dmg),
        "hdiutil",
    )?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dmg)
}

pub(crate) fn notes(given: Option<&str>, changelog: &Path, version: &str) -> Result<String, String> {
    if let Some(notes) = given {
        return Ok(notes.to_owned());
    }
    let notes = crate::changelog::notes(changelog, version)?;
    println!("==> Release notes from {}, section `## {version}`", changelog.display());
    Ok(notes)
}

pub(crate) fn toolbox(args: &[String]) -> Result<(), String> {
    let (version, given) = match args {
        [version] => (version.as_str(), None),
        [version, notes] => (version.as_str(), Some(notes.as_str())),
        _ => return Err("usage: cargo xtask release-toolbox <version> [notes]".to_owned()),
    };
    let out = crate::release_flow::default_out("toolbox", version)?;
    build_toolbox(version, given, &out, false)?;
    crate::release_flow::publish_dir(&out, false, None)
}

pub(crate) const TRIAL_NOTES: &str = "A trial build: not a release.";

pub(crate) const TRIAL_GATE: &str = "skipped";

pub(crate) fn gate_unless(trial: bool) -> Result<(), String> {
    if trial {
        println!("==> Trial build: the source check and the quality gate are skipped");
        return Ok(());
    }
    gate()
}

pub(crate) fn build_toolbox(version: &str, given: Option<&str>, out: &Path, trial: bool) -> Result<(), String> {
    let notes = notes(given, &crate::changelog::product_path("toolbox"), version)?;
    if !cfg!(target_os = "macos") {
        return Err("Zen Toolbox releases are built on macOS only for now".to_owned());
    }
    gate_unless(trial)?;
    let arch = std::env::consts::ARCH;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target = crate::bench::target_dir(&cargo)?;

    println!("==> Building Zen Toolbox {version} (release)");
    run(
        Command::new(&cargo)
            .args(["build", "--release", "--locked", "--package", "zen-toolbox-app"])
            .env("ZEN_BUILD_VERSION", version),
        "cargo build --release",
    )?;
    let binary = target.join("release/zen-toolbox");
    let dist = target.join("dist").join(format!("toolbox-{version}"));
    std::fs::create_dir_all(&dist).map_err(|error| error.to_string())?;
    crate::release_flow::clean_out(out)?;

    println!("==> Assembling Zen Toolbox.app");
    let spec = BundleSpec {
        name: "Zen Toolbox",
        identifier: "space.ifzenelse.toolbox".to_owned(),
        executable: "zen-toolbox",
        tray_only: true,
        colors: crate::icon::TOOLBOX,
    };
    let app = build_app(&spec, version, &binary, &dist)?;

    println!("==> Packing the update artifact");
    // WHY: both forms, because Toolbox 0.1.3 and older update from bin/zen-toolbox.
    let staging = dist.join("artifact");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("bin")).map_err(|error| error.to_string())?;
    std::fs::copy(app.join("Contents/MacOS/zen-toolbox"), staging.join("bin/zen-toolbox"))
        .map_err(|error| error.to_string())?;
    run(Command::new("cp").arg("-R").arg(&app).arg(&staging), "cp")?;
    let archive = out.join(format!("zen-toolbox-{version}-macos-{arch}.tar.gz"));
    run(
        Command::new("tar")
            .env("COPYFILE_DISABLE", "1")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .arg("bin")
            .arg("Zen Toolbox.app"),
        "tar",
    )?;

    println!("==> Building the installer");
    let dmg = build_dmg(version, &app, &dist, arch)?;
    let file = dmg.file_name().and_then(|name| name.to_str()).ok_or("bad file name")?.to_owned();
    let installer = out.join(&file);
    std::fs::rename(&dmg, &installer).map_err(|error| error.to_string())?;
    let digest = crate::release_flow::sha256(&installer)?;
    std::fs::write(out.join(format!("{file}.sha256")), format!("{digest}  {file}\n"))
        .map_err(|error| error.to_string())?;

    let mut record = crate::release_flow::BuildRecord::new("toolbox", "toolbox", "Zen Toolbox", version, notes);
    if trial {
        record.gate = TRIAL_GATE.to_owned();
    }
    crate::release_flow::finish(
        out,
        record,
        &[
            (archive, "update"),
            (installer.clone(), "installer"),
            (out.join(format!("{file}.sha256")), "installer-checksum"),
        ],
    )
}

pub(crate) const IDES: &[(&str, &str, &str)] = &[
    ("gogarden", "GoGarden", "go-garden"),
    ("rustronin", "RustRonin", "rust-ronin"),
    ("webwave", "WebWave", "web-wave"),
    ("jvmjizo", "JvmJizo", "jvm-jizo"),
    ("sqlscroll", "SqlScroll", "sql-scroll"),
    ("devopsdaruma", "DevopsDaruma", "devops-daruma"),
];

pub(crate) const UNRELEASED_IDES: &[(&str, &str, &str)] = &[];

pub(crate) fn ide(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("toolbox") => return toolbox(&args[1..]),
        Some("plugin") => return plugin(&args[1..]),
        _ => {}
    }
    let release = ides_arguments(args, "release")?;
    let out = crate::release_flow::default_out("ides", &release.version)?;
    build_ides(&release, &out)?;
    crate::release_flow::publish_all(&out, false, None)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct IdesRelease {
    pub(crate) version: String,
    pub(crate) allow_version: bool,
}

pub(crate) fn ides_arguments(args: &[String], command: &str) -> Result<IdesRelease, String> {
    let usage = format!(
        "usage: cargo xtask {command} ides <YY.M.N> [--allow-version]: every \
         IDE together (ADR-0146); `cargo xtask release-version` gives the next version"
    );
    if let Some(id) = args.first().filter(|id| UNRELEASED_IDES.iter().any(|ide| ide.0 == id.as_str())) {
        return Err(format!("`{id}` is not released yet: its release card moves it to release::IDES (ADR-0147)"));
    }
    if let Some(id) = args.first().filter(|id| IDES.iter().any(|ide| ide.0 == id.as_str())) {
        return Err(format!("`{id}`: the IDEs are released together under one version; {usage}"));
    }
    let [ides, version, rest @ ..] = args else { return Err(usage) };
    if ides != "ides" {
        return Err(usage);
    }
    let allow_version = match rest {
        [] => false,
        [flag] if flag == "--allow-version" => true,
        _ => return Err(usage),
    };
    Ok(IdesRelease { version: version.clone(), allow_version })
}

pub(crate) fn published_ide_versions() -> Result<Vec<String>, String> {
    let manifest = public_manifest()?;
    Ok(manifest["products"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|product| IDES.iter().any(|ide| product["id"] == ide.0))
        .flat_map(|product| product["releases"].as_array().into_iter().flatten())
        .filter_map(|release| release["version"].as_str().map(str::to_owned))
        .collect())
}

pub(crate) fn public_manifest() -> Result<serde_json::Value, String> {
    let url = std::env::var("ZEN_RELEASES")
        .unwrap_or_else(|_| "https://ide.ifzenelse.space/releases/manifest.json".to_owned());
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "30"])
        .arg(&url)
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("cannot read the published versions from {url}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("{url}: {error}"))
}

pub(crate) fn next_version() -> Result<(), String> {
    let (year, month) = crate::calendar::this_month()?;
    println!("{}", crate::calendar::next(year, month, &published_ide_versions()?));
    Ok(())
}

pub(crate) fn version_problem(
    version: &str,
    published: &[String],
    this_month: (u64, u64),
    allow: bool,
) -> Option<String> {
    let Some(calendar) = crate::calendar::parse(version) else {
        return Some(format!("`{version}` is not a calendar version YY.M.N such as 26.10.1 (ADR-0146)"));
    };
    let next = crate::calendar::next(this_month.0, this_month.1, published);
    if !allow && calendar != next {
        let month = if (calendar.year, calendar.month) == this_month { "the next number" } else { "this month" };
        return Some(format!(
            "{version} is not the IDEs' next version, {next} ({month}: YY.M of today, local time, and the \
             month's next number); `--allow-version` releases another on purpose"
        ));
    }
    let new = semver::Version::parse(version).ok()?;
    let newest = published.iter().filter_map(|published| semver::Version::parse(published).ok()).max()?;
    (new <= newest).then(|| format!("{version} is not greater than {newest}, an IDE version already published"))
}

pub(crate) fn build_ides_for(
    release: &IdesRelease,
    platforms: &[crate::native::Platform],
    out: &Path,
) -> Result<(), String> {
    let version = release.version.as_str();
    if platforms.contains(&crate::native::Platform::Macos) {
        build_ides(release, out)?;
    } else {
        let published = crate::native::published()?;
        for (id, _, _) in IDES {
            if let Some(problem) = crate::native::version_problem(id, version, platforms, &published) {
                return Err(format!("{problem}; nothing was built"));
            }
        }
        crate::release_flow::clean_out(out)?;
    }
    for &platform in platforms.iter().filter(|platform| **platform != crate::native::Platform::Macos) {
        for (id, _, _) in IDES {
            let product = crate::cross::product(id)?;
            if let Err(error) = crate::native::build(
                &product,
                version,
                platform,
                &crate::native::folder(out, id, version, platform),
                false,
            ) {
                let _ = std::fs::remove_dir_all(out);
                return Err(format!("{error}; no IDE was built (the IDEs are released together)"));
            }
        }
    }
    Ok(())
}

pub(crate) fn build_ides(release: &IdesRelease, out: &Path) -> Result<(), String> {
    let version = release.version.as_str();
    let published = published_ide_versions()?;
    if let Some(problem) = version_problem(version, &published, crate::calendar::this_month()?, release.allow_version) {
        return Err(format!("{problem}; nothing was built"));
    }
    crate::release_flow::clean_out(out)?;
    for (id, _, _) in IDES {
        if let Err(error) = build_ide(id, version, None, &out.join(format!("{id}-{version}")), false) {
            let _ = std::fs::remove_dir_all(out);
            return Err(format!("{error}; no IDE was built (the IDEs are released together)"));
        }
    }
    println!("\n==> Built every IDE at {version} into {}", out.display());
    Ok(())
}

fn ide_of(id: &str) -> Result<(&'static str, &'static str), String> {
    let &(_, name, package) = IDES.iter().find(|(product, _, _)| *product == id).ok_or_else(|| {
        format!(
            "unknown product `{id}`; releasable: toolbox, {}, plugin <id>",
            IDES.iter().map(|ide| ide.0).collect::<Vec<_>>().join(", ")
        )
    })?;
    Ok((name, package))
}

pub(crate) fn build_ide(id: &str, version: &str, given: Option<&str>, out: &Path, trial: bool) -> Result<(), String> {
    let (name, package) = ide_of(id)?;
    let notes = notes(given, &crate::changelog::product_path(id), version)?;
    bundled_plugins(id)?;
    if !cfg!(target_os = "macos") {
        return Err("releases are built on macOS only for now".to_owned());
    }
    gate_unless(trial)?;
    let arch = std::env::consts::ARCH;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target = crate::bench::target_dir(&cargo)?;

    println!("==> Building {name} {version} (release)");
    run(
        Command::new(&cargo)
            .args([
                "build",
                "--release",
                "--locked",
                "--package",
                package,
                "--package",
                "zen-cli",
                "--package",
                "zen-pty-host",
            ])
            .env("ZEN_BUILD_VERSION", version),
        "cargo build --release",
    )?;
    let dist = target.join("dist").join(format!("{id}-{version}"));
    crate::release_flow::clean_out(out)?;
    println!("==> Assembling {name}.app");
    let spec = BundleSpec {
        name,
        identifier: format!("space.ifzenelse.{id}"),
        executable: id,
        tray_only: false,
        colors: crate::icon::product_colors(id),
    };
    build_app(&spec, version, &target.join("release").join(id), &dist)?;
    let archive = out.join(format!("{id}-{version}-macos-{arch}.tar.gz"));
    run(
        Command::new("tar")
            .env("COPYFILE_DISABLE", "1")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&dist)
            .arg(format!("{name}.app")),
        "tar",
    )?;
    let mut record = crate::release_flow::BuildRecord::new("ide", id, name, version, notes);
    if trial {
        record.gate = TRIAL_GATE.to_owned();
    }
    crate::release_flow::finish(out, record, &[(archive, "update")])
}

#[derive(serde::Deserialize)]
pub(crate) struct PluginInfo {
    name: String,
    #[serde(default)]
    description: String,
    extensions: Vec<String>,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    hashbangs: Vec<String>,
    #[serde(default)]
    contents: Vec<serde_json::Value>,
    #[serde(default, rename = "languageServers")]
    language_servers: Vec<serde_json::Value>,
    #[serde(default, rename = "fileTemplates")]
    file_templates: serde_json::Map<String, serde_json::Value>,
    #[serde(default, rename = "liveTemplates")]
    live_templates: serde_json::Map<String, serde_json::Value>,
    #[serde(default, rename = "serverPlugins")]
    server_plugins: Vec<serde_json::Value>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    project: serde_json::Map<String, serde_json::Value>,
    #[serde(default, rename = "databaseDrivers")]
    database_drivers: Vec<serde_json::Value>,
    #[serde(default, rename = "projectGenerators")]
    project_generators: Vec<serde_json::Value>,
    #[serde(default)]
    tests: Vec<serde_json::Value>,
    #[serde(default, rename = "terminalAgents")]
    terminal_agents: Vec<serde_json::Value>,
    #[serde(default)]
    schemas: Vec<serde_json::Value>,
    #[serde(default)]
    runs: Vec<String>,
    #[serde(default, rename = "personOnly")]
    person_only: Vec<serde_json::Value>,
    #[serde(default, rename = "targetFiles")]
    target_files: Vec<String>,
    #[serde(default, rename = "toolWindows")]
    tool_windows: Vec<serde_json::Value>,
    #[serde(default)]
    index: Option<serde_json::Value>,
    #[serde(default, rename = "codeStyles")]
    code_styles: Vec<serde_json::Value>,
}

pub(crate) fn plugin(args: &[String]) -> Result<(), String> {
    let [id, version] = args else {
        return Err("usage: cargo xtask release plugin <id> <version>".to_owned());
    };
    let out = crate::release_flow::default_out(&format!("plugin-{id}"), version)?;
    build_plugin(id, version, &[crate::native::Platform::Macos], &out)?;
    crate::release_flow::publish_dir(&out, false, None)
}

pub(crate) fn plugin_publish_args(info: &PluginInfo) -> Vec<String> {
    fn json<T: serde::Serialize>(value: &T, empty: &str) -> String {
        serde_json::to_string(value).unwrap_or_else(|_| empty.to_owned())
    }
    let mut args = vec![
        "--name".to_owned(),
        info.name.clone(),
        "--description".to_owned(),
        info.description.clone(),
        format!("--extensions={}", info.extensions.join(",")),
        format!("--names={}", info.names.join(",")),
        format!("--hashbangs={}", info.hashbangs.join(",")),
        format!("--contents={}", json(&info.contents, "[]")),
        format!("--language-servers={}", json(&info.language_servers, "[]")),
        format!("--file-templates={}", json(&info.file_templates, "{}")),
        format!("--live-templates={}", json(&info.live_templates, "{}")),
        format!("--server-plugins={}", json(&info.server_plugins, "[]")),
        format!("--requires={}", info.requires.join(",")),
        format!("--project={}", json(&info.project, "{}")),
    ];
    if !info.database_drivers.is_empty() {
        args.push(format!("--database-drivers={}", json(&info.database_drivers, "[]")));
    }
    if !info.project_generators.is_empty() {
        args.push(format!("--project-generators={}", json(&info.project_generators, "[]")));
    }
    if !info.tests.is_empty() {
        args.push(format!("--tests={}", json(&info.tests, "[]")));
    }
    if !info.code_styles.is_empty() {
        args.push(format!("--code-styles={}", json(&info.code_styles, "[]")));
    }
    if !info.terminal_agents.is_empty() {
        args.push(format!("--terminal-agents={}", json(&info.terminal_agents, "[]")));
    }
    if !info.schemas.is_empty() {
        args.push(format!("--schemas={}", json(&info.schemas, "[]")));
    }
    if !info.runs.is_empty() {
        args.push(format!("--runs={}", info.runs.join(",")));
    }
    if !info.person_only.is_empty() {
        args.push(format!("--person-only={}", json(&info.person_only, "[]")));
    }
    if !info.target_files.is_empty() {
        args.push(format!("--target-files={}", info.target_files.join(",")));
    }
    if let Some(index) = &info.index {
        args.push(format!("--index={}", json(index, "{}")));
    }
    if !info.tool_windows.is_empty() {
        args.push(format!("--tool-windows={}", json(&info.tool_windows, "[]")));
    }
    args
}

pub(crate) fn build_plugin(
    id: &str,
    version: &str,
    platforms: &[crate::native::Platform],
    out: &Path,
) -> Result<(), String> {
    let folder = PathBuf::from("plugins").join(id);
    let mut info: PluginInfo = serde_json::from_slice(
        &std::fs::read(folder.join("plugin.json"))
            .map_err(|error| format!("cannot read plugins/{id}/plugin.json: {error}"))?,
    )
    .map_err(|error| format!("plugins/{id}/plugin.json: {error}"))?;
    let manifest = std::fs::read_to_string(folder.join("Cargo.toml")).map_err(|error| error.to_string())?;
    if !manifest.lines().any(|line| line.trim() == format!("version = \"{version}\"")) {
        return Err(format!("plugins/{id}/Cargo.toml does not have version = \"{version}\"; raise it first"));
    }
    let drivers = driver_crates(&info.database_drivers, Path::new("."));
    if drivers.is_empty() && platforms != [crate::native::Platform::Macos] {
        println!("==> The {} plug-in has no driver programs: --platform changes nothing", info.name);
    }
    let platforms: &[crate::native::Platform] = if drivers.is_empty() { &[] } else { platforms };
    for platform in platforms {
        if let Some(problem) = crate::native::tools_problem(*platform) {
            return Err(problem);
        }
    }
    let notes = crate::changelog::notes(&crate::changelog::plugin_path(id), version)?;
    gate()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target = crate::bench::target_dir(&cargo)?;
    let package = format!("zen-plugin-{id}");

    println!("==> Building the {} plug-in {version} (WebAssembly)", info.name);
    run(
        Command::new(&cargo).args([
            "build",
            "--release",
            "--locked",
            "--target",
            "wasm32-unknown-unknown",
            "--package",
            &package,
        ]),
        "cargo build --target wasm32-unknown-unknown",
    )?;
    crate::release_flow::clean_out(out)?;
    let built = target.join("wasm32-unknown-unknown/release").join(format!("{}.wasm", package.replace('-', "_")));
    let module = out.join(format!("{package}-{version}.wasm"));
    std::fs::copy(&built, &module).map_err(|error| format!("{}: {error}", built.display()))?;
    crate::notices::add_to_module(&module, id)?;
    let mut files = vec![(module, "module")];
    let mut programs = Vec::new();
    for driver in &drivers {
        for platform in platforms {
            let names = ProgramNames::new(driver, version, *platform);
            println!("==> Building the driver program {} for {}", names.installed, names.platform);
            let path = build_driver_program(&cargo, &target, driver, *platform, &out.join(&names.published))?;
            let size = std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?.len();
            programs.push(BuiltProgram {
                driver: driver.clone(),
                sha256: crate::release_flow::sha256(&path)?,
                size,
                names,
            });
            files.push((path, "driver"));
        }
    }
    info.database_drivers = with_programs(&info.database_drivers, id, version, &programs);
    let mut record = crate::release_flow::BuildRecord::new("plugin", id, &info.name, version, notes);
    record.publish_args = plugin_publish_args(&info);
    crate::release_flow::finish(out, record, &files)
}

pub(crate) fn driver_crates(declarations: &[serde_json::Value], root: &Path) -> Vec<String> {
    declarations
        .iter()
        .filter_map(|declaration| declaration["id"].as_str())
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
        .filter(|id| root.join("drivers").join(id).join("Cargo.toml").is_file())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgramNames {
    pub(crate) platform: String,
    pub(crate) published: String,
    pub(crate) installed: String,
}

impl ProgramNames {
    pub(crate) fn new(driver: &str, version: &str, platform: crate::native::Platform) -> Self {
        let exe = if platform == crate::native::Platform::Windows { ".exe" } else { "" };
        let platform = platform.id();
        Self {
            published: format!("zen-db-{driver}-{version}-{platform}{exe}"),
            installed: format!("zen-db-{driver}{exe}"),
            platform,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuiltProgram {
    pub(crate) driver: String,
    pub(crate) names: ProgramNames,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

pub(crate) fn with_programs(
    declarations: &[serde_json::Value],
    plugin: &str,
    version: &str,
    programs: &[BuiltProgram],
) -> Vec<serde_json::Value> {
    let mut declarations = declarations.to_vec();
    for declaration in &mut declarations {
        let id = declaration["id"].as_str().unwrap_or_default().to_owned();
        for program in programs.iter().filter(|program| program.driver == id) {
            if !declaration["platforms"].is_object() {
                declaration["platforms"] = serde_json::json!({});
            }
            declaration["platforms"][&program.names.platform] = serde_json::json!({
                "url": format!("artifacts/plugin-{plugin}/{version}/{}", program.names.published),
                "sha256": program.sha256,
                "size": program.size,
                "file": program.names.installed,
            });
        }
    }
    declarations
}

fn build_driver_program(
    cargo: &str,
    target: &Path,
    driver: &str,
    platform: crate::native::Platform,
    to: &Path,
) -> Result<PathBuf, String> {
    let package = format!("zen-db-{driver}");
    let notices = crate::notices::driver_notices_file(driver, platform.target(), &target.join("notices"))?;
    let mut build = Command::new(cargo);
    build.env(crate::notices::DRIVER_NOTICES_ENV, &notices);
    let built = match platform {
        crate::native::Platform::Macos => {
            build.args(["build", "--release", "--locked", "--package", &package]);
            target.join("release").join(&package)
        }
        other => {
            build.args(crate::native::cargo_build(other, &package));
            if other == crate::native::Platform::Windows {
                build.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", crate::native::windows_linker());
            }
            let exe =
                if other == crate::native::Platform::Windows { format!("{package}.exe") } else { package.clone() };
            target.join(other.target().unwrap_or_default()).join("release").join(exe)
        }
    };
    run(&mut build, &format!("cargo build ({package}, {})", platform.id()))?;
    std::fs::copy(&built, to).map_err(|error| format!("{}: {error}", built.display()))?;
    if platform == crate::native::Platform::Macos {
        run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(to), "codesign")?;
        run(Command::new("codesign").args(["--verify", "--strict"]).arg(to), "codesign --verify")?;
    }
    Ok(to.to_owned())
}

pub(crate) fn notes_sync(args: &[String]) -> Result<(), String> {
    let [product] = args else {
        return Err("usage: cargo xtask release-notes-sync <product>   (toolbox, gogarden...)".to_owned());
    };
    let (server, token) = release_settings()?;
    let key = config_dir()?.join("publisher.key");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(
        Command::new(&cargo).args(["build", "--release", "--locked", "--package", "zen-releases"]),
        "cargo build (zen-releases)",
    )?;
    let work = PathBuf::from("target/dist/notes-sync");
    std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    for (version, notes) in crate::changelog::all_notes(&crate::changelog::product_path(product))? {
        let file = work.join(format!("{product}-{version}.md"));
        std::fs::write(&file, &notes).map_err(|error| error.to_string())?;
        run(
            Command::new("target/release/zen-releases")
                .args(["set-notes", "--server", &server, "--key"])
                .arg(&key)
                .args(["--product", product, "--version", &version])
                .arg(&file)
                .env("ZEN_RELEASES_TOKEN", &token),
            "zen-releases set-notes",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BASE_PLUGINS, BundleSpec, IDES, PENDING_PLUGINS, PRODUCT_PLUGINS, UNRELEASED_IDES, bundle_command,
        bundled_manifest, bundled_plugins, ides_arguments, info_plist, source_problem, version_problem,
    };

    #[test]
    fn the_ides_take_the_next_version_of_this_month() {
        let published: Vec<String> = ["0.38.49", "0.2.25", "26.10.1"].map(str::to_owned).to_vec();
        let october = (26, 10);
        let problem =
            |version: &str, published: &[String], month: (u64, u64)| version_problem(version, published, month, false);
        assert_eq!(problem("26.10.2", &published, october), None);
        assert!(problem("26.10.3", &published, october).unwrap().contains("26.10.2"), "a number skipped");
        assert!(problem("26.11.1", &published, october).unwrap().contains("this month"), "next month, too early");
        assert!(problem("26.12.1", &published, october).is_some());
        assert!(problem("27.10.1", &published, october).unwrap().contains("this month"), "a typo in the year");
        assert!(problem("62.10.1", &published, october).is_some(), "a typo in the year");
        assert!(problem("26.1.1", &published, october).is_some(), "the 10th month is not the 1st");
        assert!(problem("26.10.1", &published, october).unwrap().contains("26.10.2"), "published already");
        assert!(problem("0.39.0", &published, october).unwrap().contains("calendar"));
        assert!(problem("26.010.2", &published, october).unwrap().contains("calendar"));
        assert_eq!(problem("26.10.1", &["0.38.49".to_owned()], october), None, "the first calendar release");
        let december: Vec<String> = ["26.12.1", "26.12.2"].map(str::to_owned).to_vec();
        assert_eq!(problem("27.1.1", &december, (27, 1)), None);
        assert!(problem("26.12.3", &december, (27, 1)).unwrap().contains("this month"));
        assert!(problem("27.1.2", &december, (27, 1)).is_some());
        assert_eq!(problem("26.12.3", &december, (26, 12)), None);
        assert_eq!(version_problem("26.11.1", &published, october, true), None);
        assert!(version_problem("26.10.1", &published, october, true).unwrap().contains("not greater"));
        assert!(version_problem("26.9.9", &published, october, true).unwrap().contains("not greater"));

        let args = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let asked = ides_arguments(&args("ides 26.10.1"), "release").unwrap();
        assert_eq!((asked.version.as_str(), asked.allow_version), ("26.10.1", false));
        assert!(ides_arguments(&args("ides 26.10.1 --allow-version"), "release").unwrap().allow_version);
        assert!(ides_arguments(&args("ides 26.10.1 --channel beta"), "release").is_err());
        assert!(ides_arguments(&args("gogarden 26.10.1"), "release").unwrap_err().contains("together"));
        assert!(ides_arguments(&args("ides 26.10.1 --force"), "release").is_err());
    }

    #[test]
    fn every_bundle_carries_the_non_affiliation_notice() {
        for (name, tray_only) in [("GoGarden", false), ("Zen Toolbox", true)] {
            let spec = BundleSpec {
                name,
                identifier: "space.ifzenelse.test".to_owned(),
                executable: "test",
                tray_only,
                colors: ([0, 0, 0], [0, 0, 0]),
            };
            let plist = info_plist(&spec, "1.2.3");
            let copyright = plist
                .split("<key>NSHumanReadableCopyright</key><string>")
                .nth(1)
                .and_then(|rest| rest.split("</string>").next())
                .unwrap();
            assert_eq!(
                copyright,
                "The ifzenelse team and the Zen IDE contributors. MIT OR Apache-2.0. Zen IDE is an independent \
                 project and is not affiliated with, sponsored by or endorsed by any other IDE vendor. Oracle, Java, \
                 MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates. Other names may be \
                 trademarks of their respective owners. Kotlin is a registered trademark of the Kotlin Foundation."
            );
            assert!(!copyright.contains(['<', '>', '&']));
        }
        let trademarks = include_str!("../../TRADEMARKS.md").split_whitespace().collect::<Vec<_>>().join(" ");
        for words in [
            "Zen IDE is an independent project",
            "not affiliated with, sponsored by or endorsed by",
            "Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates",
            "is a registered trademark of the Kotlin Foundation",
        ] {
            assert!(trademarks.contains(words), "TRADEMARKS.md no longer says `{words}`");
        }
    }

    #[test]
    fn a_plugin_lists_the_driver_programs_built_for_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("drivers/duckdb")).unwrap();
        std::fs::write(root.path().join("drivers/duckdb/Cargo.toml"), "[package]\n").unwrap();
        let declarations = vec![
            serde_json::json!({ "id": "duckdb", "name": "DuckDB", "platforms": {
                "linux-aarch64": { "url": "https://example.org/zen-db-duckdb", "sha256": "00" } } }),
            serde_json::json!({ "id": "other", "name": "Other", "platforms": {} }),
            serde_json::json!({ "id": "../duckdb", "name": "Escape" }),
        ];
        assert_eq!(super::driver_crates(&declarations, root.path()), ["duckdb"], "only drivers with a crate here");

        let mac = super::ProgramNames::new("duckdb", "0.1.0", crate::native::Platform::Macos);
        assert_eq!(mac.platform, format!("macos-{}", std::env::consts::ARCH));
        assert_eq!(mac.published, format!("zen-db-duckdb-0.1.0-macos-{}", std::env::consts::ARCH));
        assert_eq!(mac.installed, "zen-db-duckdb");
        let windows = super::ProgramNames::new("duckdb", "0.1.0", crate::native::Platform::Windows);
        assert_eq!(
            (windows.published.as_str(), windows.installed.as_str()),
            ("zen-db-duckdb-0.1.0-windows-x86_64.exe", "zen-db-duckdb.exe")
        );
        assert_eq!(
            crate::release_ci::program_platform(&windows.published),
            Some("windows-x86_64"),
            "staging reads the platform back from the name"
        );

        let built =
            super::BuiltProgram { driver: "duckdb".to_owned(), names: windows, sha256: "ab".to_owned(), size: 7 };
        let listed = super::with_programs(&declarations, "duckdb", "0.1.0", &[built]);
        let platforms = &listed[0]["platforms"];
        assert_eq!(
            platforms["windows-x86_64"],
            serde_json::json!({
                "url": "artifacts/plugin-duckdb/0.1.0/zen-db-duckdb-0.1.0-windows-x86_64.exe",
                "sha256": "ab",
                "size": 7,
                "file": "zen-db-duckdb.exe",
            })
        );
        assert_eq!(platforms["linux-aarch64"]["url"], "https://example.org/zen-db-duckdb", "declared ones stay");
        assert_eq!(listed[1], declarations[1], "another driver is untouched");
    }

    #[test]
    fn releases_come_from_pushed_main() {
        assert_eq!(source_problem("main", false, true, false), None);
        assert!(source_problem("feat/x", false, true, false).unwrap().contains("from main"));
        assert!(source_problem("main", false, false, false).unwrap().contains("origin/main"));
        assert_eq!(source_problem("HEAD", false, true, false), None);
        assert!(source_problem("HEAD", false, false, false).unwrap().contains("origin/main"));
        assert!(source_problem("main", true, true, false).unwrap().contains("uncommitted"));
        assert_eq!(source_problem("hotfix", false, false, true), None);
        assert!(source_problem("hotfix", true, false, true).is_some());
    }

    #[test]
    fn the_released_ides_are_all_six_apps() {
        let released: Vec<&str> = IDES.iter().map(|ide| ide.0).collect();
        assert_eq!(
            released,
            ["gogarden", "rustronin", "webwave", "jvmjizo", "sqlscroll", "devopsdaruma"],
            "JvmJizo from 26.10.2, SqlScroll and DevopsDaruma from 26.11.8"
        );
        assert!(UNRELEASED_IDES.is_empty(), "every IDE app is released");
        let apps = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps");
        for (id, _, package) in IDES {
            assert!(apps.join(package).join("Cargo.toml").is_file(), "{id}: apps/{package}");
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join(crate::changelog::product_path(id))
                    .is_file(),
                "{id} has its changelog"
            );
        }
        let args = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let refused = ides_arguments(&args("jvmjizo 26.10.2"), "release").unwrap_err();
        assert!(refused.contains("together"), "released with the others, never alone: {refused}");
    }

    #[test]
    fn devopsdaruma_is_released_with_the_devops_plugins_of_the_base_set() {
        assert!(IDES.iter().any(|ide| ide.0 == "devopsdaruma"), "released from 26.11.8");
        let bundled = bundled_plugins("devopsdaruma").unwrap();
        for id in ["dockerfile", "yaml", "shell", "gitlab-ci", "env", "ignore", "toml", "json"] {
            assert!(bundled.contains(&id), "devopsdaruma bundles `{id}`");
        }
        assert!(bundled.contains(&"sql"), "its Database workspace is on, so it ships SQL");
        assert!(bundled.contains(&"terraform"), "DevopsDaruma comes with Terraform");
        assert!(bundled.contains(&"kubernetes"), "and with Kubernetes");
        assert!(bundled.contains(&"gitops"), "and with GitOps");
        let main = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/devops-daruma/src/main.rs"),
        )
        .unwrap();
        assert!(main.contains("optional: Vec::new()"), "no bundled plug-in is off: Docker, YAML and shell are on");
    }

    #[test]
    fn every_ide_runs_the_shared_hand_off_test() {
        let apps = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps");
        for (id, _, package) in IDES.iter().chain(UNRELEASED_IDES) {
            let tests = std::fs::read_to_string(apps.join(package).join("tests/command_line.rs")).unwrap_or_default();
            assert!(
                tests.contains(
                    "zen_testkit::handoff::a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide"
                ),
                "apps/{package}/tests/command_line.rs ({id}) does not run zen-testkit's hand-off check"
            );
        }
    }

    #[test]
    fn every_ide_ships_the_base_set_first_then_its_own() {
        for (id, _, _) in IDES.iter().chain(UNRELEASED_IDES) {
            let bundled = bundled_plugins(id).unwrap();
            assert_eq!(&bundled[..BASE_PLUGINS.len()], BASE_PLUGINS, "{id}");
            let mut unique = bundled.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), bundled.len(), "{id} bundles a plug-in twice");
        }
        assert!(bundled_plugins("toolbox").is_err());
        assert!(bundled_plugins("nope").unwrap_err().contains("PRODUCT_PLUGINS"));
    }

    #[test]
    fn every_ide_with_database_support_on_ships_the_sql_plugin() {
        let apps = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps");
        for (id, _, package) in IDES.iter().chain(UNRELEASED_IDES) {
            let main = std::fs::read_to_string(apps.join(package).join("src/main.rs")).unwrap();
            let on = main.contains("BuiltinModule::on(&zen_workbench::database::MODULE)");
            assert_eq!(bundled_plugins(id).unwrap().contains(&"sql"), on, "{id}");
        }
    }

    #[test]
    fn bundle_plugins_refuses_a_product_without_a_bundle_before_touching_the_folder() {
        let folder = std::env::temp_dir().join(format!("zen-xtask-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let args = |product: &str| [product.to_owned(), folder.display().to_string()];
        assert!(bundle_command(&args("toolbox")).unwrap_err().contains("PRODUCT_PLUGINS"));
        assert!(bundle_command(&args("nope")).is_err());
        assert!(bundle_command(&["gogarden".to_owned()]).unwrap_err().starts_with("usage:"));
        assert!(folder.is_dir());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_bundled_manifest_carries_the_description_and_the_notes_of_its_version() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = bundled_manifest(&root, "markdown").unwrap();
        let version = manifest["version"].as_str().unwrap();
        let notes = crate::changelog::notes(&root.join(crate::changelog::plugin_path("markdown")), version).unwrap();
        assert_eq!(manifest["notes"], notes.trim());
        assert!(!manifest["notes"].as_str().unwrap().is_empty());
        assert!(!manifest["description"].as_str().unwrap().is_empty());
        assert!(bundled_manifest(&root, "no-such-plugin").is_err());
    }

    #[test]
    fn a_bundled_manifest_keeps_the_programs_and_the_terminal_agents_a_plug_in_declares() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let source = |id: &str| -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(root.join("plugins").join(id).join("plugin.json")).unwrap()).unwrap()
        };
        assert_eq!(bundled_manifest(&root, "dockerfile").unwrap()["runs"], serde_json::json!(["docker"]));
        let agents = source("ai-agents")["terminalAgents"].clone();
        assert!(agents.as_array().is_some_and(|agents| !agents.is_empty()));
        assert_eq!(bundled_manifest(&root, "ai-agents").unwrap()["terminalAgents"], agents);
        let windows = source("kubernetes")["toolWindows"].clone();
        assert!(windows[0]["choices"].as_array().is_some_and(|choices| choices.len() == 2), "{windows}");
        assert_eq!(bundled_manifest(&root, "kubernetes").unwrap()["toolWindows"], windows, "choices and confirm kept");
        assert_eq!(bundled_manifest(&root, "gitops").unwrap()["index"], source("gitops")["index"], "the index kept");
    }

    #[test]
    fn the_commands_only_a_person_starts_reach_the_bundle_and_the_catalog() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = bundled_manifest(&root, "terraform").unwrap();
        assert_eq!(manifest["runs"], serde_json::json!(["terraform"]));
        let rules = manifest["personOnly"].as_array().unwrap();
        assert_eq!(rules[0]["program"], "terraform");
        for subcommand in ["apply", "destroy", "import", "state"] {
            assert!(
                rules[0]["subcommands"].as_array().unwrap().contains(&serde_json::json!(subcommand)),
                "{subcommand}"
            );
        }
        let info: super::PluginInfo =
            serde_json::from_slice(&std::fs::read(root.join("plugins/terraform/plugin.json")).unwrap()).unwrap();
        let args = super::plugin_publish_args(&info);
        let person_only = args.iter().find_map(|arg| arg.strip_prefix("--person-only=")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(person_only).unwrap();
        assert_eq!(parsed, manifest["personOnly"], "the catalog gets what the bundle gets");
    }

    #[test]
    fn the_kubernetes_target_files_and_person_only_commands_reach_the_bundle_and_the_catalog() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let manifest = bundled_manifest(&root, "kubernetes").unwrap();
        assert_eq!(manifest["targetFiles"], serde_json::json!(["*.yaml", "*.yml", "*.json"]));
        let info: super::PluginInfo =
            serde_json::from_slice(&std::fs::read(root.join("plugins/kubernetes/plugin.json")).unwrap()).unwrap();
        let args = super::plugin_publish_args(&info);
        assert!(args.contains(&"--target-files=*.yaml,*.yml,*.json".to_owned()), "{args:?}");
        assert!(args.iter().any(|arg| arg.starts_with("--person-only=") && arg.contains("\"apply\"")), "{args:?}");
    }

    #[test]
    fn a_bundled_manifest_keeps_every_field_of_the_plug_in_s_manifest() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for entry in std::fs::read_dir(root.join("plugins")).unwrap().flatten() {
            let Ok(bytes) = std::fs::read(entry.path().join("plugin.json")) else { continue };
            let source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let id = entry.file_name().to_string_lossy().into_owned();
            let manifest = bundled_manifest(&root, &id).unwrap();
            for (key, value) in source.as_object().unwrap() {
                assert_eq!(&manifest[key], value, "plugins/{id}/plugin.json: `{key}` is lost when it is bundled");
            }
        }
    }

    #[test]
    fn the_base_set_keeps_every_plugin_the_ides_shipped() {
        for id in [
            "markdown",
            "yaml",
            "json",
            "toml",
            "dockerfile",
            "gitlab-ci",
            "ignore",
            "env",
            "shell",
            "html",
            "css",
            "javascript",
            "typescript",
            "http",
        ] {
            assert!(BASE_PLUGINS.contains(&id), "the base set lost `{id}`");
        }
    }

    #[test]
    fn every_bundled_plugin_is_a_plugin_of_the_repository() {
        let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins");
        for (product, own) in PRODUCT_PLUGINS {
            assert!(IDES.iter().chain(UNRELEASED_IDES).any(|ide| ide.0 == *product), "`{product}` is not an IDE");
            for id in BASE_PLUGINS.iter().chain(own.iter()) {
                assert!(
                    plugins.join(id).join("plugin.json").is_file() || PENDING_PLUGINS.contains(id),
                    "{product}: no plugins/{id}"
                );
                assert!(
                    !own.contains(id) || !BASE_PLUGINS.contains(id),
                    "{product}: `{id}` is in the base set already"
                );
            }
        }
    }
}
