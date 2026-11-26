use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

pub const SETTING: &str = "environment.loginShell";

pub fn on(session: &Session) -> bool {
    session.settings().get_or(SETTING, true)
}

pub fn sync(session: &Session) {
    zen_env::set_enabled(on(session));
}

/// The login shell's environment as the IDE uses it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginEnvironment {
    /// Whether the setting is on (`environment.loginShell`).
    pub enabled: bool,
    /// `notRead` (not read at this start: off when the IDE started), `reading`, `ready` or `failed`.
    pub state: String,
    /// The shell that was asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// The names of the variables the programs the IDE starts get from it; never their values.
    pub names: Vec<String>,
    /// Why it could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn status(session: &Session) -> LoginEnvironment {
    let enabled = on(session);
    let mut answer = LoginEnvironment { enabled, ..LoginEnvironment::default() };
    match zen_env::state() {
        zen_env::State::NotRead => answer.state = "notRead".to_owned(),
        zen_env::State::Reading => answer.state = "reading".to_owned(),
        zen_env::State::Failed(error) => {
            answer.state = "failed".to_owned();
            answer.error = Some(error);
        }
        zen_env::State::Ready(environment) => {
            answer.state = "ready".to_owned();
            answer.shell = Some(environment.shell.to_string_lossy().into_owned());
            answer.names = environment.variables.keys().cloned().collect();
        }
    }
    answer
}

pub struct GetLoginEnvironment;

impl Command for GetLoginEnvironment {
    type Params = NoParams;
    type Output = LoginEnvironment;
    const ID: &'static str = "ide.environment.get";
    const TITLE: &'static str = "Login Shell Environment";
    const DESCRIPTION: &'static str = "Whether the programs the IDE starts (runs, language servers, plug-in tool windows \
         and targets, the terminal) get the person's login-shell environment, read once at start, and the names of \
         its variables (never their values), or why it could not be read.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LoginEnvironment, CommandError> {
        sync(session);
        Ok(status(session))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetLoginEnvironment)?;
    Ok(())
}
