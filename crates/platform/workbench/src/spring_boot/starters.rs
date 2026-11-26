use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_project::generators::{Field, StepAnswer, Values, localized_form};

use super::{document, is_on, support};
use crate::Workbench;
use crate::dependencies::{DependencyChange, DependencyModule, Edited, change_all, project_modules};
use crate::new_project::{StepJob, start_step};

const GENERATOR: &str = "spring-boot.initializr";
const STEP: &str = "starters";
const CATALOG: &str = "gradle/libs.versions.toml";

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Starter {
    group_id: String,
    artifact_id: String,
    version: String,
    scope: String,
    bom: String,
    repository: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Bom {
    group_id: String,
    artifact_id: String,
    version: String,
    repositories: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Repository {
    name: String,
    url: String,
    snapshot_enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Offer {
    field: Value,
    ticked: Vec<String>,
    dependencies: BTreeMap<String, Starter>,
    boms: BTreeMap<String, Bom>,
    repositories: BTreeMap<String, Repository>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    root: PathBuf,
    module: String,
    boot: String,
    declared: Vec<String>,
    language: String,
}

#[derive(Default)]
pub struct Starters {
    job: Option<(Key, StepJob)>,
}

/// The parameters of `springBoot.starters.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GetParams {
    /// The module's id from `dependencies.project.list`; the one a build file names Spring Boot in without it.
    pub module: Option<String>,
    /// A build file, instead of the module's id.
    pub file: Option<PathBuf>,
    /// How long to wait for the service, in milliseconds.
    pub wait_ms: Option<u64>,
    /// Ask again after a failure.
    pub retry: bool,
}

/// What `springBoot.starters.get` found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartersState {
    /// `loading`, `ready` or `failed`.
    pub state: String,
    /// The module's id.
    pub module: String,
    /// The module's name.
    pub name: String,
    /// The build file edited.
    pub file: PathBuf,
    /// The project's Spring Boot version, read from the build.
    pub boot_version: String,
    /// The starters the service offers for that version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<Field>,
    /// The starters the module already has.
    pub ticked: Vec<String>,
    /// Said when the list comes from a saved answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Why the list could not be had.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The parameters of `springBoot.starters.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ApplyParams {
    /// The module's id, as for `springBoot.starters.get`.
    pub module: Option<String>,
    /// A build file, instead of the module's id.
    pub file: Option<PathBuf>,
    /// The ids of the starters to add.
    pub add: Vec<String>,
    /// The ids of the starters to remove.
    pub remove: Vec<String>,
}

fn text_of(session: &Session, path: &Path) -> Option<String> {
    document(session, path).map(|(_, text)| text).or_else(|| std::fs::read_to_string(path).ok())
}

fn boot_in(session: &Session, path: &Path) -> Option<String> {
    let text = text_of(session, path)?;
    support::version_in(path, &text).map(|range| text[range].to_owned())
}

fn target(
    session: &mut Session,
    module: Option<&str>,
    file: Option<&Path>,
) -> Result<(DependencyModule, String), CommandError> {
    let root = crate::run::project_root(session)?;
    let modules = project_modules(session)?;
    let named = module.and_then(|id| modules.iter().find(|found| found.id == id));
    let filed = file.and_then(|file| modules.iter().find(|found| found.file == file));
    if let Some(id) = module.filter(|_| named.is_none()) {
        return Err(CommandError::new(t!("workbench-deps-no-module", module = id)));
    }
    let booted = modules.iter().find(|found| boot_in(session, &found.file).is_some());
    let chosen = named.or(filed).or(booted).or(modules.first());
    let chosen = chosen.ok_or_else(|| CommandError::new(t!("workbench-starters-no-build")))?;
    let boot = boot_in(session, &chosen.file)
        .or_else(|| modules.iter().find_map(|found| boot_in(session, &found.file)))
        .or_else(|| boot_in(session, &root.join(CATALOG)))
        .ok_or_else(|| CommandError::new(t!("workbench-starters-no-boot", file = chosen.id.as_str())))?;
    Ok((chosen.clone(), boot))
}

fn key_of(session: &Session, module: &DependencyModule, boot: &str) -> Result<Key, CommandError> {
    let declared: BTreeSet<String> =
        module.dependencies.iter().map(|known| format!("{}:{}", known.group, known.artifact)).collect();
    Ok(Key {
        root: crate::run::project_root(session)?,
        module: module.id.clone(),
        boot: boot.to_owned(),
        declared: declared.into_iter().collect(),
        language: zen_i18n::language(),
    })
}

fn job_for(session: &mut Session, key: &Key, retry: bool) -> Result<StepJob, CommandError> {
    if !retry
        && let Some((known, job)) = &session.state_mut::<Starters>().job
        && known == key
    {
        return Ok(job.clone());
    }
    let values =
        Values::from([("bootVersion".to_owned(), json!(key.boot)), ("declared".to_owned(), json!(key.declared))]);
    let job = start_step(session, GENERATOR, STEP, values)?;
    session.state_mut::<Starters>().job = Some((key.clone(), job.clone()));
    Ok(job)
}

fn offer_of(answer: &StepAnswer) -> Result<(Offer, Option<Field>), String> {
    let starters = answer.answer.get("starters").cloned().ok_or_else(|| t!("workbench-starters-no-answer"))?;
    let offer: Offer = serde_json::from_value(starters).map_err(|error| error.to_string())?;
    let form = localized_form(&json!({ "fields": [offer.field] }), &zen_i18n::language())?;
    Ok((offer, form.fields.into_iter().next()))
}

fn plain_value(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn plain_address(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/' | ':' | '~' | '%'))
    })
}

fn refuse_unplain(wanted: &[DependencyChange]) -> Result<(), String> {
    let refused = |value: &str| Err(t!("workbench-starters-bad-value", value = value));
    for change in wanted {
        let values: Vec<&str> = match change {
            DependencyChange::Add { group, artifact, version, scope, .. } => vec![group, artifact, version, scope],
            DependencyChange::AddBom { group, artifact, version, .. } => vec![group, artifact, version],
            DependencyChange::AddRepository { id, name, url, .. } => {
                if !plain_address(url) {
                    return refused(url);
                }
                if name.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '&' | '"' | '\'' | '$' | '\\')) {
                    return refused(name);
                }
                vec![id]
            }
            _ => Vec::new(),
        };
        if let Some(value) = values.into_iter().find(|value| !plain_value(value)) {
            return refused(value);
        }
    }
    Ok(())
}

fn changes(offer: &Offer, module: &str, add: &[String], remove: &[String]) -> Result<Vec<DependencyChange>, String> {
    let known =
        |id: &String| offer.dependencies.get(id).ok_or_else(|| t!("workbench-starters-unknown", starter = id.as_str()));
    let module = module.to_owned();
    let mut wanted = Vec::new();
    for id in remove {
        let starter = known(id)?;
        wanted.push(DependencyChange::Remove {
            module: module.clone(),
            group: starter.group_id.clone(),
            artifact: starter.artifact_id.clone(),
            scope: None,
        });
    }
    let chosen = add.iter().map(known).collect::<Result<Vec<_>, _>>()?;
    let boms: BTreeSet<&str> = chosen.iter().map(|starter| starter.bom.as_str()).filter(|id| !id.is_empty()).collect();
    let mut repositories: BTreeSet<&str> =
        chosen.iter().map(|starter| starter.repository.as_str()).filter(|id| !id.is_empty()).collect();
    for id in &boms {
        repositories
            .extend(offer.boms.get(*id).into_iter().flat_map(|bom| bom.repositories.iter().map(String::as_str)));
    }
    for id in repositories {
        let Some(repository) = offer.repositories.get(id) else { continue };
        wanted.push(DependencyChange::AddRepository {
            module: module.clone(),
            id: id.to_owned(),
            name: repository.name.clone(),
            url: repository.url.clone(),
            snapshots: repository.snapshot_enabled,
        });
    }
    for id in boms {
        let Some(bom) = offer.boms.get(id) else { continue };
        wanted.push(DependencyChange::AddBom {
            module: module.clone(),
            group: bom.group_id.clone(),
            artifact: bom.artifact_id.clone(),
            version: bom.version.clone(),
        });
    }
    wanted.extend(chosen.into_iter().map(|starter| DependencyChange::Add {
        module: module.clone(),
        group: starter.group_id.clone(),
        artifact: starter.artifact_id.clone(),
        version: starter.version.clone(),
        scope: starter.scope.clone(),
    }));
    refuse_unplain(&wanted)?;
    Ok(wanted)
}

pub struct GetStarters;

impl Command for GetStarters {
    type Params = GetParams;
    type Output = StartersState;
    const ID: &'static str = "springBoot.starters.get";
    const TITLE: &'static str = "Spring Boot Starters of a Module";
    const DESCRIPTION: &'static str = "The Spring Boot version of a module, read from its build, the starters the \
         Spring Initializr service offers for that version and the ones the module already has. The service answers \
         in the background: `state` is `loading` until it does, unless `waitMs` waits. Needs the network the first \
         time; project generators' network switch applies. Needs the Spring Boot plug-in.";

    fn run(&self, session: &mut Session, params: GetParams) -> Result<StartersState, CommandError> {
        if !is_on(session) {
            return Err(CommandError::new(t!("workbench-spring-boot-off")));
        }
        let (module, boot) = target(session, params.module.as_deref(), params.file.as_deref())?;
        let key = key_of(session, &module, &boot)?;
        let job = job_for(session, &key, params.retry)?;
        let mut state = StartersState {
            state: "loading".to_owned(),
            module: module.id,
            name: module.name,
            file: module.file,
            boot_version: boot,
            ..StartersState::default()
        };
        match job
            .poll(params.wait_ms)
            .map(|answer| answer.and_then(|answer| Ok((offer_of(&answer)?, answer.from_saved))))
        {
            None => {}
            Some(Ok(((offer, field), from_saved))) => {
                state.state = "ready".to_owned();
                state.field = field;
                state.ticked = offer.ticked;
                state.notice = from_saved.then(|| t!("workbench-starters-saved"));
            }
            Some(Err(error)) => {
                state.state = "failed".to_owned();
                state.error = Some(error);
            }
        }
        Ok(state)
    }
}

pub struct ApplyStarters;

impl Command for ApplyStarters {
    type Params = ApplyParams;
    type Output = Edited;
    const ID: &'static str = "springBoot.starters.apply";
    const TITLE: &'static str = "Edit Spring Boot Starters";
    const DESCRIPTION: &'static str = "Adds and removes starters of a module by their ids from \
         `springBoot.starters.get`, which must have answered: the dependencies, BOMs and repositories the service \
         names for the project's Spring Boot version go into the build file in place, all of them or none, and the \
         project loads again.";

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<Edited, CommandError> {
        if !is_on(session) {
            return Err(CommandError::new(t!("workbench-spring-boot-off")));
        }
        let (module, boot) = target(session, params.module.as_deref(), params.file.as_deref())?;
        let key = key_of(session, &module, &boot)?;
        let known = session.state_mut::<Starters>().job.clone().filter(|(known, _)| *known == key);
        let answer = known.and_then(|(_, job)| job.poll(None));
        let answer = answer.ok_or_else(|| CommandError::new(t!("workbench-starters-not-loaded")))?;
        let (offer, _) = answer.and_then(|answer| offer_of(&answer)).map_err(CommandError::new)?;
        let wanted = changes(&offer, &module.id, &params.add, &params.remove).map_err(CommandError::new)?;
        let edited = change_all(session, &module.id, &wanted)?;
        session.state_mut::<Starters>().job = None;
        Ok(edited)
    }
}

pub fn offered(session: &Session, path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if !matches!(name, "pom.xml" | "build.gradle" | "build.gradle.kts") || !is_on(session) {
        return false;
    }
    let documents = session.state::<Workbench>().map(Workbench::documents).unwrap_or_default();
    let open = documents.iter().find(|document| document.path() == path);
    open.is_some_and(|document| {
        ["spring-boot", "springframework.boot"].iter().any(|name| document.text().contains(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> Offer {
        serde_json::from_value(json!({
            "field": {},
            "dependencies": {
                "one": { "groupId": "g", "artifactId": "one", "bom": "b", "repository": "r" },
                "two": { "groupId": "g", "artifactId": "two", "bom": "b", "scope": "runtime" },
            },
            "boms": { "b": { "groupId": "g", "artifactId": "bom", "version": "1", "repositories": ["q"] } },
            "repositories": {
                "r": { "name": "R", "url": "https://r.example" },
                "q": { "name": "Q", "url": "https://q.example", "snapshotEnabled": true },
            },
        }))
        .unwrap()
    }

    #[test]
    fn a_value_that_could_become_build_code_is_refused() {
        let with = |starter: Value, repository: &str| -> Offer {
            serde_json::from_value(json!({
                "field": {},
                "dependencies": { "one": starter },
                "repositories": { "r": { "name": "R", "url": repository } },
            }))
            .unwrap()
        };
        let add = ["one".to_owned()];
        let plain = json!({ "groupId": "g", "artifactId": "a", "repository": "r" });
        assert!(changes(&with(plain.clone(), "https://r.example/maven"), "m", &add, &[]).is_ok());
        for url in ["http://r.example", "https://r\" }; evil(); //", "https://r.example/<x>"] {
            assert!(changes(&with(plain.clone(), url), "m", &add, &[]).is_err(), "{url}");
        }
        let named: Offer = serde_json::from_value(json!({
            "field": {},
            "dependencies": { "one": plain },
            "repositories": { "r": { "name": "R</name><url>https://evil.example</url><name>", "url": "https://r.example" } },
        }))
        .unwrap();
        assert!(changes(&named, "m", &add, &[]).is_err(), "the name is written into pom.xml too");
        let quoted = json!({ "groupId": "g\"", "artifactId": "a</artifactId>" });
        assert!(changes(&with(quoted, "https://r.example"), "m", &add, &[]).is_err());
    }

    #[test]
    fn two_starters_of_a_bom_add_it_once_with_its_repositories_before_the_dependencies() {
        let add = ["one".to_owned(), "two".to_owned()];
        let wanted = changes(&offer(), "pom.xml", &add, &[]).unwrap();
        let kinds: Vec<&str> = wanted
            .iter()
            .map(|change| match change {
                DependencyChange::AddRepository { .. } => "repository",
                DependencyChange::AddBom { .. } => "bom",
                DependencyChange::Add { .. } => "dependency",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["repository", "repository", "bom", "dependency", "dependency"]);
    }

    #[test]
    fn a_starter_not_on_offer_refuses_the_whole_list() {
        assert!(changes(&offer(), "pom.xml", &["none".to_owned()], &[]).is_err());
        assert!(changes(&offer(), "pom.xml", &[], &["none".to_owned()]).is_err());
    }
}
