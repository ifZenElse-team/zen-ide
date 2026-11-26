use std::sync::atomic::AtomicBool;

use super::*;
use crate::schema::{Catalog, Column, Constraint, Level, Reference, Schema};

fn column(name: &str, type_name: &str, nullable: bool) -> Column {
    Column { name: name.into(), type_name: type_name.into(), nullable, ..Column::default() }
}

fn key(name: &str, kind: ConstraintKind, columns: &[&str], references: Option<(&str, &str, &[&str])>) -> Constraint {
    Constraint {
        name: name.into(),
        kind,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: String::new(),
        references: references.map(|(schema, table, columns)| Reference {
            schema: schema.into(),
            table: table.into(),
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            on_update: "no action".into(),
            on_delete: "no action".into(),
        }),
        index: None,
        comment: None,
    }
}

fn table(name: &str, columns: Vec<Column>, constraints: Vec<Constraint>) -> Object {
    let mut object = Object::new(ObjectKind::Table, name);
    object.detailed = true;
    object.columns = columns;
    object.constraints = constraints;
    object
}

fn shop() -> Snapshot {
    let customers = table(
        "customers",
        vec![column("id", "integer", false), column("name", "text", true)],
        vec![key("customers_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let products = table(
        "products",
        vec![column("id", "integer", false), column("title", "character varying(200)", false)],
        vec![key("products_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let orders = table(
        "orders",
        vec![column("id", "integer", false), column("customer_id", "integer", true)],
        vec![
            key("orders_pkey", ConstraintKind::PrimaryKey, &["id"], None),
            key(
                "orders_customer_id_fkey",
                ConstraintKind::ForeignKey,
                &["customer_id"],
                Some(("public", "customers", &["id"])),
            ),
        ],
    );
    let items = table(
        "order_items",
        vec![
            column("order_id", "integer", false),
            column("product_id", "integer", false),
            column("qty", "integer", false),
        ],
        vec![
            key("order_items_pkey", ConstraintKind::PrimaryKey, &["order_id", "product_id"], None),
            key(
                "order_items_order_id_fkey",
                ConstraintKind::ForeignKey,
                &["order_id"],
                Some(("public", "orders", &["id"])),
            ),
            key(
                "order_items_product_id_fkey",
                ConstraintKind::ForeignKey,
                &["product_id"],
                Some(("public", "products", &["id"])),
            ),
        ],
    );
    let audit = table(
        "audit",
        vec![column("id", "integer", false), column("customer", "integer", false)],
        vec![key(
            "audit_customer_fkey",
            ConstraintKind::ForeignKey,
            &["customer"],
            Some(("public", "customers", &["id"])),
        )],
    );
    let lonely = table("settings", vec![column("name", "text", false)], vec![]);
    let mut view = Object::new(ObjectKind::View, "big_orders");
    view.detailed = true;
    view.columns = vec![column("id", "integer", true)];
    let mut function = Object::new(ObjectKind::Function, "total");
    function.detailed = true;
    let public = Schema {
        name: "public".into(),
        level: Level::Details,
        objects: vec![customers, products, orders, items, lonely, view, function],
        ..Schema::default()
    };
    let history = Schema { name: "history".into(), level: Level::Details, objects: vec![audit], ..Schema::default() };
    let mut snapshot = Snapshot::new("shop", "postgresql");
    snapshot.catalogs =
        vec![Catalog { name: "shop".into(), current: true, schemas: vec![history, public], ..Catalog::default() }];
    snapshot
}

fn names(diagram: &ErDiagram) -> Vec<String> {
    diagram.entities.iter().map(Entity::label).collect()
}

#[test]
fn a_schema_diagram_has_its_tables_and_views_with_keys_and_relations() {
    let diagram = of_schema(&shop(), "shop", "public").unwrap();
    assert_eq!(names(&diagram), ["big_orders", "customers", "order_items", "orders", "products", "settings"]);
    let items = &diagram.entities[diagram.find("order_items").unwrap()];
    assert_eq!(items.path, ["shop", "public", "tables", "order_items"]);
    let flags: Vec<(bool, bool)> = items.columns.iter().map(|column| (column.primary, column.foreign)).collect();
    assert_eq!(flags, [(true, true), (true, true), (false, false)]);
    assert_eq!(diagram.entities[0].kind, "view");
    let relations: Vec<(String, String, bool)> = diagram
        .relations
        .iter()
        .map(|relation| {
            (
                diagram.entities[relation.from].name.clone(),
                diagram.entities[relation.to].name.clone(),
                relation.optional,
            )
        })
        .collect();
    assert_eq!(
        relations,
        [
            ("order_items".to_owned(), "orders".to_owned(), false),
            ("order_items".to_owned(), "products".to_owned(), false),
            ("orders".to_owned(), "customers".to_owned(), true),
        ]
    );
    assert!(!diagram.partial());
}

#[test]
fn a_table_diagram_has_its_neighbours_from_every_schema() {
    let snapshot = shop();
    let path: Vec<String> = ["shop", "public", "tables", "customers"].map(str::to_owned).to_vec();
    let diagram = of_table(&snapshot, &path).unwrap();
    assert_eq!(names(&diagram), ["customers", "orders", "history.audit"]);
    assert_eq!(diagram.relations.len(), 2, "{:?}", diagram.relations);
    let audit = of_table(&snapshot, &["shop", "history", "tables", "audit"].map(str::to_owned)).unwrap();
    assert_eq!(names(&audit), ["audit", "public.customers"]);
    assert!(of_table(&snapshot, &["shop", "public", "functions", "total"].map(str::to_owned)).is_none());
}

#[test]
fn the_layout_is_deterministic_without_overlaps_and_puts_referenced_tables_left() {
    let diagram = of_schema(&shop(), "shop", "public").unwrap();
    let metrics = Metrics::default();
    let first = layout(&diagram, &metrics, &AtomicBool::new(false)).unwrap();
    let again = layout(&diagram, &metrics, &AtomicBool::new(false)).unwrap();
    assert_eq!(first, again);
    for (at, a) in first.boxes.iter().enumerate() {
        for b in &first.boxes[at + 1..] {
            let apart = a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
            assert!(apart, "{a:?} overlaps {b:?}");
        }
        assert!(a.right() <= first.width + 0.5 && a.bottom() <= first.height + 0.5);
    }
    let x = |name: &str| first.boxes[diagram.find(name).unwrap()].x;
    assert!(x("customers") < x("orders") && x("orders") < x("order_items"), "{first:?}");
    assert!(x("products") < x("order_items"));
    let edge = &first.edges[0];
    assert_eq!(edge.first().unwrap()[0], first.boxes[diagram.relations[0].from].x, "starts on the referencing box");
    assert!(layout(&diagram, &metrics, &AtomicBool::new(true)).is_none(), "cancelled");
}

#[test]
fn a_long_table_shows_a_row_for_the_columns_left_out() {
    let columns = (0..60).map(|at| column(&format!("c{at}"), "int", true)).collect();
    let diagram =
        ErDiagram { entities: vec![entity("db", "s", &table("wide", columns, vec![]), false)], relations: vec![] };
    let laid = layout(&diagram, &Metrics::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(laid.rows, [39]);
    let colors = Colors::default();
    assert!(svg(&diagram, &laid, &colors).contains(">+21</text>"));
}

#[test]
fn a_big_schema_lays_out_quickly_enough_and_in_order() {
    let mut objects = Vec::new();
    for at in 0..3000 {
        let parent = format!("t{}", at / 3);
        let constraints = if at > 0 {
            vec![key(&format!("fk{at}"), ConstraintKind::ForeignKey, &["p"], Some(("s", &parent, &["id"])))]
        } else {
            Vec::new()
        };
        objects.push(table(&format!("t{at}"), vec![column("id", "int", false), column("p", "int", true)], constraints));
    }
    let schema = Schema { name: "s".into(), level: Level::Details, objects, ..Schema::default() };
    let mut snapshot = Snapshot::new("c", "postgresql");
    snapshot.catalogs = vec![Catalog { name: "db".into(), schemas: vec![schema], ..Catalog::default() }];
    let diagram = of_schema(&snapshot, "db", "s").unwrap();
    assert_eq!(diagram.relations.len(), 2999);
    let laid = layout(&diagram, &Metrics::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(laid.boxes.len(), 3000);
    assert!(laid.width > 0.0 && laid.height > 0.0);
}

#[test]
fn svg_holds_every_entity_and_relation_in_the_given_colours() {
    let diagram = of_schema(&shop(), "shop", "public").unwrap();
    let laid = layout(&diagram, &Metrics::default(), &AtomicBool::new(false)).unwrap();
    let colors = Colors { text: [1, 2, 3, 255], line: [10, 20, 30, 255], ..Colors::default() };
    let text = svg(&diagram, &laid, &colors);
    assert!(text.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(text.contains("stroke=\"#0a141e\""));
    assert!(text.contains("fill=\"#010203\">order_items</text>"));
    assert_eq!(text.matches("<polyline").count(), diagram.relations.len());
    assert!(text.contains("character varying(200)"));
    assert!(text.trim_end().ends_with("</svg>"));
}

#[test]
fn mermaid_names_entities_columns_keys_and_cardinalities() {
    let diagram = of_schema(&shop(), "shop", "public").unwrap();
    let text = mermaid(&diagram);
    assert!(text.starts_with("erDiagram\n"));
    assert!(text.contains("    customers {\n        integer id PK\n        text name\n    }\n"), "{text}");
    assert!(text.contains("        character_varying(200) title\n"), "{text}");
    assert!(text.contains("        integer order_id PK, FK\n"), "{text}");
    assert!(text.contains("    orders }o--o| customers : \"orders_customer_id_fkey\"\n"), "{text}");
    assert!(text.contains("    order_items }o--|| orders : \"order_items_order_id_fkey\"\n"), "{text}");
    let table_diagram = of_table(&shop(), &["shop", "public", "tables", "customers"].map(str::to_owned)).unwrap();
    assert!(mermaid(&table_diagram).contains("    history_audit[\"history.audit\"] {\n"));
}
