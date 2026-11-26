use super::*;

#[test]
fn every_profile_the_card_names_is_there_once_over_a_known_driver() {
    let wanted = [
        "cockroachdb",
        "yugabytedb",
        "timescaledb",
        "neon",
        "supabase",
        "redshift",
        "greenplum",
        "alloydb",
        "citus",
        "materialize",
        "risingwave",
        "questdb",
        "cratedb",
        "tidb",
        "singlestore",
        "planetscale",
        "vitess",
        "starrocks",
        "doris",
        "percona",
        "aurora-mysql",
        "valkey",
        "keydb",
        "dragonfly",
        "garnet",
    ];
    for id in wanted {
        assert!(get(id).is_some(), "{id}");
    }
    let mut ids: Vec<&str> = PROFILES.iter().map(|profile| profile.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), PROFILES.len(), "ids are unique");
    for profile in PROFILES {
        assert!(["postgresql", "mysql", "redis", "clickhouse"].contains(&profile.driver), "{}", profile.id);
        assert!(profile.docs.starts_with("https://"), "{}", profile.id);
        assert!(profile.port.is_some(), "{}", profile.id);
        for folder in profile.quirks.hidden {
            assert!(
                crate::schema::ObjectKind::ALL.iter().any(|kind| kind.folder() == *folder)
                    || ["indexes", "triggers", "policies", "partitions", "constraints", "foreignKeys"].contains(folder),
                "{} hides an unknown folder {folder}",
                profile.id
            );
        }
    }
}

#[test]
fn defaults_come_from_the_vendors() {
    let cockroach = get("cockroachdb").unwrap();
    assert_eq!((cockroach.port, cockroach.database, cockroach.user), (Some(26257), Some("defaultdb"), Some("root")));
    assert_eq!(get("yugabytedb").unwrap().port, Some(5433));
    assert_eq!((get("redshift").unwrap().port, get("redshift").unwrap().database), (Some(5439), Some("dev")));
    assert_eq!(get("materialize").unwrap().port, Some(6875));
    assert_eq!(get("risingwave").unwrap().port, Some(4566));
    assert_eq!(get("questdb").unwrap().port, Some(8812));
    assert_eq!(get("tidb").unwrap().port, Some(4000));
    assert_eq!(get("starrocks").unwrap().port, Some(9030));
    assert_eq!(get("neon").unwrap().tls, TlsMode::Require);
    assert_eq!(get("garnet").unwrap().port, Some(6379));
}

#[test]
fn a_url_picks_its_profile_by_scheme_or_by_the_services_host() {
    assert_eq!(for_url("cockroachdb", None, Some("localhost")).map(|found| found.id), Some("cockroachdb"));
    assert_eq!(for_url("redshift", None, None).map(|found| found.id), Some("redshift"));
    assert_eq!(for_scheme("valkeys").map(|(found, tls)| (found.id, tls)), Some(("valkey", true)));
    assert_eq!(for_scheme("valkey").map(|(found, tls)| (found.id, tls)), Some(("valkey", false)));
    assert_eq!(for_scheme("postgresql"), None);
    let neon = for_url("postgresql", Some("postgresql"), Some("ep-cool-darkness-123456.us-east-2.aws.neon.tech"));
    assert_eq!(neon.map(|found| found.id), Some("neon"));
    let supabase = for_url("postgres", Some("postgresql"), Some("aws-0-eu-central-1.pooler.supabase.com"));
    assert_eq!(supabase.map(|found| found.id), Some("supabase"));
    let redshift =
        for_url("postgresql", Some("postgresql"), Some("examplecluster.abc123xyz789.us-west-1.redshift.amazonaws.com"));
    assert_eq!(redshift.map(|found| found.id), Some("redshift"));
    let aurora = for_url("mysql", Some("mysql"), Some("shop.cluster-c9akciq32.us-east-1.rds.amazonaws.com"));
    assert_eq!(aurora.map(|found| found.id), Some("aurora-mysql"));
    assert_eq!(for_url("mysql", Some("mysql"), Some("shop.c9akciq32.us-east-1.rds.amazonaws.com")), None);
    assert_eq!(
        for_url("mysql", Some("mariadb"), Some("aws.connect.psdb.cloud")).map(|found| found.id),
        Some("planetscale")
    );
    assert_eq!(for_url("mysql", Some("mysql"), Some("db.neon.tech")), None, "a host of another family");
    assert_eq!(for_url("postgresql", Some("postgresql"), Some("db.example.com")), None);
}

#[test]
fn detection_reads_the_version_text_within_the_drivers_family() {
    assert_eq!(detect("postgresql", "CockroachDB CCL v24.1.0", None).map(|found| found.id), Some("cockroachdb"));
    assert_eq!(detect("postgresql", "PostgreSQL 18.0", None), None);
    assert_eq!(detect("mysql", "CockroachDB CCL v24.1.0", None), None, "only the driver's family");
    assert_eq!(detect("mariadb", "MySQL 8.0.11 (TiDB)", None).map(|found| found.id), Some("tidb"));
    let by_host = detect("postgresql", "PostgreSQL 17.2", Some("ep-x.eu-central-1.aws.neon.tech"));
    assert_eq!(by_host.map(|found| found.id), Some("neon"));
    assert_eq!(
        marked_in("postgresql", "PostgreSQL 8.0.2 on i686, Redshift 1.0", "PostgreSQL 8.0.2"),
        Some("Amazon Redshift")
    );
    assert_eq!(marked_in("postgresql", "CockroachDB CCL v24.1.0", "CockroachDB CCL v24.1.0"), None);
}

#[test]
fn a_profiles_capabilities_are_its_drivers_with_its_schemes_and_quirks() {
    let base = Capabilities {
        transactions: true,
        catalogs: true,
        isolation_levels: vec!["serializable".to_owned()],
        uri_schemes: vec!["postgresql".to_owned(), "postgres".to_owned()],
        ..Capabilities::default()
    };
    let cockroach = get("cockroachdb").unwrap().capabilities(&base);
    assert_eq!(cockroach.uri_schemes, ["cockroachdb", "postgresql", "postgres"]);
    assert!(cockroach.transactions && !cockroach.catalogs);
    let questdb = get("questdb").unwrap().capabilities(&base);
    assert!(!questdb.transactions && questdb.isolation_levels.is_empty());
    let timescale = get("timescaledb").unwrap().capabilities(&base);
    assert_eq!(timescale, base);
    let answer = serde_json::to_value(get("redshift").unwrap().answer(Some(&base))).unwrap();
    assert_eq!(answer["driver"], "postgresql");
    assert_eq!(answer["family"], "postgresql");
    assert_eq!(answer["defaultPort"], 5439);
    assert_eq!(answer["tls"], "require");
    assert_eq!(answer["catalogue"], "informationSchema");
    assert_eq!(answer["capabilities"]["uriSchemes"][0], "redshift");
    let bare = serde_json::to_value(get("valkey").unwrap().answer(None)).unwrap();
    assert!(bare.get("capabilities").is_none(), "{bare}");
}

#[test]
fn a_connection_takes_its_profiles_quirks() {
    let mut source = DataSource { driver: "postgresql".to_owned(), ..DataSource::default() };
    assert_eq!(quirks_of(&source), Quirks::NATIVE);
    source.profile = Some("redshift".to_owned());
    let quirks = quirks_of(&source);
    assert_eq!(quirks.catalogue, Catalogue::InformationSchema);
    assert!(!quirks.guards && !quirks.fingerprints);
    source.profile = Some("not-a-profile".to_owned());
    assert_eq!(quirks_of(&source), Quirks::NATIVE);
    assert_eq!(family("mariadb"), "mysql");
    assert_eq!(family("redis"), "redis");
}
