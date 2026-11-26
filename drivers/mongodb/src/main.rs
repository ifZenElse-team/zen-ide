use std::time::Duration;

use serde_json::{Value as Json, json};
use zen_db_driver_kit::{BoxFuture, Connected, Driver, DriverInfo, Failure};

mod auth;
mod connect;
mod dns;
mod session;
mod shell;
mod target;
mod values;
mod wire;

struct MongoDb;

fn capabilities() -> Json {
    json!({
        "models": ["document"],
        "language": "mongo",
        "transactions": true,
        "catalogs": true,
        "schemas": false,
        "nodeKinds": ["catalog", "collection", "view", "index"],
        "explain": false,
        "serverCancel": true,
        "bindParameters": false,
        "rowIdentity": "_id",
        "ddlPreview": false,
        "serverPaging": true,
        "sessions": false,
        "readOnlySessions": true,
        "uriSchemes": ["mongodb", "mongodb+srv"],
        "sharedConnection": false,
        "password": true,
        "settings": [
            { "key": "authSource", "kind": "text", "default": "admin" },
            { "key": "authMechanism", "kind": "choice", "choices": ["DEFAULT", "SCRAM-SHA-256", "SCRAM-SHA-1", "MONGODB-X509"], "default": "DEFAULT" },
            { "key": "replicaSet", "kind": "text" },
            { "key": "directConnection", "kind": "bool", "default": "false" }
        ],
        "introspection": false
    })
}

impl Driver for MongoDb {
    fn info(&self) -> DriverInfo {
        DriverInfo { id: "mongodb", name: "MongoDB", version: env!("CARGO_PKG_VERSION"), capabilities: capabilities() }
    }

    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>> {
        Box::pin(async move {
            let target = target::Target::of(&params)?;
            let timeout =
                Duration::from_millis(params.get("timeoutMs").and_then(Json::as_u64).unwrap_or(15_000).max(1));
            let link = connect::open(&target, timeout).await?;
            let state = params.get("state").cloned().unwrap_or(Json::Null);
            let mut server = session::Server::new(link, target, timeout, &state);
            if params.get("readOnly").and_then(Json::as_bool) == Some(true) {
                server.set_read_only();
            }
            let version = server.version().await;
            Ok(Connected { server_version: Some(version), in_transaction: false, session: Box::new(server) })
        })
    }
}

fn main() {
    zen_db_driver_kit::main(MongoDb);
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ_VERBS: &[&str] = &[
        "find",
        "findOne",
        "countDocuments",
        "estimatedDocumentCount",
        "count",
        "distinct",
        "getIndexes",
        "getCollectionNames",
        "getCollectionInfos",
        "stats",
        "show",
        "use",
    ];

    #[test]
    fn the_capabilities_name_the_document_model_and_the_language() {
        let capabilities = capabilities();
        assert_eq!(capabilities["models"][0], "document");
        assert_eq!(capabilities["language"], "mongo");
        assert_eq!(MongoDb.info().id, "mongodb");
    }

    #[test]
    fn the_manifest_reads_with_the_verbs_and_capabilities_the_program_answers() {
        let manifest: Json = serde_json::from_str(include_str!("../../../plugins/mongodb/plugin.json")).unwrap();
        let driver = &manifest["databaseDrivers"][0];
        assert_eq!(driver["id"], "mongodb");
        assert_eq!(driver["capabilities"]["language"], capabilities()["language"]);
        assert_eq!(driver["capabilities"]["models"], capabilities()["models"]);
        assert_eq!(driver["capabilities"]["rowIdentity"], capabilities()["rowIdentity"]);
        let verbs: Vec<&str> = driver["readVerbs"].as_array().unwrap().iter().filter_map(Json::as_str).collect();
        assert_eq!(verbs, READ_VERBS);
    }
}
