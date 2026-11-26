use serde_json::{Value as Json, json};
use zen_db_driver_kit::{BoxFuture, Connected, Driver, DriverInfo, Failure};

mod catalog;
mod json;
mod output;
mod process;
mod program;
mod session;
mod statement;
mod target;
mod values;

struct Oracle;

fn capabilities() -> Json {
    json!({
        "models": ["relational"],
        "language": "sql:oracle",
        "transactions": true,
        "isolationLevels": session::ISOLATION_LEVELS,
        "catalogs": false,
        "schemas": true,
        "nodeKinds": ["schema", "table", "view", "column"],
        "explain": false,
        "serverCancel": true,
        "bindParameters": false,
        "rowIdentity": "primaryKey",
        "ddlPreview": false,
        "serverPaging": false,
        "sessions": false,
        "readOnlySessions": false,
        "uriSchemes": [],
        "sharedConnection": false,
        "password": true,
        "settings": [
            { "key": "connectionType", "kind": "choice", "choices": target::CONNECTION_TYPES, "default": "serviceName" },
            { "key": "role", "kind": "choice", "choices": target::ROLES, "default": "normal" },
            { "key": "tnsAlias", "kind": "text" },
            { "key": "tnsAdmin", "kind": "path" },
            { "key": "connectString", "kind": "text" },
            { "key": "wallet", "kind": "path" },
            { "key": "program", "kind": "path" },
            { "key": "javaHome", "kind": "path" }
        ],
        "introspection": true
    })
}

impl Driver for Oracle {
    fn info(&self) -> DriverInfo {
        DriverInfo { id: "oracle", name: "Oracle", version: env!("CARGO_PKG_VERSION"), capabilities: capabilities() }
    }

    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>> {
        Box::pin(async move {
            let target = target::Target::of(&params)?;
            let state = params.get("state").cloned().unwrap_or(Json::Null);
            let server = session::Server::start(target, &state).await?;
            Ok(Connected { server_version: Some(server.version()), in_transaction: false, session: Box::new(server) })
        })
    }
}

fn main() {
    zen_db_driver_kit::main(Oracle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_declares_what_the_program_answers() {
        let manifest: Json = serde_json::from_str(include_str!("../../../plugins/oracle/plugin.json")).unwrap();
        let driver = &manifest["databaseDrivers"][0];
        assert_eq!(driver["id"], Oracle.info().id);
        assert_eq!(driver["defaultPort"], target::DEFAULT_PORT);
        for key in ["language", "settings", "isolationLevels", "rowIdentity", "introspection", "password"] {
            assert_eq!(driver["capabilities"][key], capabilities()[key], "{key}");
        }
    }
}
