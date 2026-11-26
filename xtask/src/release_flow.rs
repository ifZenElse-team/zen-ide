use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::release::run;

const SCHEMA: u32 = 1;

const PROJECT: &str = "87707608";

const JOB: &str = "release-build";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuiltFile {
    pub(crate) name: String,
    pub(crate) role: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildRecord {
    pub(crate) schema: u32,
    pub(crate) kind: String,
    pub(crate) product: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) notes: String,
    pub(crate) commit: String,
    pub(crate) tree: String,
    pub(crate) rustc: String,
    pub(crate) platform: String,
    pub(crate) gate: String,
    pub(crate) host: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) publish_args: Vec<String>,
    pub(crate) files: Vec<BuiltFile>,
}

impl BuildRecord {
    pub(crate) fn new(kind: &str, product: &str, name: &str, version: &str, notes: String) -> Self {
        let git = |args: &[&str]| output("git", args).unwrap_or_default();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
        Self {
            schema: SCHEMA,
            kind: kind.to_owned(),
            product: product.to_owned(),
            name: name.to_owned(),
            version: version.to_owned(),
            notes,
            commit: git(&["rev-parse", "HEAD"]),
            tree: git(&["rev-parse", "HEAD^{tree}"]),
            rustc: output(&rustc, &["-V"]).unwrap_or_default(),
            platform: if kind == "plugin" { "wasm32".to_owned() } else { format!("macos-{}", std::env::consts::ARCH) },
            gate: "passed".to_owned(),
            host: output("hostname", &["-s"]).unwrap_or_default(),
            publish_args: Vec::new(),
            files: Vec::new(),
        }
    }

    pub(crate) fn file(&self, role: &str) -> Result<&BuiltFile, String> {
        self.files.iter().find(|file| file.role == role).ok_or_else(|| format!("the build has no {role} file"))
    }
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    let output = match Command::new("shasum").args(["-a", "256"]).arg(path).output() {
        Ok(output) => output,
        Err(_) => Command::new("sha256sum")
            .arg(path)
            .output()
            .map_err(|error| format!("cannot run shasum or sha256sum: {error}"))?,
    };
    let digest = String::from_utf8_lossy(&output.stdout).split_whitespace().next().unwrap_or_default().to_owned();
    if !output.status.success() || digest.len() != 64 {
        return Err(format!("cannot compute the SHA-256 of {}", path.display()));
    }
    Ok(digest)
}

pub(crate) fn default_out(name: &str, version: &str) -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    Ok(crate::bench::target_dir(&cargo)?.join("release-build").join(format!("{name}-{version}")))
}

pub(crate) fn clean_out(out: &Path) -> Result<(), String> {
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    }
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))
}

pub(crate) fn finish(out: &Path, mut record: BuildRecord, files: &[(PathBuf, &str)]) -> Result<(), String> {
    let changelog = out.join("changelog.json");
    std::fs::write(&changelog, crate::changelog::json()?).map_err(|error| error.to_string())?;
    let mut all = files.to_vec();
    all.push((changelog, "changelog"));
    for (path, role) in &all {
        if let Some(problem) = crate::notices::artefact_problem(path, role)? {
            return Err(format!("{problem} (docs/licences.md); nothing was recorded"));
        }
    }
    for (path, role) in all {
        let name = path.file_name().and_then(|name| name.to_str()).ok_or("bad file name")?.to_owned();
        let size = std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?.len();
        record.files.push(BuiltFile { name, role: role.to_owned(), size, sha256: sha256(&path)? });
    }
    save_record(out, &record)?;
    println!("\n==> Built {} {} into {}", record.name, record.version, out.display());
    for file in &record.files {
        println!("  {:<44} {:>12} bytes  {}", file.name, file.size, &file.sha256[..16]);
    }
    Ok(())
}

pub(crate) fn verified(dir: &Path) -> Result<BuildRecord, String> {
    let record = checked_files(dir)?;
    if record.gate != "passed" {
        return Err(format!("the build says the gate {}; it must have passed", record.gate));
    }
    Ok(record)
}

pub(crate) fn verified_part(dir: &Path) -> Result<BuildRecord, String> {
    let record = checked_files(dir)?;
    let deferred = crate::release::Gate::Deferred.recorded();
    if record.gate != deferred {
        return Err(format!(
            "{}: the build says the gate {}; a part of a release built in CI records it as {deferred}",
            dir.display(),
            record.gate
        ));
    }
    Ok(record)
}

pub(crate) fn save_record(dir: &Path, record: &BuildRecord) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    std::fs::write(dir.join("build.json"), json).map_err(|error| format!("{}: {error}", dir.display()))
}

fn checked_files(dir: &Path) -> Result<BuildRecord, String> {
    let path = dir.join("build.json");
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let record: BuildRecord = serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if record.schema != SCHEMA {
        return Err(format!("{}: schema {} is not {SCHEMA}; update the checkout", path.display(), record.schema));
    }
    if record.files.is_empty() {
        return Err(format!("{}: no files", path.display()));
    }
    for file in &record.files {
        if file.name.contains('/') || file.name.starts_with('.') {
            return Err(format!("{}: a file name may not be a path: {}", path.display(), file.name));
        }
        let at = dir.join(&file.name);
        let size = std::fs::metadata(&at).map_err(|error| format!("{}: {error}", at.display()))?.len();
        let digest = sha256(&at)?;
        if let Some(problem) = file_problem(file, size, &digest) {
            return Err(problem);
        }
    }
    Ok(record)
}

fn file_problem(file: &BuiltFile, size: u64, digest: &str) -> Option<String> {
    if size != file.size {
        return Some(format!("{}: {size} bytes, the build recorded {}", file.name, file.size));
    }
    if !digest.eq_ignore_ascii_case(&file.sha256) {
        return Some(format!("{}: SHA-256 {digest}, the build recorded {}", file.name, file.sha256));
    }
    None
}

fn commit_problem(record: &BuildRecord) -> Option<String> {
    let git = |args: &[&str]| Command::new("git").args(args).output().ok();
    let _ = git(&["fetch", "--quiet", "origin", "main"]);
    let any_branch = std::env::var("ZEN_RELEASE_ANY_BRANCH").is_ok_and(|value| value == "1");
    let on_main = git(&["merge-base", "--is-ancestor", &record.commit, "origin/main"])
        .is_some_and(|output| output.status.success());
    let tree = output("git", &["rev-parse", &format!("{}^{{tree}}", record.commit)]);
    source_verdict(record, on_main, tree.as_deref(), any_branch)
}

fn source_verdict(record: &BuildRecord, on_main: bool, tree: Option<&str>, any_branch: bool) -> Option<String> {
    match tree {
        None => return Some(format!("commit {} is not in this repository; fetch it first", record.commit)),
        Some(tree) if tree != record.tree => {
            return Some(format!("commit {} has tree {tree}, the build recorded {}", record.commit, record.tree));
        }
        Some(_) => {}
    }
    if !on_main && !any_branch {
        return Some(format!(
            "commit {} is not on origin/main; only main is released (ZEN_RELEASE_ANY_BRANCH=1 in an emergency)",
            record.commit
        ));
    }
    None
}

pub(crate) fn publish_dir(dir: &Path, dry_run: bool, stage: Option<&str>) -> Result<(), String> {
    let record = verified(dir)?;
    println!(
        "==> {} {} ({}), built from {} on {}",
        record.name,
        record.version,
        record.platform,
        &record.commit[..12.min(record.commit.len())],
        record.host
    );
    if let Some(problem) = commit_problem(&record).or_else(|| drivers_problem(&record)) {
        return Err(format!("{problem}; nothing was published"));
    }
    if dry_run {
        let key = crate::release::config_dir()?.join("publisher.key");
        let settings = crate::release::config_dir()?.join("releases.env");
        let present = |path: &Path| if path.is_file() { "present" } else { "MISSING" };
        println!("==> Dry run: the build and its commit are verified; nothing is signed or sent.");
        println!(
            "  signing key {} ({}), release server settings {} ({})",
            key.display(),
            present(&key),
            settings.display(),
            present(&settings)
        );
        for (what, file) in plan(&record) {
            println!("  would {what}: {file}");
        }
        return Ok(());
    }
    let crate::release::Secrets { server, token, key } = crate::release::secrets()?;
    let key: Vec<std::ffi::OsString> = key.map(|key| vec!["--key".into(), key.into_os_string()]).unwrap_or_default();
    let releases = zen_releases()?;
    let path = |role: &str| -> Result<PathBuf, String> { Ok(dir.join(&record.file(role)?.name)) };
    let staged: Vec<String> = stage.map(|stage| vec!["--stage".to_owned(), stage.to_owned()]).unwrap_or_default();
    match stage {
        Some(stage) => println!("==> Staging on {server} under {stage}"),
        None => println!("==> Publishing to {server}"),
    }
    match record.kind.as_str() {
        "plugin" => {
            let programs = driver_uploads(&record, dir);
            if !programs.is_empty() {
                run(
                    Command::new(&releases)
                        .args(["upload", "--server", &server, "--jobs", "2"])
                        .args(&programs)
                        .env("ZEN_RELEASES_TOKEN", &token),
                    "zen-releases upload",
                )?;
            }
            let notes = dir.join("notes.md");
            std::fs::write(&notes, &record.notes).map_err(|error| error.to_string())?;
            run(
                Command::new(&releases)
                    .args(["publish-plugin", "--server", &server])
                    .args(&key)
                    .arg("--notes")
                    .arg(&notes)
                    .args(["--id", &record.product, "--version", &record.version])
                    .args(&record.publish_args)
                    .args(&staged)
                    .arg(path("module")?)
                    .env("ZEN_RELEASES_TOKEN", &token),
                "zen-releases publish-plugin",
            )?;
        }
        "ide" | "toolbox" => {
            run(
                Command::new(&releases)
                    .args(["publish", "--server", &server])
                    .args(&key)
                    .args(["--product", &record.product, "--name", &record.name, "--version", &record.version])
                    .args(["--platform", &record.platform])
                    .arg(format!("--notes={}", record.notes))
                    .args(&staged)
                    .arg(path("update")?)
                    .env("ZEN_RELEASES_TOKEN", &token),
                "zen-releases publish",
            )?;
            let mut downloads = Vec::new();
            for role in ["installer", "installer-checksum", "package", "download"] {
                if let Ok(file) = record.file(role) {
                    downloads.push(format!(
                        "{}/{}/{}={}",
                        record.product,
                        record.version,
                        file.name,
                        path(role)?.display()
                    ));
                }
            }
            if !downloads.is_empty() {
                run(
                    Command::new(&releases)
                        .args(["upload", "--server", &server, "--jobs", "2"])
                        .args(&downloads)
                        .env("ZEN_RELEASES_TOKEN", &token),
                    "zen-releases upload",
                )?;
            }
        }
        other => return Err(format!("unknown kind of build `{other}`")),
    }
    println!("==> Publishing the changelogs for the website");
    run(
        Command::new(&releases)
            .args(["publish-changelog", "--server", &server])
            .args(&staged)
            .arg(path("changelog")?)
            .env("ZEN_RELEASES_TOKEN", &token),
        "zen-releases publish-changelog",
    )?;
    match stage {
        Some(stage) => println!("\n{} {} is staged under {stage}.", record.name, record.version),
        None => println!("\n{} {} is published.", record.name, record.version),
    }
    Ok(())
}

// WHY: built here, from this checkout, so that the program that reads the key is never one built elsewhere.
pub(crate) fn zen_releases() -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(
        Command::new(&cargo).args(["build", "--release", "--locked", "--package", "zen-releases"]),
        "cargo build (zen-releases)",
    )?;
    Ok(crate::bench::target_dir(&cargo)?.join("release/zen-releases"))
}

fn driver_uploads(record: &BuildRecord, dir: &Path) -> Vec<String> {
    record
        .files
        .iter()
        .filter(|file| file.role == "driver")
        .map(|file| {
            format!("plugin-{}/{}/{}={}", record.product, record.version, file.name, dir.join(&file.name).display())
        })
        .collect()
}

pub(crate) fn drivers_problem(record: &BuildRecord) -> Option<String> {
    if record.kind != "plugin" {
        return None;
    }
    let declarations: Vec<serde_json::Value> = record
        .publish_args
        .iter()
        .find_map(|arg| arg.strip_prefix("--database-drivers="))
        .map(|json| serde_json::from_str(json).unwrap_or_default())
        .unwrap_or_default();
    let listed: Vec<(String, String, u64)> = declarations
        .iter()
        .filter_map(|declaration| declaration["platforms"].as_object())
        .flat_map(|platforms| platforms.values())
        .filter_map(|program| {
            let url = program["url"].as_str()?;
            (!url.contains("://")).then(|| {
                (
                    url.to_owned(),
                    program["sha256"].as_str().unwrap_or_default().to_owned(),
                    program["size"].as_u64().unwrap_or(0),
                )
            })
        })
        .collect();
    let prefix = format!("artifacts/plugin-{}/{}/", record.product, record.version);
    let built: Vec<(String, String, u64)> = record
        .files
        .iter()
        .filter(|file| file.role == "driver")
        .map(|file| (format!("{prefix}{}", file.name), file.sha256.clone(), file.size))
        .collect();
    if let Some(missing) = built.iter().find(|program| !listed.contains(program)) {
        return Some(format!("the driver program {} is not in the catalog entry as built", missing.0));
    }
    listed.iter().find(|program| !built.contains(program)).map(|program| {
        format!("the catalog entry lists the driver program {}, which this build does not have", program.0)
    })
}

fn plan(record: &BuildRecord) -> Vec<(&'static str, String)> {
    let mut steps = Vec::new();
    for file in &record.files {
        let what = match (record.kind.as_str(), file.role.as_str()) {
            ("plugin", "module") => "sign the plug-in catalog with and upload",
            ("plugin", "driver") => "upload the database driver program",
            (_, "update") => "upload and sign the manifest with",
            (_, "installer" | "installer-checksum" | "package" | "download") => "upload for download",
            (_, "changelog") => "publish the website's changelogs from",
            _ => "keep",
        };
        steps.push((what, format!("{} ({} bytes)", file.name, file.size)));
    }
    steps
}

pub(crate) fn build_command(args: &[String]) -> Result<(), String> {
    let usage = "usage: cargo xtask release-build (ides <YY.M.N> [--allow-version] | toolbox <version> | plugin <id> <version>) [--platform macos|linux|windows|all] [--out <dir>]; or a trial build of one product, never published: release-build <ide|toolbox> <version> --trial [--platform ...] [--out <dir>]";
    let mut words = Vec::new();
    let mut out = None;
    let mut trial = false;
    let mut platform = "macos".to_owned();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(args.next().ok_or(usage)?)),
            "--platform" => platform = args.next().ok_or(usage)?.clone(),
            "--allow-version" => words.push(arg.clone()),
            "--trial" => trial = true,
            option if option.starts_with("--") => return Err(format!("unknown option `{option}`; {usage}")),
            word => words.push(word.to_owned()),
        }
    }
    let platforms = crate::native::platforms(&platform)?;
    if trial {
        return trial_build(&words, &platforms, out);
    }
    if words.first().is_some_and(|word| word == "ides" || crate::release::IDES.iter().any(|ide| ide.0 == word)) {
        let release = crate::release::ides_arguments(&words, "release-build")?;
        let version = release.version.clone();
        let out = match out {
            Some(out) => std::path::absolute(&out).map_err(|error| error.to_string())?.join(format!("ides-{version}")),
            None => default_out("ides", &version)?,
        };
        return crate::release::build_ides_for(&release, &platforms, &out);
    }
    let (product, version) = match words.as_slice() {
        [plugin, id, version] if plugin == "plugin" => (format!("plugin-{id}"), version.clone()),
        [product, version] => (product.clone(), version.clone()),
        _ => return Err(usage.to_owned()),
    };
    let out = match out {
        Some(out) => std::path::absolute(&out).map_err(|error| error.to_string())?.join(format!("{product}-{version}")),
        None => default_out(&product, &version)?,
    };
    match words.as_slice() {
        [_, id, version] => crate::release::build_plugin(id, version, &platforms, &out, crate::release::Gate::Here),
        [product, version] if product == "toolbox" => {
            let parent = out.parent().ok_or("no folder for the build")?.to_owned();
            if let Some(problem) =
                crate::native::version_problem("toolbox", version, &platforms, &native_published(&platforms)?)
            {
                return Err(format!("{problem}; nothing was built"));
            }
            let toolbox = crate::cross::product("toolbox")?;
            for platform in platforms {
                let folder = crate::native::folder(&parent, "toolbox", version, platform);
                match platform {
                    crate::native::Platform::Macos => {
                        crate::release::build_toolbox(version, None, &folder, crate::release::Gate::Here)?
                    }
                    other => crate::native::build(&toolbox, version, other, &folder, crate::release::Gate::Here)?,
                }
            }
            Ok(())
        }
        _ => Err(usage.to_owned()),
    }
}

pub(crate) fn trial_product(words: &[String]) -> Result<(crate::cross::Product, String), String> {
    let usage = "a trial build is of one product: release-build <ide|toolbox> <version> --trial";
    let [id, version] = words else { return Err(usage.to_owned()) };
    let product = crate::cross::product(id).map_err(|error| format!("{error}; {usage}"))?;
    if semver::Version::parse(version).is_err() {
        return Err(format!("`{version}` is not a version such as 0.0.0; {usage}"));
    }
    Ok((product, version.clone()))
}

fn trial_build(words: &[String], platforms: &[crate::native::Platform], out: Option<PathBuf>) -> Result<(), String> {
    let (product, version) = trial_product(words)?;
    let parent = match out {
        Some(out) => std::path::absolute(&out).map_err(|error| error.to_string())?,
        None => default_out("trial", &version)?,
    };
    println!(
        "==> A trial build of {} {version}: not a release. Skipped: the source check, the quality gate, the version \
         check against the published releases and the release notes. build.json records the gate as `{}`, so \
         release-publish refuses it.",
        product.name,
        crate::release::Gate::Trial.recorded()
    );
    for &platform in platforms {
        let folder = crate::native::folder(&parent, product.id, &version, platform);
        let notes = Some(crate::release::TRIAL_NOTES);
        match platform {
            crate::native::Platform::Macos if product.id == "toolbox" => {
                crate::release::build_toolbox(&version, notes, &folder, crate::release::Gate::Trial)?;
            }
            crate::native::Platform::Macos => {
                crate::release::build_ide(product.id, &version, notes, &folder, crate::release::Gate::Trial)?
            }
            other => crate::native::build(&product, &version, other, &folder, crate::release::Gate::Trial)?,
        }
    }
    Ok(())
}

pub(crate) fn native_published(
    platforms: &[crate::native::Platform],
) -> Result<Vec<(String, String, Vec<String>)>, String> {
    if platforms.contains(&crate::native::Platform::Macos) { Ok(Vec::new()) } else { crate::native::published() }
}

pub(crate) fn publish_command(args: &[String]) -> Result<(), String> {
    let usage =
        "usage: cargo xtask release-publish (--from <dir> | --pipeline <id> | --job <id>) [--dry-run] [--stage <name>]";
    let mut dry_run = false;
    let mut stage = None;
    let mut source = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--stage" if stage.is_none() => stage = Some(args.next().ok_or(usage)?.clone()),
            "--from" | "--pipeline" | "--job" if source.is_none() => {
                source = Some((arg.clone(), args.next().ok_or(usage)?.clone()));
            }
            other => return Err(format!("unknown option `{other}`; {usage}")),
        }
    }
    let dir = match source.ok_or(usage)? {
        (from, dir) if from == "--from" => PathBuf::from(dir),
        (kind, id) => fetch(&kind, &id)?,
    };
    publish_all(&dir, dry_run, stage.as_deref())
}

pub(crate) fn publish_all(dir: &Path, dry_run: bool, stage: Option<&str>) -> Result<(), String> {
    let folders = build_folders(dir)?;
    let mut records = Vec::new();
    for folder in &folders {
        let record = verified(folder)?;
        if let Some(problem) = commit_problem(&record) {
            return Err(format!("{}: {problem}; nothing was published", folder.display()));
        }
        records.push(record);
    }
    if let Some(problem) = set_problem(&records) {
        return Err(format!("{}: {problem}; nothing was published", dir.display()));
    }
    if records.iter().any(|record| record.kind != "plugin" && !crate::native::is_macos(&record.platform))
        && let Some(problem) = platform_problem(&records, &crate::native::published()?)
    {
        return Err(format!("{problem}; nothing was published"));
    }
    if records.len() > 1 {
        println!("==> {} builds of {}, all verified", records.len(), records[0].version);
    }
    let (folders, records) = macos_first(folders, records);
    let mut published: Vec<String> = Vec::new();
    for (folder, record) in folders.iter().zip(&records) {
        if let Err(error) = publish_dir(folder, dry_run, stage) {
            if published.is_empty() {
                return Err(error);
            }
            return Err(format!(
                "{error}\n\nPublished already: {} at {}; not yet: the others. Run the same command again: it \
                 publishes them all, the published ones once more at the same version, which is allowed.",
                published.join(", "),
                record.version
            ));
        }
        published.push(if crate::native::is_macos(&record.platform) {
            record.product.clone()
        } else {
            format!("{} ({})", record.product, record.platform)
        });
    }
    Ok(())
}

fn macos_first(folders: Vec<PathBuf>, records: Vec<BuildRecord>) -> (Vec<PathBuf>, Vec<BuildRecord>) {
    let mut order: Vec<(PathBuf, BuildRecord)> = folders.into_iter().zip(records).collect();
    order.sort_by_key(|(_, record)| !crate::native::is_macos(&record.platform));
    order.into_iter().unzip()
}

pub(crate) fn set_problem(records: &[BuildRecord]) -> Option<String> {
    let first = records.first()?;
    if records.iter().any(|record| record.kind != first.kind) {
        return Some(format!(
            "builds of {} and {} are not published together; give one with --from",
            first.kind,
            records.iter().find(|record| record.kind != first.kind).map_or("", |record| record.kind.as_str())
        ));
    }
    match first.kind.as_str() {
        "ide" => {
            let mut expected: Vec<&str> = crate::release::IDES.iter().map(|ide| ide.0).collect();
            expected.sort_unstable();
            let mut platforms: Vec<&str> = records.iter().map(|record| record.platform.as_str()).collect();
            platforms.sort_unstable();
            platforms.dedup();
            for platform in platforms {
                let mut ides: Vec<&str> = records
                    .iter()
                    .filter(|record| record.platform == platform)
                    .map(|record| record.product.as_str())
                    .collect();
                ides.sort_unstable();
                if ides != expected {
                    return Some(format!(
                        "the IDEs are published together (ADR-0146): exactly one {platform} build of each of {}, \
                         found {}",
                        expected.join(", "),
                        ides.join(", ")
                    ));
                }
            }
            if let Some(record) = records.iter().find(|record| crate::calendar::parse(&record.version).is_none()) {
                return Some(format!("{} {} is not a calendar version YY.M.N", record.product, record.version));
            }
        }
        _ => {
            let mut platforms: Vec<&str> = records.iter().map(|record| record.platform.as_str()).collect();
            platforms.sort_unstable();
            platforms.dedup();
            if platforms.len() != records.len() || records.iter().any(|record| record.product != first.product) {
                return Some("several builds; give one with --from".to_owned());
            }
        }
    }
    records
        .iter()
        .any(|record| (&record.version, &record.commit) != (&first.version, &first.commit))
        .then(|| "the builds are not one release: their versions or commits differ".to_owned())
}

fn platform_problem(records: &[BuildRecord], published: &[(String, String, Vec<String>)]) -> Option<String> {
    records.iter().filter(|record| !crate::native::is_macos(&record.platform)).find_map(|record| {
        let with_macos = records.iter().any(|other| {
            other.product == record.product
                && other.version == record.version
                && crate::native::is_macos(&other.platform)
        });
        if with_macos {
            return None;
        }
        crate::native::version_problem(&record.product, &record.version, &[crate::native::Platform::Linux], published)
    })
}

fn build_folders(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if dir.join("build.json").is_file() {
        return Ok(vec![dir.to_owned()]);
    }
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.join("build.json").is_file() {
                    found.push(path);
                } else {
                    folders.push(path);
                }
            }
        }
    }
    found.sort();
    if found.is_empty() {
        return Err(format!("no build.json under {}", dir.display()));
    }
    Ok(found)
}

fn fetch(kind: &str, id: &str) -> Result<PathBuf, String> {
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("`{id}` is not a pipeline or job id"));
    }
    let glab = |path: &str| -> Result<Vec<u8>, String> {
        let output = Command::new("glab")
            .args(["api", path])
            .output()
            .map_err(|error| format!("cannot run glab (brew install glab; glab auth login): {error}"))?;
        if !output.status.success() {
            return Err(format!("glab api {path}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(output.stdout)
    };
    let job = if kind == "--job" {
        id.to_owned()
    } else {
        let jobs: serde_json::Value =
            serde_json::from_slice(&glab(&format!("projects/{PROJECT}/pipelines/{id}/jobs"))?)
                .map_err(|error| error.to_string())?;
        pick_job(&jobs)?
    };
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let dir = crate::bench::target_dir(&cargo)?.join("release-fetch").join(format!("job-{job}"));
    clean_out(&dir)?;
    println!("==> Downloading the artifacts of job {job}");
    let zip = dir.join("artifacts.zip");
    std::fs::write(&zip, glab(&format!("projects/{PROJECT}/jobs/{job}/artifacts"))?)
        .map_err(|error| error.to_string())?;
    run(Command::new("unzip").args(["-q", "-o"]).arg(&zip).arg("-d").arg(&dir), "unzip")?;
    let _ = std::fs::remove_file(&zip);
    Ok(dir)
}

fn pick_job(jobs: &serde_json::Value) -> Result<String, String> {
    let jobs = jobs.as_array().ok_or("the pipeline's jobs are not a list")?;
    let job = jobs.iter().find(|job| job["name"] == JOB).ok_or_else(|| format!("the pipeline has no `{JOB}` job"))?;
    if job["status"] != "success" {
        return Err(format!("the `{JOB}` job is {}, not success", job["status"].as_str().unwrap_or("unknown")));
    }
    job["id"].as_u64().map(|id| id.to_string()).ok_or_else(|| "the job has no id".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> BuildRecord {
        BuildRecord {
            schema: SCHEMA,
            kind: "plugin".to_owned(),
            product: "json".to_owned(),
            name: "JSON".to_owned(),
            version: "0.1.2".to_owned(),
            notes: "- Better.".to_owned(),
            commit: "c0ffee".to_owned(),
            tree: "7ree".to_owned(),
            rustc: "rustc 1.99.1".to_owned(),
            platform: "wasm32".to_owned(),
            gate: "passed".to_owned(),
            host: "air".to_owned(),
            publish_args: vec!["--name".to_owned(), "JSON".to_owned()],
            files: Vec::new(),
        }
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> BuiltFile {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        BuiltFile {
            name: name.to_owned(),
            role: "module".to_owned(),
            size: bytes.len() as u64,
            sha256: sha256(&path).unwrap(),
        }
    }

    fn save(dir: &Path, record: &BuildRecord) {
        std::fs::write(dir.join("build.json"), serde_json::to_vec(record).unwrap()).unwrap();
    }

    #[test]
    fn a_build_is_verified_file_by_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = record();
        record.files.push(write(dir.path(), "zen-plugin-json-0.1.2.wasm", b"\0asm module"));
        save(dir.path(), &record);
        assert_eq!(verified(dir.path()).unwrap(), record);

        std::fs::write(dir.path().join("zen-plugin-json-0.1.2.wasm"), b"\0asm moduli").unwrap();
        assert!(verified(dir.path()).unwrap_err().contains("SHA-256"), "same size, other bytes");
        std::fs::write(dir.path().join("zen-plugin-json-0.1.2.wasm"), b"short").unwrap();
        assert!(verified(dir.path()).unwrap_err().contains("bytes"));

        record.files[0].name = "../escape".to_owned();
        save(dir.path(), &record);
        assert!(verified(dir.path()).unwrap_err().contains("may not be a path"));
        record.files.clear();
        save(dir.path(), &record);
        assert!(verified(dir.path()).unwrap_err().contains("no files"));
    }

    #[test]
    fn a_trial_build_is_of_one_product_and_is_never_published() {
        let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        let (product, version) = trial_product(&words(&["gogarden", "0.0.0"])).unwrap();
        assert_eq!((product.id, product.package, version.as_str()), ("gogarden", "go-garden", "0.0.0"));
        assert_eq!(trial_product(&words(&["toolbox", "0.3.20"])).unwrap().0.binary, "zen-toolbox");
        for refused in [&["ides", "26.11.1"][..], &["plugin", "json", "0.1.2"], &["gogarden"], &["gogarden", "next"]] {
            assert!(trial_product(&words(refused)).err().unwrap().contains("--trial"), "{refused:?}");
        }
        let dir = tempfile::tempdir().unwrap();
        let mut record = record();
        record.gate = crate::release::Gate::Trial.recorded().to_owned();
        record.files = vec![write(dir.path(), "a.wasm", b"module")];
        save(dir.path(), &record);
        assert!(verified(dir.path()).unwrap_err().contains("it must have passed"));
    }

    #[test]
    fn a_build_without_a_passed_gate_or_from_another_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = record();
        record.files.push(write(dir.path(), "m.wasm", b"x"));
        record.gate = "skipped".to_owned();
        save(dir.path(), &record);
        assert!(verified(dir.path()).unwrap_err().contains("gate"));
        record.gate = "passed".to_owned();
        record.schema = 2;
        save(dir.path(), &record);
        assert!(verified(dir.path()).unwrap_err().contains("schema"));
    }

    #[test]
    fn only_a_commit_of_main_with_the_recorded_tree_is_published() {
        let record = record();
        assert_eq!(source_verdict(&record, true, Some("7ree"), false), None);
        assert!(source_verdict(&record, false, Some("7ree"), false).unwrap().contains("not on origin/main"));
        assert_eq!(source_verdict(&record, false, Some("7ree"), true), None, "ZEN_RELEASE_ANY_BRANCH=1");
        assert!(source_verdict(&record, true, Some("other"), true).unwrap().contains("tree"), "never another tree");
        assert!(source_verdict(&record, true, None, true).unwrap().contains("fetch it"));
    }

    #[test]
    fn the_release_build_job_is_picked_from_its_pipeline() {
        let jobs = serde_json::json!([
            { "id": 11, "name": "checks", "status": "success" },
            { "id": 12, "name": "release-build", "status": "success" }
        ]);
        assert_eq!(pick_job(&jobs).unwrap(), "12");
        let failed = serde_json::json!([{ "id": 13, "name": "release-build", "status": "failed" }]);
        assert!(pick_job(&failed).unwrap_err().contains("failed"));
        assert!(pick_job(&serde_json::json!([])).unwrap_err().contains("no `release-build`"));
    }

    #[test]
    fn macos_is_published_before_the_other_platforms_of_a_version() {
        let on = |platform: &str| BuildRecord { platform: platform.to_owned(), ..record() };
        let folders = vec![PathBuf::from("linux"), PathBuf::from("macos")];
        let (folders, records) = macos_first(folders, vec![on("linux-x86_64"), on("macos-aarch64")]);
        assert_eq!(folders, [PathBuf::from("macos"), PathBuf::from("linux")]);
        assert_eq!(records[0].platform, "macos-aarch64");
    }

    #[test]
    fn the_ides_are_published_all_or_none() {
        let ide = |product: &str, version: &str| BuildRecord {
            kind: "ide".to_owned(),
            product: product.to_owned(),
            version: version.to_owned(),
            platform: "macos-aarch64".to_owned(),
            ..record()
        };
        let all = || crate::release::IDES.iter().map(|(id, _, _)| ide(id, "26.10.1")).collect::<Vec<_>>();
        assert_eq!(set_problem(&all()), None);
        assert!(set_problem(&all()[..1]).unwrap().contains("together"), "one IDE alone");
        assert!(set_problem(&all()[..2]).unwrap().contains("together"), "an IDE missing");
        let mut twice = all();
        let last = twice.len() - 1;
        twice[last] = ide("gogarden", "26.10.1");
        assert!(set_problem(&twice).is_some(), "an IDE twice");
        let mut other_version = all();
        other_version[1].version = "26.10.2".to_owned();
        assert!(set_problem(&other_version).unwrap().contains("not one release"));
        let mut other_commit = all();
        other_commit[0].commit = "decaf".to_owned();
        assert!(set_problem(&other_commit).is_some());
        let old: Vec<BuildRecord> =
            all().into_iter().map(|record| BuildRecord { version: "0.38.50".to_owned(), ..record }).collect();
        assert!(set_problem(&old).unwrap().contains("calendar"));
        let mut with_plugin = all();
        with_plugin.push(record());
        assert!(set_problem(&with_plugin).is_some());
        assert_eq!(set_problem(&[record()]), None, "a plug-in alone");
        assert!(set_problem(&[record(), record()]).unwrap().contains("several"));

        let on = |platform: &str| -> Vec<BuildRecord> {
            all().into_iter().map(|record| BuildRecord { platform: platform.to_owned(), ..record }).collect()
        };
        let mut everything = on("macos-aarch64");
        everything.extend(on("linux-x86_64"));
        everything.extend(on("windows-x86_64"));
        assert_eq!(set_problem(&everything), None);
        assert_eq!(set_problem(&on("linux-x86_64")), None, "a platform of its own");
        let mut partial = on("macos-aarch64");
        partial.extend(on("linux-x86_64").into_iter().take(2));
        assert!(set_problem(&partial).unwrap().contains("linux-x86_64"), "an IDE missing on Linux");
        let toolbox = |platform: &str| BuildRecord {
            kind: "toolbox".to_owned(),
            product: "toolbox".to_owned(),
            version: "0.3.15".to_owned(),
            platform: platform.to_owned(),
            ..record()
        };
        assert_eq!(set_problem(&[toolbox("macos-aarch64"), toolbox("linux-x86_64"), toolbox("windows-x86_64")]), None);
        assert!(set_problem(&[toolbox("linux-x86_64"), toolbox("linux-x86_64")]).is_some());
        let mut mixed = on("linux-x86_64");
        mixed.push(toolbox("linux-x86_64"));
        assert!(set_problem(&mixed).unwrap().contains("not published together"));
    }

    #[test]
    fn a_linux_or_windows_build_follows_a_macos_release() {
        let build = |product: &str, version: &str, platform: &str| BuildRecord {
            kind: "ide".to_owned(),
            product: product.to_owned(),
            version: version.to_owned(),
            platform: platform.to_owned(),
            ..record()
        };
        let published = vec![("rustronin".to_owned(), "26.10.1".to_owned(), vec!["macos-aarch64".to_owned()])];
        assert_eq!(platform_problem(&[build("rustronin", "26.10.1", "windows-x86_64")], &published), None);
        let unreleased = [build("rustronin", "26.10.2", "linux-x86_64")];
        assert!(platform_problem(&unreleased, &published).unwrap().contains("not published for macOS"));
        let together = [build("rustronin", "26.10.2", "macos-aarch64"), build("rustronin", "26.10.2", "linux-x86_64")];
        assert_eq!(platform_problem(&together, &published), None, "published with its macOS build");
    }

    #[test]
    fn the_build_folder_is_found_under_unpacked_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("release-build/gogarden-1.2.3");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("build.json"), "{}").unwrap();
        assert_eq!(build_folders(dir.path()).unwrap(), std::slice::from_ref(&build));
        assert_eq!(build_folders(&build).unwrap(), std::slice::from_ref(&build));
        let other = dir.path().join("release-build/toolbox-1.0.0");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("build.json"), "{}").unwrap();
        assert_eq!(build_folders(dir.path()).unwrap(), [build, other], "every build, as the IDEs' release has");
        assert!(build_folders(&dir.path().join("none")).is_err());
    }

    #[test]
    fn a_plugin_build_publishes_exactly_the_driver_programs_it_built() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = record();
        record.product = "duckdb".to_owned();
        record.version = "0.1.0".to_owned();
        let program = write(dir.path(), "zen-db-duckdb-0.1.0-macos-aarch64", b"program");
        let listed = |sha256: &str, size: u64| {
            vec![format!(
                "--database-drivers={}",
                serde_json::json!([{ "id": "duckdb", "platforms": {
                    "macos-aarch64": { "url": "artifacts/plugin-duckdb/0.1.0/zen-db-duckdb-0.1.0-macos-aarch64",
                        "sha256": sha256, "size": size, "file": "zen-db-duckdb" },
                    "linux-x86_64": { "url": "https://example.org/zen-db-duckdb", "sha256": "00" },
                } }])
            )]
        };
        record.files.push(BuiltFile { role: "driver".to_owned(), ..program.clone() });
        record.publish_args = listed(&program.sha256, program.size);
        assert_eq!(drivers_problem(&record), None);
        assert_eq!(
            driver_uploads(&record, Path::new("/b")),
            ["plugin-duckdb/0.1.0/zen-db-duckdb-0.1.0-macos-aarch64=/b/zen-db-duckdb-0.1.0-macos-aarch64"]
        );
        assert!(plan(&record).iter().any(|(what, file)| what.contains("driver") && file.contains("macos-aarch64")));

        record.publish_args = listed("ff", program.size);
        assert!(drivers_problem(&record).unwrap().contains("not in the catalog entry"));
        record.publish_args = listed(&program.sha256, program.size);
        record.files.retain(|file| file.role != "driver");
        assert!(drivers_problem(&record).unwrap().contains("does not have"), "listed but not built");
        record.publish_args.clear();
        assert_eq!(drivers_problem(&record), None, "a plug-in without drivers");
    }

    #[test]
    fn the_dry_run_plan_names_every_file() {
        let mut record = record();
        record.kind = "toolbox".to_owned();
        for (name, role) in [("a.tar.gz", "update"), ("a.dmg", "installer"), ("changelog.json", "changelog")] {
            record.files.push(BuiltFile {
                name: name.to_owned(),
                role: role.to_owned(),
                size: 1,
                sha256: String::new(),
            });
        }
        let plan = plan(&record);
        assert_eq!(plan.len(), 3);
        assert!(plan[0].0.contains("sign the manifest") && plan[1].0.contains("download"));
    }
}
