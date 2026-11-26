use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::vec2;
use serde_json::json;
use zen_workbench::workspaces::{InnerTab, Workspaces};

use super::super::super::transaction_dialog::TransactionDialogRequest;
use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

fn write(root: &std::path::Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_migrations_tab_lists_a_folder_offline_and_down_asks_first_in_light_and_dark() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    write(&project, "go.mod", "module shop\nrequire github.com/golang-migrate/migrate/v4 v4.18.1\n");
    write(&project, "db/migrations/000001_users.up.sql", "CREATE TABLE users (id int);\n");
    write(&project, "db/migrations/000001_users.down.sql", "DROP TABLE users;\n");
    write(&project, "db/migrations/000002_orders.up.sql", "CREATE TABLE orders (id int);\n");
    write(&project, "db/migrations/000002_orders.down.sql", "DROP TABLE orders;\n");

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
            json!({ "name": "Shop", "url": "postgresql://ann@127.0.0.1:1/shop" }),
        )
        .unwrap();
    let connection = added["id"].as_str().unwrap().to_owned();
    let tab: InnerTab = serde_json::from_value(
        registry.dispatch(&mut session, "db.migration.open", json!({ "connection": connection })).unwrap(),
    )
    .unwrap();

    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1080.0, 420.0), 2.0);
    let deadline = Instant::now() + Duration::from_secs(20);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let mut image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, &registry, &mut session, &tab));
        while view.migrations.status["migrations"].as_array().is_none_or(Vec::is_empty) {
            assert!(Instant::now() < deadline, "the scan ended: {}", view.migrations.sets);
            std::thread::sleep(Duration::from_millis(20));
            view.migrations.key = None;
            image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, &registry, &mut session, &tab));
        }
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-migrations-{theme_name}.png")), image.image()).unwrap();
        }
    }
    let status = &view.migrations.status;
    assert_eq!(status["set"]["tool"], "golangMigrate");
    assert_eq!(status["connected"], false);
    let states: Vec<&str> =
        status["migrations"].as_array().unwrap().iter().map(|migration| migration["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["unknown", "unknown"]);
    assert!(view.migrations.summary().unwrap().contains("2 migrations"));

    view.migrations.act(&registry, &mut session, &tab, Action::Down);
    let asked = session.state_mut::<TransactionDialogRequest>().0.take();
    assert!(asked.is_none(), "a closed connection runs nothing and asks nothing");
    let error = view.migrations.error.clone().unwrap_or_default();
    assert!(error.contains("Connect"), "what would run needs the applied state: {error}");
    let state = registry.dispatch(&mut session, "db.connection.status", json!({})).unwrap();
    assert!(
        state["connections"].as_array().unwrap().iter().all(|known| known["state"] == "disconnected"),
        "the tab connects nowhere"
    );
}

#[test]
fn columns_fill_the_width_and_shrink_together_when_it_is_narrow() {
    let wide = columns(1000.0);
    assert_eq!(&wide[..3], &WIDTHS[..3]);
    assert!((wide.iter().sum::<f32>() - 1000.0).abs() < 0.01);
    let narrow = columns(360.0);
    assert!((narrow.iter().sum::<f32>() - 360.0).abs() < 0.01);
    assert!(narrow.iter().zip(WIDTHS).all(|(narrow, wide)| *narrow < wide));
}

#[test]
fn a_read_only_connection_turns_up_and_down_off_in_a_narrow_russian_tab_in_light_and_dark() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("ru"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    write(&project, "go.mod", "module shop\nrequire github.com/golang-migrate/migrate/v4 v4.18.1\n");
    for name in ["create_authentication_tables", "add_password_reset_tokens"] {
        for direction in ["up", "down"] {
            let version = if name.starts_with("create") { "20260101120000" } else { "20260102120000" };
            write(&project, &format!("auth-migrations/{version}_{name}.{direction}.sql"), "SELECT 1;\n");
        }
    }

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
            json!({ "name": "Shop", "url": "postgresql://ann@127.0.0.1:1/shop", "readOnly": true }),
        )
        .unwrap();
    let connection = added["id"].as_str().unwrap().to_owned();
    let tab: InnerTab = serde_json::from_value(
        registry.dispatch(&mut session, "db.migration.open", json!({ "connection": connection })).unwrap(),
    )
    .unwrap();

    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(460.0, 260.0), 2.0);
    let deadline = Instant::now() + Duration::from_secs(20);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let mut image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, &registry, &mut session, &tab));
        while view.migrations.status["migrations"].as_array().is_none_or(Vec::is_empty) {
            assert!(Instant::now() < deadline, "the scan ended: {}", view.migrations.sets);
            std::thread::sleep(Duration::from_millis(20));
            view.migrations.key = None;
            image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, &registry, &mut session, &tab));
        }
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-migrations-narrow-ru-{theme_name}.png")), image.image())
                .unwrap();
        }
    }
    assert_eq!(view.migrations.status["readOnly"], true, "{}", view.migrations.status);
    assert_eq!(view.migrations.status["migrations"].as_array().unwrap().len(), 2);

    view.migrations.act(&registry, &mut session, &tab, Action::Up);
    assert!(session.state_mut::<TransactionDialogRequest>().0.take().is_none(), "nothing asks");
    let refused = view.migrations.error.clone().unwrap_or_default();
    assert!(refused.contains("только для чтения"), "{refused}");
    zen_i18n::set_language(Some("en"));
}
