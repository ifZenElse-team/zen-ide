use std::path::Path;

use zen_core::Session;
use zen_i18n::t;

use super::sources::IndexQuery;
use super::{Allowed, Completing, Dependencies, versions, with_files};

const MIN_GROUP: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub from: usize,
    pub items: Vec<OfferedItem>,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedItem {
    pub label: String,
    pub detail: Option<String>,
}

pub fn completes(session: &Session, path: &Path) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    session
        .state::<Dependencies>()
        .is_some_and(|service| service.providers().iter().any(|provider| provider.handles(&name)))
}

fn listed(pairs: Vec<(String, String)>, pick: impl Fn(&(String, String)) -> &String) -> Vec<OfferedItem> {
    let mut labels: Vec<String> = pairs.iter().map(|pair| pick(pair).clone()).collect();
    labels.dedup();
    let mut seen = std::collections::HashSet::new();
    labels.retain(|label| seen.insert(label.clone()));
    labels.into_iter().map(|label| OfferedItem { label, detail: None }).collect()
}

pub fn offered(session: &mut Session, path: &Path, text: &str, caret: usize) -> Option<Offered> {
    let root = crate::run::project_root(session).ok()?;
    let service = super::service(session);
    let providers = service.providers_for(&root);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let provider = providers.iter().find(|provider| provider.handles(&name))?;
    let completing = provider.completing(path, text, caret)?;
    let repositories = with_files(session, &service, &root, |files| provider.repositories_for(files, path));
    let (offline, allowed) = with_files(session, &service, &root, |files| {
        let offline = provider.offline(files);
        let settings = session.settings();
        let setting = |key: &str| settings.get::<serde_json::Value>(key);
        (offline, Allowed::of(&setting, offline))
    });
    if offline || !allowed.search {
        return None;
    }
    let revision = service.generation();
    let (from, items, pending) = match completing {
        Completing::Group { prefix, from } => {
            if prefix.len() < MIN_GROUP {
                return None;
            }
            let (found, pending) = service.completion_index(&IndexQuery::Group(prefix));
            (from, listed(found, |pair| &pair.0), pending)
        }
        Completing::Artifact { group, prefix, from } => {
            let (found, pending) = service.completion_index(&IndexQuery::Artifact { group, prefix });
            (from, listed(found, |pair| &pair.1), pending)
        }
        Completing::Version { group, artifact, from, .. } => {
            let (found, pending, _) = service.versions(&group, &artifact, &repositories);
            let latest = versions::latest_stable(&found);
            let stable = found.into_iter().filter(|version| versions::is_stable(version));
            let items = stable
                .map(|label| OfferedItem {
                    detail: (latest.as_ref() == Some(&label)).then(|| t!("workbench-deps-latest")),
                    label,
                })
                .collect();
            (from, items, pending)
        }
    };
    Some(Offered { from, items, revision: pending.then_some(revision) })
}
