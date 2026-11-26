use std::path::PathBuf;
use std::sync::Arc;

use egui::vec2;
use serde_json::json;
use zen_core::{Registry, Session};
use zen_db::schema::{
    Catalog, Column, Constraint, ConstraintKind, Index, Level, Object, ObjectKind, Reference, Schema, Snapshot,
};
use zen_workbench::workspaces::Workspaces;

use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    id: String,
}

fn table(name: &str, size: u64, columns: &[(&str, &str)]) -> Object {
    let mut object = Object::new(ObjectKind::Table, name);
    object.detailed = true;
    object.size = Some(size);
    object.rows = Some(1000);
    object.columns = columns
        .iter()
        .enumerate()
        .map(|(at, (name, type_name))| Column {
            name: (*name).into(),
            position: at as i32 + 1,
            type_name: (*type_name).into(),
            nullable: at > 0,
            ..Column::default()
        })
        .collect();
    object
}

fn snapshot(id: &str) -> Snapshot {
    let mut orders = table("orders", 4 << 20, &[("id", "bigint"), ("customer", "integer"), ("total", "numeric(10,2)")]);
    orders.comment = Some("Placed orders".into());
    orders.constraints = vec![Constraint {
        name: "orders_customer_fkey".into(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["customer".into()],
        definition: "FOREIGN KEY (customer) REFERENCES customers(id)".into(),
        references: Some(Reference { schema: "public".into(), table: "customers".into(), ..Reference::default() }),
        index: None,
        comment: None,
    }];
    orders.indexes =
        vec![Index { name: "orders_pkey".into(), columns: vec!["id".into()], primary: true, ..Index::default() }];
    let customers = table("customers", 1 << 20, &[("id", "integer"), ("name", "text")]);
    let products = table("products", 256 << 10, &[("id", "integer")]);
    let long = table("customer_loyalty_programme_memberships_archive", 72 << 10, &[("id", "integer")]);
    let mut view = Object::new(ObjectKind::View, "big_orders");
    view.detailed = true;
    let mut function = Object::new(ObjectKind::Function, "order_count");
    function.routine = Some(zen_db::schema::Routine {
        arguments: "c integer".into(),
        result: Some("bigint".into()),
        ..Default::default()
    });
    Snapshot {
        taken: 1,
        catalogs: vec![
            Catalog {
                name: "shop".into(),
                current: true,
                schemas_read: true,
                schemas: vec![
                    Schema {
                        name: "public".into(),
                        level: Level::Details,
                        objects: vec![customers, long, orders, products, view, function],
                        ..Schema::default()
                    },
                    Schema { name: "audit".into(), ..Schema::default() },
                ],
                extensions: vec![zen_db::schema::Extension {
                    name: "pgcrypto".into(),
                    version: "1.3".into(),
                    ..Default::default()
                }],
                roles: vec![zen_db::schema::Role { name: "app_reader".into(), login: true, ..Default::default() }],
                ..Catalog::default()
            },
            Catalog { name: "postgres".into(), ..Catalog::default() },
        ],
        ..Snapshot::new(id, "postgresql")
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
    let added = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "folder": "Work", "type": "prod", "readOnly": true }),
        )
        .unwrap();
    let id = added["id"].as_str().unwrap().to_owned();
    registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Staging", "url": "postgresql://ann@staging:5432/app", "type": "test" }),
        )
        .unwrap();
    let file = dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot(&id)).unwrap();
    let read = registry.dispatch(&mut session, "db.schema.children", json!({ "connection": id, "path": [] })).unwrap();
    assert_eq!(read["total"], 2, "db.schema.children waits for the snapshot file: {read}");
    for path in [
        json!([]),
        json!(["shop"]),
        json!(["shop", "public"]),
        json!(["shop", "public", "tables"]),
        json!(["shop", "public", "tables", "orders"]),
        json!(["shop", "public", "tables", "orders", "columns"]),
    ] {
        registry.dispatch(&mut session, "db.navigator.select", json!({ "connection": id, "path": path })).unwrap();
    }
    registry
        .dispatch(
            &mut session,
            "db.navigator.expand",
            json!({ "connection": id, "path": ["shop", "public", "tables", "orders", "columns"] }),
        )
        .unwrap();
    for folder in ["#extensions", "#roles"] {
        registry
            .dispatch(&mut session, "db.navigator.expand", json!({ "connection": id, "path": ["shop", folder] }))
            .unwrap();
    }
    registry
        .dispatch(
            &mut session,
            "db.navigator.select",
            json!({ "connection": id, "path": ["shop", "public", "tables", "orders"] }),
        )
        .unwrap();
    registry.dispatch(&mut session, "db.schema.children", json!({ "connection": id, "path": [] })).unwrap();
    Setup { registry, session, _dir: dir, id }
}

fn labels(panel: &NavigatorPanel) -> Vec<String> {
    panel.rows.iter().map(|row| format!("{}{}", "  ".repeat(row.depth), row.label)).collect()
}

#[test]
fn the_navigator_in_light_and_dark_with_a_postgresql_connection_expanded() {
    let mut s = setup();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(340.0, 560.0), 2.0);
    let mut panel = NavigatorPanel::default();
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let frame = |screen: &mut crate::offscreen::Offscreen, panel: &mut NavigatorPanel, s: &mut Setup| {
            screen.render(|ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                panel.show(ui, &palette, &s.registry, &mut s.session);
            })
        };
        frame(&mut screen, &mut panel, &mut s);
        let image = frame(&mut screen, &mut panel, &mut s);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-navigator-{theme_name}.png")), image.image()).unwrap();
        }
    }
    assert_eq!(
        labels(&panel),
        [
            "Work",
            "  Shop",
            "    shop",
            "      public",
            "        Tables",
            "          customers",
            "          customer_loyalty_programme_memberships_archive",
            "          orders",
            "            Columns",
            "              id",
            "              customer",
            "              total",
            "            Constraints",
            "            Foreign Keys",
            "            Indexes",
            "            Triggers",
            "            References",
            "            Policies",
            "            Partitions",
            "          products",
            "        Foreign Tables",
            "        Views",
            "        Materialized Views",
            "        Indexes",
            "        Functions",
            "        Sequences",
            "        Aggregate Functions",
            "      audit",
            "      Extensions",
            "        pgcrypto",
            "      Event Triggers",
            "      Tablespaces",
            "      Roles",
            "        app_reader",
            "    postgres",
            "Staging",
        ]
    );
    let orders = panel.rows.iter().find(|row| row.label == "orders").unwrap();
    assert_eq!((orders.selected, orders.size_ratio), (true, Some(1.0)));
    let shop = panel.rows.iter().find(|row| row.label == "Shop").unwrap();
    assert_eq!(
        (shop.read_only, shop.detail.as_deref(), shop.color.as_deref()),
        (true, Some("db.example.com:5432"), Some("red"))
    );
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let tree = screen.accessibility_tree(|ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let names: Vec<String> = tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
    for wanted in ["Database Navigator", "Shop", "orders", "Columns", "Filter connections and objects"] {
        assert!(names.iter().any(|name| name == wanted), "{wanted} in {names:?}");
    }
    let status = s.registry.dispatch(&mut s.session, "db.connection.status", json!({ "connection": s.id })).unwrap();
    assert_eq!(status["connections"][0]["state"], "disconnected", "drawing connects nowhere");
}

#[test]
fn keys_move_the_selection_type_to_filter_and_open_a_table_in_its_object_tab() {
    let mut s = setup();
    let mut screen = crate::offscreen::Offscreen::new(vec2(340.0, 560.0), 1.0);
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    palette.apply(screen.context());
    let mut panel = NavigatorPanel::default();
    let key =
        |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    screen.render(|ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let tree = panel.tree.unwrap();
    screen.context().memory_mut(|memory| memory.request_focus(tree));
    screen.step(Vec::new(), |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    screen.step(vec![key(Key::ArrowDown)], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    screen.step(Vec::new(), |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    assert_eq!(panel.rows.iter().find(|row| row.selected).map(|row| row.label.as_str()), Some("Columns"));
    screen.step(vec![key(Key::ArrowUp)], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    screen.step(vec![key(Key::Enter)], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let state = s.registry.dispatch(&mut s.session, "workspace.host.state", json!({ "id": "database" })).unwrap();
    let opened = format!("object:{}:shop/public/tables/orders", s.id);
    assert_eq!(
        state["active"],
        opened.as_str(),
        "↩ on a table opens its object tab: {state} {:?} {:?}",
        panel.error,
        panel.rows.iter().find(|row| row.selected).map(|row| row.label.clone())
    );
    let object = s.registry.dispatch(&mut s.session, "db.object.state", json!({})).unwrap();
    assert_eq!((object["page"].as_str(), object["kind"].as_str()), (Some("properties"), Some("table")));
    assert_eq!(object["pages"], json!(["properties", "data", "ddl", "diagram"]), "Data is the next page");
    screen.context().memory_mut(|memory| memory.request_focus(tree));
    screen.step(vec![Event::Text("tot".into())], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    screen.step(Vec::new(), |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    assert_eq!(panel.filter, "tot");
    assert!(labels(&panel).iter().any(|label| label.trim() == "total"), "{:?}", labels(&panel));
    assert!(!labels(&panel).iter().any(|label| label.trim() == "customers"), "{:?}", labels(&panel));
}

fn press(at: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE }
}

fn bounds_of(tree: &egui::accesskit::TreeUpdate, label: &str, scale: f32) -> Option<Rect> {
    tree.nodes.iter().find(|(_, node)| node.label() == Some(label)).and_then(|(_, node)| node.bounds()).map(|bounds| {
        Rect::from_min_max(
            pos2(bounds.x0 as f32 / scale, bounds.y0 as f32 / scale),
            pos2(bounds.x1 as f32 / scale, bounds.y1 as f32 / scale),
        )
    })
}

#[test]
fn the_menus_open_a_client_in_the_terminal_and_generate_code_in_light_and_dark() {
    let mut s = setup();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        for (row_label, submenu, shot) in
            [("Shop", None, "db-client-menu"), ("orders", Some("Generate Code"), "db-generate-code-menu")]
        {
            let mut screen = crate::offscreen::Offscreen::new(vec2(560.0, 560.0), 2.0);
            palette.apply(screen.context());
            let mut panel = NavigatorPanel::default();
            let (registry, session) = (&s.registry, &mut s.session);
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                panel.show(ui, &palette, registry, session);
            };
            screen.render(&mut draw);
            drop(draw);
            let at = panel.rows.iter().position(|row| row.label == row_label).unwrap();
            let cell = pos2(80.0, TOOLBAR + FILTER + metrics::TREE_ROW * (at as f32 + 0.5));
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                panel.show(ui, &palette, registry, session);
            };
            screen.step(vec![Event::PointerMoved(cell), press(cell, egui::PointerButton::Secondary, true)], &mut draw);
            screen.step(vec![press(cell, egui::PointerButton::Secondary, false)], &mut draw);
            screen.step(Vec::new(), &mut draw);
            let tree = screen.accessibility_tree(&mut draw);
            let labels: Vec<&str> = tree.nodes.iter().filter_map(|(_, node)| node.label()).collect();
            match submenu {
                None => assert!(
                    labels.contains(&"Open in Terminal") && labels.contains(&"Copy Client Command"),
                    "{labels:?}"
                ),
                Some(submenu) => {
                    let found = bounds_of(&tree, submenu, 1.0).unwrap_or_else(|| panic!("{labels:?}"));
                    let scale = if found.max.x > 560.0 { 2.0 } else { 1.0 };
                    let found = bounds_of(&tree, submenu, scale).unwrap();
                    screen.step(vec![Event::PointerMoved(found.center())], &mut draw);
                    screen.step(Vec::new(), &mut draw);
                    screen.step(Vec::new(), &mut draw);
                    let tree = screen.accessibility_tree(&mut draw);
                    let labels: Vec<&str> = tree.nodes.iter().filter_map(|(_, node)| node.label()).collect();
                    for wanted in ["Go Struct", "Rust Struct", "TypeScript Interface"] {
                        assert!(labels.contains(&wanted), "{wanted}: {labels:?}");
                    }
                }
            }
            let image = screen.render(&mut draw);
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("{shot}-{theme_name}.png")), image.image()).unwrap();
            }
        }
    }
    let path = json!(["shop", "public", "tables", "orders"]);
    let code = s
        .registry
        .dispatch(
            &mut s.session,
            "db.object.generateCode",
            json!({ "connection": s.id, "path": path, "language": "go" }),
        )
        .unwrap();
    assert_eq!(code["typeName"], "Order", "{code}");
}

#[test]
fn option_f7_on_a_table_or_a_column_finds_its_usages() {
    let mut s = setup();
    let mut screen = crate::offscreen::Offscreen::new(vec2(340.0, 560.0), 1.0);
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    palette.apply(screen.context());
    let mut panel = NavigatorPanel::default();
    let find = Event::Key { key: Key::F7, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::ALT };
    screen.render(|ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let tree = panel.tree.unwrap();
    screen.context().memory_mut(|memory| memory.request_focus(tree));
    screen.step(vec![find.clone()], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let results = s.registry.dispatch(&mut s.session, "search.results.tree", json!({})).unwrap();
    assert_eq!((results["kind"].as_str(), results["title"].as_str()), (Some("database"), Some("Usages of orders")));

    let total = json!({ "connection": s.id, "path": ["shop", "public", "tables", "orders", "columns", "total"] });
    s.registry.dispatch(&mut s.session, "db.navigator.select", total).unwrap();
    screen.step(Vec::new(), |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    screen.context().memory_mut(|memory| memory.request_focus(tree));
    screen.step(vec![find], |ui| panel.show(ui, &palette, &s.registry, &mut s.session));
    let results = s.registry.dispatch(&mut s.session, "search.results.tree", json!({})).unwrap();
    assert_eq!(results["title"], "Usages of orders.total", "{:?}", panel.error);
    let status = s.registry.dispatch(&mut s.session, "db.connection.status", json!({ "connection": s.id })).unwrap();
    assert_eq!(status["connections"][0]["state"], "disconnected", "Find Usages connects nowhere");
}

fn labels_of(panel: &NavigatorPanel) -> Vec<String> {
    labels(panel)
}

#[test]
fn sizes_read_as_short_binary_units() {
    assert_eq!(
        [0, 512, 72 << 10, (3 << 20) / 2, 4 << 20, 5 << 30].map(short_size),
        ["0B", "512B", "72K", "1.5M", "4M", "5G"]
    );
}

fn draw_frames(
    screen: &mut crate::offscreen::Offscreen,
    panel: &mut NavigatorPanel,
    s: &mut Setup,
    p: &IdePalette,
    events: Vec<Event>,
) -> egui::accesskit::TreeUpdate {
    screen.step(events, |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0, p.island);
        panel.show(ui, p, &s.registry, &mut s.session);
    });
    screen.step(Vec::new(), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0, p.island);
        panel.show(ui, p, &s.registry, &mut s.session);
    });
    screen.accessibility_tree(|ui| {
        ui.painter().rect_filled(ui.max_rect(), 0, p.island);
        panel.show(ui, p, &s.registry, &mut s.session);
    })
}

#[test]
fn a_narrow_navigator_ends_long_names_with_an_ellipsis_and_gives_up_the_size_column_first_in_both_languages() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for language in ["en", "ru"] {
        let mut s = setup();
        zen_i18n::set_language(Some(language));
        s.registry
            .dispatch(
                &mut s.session,
                "db.navigator.collapse",
                json!({ "connection": s.id, "path": ["shop", "public", "tables", "orders"] }),
            )
            .unwrap();
        for (theme_name, theme) in
            [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
        {
            let palette = IdePalette::from_theme(&theme);
            let mut screen = crate::offscreen::Offscreen::new(vec2(MIN_WIDTH, 420.0), 2.0);
            palette.apply(screen.context());
            let mut panel = NavigatorPanel::default();
            draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
            let image = screen.render(|ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                panel.show(ui, &palette, &s.registry, &mut s.session);
            });
            if let Some(dir) = &snapshots {
                let name = format!("db-navigator-narrow-{language}-{theme_name}.png");
                crate::offscreen::write_png(&dir.join(name), image.image()).unwrap();
            }
            let long = panel.rows.iter().find(|row| row.label.starts_with("customer_loyalty")).unwrap();
            assert_eq!((long.size, long.size_ratio.is_some()), (Some(72 << 10), true));
            let galley = screen.context().fonts_mut(|fonts| {
                fonts.layout_job(crate::ide::vcs::truncated(&long.label, theme::regular(13.0), palette.text, 80.0))
            });
            let last = galley.rows[0].glyphs.last().map(|glyph| glyph.chr);
            assert!(galley.size().x <= 80.0 && last == Some('…'), "{last:?}");
        }
        zen_i18n::set_language(Some("en"));
    }
}

#[test]
fn delete_and_f2_ask_first_and_name_what_is_lost() {
    let mut s = setup();
    s.registry.dispatch(&mut s.session, "db.dataSource.update", json!({ "id": s.id, "readOnly": false })).unwrap();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let key = |key: Key, modifiers: Modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        let mut screen = crate::offscreen::Offscreen::new(vec2(560.0, 560.0), 2.0);
        palette.apply(screen.context());
        let mut panel = NavigatorPanel::default();
        draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
        let tree = panel.tree.unwrap();
        screen.context().memory_mut(|memory| memory.request_focus(tree));
        let found = draw_frames(&mut screen, &mut panel, &mut s, &palette, vec![key(Key::Delete, Modifiers::NONE)]);
        let labels: Vec<&str> = found.nodes.iter().filter_map(|(_, node)| node.label()).collect();
        let Some(Dialog::Delete(deletion)) = &panel.dialog else { panic!("{:?} {:?}", panel.dialog, panel.error) };
        assert_eq!(deletion.title, "Delete orders?");
        assert!(deletion.loss.starts_with("This deletes table public.orders."), "{}", deletion.loss);
        assert!(
            deletion.loss.contains("about 1,000 rows, 4 MB") || deletion.loss.contains("about 1000 rows, 4 MB"),
            "{}",
            deletion.loss
        );
        assert_eq!(deletion.statements, ["DROP TABLE \"public\".\"orders\""]);
        assert!(deletion.can_cascade && labels.contains(&"Delete"), "{labels:?}");
        let image = screen.render(|ui| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
            panel.show(ui, &palette, &s.registry, &mut s.session);
        });
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-navigator-delete-{theme_name}.png")), image.image())
                .unwrap();
        }
        draw_frames(&mut screen, &mut panel, &mut s, &palette, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(panel.dialog.is_none(), "⎋ cancels");
        screen.context().memory_mut(|memory| memory.request_focus(tree));
        draw_frames(&mut screen, &mut panel, &mut s, &palette, vec![key(Key::F2, Modifiers::NONE)]);
        let Some(Dialog::Rename { name, .. }) = &panel.dialog else { panic!("{:?}", panel.dialog) };
        assert_eq!(name, "orders");
        panel.dialog = None;
    }
    let status = s.registry.dispatch(&mut s.session, "db.connection.status", json!({ "connection": s.id })).unwrap();
    assert_eq!(status["connections"][0]["state"], "disconnected", "asking connects nowhere");
}

#[test]
fn enter_connects_a_connection_and_selecting_from_outside_scrolls_to_the_row() {
    let mut s = setup();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let mut screen = crate::offscreen::Offscreen::new(vec2(340.0, 160.0), 1.0);
    palette.apply(screen.context());
    let mut panel = NavigatorPanel::default();
    draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
    let path = json!(["shop", "public", "tables", "orders", "columns", "total"]);
    s.registry.dispatch(&mut s.session, "db.navigator.select", json!({ "connection": s.id, "path": path })).unwrap();
    let tree = draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
    let labels: Vec<&str> = tree.nodes.iter().filter_map(|(_, node)| node.label()).collect();
    let total = bounds_of(&tree, "total", 1.0)
        .unwrap_or_else(|| panic!("the selected row is drawn: {labels:?} {:?}", labels_of(&panel)));
    assert!(total.top() >= TOOLBAR + FILTER - 1.0 && total.bottom() <= 161.0, "scrolled into view: {total:?}");
    let staging = panel.rows.iter().find(|row| row.label == "Staging").unwrap().key.clone();
    s.registry.dispatch(&mut s.session, "db.navigator.select", row_params(&staging)).unwrap();
    draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
    screen.context().memory_mut(|memory| memory.request_focus(panel.tree.unwrap()));
    let enter =
        Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    draw_frames(&mut screen, &mut panel, &mut s, &palette, vec![enter]);
    let asked = s.session.state_mut::<super::super::connection_dialog::ConnectionDialogRequest>().0.take();
    assert!(asked.is_some(), "connecting Staging asks for its password: {:?}", panel.error);
}

#[test]
fn database_level_folders_show_extensions_and_roles_with_their_own_icons() {
    let mut s = setup();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let tables = json!({ "connection": s.id, "path": ["shop", "public", "tables"] });
    s.registry.dispatch(&mut s.session, "db.navigator.collapse", tables).unwrap();
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        let mut screen = crate::offscreen::Offscreen::new(vec2(340.0, 560.0), 2.0);
        palette.apply(screen.context());
        let mut panel = NavigatorPanel::default();
        draw_frames(&mut screen, &mut panel, &mut s, &palette, Vec::new());
        let image = screen.render(|ui| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
            panel.show(ui, &palette, &s.registry, &mut s.session);
        });
        if let Some(dir) = &snapshots {
            let name = format!("db-navigator-postgres-objects-{theme_name}.png");
            crate::offscreen::write_png(&dir.join(name), image.image()).unwrap();
        }
        let role = panel.rows.iter().find(|row| row.label == "app_reader").unwrap();
        assert!(leaf_item(role) && openable(role) && !deletable(role) && icon(&role.kind).is_some());
        let extension = panel.rows.iter().find(|row| row.label == "pgcrypto").unwrap();
        assert_eq!((extension.kind.as_str(), extension.detail.as_deref()), ("extension", Some("1.3")));
    }
    s.registry
        .dispatch(
            &mut s.session,
            "db.object.open",
            json!({ "connection": s.id, "path": ["shop", "#roles", "app_reader"] }),
        )
        .unwrap();
    let state = s.registry.dispatch(&mut s.session, "db.object.state", json!({})).unwrap();
    assert_eq!((state["kind"].as_str(), state["pages"].clone()), (Some("role"), json!(["properties"])));
}

struct SeenBy(Arc<std::sync::Mutex<Vec<bool>>>);

impl zen_core::Command for SeenBy {
    type Params = Value;
    type Output = ();
    const ID: &'static str = "test.seen.by";
    const TITLE: &'static str = "Seen By";
    const DESCRIPTION: &'static str = "Remembers who ran it.";

    fn run(&self, session: &mut Session, _: Value) -> Result<(), zen_core::CommandError> {
        self.0.lock().unwrap().push(zen_core::macros::by_agent(session));
        Ok(())
    }
}

#[test]
fn the_keys_that_answer_the_delete_dialog_run_it_as_whoever_pressed_them() {
    let mut s = setup();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    s.registry.register(SeenBy(Arc::clone(&seen))).unwrap();
    let press = |key: Key| {
        [true, false].map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    };
    for agent in [false, true] {
        for tabs in 1..=6 {
            let mut screen = crate::offscreen::Offscreen::new(vec2(640.0, 560.0), 1.0);
            let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
            palette.apply(screen.context());
            let mut panel = NavigatorPanel::default();
            let key = RowKey { connection: Some(s.id.clone()), folder: None, path: vec!["shop".into()] };
            panel.dialog = Some(Dialog::Delete(Deletion {
                command: "test.seen.by",
                key,
                title: "Truncate".to_owned(),
                loss: "Everything".to_owned(),
                statements: Vec::new(),
                can_cascade: false,
                cascade: false,
                token: None,
                error: None,
            }));
            let mut steps = vec![Vec::new(), Vec::new()];
            steps.extend((0..tabs).map(|_| press(Key::Tab).to_vec()));
            steps.push(press(Key::Enter).to_vec());
            steps.push(Vec::new());
            for keys in steps {
                let (registry, session) = (&s.registry, &mut s.session);
                screen.step(keys, |ui| {
                    if agent {
                        zen_core::macros::as_agent(session, |session| panel.show(ui, &palette, registry, session));
                    } else {
                        panel.show(ui, &palette, registry, session);
                    }
                });
            }
            if !seen.lock().unwrap().is_empty() {
                break;
            }
        }
        assert_eq!(seen.lock().unwrap().pop(), Some(agent), "the danger button is reachable by keys");
    }
}
