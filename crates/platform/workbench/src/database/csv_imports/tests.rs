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

fn ended(window: &mut Window, started: &Value) -> Value {
    let state = window.run("db.csvImport.state", json!({ "id": started["id"], "waitMs": 30_000 }));
    let state = state["imports"][0].clone();
    assert_ne!(state["status"], "running", "{state}");
    state
}

fn notes(window: &mut Window) -> Vec<String> {
    super::super::connections::step(&mut window.session);
    let notes = window.session.state::<crate::notifications::Notifications>().unwrap();
    notes.newest_first().map(|note| note.body.clone()).collect()
}

fn count(window: &mut Window, id: &str, table: &str) -> Value {
    let ran = window.run(
        "db.query.run",
        json!({ "connection": id, "sql": format!("SELECT count(*) AS n FROM {table}"), "waitMs": 20_000 }),
    );
    ran["results"][0]["page"][0][0]["value"].clone()
}

#[test]
fn a_csv_file_is_previewed_mapped_and_imported_into_sqlite_with_the_settings_kept() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        return;
    }
    let mut window = Window::new();
    let database = window.dir.path().join("shop.db");
    std::fs::File::create(&database).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": "sqlite", "database": database.display().to_string(), "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run(
        "db.query.run",
        json!({ "connection": id, "waitMs": 20_000, "sql": "CREATE TABLE orders (id INTEGER PRIMARY KEY, note TEXT, placed TIMESTAMP); INSERT INTO orders VALUES (1000, 'seed', NULL);" }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    let file = window.dir.path().join("orders.csv");
    let mut text = String::from("id;note;placed\n");
    for at in 1..=30 {
        text.push_str(&format!("{at};\"note {at}\";27.10.2026 10:{at:02}\n"));
    }
    text.push_str("31;bad date;soon\n2;duplicate;27.10.2026 10:00\n");
    std::fs::write(&file, &text).unwrap();
    let format = json!({ "delimiter": ";", "timestampFormat": "dd.MM.yyyy HH:mm" });

    let preview = window.run("db.csvImport.preview", json!({ "file": file, "format": format, "limit": 5 }));
    assert_eq!(
        preview["columns"],
        json!([
            { "name": "id", "kind": "integer" },
            { "name": "note", "kind": "text" },
            { "name": "placed", "kind": "text" },
        ]),
        "one bad date makes the column text: {preview}"
    );
    assert_eq!(preview["rows"].as_array().unwrap().len(), 5);
    assert_eq!(preview["rows"][0], json!(["1", "note 1", "27.10.2026 10:01"]));
    assert_eq!((preview["truncated"].as_bool(), preview["fileBytes"].as_u64()), (Some(true), Some(text.len() as u64)));

    let table = json!(["shop.db", "main", "tables", "orders"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let tab = loop {
        super::super::connections::step(&mut window.session);
        if let Ok(opened) =
            window.registry.dispatch(&mut window.session, "db.object.open", json!({ "connection": id, "path": table }))
        {
            break opened["tab"]["id"].as_str().unwrap().to_owned();
        }
        assert!(std::time::Instant::now() < deadline, "the table reaches the snapshot");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let before = super::super::tests::until_result(&mut window, &tab)["data"]["query"].clone();
    let started = window.run(
        "db.csvImport.start",
        json!({
            "connection": id,
            "file": file,
            "table": table,
            "format": format,
            "columns": [
                { "source": 0, "target": "id", "kind": "integer" },
                { "source": 1, "target": "note" },
                { "source": 2, "target": "placed", "kind": "timestamp" },
            ],
            "load": { "batch": 8, "onError": "skip" },
        }),
    );
    assert_eq!(started["table"], "\"main\".\"orders\"", "{started}");
    assert_eq!(started["statements"], json!([]));
    let state = ended(&mut window, &started);
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!((state["rowsInserted"].as_u64(), state["rowsSkipped"].as_u64()), (Some(30), Some(2)), "{state}");
    assert_eq!(state["errors"][0]["code"], "notADate", "{state}");
    assert_eq!((state["errors"][1]["row"].as_u64(), state["errors"][1]["line"].as_u64()), (Some(32), Some(33)));
    assert!(!state.to_string().contains("note 1\""), "answers never hold rows: {state}");
    assert_eq!(count(&mut window, &id, "orders"), json!(31));
    let bodies = notes(&mut window);
    let after = super::super::tests::until_result(&mut window, &tab);
    assert_ne!(after["data"]["query"], before, "the open table reads its rows again: {after}");
    assert!(
        bodies.iter().any(|body| body.starts_with("30 rows were imported into orders; 2 were skipped:")
            && body.contains("Row 31 (line 32): placed does not match the date and time format")),
        "{bodies:?}"
    );
    let kept = window.run("db.csvImport.settings", json!({}));
    assert_eq!((kept["format"]["delimiter"].as_str(), kept["load"]["onError"].as_str()), (Some(";"), Some("skip")));
    assert_eq!(kept["folder"], json!(window.dir.path()));
    assert!(window.dir.path().join("data/databases/import.json").exists());

    let people = window.dir.path().join("people.csv");
    std::fs::write(&people, "name,age,member\nAnn,41,true\nBob,,false\n").unwrap();
    let new_table = json!(["shop.db", "main", "tables", "people"]);
    let preview = window.run(
        "db.csvImport.preview",
        json!({ "file": people, "format": {}, "connection": id, "table": new_table, "create": true }),
    );
    assert_eq!(preview["target"]["exists"], false, "{preview}");
    assert_eq!(
        preview["mapping"],
        json!([
            { "source": 0, "target": "name", "kind": "text", "sqlType": "TEXT" },
            { "source": 1, "target": "age", "kind": "integer", "sqlType": "INTEGER" },
            { "source": 2, "target": "member", "kind": "boolean", "sqlType": "BOOLEAN" },
        ])
    );
    let started = window.run(
        "db.csvImport.start",
        json!({ "connection": id, "file": people, "table": new_table, "create": true, "format": {}, "remember": false }),
    );
    assert_eq!(
        started["statements"],
        json!([
            "CREATE TABLE \"main\".\"people\" (\n    \"name\" TEXT,\n    \"age\" INTEGER,\n    \"member\" BOOLEAN\n)"
        ])
    );
    assert_eq!(ended(&mut window, &started)["status"], "done");
    assert_eq!(count(&mut window, &id, "people"), json!(2));
    assert_eq!(
        window.run("db.csvImport.settings", json!({}))["load"]["onError"],
        "skip",
        "remember: false keeps the last"
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        notes(&mut window);
        let preview = window.run(
            "db.csvImport.preview",
            json!({ "file": people, "format": {}, "connection": id, "table": new_table, "limit": 1 }),
        );
        if preview["target"]["exists"] == true && preview["target"]["loading"] == false {
            assert_eq!(preview["mapping"].as_array().map(Vec::len), Some(3), "{preview}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the new table reaches the snapshot: {preview}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (code, _) = refusal(
        &mut window,
        "db.csvImport.start",
        json!({ "connection": id, "file": people, "table": new_table, "create": true, "format": {} }),
    );
    assert_eq!(code.as_deref(), Some("exists"), "the snapshot knows the new table now");
    let smuggled = json!(["shop.db", "main", "tables", "people_two"]);
    let (code, _) = refusal(
        &mut window,
        "db.csvImport.start",
        json!({ "connection": id, "file": people, "table": smuggled, "create": true, "format": {},
                "columns": [{ "source": 0, "target": "id", "sqlType": "int); DROP TABLE users; --" }] }),
    );
    assert_eq!(code.as_deref(), Some("badSqlType"));
    let none = window.dir.path().join("none.csv");
    let (code, _) =
        refusal(&mut window, "db.csvImport.start", json!({ "connection": id, "file": none, "table": table }));
    assert_eq!(code.as_deref(), Some("noFile"));
    let (code, _) =
        refusal(&mut window, "db.csvImport.preview", json!({ "file": people, "format": { "delimiter": "\"" } }));
    assert_eq!(code.as_deref(), Some("delimiterQuote"));
    assert_eq!(window.run("db.csvImport.state", json!({}))["imports"].as_array().unwrap().len(), 2);
    assert_eq!(window.run("db.csvImport.cancel", json!({ "id": started["id"] }))["cancelled"], false);
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    let op = closing["operation"].as_u64().unwrap();
    while window.run("db.operation.state", json!({ "operation": op }))["status"] == "running" {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn production_asks_first_read_only_refuses_and_truncate_always_asks() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "prod", true);
    let file = window.dir.path().join("customers.csv");
    std::fs::write(&file, "ID,Name,unknown\n1,Ann,x\n2,Bob,y\n").unwrap();
    let table = json!(["shop", "public", "tables", "customers"]);
    let params = json!({ "connection": id, "file": file, "table": table, "format": {} });
    let (code, data) = refusal(&mut window, "db.csvImport.start", params.clone());
    assert_eq!(code.as_deref(), Some("confirmExecute"), "{data}");
    assert_eq!(
        data["statements"][0]["text"], "INSERT INTO \"public\".\"customers\" (\"id\", \"name\") VALUES (…)",
        "{data}"
    );
    assert_eq!(data["retry"]["command"], "db.csvImport.start");
    assert!(!driver.server.executed().iter().any(|sql| sql.starts_with("INSERT")), "nothing ran before the answer");
    let started = window.run_confirmed("db.csvImport.start", params);
    assert_eq!(
        started["columns"],
        json!([
            { "source": 0, "target": "id", "kind": "integer" },
            { "source": 1, "target": "name", "kind": "text" },
        ]),
        "the header maps onto the table's columns by name, types from the snapshot"
    );
    assert_eq!(ended(&mut window, &started)["rowsInserted"], 2);
    let inserted: Vec<String> = driver.server.executed().into_iter().filter(|sql| sql.starts_with("INSERT")).collect();
    assert_eq!(inserted, ["INSERT INTO \"public\".\"customers\" (\"id\", \"name\") VALUES\n(1, 'Ann'),\n(2, 'Bob')"]);

    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let truncating = json!({
        "connection": id, "file": file, "table": table, "format": {}, "load": { "truncate": true },
    });
    let (code, data) = refusal(&mut window, "db.csvImport.start", truncating);
    assert_eq!((code.as_deref(), data["dangerous"].as_bool()), (Some("confirmExecute"), Some(true)), "{data}");
    assert_eq!(data["statements"][0]["text"], "TRUNCATE TABLE \"public\".\"customers\"");

    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": true }));
    let (code, _) =
        refusal(&mut window, "db.csvImport.start", json!({ "connection": id, "file": file, "table": table }));
    assert_eq!(code.as_deref(), Some("readOnly"));
    let (code, _) = refusal(
        &mut window,
        "db.csvImport.start",
        json!({ "connection": id, "file": file, "table": ["shop", "public"] }),
    );
    assert_eq!(code.as_deref(), Some("notATable"));
}
