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

pub(crate) fn matrix(tag: &str, version: &str, dry_run: bool, ci: &crate::ci_host::Ci) -> Result<(), String> {
    let plan = crate::release_ci::plan(version)?;
    let what = if dry_run { "A dry run of the release" } else { "The release" };
    println!("==> {what} of {tag}:\n{}", crate::release_ci::describe(&plan));
    match crate::unreleased::release_problem(Path::new("."), version, plan.toolbox.as_deref()) {
        Some(problem) if dry_run => println!(
            "==> A release of this commit would be refused now: {problem}\n    The dry run builds it anyway: its \
             builds record the gate as skipped, and nothing publishes them."
        ),
        Some(problem) => return Err(format!("{tag} is not released, nothing was built: {problem}")),
        None => {}
    }
    if !dry_run {
        preflight(tag, version, ci)?;
    }
    let mut matrix = matrix_json(&plan);
    matrix["tag"] = serde_json::json!(tag);
    let json = serde_json::to_string(&matrix).map_err(|error| error.to_string())?;
    std::fs::write(MATRIX_FILE, &json).map_err(|error| format!("{MATRIX_FILE}: {error}"))?;
    println!("==> The jobs that build it are in {MATRIX_FILE}:\n{json}");
    Ok(())
}

fn preflight(tag: &str, version: &str, ci: &crate::ci_host::Ci) -> Result<(), String> {
    let refused = |problem: String| format!("{tag} is not released, nothing was built: {problem}");
    let published = crate::release::published_ide_versions()?;
    if let Some(problem) = crate::release::version_problem(version, &published, crate::calendar::this_month()?, false) {
        return Err(refused(problem));
    }
    crate::release::check_source()?;
    if ci.host != crate::ci_host::Host::Github || std::env::var("ZEN_RELEASE_FROM_GITHUB").as_deref() != Ok("true") {
        return Ok(());
    }
    if let Some(problem) = ci.secrets_problem() {
        return Err(refused(format!("the stage would be refused the release secrets: {problem}")));
    }
    let environment = github_api(&format!("environments/{ENVIRONMENT}"))?;
    let custom = environment["deployment_branch_policy"]["custom_branch_policies"] == true;
    let policies = if custom {
        Some(github_api(&format!("environments/{ENVIRONMENT}/deployment-branch-policies"))?)
    } else {
        None
    };
    if let Some(problem) = environment_problem(&environment, policies.as_ref(), "tag", tag) {
        return Err(refused(problem));
    }
    println!(
        "==> Before building: {tag} is the calendar's next version, its commit is on origin/main, its ref is \
         protected, and the environment `{ENVIRONMENT}` has required reviewers and allows it"
    );
    Ok(())
}

const ENVIRONMENT: &str = "release";

fn github_api(path: &str) -> Result<serde_json::Value, String> {
    let repository = std::env::var("GITHUB_REPOSITORY").map_err(|_| "GITHUB_REPOSITORY is not set".to_owned())?;
    let output = std::process::Command::new("gh")
        .args(["api", &format!("repos/{repository}/{path}")])
        .output()
        .map_err(|error| format!("cannot run gh: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "GitHub did not answer repos/{repository}/{path} ({}): the environment `{ENVIRONMENT}` must exist, and \
             the job needs `actions: read`; nothing was built",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("{path}: {error}"))
}

pub(crate) fn environment_problem(
    environment: &serde_json::Value,
    policies: Option<&serde_json::Value>,
    kind: &str,
    name: &str,
) -> Option<String> {
    let reviewed = environment["protection_rules"].as_array().into_iter().flatten().any(|rule| {
        rule["type"] == "required_reviewers" && rule["reviewers"].as_array().is_some_and(|people| !people.is_empty())
    });
    if !reviewed {
        return Some(format!(
            "the environment `{ENVIRONMENT}` has no required reviewers, so nothing would ask before the secrets are \
             used (docs/releasing.md)"
        ));
    }
    let policy = &environment["deployment_branch_policy"];
    if policy.is_null() {
        return Some(format!(
            "the environment `{ENVIRONMENT}` allows every branch and tag; it must allow only the refs that release \
             (docs/releasing.md)"
        ));
    }
    if policy["custom_branch_policies"] == true {
        let allowed =
            policies.and_then(|policies| policies["branch_policies"].as_array()).into_iter().flatten().any(|entry| {
                entry["type"].as_str().unwrap_or("branch") == kind
                    && matches_ref(entry["name"].as_str().unwrap_or_default(), name)
            });
        return (!allowed).then(|| format!("the environment `{ENVIRONMENT}` does not allow the {kind} {name}"));
    }
    (policy["protected_branches"] == true && kind != "branch")
        .then(|| format!("the environment `{ENVIRONMENT}` allows protected branches only, not the {kind} {name}"))
}

pub(crate) fn matches_ref(pattern: &str, name: &str) -> bool {
    fn at(pattern: &[char], name: &[char]) -> bool {
        match pattern {
            [] => name.is_empty(),
            ['*', '*', rest @ ..] => (0..=name.len()).any(|skip| at(rest, &name[skip..])),
            ['*', rest @ ..] => (0..=name.len())
                .take_while(|skip| *skip == 0 || name[skip - 1] != '/')
                .any(|skip| at(rest, &name[skip..])),
            ['?', rest @ ..] => name.first().is_some_and(|c| *c != '/') && at(rest, &name[1..]),
            [c, rest @ ..] => name.first() == Some(c) && at(rest, &name[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    at(&pattern, &name)
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

pub(crate) fn part(version: &str, what: &str, platform: &str, dry_run: bool) -> Result<(), String> {
    let parts = std::path::absolute(PARTS_DIR).map_err(|error| error.to_string())?;
    let gate = part_gate(dry_run);
    if dry_run {
        crate::release::check_source()?;
        println!(
            "==> A dry run: the source check passed; the quality gate is not run here, and build.json records it as \
             `{}`, which nothing publishes",
            gate.recorded()
        );
    }
    if what == "plugins" {
        let plan = crate::release_ci::plan(version)?;
        if plan.plugins.is_empty() {
            return Err("no plug-in of the catalog has a new version: there is nothing to build".to_owned());
        }
        let platforms = crate::native::platforms("all")?;
        for (id, plugin_version) in &plan.plugins {
            let folder = parts.join(format!("plugin-{id}-{plugin_version}"));
            crate::release::build_plugin(id, plugin_version, &platforms, &folder, gate)?;
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
    let notes =
        part_notes(dry_run, crate::changelog::notes(&crate::changelog::product_path(product.id), &version).is_ok());
    match platform {
        Platform::Macos if product.id == "toolbox" => crate::release::build_toolbox(&version, notes, &folder, gate),
        Platform::Macos => crate::release::build_ide(product.id, &version, notes, &folder, gate),
        other => crate::native::build(&product, &version, other, &folder, gate, notes),
    }
}

pub(crate) fn part_gate(dry_run: bool) -> Gate {
    if dry_run { Gate::Trial } else { Gate::Deferred }
}

pub(crate) fn part_notes(dry_run: bool, has_section: bool) -> Option<&'static str> {
    (dry_run && !has_section).then_some(crate::release::TRIAL_NOTES)
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

fn dry_run_reference(tag: &str, version: &str) -> GatePass {
    let here = here(version);
    GatePass {
        schema: SCHEMA,
        tag: tag.to_owned(),
        commit: here.commit,
        tree: here.tree,
        rustc: here.rustc,
        gate: Gate::Trial.recorded().to_owned(),
    }
}

pub(crate) fn join(tag: &str, version: &str, dry_run: bool) -> Result<(), String> {
    let pass = if dry_run { dry_run_reference(tag, version) } else { gate_pass(tag, version)? };
    let gate = part_gate(dry_run);
    let plan = crate::release_ci::plan(version)?;
    let mut parts: Vec<(PathBuf, BuildRecord)> = Vec::new();
    for folder in crate::release_ci::build_folders(Path::new(PARTS_DIR))? {
        let record = crate::release_flow::verified_part(&folder, gate)?;
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
        if dry_run {
            crate::release_flow::verified_part(&to, gate)?;
        } else {
            crate::release_flow::verified(&to)?;
        }
        println!("  {}", to.strip_prefix(&root).unwrap_or(&to).display());
    }
    println!("==> The website");
    crate::release_ci::website(&root.join("site"))?;
    let built =
        BuiltRelease { schema: SCHEMA, tag: tag.to_owned(), version: version.to_owned(), groups: groups(&plan) };
    std::fs::write(PLAN_FILE, serde_json::to_vec_pretty(&built).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    if dry_run {
        println!(
            "\n==> The dry run of {tag} is joined into {BUILD_DIR}/: every part matches its build.json, all are of \
             commit {} with {}, and they are exactly the release. Their gate is recorded as `{}`: nothing publishes \
             them.",
            pass.commit,
            pass.rustc,
            gate.recorded()
        );
    } else {
        println!("\n==> {tag} is joined into {BUILD_DIR}/; nothing is uploaded yet.");
    }
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
    fn a_dry_run_builds_with_the_trial_gate_and_its_notes_when_the_section_is_missing() {
        assert_eq!(part_gate(false), Gate::Deferred);
        assert_eq!(part_gate(true), Gate::Trial);
        assert_eq!(part_gate(true).recorded(), "skipped");
        assert_eq!(part_notes(false, false), None, "a release reads its section, and fails without one");
        assert_eq!(part_notes(false, true), None);
        assert_eq!(part_notes(true, true), None, "a dry run reads the section when it is there");
        assert_eq!(part_notes(true, false), Some(crate::release::TRIAL_NOTES));
    }

    #[test]
    fn a_dry_runs_part_is_joined_only_as_a_dry_run_and_never_published() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.tar.gz"), b"archive").unwrap();
        let mut part = record("ide", "gogarden", "26.11.1", "linux-x86_64");
        part.gate = Gate::Trial.recorded().to_owned();
        part.files = vec![crate::release_flow::BuiltFile {
            name: "a.tar.gz".to_owned(),
            role: "update".to_owned(),
            size: 7,
            sha256: crate::release_flow::sha256(&dir.path().join("a.tar.gz")).unwrap(),
        }];
        crate::release_flow::save_record(dir.path(), &part).unwrap();
        assert_eq!(crate::release_flow::verified_part(dir.path(), Gate::Trial).unwrap(), part);
        let release = crate::release_flow::verified_part(dir.path(), Gate::Deferred).unwrap_err();
        assert!(release.contains("a release built in CI records it as deferred"), "{release}");
        assert!(crate::release_flow::verified(dir.path()).unwrap_err().contains("it must have passed"));

        part.gate = Gate::Deferred.recorded().to_owned();
        crate::release_flow::save_record(dir.path(), &part).unwrap();
        let dry = crate::release_flow::verified_part(dir.path(), Gate::Trial).unwrap_err();
        assert!(
            dry.contains("a part of a dry run records it as skipped"),
            "a release's part is not a dry run's: {dry}"
        );

        let reference = GatePass {
            schema: SCHEMA,
            tag: "v26.11.1".to_owned(),
            commit: "c0ffee".to_owned(),
            tree: "7ree".to_owned(),
            rustc: "rustc 1.99.1".to_owned(),
            gate: Gate::Trial.recorded().to_owned(),
        };
        assert_eq!(pass_problem(&part, &reference), None);
        part.commit = "0ther".to_owned();
        assert!(pass_problem(&part, &reference).is_some(), "every part of a dry run is of the join's commit");
    }

    fn environment(rules: serde_json::Value, policy: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "name": "release", "protection_rules": rules, "deployment_branch_policy": policy })
    }

    fn reviewers() -> serde_json::Value {
        serde_json::json!([{ "type": "required_reviewers", "reviewers": [{ "type": "User", "reviewer": { "login": "x" } }] }])
    }

    #[test]
    fn a_release_starts_only_when_its_environment_has_reviewers_and_allows_its_ref() {
        let custom = serde_json::json!({ "protected_branches": false, "custom_branch_policies": true });
        let policies = serde_json::json!({ "branch_policies": [
            { "name": "v*", "type": "tag" }, { "name": "main", "type": "branch" }
        ] });
        let good = environment(reviewers(), custom.clone());
        assert_eq!(environment_problem(&good, Some(&policies), "tag", "v26.11.11"), None);
        assert_eq!(environment_problem(&good, Some(&policies), "branch", "main"), None);
        assert!(environment_problem(&good, Some(&policies), "branch", "v26.11.11").unwrap().contains("does not allow"));
        assert!(environment_problem(&good, Some(&policies), "tag", "main").is_some(), "the type counts");
        assert!(environment_problem(&good, None, "tag", "v26.11.11").is_some(), "no policies read, nothing allowed");
        let untyped = serde_json::json!({ "branch_policies": [{ "name": "release" }] });
        assert_eq!(environment_problem(&good, Some(&untyped), "branch", "release"), None, "a branch by default");

        let wait_only = serde_json::json!([{ "type": "wait_timer", "wait_timer": 5 }]);
        for rules in
            [serde_json::json!([]), wait_only, serde_json::json!([{ "type": "required_reviewers", "reviewers": [] }])]
        {
            let problem = environment_problem(&environment(rules, custom.clone()), Some(&policies), "tag", "v26.11.11");
            assert!(problem.unwrap().contains("no required reviewers"));
        }
        let open = environment(reviewers(), serde_json::Value::Null);
        assert!(environment_problem(&open, None, "tag", "v26.11.11").unwrap().contains("every branch and tag"));
        let protected = environment(
            reviewers(),
            serde_json::json!({ "protected_branches": true, "custom_branch_policies": false }),
        );
        assert_eq!(environment_problem(&protected, None, "branch", "release"), None);
        assert!(environment_problem(&protected, None, "tag", "v26.11.11").unwrap().contains("protected branches only"));
    }

    #[test]
    fn ref_patterns_match_as_github_documents_them() {
        assert!(matches_ref("v*", "v26.11.11"));
        assert!(matches_ref("release", "release"));
        assert!(!matches_ref("release", "release2"));
        assert!(!matches_ref("v*", "x26.11.11"));
        assert!(!matches_ref("release/*", "release/a/b"), "one star stops at a slash");
        assert!(matches_ref("release/**", "release/a/b"));
        assert!(matches_ref("v2?.*", "v26.11"));
        assert!(!matches_ref("", "main"));
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
        assert_eq!(crate::release_flow::verified_part(dir.path(), Gate::Deferred).unwrap(), part);
        assert!(crate::release_flow::verified(dir.path()).unwrap_err().contains("it must have passed"));

        std::fs::write(dir.path().join("a.tar.gz"), b"archivo").unwrap();
        assert!(
            crate::release_flow::verified_part(dir.path(), Gate::Deferred).unwrap_err().contains("SHA-256"),
            "checksums first"
        );

        std::fs::write(dir.path().join("a.tar.gz"), b"archive").unwrap();
        part.gate = "passed".to_owned();
        crate::release_flow::save_record(dir.path(), &part).unwrap();
        assert!(
            crate::release_flow::verified_part(dir.path(), Gate::Deferred).unwrap_err().contains("deferred"),
            "not a part"
        );
        assert!(crate::release_flow::verified(dir.path()).is_ok());
    }
}
