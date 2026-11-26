use std::path::PathBuf;
use std::sync::Arc;

use egui::vec2;
use serde_json::json;
use zen_db::model::NodeKind;
use zen_db::schema::{CachedChildren, CachedNode, Snapshot};
use zen_workbench::workspaces::Workspaces;

use super::super::super::database_navigator::NavigatorPanel;
use super::*;

fn node(path: &[&str], kind: NodeKind, label: &str, detail: &str) -> CachedNode {
    CachedNode {
        path: path.iter().map(|part| (*part).to_owned()).collect(),
        has_children: matches!(kind, NodeKind::Catalog | NodeKind::KeyPrefix),
        kind,
        label: label.to_owned(),
        detail: Some(detail.to_owned()),
        system: false,
        size: None,
        rows: None,
        comment: None,
    }
}

fn level(path: &[&str], nodes: Vec<CachedNode>) -> CachedChildren {
    CachedChildren { path: path.iter().map(|part| (*part).to_owned()).collect(), nodes }
}

fn snapshot(id: &str) -> Snapshot {
    Snapshot {
        taken: 1,
        nodes: vec![
            level(
                &[],
                vec![node(&["db0"], NodeKind::Catalog, "db0", "2508"), node(&["db2"], NodeKind::Catalog, "db2", "1")],
            ),
            level(
                &["db0"],
                vec![
                    node(&["db0", "bulk:"], NodeKind::KeyPrefix, "bulk", "2500"),
                    node(&["db0", "session:"], NodeKind::KeyPrefix, "session", "2"),
                    node(&["db0", "user:"], NodeKind::KeyPrefix, "user", "3"),
                    node(&["db0", "board"], NodeKind::Key, "board", "zset"),
                    node(&["db0", "doc"], NodeKind::Key, "doc", "string"),
                    node(&["db0", "events"], NodeKind::Key, "events", "stream"),
                    node(&["db0", "queue"], NodeKind::Key, "queue", "list"),
                ],
            ),
            level(
                &["db0", "user:"],
                vec![
                    node(&["db0", "user:", "2:"], NodeKind::KeyPrefix, "2", "1"),
                    node(&["db0", "user:", "1"], NodeKind::Key, "1", "string"),
                    node(&["db0", "user:", "3"], NodeKind::Key, "3", "hash"),
                ],
            ),
        ],
        ..Snapshot::new(id, "redis")
    }
}

fn hash_answer() -> Value {
    json!({
        "status": "done",
        "operation": 1,
        "key": {
            "key": "user:3",
            "database": "db0",
            "type": "hash",
            "ttlMs": 3_540_000,
            "size": 96,
            "length": 4,
            "encoding": "listpack",
            "columns": [{ "name": "field", "type": "string" }, { "name": "value", "type": "string" }],
            "rows": [
                [{ "kind": "text", "value": "name" }, { "kind": "text", "value": "Ada Lovelace" }],
                [{ "kind": "text", "value": "email" }, { "kind": "text", "value": "ada@example.com" }],
                [{ "kind": "text", "value": "visits" }, { "kind": "text", "value": "42" }],
                [{ "kind": "text", "value": "plan" }, { "kind": "text", "value": "team" }]
            ],
            "truncated": false
        }
    })
}

#[test]
fn a_redis_connection_expanded_and_a_hash_shown_in_light_and_dark() {
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
            json!({ "name": "Cache", "url": "redis://cache.example.com:6379/0", "type": "test", "auth": "none" }),
        )
        .unwrap();
    assert_eq!(added["driver"], "redis");
    let id = added["id"].as_str().unwrap().to_owned();
    let file = dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot(&id)).unwrap();
    let read = registry.dispatch(&mut session, "db.schema.children", json!({ "connection": id, "path": [] })).unwrap();
    assert!(read["total"].as_u64() > Some(0), "db.schema.children waits for the snapshot file: {read}");
    for path in [json!([]), json!(["db0"]), json!(["db0", "user:"])] {
        registry.dispatch(&mut session, "db.navigator.select", json!({ "connection": id, "path": path })).unwrap();
    }
    registry
        .dispatch(&mut session, "db.navigator.expand", json!({ "connection": id, "path": ["db0", "user:"] }))
        .unwrap();
    registry
        .dispatch(&mut session, "db.navigator.select", json!({ "connection": id, "path": ["db0", "user:", "3"] }))
        .unwrap();
    let tab = InnerTab {
        id: zen_workbench::database::keys::tab_id(&id, &["db0".into(), "user:".into(), "3".into()]),
        kind: zen_workbench::database::keys::KEY_TAB.to_owned(),
        title: "user:3".to_owned(),
        path: None,
        closable: true,
        data: json!({ "connection": id, "path": ["db0", "user:", "3"], "database": "db0", "key": "user:3" }),
        group: 0,
    };
    let mut views = KeyViews::default();
    let mut kept = KeyTab::default();
    kept.take(hash_answer());
    views.tabs.insert(tab.id.clone(), kept);

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1180.0, 420.0), 2.0);
    let mut panel = NavigatorPanel::default();
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let mut frame = |screen: &mut crate::offscreen::Offscreen| {
            screen.render(|ui| {
                let all = ui.max_rect();
                ui.painter().rect_filled(all, 0, palette.island);
                let left = Rect::from_min_size(all.min, vec2(320.0, all.height()));
                let right = Rect::from_min_max(pos2(left.right() + 1.0, all.top()), all.max);
                let mut navigator = ui.new_child(UiBuilder::new().max_rect(left));
                panel.show(&mut navigator, &palette, &registry, &mut session);
                let mut value = ui.new_child(UiBuilder::new().max_rect(right));
                views.show(&mut value, &palette, &registry, &mut session, &tab);
            })
        };
        frame(&mut screen);
        let image = frame(&mut screen);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-redis-{theme_name}.png")), image.image()).unwrap();
        }
    }
    let state = registry.dispatch(&mut session, "db.navigator.state", json!({})).unwrap();
    let rows: Vec<String> = state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let depth = row["depth"].as_u64().unwrap_or(0) as usize;
            format!("{}{} {}", "  ".repeat(depth), row["label"].as_str().unwrap(), row["detail"].as_str().unwrap_or(""))
        })
        .collect();
    assert_eq!(
        rows,
        [
            "Cache cache.example.com:6379",
            "  db0 2508",
            "    bulk 2500",
            "    session 2",
            "    user 3",
            "      2 1",
            "      1 string",
            "      3 hash",
            "    board zset",
            "    doc string",
            "    events stream",
            "    queue list",
            "  db2 1",
        ]
    );
    let shown = views.tabs.get(&tab.id).unwrap();
    assert_eq!((shown.columns.len(), shown.rows.len()), (2, 4));
    assert_eq!(shown.rows[1][1].text, "ada@example.com");
    let status = registry.dispatch(&mut session, "db.connection.status", json!({ "connection": id })).unwrap();
    assert_eq!(status["connections"][0]["state"], "disconnected", "drawing connects nowhere");
}

#[test]
fn times_and_sizes_read_in_whole_units() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert_eq!(ttl_text(59_400), "59 s");
    assert_eq!(ttl_text(723_000), "12 min 3 s");
    assert_eq!(ttl_text(2 * 86_400_000 + 4 * 3_600_000), "2 d 4 h");
    assert_eq!(size_text(512), "512 B");
    assert_eq!(size_text(1229), "1.2 KB");
    assert_eq!(size_text(3_565_158), "3.4 MB");
}
