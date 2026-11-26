use serde_json::{Value, json};
use zen_syntax::ToolInputKind::{Count, Forward, Port};

use crate::plugin_runs::tests::{Fixture, code, failure, fixture};

use super::inputs::{checked, starting_value};
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

fn listed(fixture: &mut Fixture) {
    fixture.call(ListRows::ID, section()).unwrap();
}

#[test]
fn a_value_is_plain_digits_within_the_kind_s_bounds() {
    for (kind, text, expected) in [
        (Count, "0", Some("0")),
        (Count, "1000", Some("1000")),
        (Count, "1001", None),
        (Count, "-1", None),
        (Count, "+3", None),
        (Count, "03", None),
        (Count, "1e3", None),
        (Count, "", None),
        (Count, "3 ", None),
        (Port, "1024", Some("1024")),
        (Port, "1023", None),
        (Port, "65536", None),
        (Port, "99999999999", None),
        (Forward, "8080:80", Some("8080:80")),
        (Forward, "80:80", None),
        (Forward, "8080:0", None),
        (Forward, "8080:80:1", None),
        (Forward, "8080", None),
        (Forward, "8080:80;rm", None),
    ] {
        assert_eq!(checked(kind, text).as_deref(), expected, "{kind:?} {text:?}");
    }
}

#[test]
fn a_field_starts_from_the_first_number_of_its_column_only_when_it_is_in_range() {
    let input = |kind, from: &str, default| zen_syntax::ToolInput {
        kind,
        default_from: from.to_owned(),
        default,
        ..zen_syntax::ToolInput::default()
    };
    let columns = vec!["ports".to_owned()];
    let row = |text: &str| ToolRow { id: "s".to_owned(), columns: vec![text.to_owned()], state: None };
    let forward = input(Forward, "ports", None);
    assert_eq!(starting_value(&forward, &columns, Some(&row("80/TCP,443/TCP"))).as_deref(), Some(":80"));
    assert_eq!(starting_value(&forward, &columns, Some(&row("8080/TCP"))).as_deref(), Some("8080:8080"));
    assert_eq!(starting_value(&forward, &columns, Some(&row("<none>"))), None);
    let count = input(Count, "ports", Some(1));
    assert_eq!(starting_value(&count, &columns, Some(&row("5000"))).as_deref(), Some("1"), "out of range: the default");
    assert_eq!(starting_value(&count, &columns, Some(&row("3/3"))).as_deref(), Some("3"));
}

#[test]
fn the_list_tells_agents_what_the_person_will_be_asked() {
    let mut fixture = fixture(true);
    let listed = fixture.as_agent(ListWindows::ID, json!({})).unwrap();
    let actions = listed["windows"][0]["sections"][0]["actions"].as_array().unwrap().clone();
    let scale = actions.iter().find(|action| action["id"] == "scale").unwrap();
    assert_eq!(
        scale["inputs"][0],
        json!({ "id": "replicas", "title": "replicas", "kind": "count", "min": 0, "max": 1000 })
    );
    let forward = actions.iter().find(|action| action["id"] == "forward").unwrap();
    assert_eq!(forward["inputs"][0]["kind"], "forward");
    assert_eq!(forward["inputs"][0]["defaultFrom"], "name");
}

#[test]
fn a_person_s_value_is_checked_shown_in_the_question_and_passed_as_one_argument() {
    let mut fixture = fixture(true);
    listed(&mut fixture);
    let scale = with(section(), json!({ "action": "scale", "rows": ["c1"], "waitMs": 10000 }));
    let bad = with(scale.clone(), json!({ "inputs": { "replicas": "3; rm -rf /" }, "confirmed": true }));
    assert!(failure(fixture.call(Act::ID, bad).unwrap_err()).message.contains("from 0 to 1000"));
    let extra = with(scale.clone(), json!({ "inputs": { "replicas": "3", "other": "1" }, "confirmed": true }));
    assert!(fixture.call(Act::ID, extra).is_err(), "a value the action does not declare");
    let asked =
        failure(fixture.call(Act::ID, with(scale.clone(), json!({ "inputs": { "replicas": "3" } }))).unwrap_err());
    assert_eq!((asked.code.as_deref(), asked.message.as_str()), (Some(CONFIRMATION_NEEDED), "Scale web to 3?"));
    assert!(!calls(&fixture).iter().any(|call| call.starts_with("scale")), "nothing ran before the yes");
    let done = fixture.call(Act::ID, with(scale, json!({ "inputs": { "replicas": "3" }, "confirmed": true }))).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    assert!(calls(&fixture).contains(&"scale|c1|--replicas|3|".to_owned()), "{:?}", calls(&fixture));
}

#[test]
fn an_agent_never_passes_values_and_runs_once_with_the_person_s() {
    let mut fixture = fixture(true);
    listed(&mut fixture);
    let scale = with(section(), json!({ "action": "scale", "rows": ["c1"], "waitMs": 10000 }));
    let passed = with(scale.clone(), json!({ "inputs": { "replicas": "9" } }));
    assert_eq!(code(fixture.as_agent(Act::ID, passed.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    assert_eq!(code(fixture.as_agent(Act::ID, passed).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    let asked = failure(fixture.as_agent(Act::ID, scale.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(CONFIRMATION_NEEDED));
    let token = asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned();
    let notifications = fixture.call("ui.notifications.list", json!({})).unwrap().to_string();
    assert!(notifications.contains(EnterValues::ID) && notifications.contains("scale web"), "{notifications}");
    let enter = json!({ "token": token });
    assert_eq!(code(fixture.as_agent(EnterValues::ID, enter.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    let allow = json!({ "token": token, "inputs": { "replicas": "2" } });
    assert_eq!(code(fixture.as_agent(AllowDestroy::ID, allow.clone()).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    let again = with(scale.clone(), json!({ "confirmation": token }));
    assert_eq!(code(fixture.as_agent(Act::ID, again.clone()).unwrap_err()), CONFIRMATION_NEEDED, "no values yet");
    let token = {
        let asked = failure(fixture.as_agent(Act::ID, scale.clone()).unwrap_err());
        asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned()
    };
    let request = fixture.call(EnterValues::ID, json!({ "token": token })).unwrap();
    assert_eq!(request["action"], "scale");
    assert_eq!(take_entering(&mut fixture.session).map(|request| request.rows), Some(vec!["c1".to_owned()]));
    let wrong = json!({ "token": token, "inputs": { "replicas": "5000" } });
    assert!(fixture.call(AllowDestroy::ID, wrong).is_err());
    fixture.call(AllowDestroy::ID, json!({ "token": token, "inputs": { "replicas": "2" } })).unwrap();
    let confirmed = with(scale, json!({ "confirmation": token }));
    let done = fixture.as_agent(Act::ID, confirmed.clone()).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    assert!(calls(&fixture).contains(&"scale|c1|--replicas|2|".to_owned()));
    assert_eq!(code(fixture.as_agent(Act::ID, confirmed).unwrap_err()), CONFIRMATION_NEEDED, "the values count once");
}

#[test]
fn an_agent_s_forward_streams_with_the_person_s_ports_without_a_second_question() {
    let mut fixture = fixture(true);
    listed(&mut fixture);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "ask" })).unwrap();
    let forward = with(section(), json!({ "action": "forward", "rows": ["c1"] }));
    let asked = failure(fixture.as_agent(Act::ID, forward.clone()).unwrap_err());
    let token = asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned();
    fixture.call(AllowDestroy::ID, json!({ "token": token, "inputs": { "ports": "8080:80" } })).unwrap();
    let started = fixture.as_agent(Act::ID, with(forward, json!({ "confirmation": token }))).unwrap();
    let since = Instant::now();
    let mut text = String::new();
    while !text.contains("menu=") && since.elapsed() < Duration::from_secs(20) {
        text = fixture.output(&started);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(text.contains("[--address]\n[127.0.0.1]\n[c1]\n[8080:80]"), "{text}");
}

#[test]
fn an_input_is_only_ever_a_whole_argument_it_declares() {
    let action = ToolAction {
        inputs: vec![zen_syntax::ToolInput { id: "n".to_owned(), ..zen_syntax::ToolInput::default() }],
        ..ToolAction::default()
    };
    let typed: Inputs = [("n".to_owned(), "3".to_owned())].into_iter().collect();
    let fill = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        inputs::fill(args.clone(), &args, &action, &typed)
    };
    assert_eq!(fill(&["--replicas", "${input.n}"]).unwrap(), ["--replicas", "3"]);
    assert!(fill(&["--replicas=${input.n}"]).is_err());
    assert!(fill(&["${input.other}"]).is_err());
}
