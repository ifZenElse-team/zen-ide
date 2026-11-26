use serde_json::{Value, json};

use super::super::tests::{Window, open_customers, sql_connection};

fn kv_result(window: &mut Window, rows: usize) -> Value {
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> =
        vec![std::sync::Arc::new(zen_db::memory::MemoryDriver::new(false))];
    window.session.state_mut::<super::super::Database>().set_drivers(drivers);
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    window.run("db.settings.update", json!({ "page": 10_000, "rowCap": 200_000 }));
    let answer =
        window.run("db.query.run", json!({ "connection": id, "sql": format!("ROWS {rows}"), "waitMs": 30_000 }));
    assert_eq!(answer["status"], "done", "{answer}");
    let listed = window.run("db.results.list", json!({ "query": answer["id"] }));
    let result = listed["results"][0].clone();
    assert_eq!(result["rows"], rows, "{result}");
    result["id"].clone()
}

fn clipboard(window: &mut Window) -> String {
    crate::file_clipboard::clipboard_text(&mut window.session)
}

#[test]
fn plain_and_advanced_copy_write_their_options_and_remember_them() {
    let mut window = Window::new();
    let result = kv_result(&mut window, 3);
    let plain =
        window.run("db.result.copy", json!({ "id": result, "rows": [0, 1], "columns": [0, 2], "header": true }));
    assert_eq!(plain["text"], "id\tname\teven\n0\trow 0\ttrue\n1\trow 1\tfalse");
    assert_eq!((plain["rows"].as_u64(), plain["totalRows"].as_u64()), (Some(2), Some(2)));
    assert_eq!(plain["truncated"], false);

    let settings = json!({
        "header": true,
        "rowNumbers": true,
        "columnDelimiter": ";",
        "rowDelimiter": "\r\n",
        "quoting": "always",
        "nullText": "NULL",
    });
    let advanced = window.run(
        "db.result.copy",
        json!({ "id": result, "rows": [1, 2], "columns": [1, 3], "settings": settings, "remember": true }),
    );
    assert_eq!(
        advanced["text"],
        "\"#\";\"name\";\"even\";\"nothing\"\r\n2;\"row 1\";\"false\";NULL\r\n3;\"row 2\";\"true\";NULL"
    );
    let kept = window.run("db.copy.settings", json!({}));
    assert_eq!(kept["settings"]["columnDelimiter"], ";");
    assert_eq!(kept["formats"].as_array().unwrap().len(), 14);
    assert!(window.dir.path().join("data/databases/copy.json").is_file(), "kept in the data folder");
    let again =
        window.run("db.result.copy", json!({ "id": result, "rows": [0, 0], "columns": [0, 0], "advanced": true }));
    assert_eq!(again["text"], "\"#\";\"id\"\r\n1;\"0\"");

    let refused = window.refused("db.result.copy", json!({ "id": result, "settings": { "columnDelimiter": "" } }));
    assert!(refused.contains("cannot be empty"), "{refused}");
    let limited = window.run("db.result.copy", json!({ "id": result, "limit": 5 }));
    assert_eq!(limited["text"], "0\trow");
    assert_eq!((limited["truncated"].as_bool(), limited["cut"].as_str()), (Some(true), Some("limit")));
    assert!(limited["message"].as_str().unwrap().contains("larger limit"), "{limited}");

    let to_clipboard = window.run("db.result.copy", json!({ "id": result, "rows": [2, 2], "clipboard": true }));
    assert!(to_clipboard.get("text").is_none(), "the text goes to the clipboard: {to_clipboard}");
    assert_eq!(clipboard(&mut window), "2\trow 2\ttrue\t\t{n: 2, tag: x}");
}

#[test]
fn copy_as_writes_export_formats_sql_and_code_with_staged_values() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    window.run("db.result.setCell", json!({ "id": result, "row": 0, "column": 1, "text": "Ann" }));
    let copy_as = |window: &mut Window, format: &str, rows: [usize; 2], columns: [usize; 2]| -> String {
        let answer =
            window.run("db.result.copyAs", json!({ "id": result, "format": format, "rows": rows, "columns": columns }));
        answer["text"].as_str().unwrap().to_owned()
    };
    assert_eq!(
        copy_as(&mut window, "sqlInsert", [0, 1], [0, 1]),
        "INSERT INTO \"public\".\"customers\" (\"id\", \"name\") VALUES\n\t(1, 'Ann'),\n\t(2, 'Customer 2');\n"
    );
    assert_eq!(copy_as(&mut window, "sqlIn", [0, 2], [1, 1]), "\"name\" IN ('Ann', 'Customer 2', 'Customer 3')");
    assert_eq!(copy_as(&mut window, "sqlIn", [0, 1], [0, 1]), "(\"id\", \"name\") IN ((1, 'Ann'), (2, 'Customer 2'))");
    let update = copy_as(&mut window, "sqlUpdate", [1, 1], [0, 0]);
    assert!(
        update.starts_with("UPDATE \"public\".\"customers\" SET \"name\" = 'Customer 2', \"payload\" = "),
        "{update}"
    );
    assert!(update.ends_with("WHERE \"id\" = 2;"), "{update}");
    assert_eq!(copy_as(&mut window, "csv", [0, 0], [0, 1]), "id,name\n1,Ann\n");
    assert_eq!(copy_as(&mut window, "tsv", [0, 0], [0, 1]), "id\tname\n1\tAnn\n");
    assert_eq!(copy_as(&mut window, "json", [0, 0], [0, 1]), "[\n  {\"id\":1,\"name\":\"Ann\"}\n]\n");
    assert_eq!(copy_as(&mut window, "markdown", [0, 0], [0, 1]), "| id | name |\n| --- | --- |\n| 1 | Ann |\n");
    assert_eq!(
        copy_as(&mut window, "typeScript", [0, 1], [0, 1]),
        "const rows = [\n  { id: 1, name: \"Ann\" },\n  { id: 2, name: \"Customer 2\" },\n];\n"
    );
    assert!(copy_as(&mut window, "go", [0, 0], [0, 0]).contains("{\"id\": 1}"));
    assert_eq!(copy_as(&mut window, "columnNames", [0, 0], [0, 2]), "id, name, payload");
    assert_eq!(window.run("db.copy.settings", json!({}))["format"], "go", "the last format is remembered");
    assert!(
        !driver.server.executed().iter().any(|sql| sql.starts_with("UPDATE") || sql.starts_with("INSERT")),
        "copying runs nothing"
    );
}

#[test]
fn large_copies_run_in_the_background_with_a_cap_and_cancel_leaves_the_clipboard() {
    let mut window = Window::new();
    let result = kv_result(&mut window, 100_005);
    let started = window.run("db.result.copy", json!({ "id": result, "columns": [0, 1], "clipboard": true }));
    let job = started["job"].as_u64().expect("a large selection is copied in the background");
    assert_eq!(started["totalRows"], 100_005);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let done = loop {
        let state = window.run("db.result.copyState", json!({ "job": job }));
        if state["jobs"][0]["status"] != "running" {
            break state["jobs"][0].clone();
        }
        assert!(std::time::Instant::now() < deadline, "the copy ended: {state}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!((done["rows"].as_u64(), done["cut"].as_str()), (Some(100_000), Some("rows")));
    assert!(done["message"].as_str().unwrap().contains("Export"), "{done}");
    assert!(done.get("text").is_none(), "the state never holds the text");
    let text = clipboard(&mut window);
    assert_eq!(text.lines().count(), 100_000);
    assert!(text.ends_with("99999\trow 99999"));

    crate::file_clipboard::set_clipboard_text(&mut window.session, "before");
    let started = window.run("db.result.copyAs", json!({ "id": result, "format": "json", "clipboard": true }));
    let job = started["job"].as_u64().unwrap();
    window.run("db.result.copyState", json!({ "job": job, "cancel": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let ended = loop {
        let state = window.run("db.result.copyState", json!({ "job": job }));
        if state["jobs"][0]["status"] != "running" {
            break state["jobs"][0].clone();
        }
        assert!(std::time::Instant::now() < deadline, "the copy stopped: {state}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(ended["status"], "cancelled", "{ended}");
    assert_eq!(clipboard(&mut window), "before", "a cancelled copy leaves the clipboard");
    let unknown = window.refused("db.result.copyState", json!({ "job": 999 }));
    assert!(unknown.contains("999"), "{unknown}");
}

#[test]
fn paste_from_spreadsheets_checks_each_value_and_stages_cells_and_new_rows() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    let text = "name\tpayload\r\nAnn\t\"{\"\"a\"\": 1}\"\r\nBob\tnot json\r\n";
    let refused = window.refused("db.result.paste", json!({ "id": result, "text": text, "row": 23, "column": 1 }));
    assert!(refused.contains("One value does not fit"), "{refused}");
    assert_eq!(window.run("db.result.editState", json!({ "id": result }))["dirty"], false, "nothing staged");

    let checked = window.run(
        "db.result.paste",
        json!({ "id": result, "text": text, "row": 23, "column": 1, "dryRun": true, "preview": 5 }),
    );
    assert_eq!((checked["header"].as_bool(), checked["delimiter"].as_str()), (Some(true), Some("\t")));
    assert_eq!((checked["rows"].as_u64(), checked["cells"].as_u64()), (Some(2), Some(3)));
    assert_eq!(checked["applied"], false);
    let problem = &checked["problems"][0];
    assert_eq!((problem["line"].as_u64(), problem["column"].as_u64()), (Some(2), Some(2)));
    assert_eq!((problem["code"].as_str(), problem["name"].as_str()), (Some("invalidValue"), Some("payload")));
    assert!(!checked["problems"].to_string().contains("not json"), "problems never hold values: {checked}");
    assert_eq!(checked["preview"][1][1], "{\"a\": 1}");

    let pasted = window
        .run("db.result.paste", json!({ "id": result, "text": text, "row": 23, "column": 1, "skipInvalid": true }));
    assert_eq!((pasted["applied"].as_bool(), pasted["skipped"].as_u64()), (Some(true), Some(1)));
    assert_eq!(pasted["pending"]["updated"], 2);
    let page = window.run("db.result.page", json!({ "id": result, "offset": 23, "limit": 2 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Ann" }));
    assert_eq!(page["page"][0][2], json!({ "kind": "json", "value": "{\"a\": 1}" }));
    assert_eq!(page["page"][1][1], json!({ "kind": "text", "value": "Bob" }));

    let past = window.run("db.result.paste", json!({ "id": result, "text": "X\nY\nZ\n", "row": 24, "column": 1 }));
    assert_eq!((past["updatedRows"].as_u64(), past["newRows"].as_u64()), (Some(1), Some(2)), "{past}");
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!(state["pending"]["inserted"], 2);

    let inserted = window
        .run("db.result.paste", json!({ "id": result, "text": "id,name\n90,\"Smith, Jo\"\n91,\n", "mode": "insert" }));
    assert_eq!((inserted["delimiter"].as_str(), inserted["newRows"].as_u64()), (Some(","), Some(2)), "{inserted}");
    let page = window.run("db.result.page", json!({ "id": result, "offset": 27, "limit": 2 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Smith, Jo" }));
    assert_eq!(page["page"][1][1], json!({ "kind": "null" }), "an empty field is NULL");

    let not_null = window.refused("db.result.paste", json!({ "id": result, "text": "\t", "row": 0, "column": 0 }));
    assert!(not_null.contains("does not fit"), "{not_null}");
    let wide = window
        .run("db.result.paste", json!({ "id": result, "text": "a\tb\tc", "row": 0, "column": 4, "dryRun": true }));
    let codes: Vec<&str> = wide["problems"].as_array().unwrap().iter().filter_map(|p| p["code"].as_str()).collect();
    assert_eq!(codes, ["noColumn"], "{wide}");
    window.run("db.result.deleteRows", json!({ "id": result, "rows": [5] }));
    let deleted =
        window.run("db.result.paste", json!({ "id": result, "text": "x", "row": 5, "column": 1, "dryRun": true }));
    assert_eq!(deleted["problems"][0]["code"], "deletedRow");
    let empty = window.refused("db.result.paste", json!({ "id": result, "text": "" }));
    assert!(empty.contains("nothing to paste"), "{empty}");

    crate::file_clipboard::set_clipboard_text(&mut window.session, "Zed");
    window.run("db.result.paste", json!({ "id": result, "row": 3, "column": 1 }));
    let page = window.run("db.result.page", json!({ "id": result, "offset": 3, "limit": 1 }));
    assert_eq!(
        page["page"][0][1],
        json!({ "kind": "text", "value": "Zed" }),
        "the clipboard's text when none is given"
    );
    assert!(
        !driver.server.executed().iter().any(|sql| sql.starts_with("UPDATE") || sql.starts_with("INSERT")),
        "pasting never writes to the server"
    );
}

#[test]
fn a_header_in_another_column_order_pastes_each_value_under_the_column_it_names() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    let text = "payload\tname\n\"{\"\"a\"\": 1}\"\tAnn\n";
    let pasted = window.run("db.result.paste", json!({ "id": result, "text": text, "row": 0, "column": 1 }));
    assert_eq!((pasted["header"].as_bool(), pasted["cells"].as_u64()), (Some(true), Some(2)), "{pasted}");
    let page = window.run("db.result.page", json!({ "id": result, "offset": 0, "limit": 1 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Ann" }));
    assert_eq!(page["page"][0][2], json!({ "kind": "json", "value": "{\"a\": 1}" }));

    let inserted = window
        .run("db.result.paste", json!({ "id": result, "text": "name,id\nZoe,95\n", "mode": "insert", "column": 1 }));
    assert_eq!(inserted["newRows"], 1, "{inserted}");
    let rows = window.run("db.result.editState", json!({ "id": result }))["pending"]["inserted"].as_u64().unwrap();
    assert_eq!(rows, 1);
    let page = window.run("db.result.page", json!({ "id": result, "offset": 25, "limit": 1 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Zoe" }), "{page}");
    assert_eq!(page["page"][0][0]["value"], 95, "{page}");
}

#[test]
fn a_read_only_result_refuses_paste_but_copies() {
    let mut window = Window::new();
    let result = kv_result(&mut window, 2);
    let refused = window.refused("db.result.paste", json!({ "id": result, "text": "1" }));
    assert!(refused.contains("cannot"), "{refused}");
    let update = window.refused("db.result.copyAs", json!({ "id": result, "format": "sqlUpdate" }));
    assert!(update.contains("cannot"), "{update}");
    let copied = window.run("db.result.copyAs", json!({ "id": result, "format": "python", "columns": [0, 0] }));
    assert_eq!(copied["text"], "rows = [\n    {\"id\": 0},\n    {\"id\": 1},\n]\n");
}
