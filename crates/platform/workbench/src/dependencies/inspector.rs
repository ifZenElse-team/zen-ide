use std::collections::HashMap;
use std::path::Path;

use serde_json::json;
use zen_i18n::t;
use zen_project::inspect::{
    DeclaredSetting, Finding, FindingRow, FindingSeverity, FixJob, FixKind, FixState, FixWrite, InspectContext,
    InspectionFix, Inspector, Wake,
};

use super::{
    Allowed, BuildFiles, CHECK_UPDATES, CHECK_VULNERABILITIES, Dependencies, DependencyChange, FileChange,
    SEARCH_SETTING, VersionSite,
};
use crate::maven::settings::BUILD_TOOLS_PAGE;

const ID: &str = "dependencies";

pub struct DependencyInspector {
    service: Dependencies,
}

impl DependencyInspector {
    pub fn new(service: Dependencies) -> Self {
        Self { service }
    }

    fn files<T>(&self, root: &Path, network: bool, f: impl FnOnce(&BuildFiles<'_>) -> T) -> T {
        let env = self.service.env();
        let open = HashMap::new();
        let service = &self.service;
        let remote = |repositories: &[super::Repository], group: &str, artifact: &str, version: &str| {
            if network {
                service.pom(repositories, group, artifact, version)
            } else {
                service.offline_pom(repositories, group, artifact, version)
            }
        };
        f(&BuildFiles { root, env: &env, open: &open, remote_pom: &remote, seen: Default::default() })
    }
}

fn upgrade_fix(site: &VersionSite, title: String, version: &str) -> InspectionFix {
    InspectionFix {
        id: format!("dependencies.upgrade:{}:{}@{version}", site.group, site.artifact),
        title,
        kind: FixKind::Apply,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "group": site.group, "artifact": site.artifact, "version": version }),
    }
}

fn finding(site: &VersionSite, severity: FindingSeverity, code: &str, message: String) -> Finding {
    Finding {
        start: site.start,
        end: site.end,
        severity,
        message,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        source: ID.to_owned(),
        code: code.to_owned(),
        deprecated: false,
        unused: false,
    }
}

struct Planned(Vec<FixWrite>);

impl FixJob for Planned {
    fn poll(&mut self) -> FixState {
        FixState::Done { changed: self.0.drain(..).map(|change| change.path).collect() }
    }

    fn writes(&self) -> Vec<FixWrite> {
        self.0.clone()
    }
}

impl Inspector for DependencyInspector {
    fn id(&self) -> &str {
        ID
    }

    fn handles(&self, file_name: &str) -> bool {
        self.service.providers().iter().any(|provider| provider.handles(file_name))
    }

    fn generation(&self) -> u64 {
        self.service.generation()
    }

    fn set_wake(&self, wake: Wake) {
        self.service.set_wake(wake);
    }

    fn set_cache_dir(&self, dir: std::path::PathBuf) {
        self.service.set_cache_dir(dir);
    }

    fn refresh(&self) {
        self.service.refresh();
    }

    fn settings(&self) -> Vec<DeclaredSetting> {
        let group = t!("workbench-deps-group");
        let central = "repo.maven.apache.org".to_owned();
        let own = t!("workbench-deps-own-repositories");
        let declared = |key: &str, title: String, description: String, network: Vec<String>| DeclaredSetting {
            key: key.to_owned(),
            page: BUILD_TOOLS_PAGE.to_owned(),
            group: group.clone(),
            title,
            description,
            default: false,
            network,
        };
        vec![
            declared(
                SEARCH_SETTING,
                t!("workbench-deps-search"),
                t!("workbench-deps-search-description"),
                vec!["search.maven.org".to_owned(), central.clone(), own.clone()],
            ),
            declared(
                CHECK_UPDATES,
                t!("workbench-deps-check-updates"),
                t!("workbench-deps-check-updates-description"),
                vec![central.clone(), own.clone()],
            ),
            declared(
                CHECK_VULNERABILITIES,
                t!("workbench-deps-check-vulnerabilities"),
                t!("workbench-deps-check-vulnerabilities-description"),
                vec!["api.osv.dev".to_owned(), central, own],
            ),
        ]
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let name = context.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let providers = self.service.providers_for(context.root);
        let Some(provider) = providers.iter().find(|provider| provider.handles(&name)) else { return Vec::new() };
        let on = Allowed::of(context.setting, false);
        self.files(context.root, on.updates || on.advisories, |files| {
            let allowed = Allowed::of(context.setting, provider.offline(files));
            let repositories = provider.repositories_for(files, context.path);
            let mut findings = Vec::new();
            for site in provider.sites(files, context.path, context.text) {
                let (status, _) =
                    self.service.status(&site.group, &site.artifact, &site.version, &repositories, allowed);
                if !status.advisories.is_empty() {
                    let mut found = finding(
                        &site,
                        FindingSeverity::Warning,
                        "vulnerable",
                        t!("workbench-deps-vulnerable", count = status.advisories.len()),
                    );
                    found.rows = status
                        .advisories
                        .iter()
                        .map(|advisory| FindingRow {
                            label: advisory.id.clone(),
                            url: Some(advisory.url.clone()),
                            badge: advisory.score.map(|score| format!("{score:.1}")).or(advisory.rating.clone()),
                            text: advisory.summary.clone(),
                        })
                        .collect();
                    if let Some(safe) = &status.safe {
                        found.fixes.push(upgrade_fix(
                            &site,
                            t!("workbench-deps-fix-safe", version = safe.as_str()),
                            safe,
                        ));
                    }
                    findings.push(found);
                } else if let Some(newest) = status.newer.first() {
                    let mut found = finding(
                        &site,
                        FindingSeverity::WeakWarning,
                        "outdated",
                        t!("workbench-deps-outdated", version = newest.as_str()),
                    );
                    found.fixes.push(upgrade_fix(
                        &site,
                        t!("workbench-deps-fix-upgrade", version = newest.as_str()),
                        newest,
                    ));
                    findings.push(found);
                }
            }
            findings
        })
    }

    fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        let name = context.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let providers = self.service.providers_for(context.root);
        let provider = providers
            .iter()
            .find(|provider| provider.handles(&name))
            .ok_or_else(|| t!("workbench-deps-no-module", module = name.as_str()))?;
        let module = context
            .path
            .strip_prefix(context.root)
            .map_or_else(|_| context.path.to_string_lossy(), |inside| inside.to_string_lossy())
            .into_owned();
        let mut changed: Vec<FileChange> = Vec::new();
        let mut reads: HashMap<std::path::PathBuf, String> = HashMap::new();
        self.files(context.root, false, |files| -> Result<(), String> {
            for fix in fixes {
                let text = |key: &str| fix.args[key].as_str().unwrap_or_default().to_owned();
                let change = DependencyChange::SetVersion {
                    module: module.clone(),
                    group: text("group"),
                    artifact: text("artifact"),
                    version: text("version"),
                };
                let open: HashMap<_, _> =
                    changed.iter().map(|done: &FileChange| (done.path.clone(), done.text.clone())).collect();
                let edited = BuildFiles {
                    root: files.root,
                    env: files.env,
                    open: &open,
                    remote_pom: files.remote_pom,
                    seen: Default::default(),
                };
                let files_changed = provider.change(&edited, &change);
                edited.keep_reads(&mut reads, |path| open.contains_key(path));
                for file in files_changed? {
                    changed.retain(|done| done.path != file.path);
                    changed.push(file);
                }
            }
            Ok(())
        })?;
        Ok(Box::new(Planned(
            changed
                .into_iter()
                .map(|change| FixWrite { read: reads.get(&change.path).cloned(), path: change.path, text: change.text })
                .collect(),
        )))
    }
}
