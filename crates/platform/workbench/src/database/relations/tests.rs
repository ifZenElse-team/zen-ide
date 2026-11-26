use serde_json::json;
use zen_db::schema::{Constraint, ConstraintKind, Object, ObjectKind, Reference};

use super::super::tests::{Window, open_customers, sql_connection};

fn foreign(name: &str, table: &str) -> Constraint {
    Constraint {
        name: name.into(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["id".into()],
        definition: String::new(),
        references: Some(Reference {
            schema: "public".into(),
            table: table.into(),
            columns: vec!["id".into()],
            on_update: String::new(),
            on_delete: String::new(),
        }),
        index: None,
        comment: None,
    }
}

fn with_accounts(window: &Window, id: &str) {
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    let mut snapshot = zen_db::schema::read_snapshot(&file).unwrap();
    let schema = &mut snapshot.catalogs[0].schemas[0];
    let mut accounts: Object = schema.objects[0].clone();
    accounts.kind = ObjectKind::Table;
    accounts.name = "accounts".into();
    accounts.oid = zen_db::memory::TABLE + 1;
    accounts.constraints[0].name = "accounts_pkey".into();
    accounts.constraints.push(foreign("accounts_customer_fkey", "customers"));
    schema.objects[0].constraints.push(foreign("customers_account_fkey", "accounts"));
    schema.objects.push(accounts);
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
}

fn until<T>(window: &mut Window, what: &str, mut done: impl FnMut(&mut Window) -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(found) = done(window) {
            return found;
        }
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn shown(window: &mut Window, tab: &str) -> u64 {
    until(window, "the rows were read", |window| {
        let state = window.run("db.object.state", json!({ "tab": tab }));
        assert!(state["data"]["error"].is_null(), "{state}");
        let done = state["data"]["result"].is_u64() && state["data"]["reading"] == false;
        done.then(|| state["data"]["result"].as_u64().unwrap())
    })
}

fn active(window: &mut Window) -> String {
    let state = window.run("workspace.host.state", json!({ "id": "database" }));
    state["active"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn a_key_opens_the_referenced_row_its_references_and_back_and_forward_walk_them() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    with_accounts(&window, &id);
    let (customers, first) = open_customers(&mut window, &id);
    let relations = until(&mut window, "the structure was read", |window| {
        let answer = window.run("db.result.relations", json!({ "id": first }));
        (answer["reading"] == false).then_some(answer)
    });
    assert_eq!(relations["table"], "public.customers");
    assert_eq!(relations["links"][0]["name"], "customers_account_fkey");
    assert_eq!(relations["links"][0]["columns"], json!([0]));
    assert_eq!(relations["links"][0]["table"], "public.accounts");
    assert_eq!(relations["links"][0]["path"], json!(["shop", "public", "tables", "accounts"]));
    assert_eq!(relations["references"][0]["name"], "accounts_customer_fkey");
    assert_eq!(relations["references"][0]["table"], "public.accounts");
    assert!(!relations.to_string().contains("Customer"), "never data values: {relations}");

    let followed = window.run("db.result.openReferenced", json!({ "id": first, "row": 2, "column": 0 }));
    assert_eq!(followed["condition"], "\"id\" = 3");
    assert_eq!(followed["canGoBack"], true);
    let accounts = followed["tab"].as_str().unwrap().to_owned();
    assert_ne!(accounts, customers);
    assert_eq!(active(&mut window), accounts);
    shown(&mut window, &accounts);
    let executed = driver.server.executed();
    assert!(
        executed.iter().any(|sql| sql == "SELECT * FROM \"public\".\"accounts\" WHERE (\"id\" = 3)"),
        "the referenced table is read filtered by the key: {executed:?}"
    );
    let filter = window.run("db.result.filterState", json!({}));
    assert_eq!(filter["condition"], "\"id\" = 3");
    assert_eq!(filter["history"], json!(["\"id\" = 3"]));

    let back = window.run("db.data.back", json!({}));
    assert_eq!((back["tab"].as_str(), back["canGoForward"].as_bool()), (Some(customers.as_str()), Some(true)));
    assert_eq!(active(&mut window), customers);
    let forward = window.run("db.data.forward", json!({}));
    assert_eq!(forward["tab"].as_str(), Some(accounts.as_str()));
    let navigation = window.run("db.data.navigation", json!({}));
    assert_eq!(navigation["back"], json!(["customers"]));

    window.run("workspace.tab.select", json!({ "workspace": "database", "tab": customers }));
    let referencing =
        window.run("db.result.openReferencing", json!({ "id": first, "row": 0, "table": "public.accounts" }));
    assert_eq!(referencing["condition"], "\"id\" = 1");
    let read = shown(&mut window, &accounts);
    assert!(
        driver.server.executed().iter().any(|sql| sql == "SELECT * FROM \"public\".\"accounts\" WHERE (\"id\" = 1)")
    );
    let filter = window.run("db.result.filterState", json!({ "id": read }));
    assert_eq!(filter["canGoBack"], true, "the filter it had stays for the filter's own Back: {filter}");

    let back = window.run("db.data.back", json!({}));
    assert_eq!(back["tab"].as_str(), Some(customers.as_str()));
    let forward = window.run("db.data.forward", json!({}));
    assert_eq!(forward["tab"].as_str(), Some(accounts.as_str()));
    assert!(forward["query"].is_null(), "the filter did not change: no new read");

    let unknown = window.refused("db.result.openReferenced", json!({ "id": first, "row": 0, "column": 1 }));
    assert!(unknown.contains("no such foreign key"), "{unknown}");
    let missing = window.refused("db.result.openReferenced", json!({ "id": first, "row": 99, "column": 0 }));
    assert!(missing.contains("no row 99"), "{missing}");
}

#[test]
fn a_key_is_chosen_from_the_referenced_table_into_the_staged_edit() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    with_accounts(&window, &id);
    let (_, result) = open_customers(&mut window, &id);
    until(&mut window, "the structure was read", |window| {
        let answer = window.run("db.result.relations", json!({ "id": result }));
        (answer["reading"] == false).then_some(())
    });
    let started =
        window.run("db.lookup.start", json!({ "id": result, "row": 1, "column": 0, "search": "1", "limit": 10 }));
    assert_eq!((started["open"].as_bool(), started["table"].as_str()), (Some(true), Some("public.accounts")));
    assert_eq!(started["key"], json!(["id"]));
    let state = until(&mut window, "the lookup's rows were read", |window| {
        let state = window.run("db.lookup.state", json!({}));
        (state["status"] == "done").then_some(state)
    });
    assert_eq!(state["rows"].as_array().unwrap().len(), 10);
    assert_eq!(state["truncated"], true);
    assert_eq!(state["headers"][0], "id");
    let keys: Vec<&str> = state["rows"].as_array().unwrap().iter().filter_map(|row| row[0].as_str()).collect();
    assert_eq!(keys, ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"], "in the key's order: {state}");
    let sql = "SELECT * FROM \"public\".\"accounts\" WHERE (\"id\"::text LIKE '%1%' ESCAPE '!' OR \"name\"::text LIKE '%1%' ESCAPE '!') \
ORDER BY \"id\" LIMIT 11";
    let executed = driver.server.executed();
    assert!(executed.iter().any(|known| known == sql), "{executed:?}");

    let edit = window.run("db.lookup.choose", json!({ "row": 4 }));
    assert_eq!(edit["pending"]["cells"], 1, "{edit}");
    let page = window.run("db.result.page", json!({ "id": result, "offset": 1, "limit": 1 }));
    assert_eq!(page["page"][0][0]["value"], json!(5), "{page}");
    assert_eq!(window.run("db.lookup.state", json!({}))["open"], false);
    let closed = window.run("db.lookup.close", json!({}));
    assert_eq!(closed["closed"], false);
    let refused = window.refused("db.lookup.choose", json!({ "row": 0 }));
    assert!(refused.contains("No key lookup"), "{refused}");
    let not_a_key = window.refused("db.lookup.start", json!({ "id": result, "row": 0, "column": 1 }));
    assert!(not_a_key.contains("no such foreign key"), "{not_a_key}");
}

#[test]
fn a_result_without_one_known_table_has_nothing_to_follow() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let query = window.run("db.query.run", json!({ "connection": id, "sql": "ROWS 3", "waitMs": 10_000 }));
    let result = query["statements"][0]["result"].as_u64().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let answer = loop {
        let answer = window.run("db.result.relations", json!({ "id": result }));
        if answer["reading"] == false || std::time::Instant::now() > deadline {
            break answer;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!((answer["links"].clone(), answer["reading"].clone()), (json!([]), json!(false)), "{answer}");
    let refused = window.refused("db.result.openReferenced", json!({ "id": result, "row": 0 }));
    assert!(refused.contains("one known table"), "{refused}");
    assert_eq!(window.run("db.data.navigation", json!({}))["canGoBack"], false);
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_composite_key_is_followed_both_ways_and_chosen_on_postgresql() {
    let port: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(54318);
    let mut window = Window::new();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "PG", "url": format!("postgresql://postgres:zen@127.0.0.1:{port}/postgres?sslmode=disable") }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run_confirmed(
        "db.query.run",
        json!({
            "connection": id,
            "waitMs": 20_000,
            "sql": "DROP SCHEMA IF EXISTS fk_ws CASCADE;\nCREATE SCHEMA fk_ws;\n\
        CREATE TABLE fk_ws.\"Regions\" (country text, \"Code\" int, name text, PRIMARY KEY (country, \"Code\"));\n\
        CREATE TABLE fk_ws.shops (id int PRIMARY KEY, country text, region int, \
        FOREIGN KEY (country, region) REFERENCES fk_ws.\"Regions\" (country, \"Code\"));\n\
        INSERT INTO fk_ws.\"Regions\" VALUES ('it''s', 1, 'North'), ('it''s', 2, 'South'), ('pt', 1, 'Lisbon');\n\
        INSERT INTO fk_ws.shops VALUES (1, 'it''s', 2), (2, 'pt', 1), (3, NULL, NULL);",
        }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    window.run("db.schema.refresh", json!({ "connection": id, "path": ["postgres", "fk_ws"], "force": true }));
    let shops = json!(["postgres", "fk_ws", "tables", "shops"]);
    until(&mut window, "the schema was read", |window| {
        super::super::connections::step(&mut window.session);
        let children =
            window.run("db.schema.children", json!({ "connection": id, "path": ["postgres", "fk_ws", "tables"] }));
        children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "shops").then_some(())
    });
    let opened = window.run("db.object.open", json!({ "connection": id, "path": shops }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let result = shown(&mut window, &tab);
    let relations = until(&mut window, "the keys", |window| {
        super::super::connections::step(&mut window.session);
        let answer = window.run("db.result.relations", json!({ "id": result }));
        (answer["reading"] == false && answer["links"].as_array().is_some_and(|links| !links.is_empty()))
            .then_some(answer)
    });
    assert_eq!(relations["links"][0]["columns"], json!([1, 2]), "{relations}");
    assert_eq!(relations["links"][0]["table"], "fk_ws.\"Regions\"");

    let followed = window.run("db.result.openReferenced", json!({ "id": result, "row": 0, "column": 2 }));
    assert_eq!(followed["condition"], "country = 'it''s' AND \"Code\" = 2");
    let regions = followed["tab"].as_str().unwrap().to_owned();
    let found = shown(&mut window, &regions);
    let page = window.run("db.result.page", json!({ "id": found }));
    assert_eq!(page["rows"], 1, "{page}");
    assert_eq!(page["page"][0][2]["value"], "South");
    let null = window.refused("db.result.openReferenced", json!({ "id": result, "row": 2, "column": 1 }));
    assert!(null.contains("NULL"), "{null}");

    let refs = until(&mut window, "the references", |window| {
        let answer = window.run("db.result.relations", json!({ "id": found }));
        (answer["references"].as_array().is_some_and(|list| !list.is_empty())).then_some(answer)
    });
    assert_eq!(refs["references"][0]["table"], "fk_ws.shops");
    let back = window.run("db.result.openReferencing", json!({ "id": found, "row": 0, "table": "fk_ws.shops" }));
    assert_eq!(back["condition"], "country = 'it''s' AND region = 2");
    let again = shown(&mut window, &tab);
    assert_eq!(window.run("db.result.page", json!({ "id": again }))["rows"], 1);

    window.run("db.result.resetFilter", json!({ "id": again }));
    let all = until(&mut window, "all rows", |window| {
        let state = window.run("db.object.state", json!({ "tab": tab }));
        let id = state["data"]["result"].as_u64().filter(|_| state["data"]["reading"] == false)?;
        (window.run("db.result.page", json!({ "id": id }))["rows"] == 3).then_some(id)
    });
    window.run("db.lookup.start", json!({ "id": all, "row": 1, "column": 1, "search": "outh" }));
    let state = until(&mut window, "the lookup", |window| {
        let state = window.run("db.lookup.state", json!({}));
        (state["status"] == "done").then_some(state)
    });
    assert_eq!(state["rows"].as_array().unwrap().len(), 1, "{state}");
    assert_eq!(state["headers"], json!(["country", "Code", "name"]));
    let edit = window.run("db.lookup.choose", json!({ "row": 0 }));
    assert_eq!(edit["pending"]["cells"], 2, "both columns of the key: {edit}");
    let preview = window.run("db.result.previewChanges", json!({ "id": all }));
    assert!(preview["sql"].as_str().unwrap().contains("region = 2"), "{preview}");
    window.run("db.result.revert", json!({ "id": all }));

    window.run_confirmed(
        "db.query.run",
        json!({ "connection": id, "sql": "DROP SCHEMA fk_ws CASCADE", "waitMs": 10_000 }),
    );
    window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
}
