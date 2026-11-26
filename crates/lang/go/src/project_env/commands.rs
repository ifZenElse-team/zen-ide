use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Form, Item, Tone};

use super::{SHARED_FILE, Scope, Variable, parse, text_of};
use crate::toolchain::Toolchain;

pub const AGENT_NOT_ALLOWED: &str = "agentNotAllowed";

pub const SETTINGS_PAGE: &str = "goModules";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoEnvGet)?;
    registry.register(GoEnvSet)?;
    registry.register(GoEnvSettings)?;
    Ok(())
}

/// Which project.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EnvParams {
    /// The project's root folder; the open project when absent.
    pub path: Option<PathBuf>,
}

/// A project's Go environment, layer by layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoEnvInfo {
    /// The project's root folder.
    pub root: PathBuf,
    /// The shared file, relative to the root.
    pub shared_file: String,
    /// The variables of the shared file, `.zen/go.json`.
    pub shared: BTreeMap<String, String>,
    /// Whether the shared file applies: only in a trusted project.
    pub shared_applies: bool,
    /// The variables kept only on this computer for this project.
    pub personal: BTreeMap<String, String>,
    /// What every go command and gopls get, before a run configuration's own environment.
    pub effective: Vec<Variable>,
}

fn root_of(session: &Session, path: Option<&Path>) -> Result<PathBuf, CommandError> {
    let open = session.state::<zen_project::trust::OpenRoot>().map(|open| open.0.clone());
    match (path, open) {
        (Some(path), Some(open)) if path.is_relative() => Ok(open.join(path)),
        (Some(path), _) => Ok(path.to_owned()),
        (None, Some(open)) => Ok(open),
        (None, None) => Err(CommandError::new(t!("go-env-error-no-project"))),
    }
}

fn toolchain(session: &Session) -> Result<Toolchain, CommandError> {
    session.state::<Toolchain>().cloned().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))
}

pub fn info(toolchain: &Toolchain, root: &Path) -> GoEnvInfo {
    let store = toolchain.project_env();
    let trusted = zen_project::trust::trusted(root);
    GoEnvInfo {
        root: root.to_owned(),
        shared_file: SHARED_FILE.to_owned(),
        shared: store.shared(root),
        shared_applies: trusted,
        personal: store.personal(root),
        effective: store.effective(root, trusted),
    }
}

pub struct GoEnvGet;

impl Command for GoEnvGet {
    type Params = EnvParams;
    type Output = GoEnvInfo;
    const ID: &'static str = "go.env.get";
    const TITLE: &'static str = "Go Environment";
    const DESCRIPTION: &'static str = "The Go environment of project `path` (the open one when absent, ADR-0298): the \
         shared variables of its .zen/go.json (applied only when the project is trusted), the ones kept only on this \
         computer (they win), and `effective`, what every go command the IDE runs for the project and gopls get, each \
         with its source. A run configuration's own environment wins over them; the IDE's own checks keep \
         GOTOOLCHAIN=local, GOPROXY=off and add -mod=readonly to GOFLAGS.";

    fn run(&self, session: &mut Session, params: EnvParams) -> Result<GoEnvInfo, CommandError> {
        let root = root_of(session, params.path.as_deref())?;
        let info = info(&toolchain(session)?, &root);
        Ok(if zen_core::macros::by_agent(session) { info.redacted() } else { info })
    }
}

impl GoEnvInfo {
    pub fn redacted(self) -> Self {
        let hide = |layer: BTreeMap<String, String>| {
            layer.into_iter().map(|(name, value)| (name, super::redact(&value))).collect()
        };
        Self {
            shared: hide(self.shared),
            personal: hide(self.personal),
            effective: self.effective.into_iter().map(Variable::redacted).collect(),
            ..self
        }
    }
}

/// A new layer of a project's Go environment.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetParams {
    /// The layer: `project` (.zen/go.json, shared) or `personal` (only on this computer).
    pub scope: Scope,
    /// The whole layer as `NAME=value` pairs separated by `;` or new lines; empty or null clears it.
    #[serde(default)]
    pub environment: Option<String>,
    /// The project's root folder; the open project when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct GoEnvSet;

impl Command for GoEnvSet {
    type Params = SetParams;
    type Output = GoEnvInfo;
    const ID: &'static str = "go.env.set";
    const TITLE: &'static str = "Set Go Environment";
    const DESCRIPTION: &'static str = "Replaces one layer of project `path`'s Go environment (ADR-0298): `scope` \
         `project` writes .zen/go.json, shared with the repository, `personal` keeps it only on this computer; \
         `environment` is `NAME=value` pairs separated by `;`, empty clears the layer. Only GO* and CGO_* names; \
         GOROOT and PATH are refused, GOENV and GOTOOLCHAIN only on this computer, and a URL with a user or password \
         never goes into the shared file. gopls restarts with the change. The person's action only: it can run code, \
         so agents are refused (code `agentNotAllowed`). Answers as go.env.get.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<GoEnvInfo, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(AGENT_NOT_ALLOWED, t!("go-env-error-agent")));
        }
        let root = root_of(session, params.path.as_deref())?;
        let variables = parse(params.environment.as_deref().unwrap_or_default()).map_err(CommandError::new)?;
        let toolchain = toolchain(session)?;
        toolchain.set_project_env(&root, params.scope, &variables).map_err(CommandError::new)?;
        Ok(info(&toolchain, &root))
    }
}

pub struct GoEnvSettings;

impl Command for GoEnvSettings {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "go.env.settings";
    const TITLE: &'static str = "Go Modules Settings Page";
    const DESCRIPTION: &'static str = "Settings → Languages & Frameworks → Go → Go Modules as the page shows it: the \
         open project's Go environment, shared and only on this computer, with what is in effect; go.env.set changes \
         it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let toolchain = toolchain(session)?;
        let agent = zen_core::macros::by_agent(session);
        Ok(match root_of(session, None) {
            Ok(root) if agent => settings_form(&info(&toolchain, &root).redacted()),
            Ok(root) => settings_form(&info(&toolchain, &root)),
            Err(_) => Form {
                items: vec![Item::Text { text: t!("go-env-error-no-project"), tone: Tone::Secondary, spinner: false }],
                busy: false,
            },
        })
    }
}

fn note(text: String) -> Item {
    Item::Text { text, tone: Tone::Secondary, spinner: false }
}

pub fn settings_form(info: &GoEnvInfo) -> Form {
    let set = |scope: &str| {
        Action::with_value(GoEnvSet::ID, "environment")
            .param("scope", scope)
            .param("path", info.root.display().to_string())
    };
    let mut items = vec![
        Item::Group { title: t!("settings-go-env") },
        Item::Field {
            label: t!("settings-go-env-shared"),
            value: text_of(&info.shared),
            action: set("project"),
            browse: Default::default(),
        },
        note(t!("settings-go-env-shared-note")),
    ];
    if !info.shared_applies {
        items.push(Item::Text { text: t!("settings-go-env-untrusted"), tone: Tone::Error, spinner: false });
    }
    items.push(Item::Field {
        label: t!("settings-go-env-personal"),
        value: text_of(&info.personal),
        action: set("personal"),
        browse: Default::default(),
    });
    items.push(note(t!("settings-go-env-note")));
    items.push(Item::Group { title: t!("settings-go-env-effective") });
    if info.effective.is_empty() {
        items.push(note(t!("settings-go-env-none")));
    }
    for variable in &info.effective {
        let source = match variable.source {
            Scope::Project => t!("settings-go-env-from-project"),
            Scope::Personal => t!("settings-go-env-from-personal"),
        };
        items.push(Item::Text {
            text: format!("{}={}   {source}", variable.name, variable.value),
            tone: Tone::Normal,
            spinner: false,
        });
    }
    items.push(note(t!("settings-go-env-checks")));
    Form { items, busy: false }
}
