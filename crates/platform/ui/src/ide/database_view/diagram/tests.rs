use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use serde_json::json;
use zen_db::schema::{
    Catalog, Column, Constraint, ConstraintKind, Level, Object, ObjectKind, Reference, Schema, Snapshot,
};
use zen_workbench::workspaces::Workspaces;

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;
use crate::offscreen::Offscreen;

struct Setup {
    registry: Registry,
    session: Session,
    connection: String,
    dir: tempfile::TempDir,
}

fn column(name: &str, type_name: &str, nullable: bool) -> Column {
    Column { name: name.into(), type_name: type_name.into(), nullable, ..Column::default() }
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

fn shop(connection: &str) -> Snapshot {
    let customers = table(
        "customers",
        vec![column("id", "integer", false), column("name", "text", false), column("email", "text", true)],
        vec![key("customers_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let products = table(
        "products",
        vec![column("id", "integer", false), column("title", "text", false), column("price", "numeric(10,2)", false)],
        vec![key("products_pkey", ConstraintKind::PrimaryKey, &["id"], None)],
    );
    let orders = table(
        "orders",
        vec![
            column("id", "integer", false),
            column("customer_id", "integer", false),
            column("created_at", "timestamp with time zone", false),
        ],
        vec![
            key("orders_pkey", ConstraintKind::PrimaryKey, &["id"], None),
            key("orders_customer_id_fkey", ConstraintKind::ForeignKey, &["customer_id"], Some("customers")),
        ],
    );
    let items = table(
        "order_items",
        vec![
            column("order_id", "integer", false),
            column("product_id", "integer", false),
            column("quantity", "integer", false),
        ],
        vec![
            key("order_items_pkey", ConstraintKind::PrimaryKey, &["order_id", "product_id"], None),
            key("order_items_order_id_fkey", ConstraintKind::ForeignKey, &["order_id"], Some("orders")),
            key("order_items_product_id_fkey", ConstraintKind::ForeignKey, &["product_id"], Some("products")),
        ],
    );
    let mut view = Object::new(ObjectKind::View, "big_orders");
    view.detailed = true;
    view.columns = vec![column("id", "integer", true), column("total", "numeric", true)];
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "public".into(),
                level: Level::Details,
                objects: vec![customers, products, orders, items, view],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..Snapshot::new(connection, "postgresql")
    }
}

fn setup() -> Setup {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    zen_workbench::database::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let connection = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "dev", "auth": "none" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file = dir.path().join(format!("data/caches/databases/global/{connection}.snapshot"));
    zen_db::schema::write_snapshot(&file, &shop(&connection)).unwrap();
    Setup { registry, session, connection, dir }
}

fn open(setup: &mut Setup, path: Value) -> InnerTab {
    let opened = setup
        .registry
        .dispatch(&mut setup.session, "db.diagram.open", json!({ "connection": setup.connection, "path": path }))
        .unwrap();
    serde_json::from_value(opened["tab"].clone()).unwrap()
}

fn draw(screen: &mut Offscreen, view: &mut DatabaseView, setup: &mut Setup, tab: &InnerTab, events: Vec<Event>) {
    let palette = IdePalette::of(screen.context());
    let (registry, session) = (&setup.registry, &mut setup.session);
    screen.step(events, |ui| view.tab(ui, &palette, registry, session, tab));
}

fn laid_out(screen: &mut Offscreen, view: &mut DatabaseView, setup: &mut Setup, tab: &InnerTab) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while setup.session.state_mut::<Diagrams>().shown(&tab.id).is_none() {
        assert!(Instant::now() < deadline, "the diagram is laid out");
        draw(screen, view, setup, tab, Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
    draw(screen, view, setup, tab, Vec::new());
}

fn click(at: Pos2) -> Vec<Event> {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    };
    vec![Event::PointerMoved(at), press(true), press(false)]
}

fn state(setup: &mut Setup, tab: &InnerTab) -> Value {
    setup.registry.dispatch(&mut setup.session, "db.diagram.state", json!({ "tab": tab.id })).unwrap()
}

#[test]
fn the_schema_diagram_tab_draws_tables_and_relations_in_light_and_dark() {
    let mut setup = setup();
    let tab = open(&mut setup, json!(["shop", "public"]));
    assert_eq!(tab.kind, diagrams::KIND);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut view = DatabaseView::default();
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        let mut screen = Offscreen::new(vec2(1100.0, 560.0), 2.0);
        palette.apply(screen.context());
        laid_out(&mut screen, &mut view, &mut setup, &tab);
        let image = {
            let (registry, session) = (&setup.registry, &mut setup.session);
            screen.render(|ui| view.tab(ui, &palette, registry, session, &tab))
        };
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-diagram-{name}.png")), image.image()).unwrap();
        }
        let canvas = image[(2 * 1050, 2 * 540)];
        assert_eq!(canvas, palette.editor_background, "{name}: the canvas is the editor's background");
    }
    let status = setup.registry.dispatch(&mut setup.session, "db.connection.status", json!({})).unwrap();
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "drawing a diagram connects nowhere"
    );
}

#[test]
fn a_click_selects_an_entity_a_double_click_opens_it_and_the_bar_zooms() {
    let mut setup = setup();
    let tab = open(&mut setup, json!(["shop", "public"]));
    let mut view = DatabaseView::default();
    let mut screen = Offscreen::new(vec2(1100.0, 560.0), 1.0);
    IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light()).apply(screen.context());
    laid_out(&mut screen, &mut view, &mut setup, &tab);
    let answer = state(&mut setup, &tab);
    let orders =
        answer["entities"].as_array().unwrap().iter().find(|entity| entity["name"] == "orders").unwrap().clone();
    let bounds: Vec<f32> =
        orders["bounds"].as_array().unwrap().iter().map(|value| value.as_f64().unwrap() as f32).collect();
    let at = Pos2::new(bounds[0] + 24.0 + bounds[2] / 2.0, BAR + bounds[1] + 24.0 + 10.0);
    draw(&mut screen, &mut view, &mut setup, &tab, click(at));
    assert_eq!(state(&mut setup, &tab)["selected"], "orders");
    draw(&mut screen, &mut view, &mut setup, &tab, click(at));
    let active =
        setup.session.state::<Workspaces>().unwrap().active_tab(zen_workbench::database::WORKSPACE).cloned().unwrap();
    assert_eq!(active.id, format!("object:{}:shop/public/tables/orders", setup.connection), "a double click opens it");

    for _ in 0..30 {
        draw(&mut screen, &mut view, &mut setup, &tab, Vec::new());
    }
    let plus = Pos2::new(8.0 + 18.0 + 4.0 + 10.0, BAR / 2.0);
    draw(&mut screen, &mut view, &mut setup, &tab, click(plus));
    assert_eq!(state(&mut setup, &tab)["view"]["zoom"], 110, "the bar's + zooms in");
}

#[test]
fn a_tables_diagram_page_shows_its_neighbours() {
    let mut setup = setup();
    let tab = open(&mut setup, json!(["shop", "public", "tables", "orders"]));
    assert_eq!(tab.kind, zen_workbench::database::objects::KIND);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut view = DatabaseView::default();
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        let mut screen = Offscreen::new(vec2(900.0, 420.0), 2.0);
        palette.apply(screen.context());
        laid_out(&mut screen, &mut view, &mut setup, &tab);
        let image = {
            let (registry, session) = (&setup.registry, &mut setup.session);
            screen.render(|ui| view.tab(ui, &palette, registry, session, &tab))
        };
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-diagram-table-{name}.png")), image.image()).unwrap();
        }
    }
    let answer = state(&mut setup, &tab);
    assert_eq!(answer["total"], 3, "{answer}");
}

#[test]
fn the_theme_renderer_exports_png_and_svg_colours_from_the_palette() {
    let setup = setup();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    let renderer = ThemeRenderer(Arc::new(move || palette));
    let colors = renderer.colors();
    assert_eq!(colors.background, palette.editor_background.to_srgba_unmultiplied());
    assert_eq!(colors.text, palette.text.to_srgba_unmultiplied());
    let snapshot = shop("c");
    let diagram = zen_db::diagram::of_schema(&snapshot, "shop", "public").unwrap();
    let layout = zen_db::diagram::layout(
        &diagram,
        &zen_db::diagram::Metrics::default(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let file = setup.dir.path().join("shop.png");
    renderer.png(&diagram, &layout, 1.0, &file).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
        std::fs::copy(&file, dir.join("db-diagram-export-light.png")).unwrap();
        std::fs::write(dir.join("db-diagram-export-light.svg"), zen_db::diagram::svg(&diagram, &layout, &colors))
            .unwrap();
    }
}
