pub mod yaml;

use std::collections::HashMap;

use zen_secrets::Secret;

use crate::registry::{OriginKind, parse_uri};

pub const FILE_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Debug)]
pub struct Finding {
    pub id: String,
    pub kind: OriginKind,
    pub file: String,
    pub key: String,
    pub driver: String,
    pub product: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub url: Option<String>,
    pub published: bool,
    pub files: Vec<String>,
    pub password: Option<Secret>,
}

impl Finding {
    pub fn fingerprint(&self) -> String {
        let port = self.port.map(|port| port.to_string()).unwrap_or_default();
        let parts = [
            self.driver.as_str(),
            self.host.as_deref().unwrap_or_default(),
            port.as_str(),
            self.database.as_deref().unwrap_or_default(),
            self.user.as_deref().unwrap_or_default(),
            self.url.as_deref().unwrap_or_default(),
        ];
        format!("{:016x}", fnv(&parts.join("\u{1f}")))
    }

    pub fn name(&self) -> String {
        match self.kind {
            OriginKind::Compose => self.key.clone(),
            _ => match (&self.database, &self.host) {
                (Some(database), _) => format!("{} - {database}", self.product),
                (None, Some(host)) => format!("{} - {host}", self.product),
                (None, None) => self.product.clone(),
            },
        }
    }
}

pub fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn id_of(kind: OriginKind, file: &str, key: &str) -> String {
    let kind = match kind {
        OriginKind::Env => "env",
        OriginKind::Compose => "compose",
        OriginKind::Dbeaver => "dbeaver",
        OriginKind::Jetbrains => "jetbrains",
    };
    format!("{kind}-{:012x}", fnv(&format!("{file}\u{1f}{key}")) & 0xffff_ffff_ffff)
}

pub fn is_candidate(name: &str) -> bool {
    is_env_file(name) || is_compose_file(name)
}

pub fn is_env_file(name: &str) -> bool {
    name == ".env" || name.strip_prefix(".env.").is_some_and(|rest| !rest.is_empty())
}

pub fn is_compose_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".yml").or_else(|| lower.strip_suffix(".yaml")) else { return false };
    ["docker-compose", "compose"].iter().any(|base| stem == *base || stem.strip_prefix(&format!("{base}.")).is_some())
}

pub fn env_rank(name: &str) -> u8 {
    match name {
        ".env" => 0,
        _ if ["example", "sample", "template", "dist"].iter().any(|word| name.ends_with(word)) => 2,
        _ => 1,
    }
}

pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let plain = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'));
        if !plain {
            continue;
        }
        let value = value.trim();
        let (value, expand) = if let Some(rest) = value.strip_prefix('"') {
            let mut text = rest.to_owned();
            while !closed(&text, '"') {
                match lines.next() {
                    Some(next) => {
                        text.push('\n');
                        text.push_str(next);
                    }
                    None => break,
                }
            }
            (unescape(until_quote(&text, '"')), true)
        } else if let Some(rest) = value.strip_prefix('\'') {
            (until_quote(rest, '\'').to_owned(), false)
        } else {
            let value = match value.find(" #") {
                Some(at) => value[..at].trim_end(),
                None => value,
            };
            (value.to_owned(), true)
        };
        let lookup: HashMap<String, String> = pairs.iter().cloned().collect();
        let value = if expand { interpolate(&value, &lookup) } else { value };
        pairs.retain(|(known, _)| known != key);
        pairs.push((key.to_owned(), value));
    }
    pairs
}

fn closed(text: &str, quote: char) -> bool {
    let mut escaped = false;
    for c in text.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            c if c == quote && !escaped => return true,
            _ => escaped = false,
        }
    }
    false
}

fn until_quote(text: &str, quote: char) -> &str {
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        match c {
            '\\' if !escaped && quote == '"' => escaped = true,
            c if c == quote && !escaped => return &text[..at],
            _ => escaped = false,
        }
    }
    text
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub fn interpolate(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else if let Some(inner) = after.strip_prefix('{') {
            let Some(end) = inner.find('}') else {
                out.push_str(&rest[at..]);
                return out;
            };
            let expression = &inner[..end];
            let name_end =
                expression.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(expression.len());
            let (name, operator) = expression.split_at(name_end);
            let value = vars.get(name).map(String::as_str);
            let shown = if let Some(default) = operator.strip_prefix(":-") {
                value.filter(|value| !value.is_empty()).map_or_else(|| interpolate(default, vars), str::to_owned)
            } else if let Some(default) = operator.strip_prefix('-') {
                value.map_or_else(|| interpolate(default, vars), str::to_owned)
            } else if let Some(alternative) = operator.strip_prefix(":+") {
                if value.is_some_and(|value| !value.is_empty()) {
                    interpolate(alternative, vars)
                } else {
                    String::new()
                }
            } else {
                value.unwrap_or_default().to_owned()
            };
            out.push_str(&shown);
            rest = &inner[end + 1..];
        } else {
            let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
            if end == 0 {
                out.push('$');
            } else {
                out.push_str(vars.get(&after[..end]).map(String::as_str).unwrap_or_default());
            }
            rest = &after[end..];
        }
    }
    out.push_str(rest);
    out
}

pub fn scheme_driver(scheme: &str) -> Option<(&'static str, &'static str)> {
    let scheme = scheme.to_ascii_lowercase();
    let scheme = scheme.strip_prefix("jdbc:").unwrap_or(&scheme);
    let base = scheme.split('+').next().unwrap_or(scheme);
    Some(match base {
        "postgres" | "postgresql" | "pgsql" => ("postgresql", "PostgreSQL"),
        "mysql" | "mysqlx" => ("mysql", "MySQL"),
        "mariadb" => ("mariadb", "MariaDB"),
        "redis" | "rediss" => ("redis", "Redis"),
        "valkey" | "valkeys" => ("redis", "Valkey"),
        "mongodb" | "mongo" => ("mongodb", "MongoDB"),
        "clickhouse" => ("clickhouse", "ClickHouse"),
        "cassandra" => ("cassandra", "Cassandra"),
        _ => return None,
    })
}

fn url_key(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "ELASTICSEARCH_URL" | "ELASTICSEARCH_HOSTS" => Some(("elasticsearch", "Elasticsearch")),
        "OPENSEARCH_URL" => Some(("opensearch", "OpenSearch")),
        "DATABASE_URL" | "REDIS_URL" | "MONGODB_URI" | "MONGODB_URL" | "MONGO_URL" | "MONGO_URI" | "CLICKHOUSE_URL" => {
            Some(("", ""))
        }
        _ if key.ends_with("_DATABASE_URL") => Some(("", "")),
        _ => None,
    }
}

pub fn from_env(file: &str, text: &str) -> Vec<Finding> {
    let pairs = parse_env(text);
    let value = |names: &[&str]| -> Option<String> {
        names.iter().find_map(|name| {
            pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()).filter(|value| !value.is_empty())
        })
    };
    let mut found = Vec::new();
    for (key, raw) in &pairs {
        let Some((by_key, by_key_product)) = url_key(key) else { continue };
        let uri = raw.trim().strip_prefix("jdbc:").unwrap_or(raw.trim());
        let Some(parsed) = parse_uri(uri) else { continue };
        let (driver, product) = match scheme_driver(&parsed.scheme) {
            Some(known) => known,
            None if !by_key.is_empty() && parsed.scheme.starts_with("http") => (by_key, by_key_product),
            None => continue,
        };
        if parsed.host.is_none() && parsed.database.is_none() {
            continue;
        }
        found.push(Finding {
            id: id_of(OriginKind::Env, file, key),
            kind: OriginKind::Env,
            file: file.to_owned(),
            key: key.clone(),
            driver: driver.to_owned(),
            product: product.to_owned(),
            host: parsed.host.clone(),
            port: parsed.port,
            database: parsed.database.clone(),
            user: parsed.user.clone(),
            url: Some(parsed.without_password.clone()),
            published: true,
            files: vec![file.to_owned()],
            password: parsed.password.clone().filter(|password| !password.is_empty()).map(Secret::new),
        });
    }
    let groups: [(&str, &str, &str, [&[&str]; 5]); 3] = [
        ("PG*", "postgresql", "PostgreSQL", [&["PGHOST"], &["PGPORT"], &["PGDATABASE"], &["PGUSER"], &["PGPASSWORD"]]),
        (
            "POSTGRES_*",
            "postgresql",
            "PostgreSQL",
            [
                &["POSTGRES_HOST", "POSTGRES_HOSTNAME", "POSTGRES_SERVER"],
                &["POSTGRES_PORT"],
                &["POSTGRES_DB", "POSTGRES_DATABASE"],
                &["POSTGRES_USER", "POSTGRES_USERNAME"],
                &["POSTGRES_PASSWORD"],
            ],
        ),
        (
            "MYSQL_*",
            "mysql",
            "MySQL",
            [
                &["MYSQL_HOST"],
                &["MYSQL_PORT", "MYSQL_TCP_PORT"],
                &["MYSQL_DATABASE", "MYSQL_DB"],
                &["MYSQL_USER", "MYSQL_USERNAME"],
                &["MYSQL_PASSWORD"],
            ],
        ),
    ];
    for (group, driver, product, [host, port, database, user, password]) in groups {
        let (host, database, mut user, mut password) = (value(host), value(database), value(user), value(password));
        if group == "MYSQL_*" && user.is_none() {
            let root = value(&["MYSQL_ROOT_PASSWORD"]);
            if root.is_some() {
                user = Some("root".to_owned());
                password = password.or(root);
            }
        }
        if host.is_none() && database.is_none() && user.is_none() {
            continue;
        }
        found.push(Finding {
            id: id_of(OriginKind::Env, file, group),
            kind: OriginKind::Env,
            file: file.to_owned(),
            key: group.to_owned(),
            driver: driver.to_owned(),
            product: product.to_owned(),
            host: Some(host.unwrap_or_else(|| "localhost".to_owned())),
            port: value(port).and_then(|port| port.parse().ok()),
            database,
            user,
            url: None,
            published: true,
            files: vec![file.to_owned()],
            password: password.map(Secret::new),
        });
    }
    found
}

struct Image {
    driver: &'static str,
    product: &'static str,
    ports: &'static [u16],
}

fn image(reference: &str) -> Option<Image> {
    let reference = reference.split('@').next().unwrap_or(reference);
    let last = reference.rsplit('/').next().unwrap_or(reference);
    let name = last.split(':').next().unwrap_or(last).to_ascii_lowercase();
    let image = |driver, product, ports| Some(Image { driver, product, ports });
    match name.as_str() {
        "postgres" | "postgresql" | "postgis" | "timescaledb" | "timescaledb-ha" | "pgvector" => {
            image("postgresql", "PostgreSQL", &[5432])
        }
        "mysql" | "mysql-server" | "percona-server" => image("mysql", "MySQL", &[3306]),
        "mariadb" => image("mariadb", "MariaDB", &[3306]),
        "redis" | "redis-stack" | "redis-stack-server" => image("redis", "Redis", &[6379]),
        "valkey" | "valkey-bundle" => image("redis", "Valkey", &[6379]),
        "mongo" | "mongodb" | "mongodb-community-server" => image("mongodb", "MongoDB", &[27017]),
        "clickhouse-server" | "clickhouse" => image("clickhouse", "ClickHouse", &[8123, 9000]),
        "elasticsearch" => image("elasticsearch", "Elasticsearch", &[9200]),
        "opensearch" => image("opensearch", "OpenSearch", &[9200]),
        "cassandra" => image("cassandra", "Cassandra", &[9042]),
        "scylla" | "scylladb" => image("cassandra", "ScyllaDB", &[9042]),
        _ => None,
    }
}

fn published(ports: &yaml::Yaml, container: u16) -> Option<(Option<String>, u16)> {
    for entry in ports.items() {
        match entry {
            yaml::Yaml::Map(_) => {
                let target = entry.get("target").and_then(yaml::Yaml::scalar).and_then(|port| port.parse::<u16>().ok());
                if target != Some(container) {
                    continue;
                }
                let port = entry.get("published").and_then(yaml::Yaml::scalar)?;
                let port = port.split('-').next().unwrap_or(port).parse().ok()?;
                let host = entry.get("host_ip").and_then(yaml::Yaml::scalar).map(str::to_owned);
                return Some((host, port));
            }
            yaml::Yaml::Scalar(text) => {
                let text = text.split('/').next().unwrap_or(text);
                let (outside, inside) = match text.rsplit_once(':') {
                    Some((outside, inside)) => (Some(outside), inside),
                    None => (None, text),
                };
                if inside.parse::<u16>().ok() != Some(container) {
                    continue;
                }
                let outside = outside?;
                let (host, port) = match outside.rsplit_once(':') {
                    Some((host, port)) => (Some(host.trim_matches(['[', ']']).to_owned()), port),
                    None => (None, outside),
                };
                return port.parse().ok().map(|port| (host, port));
            }
            _ => {}
        }
    }
    None
}

fn environment(service: &yaml::Yaml, vars: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    match service.get("environment") {
        Some(yaml::Yaml::Map(entries)) => {
            for (key, value) in entries {
                let value = value.scalar().map(|value| interpolate(value, vars)).unwrap_or_default();
                out.insert(key.clone(), value);
            }
        }
        Some(list @ yaml::Yaml::List(_)) => {
            for item in list.items() {
                if let Some((key, value)) = item.scalar().and_then(|item| item.split_once('=')) {
                    out.insert(key.trim().to_owned(), interpolate(value, vars));
                }
            }
        }
        _ => {}
    }
    out
}

pub fn from_compose(file: &str, text: &str, vars: &HashMap<String, String>, env_file: Option<&str>) -> Vec<Finding> {
    let root = yaml::parse(text);
    let Some(yaml::Yaml::Map(services)) = root.get("services") else { return Vec::new() };
    let mut found = Vec::new();
    for (service, settings) in services {
        let Some(reference) = settings.get("image").and_then(yaml::Yaml::scalar) else { continue };
        let Some(kind) = image(&interpolate(reference, vars)) else { continue };
        let env = environment(settings, vars);
        let get =
            |names: &[&str]| names.iter().find_map(|name| env.get(*name).filter(|value| !value.is_empty()).cloned());
        let (mut user, mut password, mut database) = (None, None, None);
        match kind.product {
            "PostgreSQL" => {
                user = get(&["POSTGRES_USER", "POSTGRESQL_USERNAME"]).or_else(|| Some("postgres".to_owned()));
                password = get(&["POSTGRES_PASSWORD", "POSTGRESQL_PASSWORD"]);
                database = get(&["POSTGRES_DB", "POSTGRESQL_DATABASE"]).or_else(|| user.clone());
            }
            "MySQL" | "MariaDB" => {
                database = get(&["MARIADB_DATABASE", "MYSQL_DATABASE"]);
                match get(&["MARIADB_USER", "MYSQL_USER"]) {
                    Some(name) => {
                        user = Some(name);
                        password = get(&["MARIADB_PASSWORD", "MYSQL_PASSWORD"]);
                    }
                    None => {
                        user = Some("root".to_owned());
                        password = get(&["MARIADB_ROOT_PASSWORD", "MYSQL_ROOT_PASSWORD"]);
                    }
                }
            }
            "Redis" | "Valkey" => password = get(&["REDIS_PASSWORD", "VALKEY_PASSWORD"]),
            "MongoDB" => {
                user = get(&["MONGO_INITDB_ROOT_USERNAME"]);
                password = get(&["MONGO_INITDB_ROOT_PASSWORD"]);
                database = get(&["MONGO_INITDB_DATABASE"]);
            }
            "ClickHouse" => {
                user = get(&["CLICKHOUSE_USER"]).or_else(|| Some("default".to_owned()));
                password = get(&["CLICKHOUSE_PASSWORD"]);
                database = get(&["CLICKHOUSE_DB"]);
            }
            "Elasticsearch" => {
                password = get(&["ELASTIC_PASSWORD"]);
                user = password.as_ref().map(|_| "elastic".to_owned());
            }
            "OpenSearch" => {
                password = get(&["OPENSEARCH_INITIAL_ADMIN_PASSWORD"]);
                user = password.as_ref().map(|_| "admin".to_owned());
            }
            _ => {}
        }
        let ports = settings.get("ports").cloned().unwrap_or(yaml::Yaml::Null);
        let ports = ports.map_scalars(&|text| interpolate(text, vars));
        let exposed = kind.ports.iter().find_map(|port| published(&ports, *port));
        let (host, port, is_published) = match exposed {
            Some((host, port)) => {
                let host = host.filter(|host| !host.is_empty() && host != "0.0.0.0" && host != "::");
                (host.unwrap_or_else(|| "localhost".to_owned()), Some(port), true)
            }
            None => ("localhost".to_owned(), None, false),
        };
        let mut files = vec![file.to_owned()];
        files.extend(env_file.map(str::to_owned));
        found.push(Finding {
            id: id_of(OriginKind::Compose, file, service),
            kind: OriginKind::Compose,
            file: file.to_owned(),
            key: service.clone(),
            driver: kind.driver.to_owned(),
            product: kind.product.to_owned(),
            host: Some(host),
            port,
            database,
            user,
            url: None,
            published: is_published,
            files,
            password: password.map(Secret::new),
        });
    }
    found
}

#[cfg(test)]
mod tests;
