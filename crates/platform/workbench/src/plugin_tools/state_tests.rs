use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_syntax::{
    Expansion, Feature, IndexDeclaration, IndexNode, IndexStateDeclaration, LineFold, OutlineItem, Outliner,
    ProjectIndex, Suggestions, TextFeatures, ToolAction, ToolActionKind, ToolCommand, ToolRow, ToolSection,
    ToolWindowDeclaration, WindowValue,
};

use crate::plugin_runs::tests::{Fixture, fixture};

use super::*;

pub(crate) struct Fleet;

fn text(en: &str) -> BTreeMap<String, String> {
    [("en".to_owned(), en.to_owned())].into_iter().collect()
}

fn kubectl(args: &[&str]) -> ToolCommand {
    ToolCommand { program: "kubectl".to_owned(), args: args.iter().map(|arg| (*arg).to_owned()).collect() }
}

impl Outliner for Fleet {
    fn handles(&self, _file_name: &str) -> bool {
        false
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Fleet {
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
        Some("fleet")
    }

    fn runs(&self) -> Vec<String> {
        vec!["kubectl".to_owned()]
    }

    fn index_declaration(&self) -> Option<IndexDeclaration> {
        Some(IndexDeclaration { files: vec!["*.yaml".to_owned()] })
    }

    fn index(&self, input: &[u8]) -> Option<ProjectIndex> {
        let input: Value = serde_json::from_slice(input).ok()?;
        let nodes = input["files"]
            .as_array()?
            .iter()
            .map(|file| {
                let path = file["path"].as_str().unwrap_or_default();
                IndexNode {
                    id: format!("n:{path}"),
                    kind: "Kustomization".to_owned(),
                    name: path.to_owned(),
                    path: path.to_owned(),
                    end: file["text"].as_str().unwrap_or_default().len(),
                    ..IndexNode::default()
                }
            })
            .collect();
        Some(ProjectIndex { kinds: vec!["fleet".to_owned()], nodes, ..ProjectIndex::default() })
    }

    fn tool_windows(&self) -> Vec<ToolWindowDeclaration> {
        let scope = ["--context", "${window.kube.context}"];
        let args = |rest: &[&str]| scope.iter().chain(rest).map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let columns: Vec<String> = ["revision", "name", "namespace", "selector"].map(str::to_owned).to_vec();
        vec![ToolWindowDeclaration {
            id: "fleet".to_owned(),
            title: text("Fleet"),
            sections: vec![ToolSection {
                id: "resources".to_owned(),
                title: text("Resources"),
                rows: zen_syntax::INDEX_ROWS.to_owned(),
                actions: vec![ToolAction {
                    id: "events".to_owned(),
                    title: text("Show Events"),
                    program: "kubectl".to_owned(),
                    args: args(&[
                        "get",
                        "events",
                        "--namespace",
                        "${row.namespace}",
                        "--field-selector",
                        "${row.selector}",
                    ]),
                    kind: ToolActionKind::Stream,
                    rows: true,
                    ..ToolAction::default()
                }],
                state: Some(IndexStateDeclaration {
                    columns,
                    column_titles: [("revision".to_owned(), text("Revision"))].into_iter().collect(),
                    lists: vec![
                        kubectl(&args(&["get", "flux"]).iter().map(String::as_str).collect::<Vec<_>>()),
                        kubectl(&["--context", "${window.kube.context}", "get", "argo"]),
                    ],
                }),
                ..ToolSection::default()
            }],
            window_values: vec![WindowValue {
                plugin: "kube".to_owned(),
                window: "kube".to_owned(),
                choice: "context".to_owned(),
            }],
            ..ToolWindowDeclaration::default()
        }]
    }

    fn tool_rows(&self, section: &str, output: &str) -> Option<Vec<ToolRow>> {
        assert_eq!(section, "state:resources");
        let rows = output.lines().filter(|line| !line.trim().is_empty()).map(|line| {
            let parts: Vec<String> = line.split(' ').map(str::to_owned).collect();
            ToolRow { id: parts[0].clone(), state: Some(parts[1].clone()), columns: parts[2..].to_vec() }
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
    "  *'get namespaces') echo default; echo team; exit 0 ;;\n",
    "  *'get pods') echo \"web-$ctx Running\"; exit 0 ;;\n",
    "esac\n",
    "if [ -f \"$here/down\" ]; then echo 'Unable to connect to the server' >&2; exit 1; fi\n",
    "case \"$*\" in\n",
    "  *'get flux') echo \"n:compose.yaml ok $ctx web flux-system involvedObject.name=web\"; exit 0 ;;\n",
    "  *'get argo') echo \"error: the server doesn't have a resource type applications\" >&2; exit 1 ;;\n",
    "esac\n",
    "for arg in \"$@\"; do echo \"[$arg]\"; done\n",
);

fn fleet_fixture(trusted: bool) -> Fixture {
    let mut fixture = fixture(trusted);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let kubectl = fixture.bin.join("kubectl");
        std::fs::write(&kubectl, KUBECTL).unwrap();
        std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(fixture.bin.join("contexts"), "dev team *\nprod default\n").unwrap();
    fixture
        .session
        .state_mut::<crate::project_index::Indexes>()
        .set_plugins(Arc::new(|| vec![Arc::new(Fleet) as Arc<dyn TextFeatures>]));
    fixture.call("plugins.index.get", json!({ "waitMs": 10000 })).unwrap();
    fixture
}

fn state(fixture: &mut Fixture, extra: Value) -> Value {
    let mut params = json!({ "plugin": "fleet", "window": "fleet", "section": "resources", "waitMs": 10000 });
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    fixture.call(ListRows::ID, params).unwrap()
}

fn node(answer: &Value) -> &Value {
    answer["rows"].as_array().unwrap().iter().find(|row| row["id"] == "n:compose.yaml").unwrap()
}

fn calls(fixture: &Fixture) -> Vec<String> {
    std::fs::read_to_string(fixture.bin.join("kubecalls")).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn an_index_s_nodes_get_their_cluster_state_in_the_context_of_another_plug_in_s_window() {
    let mut fixture = fleet_fixture(true);
    let answer = state(&mut fixture, json!({}));
    let row = node(&answer);
    assert_eq!(row["cluster"]["state"], "ok", "{answer}");
    assert_eq!(row["cluster"]["columns"], json!(["dev", "web", "flux-system", "involvedObject.name=web"]));
    assert_eq!(row["columns"][1], "Kustomization", "the index's own columns stay");
    let cluster = &answer["cluster"];
    assert_eq!(cluster["from"], "State from dev");
    assert_eq!(cluster["values"], json!({ "kube.context": "dev" }));
    assert_eq!(cluster["columns"][0], json!({ "id": "revision", "title": "Revision" }));
    let errors = cluster["errors"].to_string();
    assert!(errors.contains("resource type"), "one listing may fail alone: {cluster}");
    assert!(calls(&fixture).contains(&"--context|dev|get|flux|".to_owned()), "{:?}", calls(&fixture));
}

#[test]
fn a_node_s_action_passes_the_listed_values_as_whole_arguments() {
    let mut fixture = fleet_fixture(true);
    state(&mut fixture, json!({}));
    let act = json!({
        "plugin": "fleet", "window": "fleet", "section": "resources", "action": "events",
        "rows": ["n:compose.yaml"],
    });
    let started = fixture.call(Act::ID, act).unwrap();
    let run = json!({ "runId": started["runId"], "waitMs": 10000 });
    let mut printed = String::new();
    for _ in 0..200 {
        printed = fixture.call("run.output.get", run.clone()).unwrap().to_string();
        if printed.contains("[involvedObject.name=web]") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for arg in ["[--context]", "[dev]", "[--namespace]", "[flux-system]", "[--field-selector]"] {
        assert!(printed.contains(arg), "{arg}: {printed}");
    }
    let unknown = json!({
        "plugin": "fleet", "window": "fleet", "section": "resources", "action": "events", "rows": ["n:never-listed"],
    });
    assert!(fixture.call(Act::ID, unknown).is_err(), "only rows the platform listed");
}

#[test]
fn another_context_in_the_kubernetes_window_drops_the_old_state_at_once() {
    let mut fixture = fleet_fixture(true);
    fixture.call(ListRows::ID, json!({ "plugin": "kube", "window": "kube", "section": "pods" })).unwrap();
    assert_eq!(node(&state(&mut fixture, json!({})))["cluster"]["columns"][0], "dev");
    fixture
        .call(Choose::ID, json!({ "plugin": "kube", "window": "kube", "choice": "context", "value": "prod" }))
        .unwrap();
    let at_once = state(&mut fixture, json!({ "waitMs": 0 }));
    let shown = &node(&at_once)["cluster"];
    assert!(shown.is_null() || shown["columns"][0] == "prod", "never dev's state under prod: {at_once}");
    let act = json!({
        "plugin": "fleet", "window": "fleet", "section": "resources", "action": "events", "rows": ["n:compose.yaml"],
    });
    let after = state(&mut fixture, json!({}));
    assert_eq!(after["cluster"]["from"], "State from prod", "{after}");
    assert_eq!(node(&after)["cluster"]["columns"][0], "prod");
    assert!(fixture.call(Act::ID, act).is_ok());
}

#[test]
fn without_a_cluster_the_graph_stays_and_says_why_and_waits_two_minutes() {
    let mut fixture = fleet_fixture(true);
    std::fs::write(fixture.bin.join("down"), "").unwrap();
    let answer = state(&mut fixture, json!({}));
    assert_eq!(answer["rows"].as_array().unwrap().len(), 1, "the graph without state: {answer}");
    assert!(node(&answer)["cluster"].is_null());
    assert!(answer["cluster"]["errors"].to_string().contains("Unable to connect"), "{answer}");
    assert!(answer["cluster"]["retryAfterMs"].as_u64().unwrap() > 60_000);
    let before = calls(&fixture).len();
    state(&mut fixture, json!({ "refresh": true }));
    assert_eq!(calls(&fixture).len(), before, "no listing again within the back-off");
    state(&mut fixture, json!({ "refresh": true, "force": true }));
    assert!(calls(&fixture).len() > before, "Refresh lists at once");
}

#[test]
fn an_untrusted_project_shows_the_graph_and_runs_nothing() {
    let mut fixture = fleet_fixture(false);
    let answer = state(&mut fixture, json!({}));
    assert_eq!(answer["rows"].as_array().unwrap().len(), 1);
    assert!(answer["cluster"]["errors"].to_string().contains("not trusted"), "{answer}");
    assert!(calls(&fixture).is_empty());
}
