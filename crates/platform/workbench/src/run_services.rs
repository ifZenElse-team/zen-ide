use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{Configuration, RunState};

use crate::run::{Runs, project_root, read_file, unknown_configuration};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunServices {
    pub kinds: Vec<String>,
    pub configurations: Vec<String>,
    pub hidden: Vec<String>,
}

impl RunServices {
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.configurations.is_empty() && self.hidden.is_empty()
    }

    pub fn shows(&self, configuration: &Configuration) -> bool {
        !self.hidden.contains(&configuration.id)
            && (self.kinds.contains(&configuration.kind) || self.configurations.contains(&configuration.id))
    }
}

/// A run configuration as the Services tool window shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceItem {
    /// The configuration's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The folder it is kept in, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// Where its latest run is; absent when it has not run in this window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<RunState>,
    /// Whether one of its runs is running (for a compound, one of the runs it started).
    pub running: bool,
    /// Its latest run, whose console the window shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// Whether the latest run is a debug session's.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub debug: bool,
}

/// A kind of run configuration in the Services tool window.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceKind {
    /// The kind's id.
    pub kind: String,
    /// The message id of the kind's name; empty for a kind no provider runs.
    pub name_message: String,
    /// Whether the whole kind was added (Add Service → Run Configuration → kind) rather than single configurations.
    pub added: bool,
    /// Its configurations, in the project's order, without the hidden ones.
    pub configurations: Vec<ServiceItem>,
}

/// What `run.services.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceList {
    /// The kinds shown, in the order they were added.
    pub kinds: Vec<ServiceKind>,
    /// The ids of the configurations hidden with Hide Configuration.
    pub hidden: Vec<String>,
    /// The kinds that can still be added, with the message ids of their names.
    pub available: Vec<(String, String)>,
}

fn item(runs: &Runs, configuration: &Configuration) -> ServiceItem {
    let own = runs.of_configuration(&configuration.id);
    let latest = own.last();
    let running = own.iter().any(|run| run.is_running());
    ServiceItem {
        id: configuration.id.clone(),
        name: configuration.name.clone(),
        folder: configuration.folder.clone(),
        state: if running {
            Some(RunState::Running)
        } else {
            latest.map(|run| run.output.read(|output| output.state()))
        },
        running,
        run_id: own.iter().rev().find(|run| run.is_running()).or(latest).map(|run| run.id),
        debug: latest.is_some_and(|run| run.debug),
    }
}

pub fn list(session: &mut Session) -> Result<ServiceList, CommandError> {
    let root = project_root(session)?;
    let file = read_file(session, &root)?;
    let services = session.state::<RunServices>().cloned().unwrap_or_default();
    let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
    let mut order: Vec<String> = services.kinds.clone();
    for id in &services.configurations {
        if let Some(kind) = file.get(id).map(|kept| kept.kind.clone())
            && !order.contains(&kind)
        {
            order.push(kind);
        }
    }
    let runs = session.state::<Runs>();
    let empty = Runs::default();
    let runs = runs.unwrap_or(&empty);
    let shown = order
        .into_iter()
        .map(|kind| ServiceKind {
            name_message: kinds
                .iter()
                .find(|known| known.spec.id == kind)
                .map(|known| known.spec.name_message.clone())
                .unwrap_or_default(),
            added: services.kinds.contains(&kind),
            configurations: file
                .configurations
                .iter()
                .filter(|configuration| configuration.kind == kind && services.shows(configuration))
                .map(|configuration| item(runs, configuration))
                .collect(),
            kind,
        })
        .collect();
    let available = kinds
        .iter()
        .filter(|known| !services.kinds.contains(&known.spec.id))
        .map(|known| (known.spec.id.clone(), known.spec.name_message.clone()))
        .collect();
    Ok(ServiceList { kinds: shown, hidden: services.hidden, available })
}

pub struct ListServices;

impl Command for ListServices {
    type Params = NoParams;
    type Output = ServiceList;
    const ID: &'static str = "run.services.list";
    const TITLE: &'static str = "Services";
    const DESCRIPTION: &'static str = "The run configurations the Services tool window (⌘8) shows, by kind: each \
         with where its latest run is, whether it runs and that run's id (its console: run.output.get). Start one \
         with run.process.start or debug.session.start, stop it with run.process.stop {configurationId}, rerun it \
         with run.process.rerun.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ServiceList, CommandError> {
        list(session)
    }
}

/// What to add to or remove from the Services tool window.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceParams {
    /// A kind of run configuration: all its configurations.
    pub kind: Option<String>,
    /// One configuration.
    pub configuration_id: Option<String>,
}

fn change(
    session: &mut Session,
    params: ServiceParams,
    apply: impl FnOnce(&mut RunServices, Option<String>, Option<String>),
) -> Result<ServiceList, CommandError> {
    let root = project_root(session)?;
    let file = read_file(session, &root)?;
    if params.kind.is_none() && params.configuration_id.is_none() {
        return Err(CommandError::new(t!("workbench-run-services-nothing")));
    }
    if let Some(kind) = &params.kind {
        let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
        if !kinds.iter().any(|known| known.spec.id == *kind) {
            return Err(CommandError::new(t!("workbench-run-template-unknown-kind", kind = kind.as_str())));
        }
    }
    if let Some(id) = &params.configuration_id
        && file.get(id).is_none()
    {
        return Err(unknown_configuration(id));
    }
    apply(session.state_mut::<RunServices>(), params.kind, params.configuration_id);
    list(session)
}

fn push(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

pub struct AddService;

impl Command for AddService {
    type Params = ServiceParams;
    type Output = ServiceList;
    const ID: &'static str = "run.services.add";
    const TITLE: &'static str = "Add Service";
    const DESCRIPTION: &'static str = "Shows run configurations in the Services tool window (Add Service → Run \
         Configuration Type): every configuration of `kind`, or the one `configurationId` names; the window keeps \
         the choice.";

    fn run(&self, session: &mut Session, params: ServiceParams) -> Result<ServiceList, CommandError> {
        change(session, params, |services, kind, id| {
            if let Some(kind) = kind {
                push(&mut services.kinds, kind);
            }
            if let Some(id) = id {
                services.hidden.retain(|hidden| *hidden != id);
                push(&mut services.configurations, id);
            }
        })
    }
}

pub struct RemoveService;

impl Command for RemoveService {
    type Params = ServiceParams;
    type Output = ServiceList;
    const ID: &'static str = "run.services.remove";
    const TITLE: &'static str = "Remove from Services";
    const DESCRIPTION: &'static str = "Takes a kind of run configuration, or one configuration added on its own, out \
         of the Services tool window.";

    fn run(&self, session: &mut Session, params: ServiceParams) -> Result<ServiceList, CommandError> {
        change(session, params, |services, kind, id| {
            if let Some(kind) = kind {
                services.kinds.retain(|kept| *kept != kind);
            }
            if let Some(id) = id {
                services.configurations.retain(|kept| *kept != id);
            }
        })
    }
}

/// A configuration by id.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HideParams {
    /// The configuration's id.
    pub configuration_id: String,
}

pub struct HideService;

impl Command for HideService {
    type Params = HideParams;
    type Output = ServiceList;
    const ID: &'static str = "run.services.hide";
    const TITLE: &'static str = "Hide Configuration";
    const DESCRIPTION: &'static str = "Hides a run configuration in the Services tool window, though its kind is \
         shown there; run.services.restore shows the hidden ones again.";

    fn run(&self, session: &mut Session, params: HideParams) -> Result<ServiceList, CommandError> {
        let id = params.configuration_id;
        let params = ServiceParams { kind: None, configuration_id: Some(id) };
        change(session, params, |services, _, id| {
            if let Some(id) = id {
                push(&mut services.hidden, id);
            }
        })
    }
}

pub struct RestoreServices;

impl Command for RestoreServices {
    type Params = NoParams;
    type Output = ServiceList;
    const ID: &'static str = "run.services.restore";
    const TITLE: &'static str = "Restore Hidden Configurations";
    const DESCRIPTION: &'static str = "Shows again every run configuration hidden in the Services tool window.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ServiceList, CommandError> {
        session.state_mut::<RunServices>().hidden.clear();
        list(session)
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListServices)?;
    registry.register(AddService)?;
    registry.register(RemoveService)?;
    registry.register(HideService)?;
    registry.register(RestoreServices)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use zen_run::RunEnv;
    use zen_run::fake::{FakeProvider, KIND, SHELL_KIND, ShellProvider};

    use super::*;

    #[test]
    fn services_show_the_added_kinds_with_their_run_state_and_hide_and_restore_configurations() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let providers: Vec<Arc<dyn zen_run::RunProvider>> =
            vec![Arc::new(FakeProvider::new()), Arc::new(ShellProvider)];
        session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        let mut create = |configuration: Configuration| {
            let params = json!({ "configuration": configuration });
            registry.dispatch(&mut session, "run.configuration.create", params).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let app = create(FakeProvider::configuration("app"));
        let api = create(FakeProvider::configuration("api"));
        let script = create(ShellProvider::configuration("script", "true"));

        let listed = registry.dispatch(&mut session, "run.services.list", json!({})).unwrap();
        assert_eq!(listed["kinds"], json!([]));
        assert!(listed["available"].as_array().unwrap().iter().any(|kind| kind[0] == KIND));
        let added = registry.dispatch(&mut session, "run.services.add", json!({ "kind": KIND })).unwrap();
        let names: Vec<&str> = added["kinds"][0]["configurations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["app", "api"]);
        assert!(added["kinds"][0].get("state").is_none());
        assert!(!added["available"].as_array().unwrap().iter().any(|kind| kind[0] == KIND));

        let started =
            registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": app, "waitMs": 5000 }));
        let run_id = started.unwrap()["runId"].clone();
        let listed = registry.dispatch(&mut session, "run.services.list", json!({})).unwrap();
        let item = &listed["kinds"][0]["configurations"][0];
        assert_eq!(
            (item["state"].as_str(), item["runId"].clone(), item["running"].as_bool()),
            (Some("finished"), run_id, Some(false))
        );

        let hidden = registry.dispatch(&mut session, "run.services.hide", json!({ "configurationId": api })).unwrap();
        assert_eq!(hidden["kinds"][0]["configurations"].as_array().unwrap().len(), 1);
        assert_eq!(hidden["hidden"], json!([api]));
        let restored = registry.dispatch(&mut session, "run.services.restore", json!({})).unwrap();
        assert_eq!(restored["kinds"][0]["configurations"].as_array().unwrap().len(), 2);

        let one = registry.dispatch(&mut session, "run.services.add", json!({ "configurationId": script })).unwrap();
        assert_eq!(
            (one["kinds"][1]["kind"].as_str(), one["kinds"][1]["added"].as_bool()),
            (Some(SHELL_KIND), Some(false))
        );
        let removed = registry.dispatch(&mut session, "run.services.remove", json!({ "kind": KIND })).unwrap();
        assert_eq!(removed["kinds"].as_array().unwrap().len(), 1);
        assert!(registry.dispatch(&mut session, "run.services.add", json!({})).is_err());
        assert!(registry.dispatch(&mut session, "run.services.add", json!({ "configurationId": "nope" })).is_err());
        let services = session.state::<RunServices>().unwrap();
        assert_eq!((services.kinds.len(), services.configurations.len()), (0, 1));
    }
}
