use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::release::run;
use crate::release_flow::BuildRecord;

pub(crate) const BUILD_DIR: &str = "release-build";

pub(crate) const PLAN_FILE: &str = "release-build/release-plan.json";

const STAGE_FILE: &str = "release-stage.json";

const VERIFY_FILE: &str = "release-verify.json";

pub(crate) const SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StagedProduct {
    pub(crate) kind: String,
    pub(crate) product: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedFile {
    pub(crate) name: String,
    pub(crate) role: String,
    pub(crate) url: String,
    pub(crate) link_type: String,
    pub(crate) platform: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageRecord {
    pub(crate) schema: u32,
    pub(crate) tag: String,
    pub(crate) version: String,
    pub(crate) stage: String,
    pub(crate) server: String,
    pub(crate) commit: String,
    pub(crate) products: Vec<StagedProduct>,
    pub(crate) files: Vec<StagedFile>,
    pub(crate) site: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuiltRelease {
    pub(crate) schema: u32,
    pub(crate) tag: String,
    pub(crate) version: String,
    pub(crate) groups: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) version: String,
    pub(crate) toolbox: Option<String>,
    pub(crate) plugins: Vec<(String, String)>,
}

pub(crate) fn command(args: &[String]) -> Result<(), String> {
    let ci = crate::ci_host::Ci::from_environment();
    let usage = format!(
        "usage: cargo xtask release-ci (plan | build | stage | verify | live | matrix | gate | join | part \
         <ide|toolbox|plugins> <macos|linux|windows|all>) [v<YY.M.N>] (the tag is {} in CI)",
        ci.tag_variable()
    );
    let own = || ci.tag().map(str::to_owned).ok_or_else(|| usage.clone());
    let (step, part, tag) = match args {
        [step, what, platform] if step == "part" => (step.as_str(), Some((what, platform)), own()?),
        [step, what, platform, tag] if step == "part" => (step.as_str(), Some((what, platform)), tag.clone()),
        [step] if step != "part" => (step.as_str(), None, own()?),
        [step, tag] if step != "part" => (step.as_str(), None, tag.clone()),
        _ => return Err(usage),
    };
    if let Some(problem) = ci.tag_problem(&tag) {
        return Err(problem);
    }
    let version = tag_version(&tag)?;
    match step {
        "plan" => {
            let plan = plan(&version)?;
            println!("{}", describe(&plan));
            match crate::unreleased::release_problem(Path::new("."), &version, plan.toolbox.as_deref()) {
                Some(problem) => Err(format!("the tag would be refused: {problem}")),
                None => Ok(()),
            }
        }
        "build" => build(&tag, &version),
        "stage" => stage(&tag, &version, &ci),
        "verify" => verify(&tag, &version),
        "live" => live(&tag, &version),
        "matrix" => crate::release_parts::matrix(&tag, &version),
        "gate" => crate::release_parts::gate(&tag, &version),
        "join" => crate::release_parts::join(&tag, &version),
        "part" => match part {
            Some((what, platform)) => crate::release_parts::part(&version, what, platform),
            None => Err(usage),
        },
        _ => Err(usage),
    }
}

pub(crate) fn tag_version(tag: &str) -> Result<String, String> {
    let version = tag.strip_prefix('v').unwrap_or_default();
    if crate::calendar::parse(version).is_none() {
        return Err(format!("`{tag}` is not a release tag: a release tag is v<YY.M.N>, such as v26.10.4 (ADR-0171)"));
    }
    Ok(version.to_owned())
}

pub(crate) fn stage_name(tag: &str, pipeline: Option<&str>) -> String {
    match pipeline.filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())) {
        Some(id) => format!("{tag}-p{id}"),
        None => {
            let seconds =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_secs());
            format!("{tag}-local-{seconds}")
        }
    }
}

pub(crate) fn pending(local: &[(String, String)], published: &[(String, String)]) -> Vec<(String, String)> {
    local.iter().filter(|entry| !published.contains(entry)).cloned().collect()
}

pub(crate) fn changed_plugins(local: &[(String, String)], published: &[(String, String)]) -> Vec<(String, String)> {
    pending(local, published).into_iter().filter(|(id, _)| published.iter().any(|(known, _)| known == id)).collect()
}

pub(crate) fn plan(version: &str) -> Result<Plan, String> {
    let published: Vec<(String, String)> =
        crate::native::published()?.into_iter().map(|(product, version, _)| (product, version)).collect();
    let toolbox = crate::changelog::all_notes(&crate::changelog::product_path("toolbox"))?
        .into_iter()
        .next()
        .map(|(version, _)| ("toolbox".to_owned(), version));
    let toolbox = toolbox.and_then(|entry| pending(std::slice::from_ref(&entry), &published).pop()).map(|(_, v)| v);
    let plugins = changed_plugins(&local_plugins()?, &published_plugins()?);
    Ok(Plan { version: version.to_owned(), toolbox, plugins })
}

pub(crate) fn describe(plan: &Plan) -> String {
    let names: Vec<&str> = crate::release::IDES.iter().map(|ide| ide.1).collect();
    let mut lines = vec![format!("every IDE {} ({}) for macOS, Linux and Windows", plan.version, names.join(", "))];
    if let Some(version) = &plan.toolbox {
        lines.push(format!("Zen Toolbox {version} for macOS, Linux and Windows"));
    }
    for (id, version) in &plan.plugins {
        lines.push(format!("the {id} plug-in {version}"));
    }
    lines.join("\n")
}

fn local_plugins() -> Result<Vec<(String, String)>, String> {
    let mut plugins = Vec::new();
    for entry in std::fs::read_dir("plugins").map_err(|error| format!("plugins: {error}"))?.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if !crate::changelog::plugin_path(&id).is_file() {
            continue;
        }
        let Ok(manifest) = std::fs::read_to_string(entry.path().join("Cargo.toml")) else { continue };
        let version = manifest
            .lines()
            .find_map(|line| line.trim().strip_prefix("version = \"").and_then(|rest| rest.strip_suffix('"')));
        if let Some(version) = version {
            plugins.push((id, version.to_owned()));
        }
    }
    plugins.sort();
    Ok(plugins)
}

fn published_plugins() -> Result<Vec<(String, String)>, String> {
    let manifest = std::env::var("ZEN_RELEASES")
        .unwrap_or_else(|_| "https://ide.ifzenelse.space/releases/manifest.json".to_owned());
    let url = manifest.replace("manifest.json", "plugins.json");
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "30"])
        .arg(&url)
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("cannot read the plug-in catalog from {url}"));
    }
    let catalog: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| format!("{url}: {error}"))?;
    Ok(catalog["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|plugin| {
            let id = plugin["id"].as_str().unwrap_or_default().to_owned();
            plugin["versions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |version| version["version"].as_str().map(|version| (id.clone(), version.to_owned())))
        })
        .collect())
}

fn build(tag: &str, version: &str) -> Result<(), String> {
    let plan = plan(version)?;
    println!("==> The release of {tag}:\n{}", describe(&plan));
    if let Some(problem) = crate::unreleased::release_problem(Path::new("."), version, plan.toolbox.as_deref()) {
        return Err(format!("{tag} is not released, nothing was built: {problem}"));
    }
    let root = std::path::absolute(BUILD_DIR).map_err(|error| error.to_string())?;
    crate::release_flow::clean_out(&root)?;
    let builds = root.join("builds");
    let word = |text: &str| text.to_owned();
    let out = |folder: &Path| folder.display().to_string();
    let build = |args: Vec<String>| crate::release_flow::build_command(&args);

    build(vec![word("ides"), word(version), word("--platform"), word("all"), word("--out"), out(&builds)])?;
    let mut groups = vec![format!("builds/ides-{version}")];
    if let Some(toolbox) = &plan.toolbox {
        let folder = builds.join("toolbox");
        build(vec![word("toolbox"), toolbox.clone(), word("--platform"), word("all"), word("--out"), out(&folder)])?;
        groups.push("builds/toolbox".to_owned());
    }
    for (id, plugin_version) in &plan.plugins {
        let folder = builds.join("plugins");
        build(vec![
            word("plugin"),
            id.clone(),
            plugin_version.clone(),
            word("--platform"),
            word("all"),
            word("--out"),
            out(&folder),
        ])?;
        groups.push(format!("builds/plugins/plugin-{id}-{plugin_version}"));
    }
    println!("==> The website");
    website(&root.join("site"))?;
    let built = BuiltRelease { schema: SCHEMA, tag: tag.to_owned(), version: version.to_owned(), groups };
    std::fs::write(PLAN_FILE, serde_json::to_vec_pretty(&built).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    println!("\n==> {tag} is built into {BUILD_DIR}/; nothing is uploaded yet.");
    Ok(())
}

fn stage(tag: &str, version: &str, ci: &crate::ci_host::Ci) -> Result<(), String> {
    let secrets = crate::release::secrets()?;
    let bytes = std::fs::read(PLAN_FILE).map_err(|error| format!("{PLAN_FILE}: {error}; the build job leaves it"))?;
    let built: BuiltRelease = serde_json::from_slice(&bytes).map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    if built.schema != SCHEMA || built.tag != tag || built.version != version {
        return Err(format!("{PLAN_FILE} is the build of {} {}, not of {tag}", built.tag, built.version));
    }
    let root = Path::new(BUILD_DIR);
    let groups: Vec<PathBuf> = built.groups.iter().map(|group| root.join(group)).collect();
    if groups.iter().any(|group| group.components().any(|part| part == std::path::Component::ParentDir)) {
        return Err(format!("{PLAN_FILE}: a group outside {BUILD_DIR}"));
    }
    let stage = stage_name(tag, ci.run());
    let mut entries = Vec::new();
    for group in &groups {
        for folder in build_folders(group)? {
            let record = crate::release_flow::verified(&folder)?;
            for file in &record.files {
                if let Some(served) = served(&record, file, &secrets.server) {
                    entries.push(upload_entry(&served.url, &folder.join(&file.name))?);
                }
            }
        }
    }
    println!("==> Uploading {} files, 3 at a time", entries.len());
    run(
        Command::new(crate::release_flow::zen_releases()?)
            .args(["upload", "--server", &secrets.server, "--jobs", "3"])
            .args(&entries)
            .env("ZEN_RELEASES_TOKEN", &secrets.token),
        "zen-releases upload",
    )?;
    for group in &groups {
        crate::release_flow::publish_all(group, false, Some(&stage))?;
    }
    println!("==> Staging the website");
    run(
        Command::new(crate::release_flow::zen_releases()?)
            .args(["stage-site", "--server", &secrets.server, "--stage", &stage])
            .arg(root.join("site"))
            .env("ZEN_RELEASES_TOKEN", &secrets.token),
        "zen-releases stage-site",
    )?;
    let records = records(&groups)?;
    let files = records
        .iter()
        .flat_map(|record| record.files.iter().filter_map(|file| served(record, file, &secrets.server)))
        .collect();
    let record = StageRecord {
        schema: SCHEMA,
        tag: tag.to_owned(),
        version: version.to_owned(),
        stage: stage.clone(),
        server: secrets.server.trim_end_matches('/').to_owned(),
        commit: records.first().map(|record| record.commit.clone()).unwrap_or_default(),
        products: products(&records),
        files,
        site: format!("{}/releases/staged-site/{stage}/index.html", secrets.server.trim_end_matches('/')),
    };
    std::fs::write(STAGE_FILE, serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{STAGE_FILE}: {error}"))?;
    println!("\n==> {tag} is staged as {stage}; nothing is live yet. The website as it will be: {}", record.site);
    Ok(())
}

fn staged(tag: &str, version: &str) -> Result<StageRecord, String> {
    let bytes = std::fs::read(STAGE_FILE).map_err(|error| {
        format!("{STAGE_FILE}: {error}; the stage job leaves it (run the pipeline's stage job first)")
    })?;
    let record: StageRecord = serde_json::from_slice(&bytes).map_err(|error| format!("{STAGE_FILE}: {error}"))?;
    if record.schema != SCHEMA || record.tag != tag || record.version != version {
        return Err(format!("{STAGE_FILE} is the stage of {} {}, not of {tag}", record.tag, record.version));
    }
    Ok(record)
}

fn verify(tag: &str, version: &str) -> Result<(), String> {
    let record = staged(tag, version)?;
    let dir = std::path::absolute("release-verify").map_err(|error| error.to_string())?;
    let mut checks = check_downloads(&record.files, &dir)?;
    checks.push(check_stage(&record)?);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::write(VERIFY_FILE, serde_json::to_vec_pretty(&checks).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{VERIFY_FILE}: {error}"))?;
    println!("\n==> {tag} ({}) is checked:", record.stage);
    for check in &checks {
        println!("  {check}");
    }
    println!("  the website as it will be: {}", record.site);
    println!("Press \"release\" in the pipeline to make it live.");
    Ok(())
}

pub(crate) fn website(out: &Path) -> Result<(), String> {
    crate::release_flow::clean_out(out)?;
    copy_tree(Path::new("deploy/releases/site"), out)?;
    crate::site::run(&[out.join("docs").display().to_string()])
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with('.') || (name == "docs" && from.ends_with("deploy/releases/site")) {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(&name))?;
        } else {
            std::fs::copy(&path, to.join(&name)).map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn upload_entry(url: &str, path: &Path) -> Result<String, String> {
    let at = url.find("/releases/artifacts/").ok_or_else(|| format!("{url}: not an artifact's address"))?;
    let to = &url[at + "/releases/artifacts/".len()..];
    if to.split('/').count() != 3 {
        return Err(format!("{url}: not an artifact's address"));
    }
    Ok(format!("{to}={}", path.display()))
}

fn records(groups: &[PathBuf]) -> Result<Vec<BuildRecord>, String> {
    let mut records = Vec::new();
    for group in groups {
        for folder in build_folders(group)? {
            records.push(crate::release_flow::verified(&folder)?);
        }
    }
    Ok(records)
}

pub(crate) fn build_folders(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if dir.join("build.json").is_file() {
        return Ok(vec![dir.to_owned()]);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("build.json").is_file())
        .collect();
    found.sort();
    Ok(found)
}

pub(crate) fn products(records: &[BuildRecord]) -> Vec<StagedProduct> {
    let mut products: Vec<StagedProduct> = Vec::new();
    for record in records {
        if !products.iter().any(|product| product.product == record.product && product.version == record.version) {
            products.push(StagedProduct {
                kind: record.kind.clone(),
                product: record.product.clone(),
                name: record.name.clone(),
                version: record.version.clone(),
                notes: record.notes.clone(),
            });
        }
    }
    products
}

pub(crate) fn served(record: &BuildRecord, file: &crate::release_flow::BuiltFile, server: &str) -> Option<StagedFile> {
    let base = format!("{}/releases/artifacts", server.trim_end_matches('/'));
    let (url, name) = match file.role.as_str() {
        "module" => (
            format!("{base}/plugin-{id}/{}/{id}.wasm", record.version, id = record.product),
            format!("zen-plugin-{}-{}.wasm", record.product, record.version),
        ),
        "update" | "installer" | "installer-checksum" | "package" | "download" => {
            (format!("{base}/{}/{}/{}", record.product, record.version, file.name), file.name.clone())
        }
        "driver" => (format!("{base}/plugin-{}/{}/{}", record.product, record.version, file.name), file.name.clone()),
        _ => return None,
    };
    let link_type = if file.role == "installer-checksum" { "other" } else { "package" };
    let platform = match file.role.as_str() {
        "driver" => program_platform(&file.name).unwrap_or(&record.platform).to_owned(),
        _ => record.platform.clone(),
    };
    Some(StagedFile {
        name,
        role: file.role.clone(),
        url,
        link_type: link_type.to_owned(),
        platform,
        size: file.size,
        sha256: file.sha256.clone(),
    })
}

pub(crate) fn program_platform(name: &str) -> Option<&'static str> {
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    ["macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64", "windows-x86_64"]
        .into_iter()
        .find(|platform| stem.strip_suffix(platform).is_some_and(|rest| rest.ends_with('-')))
}

fn check_downloads(files: &[StagedFile], dir: &Path) -> Result<Vec<String>, String> {
    crate::release_flow::clean_out(dir)?;
    let docker = Command::new("docker").arg("info").stdout(Stdio::null()).stderr(Stdio::null()).status();
    let docker = docker.is_ok_and(|status| status.success());
    let mut checks = Vec::new();
    let mut debs_skipped = 0;
    for file in files {
        let local = dir.join(&file.name);
        run(Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&local).arg(&file.url), "curl")?;
        let size = std::fs::metadata(&local).map_err(|error| error.to_string())?.len();
        let digest = crate::release_flow::sha256(&local)?;
        if let Some(problem) = download_problem(file, size, &digest) {
            return Err(problem);
        }
        if file.role == "update" && crate::native::is_macos(&file.platform) {
            checks.push(codesign_check(&local, &dir.join("app"))?);
        }
        if file.role == "driver" && crate::native::is_macos(&file.platform) {
            run(Command::new("codesign").args(["--verify", "--strict"]).arg(&local), "codesign --verify")?;
            checks.push(format!("{}: codesign --verify --strict passed", file.name));
        }
        if file.name.ends_with(".deb") {
            if docker {
                checks.push(deb_check(&local)?);
            } else {
                debs_skipped += 1;
            }
        }
        let _ = std::fs::remove_file(&local);
    }
    checks.insert(
        0,
        format!("{} staged files downloaded, each with the size and SHA-256 it was built with", files.len()),
    );
    if debs_skipped > 0 {
        checks.push(format!(
            "{debs_skipped} Debian packages not installed: no Docker on the build runner (install one on Ubuntu after \
             the release, docs/releasing.md)"
        ));
    }
    Ok(checks)
}

pub(crate) fn download_problem(file: &StagedFile, size: u64, digest: &str) -> Option<String> {
    (size != file.size || !digest.eq_ignore_ascii_case(&file.sha256)).then(|| {
        format!(
            "{}: the server serves {size} bytes with SHA-256 {digest}, the build has {} bytes with {}; nothing is live",
            file.url, file.size, file.sha256
        )
    })
}

fn codesign_check(archive: &Path, into: &Path) -> Result<String, String> {
    crate::release_flow::clean_out(into)?;
    run(Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(into), "tar")?;
    let app = std::fs::read_dir(into)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or_else(|| format!("{}: no application in the archive", archive.display()))?;
    run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&app), "codesign --verify")?;
    let name = app.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(into);
    Ok(format!("{name}: codesign --verify --deep --strict passed"))
}

fn deb_check(deb: &Path) -> Result<String, String> {
    let dir = deb.parent().ok_or("no folder")?;
    let name = deb.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    run(
        Command::new("docker")
            .args(["run", "--rm", "--platform", "linux/amd64", "-v"])
            .arg(format!("{}:/check:ro", dir.display()))
            .args(["ubuntu:24.04", "sh", "-c"])
            .arg(format!("apt-get update -qq && apt-get install -y -qq /check/{name} >/dev/null")),
        "docker (installing the .deb)",
    )?;
    Ok(format!("{name}: installed with apt in Ubuntu 24.04"))
}

fn fetch(url: &str) -> Result<Option<Vec<u8>>, String> {
    let output = Command::new("curl")
        .args(["-sSL", "--max-time", "60", "-w", "\n%{http_code}"])
        .arg(url)
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    let mut body = output.stdout;
    let at = body.iter().rposition(|byte| *byte == b'\n').ok_or_else(|| format!("{url}: no answer"))?;
    let status = String::from_utf8_lossy(&body[at + 1..]).trim().to_owned();
    body.truncate(at);
    match status.as_str() {
        "200" => Ok(Some(body)),
        "404" => Ok(None),
        other => Err(format!("{url} answered {other}")),
    }
}

fn check_stage(record: &StageRecord) -> Result<String, String> {
    let base = format!("{}/releases/stage/{}", record.server, record.stage);
    let manifest = fetch(&format!("{base}/manifest.json"))?;
    let listed: Vec<serde_json::Value> = match &manifest {
        Some(bytes) => {
            let manifest: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            manifest["products"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|product| {
                    let versions: Vec<&serde_json::Value> = product["releases"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|release| &release["version"])
                        .collect();
                    serde_json::json!({ "id": product["id"], "versions": versions })
                })
                .collect()
        }
        None => Vec::new(),
    };
    let summary = serde_json::json!({
        "manifest": listed,
        "plugins": fetch(&format!("{base}/plugins.json"))?.is_some(),
        "changelog": fetch(&format!("{base}/changelog.json"))?.is_some(),
        "site": fetch(&record.site)?.is_some(),
        "released": null,
    });
    if let Some(problem) = summary_problem(&summary, &record.products) {
        return Err(format!("the stage {}: {problem}", record.stage));
    }
    Ok(format!("the stage {} lists every product built, with its changelogs and website", record.stage))
}

pub(crate) fn summary_problem(summary: &serde_json::Value, products: &[StagedProduct]) -> Option<String> {
    for product in products.iter().filter(|product| product.kind != "plugin") {
        let listed = summary["manifest"].as_array().into_iter().flatten().any(|entry| {
            entry["id"] == product.product.as_str()
                && entry["versions"].as_array().into_iter().flatten().any(|version| version == product.version.as_str())
        });
        if !listed {
            return Some(format!("its manifest does not list {} {}", product.product, product.version));
        }
    }
    if products.iter().any(|product| product.kind == "plugin") && summary["plugins"] != true {
        return Some("it has no plug-in catalog".to_owned());
    }
    if summary["changelog"] != true || summary["site"] != true {
        return Some("it lacks the changelogs or the website".to_owned());
    }
    if !summary["released"].is_null() {
        return Some("it is released already".to_owned());
    }
    None
}

fn live(tag: &str, version: &str) -> Result<(), String> {
    let secrets = crate::release::secrets()?;
    let record = staged(tag, version)?;
    let checks: Vec<String> = match std::fs::read(VERIFY_FILE) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| format!("{VERIFY_FILE}: {error}"))?,
        Err(_) => {
            return Err(format!("{VERIFY_FILE} is missing: the verify job checks the stage before it is released"));
        }
    };
    if secrets.server.trim_end_matches('/') != record.server {
        return Err(format!("the stage is on {}, the release variables name {}", record.server, secrets.server));
    }
    println!("==> Releasing {} ({})", record.tag, record.stage);
    run(
        Command::new(crate::release_flow::zen_releases()?)
            .args(["release-stage", "--server", &secrets.server, "--stage", &record.stage])
            .env("ZEN_RELEASES_TOKEN", &secrets.token),
        "zen-releases release-stage",
    )?;
    println!("==> {tag} is live: {}/releases/manifest.json, {}/", record.server, record.server);
    match gitlab_context() {
        Some(gitlab) => gitlab_release(&gitlab, &record, &checks),
        None => {
            println!("Not in GitLab CI: create the GitLab release of {tag} by hand if wanted.");
            Ok(())
        }
    }
}

struct Gitlab {
    api: String,
    project: String,
    token: String,
}

fn gitlab_context() -> Option<Gitlab> {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    Some(Gitlab { api: env("CI_API_V4_URL")?, project: env("CI_PROJECT_ID")?, token: env("CI_JOB_TOKEN")? })
}

pub(crate) fn release_description(record: &StageRecord, checks: &[String]) -> String {
    let mut text = format!(
        "Every Zen IDE at {}: GoGarden, RustRonin, WebWave and JvmJizo, for macOS, Linux and Windows. Install or \
         update them with [Zen Toolbox]({}/).\n",
        record.version,
        record.server.trim_end_matches('/')
    );
    for product in &record.products {
        let what = if product.kind == "plugin" { " plug-in" } else { "" };
        text.push_str(&format!("\n## {}{what} {}\n\n{}\n", product.name, product.version, product.notes.trim()));
    }
    text.push_str("\n## Checked before release\n\n");
    for check in checks {
        text.push_str(&format!("- {check}\n"));
    }
    text
}

pub(crate) fn release_json(record: &StageRecord, checks: &[String]) -> serde_json::Value {
    let links: Vec<serde_json::Value> = record
        .files
        .iter()
        .map(|file| serde_json::json!({ "name": file.name, "url": file.url, "link_type": file.link_type }))
        .collect();
    serde_json::json!({
        "name": format!("Zen IDE {}", record.version),
        "tag_name": record.tag,
        "description": release_description(record, checks),
        "assets": { "links": links },
    })
}

fn gitlab_release(gitlab: &Gitlab, record: &StageRecord, checks: &[String]) -> Result<(), String> {
    let releases = format!("{}/projects/{}/releases", gitlab.api, gitlab.project);
    let json = release_json(record, checks);
    let (status, body) = gitlab_call(gitlab, "POST", &releases, &json)?;
    if (200..300).contains(&status) {
        println!("==> The GitLab release {} is created with {} files.", record.tag, record.files.len());
        return Ok(());
    }
    if status != 409 {
        return Err(format!("GitLab refused the release ({status}): {body}"));
    }
    let release = format!("{releases}/{}", record.tag);
    let update = serde_json::json!({ "name": json["name"], "description": json["description"] });
    let (status, body) = gitlab_call(gitlab, "PUT", &release, &update)?;
    if !(200..300).contains(&status) {
        return Err(format!("GitLab refused to update the release ({status}): {body}"));
    }
    for link in json["assets"]["links"].as_array().into_iter().flatten() {
        let _ = gitlab_call(gitlab, "POST", &format!("{release}/assets/links"), link)?;
    }
    println!("==> The GitLab release {} is updated.", record.tag);
    Ok(())
}

fn curl_call(gitlab: &Gitlab, method: &str, url: &str, body: &str) -> (Vec<String>, String) {
    let args = ["-sS", "-K", "-", "-w", "\n%{http_code}", "--data-binary", body].map(str::to_owned).to_vec();
    let config = format!(
        "header = \"JOB-TOKEN: {}\"\nheader = \"Content-Type: application/json\"\nrequest = \"{method}\"\nurl = \"{url}\"\n",
        gitlab.token
    );
    (args, config)
}

fn gitlab_call(gitlab: &Gitlab, method: &str, url: &str, json: &serde_json::Value) -> Result<(u16, String), String> {
    use std::io::Write;
    let body = serde_json::to_string(json).map_err(|error| error.to_string())?;
    let (args, config) = curl_call(gitlab, method, url, &body);
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(config.as_bytes()).map_err(|error| error.to_string())?;
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    let status = status.trim().parse().map_err(|_| format!("curl {method} {url}: no HTTP status"))?;
    Ok((status, body.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: &str, product: &str, version: &str, platform: &str) -> BuildRecord {
        BuildRecord {
            schema: 1,
            kind: kind.to_owned(),
            product: product.to_owned(),
            name: product.to_owned(),
            version: version.to_owned(),
            notes: format!("- {product} notes."),
            commit: "c0ffee".to_owned(),
            tree: "7ree".to_owned(),
            rustc: String::new(),
            platform: platform.to_owned(),
            gate: "passed".to_owned(),
            host: "air".to_owned(),
            publish_args: Vec::new(),
            files: Vec::new(),
        }
    }

    fn file(name: &str, role: &str) -> crate::release_flow::BuiltFile {
        crate::release_flow::BuiltFile {
            name: name.to_owned(),
            role: role.to_owned(),
            size: 3,
            sha256: "ab".to_owned(),
        }
    }

    #[test]
    fn a_release_tag_names_a_calendar_version() {
        assert_eq!(tag_version("v26.10.4").unwrap(), "26.10.4");
        for bad in ["26.10.4", "v0.3.15", "v26.13.1", "release-26.10.4", "v26.10.4-rc1", ""] {
            assert!(tag_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_pipeline_stages_under_a_name_of_its_own() {
        assert_eq!(stage_name("v26.10.4", Some("2142")), "v26.10.4-p2142");
        assert!(stage_name("v26.10.4", None).starts_with("v26.10.4-local-"));
        assert!(stage_name("v26.10.4", Some("../x")).starts_with("v26.10.4-local-"), "only digits");
    }

    #[test]
    fn the_job_token_reaches_curl_on_its_standard_input_and_never_on_its_command_line() {
        let gitlab = Gitlab { api: "https://x".to_owned(), project: "1".to_owned(), token: "s3cret".to_owned() };
        let (args, config) = curl_call(&gitlab, "POST", "https://x/releases", "{}");
        assert!(config.contains("JOB-TOKEN: s3cret"));
        assert!(args.iter().all(|arg| !arg.contains("s3cret")), "{args:?}");
    }

    #[test]
    fn only_what_is_not_published_is_built() {
        let local = vec![("json".to_owned(), "0.2.0".to_owned()), ("yaml".to_owned(), "0.1.1".to_owned())];
        let published = vec![("json".to_owned(), "0.2.0".to_owned()), ("yaml".to_owned(), "0.1.0".to_owned())];
        assert_eq!(pending(&local, &published), [("yaml".to_owned(), "0.1.1".to_owned())]);
        let mut with_new = local.clone();
        with_new.push(("http".to_owned(), "0.1.0".to_owned()));
        assert_eq!(
            changed_plugins(&with_new, &published),
            [("yaml".to_owned(), "0.1.1".to_owned())],
            "a plug-in never in the catalog is not put there by a tag"
        );
        let plan = Plan { version: "26.10.4".to_owned(), toolbox: Some("0.3.20".to_owned()), plugins: local };
        let text = describe(&plan);
        assert!(text.contains("every IDE 26.10.4") && text.contains("Zen Toolbox 0.3.20") && text.contains("yaml"));
        assert!(
            text.contains("(GoGarden, RustRonin, WebWave, JvmJizo, SqlScroll, DevopsDaruma)"),
            "the IDEs are named: {text}"
        );
    }

    #[test]
    fn every_uploaded_file_has_its_public_address() {
        let ide = record("ide", "gogarden", "26.10.4", "macos-aarch64");
        let archive = served(&ide, &file("gogarden-26.10.4-macos-aarch64.tar.gz", "update"), "https://x/").unwrap();
        assert_eq!(archive.url, "https://x/releases/artifacts/gogarden/26.10.4/gogarden-26.10.4-macos-aarch64.tar.gz");
        assert_eq!(archive.link_type, "package");
        assert!(served(&ide, &file("changelog.json", "changelog"), "https://x").is_none(), "not uploaded as a file");
        let checksum = served(&ide, &file("Zen.dmg.sha256", "installer-checksum"), "https://x").unwrap();
        assert_eq!(checksum.link_type, "other");
        let plugin = record("plugin", "json", "0.2.0", "wasm32");
        let module = served(&plugin, &file("zen-plugin-json-0.2.0.wasm", "module"), "https://x").unwrap();
        assert_eq!(
            module.url, "https://x/releases/artifacts/plugin-json/0.2.0/json.wasm",
            "as publish-plugin stores it"
        );
    }

    #[test]
    fn every_file_is_uploaded_where_it_is_served() {
        let plugin = record("plugin", "json", "0.2.0", "wasm32");
        let module = served(&plugin, &file("zen-plugin-json-0.2.0.wasm", "module"), "https://x").unwrap();
        assert_eq!(
            upload_entry(&module.url, Path::new("/b/zen-plugin-json-0.2.0.wasm")).unwrap(),
            "plugin-json/0.2.0/json.wasm=/b/zen-plugin-json-0.2.0.wasm",
            "a module goes where publish-plugin lists it"
        );
        assert!(upload_entry("https://x/releases/manifest.json", Path::new("/m")).is_err());
    }

    #[test]
    fn a_driver_program_is_staged_beside_its_plugin_for_its_platform() {
        let plugin = record("plugin", "duckdb", "0.1.0", "wasm32");
        let program = served(&plugin, &file("zen-db-duckdb-0.1.0-macos-aarch64", "driver"), "https://x").unwrap();
        assert_eq!(program.url, "https://x/releases/artifacts/plugin-duckdb/0.1.0/zen-db-duckdb-0.1.0-macos-aarch64");
        assert_eq!(program.platform, "macos-aarch64", "its signature is checked as a macOS program's");
        assert_eq!(
            upload_entry(&program.url, Path::new("/b/p")).unwrap(),
            "plugin-duckdb/0.1.0/zen-db-duckdb-0.1.0-macos-aarch64=/b/p"
        );
        let windows = served(&plugin, &file("zen-db-duckdb-0.1.0-windows-x86_64.exe", "driver"), "https://x").unwrap();
        assert_eq!(windows.platform, "windows-x86_64");
        assert_eq!(program_platform("zen-db-duckdb-0.1.0-linux-x86_64"), Some("linux-x86_64"));
        assert_eq!(program_platform("zen-db-duckdb-0.1.0"), None);
        assert_eq!(program_platform("zen-db-duckdbmacos-aarch64"), None);
    }

    #[test]
    fn a_download_must_be_the_file_built() {
        let ide = record("ide", "gogarden", "26.10.4", "macos-aarch64");
        let staged = served(&ide, &file("g.tar.gz", "update"), "https://x").unwrap();
        assert_eq!(download_problem(&staged, 3, "AB"), None, "the digest's case does not matter");
        assert!(download_problem(&staged, 4, "ab").unwrap().contains("4 bytes"));
        assert!(download_problem(&staged, 3, "cd").unwrap().contains("nothing is live"));
    }

    #[test]
    fn the_stage_must_list_everything_built() {
        let records = vec![
            record("ide", "gogarden", "26.10.4", "macos-aarch64"),
            record("ide", "gogarden", "26.10.4", "linux-x86_64"),
            record("plugin", "json", "0.2.0", "wasm32"),
        ];
        let good = serde_json::json!({
            "manifest": [{ "id": "gogarden", "versions": ["26.10.3", "26.10.4"] }],
            "plugins": true, "changelog": true, "site": true, "released": null
        });
        let records = products(&records);
        assert_eq!(summary_problem(&good, &records), None);
        let mut missing = good.clone();
        missing["manifest"][0]["versions"] = serde_json::json!(["26.10.3"]);
        assert!(summary_problem(&missing, &records).unwrap().contains("gogarden 26.10.4"));
        let mut no_catalog = good.clone();
        no_catalog["plugins"] = serde_json::json!(false);
        assert!(summary_problem(&no_catalog, &records).unwrap().contains("catalog"));
        let mut no_site = good.clone();
        no_site["site"] = serde_json::json!(false);
        assert!(summary_problem(&no_site, &records).is_some());
        let mut released = good;
        released["released"] = serde_json::json!("2026-10-27");
        assert!(summary_problem(&released, &records).unwrap().contains("released already"));
    }

    #[test]
    fn the_gitlab_release_carries_the_notes_and_a_link_to_every_file() {
        let records = vec![
            record("ide", "gogarden", "26.10.4", "macos-aarch64"),
            record("ide", "gogarden", "26.10.4", "linux-x86_64"),
            record("plugin", "json", "0.2.0", "wasm32"),
        ];
        let ide = &records[0];
        let stage = StageRecord {
            schema: SCHEMA,
            tag: "v26.10.4".to_owned(),
            version: "26.10.4".to_owned(),
            stage: "v26.10.4-p7".to_owned(),
            server: "https://ide.ifzenelse.space".to_owned(),
            commit: "c0ffee".to_owned(),
            products: products(&records),
            files: vec![served(ide, &file("gogarden.tar.gz", "update"), "https://ide.ifzenelse.space").unwrap()],
            site: String::new(),
        };
        assert_eq!(stage.products.len(), 2, "an IDE's platforms are one product");
        let json = release_json(&stage, &["1 staged files downloaded".to_owned()]);
        assert_eq!(json["tag_name"], "v26.10.4");
        assert_eq!(json["name"], "Zen IDE 26.10.4");
        assert_eq!(json["assets"]["links"][0]["link_type"], "package");
        let description = json["description"].as_str().unwrap();
        assert!(description.contains("## gogarden 26.10.4\n\n- gogarden notes."), "{description}");
        assert!(description.contains("## json plug-in 0.2.0"));
        assert!(description.contains("- 1 staged files downloaded"));
    }
}
