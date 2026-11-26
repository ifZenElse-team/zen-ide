use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::native::Platform;
use crate::release::{Gate, IDES};
use crate::release_ci::{BUILD_DIR, BuiltRelease, PLAN_FILE, Plan, SCHEMA};
use crate::release_flow::BuildRecord;

const PARTS_DIR: &str = "release-build/parts";

const GATE_FILE: &str = "release-gate.json";

const MATRIX_FILE: &str = "release-matrix.json";

const MACOS_TOGETHER: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GatePass {
    pub(crate) schema: u32,
    pub(crate) tag: String,
    pub(crate) commit: String,
    pub(crate) tree: String,
    pub(crate) rustc: String,
    pub(crate) gate: String,
}

pub(crate) fn matrix_json(plan: &Plan) -> serde_json::Value {
    let macos: Vec<serde_json::Value> = IDES
        .chunks(MACOS_TOGETHER)
        .map(|together| {
            let names: Vec<&str> = together.iter().map(|ide| ide.1).collect();
            let ids: Vec<&str> = together.iter().map(|ide| ide.0).collect();
            serde_json::json!({ "name": names.join(" and "), "parts": ids.join(" ") })
        })
        .collect();
    let mut cross = Vec::new();
    for (platform, system) in [("linux", "Linux"), ("windows", "Windows")] {
        let toolbox = plan.toolbox.is_some().then_some(("toolbox", "Zen Toolbox"));
        for (id, name) in IDES.iter().map(|ide| (ide.0, ide.1)).chain(toolbox) {
            cross.push(serde_json::json!({ "name": format!("{name} for {system}"), "part": id, "platform": platform }));
        }
    }
    let mut extras = Vec::new();
    if plan.toolbox.is_some() {
        extras.push("toolbox");
    }
    if !plan.plugins.is_empty() {
        extras.push("plugins");
    }
    serde_json::json!({ "macos": macos, "cross": cross, "extras": extras.join(" ") })
}

pub(crate) fn matrix(tag: &str, version: &str) -> Result<(), String> {
    let plan = crate::release_ci::plan(version)?;
    println!("==> The release of {tag}:\n{}", crate::release_ci::describe(&plan));
    if let Some(problem) = crate::unreleased::release_problem(Path::new("."), version, plan.toolbox.as_deref()) {
        return Err(format!("{tag} is not released, nothing was built: {problem}"));
    }
    let json = serde_json::to_string(&matrix_json(&plan)).map_err(|error| error.to_string())?;
    std::fs::write(MATRIX_FILE, &json).map_err(|error| format!("{MATRIX_FILE}: {error}"))?;
    println!("==> The jobs that build it are in {MATRIX_FILE}:\n{json}");
    Ok(())
}

fn here(version: &str) -> BuildRecord {
    BuildRecord::new("gate", "gate", "gate", version, String::new())
}

pub(crate) fn gate(tag: &str, version: &str) -> Result<(), String> {
    crate::release::gate()?;
    let here = here(version);
    let pass = GatePass {
        schema: SCHEMA,
        tag: tag.to_owned(),
        commit: here.commit,
        tree: here.tree,
        rustc: here.rustc,
        gate: Gate::Here.recorded().to_owned(),
    };
    std::fs::write(GATE_FILE, serde_json::to_vec_pretty(&pass).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{GATE_FILE}: {error}"))?;
    println!("\n==> The quality gate passed for {tag} (tree {}); {GATE_FILE} says so.", pass.tree);
    Ok(())
}

pub(crate) fn part(version: &str, what: &str, platform: &str) -> Result<(), String> {
    let parts = std::path::absolute(PARTS_DIR).map_err(|error| error.to_string())?;
    if what == "plugins" {
        let plan = crate::release_ci::plan(version)?;
        if plan.plugins.is_empty() {
            return Err("no plug-in of the catalog has a new version: there is nothing to build".to_owned());
        }
        let platforms = crate::native::platforms("all")?;
        for (id, plugin_version) in &plan.plugins {
            let folder = parts.join(format!("plugin-{id}-{plugin_version}"));
            crate::release::build_plugin(id, plugin_version, &platforms, &folder, Gate::Deferred)?;
        }
        return Ok(());
    }
    let [platform] = crate::native::platforms(platform)?[..] else {
        return Err(format!("a part is built for one platform (macos, linux or windows), not `{platform}`"));
    };
    let product = crate::cross::product(what)?;
    let version = if product.id == "toolbox" {
        crate::release_ci::plan(version)?
            .toolbox
            .ok_or("Zen Toolbox has no version that is not published: there is nothing to build")?
    } else {
        let published = crate::release::published_ide_versions()?;
        let month = crate::calendar::this_month()?;
        if let Some(problem) = crate::release::version_problem(version, &published, month, false) {
            return Err(format!("{problem}; nothing was built"));
        }
        version.to_owned()
    };
    let folder = crate::native::folder(&parts, product.id, &version, platform);
    match platform {
        Platform::Macos if product.id == "toolbox" => {
            crate::release::build_toolbox(&version, None, &folder, Gate::Deferred)
        }
        Platform::Macos => crate::release::build_ide(product.id, &version, None, &folder, Gate::Deferred),
        other => crate::native::build(&product, &version, other, &folder, Gate::Deferred),
    }
}

pub(crate) fn pass_problem(record: &BuildRecord, pass: &GatePass) -> Option<String> {
    let what = format!("{} {} ({})", record.product, record.version, record.platform);
    if record.commit != pass.commit || record.tree != pass.tree {
        return Some(format!(
            "{what} was built from commit {} (tree {}), the quality gate passed for {} (tree {})",
            record.commit, record.tree, pass.commit, pass.tree
        ));
    }
    (record.rustc != pass.rustc)
        .then(|| format!("{what} was built with {}, the quality gate ran with {}", record.rustc, pass.rustc))
}

fn system(platform: &str) -> &str {
    if crate::native::is_macos(platform) { "macos" } else { platform.split('-').next().unwrap_or(platform) }
}

pub(crate) fn expected(plan: &Plan) -> Vec<(String, String, String, String)> {
    let mut expected = Vec::new();
    let entry = |kind: &str, product: &str, version: &str, system: &str| {
        (kind.to_owned(), product.to_owned(), version.to_owned(), system.to_owned())
    };
    for system in ["macos", "linux", "windows"] {
        for ide in IDES {
            expected.push(entry("ide", ide.0, &plan.version, system));
        }
        if let Some(version) = &plan.toolbox {
            expected.push(entry("toolbox", "toolbox", version, system));
        }
    }
    for (id, version) in &plan.plugins {
        expected.push(entry("plugin", id, version, "wasm32"));
    }
    expected.sort();
    expected
}

pub(crate) fn parts_problem(plan: &Plan, records: &[BuildRecord]) -> Option<String> {
    let mut found: Vec<(String, String, String, String)> = records
        .iter()
        .map(|record| {
            (record.kind.clone(), record.product.clone(), record.version.clone(), system(&record.platform).to_owned())
        })
        .collect();
    found.sort();
    let expected = expected(plan);
    let name =
        |(_, product, version, system): &(String, String, String, String)| format!("{product} {version} ({system})");
    let missing: Vec<String> = expected.iter().filter(|part| !found.contains(part)).map(name).collect();
    let mut extra: Vec<String> = found.iter().filter(|part| !expected.contains(part)).map(name).collect();
    extra.extend(found.windows(2).filter(|pair| pair[0] == pair[1]).map(|pair| format!("{} twice", name(&pair[0]))));
    if missing.is_empty() && extra.is_empty() {
        return None;
    }
    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!("missing: {}", missing.join(", ")));
    }
    if !extra.is_empty() {
        problems.push(format!("not of this release: {}", extra.join(", ")));
    }
    Some(format!("the parts are not the release ({})", problems.join("; ")))
}

fn group(record: &BuildRecord, version: &str) -> String {
    match record.kind.as_str() {
        "ide" => format!("ides-{version}"),
        "toolbox" => "toolbox".to_owned(),
        _ => "plugins".to_owned(),
    }
}

pub(crate) fn groups(plan: &Plan) -> Vec<String> {
    let mut groups = vec![format!("builds/ides-{}", plan.version)];
    if plan.toolbox.is_some() {
        groups.push("builds/toolbox".to_owned());
    }
    groups.extend(plan.plugins.iter().map(|(id, version)| format!("builds/plugins/plugin-{id}-{version}")));
    groups
}

fn gate_pass(tag: &str, version: &str) -> Result<GatePass, String> {
    let bytes = std::fs::read(GATE_FILE)
        .map_err(|error| format!("{GATE_FILE}: {error}; the gate job leaves it (cargo xtask release-ci gate)"))?;
    let pass: GatePass = serde_json::from_slice(&bytes).map_err(|error| format!("{GATE_FILE}: {error}"))?;
    if pass.schema != SCHEMA || pass.tag != tag || pass.gate != Gate::Here.recorded() {
        return Err(format!("{GATE_FILE} is the gate of {} ({}), not a pass for {tag}", pass.tag, pass.gate));
    }
    let here = here(version);
    if (here.commit.as_str(), here.tree.as_str()) != (pass.commit.as_str(), pass.tree.as_str()) {
        return Err(format!(
            "{GATE_FILE} is a pass for commit {} (tree {}); this checkout is {} (tree {})",
            pass.commit, pass.tree, here.commit, here.tree
        ));
    }
    Ok(pass)
}

pub(crate) fn join(tag: &str, version: &str) -> Result<(), String> {
    let pass = gate_pass(tag, version)?;
    let plan = crate::release_ci::plan(version)?;
    let mut parts: Vec<(PathBuf, BuildRecord)> = Vec::new();
    for folder in crate::release_ci::build_folders(Path::new(PARTS_DIR))? {
        let record = crate::release_flow::verified_part(&folder)?;
        if let Some(problem) = pass_problem(&record, &pass) {
            return Err(format!("{problem}; nothing was joined"));
        }
        parts.push((folder, record));
    }
    let records: Vec<BuildRecord> = parts.iter().map(|(_, record)| record.clone()).collect();
    if let Some(problem) = parts_problem(&plan, &records) {
        return Err(format!("{problem}; nothing was joined"));
    }
    let ides: Vec<BuildRecord> = records.iter().filter(|record| record.kind == "ide").cloned().collect();
    if let Some(problem) = crate::release_flow::set_problem(&ides) {
        return Err(format!("{problem}; nothing was joined"));
    }
    let root = std::path::absolute(BUILD_DIR).map_err(|error| error.to_string())?;
    let builds = root.join("builds");
    crate::release_flow::clean_out(&builds)?;
    println!("==> {} parts of {tag}, each checked against its build.json and the gate's pass", parts.len());
    for (folder, mut record) in parts {
        let name = folder.file_name().ok_or("a part without a name")?.to_owned();
        let to = builds.join(group(&record, version)).join(&name);
        std::fs::create_dir_all(to.parent().unwrap_or(&builds)).map_err(|error| error.to_string())?;
        std::fs::rename(&folder, &to).map_err(|error| format!("{}: {error}", folder.display()))?;
        record.gate = pass.gate.clone();
        crate::release_flow::save_record(&to, &record)?;
        crate::release_flow::verified(&to)?;
        println!("  {}", to.strip_prefix(&root).unwrap_or(&to).display());
    }
    println!("==> The website");
    crate::release_ci::website(&root.join("site"))?;
    let built =
        BuiltRelease { schema: SCHEMA, tag: tag.to_owned(), version: version.to_owned(), groups: groups(&plan) };
    std::fs::write(PLAN_FILE, serde_json::to_vec_pretty(&built).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    println!("\n==> {tag} is joined into {BUILD_DIR}/; nothing is uploaded yet.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(toolbox: Option<&str>, plugins: &[(&str, &str)]) -> Plan {
        Plan {
            version: "26.11.1".to_owned(),
            toolbox: toolbox.map(str::to_owned),
            plugins: plugins.iter().map(|(id, version)| ((*id).to_owned(), (*version).to_owned())).collect(),
        }
    }

    fn record(kind: &str, product: &str, version: &str, platform: &str) -> BuildRecord {
        let mut record = BuildRecord::new(kind, product, product, version, String::new());
        record.platform = platform.to_owned();
        record.commit = "c0ffee".to_owned();
        record.tree = "7ree".to_owned();
        record.rustc = "rustc 1.99.1".to_owned();
        record.gate = Gate::Deferred.recorded().to_owned();
        record
    }

    fn release(plan: &Plan) -> Vec<BuildRecord> {
        expected(plan)
            .iter()
            .map(|(kind, product, version, system)| {
                let platform = match system.as_str() {
                    "macos" => "macos-aarch64",
                    "linux" => "linux-x86_64",
                    "windows" => "windows-x86_64",
                    other => other,
                };
                record(kind, product, version, platform)
            })
            .collect()
    }

    #[test]
    fn the_jobs_are_three_macos_pairs_and_one_job_for_every_other_ide_and_platform() {
        let ides_only = matrix_json(&planned(None, &[]));
        let macos = ides_only["macos"].as_array().unwrap();
        assert_eq!(macos.len(), IDES.len().div_ceil(MACOS_TOGETHER));
        assert_eq!(macos[0]["parts"], "gogarden rustronin");
        assert_eq!(macos[0]["name"], "GoGarden and RustRonin");
        let built: Vec<&str> = macos.iter().flat_map(|job| job["parts"].as_str().unwrap().split(' ')).collect();
        assert_eq!(built, IDES.iter().map(|ide| ide.0).collect::<Vec<_>>(), "every IDE once");
        assert_eq!(ides_only["cross"].as_array().unwrap().len(), IDES.len() * 2);
        assert_eq!(
            ides_only["cross"][0],
            serde_json::json!({ "name": "GoGarden for Linux", "part": "gogarden", "platform": "linux" })
        );
        assert_eq!(ides_only["extras"], "");

        let everything = matrix_json(&planned(Some("0.3.21"), &[("json", "0.1.3")]));
        assert_eq!(everything["cross"].as_array().unwrap().len(), (IDES.len() + 1) * 2);
        assert_eq!(everything["extras"], "toolbox plugins");
        assert_eq!(matrix_json(&planned(None, &[("json", "0.1.3")]))["extras"], "plugins");
        assert!(!serde_json::to_string(&everything).unwrap().contains('\n'), "one line, for the job's output");
    }

    #[test]
    fn the_parts_must_be_exactly_the_release() {
        let plan = planned(Some("0.3.21"), &[("json", "0.1.3")]);
        let all = release(&plan);
        assert_eq!(all.len(), IDES.len() * 3 + 3 + 1);
        assert_eq!(parts_problem(&plan, &all), None);

        let mut without = all.clone();
        without.retain(|record| !(record.product == "webwave" && record.platform == "linux-x86_64"));
        assert!(parts_problem(&plan, &without).unwrap().contains("missing: webwave 26.11.1 (linux)"));

        let mut other_version = all.clone();
        other_version[0].version = "26.11.2".to_owned();
        let problem = parts_problem(&plan, &other_version).unwrap();
        assert!(problem.contains("missing: ") && problem.contains("not of this release: "), "{problem}");

        let mut twice = all.clone();
        twice.push(all[0].clone());
        assert!(parts_problem(&plan, &twice).unwrap().contains("twice"));

        let unplanned = release(&planned(Some("0.3.21"), &[]));
        let mut with_plugin = unplanned.clone();
        with_plugin.push(record("plugin", "json", "0.1.3", "wasm32"));
        assert_eq!(parts_problem(&planned(Some("0.3.21"), &[]), &unplanned), None);
        assert!(parts_problem(&planned(Some("0.3.21"), &[]), &with_plugin).unwrap().contains("json 0.1.3"));
    }

    #[test]
    fn a_part_is_taken_only_with_the_gates_pass_for_its_tree_and_compiler() {
        let pass = GatePass {
            schema: SCHEMA,
            tag: "v26.11.1".to_owned(),
            commit: "c0ffee".to_owned(),
            tree: "7ree".to_owned(),
            rustc: "rustc 1.99.1".to_owned(),
            gate: "passed".to_owned(),
        };
        let part = record("ide", "gogarden", "26.11.1", "linux-x86_64");
        assert_eq!(pass_problem(&part, &pass), None);
        let mut other_tree = part.clone();
        other_tree.tree = "0ther".to_owned();
        assert!(pass_problem(&other_tree, &pass).unwrap().contains("the quality gate passed for c0ffee"));
        let mut other_commit = part.clone();
        other_commit.commit = "deadbeef".to_owned();
        assert!(pass_problem(&other_commit, &pass).is_some());
        let mut other_compiler = part;
        other_compiler.rustc = "rustc 1.99.2".to_owned();
        assert!(pass_problem(&other_compiler, &pass).unwrap().contains("rustc 1.99.2"));
    }

    #[test]
    fn the_joined_folders_are_the_ones_the_stage_reads() {
        assert_eq!(groups(&planned(None, &[])), ["builds/ides-26.11.1"]);
        assert_eq!(
            groups(&planned(Some("0.3.21"), &[("json", "0.1.3"), ("duckdb", "0.1.1")])),
            [
                "builds/ides-26.11.1",
                "builds/toolbox",
                "builds/plugins/plugin-json-0.1.3",
                "builds/plugins/plugin-duckdb-0.1.1"
            ]
        );
        assert_eq!(group(&record("ide", "gogarden", "26.11.1", "linux-x86_64"), "26.11.1"), "ides-26.11.1");
        assert_eq!(group(&record("toolbox", "toolbox", "0.3.21", "macos-aarch64"), "26.11.1"), "toolbox");
        assert_eq!(group(&record("plugin", "json", "0.1.3", "wasm32"), "26.11.1"), "plugins");
    }

    #[test]
    fn a_deferred_part_is_refused_by_the_publisher_until_it_is_joined() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.tar.gz"), b"archive").unwrap();
        let mut part = record("ide", "gogarden", "26.11.1", "linux-x86_64");
        part.files = vec![crate::release_flow::BuiltFile {
            name: "a.tar.gz".to_owned(),
            role: "update".to_owned(),
            size: 7,
            sha256: crate::release_flow::sha256(&dir.path().join("a.tar.gz")).unwrap(),
        }];
        crate::release_flow::save_record(dir.path(), &part).unwrap();
        assert_eq!(crate::release_flow::verified_part(dir.path()).unwrap(), part);
        assert!(crate::release_flow::verified(dir.path()).unwrap_err().contains("it must have passed"));

        std::fs::write(dir.path().join("a.tar.gz"), b"archivo").unwrap();
        assert!(crate::release_flow::verified_part(dir.path()).unwrap_err().contains("SHA-256"), "checksums first");

        std::fs::write(dir.path().join("a.tar.gz"), b"archive").unwrap();
        part.gate = "passed".to_owned();
        crate::release_flow::save_record(dir.path(), &part).unwrap();
        assert!(crate::release_flow::verified_part(dir.path()).unwrap_err().contains("deferred"), "not a part");
        assert!(crate::release_flow::verified(dir.path()).is_ok());
    }
}
