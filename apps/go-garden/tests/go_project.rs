use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::language::structure::Outliners;
use zen_workbench::model::ModelEnv;
use zen_workbench::project_model::ProjectModels;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn dispatch(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
    registry.dispatch(session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
}

const MAIN: &str = "package main\n\ntype Server struct {\n\tName string\n}\n\nfunc (s *Server) Start() error {\n\treturn nil\n}\n\nfunc main() {\n}\n";

#[test]
fn a_go_workspace_has_its_modules_libraries_and_structure() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let sdk = temp.path().join("sdk");
    let cache = temp.path().join("cache");
    std::fs::create_dir_all(sdk.join("src")).unwrap();
    std::fs::create_dir_all(cache.join("github.com/!burnt!sushi/toml@v1.4.0")).unwrap();
    write(&project.join("go.work"), "go 1.23\n\nuse (\n\t./app\n\t./lib\n)\n");
    write(
        &project.join("app/go.mod"),
        "module example.com/app\n\ngo 1.23\n\nrequire (\n\texample.com/lib v0.0.0\n\tgithub.com/BurntSushi/toml v1.3.0\n\tgolang.org/x/text v0.14.0 // indirect\n)\n",
    );
    write(
        &project.join("lib/go.mod"),
        "module example.com/lib\n\ngo 1.23\n\nrequire github.com/BurntSushi/toml v1.4.0\n",
    );
    write(&project.join("app/main.go"), MAIN);

    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = ModelEnv {
        sdk_root: Some(sdk.clone()),
        sdk_version: Some("1.23.4".to_owned()),
        package_cache: Some(cache.clone()),
    };
    session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(zen_lang_go::GoModules)], env);
    session.state_mut::<Outliners>().set(vec![Arc::new(zen_lang_go::GoOutliner)]);
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project }));
    let refreshed = dispatch(&registry, &mut session, "project.model.refresh", json!({}));
    assert_eq!(refreshed["changed"], true);

    let info = dispatch(&registry, &mut session, "project.module.info", json!({ "all": true }));
    assert_eq!(info["workspace"]["languageVersion"], "1.23");
    assert_eq!(info["modules"][0]["name"], "example.com/app");
    assert_eq!(info["modules"][0]["languageVersion"], "1.23");
    assert_eq!(info["modules"][0]["dependencies"][2]["indirect"], true);
    assert_eq!(info["modules"][1]["name"], "example.com/lib");
    assert_eq!(info["problems"], json!([]));

    let listed = dispatch(&registry, &mut session, "project.libraries.list", json!({}));
    let libraries = listed["libraries"].as_array().unwrap();
    assert_eq!(libraries[0]["kind"], "sdk");
    assert_eq!((libraries[0]["name"].as_str(), libraries[0]["version"].as_str()), (Some("Go"), Some("1.23.4")));
    assert_eq!(libraries[0]["root"], json!(sdk.join("src")));
    let names: Vec<(&str, &str, bool)> = libraries[1..]
        .iter()
        .map(|library| {
            (
                library["name"].as_str().unwrap(),
                library["version"].as_str().unwrap(),
                library["exists"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(names, [("github.com/BurntSushi/toml", "v1.4.0", true), ("golang.org/x/text", "v0.14.0", false)]);
    assert_eq!(libraries[1]["root"], json!(cache.join("github.com/!burnt!sushi/toml@v1.4.0")));

    write(&project.join("lib/go.mod"), "module example.com/lib\n\ngo 1.23\n");
    let unwatched = json!({ "paths": [project.join("app/main.go")] });
    assert_eq!(dispatch(&registry, &mut session, "project.model.refresh", unwatched)["changed"], false);
    let watched = json!({ "paths": [project.join("lib/go.mod")] });
    assert_eq!(dispatch(&registry, &mut session, "project.model.refresh", watched)["changed"], true);
    let listed = dispatch(&registry, &mut session, "project.libraries.list", json!({}));
    assert_eq!(listed["libraries"][1]["version"], "v1.3.0", "the lower version is the highest left");

    let main = project.join("app/main.go");
    let structure = dispatch(&registry, &mut session, "editor.structure.get", json!({ "path": main }));
    assert_eq!((structure["status"].as_str(), structure["source"].as_str()), (Some("ready"), Some("syntax")));
    let symbols = structure["symbols"].as_array().unwrap();
    let top: Vec<&str> = symbols.iter().map(|symbol| symbol["name"].as_str().unwrap()).collect();
    assert_eq!(top, ["Server", "main"]);
    assert_eq!(symbols[0]["kind"], "struct");
    let members: Vec<(&str, &str)> = symbols[0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| (symbol["name"].as_str().unwrap(), symbol["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(members, [("Name", "field"), ("Start", "method")]);

    let start = &symbols[0]["children"][1];
    let offset = start["selectionStart"].as_u64().unwrap();
    let spot = dispatch(&registry, &mut session, "navigate.place.goTo", json!({ "path": main, "offset": offset }));
    assert_eq!((spot["line"].as_u64(), start["line"].as_u64()), (Some(7), Some(7)));
    let structure = dispatch(&registry, &mut session, "editor.structure.get", json!({}));
    assert_eq!(structure["path"], json!(main));
    assert_eq!(structure["current"], json!([0, 1]), "the method at the caret is the current symbol");
}

#[test]
fn a_broken_go_mod_and_go_work_are_problems_until_they_are_fixed() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    write(&project.join("go.work"), "go 1.23\n\nuse (\n\t./app\n\t./gone\n)\n");
    write(&project.join("app/go.mod"), "module example.com/app\n\ngo 1.23\n\nfrobnicate x\n");
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(zen_lang_go::GoModules)], ModelEnv::default());
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": project }));
    dispatch(&registry, &mut session, "project.model.refresh", json!({}));

    let listed = dispatch(&registry, &mut session, "problems.project.list", json!({}));
    let found: Vec<(String, u64, String)> = listed
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| {
            let path = file["path"].as_str().unwrap().to_owned();
            file["problems"].as_array().unwrap().iter().map(move |problem| {
                (path.clone(), problem["line"].as_u64().unwrap(), problem["severity"].as_str().unwrap().to_owned())
            })
        })
        .collect();
    let app = project.join("app/go.mod").display().to_string();
    let work = project.join("go.work").display().to_string();
    assert_eq!(found, [(app.clone(), 5, "error".to_owned()), (work.clone(), 5, "error".to_owned())]);
    let file = dispatch(&registry, &mut session, "problems.file.list", json!({ "path": "app/go.mod" }));
    assert_eq!((file[0]["line"].as_u64(), file[0]["column"].as_u64()), (Some(5), Some(1)));

    write(&project.join("app/go.mod"), "module example.com/app\n\ngo 1.23\n");
    write(&project.join("go.work"), "go 1.23\n\nuse ./app\n");
    let watched = json!({ "paths": [project.join("app/go.mod"), project.join("go.work")] });
    assert_eq!(dispatch(&registry, &mut session, "project.model.refresh", watched)["changed"], true);
    assert_eq!(dispatch(&registry, &mut session, "problems.project.list", json!({})), json!([]));
    let file = dispatch(&registry, &mut session, "problems.file.list", json!({ "path": "go.work" }));
    assert_eq!(file, json!({ "problems": [], "checked": true }));
}
