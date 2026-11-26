use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use zen_core::{Registry, Session};
use zen_secrets::SecretStore as _;
use zen_workbench::workspaces::Workspaces;

use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    secrets: Arc<zen_secrets::MemoryStore>,
    driver: Arc<zen_db::memory::MemoryDriver>,
    dir: tempfile::TempDir,
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
    let secrets = Arc::new(zen_secrets::MemoryStore::new());
    session.set_state(zen_workbench::database::Secrets(secrets.clone()));
    let driver = Arc::new(zen_db::memory::MemoryDriver::new(false));
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> =
        vec![Arc::new(zen_db::postgres::Postgres), Arc::clone(&driver) as Arc<dyn zen_db::driver::Driver>];
    session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    Setup { registry, session, secrets, driver, dir }
}

fn wait_test(setup: &mut Setup, dialog: &mut ConnectionDialog) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while dialog.poll_test(&setup.registry, &mut setup.session) {
        assert!(std::time::Instant::now() < deadline, "the test ends");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_pasted_url_fills_every_field_and_ok_saves_the_password_only_in_the_store() {
    let mut s = setup();
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    assert_eq!(dialog.current().unwrap().id, "postgresql", "PostgreSQL first");
    assert_eq!((dialog.port.as_str(), dialog.database.as_str()), ("5432", "postgres"));
    assert!(dialog.show_all_databases && dialog.save_password, "on by default");
    dialog.by_url = true;
    dialog.url =
        "postgresql://ann:s3cret@db.example.com:6543/shop?sslmode=require&pooler=true&target_session_attrs=any"
            .to_owned();
    dialog.fill_from_url();
    assert_eq!(
        (dialog.host.as_str(), dialog.port.as_str(), dialog.database.as_str(), dialog.user.as_str()),
        ("db.example.com", "6543", "shop", "ann")
    );
    assert_eq!((dialog.password.as_str(), dialog.tls_mode.as_str()), ("s3cret", "require"));
    assert!(!dialog.url.contains("s3cret"), "{}", dialog.url);
    assert_eq!(dialog.settings["pooler"], json!(true));
    assert_eq!(dialog.properties, vec![("target_session_attrs".to_owned(), "any".to_owned())]);
    dialog.name = "Shop".to_owned();
    dialog.connection_type = "test".to_owned();
    dialog.isolation = Some("serializable".to_owned());
    dialog.bootstrap = "SET work_mem = '64MB'\n\n".to_owned();
    let (command, params) = dialog.save_call();
    assert_eq!(command, "db.dataSource.add");
    let saved = s.registry.dispatch(&mut s.session, command, params).unwrap();
    assert!(dialog.connect_after, "Connect after saving is on by default");
    assert_eq!(
        dialog.connect_call(&saved),
        Some(json!({ "connection": saved["id"], "password": "s3cret" })),
        "OK connects the new connection with the password typed"
    );
    dialog.connect_after = false;
    assert_eq!(dialog.connect_call(&saved), None);
    assert_eq!(saved["hasPassword"], true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let params = json!({ "operation": saved["secretOperation"] });
        let state = s.registry.dispatch(&mut s.session, "db.operation.state", params).unwrap();
        if state["status"] != "running" || std::time::Instant::now() > deadline {
            assert_eq!(state["status"], "done", "{state}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!((saved["type"].as_str(), saved["port"].as_u64()), (Some("test"), Some(6543)));
    assert_eq!(saved["bootstrap"]["initStatements"], json!(["SET work_mem = '64MB'"]));
    assert_eq!(saved["bootstrap"]["isolation"], "serializable");
    let file = std::fs::read_to_string(s.dir.path().join("data/databases/data-sources.json")).unwrap();
    assert!(!file.contains("s3cret") && file.contains("\"passwordSaved\": true"), "{file}");
    let id = saved["id"].as_str().unwrap().to_owned();
    let item = zen_secrets::database::id(None, &id, zen_secrets::database::Kind::Password).unwrap();
    assert_eq!(s.secrets.get(&item).unwrap().unwrap().expose(), "s3cret");

    let edit = ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "id": id, "page": "general" })).unwrap();
    assert_eq!(edit.save_call().0, "db.dataSource.update", "Edit reopens the same dialog");
    assert_eq!((edit.name.as_str(), edit.host.as_str(), edit.page), ("Shop", "db.example.com", Page::General));
    assert!(edit.has_password && edit.password.is_empty(), "the password is never read back");
    assert_eq!(edit.properties, vec![("target_session_attrs".to_owned(), "any".to_owned())]);
    let (_, update) = edit.save_call();
    assert!(update.get("password").is_none(), "an untouched password stays as saved");
}

#[test]
fn a_key_value_driver_shows_its_own_fields_and_test_connection_reports_the_server() {
    let mut s = setup();
    s.driver.server.require_password("kv");
    let mut dialog =
        ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "driver": "memory-kv", "host": "cache" }))
            .unwrap();
    assert_eq!(dialog.current().unwrap().id, "memory-kv");
    assert!(!dialog.show_all_databases, "no catalogues");
    assert_eq!(
        (dialog.settings["mode"].clone(), dialog.settings["separator"].clone()),
        (json!("standalone"), json!(":"))
    );
    dialog.password = "kv".to_owned();
    dialog.start_test(&s.registry, &mut s.session);
    assert!(matches!(dialog.test, Some(Test::Running(_))));
    wait_test(&mut s, &mut dialog);
    let Some(Test::Passed(text)) = &dialog.test else { panic!("{:?}", dialog.test) };
    assert!(text.contains("Memory 1.0") && text.contains("round trip"), "{text}");
    assert_eq!(s.registry.dispatch(&mut s.session, "db.dataSource.list", json!({})).unwrap()["total"], 0);
    assert!(s.secrets.ids().is_empty(), "testing saves nothing");
    dialog.password = "nope-xyz".to_owned();
    dialog.start_test(&s.registry, &mut s.session);
    wait_test(&mut s, &mut dialog);
    let Some(Test::Failed(text)) = &dialog.test else { panic!("{:?}", dialog.test) };
    assert!(text.contains("user name or password") && !text.contains("nope-xyz"), "{text}");
}

#[test]
fn the_dialog_in_light_and_dark_for_postgresql_and_a_key_value_driver() {
    let mut s = setup();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(960.0, 640.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        for (driver, pages) in [
            (
                "postgresql",
                &[
                    ("main", Page::Main),
                    ("ssh", Page::SshSsl),
                    ("advanced", Page::Advanced),
                    ("general", Page::General),
                ][..],
            ),
            ("memory-kv", &[("main", Page::Main), ("advanced", Page::Advanced)][..]),
        ] {
            let mut dialog =
                ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "driver": driver, "name": "Orders" }))
                    .unwrap();
            for (page_name, page) in pages.iter().filter(|_| snapshots.is_some()) {
                dialog.page = *page;
                let mut frame = |screen: &mut crate::offscreen::Offscreen| {
                    screen.render(|ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                        let _ = show(ui, &palette, &mut dialog);
                    })
                };
                frame(&mut screen);
                let image = frame(&mut screen);
                if let Some(dir) = &snapshots {
                    let file = dir.join(format!("db-connection-{driver}-{page_name}-{theme_name}.png"));
                    crate::offscreen::write_png(&file, image.image()).unwrap();
                    if *page == Page::SshSsl {
                        let wheel = vec![
                            egui::Event::PointerMoved(egui::pos2(640.0, 400.0)),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: vec2(0.0, -400.0),
                                modifiers: egui::Modifiers::NONE,
                                phase: egui::TouchPhase::Move,
                                source: egui::MouseWheelSource::Wheel,
                            },
                        ];
                        let mut draw = |ui: &mut Ui| {
                            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                            let _ = show(ui, &palette, &mut dialog);
                        };
                        screen.step(wheel, &mut draw);
                        for _ in 0..20 {
                            screen.step(Vec::new(), &mut draw);
                        }
                        let scrolled = screen.render(&mut draw);
                        let file = dir.join(format!("db-connection-{driver}-ssh-scrolled-{theme_name}.png"));
                        crate::offscreen::write_png(&file, scrolled.image()).unwrap();
                    }
                    for (variant, size, language) in
                        [("narrow", vec2(860.0, 600.0), "en"), ("ru", vec2(960.0, 640.0), "ru")]
                    {
                        zen_i18n::set_language(Some(language));
                        let mut other = crate::offscreen::Offscreen::new(size, 2.0);
                        palette.apply(other.context());
                        let mut draw = |ui: &mut Ui| {
                            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                            let _ = show(ui, &palette, &mut dialog);
                        };
                        other.render(&mut draw);
                        let frame = other.render(&mut draw);
                        let file = dir.join(format!("db-connection-{driver}-{page_name}-{theme_name}-{variant}.png"));
                        crate::offscreen::write_png(&file, frame.image()).unwrap();
                        zen_i18n::set_language(Some("en"));
                    }
                }
            }
            dialog.page = Page::Main;
            screen.render(|ui| {
                let _ = show(ui, &palette, &mut dialog);
            });
            let tree = screen.accessibility_tree(|ui| {
                let _ = show(ui, &palette, &mut dialog);
            });
            let names: Vec<String> =
                tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
            for wanted in ["Host:", "User:", "Password:", "Save password", "Test Connection", "OK", "Main", "General"] {
                assert!(names.iter().any(|name| name == wanted), "{wanted} in {names:?}");
            }
        }
    }
    let _ = s.secrets.accesses();
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_connection_saved_by_the_dialog_connects_after_a_restart_with_its_saved_password() {
    let port: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(54318);
    let mut s = setup();
    let url = format!("postgresql://postgres:zen@127.0.0.1:{port}/postgres?sslmode=disable");
    let mut dialog =
        ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "url": url, "name": "PG 18" })).unwrap();
    assert!(dialog.save_password);
    dialog.start_test(&s.registry, &mut s.session);
    wait_test(&mut s, &mut dialog);
    let Some(Test::Passed(text)) = &dialog.test else { panic!("{:?}", dialog.test) };
    assert!(text.contains("PostgreSQL 18"), "{text}");
    let (command, params) = dialog.save_call();
    let saved = s.registry.dispatch(&mut s.session, command, params).unwrap();
    let id = saved["id"].as_str().unwrap().to_owned();
    assert_eq!(saved["hasPassword"], true);

    let mut restarted = Session::new();
    let project = s.dir.path().join("project");
    s.registry.dispatch(&mut restarted, "file.project.open", json!({ "path": project })).unwrap();
    restarted.state_mut::<Workspaces>().set_data_dir(s.dir.path().join("data"));
    zen_workbench::database::install(&mut restarted);
    restarted.set_state(zen_workbench::database::Secrets(s.secrets.clone()));
    let got = s.registry.dispatch(&mut restarted, "db.dataSource.get", json!({ "id": id })).unwrap();
    assert_eq!((got["hasPassword"].as_bool(), got["port"].as_u64()), (Some(true), Some(u64::from(port))));
    let started = s.registry.dispatch(&mut restarted, "db.connection.connect", json!({ "connection": id })).unwrap();
    let operation = started["operation"].clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let state = loop {
        let state =
            s.registry.dispatch(&mut restarted, "db.operation.state", json!({ "operation": operation })).unwrap();
        if state["status"] != "running" || std::time::Instant::now() > deadline {
            break state;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(state["status"], "done", "{state}");
    let status = s.registry.dispatch(&mut restarted, "db.connection.status", json!({ "connection": id })).unwrap();
    assert_eq!(status["connections"][0]["state"], "connected");
    let closing = s
        .registry
        .dispatch(&mut restarted, "db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }))
        .unwrap();
    assert!(closing["operation"].is_u64());

    let refused = s
        .registry
        .dispatch(&mut restarted, "db.dataSource.test", json!({ "host": "127.0.0.1", "port": 1, "password": "x" }))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let state = s
        .registry
        .dispatch(&mut restarted, "db.operation.state", json!({ "operation": refused["operation"] }))
        .unwrap();
    assert_eq!(state["cause"], "refused", "{state}");
    let wrong = s
        .registry
        .dispatch(&mut restarted, "db.dataSource.test", json!({ "connection": id, "password": "not-it" }))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let state =
        s.registry.dispatch(&mut restarted, "db.operation.state", json!({ "operation": wrong["operation"] })).unwrap();
    assert_eq!(state["cause"], "authentication", "{state}");
    assert!(!state.to_string().contains("not-it"));
}

#[test]
fn an_sqlite_connection_has_a_path_with_open_and_create_and_tests_against_the_file() {
    let mut s = setup();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> =
        vec![Arc::new(zen_db::postgres::Postgres), Arc::new(zen_db::sqlite::Sqlite)];
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let file = s.dir.path().join("project/shop.db");
    std::fs::File::create(&file).unwrap();
    let params = json!({ "driver": "sqlite", "database": file.display().to_string(), "page": "main" });
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &params).unwrap();
    assert_eq!(dialog.current().unwrap().id, "sqlite");
    let fields = dialog.fields();
    assert_eq!((fields["name"].as_str(), fields["host"].as_str()), (Some("shop.db"), Some("")));
    assert!(fields.get("port").is_none() && fields.get("password").is_none(), "{fields}");
    dialog.set_file(":memory:");
    assert_eq!(dialog.fields()["database"], ":memory:");
    dialog.set_file(&file.display().to_string());

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(960.0, 640.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        for (page_name, page) in [("main", Page::Main), ("advanced", Page::Advanced)] {
            dialog.page = page;
            let mut frame = |screen: &mut crate::offscreen::Offscreen| {
                screen.render(|ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                    let _ = show(ui, &palette, &mut dialog);
                })
            };
            frame(&mut screen);
            let image = frame(&mut screen);
            if let Some(dir) = &snapshots {
                let png = dir.join(format!("db-connection-sqlite-{page_name}-{theme_name}.png"));
                crate::offscreen::write_png(&png, image.image()).unwrap();
            }
        }
        dialog.page = Page::Main;
        let tree = screen.accessibility_tree(|ui| {
            let _ = show(ui, &palette, &mut dialog);
        });
        let names: Vec<String> = tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
        for wanted in ["Path:", "Open…", "Create…", "Test Connection", "OK"] {
            assert!(names.iter().any(|name| name == wanted), "{wanted} in {names:?}");
        }
        for unwanted in ["Host:", "Password:", "User:"] {
            assert!(!names.iter().any(|name| name == unwanted), "{unwanted} in {names:?}");
        }
    }

    if zen_db::sqlite::process::find_program(None).is_ok() {
        dialog.start_test(&s.registry, &mut s.session);
        wait_test(&mut s, &mut dialog);
        let Some(Test::Passed(text)) = &dialog.test else { panic!("{:?}", dialog.test) };
        assert!(text.contains("SQLite 3."), "{text}");
    }
    let (command, params) = dialog.save_call();
    let saved = s.registry.dispatch(&mut s.session, command, params).unwrap();
    assert_eq!((saved["driver"].as_str(), saved["name"].as_str()), (Some("sqlite"), Some("shop.db")));
    assert_eq!(saved["database"], file.display().to_string());
    assert!(saved.get("host").is_none(), "{saved}");
}

#[test]
fn mysql_takes_its_own_defaults_and_uri() {
    let mut s = setup();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = zen_db::drivers();
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    let mysql = dialog.drivers.iter().position(|driver| driver.id == "mysql").unwrap();
    dialog.choose_driver(mysql);
    assert_eq!((dialog.port.as_str(), dialog.user.as_str(), dialog.database.as_str()), ("3306", "root", ""));
    assert!(!dialog.show_all_databases, "MySQL's databases are schemas of one catalogue");
    dialog.by_url = true;
    dialog.url = "mariadb://ann:pw@db.example.com/shop?ssl-mode=VERIFY_IDENTITY".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current().unwrap().id, "mariadb");
    assert_eq!(
        (dialog.port.as_str(), dialog.database.as_str(), dialog.tls_mode.as_str()),
        ("3306", "shop", "verify-full")
    );
    assert!(dialog.properties.is_empty(), "{:?}", dialog.properties);
}

#[test]
fn a_url_without_a_password_and_redis_default_to_no_authentication() {
    let mut s = setup();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = zen_db::drivers();
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    assert!(!dialog.no_auth, "PostgreSQL asks for a password");
    let redis = dialog.drivers.iter().position(|driver| driver.id == "redis").unwrap();
    dialog.choose_driver(redis);
    assert!(dialog.no_auth, "Redis usually needs none");
    let postgres = dialog.drivers.iter().position(|driver| driver.id == "postgresql").unwrap();
    dialog.choose_driver(postgres);
    dialog.by_url = true;
    dialog.url = "redis://cache.local:6379/0".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current().unwrap().id, "redis");
    assert_eq!(dialog.fields()["auth"], "none");
    dialog.url = "postgresql://ann:pw@db.local/shop".to_owned();
    dialog.fill_from_url();
    assert_eq!((dialog.fields()["auth"].as_str(), dialog.password.as_str()), (Some("password"), "pw"));
    dialog.password.clear();
    dialog.url = "postgresql://ann@db.local/shop".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.fields()["auth"], "password", "PostgreSQL without a password in the URL asks at connect");
}

#[test]
fn profiles_are_listed_by_family_fill_their_defaults_and_a_url_picks_one() {
    let mut s = setup();
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(zen_db::drivers());
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    let rows = dialog.rows();
    assert!(matches!(rows[0], Row::Family(_)) && rows[1] == Row::Driver(0), "{rows:?}");
    let titles: Vec<String> = rows
        .iter()
        .filter_map(|row| match row {
            Row::Family(number) => Some(dialog.family_title(*number)),
            _ => None,
        })
        .collect();
    assert_eq!(
        titles,
        [
            "PostgreSQL and compatible",
            "MySQL and compatible",
            "SQLite and compatible",
            "Redis and compatible",
            "ClickHouse and compatible"
        ]
    );
    let mysql_family = rows.iter().position(|row| *row == Row::Family(1)).unwrap();
    assert_eq!(rows[mysql_family + 1], Row::Driver(1), "MySQL first, then MariaDB, then its profiles");
    assert_eq!(rows[mysql_family + 2], Row::Driver(2));

    dialog.search = "roach".to_owned();
    let found = dialog.rows();
    assert_eq!(found.len(), 2, "{found:?}");
    let Row::Profile(index) = found[1] else { panic!("{found:?}") };
    dialog.choose_profile(index);
    assert_eq!(
        (dialog.current().unwrap().id.as_str(), dialog.current().unwrap().name.as_str()),
        ("postgresql", "CockroachDB")
    );
    assert_eq!((dialog.port.as_str(), dialog.database.as_str(), dialog.user.as_str()), ("26257", "defaultdb", "root"));
    assert!(!dialog.current().unwrap().has("catalogs"), "only its own database is read");
    let fields = dialog.fields();
    assert_eq!((fields["profile"].as_str(), fields["driver"].as_str()), (Some("cockroachdb"), Some("postgresql")));
    assert_eq!(fields["name"], "CockroachDB - localhost");
    dialog.search = "nothing-like-this".to_owned();
    assert!(dialog.rows().is_empty());
    dialog.search.clear();

    let quest = dialog.profiles.iter().position(|profile| profile.id == "questdb").unwrap();
    dialog.choose_profile(quest);
    assert_eq!(dialog.settings["pooler"], json!(true));
    assert!(!dialog.current().unwrap().has("transactions"));
    dialog.choose_driver(0);
    assert_eq!(dialog.fields()["profile"], "", "a driver of its own takes the profile off");

    dialog.by_url = true;
    dialog.url = "postgresql://alex:pw@ep-cool-darkness-123456.us-east-2.aws.neon.tech/neondb".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current_profile().map(|profile| profile.id.as_str()), Some("neon"));
    assert_eq!(
        (dialog.tls_mode.as_str(), dialog.database.as_str(), dialog.port.as_str()),
        ("require", "neondb", "5432")
    );
    dialog.url = "valkeys://cache.example.com:6380".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current().unwrap().id, "redis");
    assert_eq!(dialog.current_profile().map(|profile| profile.id.as_str()), Some("valkey"));
    assert_eq!((dialog.tls_mode.as_str(), dialog.port.as_str()), ("require", "6380"));
    dialog.url = "mysql://root@shop.cluster-c9akciq32.us-east-1.rds.amazonaws.com/shop".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current_profile().map(|profile| profile.id.as_str()), Some("aurora-mysql"));

    let (command, params) = dialog.save_call();
    let saved = s.registry.dispatch(&mut s.session, command, params).unwrap();
    assert_eq!((saved["driver"].as_str(), saved["profile"].as_str()), (Some("mysql"), Some("aurora-mysql")));
    let id = saved["id"].as_str().unwrap().to_owned();
    let edit = ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "id": id })).unwrap();
    assert_eq!(edit.current_profile().map(|profile| profile.id.as_str()), Some("aurora-mysql"));
    assert_eq!(edit.current().unwrap().name, "Amazon Aurora MySQL");

    let mut detected = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    detected.detected_profile = Some("cockroachdb".to_owned());
    detected.port = "5432".to_owned();
    detected.set_profile(Some("cockroachdb"));
    assert_eq!(detected.port, "5432", "using a detected profile keeps the fields");
    assert_eq!(detected.detected_profile, None);
    let opened = ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "profile": "tidb" })).unwrap();
    assert_eq!((opened.current().unwrap().id.as_str(), opened.port.as_str()), ("mysql", "4000"));

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(960.0, 640.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        for (shot, search, profile) in [("profiles", "", "cockroachdb"), ("search", "db", "tidb")] {
            let mut dialog =
                ConnectionDialog::open(&s.registry, &mut s.session, &json!({ "profile": profile })).unwrap();
            dialog.search = search.to_owned();
            if shot == "profiles" {
                dialog.detected_profile = Some("yugabytedb".to_owned());
                dialog.test = Some(Test::Passed("Connected: PostgreSQL 11.2-YB-2.20.1.0-b0, 3 ms".to_owned()));
            }
            let mut frame = |screen: &mut crate::offscreen::Offscreen| {
                screen.render(|ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                    let _ = show(ui, &palette, &mut dialog);
                })
            };
            frame(&mut screen);
            let image = frame(&mut screen);
            if let Some(dir) = &snapshots {
                let png = dir.join(format!("db-connection-{shot}-{theme_name}.png"));
                crate::offscreen::write_png(&png, image.image()).unwrap();
            }
            let tree = screen.accessibility_tree(|ui| {
                let _ = show(ui, &palette, &mut dialog);
            });
            let names: Vec<String> =
                tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
            let wanted: &[&str] = if shot == "profiles" {
                &["CockroachDB", "Amazon Redshift", "Valkey", "Use the YugabyteDB profile"]
            } else {
                &["TiDB", "CockroachDB", "KeyDB", "MariaDB"]
            };
            for wanted in wanted {
                assert!(names.iter().any(|name| name == wanted), "{wanted} in {names:?}");
            }
        }
    }
}

#[test]
fn a_plugins_driver_is_listed_after_the_built_in_ones_with_its_own_fields() {
    let mut s = setup();
    let declaration: zen_db::external::DriverDeclaration = serde_json::from_value(json!({
        "id": "duckdb",
        "name": "DuckDB",
        "capabilities": {
            "models": ["relational"],
            "language": "sql:duckdb",
            "settings": [{ "key": "file", "kind": "path" }],
        },
    }))
    .unwrap();
    let declared = zen_db::external::Declared::default();
    declared.set(vec![zen_db::external::ExternalSpec {
        declaration,
        program: s.dir.path().join("zen-duckdb"),
        sha256: None,
        plugin: Some("duckdb".into()),
    }]);
    let more = declared.clone();
    s.session.state_mut::<zen_workbench::database::Database>().set_more_drivers(Arc::new(move || more.as_drivers()));
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    let ids: Vec<&str> = dialog.drivers.iter().map(|driver| driver.id.as_str()).collect();
    assert_eq!(ids, ["postgresql", "memory-kv", "duckdb"]);
    dialog.choose_driver(2);
    assert_eq!(dialog.current().unwrap().name, "DuckDB");
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(960.0, 640.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let mut frame = |screen: &mut crate::offscreen::Offscreen| {
            screen.render(|ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                let _ = show(ui, &palette, &mut dialog);
            })
        };
        frame(&mut screen);
        let image = frame(&mut screen);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-connection-external-{theme_name}.png")), image.image())
                .unwrap();
        }
    }
    assert_eq!(declared.drivers()[0].running(), 0, "listing and choosing start nothing");
}

#[test]
fn a_sqlite_url_picks_sqlite_and_keeps_the_files_absolute_path() {
    let mut s = setup();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = zen_db::drivers();
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    dialog.by_url = true;
    dialog.url = "sqlite:///Users/ann/shop.db".to_owned();
    dialog.fill_from_url();
    assert_eq!(dialog.current().unwrap().id, "sqlite");
    assert_eq!(dialog.database, "/Users/ann/shop.db");
}
#[test]
fn a_driver_that_asks_no_password_starts_without_authentication() {
    let mut s = setup();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> =
        vec![Arc::new(zen_db::postgres::Postgres), Arc::new(zen_db::memory::MemoryDriver::sql().without_password())];
    s.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let mut dialog = ConnectionDialog::open(&s.registry, &mut s.session, &json!({})).unwrap();
    assert!(!dialog.no_auth, "PostgreSQL asks for a password");
    let index = dialog.drivers.iter().position(|driver| driver.id == zen_db::memory::SQL).unwrap();
    dialog.choose_driver(index);
    assert!(dialog.no_auth, "a driver without passwords starts with no authentication");
}
