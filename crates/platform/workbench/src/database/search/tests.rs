use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use zen_db::driver::ContextKind;
use zen_db::history::{HistorySink, Provenance, Ran, Source};
use zen_db::model::NodeKind;
use zen_db::schema::{CachedChildren, CachedNode, Catalog, Column, Level, Object, ObjectKind, Schema, Snapshot};

use super::super::tests::Window;
use super::{CONTRIBUTOR, NameIndex, match_names};

fn column(name: &str, position: i32) -> Column {
    Column { name: name.into(), position, type_name: "bigint".into(), ..Column::default() }
}

fn table(kind: ObjectKind, name: &str, columns: &[&str]) -> Object {
    let mut object = Object::new(kind, name);
    object.detailed = true;
    object.columns = columns.iter().zip(1..).map(|(name, at)| column(name, at)).collect();
    object
}

fn shop(connection: &str) -> Snapshot {
    let mut hidden = table(ObjectKind::Table, "pg_orders_hidden", &["oid"]);
    hidden.system = true;
    let objects = vec![
        table(ObjectKind::Table, "orders", &["id", "customer_id", "total"]),
        table(ObjectKind::Table, "customers", &["id", "name"]),
        table(ObjectKind::Table, "order_items", &["order_id", "quantity"]),
        table(ObjectKind::View, "big_orders", &["id"]),
        table(ObjectKind::Function, "recalc_orders", &[]),
        hidden,
    ];
    let catalog_schema = Schema { name: "pg_catalog".into(), system: true, ..Schema::default() };
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![
                Schema { name: "public".into(), level: Level::Details, objects, ..Schema::default() },
                catalog_schema,
            ],
            ..Catalog::default()
        }],
        ..Snapshot::new(connection, "postgresql")
    }
}

fn index_of(snapshot: &Snapshot) -> NameIndex {
    NameIndex::build([("c1", "Shop", snapshot)])
}

fn names(index: &NameIndex, query: &str) -> Vec<(String, String)> {
    let (hits, _) = match_names(index, query, 50, &|_, _| true, &|| false).unwrap();
    hits.iter()
        .map(|hit| {
            let name = &index.names[hit.name as usize];
            (name.text.to_string(), name.kind.to_string())
        })
        .collect()
}

#[test]
fn names_of_a_snapshot_match_best_first_without_system_objects() {
    let snapshot = shop("c1");
    let index = index_of(&snapshot);
    assert_eq!(index.len(), 1 + 5 + 3 + 2 + 2 + 1, "a schema, five objects and their columns; system ones left out");
    let found = names(&index, "orders");
    assert_eq!(found[0], ("orders".into(), "table".into()), "the exact name first: {found:?}");
    assert!(found.contains(&("big_orders".into(), "view".into())));
    assert!(found.contains(&("recalc_orders".into(), "function".into())));
    assert!(!found.iter().any(|(name, _)| name.starts_with("pg_")), "{found:?}");
    let humps = names(&index, "cuid");
    assert_eq!(humps, [("customer_id".to_owned(), "column".to_owned())], "camel humps over words");
    let ids = names(&index, "id");
    assert!(ids.iter().take_while(|(name, _)| name == "id").count() >= 3, "every table's `id`: {ids:?}");
}

#[test]
fn a_qualifier_before_the_dot_narrows_by_table_or_schema() {
    let snapshot = shop("c1");
    let index = index_of(&snapshot);
    let (hits, _) = match_names(&index, "orders.id", 50, &|_, _| true, &|| false).unwrap();
    let details: Vec<String> =
        hits.iter().map(|hit| index.owners[index.names[hit.name as usize].owner as usize].detail.clone()).collect();
    assert_eq!(details[0], "public.orders", "the table named so first: {details:?}");
    assert!(!details.contains(&"public.customers".to_owned()), "{details:?}");
    let (hits, _) = match_names(&index, "orders.", 50, &|_, _| true, &|| false).unwrap();
    let details: Vec<&str> =
        hits.iter().map(|hit| index.owners[index.names[hit.name as usize].owner as usize].detail.as_str()).collect();
    assert_eq!(
        details,
        ["public.orders", "public.orders", "public.orders", "public.big_orders"],
        "orders' columns first"
    );
    let public = names(&index, "public.cust");
    assert_eq!(public, [("customers".to_owned(), "table".to_owned())]);
    assert!(names(&index, "nowhere.orders").is_empty());
}

#[test]
fn a_search_stops_when_cancelled_and_filters_by_connection_and_kind() {
    let snapshot = shop("c1");
    let index = index_of(&snapshot);
    assert!(match_names(&index, "orders", 50, &|_, _| true, &|| true).is_none());
    let (views, total) = match_names(&index, "orders", 50, &|_, kind| kind == "view", &|| false).unwrap();
    assert_eq!((views.len(), total), (1, 1));
    let (none, _) = match_names(&index, "orders", 50, &|connection, _| connection == "other", &|| false).unwrap();
    assert!(none.is_empty());
    let (two, total) = match_names(&index, "o", 2, &|_, _| true, &|| false).unwrap();
    assert_eq!(two.len(), 2);
    assert!(total > 2, "the total counts every match: {total}");
}

#[test]
fn a_drivers_own_tree_is_searched_by_its_labels() {
    let node = |path: &[&str], kind: NodeKind, label: &str| CachedNode {
        path: path.iter().map(|part| (*part).to_owned()).collect(),
        kind,
        label: label.into(),
        has_children: false,
        detail: None,
        system: false,
        size: None,
        rows: None,
        comment: None,
    };
    let snapshot = Snapshot {
        nodes: vec![CachedChildren {
            path: vec!["0".into()],
            nodes: vec![
                node(&["0", "session:1"], NodeKind::Key, "session:1"),
                node(&["0", "session:"], NodeKind::KeyPrefix, "session:"),
            ],
        }],
        ..Snapshot::new("r", "redis")
    };
    let index = NameIndex::build([("r", "Cache", &snapshot)]);
    assert_eq!(names(&index, "session"), [("session:1".to_owned(), "key".to_owned())], "prefixes are not objects");
}

fn wide(connection: &str, tables: usize, columns: usize) -> Snapshot {
    let names: Vec<String> = (0..columns).map(|at| format!("column_{at}_value")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let objects = (0..tables).map(|at| table(ObjectKind::Table, &format!("table_{at}_records"), &names)).collect();
    let mut snapshot = shop(connection);
    snapshot.catalogs[0].schemas[0].objects = objects;
    snapshot
}

#[test]
fn a_hundred_thousand_names_are_matched_in_milliseconds() {
    let snapshot = wide("c1", 1000, 99);
    let index = index_of(&snapshot);
    assert_eq!(index.len(), 1 + 1000 * 100, "a schema, 1000 tables and 99 000 columns");
    let mut slowest = Duration::ZERO;
    for (query, expected) in [("table_999_records", 1), ("t9r", 111), ("column_98_value", 1000), ("zzz", 0)] {
        let started = Instant::now();
        let (hits, total) = match_names(&index, query, 50, &|_, _| true, &|| false).unwrap();
        let took = started.elapsed();
        slowest = slowest.max(took);
        assert!(total >= expected, "{query}: {total} matches");
        assert!(hits.len() <= 50);
        #[allow(clippy::print_stderr, reason = "the measured time is reported for the card")]
        {
            eprintln!("{query}: {total} of {} names in {took:?}", index.len());
        }
    }
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(slowest < Duration::from_millis(10), "100 k names took {slowest:?}");
    }
}

fn add_shop(window: &mut Window) -> String {
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> =
        vec![std::sync::Arc::new(zen_db::postgres::Postgres)];
    window.session.state_mut::<super::super::Database>().set_drivers(drivers);
    let added = window.run("db.dataSource.add", json!({ "name": "Shop", "url": "postgresql://ann@db.local/shop" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &shop(&id)).unwrap();
    id
}

fn record(window: &Window, connection: &str, text: &str) {
    let journal = super::super::history::journal(&window.session).unwrap();
    journal.record(Ran {
        connection: connection.to_owned(),
        context: ContextKind::Main,
        schema: Some("public".into()),
        text: text.to_owned(),
        started: SystemTime::now(),
        duration: Duration::from_millis(3),
        rows: Some(1),
        affected: None,
        error: None,
        provenance: Provenance::of(Source::Statement),
        params: Vec::new(),
    });
    assert!(journal.flush(Duration::from_secs(10)));
}

fn loaded(window: &mut Window, params: Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = window.run("db.object.goTo", params.clone());
        if found["loading"] == false || Instant::now() > deadline {
            return found;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn go_to_object_finds_objects_scripts_and_history_and_opens_them_without_connecting() {
    let mut window = Window::new();
    let id = add_shop(&mut window);
    record(&window, &id, "SELECT count(*)\nFROM orders WHERE total > 10");
    let project = window.dir.path().join("project");
    let script = window.run("db.script.new", json!({ "connection": id, "name": "orders_report" }));
    let script_path = PathBuf::from(script["path"].as_str().unwrap());
    window.run("workspace.host.collapse", json!({}));

    let found = loaded(&mut window, json!({ "query": "orders" }));
    let items = found["items"].as_array().unwrap();
    let kinds: Vec<(&str, &str)> =
        items.iter().map(|item| (item["name"].as_str().unwrap(), item["kind"].as_str().unwrap())).collect();
    assert_eq!(kinds[0], ("orders", "table"), "{kinds:?}");
    assert_eq!(items[0]["detail"], "public");
    assert_eq!(items[0]["connectionName"], "Shop");
    assert_eq!(items[0]["path"], json!(["shop", "public", "tables", "orders"]));
    assert_eq!(items[0]["ranges"], json!([[0, 6]]));
    assert!(kinds.contains(&("orders_report.sql", "script")), "{kinds:?}");
    let history = items.iter().find(|item| item["kind"] == "history").expect("the statement of the history");
    assert_eq!(history["name"], "SELECT count(*) …");
    assert_eq!(history["command"], "db.history.openInConsole");
    let position = |kind: &str| items.iter().position(|item| item["kind"] == kind).unwrap();
    assert!(position("view") < position("script") && position("script") < position("history"), "{kinds:?}");
    assert_eq!(found["total"], items.len());
    assert!(project.join("orders_report.sql") == script_path);

    let limited = window.run("db.object.goTo", json!({ "query": "orders", "limit": 2 }));
    assert_eq!((limited["items"].as_array().unwrap().len(), limited["truncated"].as_bool()), (2, Some(true)));
    let columns = window.run("db.object.goTo", json!({ "query": "total", "kinds": ["column"] }));
    let column = &columns["items"][0];
    assert_eq!((column["name"].as_str(), column["detail"].as_str()), (Some("total"), Some("public.orders")));
    assert_eq!(column["params"]["page"], "properties");

    let opened = window.run(items[0]["command"].as_str().unwrap(), items[0]["params"].clone());
    assert_eq!(opened["tab"]["id"], format!("object:{id}:shop/public/tables/orders"));
    let state = window.run("workspace.host.state", json!({ "id": super::super::WORKSPACE }));
    assert_eq!(state["expanded"], true, "opening an object expands the workspace");
    let status = window.run("db.connection.status", json!({ "connection": id }));
    assert_eq!(status["connections"][0]["state"], "disconnected", "nothing connects: {status}");
    let opened = window.run(column["command"].as_str().unwrap(), column["params"].clone());
    assert_eq!(opened["page"], "properties");
}

#[test]
fn the_database_tab_of_search_everywhere_lists_objects_with_kind_schema_and_connection() {
    let mut window = Window::new();
    let id = add_shop(&mut window);
    loaded(&mut window, json!({ "query": "x" }));
    let listed = window.run("search.everywhere.contributors", json!({}));
    let tab = listed.as_array().unwrap().iter().find(|known| known["id"] == CONTRIBUTOR).cloned().unwrap();
    assert_eq!((tab["title"].as_str(), tab["hasTab"].as_bool()), (Some("Database"), Some(true)));
    let answer =
        window.run("search.everywhere.query", json!({ "tab": CONTRIBUTOR, "query": "custom", "waitMs": 5000 }));
    let item = &answer["groups"][0]["items"][0];
    assert_eq!(item["name"], "customers", "{answer}");
    assert_eq!(item["detail"], "public · Shop");
    assert_eq!(item["hint"], "Table");
    assert_eq!(item["target"]["kind"], "run");
    assert_eq!(item["target"]["command"], "db.object.open");
    assert_eq!(item["target"]["icon"], "table");
    assert_eq!(
        item["target"]["params"],
        json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] })
    );
    let all = window.run("search.everywhere.query", json!({ "query": "big_orders", "waitMs": 5000 }));
    let groups: Vec<&str> =
        all["groups"].as_array().unwrap().iter().filter_map(|group| group["contributor"].as_str()).collect();
    assert!(groups.contains(&CONTRIBUTOR), "a group in All: {all}");
}
