use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{CommandError, CommandId, Session};

pub trait Command: Send + Sync + 'static {
    type Params: DeserializeOwned + JsonSchema;
    type Output: Serialize;

    const ID: &'static str;
    const TITLE: &'static str;
    const DESCRIPTION: &'static str;

    fn is_available(&self, _session: &Session) -> bool {
        true
    }

    fn run(&self, session: &mut Session, params: Self::Params) -> Result<Self::Output, CommandError>;
}

/// The parameters of a command that takes none. Accepts `{}` and `null`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct NoParams {}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub id: CommandId,
    pub title: String,
    pub description: String,
    pub params_schema: serde_json::Value,
    pub aliases: Vec<CommandId>,
}
