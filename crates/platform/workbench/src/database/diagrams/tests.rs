use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::diagram::{Colors, ErDiagram, Layout};
use zen_db::schema::{
    Catalog, Column, Constraint, ConstraintKind, Level, Object, ObjectKind, Reference, Schema, Snapshot,
};

use super::{Diagrams, Renderer};
use crate::workspaces::Workspaces;

struct Window {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
    id: String,
}

struct FakeRenderer;

impl Renderer for FakeRenderer {
    fn colors(&self) -> Colors {
        Colors { text: [17, 34, 51, 255], ..Colors::default() }
    }

    fn png(&self, diagram: &ErDiagram, layout: &Layout, scale: f32, path: &Path) -> Result<(), String> {
        let text = format!("{} {} {scale}", diagram.entities.len(), layout.boxes.len());
        std::fs::write(path, text).map_err(|error| error.to_string())
    }
}

fn column(name: &str, nullable: bool) -> Column {
    Column { name: name.into(), type_name: "integer".into(), nullable, ..Column::default() }
}

fn key(name: &str, kind: ConstraintKind, columns: &[&str], target: Option<&str>) -> Constraint {
    Constraint {
        name: name.into(),
        kind,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: String::new(),
        references: target.map(|table| Reference {
            schema: "public".into(),
            table: table.into(),
            columns: vec!["id".into()],
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

fn snapshot(connection: &str) -> Snapshot {
    let customers = table(
        "customers",
        vec![column("id", false), column("name", true)],
        vec![key("customers_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let products = table(
        "products",
        vec![column("id", false)],
        vec![key("products_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let orders = table(
        "orders",
        vec![column("id", false), column("customer_id", true)],
        vec![
            key("orders_pkey", ConstraintKind::PrimaryKey, &["id"], None),
            key("orders_customer_id_fkey", ConstraintKind::ForeignKey, &["customer_id"], Some("customers")),
        ],
    );
    let items = table(
        "order_items",
        vec![column("order_id", false), column("product_id", false)],
        vec![
            key("order_items_order_id_fkey", ConstraintKind::ForeignKey, &["order_id"], Some("orders")),
            key("order_items_product_id_fkey", ConstraintKind::ForeignKey, &["product_id"], Some("products")),
        ],
    );
    let mut count = Object::new(ObjectKind::Function, "order_count");
    count.detailed = true;
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "public".into(),
                level: Level::Details,
                objects: vec![customers, products, orders, items, count],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..Snapshot::new(connection, "postgresql")
    }
}

impl Window {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        super::super::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut session = Session::new();
        super::super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
        session.set_state(super::super::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::postgres::Postgres)];
        session.state_mut::<super::super::Database>().set_drivers(drivers);
        let mut window = Self { registry, session, dir, id: String::new() };
        let added = window.run("db.dataSource.add", json!({ "name": "Shop", "url": "postgresql://ann@db.local/shop" }));
        window.id = added["id"].as_str().unwrap().to_owned();
        let file = window.dir.path().join(format!("data/caches/databases/global/{}.snapshot", window.id));
        zen_db::schema::write_snapshot(&file, &snapshot(&window.id)).unwrap();
        window
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn refused(&mut self, command: &str, params: Value) -> String {
        match self.registry.dispatch(&mut self.session, command, params) {
            Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
            other => panic!("not refused: {other:?}"),
        }
    }

    fn schema_diagram(&mut self) -> String {
        let opened = self.run("db.diagram.open", json!({ "connection": self.id, "path": ["shop", "public"] }));
        opened["tab"]["id"].as_str().unwrap().to_owned()
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn view_diagram_opens_a_schema_tab_laid_out_in_the_background() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    assert_eq!(tab, format!("diagram:{}:shop/public", window.id));
    let state = window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 5000 }));
    assert_eq!(state["status"], "ready", "{state}");
    assert_eq!(names(&state["entities"]), ["customers", "order_items", "orders", "products"]);
    assert_eq!((state["total"].as_u64(), state["relationTotal"].as_u64()), (Some(4), Some(3)));
    assert_eq!(state["truncated"], false);
    let customers = &state["entities"][0];
    assert_eq!(customers["path"], json!(["shop", "public", "tables", "customers"]));
    assert_eq!(
        customers["columns"][0],
        json!({ "name": "id", "type": "integer", "keys": ["primary"], "nullable": false })
    );
    assert_eq!(customers["bounds"].as_array().unwrap().len(), 4);
    assert_eq!(
        state["relations"][2],
        json!({
            "name": "orders_customer_id_fkey",
            "from": "orders",
            "fromColumns": ["customer_id"],
            "to": "customers",
            "toColumns": ["id"],
            "cardinality": "many-to-one",
            "optional": true
        })
    );
    assert_eq!(state["view"], json!({ "zoom": 100, "x": -24, "y": -24 }));

    let small = window.run("db.diagram.state", json!({ "limit": 1, "offset": 1, "columns": false }));
    assert_eq!(names(&small["entities"]), ["order_items"]);
    assert!(small["entities"][0].get("columns").is_none());
    assert_eq!((small["truncated"].as_bool(), small["relations"].as_array().unwrap().len()), (Some(true), 1));

    let again = window.schema_diagram();
    assert_eq!(again, tab, "the same schema comes to front");
    let tabs = window.session.state::<Workspaces>().unwrap().tabs(super::WORKSPACE).len();
    assert_eq!(tabs, 2, "Overview and the diagram");
    let refused = window.refused("db.diagram.open", json!({ "connection": window.id, "path": ["shop", "nowhere"] }));
    assert_eq!(refused, "unknownObject");
}

#[test]
fn a_table_diagram_is_its_object_tabs_diagram_page() {
    let mut window = Window::new();
    let opened = window
        .run("db.diagram.open", json!({ "connection": window.id, "path": ["shop", "public", "tables", "orders"] }));
    assert_eq!(opened["of"], "table");
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    assert_eq!(tab, format!("object:{}:shop/public/tables/orders", window.id));
    assert_eq!(window.run("db.object.state", json!({ "tab": tab }))["page"], "diagram");
    let state = window.run("db.diagram.state", json!({ "waitMs": 5000 }));
    assert_eq!(names(&state["entities"]), ["customers", "order_items", "orders"]);
    assert_eq!(state["relationTotal"], 2);
    let function = window.run(
        "db.object.open",
        json!({ "connection": window.id, "path": ["shop", "public", "functions", "order_count"] }),
    );
    let refused = window.refused("db.diagram.state", json!({ "tab": function["tab"]["id"] }));
    assert_eq!(refused, "noDiagram");
}

#[test]
fn zoom_and_scroll_are_commands_kept_with_the_tab() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    window.run("db.diagram.state", json!({ "waitMs": 5000 }));
    let zoomed = window.run("db.diagram.zoom", json!({ "action": "in", "width": 1000, "height": 600 }));
    assert_eq!(zoomed["zoom"], 110);
    assert_eq!(zoomed["x"], 21, "the centre stays: -24 + 500 - 500 / 1.1");
    assert_eq!(window.run("db.diagram.zoom", json!({ "action": "out" }))["zoom"], 100);
    assert_eq!(window.run("db.diagram.zoom", json!({ "action": "out" }))["zoom"], 90);
    assert_eq!(window.run("db.diagram.zoom", json!({ "action": "reset" }))["zoom"], 100);
    let fitted = window.run("db.diagram.zoom", json!({ "action": "fit", "width": 300, "height": 200 }));
    assert!(fitted["zoom"].as_u64().unwrap() < 100, "{fitted}");
    let view = window.run("db.diagram.view", json!({ "zoom": 1000, "x": 5, "y": 6, "dx": 10, "dy": -2 }));
    assert_eq!(view, json!({ "zoom": 400, "x": 15, "y": 4 }));
    let saved = crate::workspaces::saved(&mut window.session);
    let kept = serde_json::to_value(&saved).unwrap().to_string();
    assert!(kept.contains("\"zoom\":400"), "{kept}");
    assert_eq!(super::view_of(&window.session, &tab).map(|view| view.zoom), Some(400));

    let object = window
        .run("db.diagram.open", json!({ "connection": window.id, "path": ["shop", "public", "tables", "orders"] }));
    let object = object["tab"]["id"].as_str().unwrap().to_owned();
    window.run("db.diagram.view", json!({ "tab": object, "dx": 100 }));
    assert_eq!(super::view_of(&window.session, &object).map(|view| view.x), Some(76));
    assert_eq!(super::view_of(&window.session, &tab).map(|view| view.x), Some(15), "each tab has its own view");
}

#[test]
fn selecting_an_entity_marks_it_and_opens_its_object_tab_on_request() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    let selected = window.run("db.diagram.select", json!({ "entity": "orders" }));
    assert_eq!(selected["path"], json!(["shop", "public", "tables", "orders"]));
    assert!(selected.get("opened").is_none());
    assert_eq!(window.run("db.diagram.state", json!({ "tab": tab }))["selected"], "orders");
    assert_eq!(window.refused("db.diagram.select", json!({ "tab": tab, "entity": "nothing" })), "unknownEntity");
    let opened = window.run("db.diagram.select", json!({ "tab": tab, "entity": "public.customers", "open": true }));
    assert_eq!(opened["opened"]["id"], format!("object:{}:shop/public/tables/customers", window.id));
    let cleared = window.run("db.diagram.select", json!({ "tab": tab }));
    assert!(cleared.get("entity").is_none());
}

#[test]
fn export_answers_mermaid_and_writes_svg_and_png_with_the_theme() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    window.run("db.diagram.state", json!({ "waitMs": 5000 }));
    let mermaid = window.run("db.diagram.export", json!({ "format": "mermaid" }));
    let text = mermaid["text"].as_str().unwrap();
    assert!(text.starts_with("erDiagram\n"), "{text}");
    assert!(text.contains("orders }o--o| customers : \"orders_customer_id_fkey\""), "{text}");
    let cut = window.run("db.diagram.export", json!({ "format": "mermaid", "limit": 10 }));
    assert_eq!((cut["text"].as_str(), cut["truncated"].as_bool()), (Some("erDiagram\n"), Some(true)));
    assert_eq!(window.refused("db.diagram.export", json!({ "format": "svg" })), "noRenderer");

    window.session.state_mut::<Diagrams>().set_renderer(Arc::new(FakeRenderer));
    let svg = window.run("db.diagram.export", json!({ "format": "svg" }));
    assert!(svg["text"].as_str().unwrap().contains("fill=\"#112233\">customers</text>"), "{svg}");
    assert_eq!(window.refused("db.diagram.export", json!({ "format": "png" })), "pathNeeded");
    assert_eq!(window.refused("db.diagram.export", json!({ "format": "png", "path": "shop.png" })), "invalidPath");

    let file = window.dir.path().join("shop.svg");
    let started = window.run("db.diagram.export", json!({ "format": "svg", "path": file }));
    assert!(started.get("text").is_none());
    let state = window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 5000, "limit": 0 }));
    assert_eq!(state["export"]["status"], "done", "{state}");
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("<svg"));
    assert_eq!(window.refused("db.diagram.export", json!({ "format": "svg", "path": file })), "exists");

    let png = window.dir.path().join("shop.png");
    window.run("db.diagram.export", json!({ "format": "png", "path": png, "scale": 3 }));
    let state = window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 5000, "limit": 0 }));
    assert_eq!((state["export"]["status"].as_str(), state["export"]["format"].as_str()), (Some("done"), Some("png")));
    assert_eq!(std::fs::read_to_string(&png).unwrap(), "4 4 3");
    window.run("db.diagram.export", json!({ "format": "mermaid", "path": png, "overwrite": true }));
    window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 5000, "limit": 0 }));
    assert!(std::fs::read_to_string(&png).unwrap().starts_with("erDiagram"));
    let parts: Vec<String> = std::fs::read_dir(window.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".part"))
        .collect();
    assert!(parts.is_empty(), "{parts:?}");
}

#[test]
fn an_agent_exports_no_diagram_over_or_into_a_file_hidden_from_it() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    window.run("db.diagram.state", json!({ "waitMs": 5000 }));
    let project = window.dir.path().join("project");
    std::fs::write(project.join(".env"), "TOKEN=hunter2\n").unwrap();
    std::fs::create_dir_all(project.join("secrets")).unwrap();
    window
        .session
        .state_mut::<crate::hidden_files::HiddenFromAgents>()
        .set(&[".env".to_owned(), "secrets/**".to_owned()]);
    for path in [project.join(".env"), project.join("secrets/shop.mmd")] {
        let params = json!({ "tab": tab, "format": "mermaid", "path": path, "overwrite": true });
        let refused = zen_core::macros::as_agent(&mut window.session, |session| {
            window.registry.dispatch(session, "db.diagram.export", params)
        });
        assert!(refused.is_err(), "{}: {refused:?}", path.display());
    }
    window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 1000, "limit": 0 }));
    assert_eq!(std::fs::read_to_string(project.join(".env")).unwrap(), "TOKEN=hunter2\n", "left as it was");
    assert!(!project.join("secrets/shop.mmd").exists());
    let mine = project.join("secrets/shop.mmd");
    window.run("db.diagram.export", json!({ "tab": tab, "format": "mermaid", "path": mine }));
    window.run("db.diagram.state", json!({ "tab": tab, "waitMs": 5000, "limit": 0 }));
    assert!(std::fs::read_to_string(&mine).unwrap().starts_with("erDiagram"));
}

#[test]
fn cancel_answers_and_open_lays_the_diagram_out_again() {
    let mut window = Window::new();
    let tab = window.schema_diagram();
    window.run("db.diagram.state", json!({ "waitMs": 5000 }));
    let cancelled = window.run("db.diagram.cancel", json!({ "tab": tab }));
    assert_eq!(cancelled["cancelled"], false, "nothing was running");
    window.schema_diagram();
    let state = window.run("db.diagram.state", json!({ "waitMs": 5000, "limit": 0 }));
    assert_eq!(state["status"], "ready");
    assert!(state["size"][0].as_f64().unwrap() > 0.0);
}
