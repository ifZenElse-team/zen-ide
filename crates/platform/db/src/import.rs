use serde_json::Value as Json;

use crate::registry::{SshSettings, parse_uri};
use crate::types::{DEVELOPMENT, PRODUCTION, TEST};

pub const FILE_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub key: String,
    pub name: String,
    pub folder: Option<String>,
    pub driver: Option<String>,
    pub product: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub connection_type: Option<String>,
    pub read_only: bool,
    pub ssh: Option<SshSettings>,
}

fn known_driver(text: &str) -> Option<&'static str> {
    let text = text.to_ascii_lowercase();
    let starts = |prefix: &str| text.starts_with(prefix);
    Some(if starts("postgres") {
        "postgresql"
    } else if starts("mariadb") {
        "mariadb"
    } else if starts("mysql") {
        "mysql"
    } else if starts("sqlite") {
        "sqlite"
    } else if starts("redis") {
        "redis"
    } else if starts("clickhouse") || starts("com_clickhouse") {
        "clickhouse"
    } else if starts("mongo") {
        "mongodb"
    } else {
        return None;
    })
}

fn json_text(value: &Json) -> Option<String> {
    match value {
        Json::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Json::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn from_url(entry: &mut Imported, url: &str) {
    let url = url.trim();
    let url = url.strip_prefix("jdbc:").unwrap_or(url);
    let Some(parsed) = parse_uri(url) else { return };
    entry.host = entry.host.take().or(parsed.host);
    entry.port = entry.port.or(parsed.port);
    entry.database = entry.database.take().or(parsed.database);
    entry.user = entry.user.take().or(parsed.user);
}

pub fn dbeaver(text: &str) -> Result<Vec<Imported>, String> {
    let root: Json = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let Some(connections) = root.get("connections").and_then(Json::as_object) else {
        return Err("no \"connections\" object".to_owned());
    };
    let mut found = Vec::new();
    for (key, connection) in connections {
        let configuration = connection.get("configuration").cloned().unwrap_or(Json::Null);
        let provider = connection.get("provider").and_then(json_text).unwrap_or_default();
        let driver_id = connection.get("driver").and_then(json_text).unwrap_or_default();
        let mut entry = Imported {
            key: key.clone(),
            name: connection.get("name").and_then(json_text).unwrap_or_else(|| key.clone()),
            folder: connection.get("folder").and_then(json_text),
            driver: known_driver(&provider).or_else(|| known_driver(&driver_id)).map(str::to_owned),
            product: if driver_id.is_empty() { provider.clone() } else { driver_id.clone() },
            host: configuration.get("host").and_then(json_text),
            port: configuration.get("port").and_then(json_text).and_then(|port| port.parse().ok()),
            database: configuration.get("database").and_then(json_text),
            user: configuration.get("user").and_then(json_text),
            connection_type: configuration.get("type").and_then(json_text).and_then(|kind| match kind.as_str() {
                "dev" => Some(DEVELOPMENT.to_owned()),
                "test" => Some(TEST.to_owned()),
                "prod" => Some(PRODUCTION.to_owned()),
                _ => None,
            }),
            read_only: connection.get("read-only").and_then(Json::as_bool).unwrap_or(false),
            ssh: None,
        };
        if let Some(url) = configuration.get("url").and_then(json_text) {
            from_url(&mut entry, &url);
        }
        if let Some(tunnel) = configuration.get("handlers").and_then(|handlers| handlers.get("ssh_tunnel"))
            && tunnel.get("enabled").and_then(Json::as_bool).unwrap_or(false)
        {
            let properties = tunnel.get("properties").cloned().unwrap_or(Json::Null);
            if let Some(host) = properties.get("host").and_then(json_text) {
                entry.ssh = Some(SshSettings {
                    host,
                    user: tunnel.get("user").and_then(json_text).or_else(|| properties.get("user").and_then(json_text)),
                    port: properties
                        .get("port")
                        .and_then(json_text)
                        .and_then(|port| port.parse().ok())
                        .filter(|port| *port != 22),
                    jump: None,
                });
            }
        }
        found.push(entry);
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

pub fn jetbrains(shared: &str, local: Option<&str>) -> Result<Vec<Imported>, String> {
    let document = roxmltree::Document::parse(shared).map_err(|error| error.to_string())?;
    let local = local.and_then(|local| roxmltree::Document::parse(local).ok());
    let user_of = |uuid: &str| -> Option<String> {
        let local = local.as_ref()?;
        let source = local
            .descendants()
            .find(|node| node.has_tag_name("data-source") && node.attribute("uuid") == Some(uuid))?;
        let user = source.children().find(|node| node.has_tag_name("user-name"))?;
        user.text().map(str::trim).filter(|user| !user.is_empty()).map(str::to_owned)
    };
    let child = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|child| child.has_tag_name(name))
            .and_then(|child| child.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let mut found = Vec::new();
    for source in document.descendants().filter(|node| node.has_tag_name("data-source")) {
        let uuid = source.attribute("uuid").unwrap_or_default().to_owned();
        let name = source.attribute("name").unwrap_or(&uuid).to_owned();
        let driver_ref = child(source, "driver-ref").unwrap_or_default();
        let url = child(source, "jdbc-url").unwrap_or_default();
        let scheme = url.strip_prefix("jdbc:").unwrap_or(&url).split(':').next().unwrap_or_default().to_owned();
        let mut entry = Imported {
            key: if uuid.is_empty() { name.clone() } else { uuid.clone() },
            name,
            folder: source.attribute("group").map(str::to_owned).filter(|group| !group.is_empty()),
            driver: known_driver(&driver_ref).or_else(|| known_driver(&scheme)).map(str::to_owned),
            product: if driver_ref.is_empty() { scheme } else { driver_ref },
            read_only: source.attribute("read-only") == Some("true"),
            user: user_of(&uuid).or_else(|| child(source, "user-name")),
            ..Imported::default()
        };
        from_url(&mut entry, &url);
        found.push(entry);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbeaver_connections_are_read_without_credentials() {
        let text = r#"{
            "folders": { "Team": {} },
            "connections": {
                "postgres-jdbc-1": {
                    "provider": "postgresql", "driver": "postgres-jdbc", "name": "Shop", "folder": "Team",
                    "read-only": true,
                    "configuration": {
                        "host": "db.local", "port": "5433", "database": "shop", "url": "jdbc:postgresql://db.local:5433/shop",
                        "type": "prod", "auth-model": "native",
                        "handlers": { "ssh_tunnel": { "enabled": true, "properties": { "host": "bastion", "port": 2222 } } }
                    }
                },
                "mysql8-2": { "provider": "mysql", "driver": "mysql8", "name": "Legacy",
                    "configuration": { "url": "jdbc:mysql://127.0.0.1:3307/legacy", "type": "dev" } },
                "sqlite-3": { "provider": "generic", "driver": "sqlite_jdbc", "name": "Local",
                    "configuration": { "url": "jdbc:sqlite:/tmp/app.db" } },
                "oracle-4": { "provider": "oracle", "driver": "oracle_thin", "name": "Erp", "configuration": { "host": "erp" } }
            }
        }"#;
        let found = dbeaver(text).unwrap();
        assert_eq!(found.len(), 4);
        let shop = found.iter().find(|entry| entry.name == "Shop").unwrap();
        assert_eq!(
            (shop.driver.as_deref(), shop.host.as_deref(), shop.port),
            (Some("postgresql"), Some("db.local"), Some(5433))
        );
        assert_eq!(
            (shop.folder.as_deref(), shop.connection_type.as_deref(), shop.read_only),
            (Some("Team"), Some("prod"), true)
        );
        assert_eq!(shop.ssh.as_ref().map(|ssh| (ssh.host.as_str(), ssh.port)), Some(("bastion", Some(2222))));
        let legacy = found.iter().find(|entry| entry.name == "Legacy").unwrap();
        assert_eq!(
            (legacy.driver.as_deref(), legacy.port, legacy.database.as_deref()),
            (Some("mysql"), Some(3307), Some("legacy"))
        );
        let local = found.iter().find(|entry| entry.name == "Local").unwrap();
        assert_eq!((local.driver.as_deref(), local.database.as_deref()), (Some("sqlite"), Some("/tmp/app.db")));
        assert_eq!(found.iter().find(|entry| entry.name == "Erp").unwrap().driver, None);
        assert!(dbeaver("{}").is_err());
        assert!(dbeaver("not json").is_err());
    }

    #[test]
    fn jetbrains_data_sources_are_read_with_local_user_names() {
        let shared = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
    <data-source source="LOCAL" name="shop@localhost" uuid="a1">
      <driver-ref>postgresql</driver-ref>
      <synchronize>true</synchronize>
      <jdbc-driver>org.postgresql.Driver</jdbc-driver>
      <jdbc-url>jdbc:postgresql://localhost:5432/shop</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="cache" uuid="b2">
      <driver-ref>redis</driver-ref>
      <jdbc-url>jdbc:redis://localhost:6379/0</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="warehouse" uuid="c3">
      <driver-ref>snowflake</driver-ref>
      <jdbc-url>jdbc:snowflake://acme.snowflakecomputing.com</jdbc-url>
    </data-source>
  </component>
</project>"#;
        let local = r#"<project version="4"><component name="dataSourceStorageLocal">
            <data-source name="shop@localhost" uuid="a1"><secret-storage>master_key</secret-storage><user-name>ann</user-name></data-source>
        </component></project>"#;
        let found = jetbrains(shared, Some(local)).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].key, "a1");
        assert_eq!((found[0].driver.as_deref(), found[0].user.as_deref()), (Some("postgresql"), Some("ann")));
        assert_eq!(
            (found[0].host.as_deref(), found[0].port, found[0].database.as_deref()),
            (Some("localhost"), Some(5432), Some("shop"))
        );
        assert_eq!(found[1].driver.as_deref(), Some("redis"));
        assert_eq!((found[2].driver.as_deref(), found[2].product.as_str()), (None, "snowflake"));
        assert!(jetbrains("<project", None).is_err());
    }
}
