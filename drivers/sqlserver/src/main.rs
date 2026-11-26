use serde_json::{Value as Json, json};
use zen_db_driver_kit::{BoxFuture, Connected, Driver, DriverInfo, Failure};

mod catalog;
mod connect;
mod link;
mod session;
mod values;
mod words;

struct SqlServer;

fn capabilities() -> Json {
    json!({
        "models": ["relational"],
        "language": "sql:sqlserver",
        "transactions": true,
        "isolationLevels": session::ISOLATION_LEVELS,
        "catalogs": true,
        "schemas": true,
        "nodeKinds": ["catalog", "schema", "table", "view", "column"],
        "explain": false,
        "serverCancel": true,
        "bindParameters": true,
        "rowIdentity": "primaryKey",
        "ddlPreview": false,
        "serverPaging": true,
        "sessions": false,
        "readOnlySessions": false,
        "uriSchemes": ["sqlserver"],
        "sharedConnection": false,
        "password": true,
        "settings": [
            { "key": "authentication", "kind": "choice", "choices": ["password", "accessToken"], "default": "password" },
            { "key": "strict", "kind": "bool", "default": "false" }
        ],
        "introspection": true
    })
}

impl Driver for SqlServer {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: "sqlserver",
            name: "SQL Server",
            version: env!("CARGO_PKG_VERSION"),
            capabilities: capabilities(),
        }
    }

    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>> {
        Box::pin(async move {
            let target = connect::Target::of(&params)?;
            let client = connect::open(&target).await?;
            let state = params.get("state").cloned().unwrap_or(Json::Null);
            let (server, version) = session::Server::start(client, &state).await?;
            Ok(Connected { server_version: Some(version), in_transaction: false, session: Box::new(server) })
        })
    }
}

fn main() {
    zen_db_driver_kit::main(SqlServer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_name_the_language_and_the_settings() {
        let capabilities = capabilities();
        assert_eq!(capabilities["language"], "sql:sqlserver");
        assert_eq!(capabilities["isolationLevels"][3], "SNAPSHOT");
        assert_eq!(SqlServer.info().id, "sqlserver");
    }
}
