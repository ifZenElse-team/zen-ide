use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::Capabilities;
use crate::registry::{DataSource, TlsMode};

/// How a profile's structure is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Catalogue {
    /// The base driver's own catalogue queries (`pg_catalog`, MySQL's `information_schema`).
    Native,
    /// The standard `information_schema` views: schemas, tables, views, columns and keys only.
    InformationSchema,
    /// QuestDB's meta functions `tables()` and `table_columns()`.
    MetaFunctions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quirks {
    pub catalogue: Catalogue,
    pub fingerprints: bool,
    pub guards: bool,
    pub transactions: bool,
    pub all_databases: bool,
    pub hidden: &'static [&'static str],
}

impl Quirks {
    pub const NATIVE: Quirks = Quirks {
        catalogue: Catalogue::Native,
        fingerprints: true,
        guards: true,
        transactions: true,
        all_databases: true,
        hidden: &[],
    };
}

const TABLES_ONLY: &[&str] = &[
    "foreignTables",
    "materializedViews",
    "functions",
    "procedures",
    "sequences",
    "types",
    "events",
    "indexes",
    "triggers",
    "policies",
    "partitions",
];

const LIMITED: Quirks = Quirks {
    catalogue: Catalogue::InformationSchema,
    fingerprints: false,
    guards: false,
    transactions: true,
    all_databases: false,
    hidden: TABLES_ONLY,
};

const NO_ROUTINES: &[&str] = &["functions", "procedures", "triggers", "events"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: &'static str,
    pub name: &'static str,
    pub driver: &'static str,
    pub icon: &'static str,
    pub port: Option<u16>,
    pub database: Option<&'static str>,
    pub user: Option<&'static str>,
    pub schemes: &'static [&'static str],
    pub hosts: &'static [&'static str],
    pub markers: &'static [&'static str],
    pub tls: TlsMode,
    pub settings: &'static [&'static str],
    pub quirks: Quirks,
    pub docs: &'static str,
}

const fn pg(id: &'static str, name: &'static str, docs: &'static str) -> Profile {
    Profile {
        id,
        name,
        driver: "postgresql",
        icon: "postgresql",
        port: Some(5432),
        database: Some("postgres"),
        user: Some("postgres"),
        schemes: &[],
        hosts: &[],
        markers: &[],
        tls: TlsMode::Prefer,
        settings: &[],
        quirks: Quirks::NATIVE,
        docs,
    }
}

const fn my(id: &'static str, name: &'static str, docs: &'static str) -> Profile {
    Profile {
        id,
        name,
        driver: "mysql",
        icon: "mysql",
        port: Some(3306),
        database: None,
        user: Some("root"),
        schemes: &[],
        hosts: &[],
        markers: &[],
        tls: TlsMode::Prefer,
        settings: &[],
        quirks: Quirks::NATIVE,
        docs,
    }
}

const fn kv(id: &'static str, name: &'static str, markers: &'static [&'static str], docs: &'static str) -> Profile {
    Profile {
        id,
        name,
        driver: "redis",
        icon: "redis",
        port: Some(6379),
        database: None,
        user: None,
        schemes: &[],
        hosts: &[],
        markers,
        tls: TlsMode::Prefer,
        settings: &[],
        quirks: Quirks::NATIVE,
        docs,
    }
}

pub const PROFILES: &[Profile] = &[
    Profile {
        port: Some(26257),
        database: Some("defaultdb"),
        user: Some("root"),
        schemes: &["cockroachdb"],
        hosts: &["*.cockroachlabs.cloud"],
        markers: &["CockroachDB"],
        quirks: Quirks { guards: true, ..LIMITED },
        ..pg("cockroachdb", "CockroachDB", "https://www.cockroachlabs.com/docs/stable/connection-parameters")
    },
    Profile {
        port: Some(5433),
        database: Some("yugabyte"),
        user: Some("yugabyte"),
        markers: &["-YB-", "YugabyteDB"],
        quirks: Quirks { fingerprints: false, ..Quirks::NATIVE },
        ..pg("yugabytedb", "YugabyteDB", "https://docs.yugabyte.com/preview/reference/configuration/default-ports/")
    },
    Profile {
        hosts: &["*.tsdb.cloud.timescale.com"],
        markers: &["TimescaleDB"],
        ..pg("timescaledb", "TimescaleDB", "https://docs.timescale.com/use-timescale/latest/integrations/psql/")
    },
    Profile {
        database: Some("neondb"),
        user: None,
        hosts: &["*.neon.tech"],
        tls: TlsMode::Require,
        ..pg("neon", "Neon", "https://neon.tech/docs/connect/connect-securely")
    },
    Profile {
        hosts: &["*.supabase.co", "*.pooler.supabase.com"],
        tls: TlsMode::Require,
        ..pg("supabase", "Supabase", "https://supabase.com/docs/guides/database/connecting-to-postgres")
    },
    Profile {
        port: Some(5439),
        database: Some("dev"),
        user: Some("awsuser"),
        schemes: &["redshift"],
        hosts: &["*.redshift.amazonaws.com", "*.redshift-serverless.amazonaws.com"],
        markers: &["Redshift"],
        tls: TlsMode::Require,
        quirks: LIMITED,
        ..pg(
            "redshift",
            "Amazon Redshift",
            "https://docs.aws.amazon.com/redshift/latest/mgmt/configuring-connections.html",
        )
    },
    Profile {
        user: Some("gpadmin"),
        markers: &["Greenplum"],
        ..pg(
            "greenplum",
            "Greenplum",
            "https://techdocs.broadcom.com/us/en/vmware-tanzu/data-solutions/tanzu-greenplum/7/greenplum-database/landing-index.html",
        )
    },
    Profile { tls: TlsMode::Require, ..pg("alloydb", "AlloyDB", "https://cloud.google.com/alloydb/docs/connect-psql") },
    Profile { markers: &["Citus"], ..pg("citus", "Citus", "https://docs.citusdata.com/en/stable/") },
    Profile {
        port: Some(6875),
        database: Some("materialize"),
        user: Some("materialize"),
        hosts: &["*.materialize.cloud"],
        markers: &["Materialize"],
        settings: &["pooler"],
        quirks: Quirks { transactions: false, ..LIMITED },
        ..pg("materialize", "Materialize", "https://materialize.com/docs/integrations/sql-clients/")
    },
    Profile {
        port: Some(4566),
        database: Some("dev"),
        user: Some("root"),
        hosts: &["*.risingwave.cloud"],
        markers: &["RisingWave"],
        settings: &["pooler"],
        quirks: Quirks { transactions: false, ..LIMITED },
        ..pg("risingwave", "RisingWave", "https://docs.risingwave.com/get-started/quickstart")
    },
    Profile {
        port: Some(8812),
        database: Some("qdb"),
        user: Some("admin"),
        markers: &["QuestDB"],
        settings: &["pooler"],
        quirks: Quirks { catalogue: Catalogue::MetaFunctions, transactions: false, ..LIMITED },
        ..pg("questdb", "QuestDB", "https://questdb.com/docs/reference/api/postgres/")
    },
    Profile {
        database: Some("doc"),
        user: Some("crate"),
        hosts: &["*.cratedb.net"],
        markers: &["CrateDB"],
        settings: &["pooler"],
        quirks: Quirks { transactions: false, ..LIMITED },
        ..pg("cratedb", "CrateDB", "https://cratedb.com/docs/crate/reference/en/latest/interfaces/postgres.html")
    },
    Profile {
        port: Some(4000),
        hosts: &["*.tidbcloud.com"],
        markers: &["TiDB"],
        quirks: Quirks { hidden: NO_ROUTINES, ..Quirks::NATIVE },
        ..my("tidb", "TiDB", "https://docs.pingcap.com/tidb/stable/dev-guide-connect-to-tidb")
    },
    Profile {
        hosts: &["*.svc.singlestore.com"],
        markers: &["SingleStore", "MemSQL"],
        quirks: Quirks { hidden: &["triggers", "events"], ..Quirks::NATIVE },
        ..my("singlestore", "SingleStore", "https://docs.singlestore.com/cloud/connect-to-singlestore/")
    },
    Profile {
        user: None,
        hosts: &["*.psdb.cloud"],
        tls: TlsMode::VerifyFull,
        quirks: Quirks { hidden: &["triggers", "events"], ..Quirks::NATIVE },
        ..my("planetscale", "PlanetScale", "https://planetscale.com/docs/vitess/connecting/connection-strings")
    },
    Profile {
        port: Some(15306),
        user: None,
        markers: &["Vitess"],
        quirks: Quirks { hidden: &["triggers", "events"], ..Quirks::NATIVE },
        ..my("vitess", "Vitess", "https://vitess.io/docs/get-started/local/")
    },
    Profile {
        port: Some(9030),
        markers: &["StarRocks"],
        quirks: Quirks { fingerprints: false, hidden: NO_ROUTINES, ..Quirks::NATIVE },
        ..my("starrocks", "StarRocks", "https://docs.starrocks.io/docs/quick_start/shared-nothing/")
    },
    Profile {
        port: Some(9030),
        markers: &["Doris"],
        quirks: Quirks { fingerprints: false, hidden: NO_ROUTINES, ..Quirks::NATIVE },
        ..my("doris", "Apache Doris", "https://doris.apache.org/docs/gettingStarted/quick-start")
    },
    Profile {
        markers: &["Percona"],
        ..my(
            "percona",
            "Percona Server for MySQL",
            "https://docs.percona.com/percona-server/8.4/quickstart-overview.html",
        )
    },
    Profile {
        user: Some("admin"),
        hosts: &["*.cluster-*.rds.amazonaws.com", "*.cluster-ro-*.rds.amazonaws.com"],
        markers: &["mysql_aurora"],
        ..my(
            "aurora-mysql",
            "Amazon Aurora MySQL",
            "https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/Aurora.Connecting.html",
        )
    },
    Profile { schemes: &["valkey"], ..kv("valkey", "Valkey", &["Valkey"], "https://valkey.io/topics/cli/") },
    kv("keydb", "KeyDB", &["KeyDB"], "https://docs.keydb.dev/docs/"),
    kv("dragonfly", "Dragonfly", &["Dragonfly"], "https://www.dragonflydb.io/docs/getting-started"),
    kv("garnet", "Garnet", &["Garnet"], "https://microsoft.github.io/garnet/docs/getting-started"),
];

pub fn family(driver: &str) -> &str {
    match driver {
        "mariadb" => "mysql",
        other => other,
    }
}

pub fn get(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|profile| profile.id == id)
}

pub fn of_source(source: &DataSource) -> Option<&'static Profile> {
    source.profile.as_deref().and_then(get)
}

pub fn quirks_of(source: &DataSource) -> Quirks {
    of_source(source).map_or(Quirks::NATIVE, |profile| profile.quirks)
}

fn glob(pattern: &str, text: &str) -> bool {
    fn walk(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|skip| walk(rest, &text[skip..])),
            Some((c, rest)) => text.first().is_some_and(|t| t.eq_ignore_ascii_case(c)) && walk(rest, &text[1..]),
        }
    }
    walk(pattern.as_bytes(), text.as_bytes())
}

pub fn for_scheme(scheme: &str) -> Option<(&'static Profile, bool)> {
    let scheme = scheme.to_ascii_lowercase();
    let plain = PROFILES.iter().find(|profile| profile.schemes.contains(&scheme.as_str()));
    if let Some(profile) = plain {
        return Some((profile, false));
    }
    let base = scheme.strip_suffix('s')?;
    PROFILES
        .iter()
        .find(|profile| profile.driver == "redis" && profile.schemes.contains(&base))
        .map(|profile| (profile, true))
}

pub fn for_host(driver: Option<&str>, host: &str) -> Option<&'static Profile> {
    let host = host.trim().trim_end_matches('.');
    PROFILES
        .iter()
        .filter(|profile| driver.is_none_or(|driver| family(profile.driver) == family(driver)))
        .find(|profile| profile.hosts.iter().any(|pattern| glob(pattern, host)))
}

pub fn for_url(scheme: &str, driver: Option<&str>, host: Option<&str>) -> Option<&'static Profile> {
    for_scheme(scheme)
        .map(|(profile, _)| profile)
        .or_else(|| host.and_then(|host| host.split(',').find_map(|host| for_host(driver, host))))
}

pub fn detect(driver: &str, version: &str, host: Option<&str>) -> Option<&'static Profile> {
    if let Some(found) = host.and_then(|host| for_host(Some(driver), host)) {
        return Some(found);
    }
    let version = version.to_ascii_lowercase();
    PROFILES
        .iter()
        .filter(|profile| family(profile.driver) == family(driver))
        .find(|profile| profile.markers.iter().any(|marker| version.contains(&marker.to_ascii_lowercase())))
}

pub fn marked_in(driver: &str, text: &str, shown: &str) -> Option<&'static str> {
    let (text, shown) = (text.to_ascii_lowercase(), shown.to_ascii_lowercase());
    PROFILES
        .iter()
        .filter(|profile| family(profile.driver) == family(driver))
        .find(|profile| {
            profile.markers.iter().any(|marker| {
                let marker = marker.to_ascii_lowercase();
                text.contains(&marker) && !shown.contains(&marker)
            })
        })
        .map(|profile| profile.name)
}

impl Profile {
    pub fn capabilities(&self, base: &Capabilities) -> Capabilities {
        let mut capabilities = base.clone();
        let mut schemes: Vec<String> = self.schemes.iter().map(|scheme| (*scheme).to_owned()).collect();
        schemes.extend(base.uri_schemes.iter().cloned());
        capabilities.uri_schemes = schemes;
        if !self.quirks.transactions {
            capabilities.transactions = false;
            capabilities.isolation_levels.clear();
        }
        if !self.quirks.all_databases {
            capabilities.catalogs = false;
        }
        capabilities
    }

    pub fn answer(&self, base: Option<&Capabilities>) -> ProfileAnswer {
        ProfileAnswer {
            id: self.id.to_owned(),
            name: self.name.to_owned(),
            driver: self.driver.to_owned(),
            family: family(self.driver).to_owned(),
            icon: self.icon.to_owned(),
            default_port: self.port,
            default_database: self.database.map(str::to_owned),
            default_user: self.user.map(str::to_owned),
            uri_schemes: self.schemes.iter().map(|scheme| (*scheme).to_owned()).collect(),
            hosts: self.hosts.iter().map(|host| (*host).to_owned()).collect(),
            tls: self.tls,
            settings: self.settings.iter().map(|setting| (*setting).to_owned()).collect(),
            catalogue: self.quirks.catalogue,
            hidden: self.quirks.hidden.iter().map(|folder| (*folder).to_owned()).collect(),
            docs: self.docs.to_owned(),
            capabilities: base.map(|base| self.capabilities(base)),
        }
    }
}

/// A profile as the connection dialog and agents see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAnswer {
    /// Its id (`cockroachdb`).
    pub id: String,
    /// Its name as shown.
    pub name: String,
    /// The driver that connects to it.
    pub driver: String,
    /// The family it is listed in.
    pub family: String,
    /// The icon drawn for it.
    pub icon: String,
    /// The server's default port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    /// The database a new connection names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_database: Option<String>,
    /// The user a new connection names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_user: Option<String>,
    /// URI schemes of its own, besides the driver's.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub uri_schemes: Vec<String>,
    /// Host names of its managed service, with `*` for any text.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub hosts: Vec<String>,
    /// The TLS mode of a new connection.
    pub tls: TlsMode,
    /// Driver settings a new connection turns on.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub settings: Vec<String>,
    /// How its structure is read.
    pub catalogue: Catalogue,
    /// Navigator folders it has not.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub hidden: Vec<String>,
    /// The vendor's documentation of connecting.
    pub docs: String,
    /// What its connections can do, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
}

#[cfg(test)]
mod tests;
