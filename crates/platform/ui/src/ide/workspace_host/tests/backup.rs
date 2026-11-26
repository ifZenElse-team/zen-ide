use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use zen_db::backup::{Origin, Probe};

use super::{at, click_at, database_window, shows, snapshot_png, until};

const PG_DUMP: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "pg_dump (PostgreSQL) 17.2"; exit 0; fi
for arg in "$@"; do case $arg in --file=*) file=${arg#--file=};; esac; done
echo "pg_dump: dumping contents of table public.orders" >&2
printf 'PGDMP fake' > "$file"
"#;

const PG_RESTORE: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "pg_restore (PostgreSQL) 17.2"; exit 0; fi
echo "pg_restore: processing data for table public.orders" >&2
"#;

fn tool(folder: &Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = folder.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // WHY: a new script's first start can outlast the 5 s probe on a loaded machine, so it is paid here.
    let answer = std::process::Command::new(&path).arg("--version").output().unwrap();
    assert!(answer.status.success(), "{name}: {answer:?}");
}

#[test]
fn the_backup_dialog_shows_the_tool_found_runs_in_the_run_window_and_restore_asks_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        let tools = window.dir.path().join("tools");
        let out = window.dir.path().join("backups");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        tool(&tools, "pg_dump", PG_DUMP);
        tool(&tools, "pg_restore", PG_RESTORE);
        zen_workbench::database::backups::set_probe(
            &mut window.session,
            Probe {
                folders: vec![(tools.clone(), Origin::Path)],
                docker: None,
                timeout: std::time::Duration::from_secs(60),
            },
        );
        let added = window.run(
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": "postgresql", "host": "localhost", "database": "shop", "user": "app", "password": "pw" }),
        );
        let id = added["id"].as_str().unwrap().to_owned();
        window.run("workspace.host.expand", json!({ "id": "database" }));

        super::super::super::backup_dialog::request(
            &mut window.session,
            super::super::super::backup_dialog::Mode::Backup,
            json!({ "connection": id, "path": ["shop", "public"] }),
        );
        let texts = until(&mut window, "the tool", |texts| shows(texts, "Automatic: pg_dump (PostgreSQL) 17.2 (PATH)"));
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["backupDatabase"]));
        assert!(shows(&texts, "Backup Database") && shows(&texts, "Custom") && shows(&texts, "No owner"), "{texts:?}");
        assert!(texts.iter().any(|(_, text)| text == "public"), "the schema chosen in the navigator: {texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-backup-{name}"));
        window.view.backup_dialog.as_mut().unwrap().set_folder(out.to_str().unwrap());
        let (command, call) = window.view.backup_dialog.as_ref().unwrap().start_call();
        assert_eq!(command, "db.backup.start");
        assert_eq!(call["pgDump"]["schemas"], json!(["public"]), "{call}");
        click_at(&mut window, at(&texts, "Start Backup") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the run", |texts| shows(texts, "Backup written to"));
        assert!(window.view.backup_dialog.is_none());
        assert!(shows(&texts, "Using pg_dump (PostgreSQL) 17.2"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-backup-run-{name}"));
        let written: Vec<PathBuf> = std::fs::read_dir(&out).unwrap().flatten().map(|entry| entry.path()).collect();
        assert_eq!(written.len(), 1, "{written:?}");
        assert!(written[0].file_name().unwrap().to_string_lossy().starts_with("shop_"), "{written:?}");

        super::super::super::backup_dialog::request(
            &mut window.session,
            super::super::super::backup_dialog::Mode::Restore,
            json!({ "connection": id }),
        );
        until(&mut window, "the restore dialog", |texts| shows(texts, "Restore Database"));
        window.view.backup_dialog.as_mut().unwrap().set_file(written[0].to_str().unwrap());
        let texts = until(&mut window, "the restore tool", |texts| {
            shows(texts, "Automatic: pg_restore (PostgreSQL) 17.2 (PATH)")
        });
        click_at(&mut window, at(&texts, "Restore") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the question", |texts| shows(texts, "Restore Anyway"));
        assert!(texts.iter().any(|(_, text)| text.contains("into database shop of Shop?")), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-restore-{name}"));
        click_at(&mut window, at(&texts, "Restore Anyway") + vec2(6.0, 6.0));
        until(&mut window, "the restore", |texts| shows(texts, "Restore into shop finished"));
        assert!(window.view.backup_dialog.is_none());

        let restored = window.run("db.backup.state", json!({ "limit": 1 }))["jobs"][0]["runId"].as_u64().unwrap();
        assert!(
            window.registry.dispatch(&mut window.session, "run.process.rerun", json!({ "runId": restored })).is_err()
        );
        let texts = until(&mut window, "the rerun's question", |texts| shows(texts, "Restore Anyway"));
        assert!(shows(&texts, "Restore Database") && !shows(&texts, "unknown kind"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-restore-rerun-{name}"));
        click_at(&mut window, at(&texts, "Restore Anyway") + vec2(6.0, 6.0));
        until(&mut window, "the dialog closed", |texts| !shows(texts, "Restore Anyway"));
        let state = window.run("db.backup.state", json!({ "limit": 1 }));
        assert_ne!(state["jobs"][0]["runId"].as_u64(), Some(restored), "{state}");
    }
}
