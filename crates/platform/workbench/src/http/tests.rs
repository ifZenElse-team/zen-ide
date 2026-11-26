use serde_json::{Value, json};
use zen_http::test_server::TestServer;

use super::*;

const IDS: [&str; 5] =
    ["http.requests.list", "http.environment.select", "http.request.run", "http.responses.list", "http.response.get"];

struct Setup {
    registry: Registry,
    session: Session,
    project: tempfile::TempDir,
    data: tempfile::TempDir,
}

fn setup() -> Setup {
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
    session.state_mut::<HttpClient>().set_data_dir(data.path().to_owned());
    Setup { registry, session, project, data }
}

impl Setup {
    fn dispatch(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.registry.dispatch(&mut self.session, id, params).map_err(|error| error.to_string())
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.project.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }
}

fn files_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

#[test]
fn every_http_command_is_registered_with_three_segments() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    for id in IDS {
        assert_eq!(id.split('.').count(), 3, "{id}");
        assert!(registry.commands().any(|info| info.id.as_str() == id), "{id} is not registered");
    }
}

#[test]
fn listing_opening_and_selecting_send_nothing() {
    let mut setup = setup();
    let server = TestServer::start(vec![TestServer::json(200, "{}")]);
    let file = setup.write(
        "api.http",
        &format!(
            "@base = {}\n\n### List\nGET {{{{base}}}}/items\n\n###\n# @name create\nPOST {{{{base}}}}/items\nContent-Type: application/json\n\n{{\"a\": 1}}\n",
            server.url("")
        ),
    );
    setup.write("http-client.env.json", r#"{"dev": {"token": "a"}, "prod": {"token": "b"}}"#);
    setup.dispatch("file.document.open", json!({ "path": file })).unwrap();
    let listed = setup.dispatch("http.requests.list", json!({ "path": file })).unwrap();
    assert_eq!(listed["requests"].as_array().unwrap().len(), 2);
    assert_eq!(listed["requests"][0]["name"], "List");
    assert_eq!(listed["requests"][0]["line"], 4, "lines count from 1, as everywhere an agent reads them");
    assert_eq!(listed["requests"][0]["startLine"], 4);
    assert_eq!(listed["requests"][1]["name"], "create");
    assert_eq!(listed["requests"][1]["method"], "POST");
    assert_eq!(listed["variables"][0]["name"], "base");
    assert_eq!(listed["environments"], json!(["dev", "prod"]));
    assert_eq!(listed["env"], Value::Null);
    assert_eq!(setup.dispatch("http.environment.select", json!({ "env": "dev" })).unwrap()["env"], "dev");
    assert_eq!(setup.dispatch("http.requests.list", json!({ "path": file })).unwrap()["env"], "dev");
    assert_eq!(setup.dispatch("http.responses.list", json!({})).unwrap()["responses"], json!([]));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(server.connections(), 0, "nothing is sent until a request is run");
    assert!(!setup.data.path().join("http-client").exists());
}

#[test]
fn runs_get_and_post_against_a_local_server_and_keeps_the_history_outside_the_project() {
    let mut setup = setup();
    let server = TestServer::start(vec![
        TestServer::json(200, "{\"items\":[{\"id\":1}]}"),
        TestServer::reply(201, "Created", &[("Content-Type", "text/plain")], "made"),
    ]);
    let file = setup.write(
        "requests/api.http",
        "GET http://{{host}}/items?q={{q}}\nAccept: application/json\nX-Token: {{token}}\n\n###\n# @name create\nPOST http://{{host}}/items\nContent-Type: application/json\n\n{\"name\": \"{{name}}\"}\n",
    );
    setup.write(
        "http-client.env.json",
        &format!(
            r#"{{"$shared": {{"q": "tea"}}, "dev": {{"host": "127.0.0.1:{}", "token": "public"}}}}"#,
            server.port()
        ),
    );
    setup.write("http-client.private.env.json", r#"{"dev": {"token": "private", "name": "green"}}"#);
    let got =
        setup.dispatch("http.request.run", json!({ "path": file, "index": 0, "env": "dev", "waitMs": 10000 })).unwrap();
    assert_eq!(got["state"], "done", "{got}");
    assert_eq!(got["url"], format!("http://127.0.0.1:{}/items?q=tea", server.port()));
    assert_eq!(got["environment"], "dev");
    assert_eq!(got["response"]["status"], 200);
    assert_eq!(got["response"]["body"], "{\"items\":[{\"id\":1}]}");
    assert_eq!(got["response"]["contentType"], "application/json");
    assert!(got["durationMs"].is_u64());
    setup.dispatch("http.environment.select", json!({ "env": "dev" })).unwrap();
    let started = setup.dispatch("http.request.run", json!({ "path": file, "name": "create" })).unwrap();
    let id = started["id"].as_u64().unwrap();
    let posted = setup.dispatch("http.response.get", json!({ "id": id, "waitMs": 10000 })).unwrap();
    assert_eq!(posted["state"], "done");
    assert_eq!(posted["response"]["status"], 201);
    assert_eq!(posted["requestBody"], "{\"name\": \"{{name}}\"}", "a private value is not kept");
    let seen = server.finish();
    assert_eq!(seen[0].target, "/items?q=tea");
    assert_eq!(seen[0].header("x-token"), Some("private"));
    assert_eq!(seen[0].header("accept"), Some("application/json"));
    assert_eq!((seen[1].method.as_str(), seen[1].body.as_slice()), ("POST", &b"{\"name\": \"green\"}"[..]));

    let listed = setup.dispatch("http.responses.list", json!({ "path": file })).unwrap();
    let ids: Vec<u64> = listed["responses"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [id, got["id"].as_u64().unwrap()], "newest first");
    let history = PathBuf::from(listed["historyFile"].as_str().unwrap());
    assert!(history.starts_with(setup.data.path().join("http-client")), "{}", history.display());
    assert!(history.is_file());
    assert!(!setup.project.path().join(".idea").exists());
    let in_project: Vec<PathBuf> = files_under(setup.project.path());
    assert_eq!(
        in_project,
        [
            setup.project.path().join("http-client.env.json"),
            setup.project.path().join("http-client.private.env.json"),
            file.clone(),
        ],
        "the HTTP Client writes nothing into the project"
    );

    let mut again = Session::new();
    setup.registry.dispatch(&mut again, "file.project.open", json!({ "path": setup.project.path() })).unwrap();
    again.state_mut::<HttpClient>().set_data_dir(setup.data.path().to_owned());
    let kept = setup.registry.dispatch(&mut again, "http.responses.list", json!({ "limit": 1 })).unwrap();
    assert_eq!(kept["responses"].as_array().unwrap().len(), 1);
    assert_eq!(kept["responses"][0]["response"]["body"], "made");
}

#[test]
fn refusals_name_what_is_wrong() {
    let mut setup = setup();
    let file = setup.write("a.http", "GET http://{{host}}/a\n\n###\nGET http://127.0.0.1:9/b\n");
    let not_http = setup.write("a.txt", "GET http://127.0.0.1:9/\n");
    let error = setup.dispatch("http.requests.list", json!({ "path": not_http })).unwrap_err();
    assert!(error.contains(&t!("http-error-not-http", path = not_http.display().to_string())), "{error}");
    let error = setup.dispatch("http.request.run", json!({ "path": file })).unwrap_err();
    assert!(error.contains(&t!("http-error-which-request")), "{error}");
    let error = setup.dispatch("http.request.run", json!({ "path": file, "index": 5 })).unwrap_err();
    assert!(error.contains(&t!("http-error-no-request", request = "5", path = file.display().to_string())), "{error}");
    let error = setup.dispatch("http.request.run", json!({ "path": file, "index": 0 })).unwrap_err();
    assert!(error.contains(&t!("http-error-unresolved", names = "host")), "{error}");
    let error = setup.dispatch("http.request.run", json!({ "path": file, "index": 1, "env": "staging" })).unwrap_err();
    assert!(error.contains(&t!("http-error-no-environment", env = "staging")), "{error}");
    setup.write("http-client.env.json", "{");
    let listed = setup.dispatch("http.requests.list", json!({ "path": file })).unwrap();
    assert!(listed["environmentError"].is_string());
    assert!(setup.dispatch("http.request.run", json!({ "path": file, "index": 1 })).is_err());
    let error = setup.dispatch("http.response.get", json!({ "id": 99 })).unwrap_err();
    assert!(error.contains(&t!("http-error-no-response", id = "99")), "{error}");
    assert_eq!(setup.dispatch("http.responses.list", json!({})).unwrap()["responses"], json!([]));
}

#[test]
fn a_server_that_is_not_there_fails_the_exchange() {
    let mut setup = setup();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let file = setup.write("down.http", &format!("GET http://127.0.0.1:{port}/\n"));
    let got = setup.dispatch("http.request.run", json!({ "path": file, "waitMs": 10000 })).unwrap();
    assert_eq!(got["state"], "failed");
    assert!(got["error"].as_str().is_some_and(|error| !error.is_empty()));
    assert_eq!(got["response"], Value::Null);
}

#[test]
fn without_a_data_folder_the_history_stays_in_memory() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    let server = TestServer::start(vec![TestServer::json(200, "[]")]);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("one.rest");
    std::fs::write(&file, format!("{}\n", server.url("/"))).unwrap();
    let got = registry.dispatch(&mut session, "http.request.run", json!({ "path": file, "waitMs": 10000 })).unwrap();
    assert_eq!(got["state"], "done");
    let listed = registry.dispatch(&mut session, "http.responses.list", json!({})).unwrap();
    assert_eq!(listed["historyFile"], Value::Null);
    assert_eq!(listed["responses"].as_array().unwrap().len(), 1);
    assert_eq!(files_under(folder.path()), [file]);
}

#[test]
fn private_values_are_sent_but_never_kept() {
    let mut setup = setup();
    let server = TestServer::start(vec![TestServer::json(200, "{}")]);
    setup.write(zen_http::PUBLIC_FILE, &format!(r#"{{"dev": {{"base": "{}"}}}}"#, server.url("")));
    setup.write(zen_http::PRIVATE_FILE, r#"{"dev": {"token": "s3cr3t-value"}}"#);
    let file = setup.write(
        "api.http",
        "POST {{base}}/login?key={{token}}\nAuthorization: Bearer {{token}}\n\n{\"t\": \"{{token}}\"}\n",
    );
    let ran = setup.dispatch("http.request.run", json!({ "path": file, "env": "dev", "waitMs": 10000 })).unwrap();
    assert_eq!(ran["state"], "done", "{ran}");
    let seen = server.finish();
    assert_eq!(seen[0].target, "/login?key=s3cr3t-value");
    assert_eq!(seen[0].header("authorization"), Some("Bearer s3cr3t-value"));
    assert_eq!(ran["url"], format!("{}/login?key={{{{token}}}}", server.url("")));
    assert!(!ran.to_string().contains("s3cr3t"), "{ran}");
    let listed = setup.dispatch("http.responses.list", json!({})).unwrap();
    assert!(!listed.to_string().contains("s3cr3t"));
    for kept in files_under(setup.data.path()) {
        assert!(!std::fs::read_to_string(&kept).unwrap().contains("s3cr3t"), "{}", kept.display());
    }
}

#[test]
fn body_files_and_schemes_are_checked_before_anything_is_sent() {
    let mut setup = setup();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "x").unwrap();
    let body = format!("POST http://127.0.0.1:9/\n\n< {}\n", outside.path().join("secret.txt").display());
    let file = setup.write("a.http", &body);
    let error = setup.dispatch("http.request.run", json!({ "path": file })).unwrap_err();
    assert!(
        error.contains(&t!("http-error-body-outside", path = outside.path().join("secret.txt").display().to_string())),
        "{error}"
    );
    let file = setup.write("b.http", "GET file:///etc/hosts\n");
    let error = setup.dispatch("http.request.run", json!({ "path": file })).unwrap_err();
    assert!(error.contains(&t!("http-error-scheme", scheme = "file")), "{error}");
    let listed = setup.dispatch("http.responses.list", json!({})).unwrap();
    assert_eq!(listed["responses"], json!([]));
}

#[test]
fn the_history_is_never_kept_inside_the_project() {
    let project = tempfile::tempdir().unwrap();
    let mut client = HttpClient::default();
    client.set_data_dir(project.path().join(".idea"));
    assert_eq!(client.history_file(Some(project.path())), None);
    client.set_data_dir(project.path().parent().unwrap().join("elsewhere"));
    assert!(client.history_file(Some(project.path())).is_some());
}

#[cfg(unix)]
#[test]
fn an_agents_request_never_reads_a_hidden_file_and_never_sees_one_read_before() {
    let mut setup = setup();
    let server = TestServer::start(vec![TestServer::json(200, "TOKEN=abc123 echoed")]);
    setup.write(".env", "TOKEN=abc123\n");
    std::os::unix::fs::symlink(setup.project.path().join(".env"), setup.project.path().join("innocent.txt")).unwrap();
    let url = server.url("/echo");
    let file = setup.write(
        "api.http",
        &format!(
            "### plain\nPOST {url}\nContent-Type: text/plain\n\n< ./.env\n\n### link\nPOST {url}\n\n< innocent.txt\n\n\
             ### parts\nPOST {url}\nContent-Type: multipart/form-data; boundary=b\n\n--b\n\
             Content-Disposition: form-data; name=\"f\"; filename=\"a\"\n\n< ./.env\n--b--\n"
        ),
    );
    let ran = setup.dispatch("http.request.run", json!({ "path": file, "name": "plain", "waitMs": 10000 })).unwrap();
    assert!(ran["requestBody"].as_str().unwrap().contains("abc123"), "the user's own request: {ran}");
    setup.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let Setup { registry, session, .. } = &mut setup;
    let (refused, listed, got) = zen_core::macros::as_agent(session, |session| {
        let refused: Vec<String> = ["plain", "link", "parts"]
            .iter()
            .map(|name| {
                let params = json!({ "path": file, "name": name, "waitMs": 2000 });
                registry.dispatch(session, "http.request.run", params).unwrap_err().to_string()
            })
            .collect();
        let listed = registry.dispatch(session, "http.responses.list", json!({})).unwrap();
        let got = registry.dispatch(session, "http.response.get", json!({ "id": ran["id"] })).unwrap();
        (refused, listed, got)
    });
    for error in &refused {
        assert!(error.contains("hidden from agents"), "{error}");
    }
    for answer in [&listed, &got] {
        assert!(!answer.to_string().contains("abc123"), "{answer}");
    }
    assert_eq!(listed["responses"].as_array().unwrap().len(), 1, "nothing more was run: {listed}");
    assert_eq!(got["requestBody"], "[redacted]");
    assert_eq!(got["response"]["body"], "[redacted]");
    assert_eq!(got["response"]["status"], 200, "the rest stays");
    server.finish();
}

#[test]
fn an_agents_request_takes_no_value_from_a_hidden_environment_file() {
    let mut setup = setup();
    let server = TestServer::start(vec![TestServer::json(200, "{}"), TestServer::json(200, "{}")]);
    let file = setup.write(
        "api.http",
        &format!("### public\nGET {}/items?host={{{{host}}}}\n\n### private\nGET {}/items\nAuthorization: Bearer {{{{token}}}}\n", server.url(""), server.url("")),
    );
    setup.write("http-client.env.json", r#"{"dev": {"host": "public-value"}}"#);
    setup.write("http-client.private.env.json", r#"{"dev": {"token": "abc123-token"}}"#);
    setup
        .session
        .state_mut::<crate::hidden_files::HiddenFromAgents>()
        .set(&["http-client.private.env.json".to_owned()]);
    let Setup { registry, session, .. } = &mut setup;
    let (public, private) = zen_core::macros::as_agent(session, |session| {
        let run = |session: &mut Session, name: &str| {
            let params = json!({ "path": file, "name": name, "env": "dev", "waitMs": 10000 });
            registry.dispatch(session, "http.request.run", params).map_err(|error| error.to_string())
        };
        (run(session, "public"), run(session, "private"))
    });
    let public = public.unwrap();
    assert_eq!(public["state"], "done", "a request without a private value runs: {public}");
    assert!(public["url"].as_str().unwrap().contains("public-value"));
    let private = private.unwrap_err();
    assert!(private.contains("hidden from agents") && private.contains("http-client.private.env.json"), "{private}");
    assert!(!private.contains("abc123"));
    let seen = server.finish();
    assert_eq!(seen.len(), 1, "the refused request was never sent");
    assert!(!format!("{seen:?}").contains("abc123"));
}

#[test]
fn a_file_of_a_project_not_trusted_cannot_turn_the_proxy_off() {
    let mut session = Session::new();
    session.set_state(crate::build_tools::BuildEnv::default());
    let project = tempfile::tempdir().unwrap();
    assert!(!may_skip_the_proxy(&session, Some(project.path())), "# @no-proxy in a stranger's file");
    crate::trust::trust_session_in_tests(&mut session, project.path());
    assert!(may_skip_the_proxy(&session, Some(project.path())));
    assert!(may_skip_the_proxy(&session, None), "a file outside any project is the person's own");
}
