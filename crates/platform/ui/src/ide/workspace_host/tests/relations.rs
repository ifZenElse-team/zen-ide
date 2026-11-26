use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use serde_json::json;
use zen_db::schema::{Column, Constraint, ConstraintKind, Object, ObjectKind, Reference};

use super::{Window, at, button, click_at, key, shows, sql_window, until};

fn with_purchases(window: &Window, id: &str) {
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    let mut snapshot = zen_db::schema::read_snapshot(&file).unwrap();
    let column = |position: i32, name: &str, type_name: &str| Column {
        name: name.into(),
        position,
        type_name: type_name.into(),
        nullable: position == 3,
        ..Column::default()
    };
    let mut purchases = Object::new(ObjectKind::Table, "purchases");
    purchases.oid = zen_db::memory::PURCHASES;
    purchases.detailed = true;
    purchases.columns =
        vec![column(1, "id", "bigint"), column(2, "customer_id", "bigint"), column(3, "total", "numeric")];
    purchases.constraints = vec![
        Constraint {
            name: "purchases_pkey".into(),
            kind: ConstraintKind::PrimaryKey,
            columns: vec!["id".into()],
            definition: "PRIMARY KEY (id)".into(),
            references: None,
            index: None,
            comment: None,
        },
        Constraint {
            name: "purchases_customer_id_fkey".into(),
            kind: ConstraintKind::ForeignKey,
            columns: vec!["customer_id".into()],
            definition: "FOREIGN KEY (customer_id) REFERENCES customers(id)".into(),
            references: Some(Reference {
                schema: "public".into(),
                table: "customers".into(),
                columns: vec!["id".into()],
                on_update: "no action".into(),
                on_delete: "restrict".into(),
            }),
            index: None,
            comment: None,
        },
    ];
    snapshot.catalogs[0].schemas[0].objects.push(purchases);
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
}

fn png(window: &mut Window, dir: &Option<PathBuf>, name: &str, events: Vec<Vec<Event>>) {
    let Some(dir) = dir else { return };
    let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
    window.view.palette.apply(screen.context());
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    for step in events {
        screen.step(step, |ui| view.show(ui, registry, session));
    }
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    let frame = screen.render(|ui| view.show(ui, registry, session));
    crate::offscreen::write_png(&dir.join(format!("{name}.png")), frame.image()).unwrap();
}

fn object_state(window: &mut Window) -> serde_json::Value {
    window.run("db.object.state", json!({}))
}

fn purchases_shown(window: &mut Window, id: &str) -> Vec<(Pos2, String)> {
    window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "purchases"] }));
    window.run("db.object.readData", json!({}));
    until(window, "the purchases were read", |texts| shows(texts, "103.03") && shows(texts, "customer_id"))
}

fn customer_cell(texts: &[(Pos2, String)], total: &str) -> Pos2 {
    let row = at(texts, total);
    Pos2::new(at(texts, "customer_id").x + 4.0, row.y + 4.0)
}

#[test]
fn key_cells_link_to_their_rows_references_open_back_returns_and_a_key_is_chosen_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        with_purchases(&window, &id);
        purchases_shown(&mut window, &id);
        let relations = window.run("db.result.relations", json!({}));
        assert_eq!(relations["table"], "public.purchases", "{relations}");
        assert_eq!(relations["links"][0]["columns"], json!([1]), "{relations}");
        assert_eq!(relations["links"][0]["table"], "public.customers", "{relations}");
        let texts = window.frames(2);
        let cell = customer_cell(&texts, "103.03");
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)]);
        window.frame_after(0.05, vec![button(cell, PointerButton::Secondary, false)]);
        let texts = window.frame_after(0.05, Vec::new());
        let navigate = at(&texts, "Navigate");
        window.frame_after(0.05, vec![Event::PointerMoved(navigate + vec2(6.0, 6.0))]);
        let texts = window.frames(2);
        assert!(shows(&texts, "Open Referenced Row (public.customers)"), "{texts:?}");
        let edit = at(&texts, "Edit");
        window.frame_after(0.05, vec![Event::PointerMoved(edit + vec2(6.0, 6.0))]);
        let texts = window.frames(2);
        assert!(shows(&texts, "Choose Value…"), "{texts:?}");
        window.frame_after(0.05, vec![key(egui::Key::Escape, Modifiers::NONE)]);

        let texts = window.frames(1);
        let row = at(&texts, "103.03");
        let total_left = texts
            .iter()
            .filter(|(pos, text)| text == "123" && pos.y < row.y)
            .map(|(pos, _)| pos.x)
            .fold(f32::MIN, f32::max)
            - 6.0;
        let arrow = Pos2::new(total_left - 9.0, row.y + 6.0);
        window.frame_after(0.05, vec![Event::PointerMoved(arrow)]);
        window.frame_after(0.05, vec![Event::PointerMoved(arrow), button(arrow, PointerButton::Primary, true)]);
        window.frame_after(0.05, vec![button(arrow, PointerButton::Primary, false)]);
        let texts = until(&mut window, "the referenced customer", |texts| {
            shows(texts, "WHERE (\"id\" = 22)") && shows(texts, "Customer 22")
        });
        let state = object_state(&mut window);
        assert_eq!(state["name"], "customers", "{state}");
        assert!(
            driver
                .server
                .executed()
                .iter()
                .any(|sql| sql == "SELECT * FROM \"public\".\"customers\" WHERE (\"id\" = 22)"),
            "{:?}",
            driver.server.executed()
        );
        let navigation = window.run("db.data.navigation", json!({}));
        assert_eq!(navigation["back"], json!(["purchases"]));
        png(&mut window, &snapshots, &format!("2IDk654b-referenced-{name}"), Vec::new());

        let customers = window.run("db.result.relations", json!({}));
        let referencing: Vec<&str> =
            customers["references"].as_array().unwrap().iter().filter_map(|known| known["table"].as_str()).collect();
        assert!(referencing.contains(&"public.purchases"), "{customers}");
        let cell = at(&texts, "Customer 2") + vec2(4.0, 4.0);
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)]);
        window.frame_after(0.05, vec![button(cell, PointerButton::Secondary, false)]);
        let texts = window.frame_after(0.05, Vec::new());
        let navigate = at(&texts, "Navigate");
        window.frame_after(0.05, vec![Event::PointerMoved(navigate + vec2(6.0, 6.0))]);
        let texts = window.frames(2);
        let references = at(&texts, "References");
        png(
            &mut window,
            &snapshots,
            &format!("2IDk654b-menu-{name}"),
            vec![
                vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)],
                vec![button(cell, PointerButton::Secondary, false)],
                vec![Event::PointerMoved(navigate + vec2(6.0, 6.0))],
                Vec::new(),
                vec![Event::PointerMoved(references + vec2(6.0, 6.0))],
            ],
        );
        window.frame_after(0.05, vec![key(egui::Key::Escape, Modifiers::NONE)]);

        window.run("db.data.back", json!({}));
        let texts =
            until(&mut window, "back on purchases", |texts| shows(texts, "SELECT * FROM \"public\".\"purchases\""));
        let cell = customer_cell(&texts, "103.03");
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![key(egui::Key::Enter, Modifiers::SHIFT)]);
        until(&mut window, "the lookup", |texts| shows(texts, "Choose id from public.customers"));
        let texts =
            until(&mut window, "the lookup's rows", |texts| shows(texts, "25 rows") && shows(texts, "Customer 5"));
        let lookup = window.run("db.lookup.state", json!({}));
        assert_eq!((lookup["table"].as_str(), lookup["headers"][0].as_str()), (Some("public.customers"), Some("id")));
        let keys: Vec<&str> = lookup["rows"].as_array().unwrap().iter().filter_map(|row| row[0].as_str()).collect();
        assert_eq!(keys[..4], ["1", "2", "3", "4"], "in the key's order: {lookup}");
        assert!(
            driver
                .server
                .executed()
                .iter()
                .any(|sql| sql == "SELECT * FROM \"public\".\"customers\" ORDER BY \"id\" LIMIT 201"),
            "the lookup reads the referenced table: {:?}",
            driver.server.executed()
        );
        png(&mut window, &snapshots, &format!("2IDk654b-lookup-{name}"), Vec::new());
        let choice = texts.iter().rev().find(|(_, text)| text == "Customer 5").map(|(pos, _)| *pos).unwrap();
        click_at(&mut window, choice + vec2(4.0, 4.0));
        let edit = window.run("db.result.editState", json!({}));
        assert_eq!(edit["pending"]["cells"], 1, "{edit}");
        let page = window.run("db.result.page", json!({ "id": edit["id"], "offset": 2, "limit": 1 }));
        assert_eq!(page["page"][0][1]["value"], json!(5), "the key of the row chosen: {page}");
        assert_eq!(window.run("db.lookup.state", json!({}))["open"], false);
        window.run("db.result.revert", json!({}));
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}
