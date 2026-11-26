use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub(crate) const WASM: &str = "wasm32-unknown-unknown";

pub(crate) const LINUX: &str = "x86_64-unknown-linux-gnu";

pub(crate) const WINDOWS: &str = "x86_64-pc-windows-gnu";

pub(crate) const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES";

pub(crate) const LICENCES_FOLDER: &str = "Licenses";

pub(crate) const PROJECT_FILES: [&str; 3] = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE"];

pub(crate) const RUST_COPYRIGHT_FILE: &str = "RUST-STANDARD-LIBRARY-COPYRIGHT.html";

pub(crate) const PROJECT_FONT_LICENCES: [&str; 3] = ["OFL-Inter.txt", "OFL-JetBrainsMono.txt", "OFL-STIXTwoMath.txt"];

pub(crate) const EGUI_FONT_LICENCES: [(&str, &str); 4] = [
    ("fonts/OFL.txt", "OFL-NotoEmoji.txt"),
    ("fonts/UFL.txt", "UFL-Ubuntu.txt"),
    ("fonts/Hack-Regular.txt", "Hack.txt"),
    ("fonts/emoji-icon-font-mit-license.txt", "MIT-emoji-icon-font.txt"),
];

pub(crate) const WASM_SECTION: &str = "zen-licences";

pub(crate) const HEADER: &str = "Third-party notices for ";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Shipped {
    pub(crate) package: String,
    pub(crate) target: Option<String>,
}

impl Shipped {
    pub(crate) fn new(package: &str, target: Option<&str>) -> Self {
        Self { package: package.to_owned(), target: target.map(str::to_owned) }
    }
}

#[derive(Clone, Debug)]
struct Package {
    name: String,
    version: String,
    license: String,
    repository: String,
    dir: PathBuf,
    license_file: Option<String>,
}

#[derive(Default)]
struct Graph {
    packages: BTreeMap<String, Package>,
    deps: BTreeMap<String, Vec<String>>,
    members: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct Override {
    #[serde(default)]
    licence: Option<String>,
    #[serde(default)]
    copyright: Option<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Overrides {
    crates: BTreeMap<String, Override>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Text {
    origin: String,
    body: String,
}

#[derive(Debug, Default)]
pub(crate) struct Notices {
    pub(crate) text: String,
    pub(crate) missing: Vec<String>,
    pub(crate) crates: usize,
    used_overrides: BTreeSet<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn host() -> Result<String, String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(rustc).arg("-vV").output().map_err(|error| format!("cannot run rustc: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .ok_or_else(|| "rustc -vV names no host".to_owned())
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    Command::new(rustc)
        .arg("-V")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

static GRAPHS: Mutex<BTreeMap<String, Arc<Graph>>> = Mutex::new(BTreeMap::new());

fn graph(target: &str) -> Result<Arc<Graph>, String> {
    let mut graphs = GRAPHS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(graph) = graphs.get(target) {
        return Ok(Arc::clone(graph));
    }
    let graph = Arc::new(read_graph(target)?);
    graphs.insert(target.to_owned(), Arc::clone(&graph));
    Ok(graph)
}

fn read_graph(target: &str) -> Result<Graph, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked", "--filter-platform", target])
        .current_dir(root())
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| format!("cargo metadata: {error}"))?;
    parse(&metadata)
}

fn parse(metadata: &serde_json::Value) -> Result<Graph, String> {
    let text = |value: &serde_json::Value| value.as_str().unwrap_or_default().to_owned();
    let mut graph = Graph::default();
    let members: BTreeSet<String> = metadata["workspace_members"].as_array().into_iter().flatten().map(text).collect();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let id = text(&package["id"]);
        let manifest = PathBuf::from(text(&package["manifest_path"]));
        let entry = Package {
            name: text(&package["name"]),
            version: text(&package["version"]),
            license: text(&package["license"]),
            repository: text(&package["repository"]),
            dir: manifest.parent().map(Path::to_path_buf).unwrap_or_default(),
            license_file: package["license_file"].as_str().map(str::to_owned),
        };
        if members.contains(&id) {
            graph.members.insert(entry.name.clone(), id.clone());
        }
        graph.packages.insert(id, entry);
    }
    let nodes = metadata["resolve"]["nodes"].as_array().ok_or("cargo metadata has no resolve graph")?;
    for node in nodes {
        let shipped = node["deps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|dep| {
                dep["dep_kinds"].as_array().into_iter().flatten().any(|kind| kind["kind"].as_str() != Some("dev"))
            })
            .map(|dep| text(&dep["pkg"]))
            .collect();
        graph.deps.insert(text(&node["id"]), shipped);
    }
    Ok(graph)
}

fn closure(graph: &Graph, package: &str) -> Result<BTreeSet<String>, String> {
    let start = graph.members.get(package).ok_or_else(|| format!("`{package}` is not a package of the workspace"))?;
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.clone()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        stack.extend(graph.deps.get(&id).into_iter().flatten().cloned());
    }
    let members: BTreeSet<&String> = graph.members.values().collect();
    Ok(seen.into_iter().filter(|id| !members.contains(id)).collect())
}

fn is_licence_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "UNLICENSE", "NOTICE"].iter().any(|prefix| upper.starts_with(prefix))
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

fn own_texts(package: &Package) -> Result<Vec<Text>, String> {
    let mut paths = BTreeSet::new();
    for entry in std::fs::read_dir(&package.dir).map_err(|error| format!("{}: {error}", package.dir.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_file() && is_licence_name(&name) {
            paths.insert(name);
        } else if path.is_dir() && matches!(name.to_ascii_uppercase().as_str(), "LICENSES" | "LICENCES") {
            for inner in std::fs::read_dir(&path).map_err(|error| error.to_string())?.flatten() {
                if inner.path().is_file() {
                    paths.insert(format!("{name}/{}", inner.file_name().to_string_lossy()));
                }
            }
        }
    }
    if let Some(file) = &package.license_file
        && package.dir.join(file).is_file()
    {
        paths.insert(file.clone());
    }
    paths.into_iter().map(|origin| Ok(Text { body: read_text(&package.dir.join(&origin))?, origin })).collect()
}

fn overrides(root: &Path) -> Result<BTreeMap<String, Override>, String> {
    let path = root.join("licenses/crates.json");
    let parsed: Overrides =
        serde_json::from_str(&read_text(&path)?).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(parsed.crates)
}

fn texts_of(package: &Package, rule: Option<&Override>, root: &Path) -> Result<Option<Vec<Text>>, String> {
    let mut texts = own_texts(package)?;
    let rule = rule.cloned().unwrap_or_default();
    for file in &rule.files {
        texts.push(Text { origin: file.clone(), body: read_text(&package.dir.join(file))? });
    }
    if texts.iter().all(|text| rule.files.contains(&text.origin)) {
        let Some(licence) = &rule.licence else { return Ok(None) };
        let standard = read_text(&root.join("licenses/texts").join(format!("{licence}.txt")))?;
        let body = match &rule.copyright {
            Some(copyright) => format!("{copyright}\n\n{standard}"),
            None => standard,
        };
        texts.push(Text {
            origin: format!("{licence} (the licence the crate declares; it ships no licence file)"),
            body,
        });
    }
    Ok(Some(texts))
}

fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn generate(title: &str, shipped: &[Shipped]) -> Result<Notices, String> {
    let root = root();
    let host = host()?;
    let mut ids: BTreeMap<String, Package> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for item in shipped {
        let target = item.target.clone().unwrap_or_else(|| host.clone());
        let graph = graph(&target)?;
        for id in closure(&graph, &item.package)? {
            ids.insert(id.clone(), graph.packages[&id].clone());
        }
        targets.insert(target);
    }
    render(title, ids.into_values().collect(), &targets, &overrides(&root)?, &root)
}

fn render(
    title: &str,
    mut packages: Vec<Package>,
    targets: &BTreeSet<String>,
    rules: &BTreeMap<String, Override>,
    root: &Path,
) -> Result<Notices, String> {
    packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    let mut notices = Notices { crates: packages.len(), ..Notices::default() };
    let mut groups: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    let mut listing = String::new();
    for package in &packages {
        let rule = rules.get(&package.name);
        let Some(texts) = texts_of(package, rule, root)? else {
            notices.missing.push(format!("{} {} ({})", package.name, package.version, package.license));
            continue;
        };
        if rule.is_some() {
            notices.used_overrides.insert(package.name.clone());
        }
        let licence = if package.license.is_empty() { "see its licence text" } else { package.license.as_str() };
        listing.push_str(&format!("  {} {}, {licence}", package.name, package.version));
        if !package.repository.is_empty() {
            listing.push_str(&format!(", {}", package.repository));
        }
        listing.push('\n');
        for text in texts {
            let key = normalised(&text.body);
            let user = format!("{} {}: {}", package.name, package.version, text.origin);
            match index.get(&key) {
                Some(&at) => groups[at].2.push(user),
                None => {
                    index.insert(key, groups.len());
                    groups.push((user.clone(), text.body.trim_end().to_owned(), Vec::new()));
                }
            }
        }
    }
    let rule = "=".repeat(78);
    let thin = "-".repeat(78);
    let mut out = format!(
        "{HEADER}{title}\n\n{title} contains the third-party software listed below, each under its own licence. \
         This file is generated by `cargo xtask notices` from Cargo.lock and the licence files the crates \
         publish; ATTRIBUTION.md lists the bundled fonts, icons and data. Zen IDE itself is licensed under the \
         MIT License or the Apache License, Version 2.0 (LICENSE-MIT, LICENSE-APACHE, NOTICE).\n\n\
         Rust crates ({})\n\n{listing}\nToolchain\n\n",
        packages.len()
    );
    out.push_str(&format!(
        "  The Rust standard library ({}), Apache-2.0 OR MIT, https://github.com/rust-lang/rust; its full copyright \
         notices are {RUST_COPYRIGHT_FILE} next to this file in the IDE and Zen Toolbox, and the texts of both \
         licences are below.\n",
        rustc_version()
    ));
    let windows = targets.contains(WINDOWS);
    if windows {
        out.push_str(
            "  The mingw-w64 runtime and winpthreads, linked into the Windows programs by the x86_64-pc-windows-gnu \
             target; their notices are below.\n",
        );
    }
    out.push_str("\nLicence texts\n\n");
    let mut sections = groups;
    if windows {
        for file in ["mingw-w64-runtime.txt", "mingw-w64-winpthreads.txt"] {
            let body = read_text(&root.join("licenses/toolchain").join(file))?;
            sections.push((format!("mingw-w64: {file}"), body.trim_end().to_owned(), Vec::new()));
        }
    }
    for (first, body, others) in sections {
        out.push_str(&format!("{rule}\n{first}\n"));
        for other in others {
            out.push_str(&format!("{other}\n"));
        }
        out.push_str(&format!("{thin}\n{body}\n\n"));
    }
    notices.text = out;
    Ok(notices)
}

pub(crate) fn ide_shipped(product: &str, target: Option<&str>) -> Result<Vec<Shipped>, String> {
    let (_, _, package) = crate::release::IDES
        .iter()
        .chain(crate::release::UNRELEASED_IDES)
        .find(|ide| ide.0 == product)
        .ok_or_else(|| format!("unknown IDE `{product}`"))?;
    let mut shipped = vec![Shipped::new(package, target), Shipped::new("zen-cli", target)];
    for plugin in crate::release::bundled_plugins(product)? {
        shipped.push(Shipped::new(&format!("zen-plugin-{plugin}"), Some(WASM)));
    }
    Ok(shipped)
}

pub(crate) fn product_shipped(product: &str, target: Option<&str>) -> Result<Vec<Shipped>, String> {
    if product == "toolbox" {
        return Ok(vec![Shipped::new("zen-toolbox-app", target)]);
    }
    ide_shipped(product, target)
}

fn every_plugin(root: &Path) -> Vec<String> {
    folders(&root.join("plugins"), "plugin.json")
}

fn every_driver(root: &Path) -> Vec<String> {
    folders(&root.join("drivers"), "Cargo.toml").into_iter().filter(|id| id != "kit").collect()
}

fn folders(parent: &Path, marker: &str) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join(marker).is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    ids
}

pub(crate) fn everything_shipped() -> Result<Vec<Shipped>, String> {
    let root = root();
    let mut shipped = Vec::new();
    for target in [None, Some(LINUX), Some(WINDOWS)] {
        for (id, _, _) in crate::release::IDES {
            shipped.extend(ide_shipped(id, target)?);
        }
        shipped.push(Shipped::new("zen-toolbox-app", target));
        for driver in every_driver(&root) {
            shipped.push(Shipped::new(&format!("zen-db-{driver}"), target));
        }
    }
    for plugin in every_plugin(&root) {
        shipped.push(Shipped::new(&format!("zen-plugin-{plugin}"), Some(WASM)));
    }
    shipped.sort();
    shipped.dedup();
    Ok(shipped)
}

pub(crate) fn check() -> Result<Notices, String> {
    let notices = generate("every Zen IDE program", &everything_shipped()?)?;
    let mut problems = Vec::new();
    if !notices.missing.is_empty() {
        problems.push(format!(
            "{} shipped crate(s) publish no licence text; add each to licenses/crates.json with the licence it \
             declares (docs/licences.md):\n  {}",
            notices.missing.len(),
            notices.missing.join("\n  ")
        ));
    }
    let unused: Vec<String> =
        overrides(&root())?.into_keys().filter(|name| !notices.used_overrides.contains(name)).collect();
    if !unused.is_empty() {
        problems.push(format!(
            "licenses/crates.json names crate(s) that no shipped program uses any more: {}",
            unused.join(", ")
        ));
    }
    if problems.is_empty() { Ok(notices) } else { Err(problems.join("\n")) }
}

fn require_complete(notices: Notices, title: &str) -> Result<String, String> {
    if notices.missing.is_empty() {
        return Ok(notices.text);
    }
    Err(format!(
        "{title}: {} crate(s) publish no licence text: {}; add them to licenses/crates.json (docs/licences.md)",
        notices.missing.len(),
        notices.missing.join(", ")
    ))
}

pub(crate) fn notices_text(title: &str, shipped: &[Shipped]) -> Result<String, String> {
    require_complete(generate(title, shipped)?, title)
}

fn rust_copyright() -> Result<PathBuf, String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(rustc)
        .args(["--print", "sysroot"])
        .output()
        .map_err(|error| format!("cannot run rustc: {error}"))?;
    let path =
        PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("share/doc/rust/COPYRIGHT-library.html");
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{} is missing: the Rust toolchain ships it with rustc", path.display()))
    }
}

fn egui_fonts_dir() -> Result<PathBuf, String> {
    let host = host()?;
    let graph = graph(&host)?;
    graph
        .packages
        .values()
        .find(|package| package.name == "epaint_default_fonts")
        .map(|package| package.dir.clone())
        .ok_or_else(|| "epaint_default_fonts is not in Cargo.lock".to_owned())
}

pub(crate) fn lay_out(folder: &Path, title: &str, shipped: &[Shipped], fonts: bool) -> Result<(), String> {
    let root = root();
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let copy = |from: &Path, to: &Path| {
        std::fs::copy(from, to).map(|_| ()).map_err(|error| format!("{}: {error}", from.display()))
    };
    for file in PROJECT_FILES {
        copy(&root.join(file), &folder.join(file))?;
    }
    std::fs::write(folder.join(NOTICES_FILE), notices_text(title, shipped)?).map_err(|error| error.to_string())?;
    copy(&rust_copyright()?, &folder.join(RUST_COPYRIGHT_FILE))?;
    if fonts {
        let target = folder.join("fonts");
        std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        for file in PROJECT_FONT_LICENCES {
            copy(&root.join("assets/fonts").join(file), &target.join(file))?;
        }
        let egui = egui_fonts_dir()?;
        for (from, to) in EGUI_FONT_LICENCES {
            copy(&egui.join(from), &target.join(to))?;
        }
    }
    Ok(())
}

pub(crate) fn required_files(fonts: bool) -> Vec<String> {
    let mut files: Vec<String> = PROJECT_FILES.iter().map(|file| (*file).to_owned()).collect();
    files.push(NOTICES_FILE.to_owned());
    files.push(RUST_COPYRIGHT_FILE.to_owned());
    if fonts {
        files.extend(PROJECT_FONT_LICENCES.iter().map(|file| format!("fonts/{file}")));
        files.extend(EGUI_FONT_LICENCES.iter().map(|(_, file)| format!("fonts/{file}")));
    }
    files
}

pub(crate) fn wasm_section(name: &str, body: &[u8]) -> Vec<u8> {
    let mut payload = leb128(name.len() as u64);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(body);
    let mut section = vec![0u8];
    section.extend(leb128(payload.len() as u64));
    section.extend(payload);
    section
}

fn leb128(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            return bytes;
        }
        bytes.push(byte | 0x80);
    }
}

fn read_leb128(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

pub(crate) fn wasm_custom_section<'a>(module: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    if module.get(..8)? != b"\0asm\x01\0\0\0" {
        return None;
    }
    let mut at = 8;
    while at < module.len() {
        let id = module[at];
        at += 1;
        let size = usize::try_from(read_leb128(module, &mut at)?).ok()?;
        let end = at.checked_add(size)?;
        let section = module.get(at..end)?;
        if id == 0 {
            let mut inner = 0;
            let length = usize::try_from(read_leb128(section, &mut inner)?).ok()?;
            let name = section.get(inner..inner.checked_add(length)?)?;
            if name == wanted.as_bytes() {
                return section.get(inner + length..);
            }
        }
        at = end;
    }
    None
}

fn with_project_licences(mut text: String) -> Result<String, String> {
    for file in PROJECT_FILES {
        let body = read_text(&root().join(file))?;
        text.push_str(&format!("{}\nZen IDE: {file}\n{}\n{}\n\n", "=".repeat(78), "-".repeat(78), body.trim_end()));
    }
    Ok(text)
}

pub(crate) fn plugin_notices(id: &str) -> Result<Vec<u8>, String> {
    let package = format!("zen-plugin-{id}");
    let text = notices_text(&format!("the {package} plug-in"), &[Shipped::new(&package, Some(WASM))])?;
    let mut text = with_project_licences(text)?;
    text.push_str(&data_notices(&root().join("plugins").join(id))?);
    Ok(text.into_bytes())
}

pub(crate) fn data_notices(plugin: &Path) -> Result<String, String> {
    let file = plugin.join("schemas").join("NOTICES");
    let Ok(notices) = std::fs::read_to_string(&file) else { return Ok(String::new()) };
    let mut out = format!("\n\n{notices}");
    let mut licences: Vec<&str> =
        notices.lines().filter_map(|line| line.strip_prefix("Licence: ")).map(str::trim).collect();
    licences.sort_unstable();
    licences.dedup();
    for licence in licences {
        let text = root().join("licenses/texts").join(format!("{licence}.txt"));
        let text = std::fs::read_to_string(&text)
            .map_err(|error| format!("{}: licence {licence} has no text ({error})", file.display()))?;
        out.push_str(&format!("\n\n{licence}\n\n{text}"));
    }
    Ok(out)
}

pub(crate) fn add_to_module(module: &Path, id: &str) -> Result<(), String> {
    let mut bytes = std::fs::read(module).map_err(|error| format!("{}: {error}", module.display()))?;
    if wasm_custom_section(&bytes, WASM_SECTION).is_some() {
        return Ok(());
    }
    bytes.extend(wasm_section(WASM_SECTION, &plugin_notices(id)?));
    std::fs::write(module, bytes).map_err(|error| format!("{}: {error}", module.display()))
}

pub(crate) fn driver_notices_file(driver: &str, target: Option<&str>, dir: &Path) -> Result<PathBuf, String> {
    let package = format!("zen-db-{driver}");
    let text = notices_text(&format!("the {package} driver program"), &[Shipped::new(&package, target)])?;
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{package}-{}.txt", target.unwrap_or("host")));
    std::fs::write(&path, with_project_licences(text)?).map_err(|error| error.to_string())?;
    Ok(std::path::absolute(&path).unwrap_or(path))
}

pub(crate) const DRIVER_NOTICES_ENV: &str = "ZEN_THIRD_PARTY_NOTICES";

pub(crate) fn artefact_problem(path: &Path, role: &str) -> Result<Option<String>, String> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let read = || std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()));
    let found = if name.ends_with(".tar.gz") {
        tar_entries(&read()?)?
    } else if name.ends_with(".zip") {
        zip_entries(path)?
    } else if name.ends_with(".deb") {
        let entries = deb_entries(&read()?)?;
        if !entries.iter().any(|entry| entry.starts_with("usr/share/doc/") && entry.ends_with("/copyright")) {
            return Ok(Some(format!("{name} has no /usr/share/doc/<package>/copyright")));
        }
        entries
    } else if name.ends_with(".wasm") {
        let module = read()?;
        let section = wasm_custom_section(&module, WASM_SECTION).unwrap_or_default();
        return Ok((!section.starts_with(HEADER.as_bytes()))
            .then(|| format!("{name} has no `{WASM_SECTION}` custom section with its notices")));
    } else if role == "driver" {
        let program = read()?;
        let marker = format!("{HEADER}the zen-db-");
        return Ok((!program.windows(marker.len()).any(|window| window == marker.as_bytes()))
            .then(|| format!("{name} carries no third-party notices (`--licences` prints them)")));
    } else {
        return Ok(None);
    };
    let missing: Vec<String> = required_files(true)
        .into_iter()
        .filter(|file| {
            let wanted = format!("{LICENCES_FOLDER}/{file}");
            !found.iter().any(|entry| entry == &wanted || entry.ends_with(&format!("/{wanted}")))
        })
        .collect();
    Ok((!missing.is_empty()).then(|| format!("{name} lacks {LICENCES_FOLDER}/{}", missing.join(", "))))
}

fn clean_entry(path: &str) -> String {
    path.trim_start_matches("./").trim_end_matches('/').to_owned()
}

fn tar_entries(gzipped: &[u8]) -> Result<Vec<String>, String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(gzipped));
    let mut names = Vec::new();
    for entry in archive.entries().map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path().map_err(|error| error.to_string())?;
        names.push(clean_entry(&path.to_string_lossy()));
    }
    Ok(names)
}

fn zip_entries(path: &Path) -> Result<Vec<String>, String> {
    let output =
        Command::new("unzip").arg("-Z1").arg(path).output().map_err(|error| format!("cannot run unzip: {error}"))?;
    if !output.status.success() {
        return Err(format!("unzip cannot read {}", path.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(clean_entry).collect())
}

fn deb_entries(deb: &[u8]) -> Result<Vec<String>, String> {
    let mut at = 8;
    if deb.get(..8) != Some(b"!<arch>\n".as_slice()) {
        return Err("not a .deb (ar) archive".to_owned());
    }
    while at + 60 <= deb.len() {
        let header = &deb[at..at + 60];
        let name = String::from_utf8_lossy(&header[..16]).trim().trim_end_matches('/').to_owned();
        let size: usize = String::from_utf8_lossy(&header[48..58]).trim().parse().map_err(|_| "a bad ar header")?;
        let body = deb.get(at + 60..at + 60 + size).ok_or("a truncated .deb")?;
        if name == "data.tar.gz" {
            return tar_entries(body);
        }
        at += 60 + size + size % 2;
    }
    Err("the .deb has no data.tar.gz".to_owned())
}

pub(crate) fn command(args: &[String]) -> Result<(), String> {
    let usage = "usage: cargo xtask notices                      check every shipped program (CI runs it as a test)\n       \
                 cargo xtask notices <product> [<target>] <file>  write the notices of an IDE or toolbox";
    match args {
        [] => {
            let notices = check()?;
            println!("Every shipped crate has a licence text ({} crates).", notices.crates);
            Ok(())
        }
        [product, file] => write(product, None, file),
        [product, target, file] => write(product, Some(target), file),
        _ => Err(usage.to_owned()),
    }
}

fn write(product: &str, target: Option<&str>, file: &str) -> Result<(), String> {
    let title = product.to_owned();
    let text = notices_text(&title, &product_shipped(product, target)?)?;
    std::fs::write(file, text).map_err(|error| format!("{file}: {error}"))?;
    println!("Wrote {file}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use super::{Override, Package, data_notices, render, root, wasm_custom_section, wasm_section};

    #[test]
    fn every_shipped_schema_has_its_notices() {
        for plugin in ["gitlab-ci", "yaml"] {
            let folder = root().join("plugins").join(plugin);
            let text = data_notices(&folder).unwrap();
            assert!(text.contains("Source: https://") && text.contains("Licence: "), "{plugin}");
            assert!(text.contains("Permission is hereby granted") || text.contains("Apache License"), "{plugin}");
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(folder.join("plugin.json")).unwrap()).unwrap();
            for schema in manifest["schemas"].as_array().unwrap() {
                let entry = schema["entry"].as_str().unwrap();
                assert!(text.contains(entry), "{plugin}: {entry} is in its NOTICES");
            }
        }
    }

    fn package(dir: &std::path::Path, name: &str, license: &str) -> Package {
        Package {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            license: license.to_owned(),
            repository: String::new(),
            dir: dir.to_path_buf(),
            license_file: None,
        }
    }

    #[test]
    fn every_shipped_crate_has_a_licence_text() {
        let notices = super::check().unwrap_or_else(|error| panic!("{error}"));
        assert!(notices.crates > 100, "only {} crates found", notices.crates);
    }

    #[test]
    fn a_crate_without_a_licence_file_is_missing_until_licenses_crates_json_names_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("licenses/texts")).unwrap();
        std::fs::write(root.path().join("licenses/texts/MIT.txt"), "Permission is hereby granted").unwrap();
        let with_file = root.path().join("a");
        let without = root.path().join("b");
        std::fs::create_dir_all(&with_file).unwrap();
        std::fs::create_dir_all(&without).unwrap();
        std::fs::write(with_file.join("LICENSE-MIT"), "Copyright (c) A\n\nPermission is hereby granted").unwrap();
        std::fs::write(with_file.join("NOTICE"), "A notice").unwrap();
        let packages = vec![package(&with_file, "a", "MIT"), package(&without, "b", "MIT")];
        let targets = BTreeSet::new();

        let notices = render("Test", packages.clone(), &targets, &BTreeMap::new(), root.path()).unwrap();
        assert_eq!(notices.missing, ["b 1.0.0 (MIT)"]);
        assert!(notices.text.contains("Copyright (c) A") && notices.text.contains("A notice"));

        let rule =
            Override { licence: Some("MIT".to_owned()), copyright: Some("Copyright (c) B".to_owned()), files: vec![] };
        let rules = BTreeMap::from([("b".to_owned(), rule)]);
        let notices = render("Test", packages, &targets, &rules, root.path()).unwrap();
        assert!(notices.missing.is_empty());
        assert!(notices.text.contains("Copyright (c) B\n\nPermission is hereby granted"));
        assert!(notices.used_overrides.contains("b"));
    }

    #[test]
    fn identical_licence_texts_are_written_once() {
        let root = tempfile::tempdir().unwrap();
        let dirs: Vec<PathBuf> = ["a", "b"].iter().map(|name| root.path().join(name)).collect();
        for dir in &dirs {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("LICENSE-APACHE"), "Apache License\n   Version 2.0").unwrap();
        }
        let packages = vec![package(&dirs[0], "a", "Apache-2.0"), package(&dirs[1], "b", "Apache-2.0")];
        let notices = render("Test", packages, &BTreeSet::new(), &BTreeMap::new(), root.path()).unwrap();
        assert_eq!(notices.text.matches("Apache License\n   Version 2.0").count(), 1);
        assert!(notices.text.contains("b 1.0.0: LICENSE-APACHE"));
    }

    #[test]
    fn the_notices_ride_in_a_custom_section_of_the_plugin_module() {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        module.extend([1, 1, 0]);
        assert_eq!(wasm_custom_section(&module, "zen-licences"), None);
        let body = "x".repeat(300);
        module.extend(wasm_section("zen-licences", body.as_bytes()));
        assert_eq!(wasm_custom_section(&module, "zen-licences"), Some(body.as_bytes()));
        assert_eq!(wasm_custom_section(b"not wasm", "zen-licences"), None);
    }

    fn targz(entries: &[String]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, entry, &b"x"[..]).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn a_release_artefact_without_its_licences_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let inside: Vec<String> = super::required_files(true)
            .iter()
            .map(|file| format!("GoGarden.app/Contents/Resources/Licenses/{file}"))
            .collect();
        let archive = dir.path().join("gogarden-26.11.1-macos-aarch64.tar.gz");
        std::fs::write(&archive, targz(&inside)).unwrap();
        assert_eq!(super::artefact_problem(&archive, "update").unwrap(), None);

        std::fs::write(&archive, targz(&inside[1..])).unwrap();
        let problem = super::artefact_problem(&archive, "update").unwrap().unwrap();
        assert!(problem.contains("Licenses/LICENSE-MIT"), "{problem}");

        let module = dir.path().join("zen-plugin-json-0.1.0.wasm");
        std::fs::write(&module, b"\0asm\x01\0\0\0").unwrap();
        assert!(super::artefact_problem(&module, "module").unwrap().is_some());
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend(wasm_section("zen-licences", b"Third-party notices for the zen-plugin-json plug-in"));
        std::fs::write(&module, bytes).unwrap();
        assert_eq!(super::artefact_problem(&module, "module").unwrap(), None);

        let program = dir.path().join("zen-db-duckdb-0.1.0-linux-x86_64");
        std::fs::write(&program, b"\x7fELF no notices").unwrap();
        assert!(super::artefact_problem(&program, "driver").unwrap().is_some());
        std::fs::write(&program, b"\x7fELF Third-party notices for the zen-db-duckdb driver program").unwrap();
        assert_eq!(super::artefact_problem(&program, "driver").unwrap(), None);

        let checksum = dir.path().join("Zen-Toolbox.dmg.sha256");
        std::fs::write(&checksum, "00").unwrap();
        assert_eq!(super::artefact_problem(&checksum, "installer-checksum").unwrap(), None);
    }

    #[test]
    fn the_licences_folder_holds_the_project_licences_the_notices_and_the_font_licences() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Licenses");
        super::lay_out(&folder, "Zen Toolbox", &super::product_shipped("toolbox", None).unwrap(), true).unwrap();
        for file in super::required_files(true) {
            assert!(folder.join(&file).is_file(), "no {file}");
        }
        let notices = std::fs::read_to_string(folder.join("THIRD-PARTY-NOTICES")).unwrap();
        assert!(notices.starts_with("Third-party notices for Zen Toolbox\n"));
        assert!(notices.contains("  egui 0.37.0, MIT OR Apache-2.0"));
        assert!(notices.contains("epaint_default_fonts 0.37.0: fonts/UFL.txt"));
    }
}
