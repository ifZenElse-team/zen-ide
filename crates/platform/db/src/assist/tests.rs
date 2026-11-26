use super::*;
use crate::schema::{CachedNode, Constraint, Reference, Routine};

fn column(position: i32, name: &str, type_name: &str) -> Column {
    Column { name: name.into(), position, type_name: type_name.into(), nullable: true, ..Column::default() }
}

fn table(name: &str, columns: &[(&str, &str)]) -> Object {
    let mut object = Object::new(ObjectKind::Table, name);
    object.detailed = true;
    object.columns = columns
        .iter()
        .enumerate()
        .map(|(at, (name, type_name))| column(i32::try_from(at + 1).unwrap(), name, type_name))
        .collect();
    object.constraints.push(Constraint {
        name: format!("{name}_pkey"),
        kind: ConstraintKind::PrimaryKey,
        columns: vec![columns[0].0.into()],
        definition: String::new(),
        references: None,
        index: None,
        comment: None,
    });
    object
}

fn foreign_key(object: &mut Object, name: &str, columns: &[&str], table: &str, references: &[&str]) {
    object.constraints.push(Constraint {
        name: name.into(),
        kind: ConstraintKind::ForeignKey,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: String::new(),
        references: Some(Reference {
            schema: "public".into(),
            table: table.into(),
            columns: references.iter().map(|column| (*column).to_owned()).collect(),
            on_update: "no action".into(),
            on_delete: "no action".into(),
        }),
        index: None,
        comment: None,
    });
}

fn shop() -> Snapshot {
    let customers = table("customers", &[("id", "bigint"), ("name", "text"), ("email", "text")]);
    let mut orders = table("orders", &[("id", "bigint"), ("customer_id", "bigint"), ("total", "numeric")]);
    foreign_key(&mut orders, "orders_customer_fk", &["customer_id"], "customers", &["id"]);
    let mut items = table("order_items", &[("order_id", "bigint"), ("product_id", "bigint"), ("qty", "integer")]);
    foreign_key(&mut items, "items_order_fk", &["order_id"], "orders", &["id"]);
    foreign_key(&mut items, "items_product_fk", &["product_id"], "products", &["id"]);
    let products = table("products", &[("id", "bigint"), ("title", "text"), ("price", "numeric")]);
    let mut active = table("active_customers", &[("id", "bigint"), ("name", "text")]);
    active.kind = ObjectKind::View;
    active.constraints.clear();
    let mut discount = Object::new(ObjectKind::Function, "discount");
    discount.routine = Some(Routine {
        arguments: "total numeric".into(),
        result: Some("numeric".into()),
        language: "sql".into(),
        kind: "function".into(),
        definition: None,
    });
    let leads = table("Leads", &[("id", "bigint"), ("Full Name", "text")]);
    let events = Object::new(ObjectKind::Table, "events");
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![
                Schema {
                    name: "public".into(),
                    level: Level::Details,
                    objects: vec![customers, orders, items, products, active, discount],
                    ..Schema::default()
                },
                Schema { name: "sales".into(), level: Level::Details, objects: vec![leads], ..Schema::default() },
                Schema { name: "big".into(), level: Level::Names, objects: vec![events], ..Schema::default() },
                Schema { name: "pg_catalog".into(), system: true, ..Schema::default() },
            ],
            ..Catalog::default()
        }],
        ..Snapshot::new("shop", "postgresql")
    }
}

fn at_caret(text: &str) -> (String, usize) {
    let caret = text.find('|').expect("a caret");
    (text.replacen('|', "", 1), caret)
}

fn suggest(snapshot: &Snapshot, text: &str) -> Option<Suggestions> {
    let (text, caret) = at_caret(text);
    let view = Catalogue::new(snapshot, SqlDialect::Postgres, None, None);
    complete(&text, caret, &view)
}

fn labels(suggestions: &Suggestions, kind: ItemKind) -> Vec<String> {
    suggestions.items.iter().filter(|item| item.kind == kind).map(|item| item.label.clone()).collect()
}

fn inserted(suggestions: &Suggestions, label: &str) -> Vec<String> {
    suggestions.items.iter().filter(|item| item.label == label).map(|item| item.insert.clone()).collect()
}

#[test]
fn a_statement_starts_with_its_keywords_in_the_case_typed() {
    let snapshot = shop();
    let empty = suggest(&snapshot, "|").unwrap();
    assert!(labels(&empty, ItemKind::Keyword).contains(&"SELECT".to_owned()));
    assert!(labels(&empty, ItemKind::Keyword).contains(&"COMMENT ON".to_owned()), "PostgreSQL's own");
    let lower = suggest(&snapshot, "sel|").unwrap();
    assert!(labels(&lower, ItemKind::Keyword).contains(&"select".to_owned()));
    assert_eq!(lower.from, 0);
    let upper = suggest(&snapshot, "SELECT * FR|").unwrap();
    assert!(labels(&upper, ItemKind::Keyword).contains(&"FROM".to_owned()));
}

#[test]
fn from_offers_tables_views_schemas_and_other_schemas_qualified() {
    let snapshot = shop();
    let found = suggest(&snapshot, "SELECT * FROM |").unwrap();
    let tables = labels(&found, ItemKind::Table);
    assert!(tables.contains(&"customers".to_owned()) && tables.contains(&"orders".to_owned()), "{tables:?}");
    assert_eq!(labels(&found, ItemKind::View), vec!["active_customers".to_owned()]);
    assert!(labels(&found, ItemKind::Schema).contains(&"sales".to_owned()));
    assert_eq!(inserted(&found, "Leads"), vec!["sales.\"Leads\"".to_owned()]);
    assert!(labels(&found, ItemKind::Column).is_empty(), "no columns where a table goes");
    let qualified = suggest(&snapshot, "SELECT * FROM sales.|").unwrap();
    assert_eq!(labels(&qualified, ItemKind::Table), vec!["Leads".to_owned()]);
    assert_eq!(inserted(&qualified, "Leads"), vec!["\"Leads\"".to_owned()]);
    let typed = suggest(&snapshot, "SELECT * FROM public.cus|").unwrap();
    assert_eq!(typed.from, "SELECT * FROM public.".len());
    assert!(labels(&typed, ItemKind::Table).contains(&"customers".to_owned()));
    let quoted = suggest(&snapshot, "SELECT * FROM sales.\"Le|").unwrap();
    assert_eq!(quoted.from, "SELECT * FROM sales.".len(), "the quote is replaced too");
    assert_eq!(labels(&quoted, ItemKind::Table), vec!["Leads".to_owned()]);
}

#[test]
fn an_alias_gives_its_tables_columns_wherever_the_from_list_is() {
    let snapshot = shop();
    let found = suggest(&snapshot, "SELECT o.| FROM orders o JOIN customers c ON c.id = o.customer_id").unwrap();
    assert_eq!(labels(&found, ItemKind::Column), vec!["id", "customer_id", "total"]);
    assert_eq!(found.from, "SELECT o.".len());
    let typed = suggest(&snapshot, "SELECT c.na| FROM customers AS c").unwrap();
    assert_eq!(labels(&typed, ItemKind::Column), vec!["id", "name", "email"]);
    assert_eq!(typed.from, "SELECT c.".len());
    let table_name = suggest(&snapshot, "SELECT customers.| FROM customers").unwrap();
    assert_eq!(labels(&table_name, ItemKind::Column), vec!["id", "name", "email"]);
    let schema = suggest(&snapshot, "SELECT public.customers.| FROM public.customers").unwrap();
    assert_eq!(labels(&schema, ItemKind::Column), vec!["id", "name", "email"]);
}

#[test]
fn columns_of_every_table_in_scope_and_shared_names_qualified() {
    let snapshot = shop();
    let found = suggest(&snapshot, "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id WHERE |").unwrap();
    assert_eq!(inserted(&found, "total"), vec!["total".to_owned()]);
    assert_eq!(inserted(&found, "email"), vec!["email".to_owned()]);
    let ids = inserted(&found, "id");
    assert!(ids.contains(&"o.id".to_owned()) && ids.contains(&"c.id".to_owned()), "{ids:?}");
    assert!(labels(&found, ItemKind::Alias).contains(&"o".to_owned()));
    assert!(labels(&found, ItemKind::Function).contains(&"discount".to_owned()), "the snapshot's functions");
    assert!(labels(&found, ItemKind::Function).contains(&"coalesce".to_owned()), "built-in functions");
    let call = found.items.iter().find(|item| item.label == "coalesce").unwrap();
    assert_eq!((call.insert.as_str(), call.caret), ("coalesce()", Some("coalesce(".len())));
    let first = found.items.first().unwrap();
    assert_eq!(first.kind, ItemKind::Column, "columns come first");
    let select = suggest(&snapshot, "SELECT | FROM products").unwrap();
    assert_eq!(labels(&select, ItemKind::Column), vec!["id", "title", "price"], "the FROM list after the caret counts");
}

#[test]
fn ctes_subqueries_and_correlated_subqueries_resolve_their_columns() {
    let snapshot = shop();
    let cte =
        suggest(&snapshot, "WITH recent AS (SELECT id, total AS amount FROM orders) SELECT r.| FROM recent r").unwrap();
    assert_eq!(labels(&cte, ItemKind::Column), vec!["id", "amount"]);
    let listed = suggest(&snapshot, "WITH t(a, b) AS (SELECT 1, 2) SELECT t.| FROM t").unwrap();
    assert_eq!(labels(&listed, ItemKind::Column), vec!["a", "b"]);
    let star = suggest(&snapshot, "WITH c AS (SELECT * FROM customers) SELECT c.| FROM c").unwrap();
    assert_eq!(labels(&star, ItemKind::Column), vec!["id", "name", "email"]);
    let derived =
        suggest(&snapshot, "SELECT s.| FROM (SELECT c.name, count(*) AS n FROM customers c GROUP BY c.name) s")
            .unwrap();
    assert_eq!(labels(&derived, ItemKind::Column), vec!["name", "n"]);
    let correlated =
        suggest(&snapshot, "SELECT * FROM customers c WHERE EXISTS (SELECT 1 FROM orders o WHERE o.customer_id = c.|)")
            .unwrap();
    assert_eq!(labels(&correlated, ItemKind::Column), vec!["id", "name", "email"]);
    let inner = suggest(&snapshot, "SELECT * FROM customers c WHERE EXISTS (SELECT 1 FROM orders o WHERE |)").unwrap();
    let columns = labels(&inner, ItemKind::Column);
    assert!(columns.contains(&"total".to_owned()) && columns.contains(&"email".to_owned()), "{columns:?}");
    let in_from = suggest(&snapshot, "WITH recent AS (SELECT 1) SELECT * FROM rec|").unwrap();
    assert!(labels(&in_from, ItemKind::Table).contains(&"recent".to_owned()));
}

#[test]
fn joins_come_from_foreign_keys_both_ways() {
    let snapshot = shop();
    let found = suggest(&snapshot, "SELECT * FROM orders o JOIN |").unwrap();
    let joins = labels(&found, ItemKind::Join);
    assert!(joins.contains(&"customers c ON c.id = o.customer_id".to_owned()), "{joins:?}");
    assert!(joins.contains(&"order_items oi ON oi.order_id = o.id".to_owned()), "{joins:?}");
    let join = found.items.iter().find(|item| item.kind == ItemKind::Join).unwrap();
    assert_eq!(found.items.first().map(|item| item.kind), Some(ItemKind::Join), "joins first");
    assert!(join.filter == "customers" || join.filter == "order_items", "matched by the table's name");
    let taken = suggest(&snapshot, "SELECT * FROM order_items oi JOIN orders o ON o.id = oi.order_id JOIN |").unwrap();
    let joins = labels(&taken, ItemKind::Join);
    assert!(joins.contains(&"products p ON p.id = oi.product_id".to_owned()), "{joins:?}");
    assert!(joins.iter().any(|join| join.starts_with("order_items oi1 ON")), "a taken alias gets a number: {joins:?}");
    let on = suggest(&snapshot, "SELECT * FROM orders o JOIN customers c ON |").unwrap();
    assert_eq!(labels(&on, ItemKind::Join), vec!["c.id = o.customer_id".to_owned()]);
    assert!(labels(&on, ItemKind::Column).contains(&"email".to_owned()));
}

#[test]
fn insert_and_update_offer_their_tables_columns() {
    let snapshot = shop();
    let insert = suggest(&snapshot, "INSERT INTO customers (|").unwrap();
    assert_eq!(labels(&insert, ItemKind::Column), vec!["id", "name", "email"]);
    let more = suggest(&snapshot, "INSERT INTO public.customers (id, na|) VALUES (1, 'x')").unwrap();
    assert_eq!(labels(&more, ItemKind::Column), vec!["id", "name", "email"]);
    let update = suggest(&snapshot, "UPDATE customers SET |").unwrap();
    assert_eq!(labels(&update, ItemKind::Column), vec!["id", "name", "email"]);
    let target = suggest(&snapshot, "UPDATE |").unwrap();
    assert!(labels(&target, ItemKind::Table).contains(&"customers".to_owned()));
    let delete = suggest(&snapshot, "DELETE FROM orders WHERE |").unwrap();
    assert!(labels(&delete, ItemKind::Column).contains(&"total".to_owned()));
    let index = suggest(&snapshot, "CREATE INDEX i ON |").unwrap();
    assert!(labels(&index, ItemKind::Table).contains(&"orders".to_owned()));
    let indexed = suggest(&snapshot, "CREATE INDEX i ON orders (|)").unwrap();
    assert_eq!(labels(&indexed, ItemKind::Column), vec!["id", "customer_id", "total"]);
}

#[test]
fn nothing_inside_literals_comments_or_numbers() {
    let snapshot = shop();
    assert!(suggest(&snapshot, "SELECT 'abc|").is_none());
    assert!(suggest(&snapshot, "SELECT 'a|bc' FROM t").is_none());
    assert!(suggest(&snapshot, "-- sel|").is_none());
    assert!(suggest(&snapshot, "SELECT /* fr| */ 1").is_none());
    assert!(suggest(&snapshot, "SELECT 12|").is_none());
    assert!(suggest(&snapshot, "SELECT $$ bo|dy $$").is_none());
    assert!(suggest(&snapshot, "SELECT 'x' |").is_some(), "after a closed literal");
}

#[test]
fn broken_statements_still_complete() {
    let snapshot = shop();
    let unclosed = suggest(&snapshot, "SELECT o.| FROM orders o WHERE (o.total > (1").unwrap();
    assert_eq!(labels(&unclosed, ItemKind::Column), vec!["id", "customer_id", "total"]);
    let open_literal = suggest(&snapshot, "SELECT c.| FROM customers c WHERE c.name = 'x").unwrap();
    assert_eq!(labels(&open_literal, ItemKind::Column), vec!["id", "name", "email"]);
    let trailing_comma = suggest(&snapshot, "SELECT id, | FROM customers, ").unwrap();
    assert!(labels(&trailing_comma, ItemKind::Column).contains(&"email".to_owned()));
    let no_from_table = suggest(&snapshot, "SELECT * FROM WHERE |").unwrap();
    assert!(labels(&no_from_table, ItemKind::Column).is_empty());
    let after_alias = suggest(&snapshot, "SELECT * FROM orders wh|").unwrap();
    assert!(labels(&after_alias, ItemKind::Keyword).contains(&"where".to_owned()));
    let comment_inside = suggest(&snapshot, "SELECT o.| -- the total\nFROM /* main */ orders o").unwrap();
    assert_eq!(labels(&comment_inside, ItemKind::Column), vec!["id", "customer_id", "total"]);
}

#[test]
fn details_not_read_are_asked_for() {
    let snapshot = shop();
    let found = suggest(&snapshot, "SELECT e.| FROM big.events e").unwrap();
    assert!(labels(&found, ItemKind::Column).is_empty());
    assert_eq!(found.missing, vec![vec!["shop".to_owned(), "big".into(), "tables".into(), "events".into()]]);
}

#[test]
fn mysql_quotes_with_backticks_and_reads_its_comments() {
    let mut snapshot = shop();
    snapshot.catalogs[0].name = "def".into();
    let view = Catalogue::new(&snapshot, SqlDialect::MySql, None, Some("sales"));
    let (text, caret) = at_caret("SELECT l.| FROM Leads l # comment");
    let found = complete(&text, caret, &view).unwrap();
    assert_eq!(inserted(&found, "Full Name"), vec!["`Full Name`".to_owned()]);
    assert_eq!(view.default_schema(), Some("sales"));
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn resolved(snapshot: &Snapshot, text: &str) -> Option<Target> {
    let (text, offset) = at_caret(text);
    let view = Catalogue::new(snapshot, SqlDialect::Postgres, None, None);
    resolve(&text, offset, &view).map(|resolved| resolved.target)
}

#[test]
fn names_resolve_to_tables_columns_schemas_and_functions() {
    let snapshot = shop();
    let orders = path(&["shop", "public", "tables", "orders"]);
    assert_eq!(
        resolved(&snapshot, "SELECT o.to|tal FROM orders o"),
        Some(Target::Column { path: orders.clone(), column: "total".into() })
    );
    assert_eq!(resolved(&snapshot, "SELECT o.total FROM ord|ers o"), Some(Target::Object { path: orders.clone() }));
    assert_eq!(resolved(&snapshot, "SELECT o.total FROM orders |o"), Some(Target::Object { path: orders.clone() }));
    assert_eq!(resolved(&snapshot, "SELECT |o.total FROM orders o"), Some(Target::Object { path: orders.clone() }));
    assert_eq!(
        resolved(&snapshot, "SELECT * FROM pub|lic.customers"),
        Some(Target::Schema { path: path(&["shop", "public"]) })
    );
    assert_eq!(
        resolved(&snapshot, "SELECT na|me FROM customers"),
        Some(Target::Column { path: path(&["shop", "public", "tables", "customers"]), column: "name".into() })
    );
    assert_eq!(
        resolved(&snapshot, "SELECT disc|ount(total) FROM orders"),
        Some(Target::Object { path: path(&["shop", "public", "functions", "discount(total numeric)"]) })
    );
    assert_eq!(
        resolved(&snapshot, "SELECT public.customers.em|ail FROM public.customers"),
        Some(Target::Column { path: path(&["shop", "public", "tables", "customers"]), column: "email".into() })
    );
    assert_eq!(
        resolved(&snapshot, "INSERT INTO customers (id, em|ail) VALUES (1, 'a')"),
        Some(Target::Column { path: path(&["shop", "public", "tables", "customers"]), column: "email".into() })
    );
    assert_eq!(
        resolved(&snapshot, "DROP TABLE sales.\"Le|ads\""),
        Some(Target::Object { path: path(&["shop", "sales", "tables", "Leads"]) })
    );
    assert_eq!(resolved(&snapshot, "SELECT 1 FROM no|where"), None);
    assert_eq!(resolved(&snapshot, "WITH x AS (SELECT 1) SELECT * FROM |x"), None, "a CTE is no object");
}

fn unknowns(snapshot: &Snapshot, text: &str) -> Vec<(UnknownKind, String)> {
    let view = Catalogue::new(snapshot, SqlDialect::Postgres, None, None);
    check(text, &view, &[]).into_iter().map(|unknown| (unknown.kind, unknown.name)).collect()
}

#[test]
fn unknown_tables_and_columns_are_found_only_where_certain() {
    let snapshot = shop();
    assert_eq!(unknowns(&snapshot, "SELECT * FROM custmers"), vec![(UnknownKind::Table, "custmers".to_owned())]);
    assert_eq!(unknowns(&snapshot, "SELECT nme FROM customers"), vec![(UnknownKind::Column, "nme".to_owned())]);
    assert_eq!(unknowns(&snapshot, "SELECT c.nme FROM customers c"), vec![(UnknownKind::Column, "nme".to_owned())]);
    assert_eq!(unknowns(&snapshot, "SELECT * FROM sales.nothing"), vec![(UnknownKind::Table, "nothing".to_owned())]);
    let fine = [
        "SELECT * FROM big.nothing",
        "SELECT e.whatever FROM big.events e",
        "SELECT * FROM pg_class",
        "SELECT * FROM unknown_schema.t",
        "SELECT count(*) AS n, name label, 'x' tag FROM customers ORDER BY n",
        "WITH x AS (SELECT 1 AS a) SELECT a FROM x",
        "SELECT id::text, CASE WHEN id > 1 THEN 'a' ELSE 'b' END FROM customers WHERE name ILIKE 'a%' AND email IS NOT NULL",
        "UPDATE customers SET email = 'x' WHERE id = 1",
        "DELETE FROM orders WHERE customer_id IN (SELECT id FROM customers WHERE name = 'a')",
        "INSERT INTO orders (id, total) VALUES (1, 2) ON CONFLICT (id) DO UPDATE SET total = EXCLUDED.total",
        "SELECT * FROM \"Leads\"",
        "SELECT c FROM customers c",
        "SELECT name FROM customers ORDER BY name COLLATE \"C\" DESC NULLS LAST",
        "SELECT * FROM generate_series(1, 3) g WHERE g > 1",
        "SELECT s.anything FROM (SELECT * FROM customers) s",
        "SELECT date '2020-01-01', now(), current_date FROM customers",
        "SELECT * FROM orders o WHERE o.total > :limit AND id = $1",
        "DROP TABLE IF EXISTS gone",
        "CREATE TABLE fresh (a int)",
        "ALTER TABLE customers ADD COLUMN age int",
    ];
    for text in fine {
        assert_eq!(unknowns(&snapshot, text), Vec::new(), "{text}");
    }
    let view = Catalogue::new(&snapshot, SqlDialect::Postgres, None, None);
    assert!(check("INSERT INTO fresh VALUES (1)", &view, &["fresh".to_owned()]).is_empty(), "created in the script");
    assert_eq!(created("CREATE TABLE IF NOT EXISTS public.fresh (a int)", SqlDialect::Postgres), vec!["fresh"]);
}

#[test]
fn descriptions_list_columns_and_keys() {
    let snapshot = shop();
    let target = Target::Column { path: path(&["shop", "public", "tables", "orders"]), column: "total".into() };
    let described = describe(&snapshot, &target, 2).unwrap();
    assert_eq!(described.name, "orders");
    assert_eq!(described.columns.len(), 2);
    assert!(described.truncated);
    assert_eq!(described.primary_key, vec!["id".to_owned()]);
    assert_eq!(described.column.map(|column| column.type_name), Some("numeric".to_owned()));
}

fn redis_snapshot() -> Snapshot {
    let key = |path: &[&str], detail: &str| CachedNode {
        path: path.iter().map(|part| (*part).to_owned()).collect(),
        kind: NodeKind::Key,
        label: path.last().unwrap().to_string(),
        has_children: false,
        detail: Some(detail.into()),
        system: false,
        size: None,
        rows: None,
        comment: None,
    };
    let mut snapshot = Snapshot::new("kv", "redis");
    snapshot.keep(&["db0".to_owned()], vec![key(&["db0", "greeting"], "string")]);
    snapshot.keep(
        &["db0".to_owned(), "user:".to_owned()],
        vec![key(&["db0", "user:", "1"], "hash"), key(&["db0", "user:", "two words"], "hash")],
    );
    snapshot
}

#[test]
fn redis_completes_commands_and_known_keys() {
    let snapshot = redis_snapshot();
    let commands = complete_redis("hge", 3, &snapshot, 100).unwrap();
    assert!(commands.items.iter().any(|item| item.label == "hgetall" && item.kind == ItemKind::Command));
    let get = commands.items.iter().find(|item| item.label == "hget").unwrap();
    assert_eq!(get.detail.as_deref(), Some("key field"));
    let keys = complete_redis("HGETALL user:", 13, &snapshot, 100).unwrap();
    let found: Vec<&str> = keys.items.iter().map(|item| item.insert.as_str()).collect();
    assert_eq!(found, vec!["user:1", "\"user:two words\""]);
    assert_eq!(keys.from, "HGETALL ".len());
    let value = complete_redis("SET greeting ", 13, &snapshot, 100).unwrap();
    assert!(value.items.is_empty(), "a value is no key");
    let many = complete_redis("DEL greeting ", 13, &snapshot, 1).unwrap();
    assert_eq!(many.items.len(), 1, "keys at most the limit");
    assert!(complete_redis("GET \"gr", 7, &snapshot, 100).is_none());
}

#[test]
fn a_drivers_own_tree_reads_as_tables_with_the_columns_known() {
    let node = |path: &[&str], kind: NodeKind, detail: Option<&str>| CachedNode {
        path: path.iter().map(|part| (*part).to_owned()).collect(),
        kind,
        label: path.last().unwrap().to_string(),
        has_children: true,
        detail: detail.map(str::to_owned),
        system: false,
        size: None,
        rows: None,
        comment: None,
    };
    let mut snapshot = Snapshot::new("ch", "clickhouse");
    snapshot.keep(
        &["default".to_owned(), "tables".to_owned()],
        vec![node(&["default", "tables", "hits"], NodeKind::Table, Some("MergeTree"))],
    );
    snapshot.keep(
        &["default".to_owned(), "tables".to_owned(), "hits".to_owned()],
        vec![node(&["default", "tables", "hits", "url"], NodeKind::Column, Some("String"))],
    );
    let view = relational_view(&snapshot).unwrap();
    let catalogue = Catalogue::new(&view, SqlDialect::ClickHouse, None, None);
    let (text, caret) = at_caret("SELECT h.| FROM hits h");
    let found = complete(&text, caret, &catalogue).unwrap();
    assert_eq!(labels(&found, ItemKind::Column), vec!["url"]);
    let target = resolve("SELECT h.url FROM hits h", "SELECT h.url FROM hi".len(), &catalogue).unwrap().target;
    assert_eq!(own_tree_path(&snapshot, target.path()), Some(path(&["default", "tables", "hits"])));
}

fn used(snapshot: &Snapshot, text: &str, target: &Target) -> Vec<(String, Option<Access>)> {
    let view = Catalogue::new(snapshot, SqlDialect::Postgres, None, None);
    usages(text, &view, target).into_iter().map(|usage| (text[usage.range].to_owned(), usage.access)).collect()
}

#[test]
fn usages_of_a_table_follow_aliases_and_skip_other_schemas_and_ctes() {
    let snapshot = shop();
    let orders = Target::Object { path: path(&["shop", "public", "tables", "orders"]) };
    assert_eq!(
        used(&snapshot, "SELECT o.total FROM orders o JOIN public.orders p ON p.id = o.id", &orders),
        vec![("orders".into(), Some(Access::Read)), ("orders".into(), Some(Access::Read))]
    );
    assert_eq!(used(&snapshot, "WITH orders AS (SELECT 1) SELECT * FROM orders", &orders), Vec::new());
    assert_eq!(used(&snapshot, "SELECT * FROM sales.orders", &orders), Vec::new());
    assert_eq!(
        used(&snapshot, "UPDATE Orders SET total = 0 WHERE id IN (SELECT order_id FROM order_items)", &orders),
        vec![("Orders".into(), Some(Access::Write))]
    );
    assert_eq!(
        used(&snapshot, "INSERT INTO orders (id) SELECT id FROM orders", &orders),
        vec![("orders".into(), Some(Access::Write)), ("orders".into(), Some(Access::Read))]
    );
    assert_eq!(used(&snapshot, "ALTER TABLE orders ADD COLUMN x int", &orders), vec![("orders".into(), None)]);
    let leads = Target::Object { path: path(&["shop", "sales", "tables", "Leads"]) };
    assert_eq!(used(&snapshot, "SELECT * FROM sales.\"Leads\"", &leads), vec![("Leads".into(), Some(Access::Read))]);
}

#[test]
fn usages_of_a_column_know_reads_from_writes() {
    let snapshot = shop();
    let total = Target::Column { path: path(&["shop", "public", "tables", "orders"]), column: "total".into() };
    assert_eq!(
        used(&snapshot, "UPDATE orders SET total = total + 1 WHERE total > 0", &total),
        vec![
            ("total".into(), Some(Access::Write)),
            ("total".into(), Some(Access::Read)),
            ("total".into(), Some(Access::Read)),
        ]
    );
    assert_eq!(
        used(&snapshot, "INSERT INTO orders (id, total) VALUES (1, 2)", &total),
        vec![("total".into(), Some(Access::Write))]
    );
    assert_eq!(
        used(&snapshot, "SELECT c.name, o.total FROM customers c JOIN orders o ON o.customer_id = c.id", &total),
        vec![("total".into(), Some(Access::Read))]
    );
    assert_eq!(used(&snapshot, "SELECT total FROM products", &total), Vec::new(), "another table's column");
}

#[test]
fn mentions_look_into_routine_bodies_and_respect_qualifiers() {
    let body = "CREATE FUNCTION f() RETURNS void AS $fn$ BEGIN DELETE FROM public.orders; \
                UPDATE sales.orders SET x = 1; -- orders\n END $fn$ LANGUAGE plpgsql";
    let found: Vec<&str> =
        mentions(body, SqlDialect::Postgres, "orders", Some("public")).into_iter().map(|range| &body[range]).collect();
    assert_eq!(found, vec!["orders"]);
    let at = mentions(body, SqlDialect::Postgres, "orders", None);
    assert_eq!(at.len(), 2);
    assert!(at.iter().all(|range| &body[range.clone()] == "orders"));
}

#[test]
fn a_quoted_name_matches_in_its_own_case_only() {
    let snapshot = shop();
    let orders = path(&["shop", "public", "tables", "orders"]);
    assert_eq!(resolved(&snapshot, "SELECT * FROM ORD|ERS"), Some(Target::Object { path: orders }));
    assert_eq!(resolved(&snapshot, "SELECT * FROM \"ORD|ERS\""), None);
}

#[test]
fn a_reserved_word_is_never_a_generated_alias() {
    assert_eq!(unique_alias("as", &[]), "as1");
    assert_eq!(unique_alias("o", &["o".to_owned()]), "o1");
}
