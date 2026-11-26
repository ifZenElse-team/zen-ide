use std::path::PathBuf;

use serde_json::{Value, json};
use zen_db::schema::{Object, ObjectKind, Routine, Trigger};

use super::super::injected::tests::{open, with_highlighting};
use super::super::tests::{Window, orders_connection, sql_connection};

const QUERIES: &str = "SELECT c.name FROM customers c;\nUPDATE customers SET name = 'x' WHERE id = 1;\n-- customers\nSELECT 1 FROM orders;\n";

const GO: &str = "package main\n\nconst q = \"SELECT id FROM customers WHERE name = $1\"\n\nvar s = \"customers\"\n";

fn shop_connection(window: &mut Window) -> String {
    let id = orders_connection(window, false);
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    let mut snapshot = zen_db::schema::read_snapshot(&file).unwrap();
    let mut view = Object::new(ObjectKind::View, "named_customers");
    view.detailed = true;
    view.definition = Some(
        " SELECT customers.id,\n    customers.name\n   FROM customers\n  WHERE customers.name IS NOT NULL;".into(),
    );
    let mut function = Object::new(ObjectKind::Function, "touch");
    function.routine = Some(Routine {
        arguments: String::new(),
        result: Some("trigger".into()),
        language: "plpgsql".into(),
        kind: "function".into(),
        definition: Some(
            "CREATE FUNCTION public.touch() RETURNS trigger AS $$\nBEGIN\n  UPDATE public.customers SET name = upper(name);\n  RETURN NEW;\nEND $$ LANGUAGE plpgsql"
                .into(),
        ),
    });
    let objects = &mut snapshot.catalogs[0].schemas[0].objects;
    objects[0].triggers.push(Trigger {
        name: "customers_touch".into(),
        definition: "CREATE TRIGGER customers_touch BEFORE UPDATE OF name ON public.customers FOR EACH ROW EXECUTE FUNCTION touch()".into(),
        enabled: true,
        function: "touch".into(),
    });
    objects.push(view);
    objects.push(function);
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
    id
}

fn write(window: &mut Window, name: &str, text: &str) -> PathBuf {
    let path = window.dir.path().join("project").join(name);
    std::fs::write(&path, text).unwrap();
    refresh(window, &path);
    path
}

fn refresh(window: &mut Window, path: &std::path::Path) {
    while window.run("search.index.status", json!({}))["ready"] != true {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    window.session.state::<crate::Workbench>().unwrap().index.clone().unwrap().refresh(&[path.to_owned()]);
}

fn open_code(window: &mut Window, name: &str, text: &str) -> PathBuf {
    let path = open(window, name, text);
    refresh(window, &path);
    path
}

fn places(answer: &Value) -> Vec<String> {
    answer["usages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let place = row["path"].as_str().or(row["holder"].as_str()).unwrap();
            let access = row["access"].as_str().unwrap_or("-");
            format!("{place}:{}:{} {access}", row["line"], row["column"])
        })
        .collect()
}

fn customers() -> Value {
    json!(["shop", "public", "tables", "customers"])
}

#[test]
fn usages_of_a_table_are_found_in_scripts_code_consoles_and_the_database() {
    let mut window = Window::new();
    let id = shop_connection(&mut window);
    let (other, _) = sql_connection(&mut window, "prod", true);
    with_highlighting(&mut window);
    write(&mut window, "queries.sql", QUERIES);
    let elsewhere = write(&mut window, "elsewhere.sql", "SELECT * FROM customers;\n");
    window.run("db.script.bind", json!({ "path": elsewhere, "connection": other }));
    write(&mut window, "notes.md", "customers\n");
    open_code(&mut window, "repo.go", GO);
    let console = window.run("db.console.new", json!({ "connection": id, "text": "select * from Customers" }));
    let console = PathBuf::from(console["path"].as_str().unwrap());

    let found = window.run("db.usages.find", json!({ "connection": id, "path": customers(), "waitMs": 10_000 }));
    assert_eq!(found["done"], true, "{found}");
    assert_eq!(found["label"], "Table public.customers");
    assert_eq!(
        places(&found),
        vec![
            format!("Consoles/{}:1:15 read", console.file_name().unwrap().to_string_lossy()),
            "queries.sql:1:20 read".to_owned(),
            "queries.sql:2:8 write".to_owned(),
            "repo.go:3:27 read".to_owned(),
            "Function public.touch:3:17 -".to_owned(),
            "Trigger customers_touch · Table public.customers:1:64 -".to_owned(),
            "Foreign key orders_customer_fk · Table public.orders:1:38 -".to_owned(),
            "View public.named_customers:1:9 read".to_owned(),
            "View public.named_customers:2:5 read".to_owned(),
            "View public.named_customers:3:9 read".to_owned(),
            "View public.named_customers:4:9 read".to_owned(),
        ],
        "{found}"
    );
    assert_eq!(found["total"], 11);
    assert_ne!(window.run("db.connection.status", json!({ "connection": id }))["state"], "connected");

    let limited =
        window.run("db.usages.find", json!({ "connection": id, "path": customers(), "waitMs": 10_000, "limit": 2 }));
    assert_eq!((limited["usages"].as_array().unwrap().len(), &limited["truncated"]), (2, &json!(true)));

    let tree = window.run("search.results.tree", json!({ "tab": limited["tab"] }));
    assert_eq!(tree["kind"], "database");
    assert_eq!(tree["title"], "Usages of customers");
    let text = tree.to_string();
    assert!(text.contains("\"command\":\"db.object.open\""), "{tree}");
    assert!(text.contains("named_customers"), "{tree}");
    assert_eq!((&tree["total"], &tree["files"], &tree["elsewhere"]), (&json!(11), &json!(3), &json!(7)), "{tree}");

    let again = window
        .run("db.usages.find", json!({ "connection": id, "path": customers(), "waitMs": 10_000, "newTab": true }));
    let titles: Vec<Value> = [&limited, &again]
        .iter()
        .map(|answer| window.run("search.results.tree", json!({ "tab": answer["tab"] }))["title"].clone())
        .collect();
    assert_eq!(titles, [json!("Usages of customers"), json!("Usages of customers (2)")]);
}

#[test]
fn usages_of_a_column_and_a_rename_that_replaces_only_files() {
    let mut window = Window::new();
    let id = shop_connection(&mut window);
    with_highlighting(&mut window);
    let queries = write(&mut window, "queries.sql", QUERIES);
    let go = open_code(&mut window, "repo.go", GO);
    let console = window.run("db.console.new", json!({ "connection": id, "text": "select name from customers" }));
    let console = PathBuf::from(console["path"].as_str().unwrap());

    let path = json!(["shop", "public", "tables", "customers", "columns", "name"]);
    let found = window.run("db.usages.find", json!({ "connection": id, "path": path, "waitMs": 10_000 }));
    assert_eq!(found["label"], "Column public.customers.name", "{found}");
    let listed = places(&found);
    assert!(listed.contains(&"queries.sql:1:10 read".to_owned()), "{listed:?}");
    assert!(listed.contains(&"queries.sql:2:22 write".to_owned()), "{listed:?}");
    assert!(listed.contains(&"repo.go:3:43 read".to_owned()), "{listed:?}");
    assert!(listed.iter().any(|place| place.starts_with("Consoles/")), "{listed:?}");
    assert!(listed.iter().any(|place| place.starts_with("View public.named_customers:2:")), "{listed:?}");
    assert!(listed.iter().any(|place| place.starts_with("Trigger customers_touch")), "{listed:?}");
    assert!(!listed.iter().any(|place| place.contains("orders_customer_fk")), "{listed:?}");

    let renamed = window.run(
        "db.usages.rename",
        json!({ "connection": id, "path": customers(), "newName": "clients", "waitMs": 10_000 }),
    );
    assert_eq!(renamed["newName"], "clients");
    let tree = window.run("search.results.tree", json!({ "tab": renamed["tab"] }));
    assert_eq!(tree["title"], "Rename customers to clients", "{tree}");
    let replaced = window.run("search.results.replace", json!({ "tab": renamed["tab"], "nodes": ["found"] }));
    assert_eq!((replaced["files"].as_u64(), replaced["replacements"].as_u64()), (Some(2), Some(3)), "{replaced}");
    let text = std::fs::read_to_string(&queries).unwrap();
    assert_eq!(
        text,
        "SELECT c.name FROM clients c;\nUPDATE clients SET name = 'x' WHERE id = 1;\n-- customers\nSELECT 1 FROM orders;\n"
    );
    let go_text = window
        .session
        .state::<crate::Workbench>()
        .unwrap()
        .documents()
        .iter()
        .find(|document| document.path() == go)
        .unwrap()
        .text()
        .to_owned();
    assert_eq!(go_text, GO.replacen("FROM customers", "FROM clients", 1));
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "select name from customers");
    let left = window.run("search.results.tree", json!({ "tab": renamed["tab"] }));
    assert_eq!(left["total"], 8, "the console and the database's objects stay listed: {left}");
}

#[test]
fn find_usages_in_sql_goes_to_the_database_and_unknown_targets_are_refused() {
    let mut window = Window::new();
    let id = shop_connection(&mut window);
    with_highlighting(&mut window);
    write(&mut window, "queries.sql", QUERIES);
    let go = open_code(&mut window, "repo.go", GO);
    window.run("db.injection.setDefault", json!({ "connection": id }));

    let at = GO.find("customers").unwrap() + 2;
    let found =
        window.run("navigate.usages.find", json!({ "path": go, "offset": at, "waitMs": 10_000, "compact": true }));
    assert_eq!(found["symbol"], "Table public.customers", "{found}");
    assert!(found["tab"].as_u64().is_some());
    let rows: Vec<&str> = found["rows"].as_array().unwrap().iter().map(|row| row.as_str().unwrap()).collect();
    assert!(rows.contains(&"queries.sql:1:20 SELECT c.name FROM customers c;"), "{rows:?}");

    let by_place = window.run("db.usages.find", json!({ "file": go, "offset": at, "waitMs": 10_000 }));
    assert_eq!(by_place["target"]["path"], customers());

    let refused =
        window.refused("db.usages.find", json!({ "connection": id, "path": ["shop", "public", "tables", "nothing"] }));
    assert!(refused.contains("nothing"), "{refused}");
    let schema = window.refused("db.usages.find", json!({ "connection": id, "path": ["shop", "public"] }));
    assert!(!schema.is_empty());
    let empty = window.refused("db.usages.rename", json!({ "connection": id, "path": customers(), "newName": " " }));
    assert!(!empty.is_empty());
}
