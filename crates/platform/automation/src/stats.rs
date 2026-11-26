use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStats {
    pub calls: u64,
    pub errors: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub elapsed_ms: u64,
}

impl ToolStats {
    fn add(&mut self, other: &ToolStats) {
        self.calls += other.calls;
        self.errors += other.errors;
        self.bytes_in += other.bytes_in;
        self.bytes_out += other.bytes_out;
        self.elapsed_ms += other.elapsed_ms;
    }
}

#[derive(Debug, Default)]
pub struct Stats {
    tools: Mutex<BTreeMap<String, ToolStats>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub total: ToolStats,
    pub tools: BTreeMap<String, ToolStats>,
}

impl Stats {
    pub fn count(&self, tool: &str, bytes_in: usize, bytes_out: usize, elapsed: Duration, error: bool) {
        let mut tools = self.tools.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = tools.entry(tool.to_owned()).or_default();
        entry.calls += 1;
        entry.errors += u64::from(error);
        entry.bytes_in += bytes_in as u64;
        entry.bytes_out += bytes_out as u64;
        entry.elapsed_ms += u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    }

    pub fn report(&self, reset: bool) -> Report {
        let mut tools = self.tools.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut total = ToolStats::default();
        for stats in tools.values() {
            total.add(stats);
        }
        let report = Report { total, tools: tools.clone() };
        if reset {
            tools.clear();
        }
        report
    }
}

pub fn counted_name(tool: &str, arguments: &Value) -> String {
    match arguments.get("command").and_then(Value::as_str) {
        Some(command) if tool == "dispatch_command" => command.to_owned(),
        _ => tool.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_counted_per_tool_and_in_all() {
        let stats = Stats::default();
        stats.count("get_document_text", 30, 1200, Duration::from_millis(3), false);
        stats.count("get_document_text", 30, 800, Duration::from_millis(2), false);
        stats.count("editor.text.replace", 120, 25, Duration::from_millis(9), true);
        let report = stats.report(true);
        assert_eq!(report.tools["get_document_text"].calls, 2);
        assert_eq!(report.tools["get_document_text"].bytes_out, 2000);
        assert_eq!(report.total.calls, 3);
        assert_eq!(report.total.errors, 1);
        assert_eq!(report.total.elapsed_ms, 14);
        assert_eq!(stats.report(false), Report::default(), "reset");
        let dispatch = serde_json::json!({ "command": "problems.file.list", "params": {} });
        assert_eq!(counted_name("dispatch_command", &dispatch), "problems.file.list");
        assert_eq!(counted_name("list_windows", &dispatch), "list_windows");
    }
}
