use serde_json::{Value as Json, json};
use zen_db_driver_kit::{BoxFuture, Connected, Driver, DriverInfo, Failure};

mod process;
mod session;
mod statement;
mod values;

struct DuckDb;

fn capabilities() -> Json {
    json!({
        "models": ["relational"],
        "language": "sql:duckdb",
        "transactions": true,
        "catalogs": true,
        "schemas": true,
        "nodeKinds": ["catalog", "schema", "table", "view", "column"],
        "explain": false,
        "serverCancel": true,
        "bindParameters": false,
        "ddlPreview": false,
        "serverPaging": false,
        "sessions": false,
        "readOnlySessions": false,
        "uriSchemes": ["duckdb"],
        "sharedConnection": false,
        "password": false,
        "settings": [
            { "key": "file", "kind": "path" },
            { "key": "program", "kind": "path" }
        ],
        "introspection": false
    })
}

fn file_of(params: &Json) -> String {
    let setting = params.pointer("/settings/file").and_then(Json::as_str).filter(|file| !file.trim().is_empty());
    let from_url = params.get("url").and_then(Json::as_str).and_then(|url| url.strip_prefix("duckdb:")).map(|rest| {
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        if rest == ":memory:" { String::new() } else { rest.to_owned() }
    });
    setting
        .map(str::to_owned)
        .or(from_url)
        .or_else(|| params.get("database").and_then(Json::as_str).map(str::to_owned))
        .unwrap_or_default()
}

impl Driver for DuckDb {
    fn info(&self) -> DriverInfo {
        DriverInfo { id: "duckdb", name: "DuckDB", version: env!("CARGO_PKG_VERSION"), capabilities: capabilities() }
    }

    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>> {
        Box::pin(async move {
            let program = process::find_program(params.pointer("/settings/program").and_then(Json::as_str))?;
            let target = session::Target {
                program,
                file: file_of(&params),
                read_only: params.get("readOnly").and_then(Json::as_bool) == Some(true),
            };
            let started = process::Process::start(&target.program, &target.file, target.read_only).await?;
            let mut server = session::Server::new(target, started);
            if let Some(state) = params.get("state") {
                server.start_in(state).await?;
            }
            Ok(Connected { server_version: Some(server.version()), in_transaction: false, session: Box::new(server) })
        })
    }
}

fn main() {
    zen_db_driver_kit::main(DuckDb);
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ_VERBS: &[&str] =
        &["SELECT", "WITH", "FROM", "VALUES", "TABLE", "SHOW", "DESCRIBE", "SUMMARIZE", "EXPLAIN", "PIVOT", "UNPIVOT"];

    #[test]
    fn the_file_comes_from_the_setting_the_address_or_the_database() {
        assert_eq!(file_of(&json!({ "settings": { "file": "/data/a.duckdb" }, "database": "x" })), "/data/a.duckdb");
        assert_eq!(file_of(&json!({ "url": "duckdb:///data/b.duckdb" })), "/data/b.duckdb");
        assert_eq!(file_of(&json!({ "url": "duckdb::memory:" })), "");
        assert_eq!(file_of(&json!({ "database": "/data/c.duckdb" })), "/data/c.duckdb");
        assert_eq!(file_of(&json!({})), "");
    }

    #[test]
    fn the_manifest_reads_with_the_verbs_and_capabilities_the_program_answers() {
        let manifest: Json = serde_json::from_str(include_str!("../../../plugins/duckdb/plugin.json")).unwrap();
        let driver = &manifest["databaseDrivers"][0];
        assert_eq!(driver["id"], DuckDb.info().id);
        assert_eq!(driver["capabilities"]["language"], capabilities()["language"]);
        assert_eq!(driver["capabilities"]["settings"], capabilities()["settings"]);
        let verbs: Vec<&str> = driver["readVerbs"].as_array().unwrap().iter().filter_map(Json::as_str).collect();
        assert_eq!(verbs, READ_VERBS);
    }
}
