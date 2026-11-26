use super::*;
use crate::schema::{Catalog, Column, Constraint, Level, Reference};
use crate::value::ColumnSource;

fn column(name: &str, position: i32) -> Column {
    Column { name: name.into(), position, type_name: "integer".into(), ..Column::default() }
}

fn constraint(name: &str, kind: ConstraintKind, columns: &[&str], references: Option<Reference>) -> Constraint {
    Constraint {
        name: name.into(),
        kind,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: String::new(),
        references,
        index: None,
        comment: None,
    }
}

fn reference(schema: &str, table: &str, columns: &[&str]) -> Option<Reference> {
    Some(Reference {
        schema: schema.into(),
        table: table.into(),
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        on_update: String::new(),
        on_delete: String::new(),
    })
}

fn snapshot() -> Snapshot {
    let mut regions = Object::new(ObjectKind::Table, "regions");
    regions.columns = vec![column("country", 1), column("code", 2), column("name", 3)];
    regions.constraints = vec![constraint("regions_pkey", ConstraintKind::PrimaryKey, &["country", "code"], None)];
    let mut customers = Object::new(ObjectKind::Table, "customers");
    customers.oid = 7;
    customers.columns = vec![column("id", 1), column("country", 2), column("region", 3), column("referrer", 4)];
    customers.constraints = vec![
        constraint("customers_pkey", ConstraintKind::PrimaryKey, &["id"], None),
        constraint(
            "customers_region_fkey",
            ConstraintKind::ForeignKey,
            &["country", "region"],
            reference("geo", "regions", &["country", "code"]),
        ),
        constraint(
            "customers_referrer_fkey",
            ConstraintKind::ForeignKey,
            &["referrer"],
            reference("shop", "customers", &[]),
        ),
    ];
    let mut orders = Object::new(ObjectKind::Table, "orders");
    orders.columns = vec![column("id", 1), column("customer_id", 2)];
    orders.constraints = vec![constraint(
        "orders_customer_fkey",
        ConstraintKind::ForeignKey,
        &["customer_id"],
        reference("shop", "customers", &["id"]),
    )];
    let schema = |name: &str, objects: Vec<Object>| Schema {
        name: name.into(),
        level: Level::Details,
        objects,
        ..Schema::default()
    };
    Snapshot {
        catalogs: vec![Catalog {
            name: "db".into(),
            current: true,
            schemas_read: true,
            schemas: vec![schema("geo", vec![regions]), schema("shop", vec![customers, orders])],
            ..Catalog::default()
        }],
        ..Snapshot::new("c", "postgresql")
    }
}

#[test]
fn a_tables_keys_point_at_their_targets_and_an_unnamed_target_is_its_primary_key() {
    let snapshot = snapshot();
    let (_, _, customers) = find_table(&snapshot, "db", "shop", "customers").unwrap();
    let links = outgoing(&snapshot, "db", "shop", customers);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].columns, ["country", "region"]);
    assert_eq!((links[0].target_schema.as_str(), links[0].target_table.as_str()), ("geo", "regions"));
    assert_eq!(links[0].target_columns, ["country", "code"]);
    assert_eq!(links[1].target_columns, ["id"], "a SQLite key without columns points at the primary key");
    let referencing = incoming(&snapshot, "db", "shop", "customers");
    let names: Vec<&str> = referencing.iter().map(|link| link.name.as_str()).collect();
    assert_eq!(names, ["customers_referrer_fkey", "orders_customer_fkey"]);
    assert!(incoming(&snapshot, "elsewhere", "shop", "customers").is_empty());
    assert!(find_table(&snapshot, "db", "shop", "missing").is_none());
}

#[test]
fn result_columns_map_to_table_columns_by_source_or_by_name() {
    let snapshot = snapshot();
    let (_, _, customers) = find_table(&snapshot, "db", "shop", "customers").unwrap();
    let mut sourced = ResultColumn::new("renamed", "int4");
    sourced.source = Some(ColumnSource { table: 7, position: 3 });
    let mut origin = ResultColumn::new("alias", "int");
    origin.source = Some(ColumnSource { table: 7, position: 0 });
    origin.origin = Some("referrer".into());
    let mut other = ResultColumn::new("id", "int4");
    other.source = Some(ColumnSource { table: 8, position: 1 });
    let columns = [sourced, origin, other, ResultColumn::new("country", "int4")];
    assert_eq!(
        table_columns(&columns, customers, 7, false),
        [Some("region".to_owned()), Some("referrer".to_owned()), None, None]
    );
    assert_eq!(table_columns(&columns, customers, 7, true)[2..], [Some("id".to_owned()), Some("country".to_owned())]);
}

#[test]
fn key_conditions_are_written_in_each_dialect_and_a_null_points_nowhere() {
    let country = Value::Text("it's".into());
    let code = Value::Integer(7);
    let pairs = [("country", &country), ("Code", &code)];
    assert_eq!(key_condition(&pairs, SqlDialect::Postgres).as_deref(), Some("\"country\" = 'it''s' AND \"Code\" = 7"));
    assert_eq!(key_condition(&pairs, SqlDialect::MySql).as_deref(), Some("`country` = 'it''s' AND `Code` = 7"));
    let slash = Value::Text("a\\b".into());
    assert_eq!(
        key_condition(&[("order", &slash)], SqlDialect::MariaDb).as_deref(),
        Some("`order` = _utf8mb4 X'615c62'")
    );
    assert_eq!(key_condition(&[("order", &slash)], SqlDialect::ClickHouse).as_deref(), Some("\"order\" = 'a\\\\b'"));
    assert_eq!(key_condition(&[("country", &country), ("code", &Value::Null)], SqlDialect::Sqlite), None);
    assert_eq!(key_condition(&[], SqlDialect::Sqlite), None);
}

#[test]
fn the_lookup_reads_a_limited_page_searched_in_the_dialect() {
    let columns = vec!["code".to_owned(), "name".to_owned()];
    assert_eq!(search_condition(&columns, "  ", SqlDialect::Postgres), None);
    let condition = search_condition(&columns, "5%", SqlDialect::Postgres).unwrap();
    assert_eq!(condition, "\"code\"::text LIKE '%5!%%' ESCAPE '!' OR \"name\"::text LIKE '%5!%%' ESCAPE '!'");
    assert_eq!(
        lookup_statement(SqlDialect::Postgres, "geo.regions", Some(&condition), Some("country, code"), 201),
        format!("SELECT * FROM geo.regions WHERE ({condition}) ORDER BY country, code LIMIT 201")
    );
    let sqlite = search_condition(&columns[..1], "x", SqlDialect::Sqlite).unwrap();
    assert_eq!(sqlite, "CAST(\"code\" AS TEXT) LIKE '%x%' ESCAPE '!'");
    assert_eq!(lookup_statement(SqlDialect::MySql, "`t`", None, None, 10), "SELECT * FROM `t` LIMIT 10");
    assert_eq!(
        lookup_statement(SqlDialect::Oracle, "HR.T", None, Some("ID"), 10),
        "SELECT * FROM HR.T ORDER BY ID FETCH FIRST 10 ROWS ONLY"
    );
}
