use std::path::PathBuf;

use serde_json::{Value, json};

use super::super::tests::Window;

fn sqlite(window: &mut Window, name: &str, file: &str, sql: &str) -> String {
    window.run("db.databaseFile.create", json!({ "path": file }));
    let added = window
        .run("db.dataSource.add", json!({ "name": name, "driver": "sqlite", "database": file, "scope": "project" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let ran = window.run("db.query.run", json!({ "connection": id, "sql": sql, "waitMs": 30_000 }));
    assert_eq!(ran["status"], "done", "{ran}");
    id
}

fn finished(window: &mut Window, id: &Value) -> Value {
    let answer = window.run("db.compare.state", json!({ "id": id, "waitMs": 30_000 }));
    let state = answer["compares"][0].clone();
    assert_ne!(state["status"], "running", "{state}");
    state
}

fn names(state: &Value) -> Vec<(String, String)> {
    state["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| (object["name"].as_str().unwrap().to_owned(), object["change"].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn two_connections_compare_and_the_script_only_ever_opens_in_a_console() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    }
    let mut window = Window::new();
    let a = sqlite(
        &mut window,
        "A",
        "a.sqlite",
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER, note TEXT);\n\
         CREATE TABLE extra (id INTEGER PRIMARY KEY);\n\
         INSERT INTO items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 2), (3, 'c', 3);",
    );
    let b = sqlite(
        &mut window,
        "B",
        "b.sqlite",
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER);\n\
         INSERT INTO items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 20), (9, 'z', 9);",
    );
    let side = |id: &str| json!({ "kind": "connection", "connection": id, "schema": "main" });
    let started = window.run("db.compare.schemas", json!({ "source": side(&a), "target": side(&b) }));
    let state = finished(&mut window, &started["id"]);
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!(state["source"]["label"], "A · main");
    assert_eq!(state["source"]["cached"], false, "read again before comparing");
    assert_eq!(names(&state), [("extra".to_owned(), "added".to_owned()), ("items".to_owned(), "changed".to_owned())]);
    assert_eq!(state["objects"][1]["items"][0]["name"], "note");
    assert_eq!((state["summary"]["added"].as_u64(), state["summary"]["changed"].as_u64()), (Some(1), Some(1)));
    let paged = window.run("db.compare.state", json!({ "id": started["id"], "limit": 1 }));
    assert_eq!(paged["compares"][0]["truncated"], true);
    assert_eq!(paged["compares"][0]["total"], 2);
    let listed = window.run("db.compare.state", json!({ "limit": 1 }));
    assert_eq!(listed["compares"][0]["request"], json!({ "source": side(&a), "target": side(&b) }));

    let ddl = window.run("db.compare.ddl", json!({ "id": started["id"], "kind": "table", "key": "items" }));
    assert!(ddl["source"].as_str().unwrap().contains("note"), "{ddl}");
    assert!(!ddl["target"].as_str().unwrap().contains("note"), "{ddl}");
    let only_source = window.run("db.compare.ddl", json!({ "id": started["id"], "kind": "table", "key": "extra" }));
    assert_eq!(only_source["target"], "");

    let script = window.run("db.compare.script", json!({ "id": started["id"] }));
    let text = script["text"].as_str().unwrap();
    assert!(text.contains("CREATE TABLE extra"), "{text}");
    assert!(text.contains("ALTER TABLE \"main\".\"items\" ADD COLUMN \"note\" TEXT;"), "{text}");
    let opened = window.run("db.compare.script", json!({ "id": started["id"], "open": true }));
    let console = PathBuf::from(opened["console"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(&console).unwrap(), text);
    assert!(console.to_string_lossy().contains(&format!("/{b}/")), "a console of the target: {console:?}");
    let again = window.run("db.compare.schemas", json!({ "source": side(&a), "target": side(&b) }));
    let state = finished(&mut window, &again["id"]);
    assert_eq!(state["summary"]["added"], 1, "nothing ran the script");

    let saved = window.run("db.compare.saveSnapshot", json!({ "connection": a, "schema": "main" }));
    let file = PathBuf::from(saved["path"].as_str().unwrap());
    assert!(file.starts_with(window.dir.path().join("data/databases/snapshots")), "{file:?}");
    assert_eq!(saved["objects"], 2);
    let refused = window.refused("db.compare.saveSnapshot", json!({ "connection": a, "schema": "main", "path": file }));
    assert!(refused.contains("exists") || refused.contains("already"), "{refused}");
    let listed = window.run("db.compare.snapshots", json!({}));
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["snapshots"][0]["connection"], "A");
    let offline = json!({ "kind": "connection", "connection": b, "schema": "main", "refresh": false });
    let from_file =
        window.run("db.compare.schemas", json!({ "source": { "kind": "snapshot", "path": file }, "target": offline }));
    let state = finished(&mut window, &from_file["id"]);
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!(state["target"]["cached"], true);
    assert_eq!(names(&state).len(), 2);

    let data = window.run(
        "db.compare.data",
        json!({ "source": { "connection": a, "table": "items" }, "target": { "connection": b, "table": "items" } }),
    );
    let state = finished(&mut window, &data["id"]);
    assert_eq!(state["status"], "done", "{state}");
    let counts = &state["data"]["counts"];
    assert_eq!(
        [&counts["same"], &counts["changed"], &counts["added"], &counts["removed"]],
        [&json!(1), &json!(1), &json!(1), &json!(1)],
        "{state}"
    );
    assert_eq!(state["data"]["columns"], json!(["id", "name", "qty"]), "the columns both tables have");
    let rows = window.run("db.compare.script", json!({ "id": data["id"] }));
    let text = rows["text"].as_str().unwrap();
    assert!(text.contains("INSERT INTO \"items\" (\"id\", \"name\", \"qty\") VALUES (3, 'c', 3);"), "{text}");
    assert!(text.contains("UPDATE \"items\" SET \"qty\" = 2 WHERE \"id\" = 2;"), "{text}");
    assert!(!text.contains("DELETE"), "{text}");
    let deleting = window.run("db.compare.script", json!({ "id": data["id"], "drop": true }));
    assert!(deleting["text"].as_str().unwrap().contains("DELETE FROM \"items\" WHERE \"id\" = 9;"));

    let listed = window.run("db.compare.state", json!({}));
    assert_eq!(listed["compares"].as_array().unwrap().len(), 4);
    assert!(listed["compares"][0]["objects"].is_null(), "listing leaves the findings out");
    let unknown = window.refused("db.compare.cancel", json!({ "id": 99 }));
    assert!(unknown.contains("99"), "{unknown}");
    let tab = window.run("db.compare.open", json!({ "source": a, "sourceSchema": "main" }));
    assert_eq!(tab["kind"], "compare");
    assert_eq!(tab["data"]["sourceSchema"], "main");
    for id in [a, b] {
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn comparisons_refuse_what_they_cannot_read() {
    let mut window = Window::new();
    let refused = window.refused(
        "db.compare.schemas",
        json!({
            "source": { "kind": "snapshot", "path": "/nowhere/x.json" },
            "target": { "kind": "snapshot", "path": "/nowhere/y.json" }
        }),
    );
    assert!(refused.contains("/nowhere/x.json"), "{refused}");
    let missing = window.refused(
        "db.compare.data",
        json!({ "source": { "connection": "nope", "table": "t" }, "target": { "connection": "nope", "table": "t" } }),
    );
    assert!(missing.contains("nope"), "{missing}");
}
