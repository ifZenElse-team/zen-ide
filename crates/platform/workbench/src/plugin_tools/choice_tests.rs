use std::collections::BTreeMap;

use serde_json::{Value, json};
use zen_syntax::{
    Expansion, Feature, LineFold, OutlineItem, Outliner, Suggestions, TextFeatures, ToolAction, ToolActionKind,
    ToolChoice, ToolCommand, ToolRow, ToolSection, ToolWindowDeclaration,
};

use crate::plugin_runs::tests::{Fixture, code, failure, fixture};

use super::*;

pub(crate) struct Kube;

fn text(en: &str) -> BTreeMap<String, String> {
    [("en".to_owned(), en.to_owned())].into_iter().collect()
}

fn command(args: &[&str]) -> ToolCommand {
    ToolCommand { program: "kubectl".to_owned(), args: args.iter().map(|arg| (*arg).to_owned()).collect() }
}

const SCOPE: [&str; 4] = ["--context", "${window.context}", "--namespace", "${window.namespace}"];

fn scoped(args: &[&str]) -> Vec<String> {
    SCOPE.iter().chain(args).map(|arg| (*arg).to_owned()).collect()
}

impl Outliner for Kube {
    fn handles(&self, _file_name: &str) -> bool {
        false
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Kube {
    fn supports(&self, _feature: Feature) -> bool {
        false
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }

    fn plugin_id(&self) -> Option<&str> {
        Some("kube")
    }

    fn runs(&self) -> Vec<String> {
        vec!["kubectl".to_owned()]
    }

    fn tool_windows(&self) -> Vec<ToolWindowDeclaration> {
        let action = |id: &str, kind, args: &[&str], confirm: &str| ToolAction {
            id: id.to_owned(),
            title: text(id),
            program: "kubectl".to_owned(),
            args: scoped(args),
            kind,
            rows: true,
            confirm: if confirm.is_empty() { BTreeMap::new() } else { text(confirm) },
            inputs: Vec::new(),
        };
        vec![ToolWindowDeclaration {
            id: "kube".to_owned(),
            title: text("Kube"),
            icon: "kubernetes".to_owned(),
            side: String::new(),
            choices: vec![
                ToolChoice {
                    id: "context".to_owned(),
                    title: text("Context"),
                    columns: vec!["name".to_owned(), "namespace".to_owned()],
                    list: command(&["config", "view"]),
                    initial_from: String::new(),
                    source: Some(zen_syntax::ToolSource {
                        variable: "ZEN_TEST_KUBECONFIG".to_owned(),
                        default: "~/.kube/config".to_owned(),
                    }),
                },
                ToolChoice {
                    id: "namespace".to_owned(),
                    title: text("Namespace"),
                    columns: vec!["name".to_owned()],
                    list: command(&["--context", "${window.context}", "get", "namespaces"]),
                    initial_from: "context.namespace".to_owned(),
                    source: None,
                },
            ],
            sections: vec![ToolSection {
                id: "pods".to_owned(),
                title: text("Pods"),
                columns: vec!["name".to_owned(), "status".to_owned()],
                column_titles: BTreeMap::new(),
                list: ToolCommand { program: "kubectl".to_owned(), args: scoped(&["get", "pods"]) },
                actions: vec![
                    action(
                        "restart",
                        ToolActionKind::Change,
                        &["restart", "${row.id}"],
                        "Restart ${rows} in ${window.context}?",
                    ),
                    action(
                        "delete",
                        ToolActionKind::Destroy,
                        &["delete", "${row.id}"],
                        "Delete ${rows} from ${window.context}?",
                    ),
                    action("logs", ToolActionKind::Stream, &["logs", "${row.id}"], ""),
                    ToolAction {
                        inputs: vec![zen_syntax::ToolInput {
                            id: "replicas".to_owned(),
                            title: text("Replicas"),
                            kind: zen_syntax::ToolInputKind::Count,
                            ..zen_syntax::ToolInput::default()
                        }],
                        ..action(
                            "scale",
                            ToolActionKind::Change,
                            &["scale", "${row.id}", "--replicas", "${input.replicas}"],
                            "Scale ${rows} in ${window.context} to ${input.replicas}?",
                        )
                    },
                ],
                rows: String::new(),
                state: None,
            }],
            window_values: Vec::new(),
        }]
    }

    fn tool_rows(&self, section: &str, output: &str) -> Option<Vec<ToolRow>> {
        let rows = output.lines().filter(|line| !line.trim().is_empty()).map(|line| {
            let parts: Vec<&str> = line.split(' ').collect();
            let current = parts.last() == Some(&"*");
            let columns: Vec<String> =
                parts.iter().filter(|part| **part != "*").map(|part| (*part).to_owned()).collect();
            let id = columns.first().cloned().unwrap_or_default();
            let state = match section {
                "choice:context" => current.then(|| "current".to_owned()),
                "choice:namespace" => (id == "default").then(|| "current".to_owned()),
                _ => columns.get(1).cloned(),
            };
            ToolRow { id, columns, state }
        });
        Some(rows.collect())
    }
}

const KUBECTL: &str = concat!(
    "#!/bin/sh\n",
    "here=${0%/*}\n",
    "printf '%s|' \"$@\" >> \"$here/kubecalls\"; echo >> \"$here/kubecalls\"\n",
    "ctx=''; prev=''\n",
    "for arg in \"$@\"; do [ \"$prev\" = --context ] && ctx=$arg; prev=$arg; done\n",
    "case \"$*\" in\n",
    "  'config view') while IFS= read -r line; do echo \"$line\"; done < \"$here/contexts\"; exit 0 ;;\n",
    "  *'get namespaces') echo default; echo team; echo shop; exit 0 ;;\n",
    "  *'get pods') echo \"web-$ctx Running\"; echo \"db-$ctx Pending\"; exit 0 ;;\n",
    "esac\n",
    "for arg in \"$@\"; do echo \"[$arg]\"; done\n",
);

fn kube_fixture() -> Fixture {
    let fixture = fixture(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let kubectl = fixture.bin.join("kubectl");
        std::fs::write(&kubectl, KUBECTL).unwrap();
        std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(fixture.bin.join("contexts"), "dev team *\nprod shop\n").unwrap();
    fixture
}

fn pods() -> Value {
    json!({ "plugin": "kube", "window": "kube", "section": "pods" })
}

fn with(mut params: Value, extra: Value) -> Value {
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    params
}

fn calls(fixture: &Fixture) -> Vec<String> {
    std::fs::read_to_string(fixture.bin.join("kubecalls")).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn rows(fixture: &mut Fixture, extra: Value) -> Value {
    fixture.call(ListRows::ID, with(pods(), extra)).unwrap()
}

fn choice<'a>(answer: &'a Value, id: &str) -> &'a Value {
    answer["choices"].as_array().unwrap().iter().find(|choice| choice["id"] == id).unwrap()
}

fn ids(answer: &Value) -> Vec<String> {
    answer["rows"].as_array().unwrap().iter().map(|row| row["id"].as_str().unwrap().to_owned()).collect()
}

fn choose(fixture: &mut Fixture, choice: &str, value: &str) -> Result<Value, zen_core::DispatchError> {
    fixture.call(Choose::ID, json!({ "plugin": "kube", "window": "kube", "choice": choice, "value": value }))
}

#[test]
fn the_choices_start_from_the_current_values_and_every_listing_passes_them() {
    let mut fixture = kube_fixture();
    let answer = rows(&mut fixture, json!({}));
    assert_eq!(choice(&answer, "context")["value"], "dev", "{answer}");
    assert_eq!(choice(&answer, "namespace")["value"], "team", "the context's own namespace");
    assert_eq!(choice(&answer, "context")["options"][1], json!({ "id": "prod", "label": "prod" }));
    let source = choice(&answer, "context")["source"][0]["path"].as_str().unwrap().to_owned();
    assert!(source.ends_with("/.kube/config") && !source.starts_with('~'), "the default, in the home folder: {source}");
    assert!(choice(&answer, "namespace").get("source").is_none());
    assert_eq!(ids(&answer), ["web-dev", "db-dev"]);
    assert_eq!(
        calls(&fixture),
        ["config|view|", "--context|dev|get|namespaces|", "--context|dev|--namespace|team|get|pods|"]
    );
    let listed = fixture.as_agent(ListWindows::ID, json!({})).unwrap();
    let kube = listed["windows"].as_array().unwrap().iter().find(|window| window["plugin"] == "kube").unwrap();
    assert_eq!(kube["choices"][0]["id"], "context");
    assert_eq!(kube["sections"][0]["actions"][0]["confirms"], true);
}

#[test]
fn choosing_is_the_person_s_and_drops_the_rows_listed_under_the_old_values() {
    let mut fixture = kube_fixture();
    rows(&mut fixture, json!({}));
    let params = json!({ "plugin": "kube", "window": "kube", "choice": "context", "value": "prod" });
    assert_eq!(code(fixture.as_agent(Choose::ID, params).unwrap_err()), crate::trust::AGENT_NOT_ALLOWED);
    assert!(failure(choose(&mut fixture, "context", "staging").unwrap_err()).message.contains("staging"));
    choose(&mut fixture, "context", "prod").unwrap();
    let old = with(pods(), json!({ "action": "restart", "rows": ["web-dev"], "confirmed": true, "waitMs": 10000 }));
    assert!(fixture.call(Act::ID, old).is_err(), "a row listed in the old context is never acted on");
    let answer = rows(&mut fixture, json!({}));
    assert_eq!(choice(&answer, "context")["value"], "prod");
    assert_eq!(choice(&answer, "namespace")["value"], "shop", "the namespace starts again from the new context");
    assert_eq!(ids(&answer), ["web-prod", "db-prod"]);
    choose(&mut fixture, "namespace", "default").unwrap();
    rows(&mut fixture, json!({}));
    assert_eq!(calls(&fixture).last().unwrap(), "--context|prod|--namespace|default|get|pods|");
    assert!(calls(&fixture).iter().all(|call| !call.contains("use-context") && !call.contains("set-context")));
}

#[test]
fn a_context_gone_from_the_kubeconfig_stops_listing_and_actions_until_the_person_chooses() {
    let mut fixture = kube_fixture();
    rows(&mut fixture, json!({}));
    choose(&mut fixture, "context", "prod").unwrap();
    rows(&mut fixture, json!({}));
    std::fs::write(fixture.bin.join("contexts"), "dev team *\n").unwrap();
    let answer = rows(&mut fixture, json!({ "refresh": true, "force": true }));
    assert!(answer["error"].as_str().unwrap().contains("prod"), "{answer}");
    assert_eq!(choice(&answer, "context")["valid"], false);
    let restart =
        with(pods(), json!({ "action": "restart", "rows": ["web-prod"], "confirmed": true, "waitMs": 10000 }));
    assert_eq!(code(fixture.call(Act::ID, restart).unwrap_err()), CHOICE_NEEDED);
    assert!(!calls(&fixture).iter().any(|call| call.contains("restart")));
    choose(&mut fixture, "context", "dev").unwrap();
    let answer = rows(&mut fixture, json!({}));
    assert_eq!(ids(&answer), ["web-dev", "db-dev"]);
}

#[test]
fn a_confirm_text_names_the_context_and_the_rows_and_is_asked_every_time() {
    let mut fixture = kube_fixture();
    rows(&mut fixture, json!({}));
    let restart = with(pods(), json!({ "action": "restart", "rows": ["web-dev"], "waitMs": 10000 }));
    let asked = failure(fixture.call(Act::ID, restart.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(CONFIRMATION_NEEDED));
    assert_eq!(asked.message, "Restart web-dev in dev?");
    let done = fixture.call(Act::ID, with(restart.clone(), json!({ "confirmed": true }))).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    assert!(calls(&fixture).contains(&"--context|dev|--namespace|team|restart|web-dev|".to_owned()));
    fixture.call("plugins.runAccess.set", json!({ "plugin": "kube", "access": "full" })).unwrap();
    let agent = failure(fixture.as_agent(Act::ID, with(restart, json!({ "confirmed": true }))).unwrap_err());
    assert_eq!(agent.code.as_deref(), Some(CONFIRMATION_NEEDED), "even with full access the person is asked");
    let notifications = fixture.call("ui.notifications.list", json!({})).unwrap();
    assert!(notifications.to_string().contains("Restart web-dev in dev?"), "{notifications}");
    let logs = with(pods(), json!({ "action": "logs", "rows": ["db-dev"] }));
    assert!(fixture.call(Act::ID, logs).is_ok(), "an action without a confirm text is not asked");
}

#[test]
fn the_choices_are_kept_per_project() {
    let mut fixture = kube_fixture();
    let data = fixture.project.with_file_name("data");
    fixture.session.state_mut::<ToolState>().choices_file = Some(data.join(CHOICES_FILE));
    rows(&mut fixture, json!({}));
    choose(&mut fixture, "context", "prod").unwrap();
    rows(&mut fixture, json!({}));
    choose(&mut fixture, "namespace", "default").unwrap();
    let state = fixture.session.state_mut::<ToolState>();
    state.choices.clear();
    state.sections.clear();
    state.remembered = None;
    let answer = rows(&mut fixture, json!({}));
    assert_eq!(choice(&answer, "context")["value"], "prod", "{answer}");
    assert_eq!(choice(&answer, "namespace")["value"], "default");
    let saved = std::fs::read_to_string(data.join(CHOICES_FILE)).unwrap();
    assert!(saved.contains(&fixture.project.to_string_lossy().into_owned()), "{saved}");
}

#[test]
fn a_window_value_is_a_whole_argument_of_a_choice_that_has_one() {
    let values: BTreeMap<String, String> = [("context".to_owned(), "dev".to_owned())].into_iter().collect();
    let args = ["--context".to_owned(), "${window.context}".to_owned()];
    assert_eq!(filled_in(&args, None, &[], &values).unwrap(), ["--context", "dev"]);
    assert!(filled_in(&["--context=${window.context}".to_owned()], None, &[], &values).is_err());
    assert!(filled_in(&["${window.cluster}".to_owned()], None, &[], &values).is_err());
}

#[test]
fn an_index_section_lists_the_index_nodes_with_their_worst_problem() {
    let node = |id: &str, start: usize, end: usize| zen_syntax::IndexNode {
        id: id.to_owned(),
        kind: "Kustomization".to_owned(),
        name: id.to_owned(),
        namespace: "flux-system".to_owned(),
        path: "apps.yaml".to_owned(),
        start,
        end,
        ..zen_syntax::IndexNode::default()
    };
    let problem = |start: usize, severity: &str| zen_syntax::IndexProblem {
        path: "apps.yaml".to_owned(),
        start,
        end: start + 2,
        severity: severity.to_owned(),
        message: "x".to_owned(),
    };
    let answer = crate::project_index::IndexAnswer {
        plugin: "gitops".to_owned(),
        index: zen_syntax::ProjectIndex {
            nodes: vec![
                node("apps", 0, 50),
                node("infra", 51, 99),
                node("clean", 100, 150),
                zen_syntax::IndexNode { in_graph: false, ..node("key:image.tag", 10, 12) },
            ],
            problems: vec![problem(10, "warning"), problem(20, "error"), problem(60, "warning")],
            ..zen_syntax::ProjectIndex::default()
        },
        ..crate::project_index::IndexAnswer::default()
    };
    let rows = index_rows_of(vec![answer]);
    let seen: Vec<(&str, Option<&str>)> = rows.iter().map(|row| (row.id.as_str(), row.state.as_deref())).collect();
    assert_eq!(seen, [("apps", Some("error")), ("infra", Some("warning")), ("clean", None)]);
    assert_eq!(rows[0].columns, ["apps", "Kustomization", "flux-system", "apps.yaml"]);
}

#[test]
fn the_gitops_window_has_an_icon_that_passes_the_check_and_an_index_section() {
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../../plugins/gitops/plugin.json")).expect("plugin.json is JSON");
    let windows: Vec<ToolWindowDeclaration> = serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    assert_eq!(zen_image::icon::problem(&windows[0].icon), None, "drawn, not the generic icon");
    assert_eq!(windows[0].side, "right");
    assert_eq!(windows[0].sections[0].rows, zen_syntax::INDEX_ROWS);
}

#[test]
fn an_agent_s_scale_names_the_context_and_runs_only_with_the_person_s_count() {
    let mut fixture = kube_fixture();
    rows(&mut fixture, json!({}));
    fixture.call("plugins.runAccess.set", json!({ "plugin": "kube", "access": "full" })).unwrap();
    let scale = with(pods(), json!({ "action": "scale", "rows": ["web-dev"], "waitMs": 10000 }));
    let asked = failure(fixture.as_agent(Act::ID, scale.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(CONFIRMATION_NEEDED));
    let notifications = fixture.call("ui.notifications.list", json!({})).unwrap().to_string();
    assert!(notifications.contains("scale web-dev in dev / team"), "{notifications}");
    let token = asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned();
    fixture.call(AllowDestroy::ID, json!({ "token": token, "inputs": { "replicas": "4" } })).unwrap();
    let done = fixture.as_agent(Act::ID, with(scale, json!({ "confirmation": token }))).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    assert!(calls(&fixture).contains(&"--context|dev|--namespace|team|scale|web-dev|--replicas|4|".to_owned()));
}
