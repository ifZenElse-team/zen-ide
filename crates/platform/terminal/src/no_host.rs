use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host;

impl Host {
    pub fn installed(_dir: impl Into<PathBuf>) -> Option<Host> {
        None
    }

    pub fn forget_all(&self) {}

    pub fn forget_older_than(&self, _age: Duration) {}

    pub fn agents_folder(&self) -> Option<PathBuf> {
        None
    }

    pub fn agent_report(&self, _terminal: &str, _agent: &str) -> Option<serde_json::Value> {
        None
    }

    pub fn report_agent(&self, _terminal: &str, _agent: &str, _report: &serde_json::Value) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub fn forget_agents(&self, _terminal: &str) {}
}
