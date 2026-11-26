use serde_json::{Value, json};

use crate::plugin_runs::tests::{Fixture, code, failure, fixture};

use super::*;

fn section() -> Value {
    json!({ "plugin": "dockerfile", "window": "docker", "section": "containers" })
}

fn with(mut params: Value, extra: Value) -> Value {
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    params
}

fn calls(fixture: &Fixture) -> Vec<String> {
    std::fs::read_to_string(fixture.bin.join("calls")).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn rows(fixture: &mut Fixture, extra: Value) -> Value {
    fixture.call(ListRows::ID, with(section(), extra)).unwrap()
}

#[test]
fn the_windows_are_listed_with_their_sections_and_actions() {
    let mut fixture = fixture(true);
    let listed = fixture.as_agent(ListWindows::ID, json!({})).unwrap();
    let window = &listed["windows"][0];
    assert_eq!(window["plugin"], "dockerfile");
    assert_eq!(window["sections"][0]["columns"], json!(["name", "state"]));
    assert_eq!(window["sections"][0]["columnTitles"], json!(["Name", "state"]));
    assert_eq!(window["sections"][0]["actions"][2]["kind"], "destroy");
}

#[test]
fn rows_come_from_the_plug_in_s_reading_of_the_declared_listing() {
    let mut fixture = fixture(true);
    let answer = fixture.as_agent(ListRows::ID, section()).unwrap();
    assert_eq!(answer["rows"][0], json!({ "id": "c1", "columns": ["web", "running"], "state": "running" }));
    assert_eq!(answer["rows"][1]["id"], "c2");
    assert_eq!(calls(&fixture), ["ps|-a|"]);
    rows(&mut fixture, json!({}));
    assert_eq!(calls(&fixture).len(), 1, "rows listed before are answered without a new listing");
    rows(&mut fixture, json!({ "refresh": true }));
    assert_eq!(calls(&fixture).len(), 2);
}

#[test]
fn an_untrusted_project_lists_and_changes_nothing_even_for_the_person() {
    let mut fixture = fixture(false);
    assert_eq!(code(fixture.call(ListRows::ID, section()).unwrap_err()), crate::trust::NOT_TRUSTED);
    let act = with(section(), json!({ "action": "prune", "confirmed": true }));
    assert_eq!(code(fixture.call(Act::ID, act).unwrap_err()), crate::trust::NOT_TRUSTED);
    assert!(calls(&fixture).is_empty());
}

#[test]
fn a_row_value_goes_in_as_one_argument_and_only_from_listed_rows() {
    let mut fixture = fixture(true);
    let made_up = with(section(), json!({ "action": "stop", "rows": ["c1; rm -rf /"], "waitMs": 10000 }));
    assert!(failure(fixture.call(Act::ID, made_up).unwrap_err()).message.contains("c1; rm -rf /"));
    rows(&mut fixture, json!({}));
    let rename = with(section(), json!({ "action": "rename", "rows": ["c1"], "waitMs": 10000 }));
    let done = fixture.call(Act::ID, rename).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    assert_eq!(
        done["done"][0]["output"].as_str().unwrap().lines().take(3).collect::<Vec<_>>(),
        ["[rename]", "[c1]", "[web]"]
    );
    assert_eq!(calls(&fixture).last().map(String::as_str), Some("ps|-a|"), "the rows are listed again after a change");
}

#[test]
fn agents_change_as_the_plug_in_s_run_access_allows() {
    let mut fixture = fixture(true);
    rows(&mut fixture, json!({}));
    let stop = with(section(), json!({ "action": "stop", "rows": ["c1"], "waitMs": 10000 }));
    assert_eq!(code(fixture.as_agent(Act::ID, stop.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    let sneak = with(section(), json!({ "action": "sneak", "rows": ["c1"], "waitMs": 10000 }));
    assert_eq!(code(fixture.as_agent(Act::ID, sneak).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    let done = fixture.as_agent(Act::ID, stop).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
}

#[test]
fn a_destroying_action_asks_the_person_every_time() {
    let mut fixture = fixture(true);
    let prune = with(section(), json!({ "action": "prune", "waitMs": 10000 }));
    let refused = failure(fixture.call(Act::ID, prune.clone()).unwrap_err());
    assert_eq!(refused.code.as_deref(), Some(CONFIRMATION_NEEDED), "the person confirms in the dialog");
    assert!(fixture.call(Act::ID, with(prune.clone(), json!({ "confirmed": true }))).is_ok());
    let pruned = calls(&fixture).iter().filter(|call| call.starts_with("container|prune")).count();
    assert_eq!(pruned, 1);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    let agent = with(prune.clone(), json!({ "confirmed": true }));
    let asked = failure(fixture.as_agent(Act::ID, agent.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(CONFIRMATION_NEEDED), "an agent's own yes counts for nothing");
    let token = asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned();
    let allow = json!({ "token": token });
    assert_eq!(code(fixture.as_agent(AllowDestroy::ID, allow.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    fixture.call(AllowDestroy::ID, allow).unwrap();
    let confirmed = with(agent, json!({ "confirmation": token }));
    assert!(fixture.as_agent(Act::ID, confirmed.clone()).is_ok());
    assert_eq!(code(fixture.as_agent(Act::ID, confirmed).unwrap_err()), CONFIRMATION_NEEDED, "a yes counts once");
    let pruned = calls(&fixture).iter().filter(|call| call.starts_with("container|prune")).count();
    assert_eq!(pruned, 2);
}

#[test]
fn a_failed_listing_is_shown_and_waits_before_listing_again() {
    let mut fixture = fixture(true);
    std::fs::write(fixture.bin.join("down"), "").unwrap();
    let answer = rows(&mut fixture, json!({}));
    assert!(answer["error"].as_str().unwrap().contains("Cannot connect"), "{answer}");
    assert!(answer["retryAfterMs"].as_u64().unwrap() > 20_000, "{answer}");
    rows(&mut fixture, json!({ "refresh": true }));
    assert_eq!(calls(&fixture).len(), 1, "no listing within 30 s of a failure");
    std::fs::remove_file(fixture.bin.join("down")).unwrap();
    let answer = rows(&mut fixture, json!({ "refresh": true, "force": true }));
    assert_eq!(calls(&fixture).len(), 2);
    assert!(answer.get("error").is_none() && answer["rows"][0]["id"] == "c1", "{answer}");
    let notifications = fixture.call("ui.notifications.list", json!({})).unwrap();
    assert!(!notifications.to_string().contains("Cannot connect"), "the error is in the window, not in a notification");
}

#[test]
fn rows_are_not_listed_again_while_an_action_on_the_section_runs() {
    let mut fixture = fixture(true);
    rows(&mut fixture, json!({}));
    std::fs::write(fixture.bin.join("slow"), "").unwrap();
    let stop = with(section(), json!({ "action": "stop", "rows": ["c1"] }));
    assert_eq!(fixture.call(Act::ID, stop.clone()).unwrap()["running"], true);
    let answer = rows(&mut fixture, json!({ "refresh": true, "force": true }));
    assert_eq!(answer["busy"], true);
    assert_eq!(calls(&fixture).iter().filter(|call| call.starts_with("ps")).count(), 1);
    assert!(failure(fixture.call(Act::ID, stop).unwrap_err()).message.contains("still running"));
    std::fs::write(fixture.bin.join("go"), "").unwrap();
    let started = Instant::now();
    while rows(&mut fixture, json!({}))["busy"] == true {
        assert!(started.elapsed() < Duration::from_secs(20), "the action ends");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(calls(&fixture).iter().filter(|call| call.starts_with("ps")).count(), 2, "listed again once it ended");
}

#[test]
fn a_placeholder_that_names_no_column_is_refused() {
    let columns = vec!["name".to_owned()];
    let row = ToolRow { id: "c1".to_owned(), columns: vec!["web".to_owned()], state: None };
    let args = ["${row.size}".to_owned()];
    assert!(filled(&args, Some(&row), &columns).is_err());
    let args = ["x${row.id}".to_owned()];
    assert!(filled(&args, Some(&row), &columns).is_err(), "a row value is a whole argument, never part of one");
    let args = ["--name=${row.name}x".to_owned(), "${row.name}".to_owned()];
    assert!(filled(&args, Some(&row), &columns).is_err());
    let args = ["${row.name}".to_owned(), "${row.id}".to_owned()];
    assert_eq!(filled(&args, Some(&row), &columns).unwrap(), ["web", "c1"]);
}

#[test]
fn a_stream_action_runs_in_a_run_tab_under_the_run_access_rules() {
    let mut fixture = fixture(true);
    rows(&mut fixture, json!({}));
    let logs = with(section(), json!({ "action": "logs", "rows": ["c2"] }));
    assert_eq!(code(fixture.as_agent(Act::ID, logs.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    let started = fixture.call(Act::ID, logs).unwrap();
    let since = Instant::now();
    let mut text = String::new();
    while !text.contains("menu=") && since.elapsed() < Duration::from_secs(20) {
        text = fixture.output(&started);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(text.contains("[logs]") && text.contains("[c2]"), "{text}");
}

#[test]
fn the_window_does_not_wait_for_a_listing() {
    let mut fixture = fixture(true);
    std::fs::write(fixture.bin.join("slowlist"), "").unwrap();
    let answer = rows(&mut fixture, json!({ "waitMs": 0 }));
    assert_eq!(answer["listing"], true, "{answer}");
    assert_eq!(answer["rows"], json!([]));
    std::fs::write(fixture.bin.join("go"), "").unwrap();
    let answer = rows(&mut fixture, json!({}));
    assert!(answer.get("listing").is_none(), "{answer}");
    assert_eq!(answer["rows"][0]["id"], "c1");
}

#[test]
fn a_window_answers_its_side_and_whether_its_icon_is_drawn_but_not_the_markup() {
    let mut fixture = fixture(true);
    let listed = fixture.as_agent(ListWindows::ID, json!({})).unwrap();
    let window = &listed["windows"][0];
    assert_eq!(window["side"], "right");
    assert_eq!(window["icon"], true);
    assert!(!listed.to_string().contains("<svg"), "the markup is not sent to agents: {listed}");
    assert!(icon("dockerfile", "docker").is_some_and(|svg| svg.starts_with("<svg")));
    assert!(icon("dockerfile", "nothing").is_none() && icon("nobody", "docker").is_none());
}

#[test]
fn the_plug_ins_tool_window_icons_are_drawn_and_docker_and_kubernetes_open_on_the_right() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut sides = BTreeMap::new();
    for entry in std::fs::read_dir(root.join("plugins")).unwrap().flatten() {
        let Ok(bytes) = std::fs::read(entry.path().join("plugin.json")) else { continue };
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        let windows: Vec<ToolWindowDeclaration> =
            serde_json::from_value(manifest["toolWindows"].clone()).unwrap_or_default();
        for window in windows {
            let id = entry.file_name().to_string_lossy().into_owned();
            assert_eq!(zen_image::icon::problem(&window.icon), None, "plugins/{id}: the icon of `{}`", window.id);
            sides.insert(id, side_of(&window).to_owned());
        }
    }
    assert_eq!(sides.get("dockerfile").map(String::as_str), Some("right"));
    assert_eq!(sides.get("kubernetes").map(String::as_str), Some("right"));
}
