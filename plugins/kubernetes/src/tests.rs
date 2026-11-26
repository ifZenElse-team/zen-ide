use serde_json::{Value, json};

use super::*;

const CONFIG: &str = r#"{
  "kind": "Config", "apiVersion": "v1", "current-context": "dev",
  "clusters": [{ "name": "dev-cluster", "cluster": { "server": "https://127.0.0.1:6443", "certificate-authority-data": "DATA+OMITTED" } }],
  "users": [{ "name": "dev-admin", "user": { "token": "REDACTED" } }],
  "contexts": [
    { "name": "dev", "context": { "cluster": "dev-cluster", "user": "dev-admin", "namespace": "team" } },
    { "name": "prod", "context": { "cluster": "prod-cluster", "user": "ops" } }
  ]
}"#;

const PODS: &str = r#"{ "apiVersion": "v1", "kind": "List", "items": [
  { "metadata": { "name": "web-7d4b9", "creationTimestamp": "2026-11-13T08:00:00Z" },
    "spec": { "nodeName": "node-1", "containers": [{ "name": "web" }, { "name": "proxy" }] },
    "status": { "phase": "Running", "containerStatuses": [
      { "name": "web", "ready": true, "restartCount": 2, "state": { "running": {} } },
      { "name": "proxy", "ready": true, "restartCount": 1, "state": { "running": {} } } ] } },
  { "metadata": { "name": "worker-1", "creationTimestamp": "2026-11-13T09:00:00Z" },
    "spec": { "containers": [{ "name": "worker" }] },
    "status": { "phase": "Running", "containerStatuses": [
      { "name": "worker", "ready": false, "restartCount": 7, "state": { "waiting": { "reason": "CrashLoopBackOff" } } } ] } },
  { "metadata": { "name": "job-1", "creationTimestamp": "2026-11-12T09:00:00Z" },
    "spec": { "containers": [{ "name": "job" }] },
    "status": { "phase": "Succeeded", "containerStatuses": [
      { "name": "job", "ready": false, "restartCount": 0, "state": { "terminated": { "reason": "Completed" } } } ] } },
  { "metadata": { "name": "old-1", "deletionTimestamp": "2026-11-13T10:00:00Z" }, "spec": { "containers": [] }, "status": { "phase": "Running" } }
] }"#;

fn row(id: &str, columns: &[&str], state: Option<&str>) -> kube::Row {
    kube::Row {
        id: id.to_owned(),
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        state: state.map(str::to_owned),
    }
}

#[test]
fn contexts_come_from_the_kubeconfig_with_the_current_one_marked() {
    assert_eq!(
        kube::rows("choice:context", CONFIG),
        [
            row("dev", &["dev", "dev-cluster", "dev-admin", "team"], Some("current")),
            row("prod", &["prod", "prod-cluster", "ops", ""], None),
        ]
    );
    assert_eq!(kube::rows("contexts", CONFIG), kube::rows("choice:context", CONFIG));
    assert!(kube::rows("contexts", "error: no configuration").is_empty());
}

#[test]
fn namespaces_mark_default_only_as_a_choice() {
    let listed = r#"{ "items": [
        { "metadata": { "name": "default", "creationTimestamp": "2026-01-01T00:00:00Z" }, "status": { "phase": "Active" } },
        { "metadata": { "name": "team" }, "status": { "phase": "Terminating" } } ] }"#;
    assert_eq!(
        kube::rows("choice:namespace", listed),
        [
            row("default", &["default", "Active", "2026-01-01T00:00:00Z"], Some("current")),
            row("team", &["team", "Terminating", ""], None)
        ]
    );
    assert_eq!(kube::rows("namespaces", listed)[0].state, None);
}

#[test]
fn pods_read_ready_containers_restarts_and_what_they_wait_for() {
    assert_eq!(
        kube::rows("pods", PODS),
        [
            row(
                "pod/web-7d4b9",
                &["web-7d4b9", "2/2", "Running", "3", "node-1", "2026-11-13T08:00:00Z"],
                Some("running")
            ),
            row(
                "pod/worker-1",
                &["worker-1", "0/1", "CrashLoopBackOff", "7", "", "2026-11-13T09:00:00Z"],
                Some("dead")
            ),
            row("pod/job-1", &["job-1", "0/1", "Succeeded", "0", "", "2026-11-12T09:00:00Z"], Some("exited")),
            row("pod/old-1", &["old-1", "0/0", "Terminating", "0", "", ""], Some("restarting")),
        ]
    );
}

#[test]
fn deployments_services_and_config_maps_are_read_as_kubectl_prints_them() {
    let deployments = r#"{ "items": [
        { "metadata": { "name": "api" }, "spec": { "replicas": 3 }, "status": { "readyReplicas": 2, "updatedReplicas": 3, "availableReplicas": 2 } },
        { "metadata": { "name": "idle" }, "spec": { "replicas": 0 }, "status": {} } ] }"#;
    assert_eq!(
        kube::rows("deployments", deployments),
        [
            row("deployment/api", &["api", "2/3", "3", "2", ""], Some("starting")),
            row("deployment/idle", &["idle", "0/0", "0", "0", ""], Some("exited"))
        ]
    );
    let services = r#"{ "items": [ { "metadata": { "name": "web" }, "spec": { "type": "NodePort", "clusterIP": "10.0.0.7",
        "ports": [ { "port": 80, "protocol": "TCP", "nodePort": 30080 }, { "port": 443, "protocol": "TCP" } ] } } ] }"#;
    assert_eq!(
        kube::rows("services", services),
        [row("service/web", &["web", "NodePort", "10.0.0.7", "80:30080/TCP,443/TCP", ""], None)]
    );
    let maps = r#"{ "items": [ { "metadata": { "name": "settings" }, "data": { "a": "1", "b": "2" }, "binaryData": { "c": "AA==" } } ] }"#;
    assert_eq!(kube::rows("configmaps", maps), [row("configmap/settings", &["settings", "3", ""], None)]);
}

#[test]
fn secrets_are_names_only() {
    let listed = "secret/db-password\nsecret/tls.web\n\nsecret/Bad Name\nnot-a-secret\n";
    assert_eq!(
        kube::rows("secrets", listed),
        [row("secret/db-password", &["db-password"], None), row("secret/tls.web", &["tls.web"], None)]
    );
}

#[test]
fn events_are_newest_first_with_warnings_marked() {
    let listed = r#"{ "items": [
        { "metadata": { "name": "e1" }, "lastTimestamp": "2026-11-13T08:00:00Z", "type": "Normal", "reason": "Pulled",
          "involvedObject": { "kind": "Pod", "name": "web" }, "message": "Pulled the image" },
        { "metadata": { "name": "e2" }, "eventTime": "2026-11-13T09:00:00Z", "type": "Warning", "reason": "BackOff",
          "involvedObject": { "kind": "Pod", "name": "worker" }, "message": "Back-off restarting" } ] }"#;
    assert_eq!(
        kube::rows("events", listed),
        [
            row(
                "e2",
                &["2026-11-13T09:00:00Z", "Warning", "BackOff", "pod/worker", "Back-off restarting"],
                Some("unhealthy")
            ),
            row("e1", &["2026-11-13T08:00:00Z", "Normal", "Pulled", "pod/web", "Pulled the image"], None),
        ]
    );
}

#[test]
fn the_module_answers_abi_1_so_that_the_host_calls_its_rows() {
    assert_eq!(zen_abi(), 1);
}

#[test]
fn the_rows_are_json_for_the_host() {
    let answer = tool_rows_request(
        b"pods\0{\"items\":[{\"metadata\":{\"name\":\"a\\\"b\"},\"status\":{\"phase\":\"Pending\"}}]}",
    );
    let rows: Value = serde_json::from_slice(&answer).unwrap();
    assert_eq!(rows[0]["id"], "pod/a\"b");
    assert_eq!(rows[0]["state"], "starting");
    assert_eq!(tool_rows_request(b"no separator"), b"[]");
    assert_eq!(tool_rows_request(b"unknown\0{}"), b"[]");
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../plugin.json")).unwrap()
}

fn args(command: &Value) -> Vec<String> {
    command["args"].as_array().unwrap().iter().map(|arg| arg.as_str().unwrap().to_owned()).collect()
}

#[test]
fn every_command_runs_kubectl_in_the_window_s_context_and_never_writes_the_kubeconfig() {
    let manifest = manifest();
    assert_eq!(manifest["runs"], json!(["kubectl"]));
    let window = &manifest["toolWindows"][0];
    let choices: Vec<&str> = window["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(choices, ["context", "namespace"]);
    let mut commands: Vec<(String, Value)> = Vec::new();
    for choice in window["choices"].as_array().unwrap() {
        commands.push((format!("choice {}", choice["id"].as_str().unwrap()), choice["list"].clone()));
    }
    for section in window["sections"].as_array().unwrap() {
        commands.push((format!("{} list", section["id"].as_str().unwrap()), section["list"].clone()));
        for action in section["actions"].as_array().unwrap() {
            commands.push((
                format!("{} {}", section["id"].as_str().unwrap(), action["id"].as_str().unwrap()),
                action.clone(),
            ));
        }
    }
    for (name, command) in &commands {
        assert_eq!(command["program"], "kubectl", "{name}");
        let args = args(command);
        for forbidden in ["use-context", "set-context", "set-cluster", "set-credentials", "--raw", "--kubeconfig"] {
            assert!(!args.iter().any(|arg| arg == forbidden), "{name} has {forbidden}");
        }
        let reads_config = args.first().map(String::as_str) == Some("config");
        if reads_config {
            assert_eq!(args, ["config", "view", "-o", "json"], "{name}: the kubeconfig is only read");
            continue;
        }
        let at = args.iter().position(|arg| arg == "--context").unwrap_or_else(|| panic!("{name} has no --context"));
        assert_eq!(args[at + 1], "${window.context}", "{name}");
        if !name.starts_with("choice ") && !name.starts_with("namespaces") {
            let at = args.iter().position(|arg| arg == "--namespace").unwrap_or_else(|| panic!("{name}: --namespace"));
            assert_eq!(args[at + 1], "${window.namespace}", "{name}");
        }
        for arg in &args {
            if arg.contains("${") {
                assert!(arg.starts_with("${") && arg.ends_with('}') && arg.matches("${").count() == 1, "{name}: {arg}");
            }
        }
    }
}

#[test]
fn every_change_and_destroy_names_the_context_in_its_question() {
    let manifest = manifest();
    let mut asked = 0;
    for section in manifest["toolWindows"][0]["sections"].as_array().unwrap() {
        for action in section["actions"].as_array().unwrap() {
            let kind = action["kind"].as_str().unwrap();
            if kind != "change" && kind != "destroy" {
                continue;
            }
            asked += 1;
            for language in ["en", "ru"] {
                let question = action["confirm"][language].as_str().unwrap_or_default();
                assert!(
                    question.contains("${window.context}") && question.contains("${rows}"),
                    "{}: {question}",
                    action["id"]
                );
            }
        }
    }
    assert!(asked >= 5, "restart and every delete ask");
}

#[test]
fn secrets_are_listed_by_name_only_and_never_read_back() {
    let manifest = manifest();
    let sections = manifest["toolWindows"][0]["sections"].as_array().unwrap();
    let secrets = sections.iter().find(|section| section["id"] == "secrets").unwrap();
    let list = args(&secrets["list"]);
    assert!(list.windows(2).any(|pair| pair == ["-o", "name"]), "{list:?}");
    for action in secrets["actions"].as_array().unwrap() {
        let args = args(action);
        assert!(!args.iter().any(|arg| arg == "get" || arg == "-o" || arg.starts_with("--output")), "{args:?}");
    }
}

#[test]
fn every_title_is_in_english_and_russian() {
    let manifest = manifest();
    let window = &manifest["toolWindows"][0];
    let mut titles = vec![&window["title"]];
    for choice in window["choices"].as_array().unwrap() {
        titles.push(&choice["title"]);
    }
    for section in window["sections"].as_array().unwrap() {
        titles.push(&section["title"]);
        for column in section["columns"].as_array().unwrap() {
            titles.push(&section["columnTitles"][column.as_str().unwrap()]);
        }
        for action in section["actions"].as_array().unwrap() {
            titles.push(&action["title"]);
        }
    }
    for title in titles {
        assert!(title["en"].is_string() && title["ru"].is_string(), "{title}");
    }
}

fn targets(path: &str, text: &str) -> Value {
    serde_json::from_slice(&run_targets_request(format!("{path}\0{text}").as_bytes())).unwrap()
}

#[test]
fn a_manifest_offers_diff_and_apply_in_the_window_s_context_on_its_first_api_version() {
    let manifest = "# the web tier\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n---\napiVersion: v1\nkind: Service\n";
    let offered = targets("deploy/web.yaml", manifest);
    let ids: Vec<&str> = offered.as_array().unwrap().iter().map(|target| target["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["diff", "apply"]);
    assert!(offered.as_array().unwrap().iter().all(|target| target["line"] == 1));
    let diff = &offered[0];
    assert_eq!(
        diff["args"],
        json!([
            "--context",
            "${window.kubernetes.context}",
            "--namespace",
            "${window.kubernetes.namespace}",
            "diff",
            "-f",
            "${file}"
        ])
    );
    assert_eq!(diff["output"], "diff");
    assert_eq!(diff["env"]["KUBECTL_EXTERNAL_DIFF"], "diff -N -U1000000", "full context, so both sides are whole");
    assert_eq!(offered[1]["args"][4], "apply");
    assert!(offered[1].get("output").is_none());
    let json_manifest = "{\n  \"apiVersion\": \"v1\",\n  \"kind\": \"ConfigMap\"\n}\n";
    assert_eq!(targets("settings.json", json_manifest)[0]["line"], 1);
}

#[test]
fn other_yaml_and_json_files_and_empty_folders_offer_nothing() {
    assert_eq!(targets(".gitlab-ci.yml", "stages: [build]\nbuild:\n  script: make\n"), json!([]));
    assert_eq!(targets("package.json", "{\n  \"name\": \"web\"\n}\n"), json!([]));
    assert_eq!(targets("values.yaml", "kind: none\n  apiVersion: nested\n"), json!([]), "apiVersion at the top only");
    assert_eq!(targets("deploy/", ""), json!([]));
    assert_eq!(targets("/", "a.yaml\n"), json!([]), "never the whole project");
}

#[test]
fn a_folder_of_manifests_offers_diff_and_apply_recursively() {
    let offered = targets("deploy/", "deploy/web.yaml\ndeploy/db/db.yaml\n");
    let names: Vec<&str> = offered.as_array().unwrap().iter().map(|target| target["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Diff Folder with Cluster", "Apply Folder to Cluster"]);
    assert_eq!(offered[0]["args"].as_array().unwrap().last().unwrap(), "-R");
    assert_eq!(offered[1]["args"][4], "apply");
}

#[test]
fn every_kubectl_command_that_changes_a_cluster_is_the_person_s_and_diff_is_not() {
    let manifest = manifest();
    assert_eq!(manifest["targetFiles"], json!(["*.yaml", "*.yml", "*.json"]));
    let rules = manifest["personOnly"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["program"], "kubectl");
    let listed: Vec<&str> =
        rules[0]["subcommands"].as_array().unwrap().iter().map(|word| word.as_str().unwrap()).collect();
    assert_eq!(listed, manifests::PERSON_ONLY);
    for reading in ["diff", "get", "describe", "logs", "config"] {
        assert!(!listed.contains(&reading), "{reading} only reads");
    }
}

#[test]
fn the_contexts_name_the_kubeconfig_files_they_come_from() {
    let manifest = manifest();
    let context = &manifest["toolWindows"][0]["choices"][0];
    assert_eq!(context["id"], "context");
    assert_eq!(context["source"], json!({ "variable": "KUBECONFIG", "default": "~/.kube/config" }));
}

#[test]
fn a_port_forward_binds_to_loopback_and_a_scale_names_the_context_the_rows_and_the_count() {
    let manifest = manifest();
    let sections = manifest["toolWindows"][0]["sections"].as_array().unwrap();
    let action = |section: &str, id: &str| -> Value {
        let section = sections.iter().find(|s| s["id"] == section).unwrap();
        section["actions"].as_array().unwrap().iter().find(|a| a["id"] == id).cloned().unwrap()
    };
    for section in sections {
        for declared in section["actions"].as_array().unwrap() {
            let forwards = declared["inputs"].as_array().into_iter().flatten().any(|input| input["kind"] == "forward");
            if forwards {
                let args = args(declared);
                let at = args.iter().position(|arg| arg == "--address").expect("a forward names its address");
                assert_eq!(args[at + 1], "127.0.0.1", "{}: loopback only", section["id"]);
                assert_eq!(declared["kind"], "stream", "Stop ends a forward");
            }
        }
    }
    for section in ["pods", "services"] {
        let forward = action(section, "forward");
        assert_eq!(args(&forward).last().map(String::as_str), Some("${input.ports}"));
    }
    assert_eq!(action("services", "forward")["inputs"][0]["defaultFrom"], "ports");
    let scale = action("deployments", "scale");
    assert_eq!(scale["kind"], "change");
    assert_eq!(
        scale["inputs"][0],
        json!({ "id": "replicas", "title": { "en": "Replicas", "ru": "Реплики" }, "kind": "count" })
    );
    for language in ["en", "ru"] {
        let question = scale["confirm"][language].as_str().unwrap();
        for named in ["${rows}", "${window.context}", "${window.namespace}", "${input.replicas}"] {
            assert!(question.contains(named), "{language}: {named}");
        }
    }
}
