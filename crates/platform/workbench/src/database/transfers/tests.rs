use serde_json::{Value, json};

use super::super::tests::{Window, sql_connection};

fn refusal(window: &mut Window, command: &str, params: Value) -> (Option<String>, Value) {
    match window.registry.dispatch(&mut window.session, command, params).unwrap_err() {
        zen_core::DispatchError::Failed { error, .. } => {
            (error.code, error.data.map(Value::Object).unwrap_or(Value::Null))
        }
        other => panic!("{other:?}"),
    }
}

fn notes(window: &mut Window) -> Vec<String> {
    super::super::connections::step(&mut window.session);
    let notes = window.session.state::<crate::notifications::Notifications>().unwrap();
    notes.newest_first().map(|note| note.body.clone()).collect()
}

#[test]
fn tables_are_planned_confirmed_on_production_and_copied_with_a_new_table() {
    let mut window = Window::new();
    let (source, driver) = sql_connection(&mut window, "dev", true);
    let (target, _) = sql_connection(&mut window, "prod", false);
    let table = json!(["shop", "public", "tables", "customers"]);

    let preview = window.run("db.transfer.preview", json!({ "source": source, "target": target, "tables": [table] }));
    assert_eq!(preview["schema"], json!(["shop", "public"]), "{preview}");
    let planned = &preview["tables"][0];
    assert_eq!((planned["exists"].as_bool(), planned["create"].as_bool()), (Some(true), Some(false)), "{preview}");
    assert_eq!(planned["columns"].as_array().map(Vec::len), Some(6));
    assert_eq!(preview["load"]["batch"], 1000);

    let params = json!({
        "source": source,
        "target": target,
        "schema": ["shop", "public"],
        "tables": [{
            "source": table,
            "name": "copy",
            "columns": [{ "source": "id" }, { "source": "name", "target": "label" }, { "source": "created" }],
        }],
        "load": { "batch": 10 },
    });
    let (code, data) = refusal(&mut window, "db.transfer.start", params.clone());
    assert_eq!(code.as_deref(), Some("confirmExecute"), "{data}");
    assert_eq!(data["retry"]["command"], "db.transfer.start");
    assert!(!driver.server.executed().iter().any(|sql| sql.starts_with("CREATE")), "nothing ran before the answer");
    let started = window.run_confirmed("db.transfer.start", params);
    assert_eq!(
        started["tables"][0]["statements"],
        json!([
            "CREATE TABLE \"public\".\"copy\" (\n    \"id\" bigint NOT NULL,\n    \"label\" text,\n    \"created\" timestamp with time zone,\n    PRIMARY KEY (\"id\")\n)"
        ]),
        "{started}"
    );
    let state = window.run("db.transfer.state", json!({ "id": started["id"], "waitMs": 30_000 }));
    let state = &state["transfers"][0];
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!((state["rowsRead"].as_u64(), state["rowsInserted"].as_u64()), (Some(25), Some(25)), "{state}");
    assert!(!state.to_string().contains("Customer 1"), "answers never hold rows: {state}");
    let inserts: Vec<String> =
        driver.server.executed().into_iter().filter(|sql| sql.starts_with("INSERT INTO \"public\".\"copy\"")).collect();
    assert_eq!(inserts.len(), 3, "batches of 10");
    assert!(
        inserts[0]
            .starts_with("INSERT INTO \"public\".\"copy\" (\"id\", \"label\", \"created\") VALUES\n(1, 'Customer 1', "),
        "{}",
        inserts[0]
    );
    let bodies = notes(&mut window);
    assert!(bodies.iter().any(|body| body == "25 rows were copied into one table."), "{bodies:?}");
    assert!(window.dir.path().join("data/databases/transfer.json").exists());
    assert_eq!(window.run("db.transfer.cancel", json!({ "id": started["id"] }))["cancelled"], false);

    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": target, "schema": ["shop", "public"], "tables": [{ "source": table, "create": true }] }),
    );
    assert_eq!(code.as_deref(), Some("exists"));
    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": target, "schema": ["shop"], "tables": [{ "source": table }] }),
    );
    assert_eq!(code.as_deref(), Some("badSchema"));
    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": target, "schema": ["shop", "public"], "tables": [{ "source": table, "columns": [{ "source": "nope" }] }] }),
    );
    assert_eq!(code.as_deref(), Some("badMapping"));
    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": target, "schema": ["shop", "public"], "tables": [] }),
    );
    assert_eq!(code.as_deref(), Some("noTables"));
}

#[test]
fn a_table_is_never_copied_into_itself_and_a_type_is_one_type() {
    let mut window = Window::new();
    let (source, driver) = sql_connection(&mut window, "dev", true);
    let table = json!(["shop", "public", "tables", "customers"]);
    let (code, _) =
        refusal(&mut window, "db.transfer.preview", json!({ "source": source, "target": source, "tables": [table] }));
    assert_eq!(code.as_deref(), Some("sameTable"), "the same connection maps the table onto itself");
    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": source, "schema": ["shop", "public"], "tables": [{ "source": table, "name": "CUSTOMERS" }], "load": { "truncate": true } }),
    );
    assert_eq!(code.as_deref(), Some("sameTable"), "whatever the case of its name");
    let (code, _) = refusal(
        &mut window,
        "db.transfer.start",
        json!({ "source": source, "target": source, "schema": ["shop", "public"], "tables": [{ "source": table, "name": "copy", "create": true, "columns": [{ "source": "id", "type": "int); DROP TABLE customers; --" }] }] }),
    );
    assert_eq!(code.as_deref(), Some("badMapping"), "a type never holds a second statement");
    assert!(!driver.server.executed().iter().any(|sql| sql.contains("TRUNCATE") || sql.contains("DROP")));
    let many: Vec<Value> = (0..201).map(|n| json!(["shop", "public", "tables", format!("t{n}")])).collect();
    let (code, _) =
        refusal(&mut window, "db.transfer.preview", json!({ "source": source, "target": source, "tables": many }));
    assert_eq!(code.as_deref(), Some("tooManyTables"), "never some of them silently");
}

fn connected(window: &mut Window, name: &str, url: String) -> String {
    let added = window.run("db.dataSource.add", json!({ "name": name, "url": url, "password": "zen", "type": "dev" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let operation = window.run("db.connection.connect", json!({ "connection": id }))["operation"].clone();
    while window.run("db.operation.state", json!({ "operation": operation }))["status"] == "running" {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    id
}

fn sql(window: &mut Window, connection: &str, text: &str) -> Value {
    let answer =
        window.run_confirmed("db.query.run", json!({ "connection": connection, "sql": text, "waitMs": 30_000 }));
    assert_eq!(answer["status"], "done", "{text}: {answer}");
    answer
}

#[test]
#[ignore = "needs PostgreSQL and MySQL in Docker (docs/development.md): ZEN_DB_PG_PORTS, ZEN_DB_MYSQL_SERVERS"]
fn a_postgresql_table_is_copied_into_a_new_mysql_table() {
    let pg_port = std::env::var("ZEN_DB_PG_PORTS").ok().and_then(|ports| ports.split(',').next()?.trim().parse().ok());
    let my_port = std::env::var("ZEN_DB_MYSQL_SERVERS").ok().and_then(|servers| {
        servers.split(',').find_map(|pair| pair.trim().strip_prefix("mysql:")?.parse::<u16>().ok())
    });
    let (Some(pg_port), Some(my_port)): (Option<u16>, Option<u16>) = (pg_port, my_port) else { return };
    let mut window = Window::new();
    window.session.state_mut::<super::super::Database>().set_drivers(zen_db::drivers());
    let pg =
        connected(&mut window, "PG", format!("postgresql://postgres@127.0.0.1:{pg_port}/postgres?sslmode=disable"));
    let my = connected(&mut window, "MySQL", format!("mysql://root@127.0.0.1:{my_port}/shop?ssl-mode=DISABLED"));
    sql(&mut window, &pg, "DROP TABLE IF EXISTS moved");
    sql(
        &mut window,
        &pg,
        "CREATE TABLE moved (id int PRIMARY KEY, label varchar(40), price numeric(10,2), at timestamptz, ok boolean, note text)",
    );
    sql(
        &mut window,
        &pg,
        "INSERT INTO moved SELECT n, 'row ' || n, n * 1.25, timestamptz '2026-01-01 10:00:00+03' + n * interval '1 hour', n % 2 = 0, CASE WHEN n % 10 = 0 THEN NULL ELSE 'é ' || n END FROM generate_series(1, 2500) n",
    );
    sql(&mut window, &my, "DROP TABLE IF EXISTS moved");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let table = json!(["postgres", "public", "tables", "moved"]);
    let preview = loop {
        super::super::connections::step(&mut window.session);
        match window.registry.dispatch(
            &mut window.session,
            "db.transfer.preview",
            json!({ "source": pg, "target": my, "tables": [table], "schema": ["shop", "shop"] }),
        ) {
            Ok(preview)
                if !preview["tables"][0]["loading"].as_bool().unwrap_or(false)
                    && preview["tables"][0]["columns"].as_array().is_some_and(|columns| !columns.is_empty()) =>
            {
                break preview;
            }
            _ => {}
        }
        assert!(std::time::Instant::now() < deadline, "the structure was read");
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert_eq!(preview["tables"][0]["create"], true, "{preview}");
    let started = window.run_confirmed(
        "db.transfer.start",
        json!({ "source": pg, "target": my, "schema": ["shop", "shop"], "tables": [{ "source": table, "create": true }], "load": { "batch": 500 } }),
    );
    let state = window.run("db.transfer.state", json!({ "id": started["id"], "waitMs": 60_000 }));
    let state = &state["transfers"][0];
    assert_eq!(state["status"], "done", "{started}\n{state}");
    assert_eq!((state["rowsRead"].as_u64(), state["rowsInserted"].as_u64()), (Some(2500), Some(2500)), "{state}");
    let counted = sql(&mut window, &my, "SELECT count(*), sum(price), sum(ok), count(note) FROM moved");
    let row = counted["results"][0]["page"][0].to_string();
    assert!(
        row.contains("2500") && row.contains("3907812.50") && row.contains("1250") && row.contains("2250"),
        "{row}"
    );
    let one = sql(&mut window, &my, "SELECT label, note FROM moved WHERE id = 7");
    assert!(one["results"][0]["page"][0].to_string().contains("é 7"), "{one}");
}
