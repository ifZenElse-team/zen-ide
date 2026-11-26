use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use zen_core::{Command, CommandError, Session};

/// The parameters of `mcp.session.stats`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsParams {
    /// Whether to start counting again after this answer.
    #[serde(default)]
    pub reset: bool,
}

pub type Report = Arc<dyn Fn(bool) -> Value + Send + Sync>;

pub struct McpSessionStats(pub Report);

impl Command for McpSessionStats {
    type Params = StatsParams;
    type Output = Value;
    const ID: &'static str = "mcp.session.stats";
    const TITLE: &'static str = "MCP Session Statistics";
    const DESCRIPTION: &'static str = "What the agents' MCP calls cost since the server started (or the last `reset`): \
         {total, tools}, each with calls, errors, bytesIn (arguments), bytesOut (the answers' text) and elapsedMs; a \
         command run through dispatch_command counts under its id. Every answer also carries its own time in \
         `_meta.elapsedMs`.";

    fn run(&self, _: &mut Session, params: StatsParams) -> Result<Value, CommandError> {
        Ok((self.0)(params.reset))
    }
}
