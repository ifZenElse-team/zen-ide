use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use super::*;

fn labels(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|row| format!("{}{}", "  ".repeat(row.depth), row.label)).collect()
}

fn rows_of(project: &Value, tree: Option<&Value>, expanded: &BTreeSet<String>) -> Vec<Row> {
    let shortcuts = BTreeMap::new();
    let inputs = MavenInputs { project, tree, root: Path::new("/p"), configurations: &[], shortcuts: &shortcuts };
    maven_rows(inputs, expanded)
}

fn maven() -> Value {
    json!({
        "project": {
            "pom": "/p/pom.xml", "groupId": "demo", "artifactId": "shop", "name": "Shop", "packaging": "pom",
            "profiles": [{ "id": "fast", "activeByDefault": true }, { "id": "ci", "activeByDefault": false }],
            "dependencies": [],
            "plugins": [{ "groupId": "org.apache.maven.plugins", "artifactId": "maven-clean-plugin", "version": "", "prefix": "clean", "goals": ["clean", "help"] }],
            "modules": [{
                "pom": "/p/core/pom.xml", "groupId": "demo", "artifactId": "core", "name": "core", "packaging": "jar",
                "profiles": [], "plugins": [], "modules": [],
                "dependencies": [{ "groupId": "junit", "artifactId": "junit", "version": "4.13.2", "scope": "test" }]
            }]
        },
        "lifecycle": ["clean", "compile"],
        "selectedProfiles": ["fast"]
    })
}

#[test]
fn maven_shows_profiles_and_projects_open_and_the_rest_closed() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let none = BTreeSet::new();
    let rows = rows_of(&maven(), None, &none);
    assert_eq!(labels(&rows), ["Profiles", "  fast", "  ci", "Shop", "  Lifecycle", "  Plugins", "  Modules"]);
    assert_eq!(rows[1].checked, Some(true));
    assert_eq!(rows[2].action, Some(RowAction::Profile("ci".to_owned())));

    let open: BTreeSet<String> = ["/@shop/lifecycle", "/@shop/plugins", "/@shop/plugins/clean", "/@shop/modules"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let rows = rows_of(&maven(), None, &open);
    let compile = rows.iter().find(|row| row.label == "compile").unwrap();
    assert_eq!(compile.action, Some(RowAction::Goals { goals: "compile".to_owned(), pom: "pom.xml".to_owned() }));
    let goal = rows.iter().find(|row| row.label == "clean:clean").unwrap();
    assert_eq!(goal.action, Some(RowAction::Goals { goals: "clean:clean".to_owned(), pom: "pom.xml".to_owned() }));
    let core = rows.iter().find(|row| row.label == "core").unwrap();
    assert!(core.parent && open_by_default(&core.id), "a module's project starts open");
    let dependencies = rows.iter().position(|row| row.id == "/@shop/modules/@core/dependencies").unwrap();
    assert_eq!(rows.len(), dependencies + 1, "its dependencies start closed");
}

#[test]
fn maven_shows_the_resolved_tree_once_loaded() {
    zen_i18n::register(crate::toolbox::LOCALES);
    let tree = json!({ "roots": [
        { "groupId": "demo", "artifactId": "core", "type": "jar", "version": "1", "children": [
            { "groupId": "junit", "artifactId": "junit", "type": "jar", "version": "4.13.2", "scope": "test", "children": [
                { "groupId": "org.hamcrest", "artifactId": "hamcrest-core", "type": "jar", "version": "1.3", "scope": "test" }
            ]}
        ]}
    ]});
    let open: BTreeSet<String> =
        ["/@shop/modules", "/@shop/modules/@core/dependencies", "/@shop/modules/@core/dependencies/junit:junit"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    let rows = rows_of(&maven(), Some(&tree), &open);
    let junit = rows.iter().find(|row| row.label == "junit:junit").unwrap();
    assert_eq!(junit.detail, "4.13.2 (test)");
    assert!(junit.parent);
    let hamcrest = rows.iter().find(|row| row.label == "org.hamcrest:hamcrest-core").unwrap();
    assert_eq!(hamcrest.depth, junit.depth + 1);
    assert_eq!(
        hamcrest.action,
        Some(RowAction::Dependency {
            group: "org.hamcrest".into(),
            artifact: "hamcrest-core".into(),
            version: "1.3".into()
        })
    );
}

fn gradle_model() -> Value {
    json!({ "builds": [{
        "folder": "/p", "projectPath": ".",
        "projects": [
            {
                "build": ":", "path": ":", "name": "shop", "folder": "/p",
                "tasks": [
                    { "name": "zeta", "group": "other" },
                    { "name": "init", "group": "build setup" },
                    { "name": "build", "group": "build" }
                ],
                "configurations": []
            },
            {
                "build": ":", "path": ":app", "name": "app", "folder": "/p/app", "buildFile": "/p/app/build.gradle",
                "tasks": [{ "name": "run", "group": "application", "description": "Runs it" }],
                "configurations": [
                    { "name": "annotationProcessor", "dependencies": [] },
                    { "name": "compileClasspath", "dependencies": [
                        { "project": ":lib" },
                        { "groupId": "org.a", "artifactId": "a", "version": "2", "requested": "org.a:a:1",
                          "children": [{ "groupId": "org.b", "artifactId": "b", "version": "1", "repeated": true }] },
                        { "requested": "no.such:thing:1", "unresolved": "Could not find it" }
                    ] },
                    { "name": "testCompileClasspath", "dependencies": [], "error": "broken\nmore" }
                ]
            },
            {
                "build": ":tools", "path": ":tools", "name": "tools", "folder": "/p/tools",
                "tasks": [{ "name": "check", "group": "verification" }],
                "configurations": []
            },
            {
                "build": ":tools", "path": ":tools:core", "name": "core", "folder": "/p/tools/core",
                "tasks": [{ "name": "jar", "group": "build" }],
                "configurations": []
            },
            { "build": ":buildSrc", "path": ":buildSrc", "name": "buildSrc", "folder": "/p/buildSrc", "tasks": [], "configurations": [] }
        ]
    }, {
        "folder": "/q", "projectPath": "/q",
        "projects": [{ "build": ":", "path": ":", "name": "other", "folder": "/q", "tasks": [{ "name": "build", "group": "build" }], "configurations": [] }]
    }]})
}

fn gradle(model: &Value, configurations: &[Value], expanded: &[&str]) -> Vec<Row> {
    let open: BTreeSet<String> = expanded.iter().map(|id| (*id).to_owned()).collect();
    let shortcuts = BTreeMap::new();
    gradle_rows(GradleInputs { model, configurations, shortcuts: &shortcuts }, &open)
}

#[test]
fn the_window_says_which_tool_runs_and_where_it_was_found() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert_eq!(tool_line(Tool::Maven, &json!({})), None, "nothing found: the banner speaks");
    let distribution =
        json!({ "executable": { "path": "/h/mvn", "source": "wrapperDistribution", "version": "3.9.11" } });
    assert_eq!(tool_line(Tool::Maven, &distribution).unwrap(), "Maven 3.9.11, from a wrapper's download");
    let wrapper = json!({ "executable": { "path": "/p/gradlew", "source": "projectWrapper" } });
    assert_eq!(tool_line(Tool::Gradle, &wrapper).unwrap(), "Gradle, the project's wrapper");
}

#[test]
fn gradle_shows_projects_and_tasks_by_group_with_included_builds_and_build_src() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let settings_only = json!({ "builds": [{ "projectPath": ".", "projects": [
        { "build": ":", "path": ":", "name": "shop", "folder": "/p" },
        { "build": ":", "path": ":app", "name": "app", "folder": "/p/app", "buildFile": "/p/app/build.gradle" }
    ]}]});
    let rows = gradle(&settings_only, &[], &[]);
    assert_eq!(labels(&rows), ["shop", "  Reload All Gradle Projects to load the tasks", "  app"]);

    let model = gradle_model();
    let rows = gradle(&model, &[], &[]);
    assert_eq!(
        labels(&rows),
        ["shop", "  Tasks", "  app", "tools", "buildSrc", "other", "  Tasks"],
        "included builds and buildSrc are projects at the top; a linked build opens like the project's own"
    );
    assert_eq!(rows[3].detail, "(included build)");
    assert_eq!(rows[4].detail, "");

    let rows = gradle(
        &model,
        &[],
        &[
            "@:/tasks",
            "@:app",
            "@:app/tasks",
            "@:app/tasks/application",
            "@:tools",
            "@:tools:core",
            "@:tools:core/tasks",
            "@:tools:core/tasks/build",
        ],
    );
    assert_eq!(
        labels(&rows)[..9],
        [
            "shop",
            "  Tasks",
            "    build",
            "    build setup",
            "    other",
            "  app",
            "    Tasks",
            "      application",
            "        run"
        ],
        "groups by name, other last"
    );
    let run = rows.iter().find(|row| row.label == "run").unwrap();
    assert_eq!(run.action, Some(RowAction::Tasks { tasks: ":app:run".to_owned(), project_path: ".".to_owned() }));
    assert_eq!(run.detail, "Runs it");
    let jar = rows.iter().find(|row| row.label == "jar").unwrap();
    assert_eq!(
        jar.action,
        Some(RowAction::Tasks { tasks: ":tools:core:jar".to_owned(), project_path: ".".to_owned() }),
        "a task of an included build runs from the composite"
    );
    assert_eq!(gradle(&model, &[], &["@:"]).len(), 5, "a closed root hides its projects, not the other builds");
    let linked = gradle(&model, &[], &["linked0/@:/tasks", "linked0/@:/tasks/build"]);
    let build = linked.last().unwrap();
    assert_eq!(build.action, Some(RowAction::Tasks { tasks: ":build".to_owned(), project_path: "/q".to_owned() }));
    assert_eq!(linked_index(&build.id), Some(1));
}

#[test]
fn gradle_lists_dependencies_by_configuration_as_gradle_resolved_them() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let model = gradle_model();
    let open = [
        "@:app",
        "@:app/dependencies",
        "@:app/dependencies/compileClasspath",
        "@:app/dependencies/compileClasspath/org.a:a",
    ];
    let rows = gradle(&model, &[], &open);
    let at = rows.iter().position(|row| row.label == "Dependencies").unwrap();
    assert_eq!(
        labels(&rows)[at..at + 7],
        [
            "    Dependencies",
            "      compileClasspath",
            "        project :lib",
            "        org.a:a",
            "          org.b:b",
            "        no.such:thing:1",
            "      testCompileClasspath"
        ],
        "an empty configuration is left out"
    );
    assert_eq!(rows[at + 3].detail, "2 (requested org.a:a:1)");
    assert_eq!(rows[at + 4].detail, "1 (*)");
    assert_eq!(rows[at + 5].detail, "(unresolved)");
    assert_eq!(rows[at + 6].detail, "broken");
    assert!(rows[at + 3].action.is_none(), "a Gradle library has no POM to open");
}

#[test]
fn gradle_lists_run_configurations_under_their_build_and_a_task_has_its_menu() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let model = gradle_model();
    let saved = json!({
        "id": "c1", "kind": "gradle", "name": "shop [:app:run]",
        "options": { "projectPath": ".", "tasks": ":app:run" },
        "common": { "beforeLaunch": [{ "type": "gradle.task", "projectPath": ".", "tasks": ":build" }] }
    });
    let elsewhere =
        json!({ "id": "c2", "kind": "gradle", "name": "other", "options": { "projectPath": "/q", "tasks": "x" } });
    let maven = json!({ "id": "c3", "kind": "maven", "name": "m", "options": { "projectPath": "." } });
    let configurations = [saved, elsewhere, maven];
    let rows = gradle(
        &model,
        &configurations,
        &["@:/configurations", "linked0/@:/configurations", "@:/tasks", "@:/tasks/build"],
    );
    let names: Vec<&str> = rows
        .iter()
        .filter(|row| matches!(row.action, Some(RowAction::Configuration(_))))
        .map(|row| row.label.as_str())
        .collect();
    assert_eq!(names, ["shop [:app:run]", "other"]);

    let task = rows.iter().find(|row| row.label == "build" && row.icon == RowIcon::Run).unwrap();
    let triggers =
        [json!({ "when": "beforeSync", "task": { "type": "gradle.task", "projectPath": ".", "tasks": ":build" } })];
    let context =
        MenuContext { project: &Value::Null, builds: &[], triggers: &triggers, configurations: &configurations };
    let menu = tool_menu::menu_state(&tool_menu::gradle_menu(task, &context));
    let entries = menu.as_array().unwrap();
    let labelled = |label: &str| {
        entries.iter().find(|entry| entry["label"] == label).unwrap_or_else(|| panic!("{label} in {menu}"))
    };
    assert_eq!(labelled("Run ':build'")["command"], "gradle.task.run");
    assert_eq!(labelled("Debug ':build'")["command"], "gradle.task.debug");
    assert_eq!(labelled("Create Run Configuration")["command"], "gradle.runConfiguration.create");
    let before = labelled("Execute Before Sync");
    assert_eq!((before["checked"].clone(), before["params"]["on"].clone()), (json!(true), json!(false)));
    assert_eq!(labelled("Execute After Sync")["checked"], false);
    assert_eq!(labelled("Execute Before Build")["command"], "build.triggers.set");
    let runs = labelled("Execute Before Run/Debug")["items"].as_array().unwrap().clone();
    assert_eq!(runs[0]["command"], TASK_BEFORE_RUN);
    assert_eq!(runs[0]["checked"], true, "{menu}");
    let shortcut = labelled("Assign Shortcut…");
    assert_eq!(shortcut["params"]["tool"], "gradle");
    assert_eq!(shortcut["params"]["action"], task_shortcut_action(".", ":build"));
}

#[test]
fn a_linked_gradle_build_can_be_unlinked_and_the_project_has_its_settings() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let model = gradle_model();
    let builds = model["builds"].as_array().unwrap().clone();
    let rows = gradle(&model, &[], &[]);
    let context = MenuContext { project: &Value::Null, builds: &builds, triggers: &[], configurations: &[] };
    let own = rows.iter().find(|row| row.label == "shop").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::gradle_menu(own, &context)).to_string();
    assert!(menu.contains("gradle.project.reload") && menu.contains("view.settings.open"), "{menu}");
    assert!(!menu.contains("gradle.project.unlink"), "{menu}");
    let linked = rows.iter().find(|row| row.label == "other").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::gradle_menu(linked, &context));
    assert_eq!(menu[1]["command"], "gradle.project.unlink");
    assert_eq!(menu[1]["params"]["path"], "/q");
}

#[test]
fn only_maven_and_gradle_depend_on_the_project() {
    let window = BuildToolWindow::default();
    assert!(!window.available("maven") && !window.available("gradle"));
    assert!(window.available("project") && window.available("caffeine"));
}

fn ids(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn maven_lists_linked_projects_flat_modules_ignored_ones_and_run_configurations() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut project = maven();
    project["linked"] = json!([{ "pom": "/elsewhere/pom.xml", "artifactId": "tools", "name": "tools", "profiles": [{ "id": "extra" }], "plugins": [], "modules": [], "dependencies": [] }]);
    project["ignored"] = json!(["/p/core/pom.xml"]);
    project["view"] = json!({ "groupModules": false, "showIgnored": true, "basicPhases": true });
    let configurations = [
        json!({ "id": "c1", "name": "shop [install]", "kind": "maven", "options": { "pom": "pom.xml", "goals": "install" } }),
        json!({ "id": "c2", "name": "app", "kind": "java", "options": {} }),
    ];
    let shortcuts: BTreeMap<String, String> =
        [(goal_shortcut_action("pom.xml", "compile"), "⌃⌥C".to_owned())].into_iter().collect();
    let open = ids(&["/@shop/lifecycle", "/@shop/configurations"]);
    let inputs = MavenInputs {
        project: &project,
        tree: None,
        root: Path::new("/p"),
        configurations: &configurations,
        shortcuts: &shortcuts,
    };
    let rows = maven_rows(inputs, &open);
    assert_eq!(
        labels(&rows),
        [
            "Profiles",
            "  fast",
            "  ci",
            "  extra",
            "Shop",
            "  Lifecycle",
            "    clean",
            "    compile",
            "  Plugins",
            "  Run Configurations",
            "    shop [install]",
            "core",
            "  Lifecycle",
            "  Dependencies",
            "tools",
            "  Lifecycle"
        ]
    );
    let compile = rows.iter().find(|row| row.label == "compile").unwrap();
    assert_eq!(compile.detail, "⌃⌥C", "the shortcut assigned to the goal");
    let configuration = rows.iter().find(|row| row.label == "shop [install]").unwrap();
    assert_eq!(configuration.action, Some(RowAction::Configuration("c1".to_owned())));
    let core = rows.iter().find(|row| row.label == "core").unwrap();
    assert!(core.muted && core.detail == "(ignored)", "{core:?}");

    project["view"]["showIgnored"] = json!(false);
    project["rootLinked"] = json!(false);
    let rows = rows_of(&project, None, &BTreeSet::new());
    assert!(!labels(&rows).iter().any(|label| label == "core" || label == "Shop"), "{:?}", labels(&rows));
    assert!(labels(&rows).contains(&"tools".to_owned()));
}

#[test]
fn the_menus_offer_what_a_project_and_a_goal_can_do_with_their_ticks() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let project = maven();
    let task = tool_menu::goal_task("pom.xml", "compile");
    let triggers = [json!({ "when": "afterBuild", "task": task })];
    let configurations = [
        json!({ "id": "c1", "name": "App", "kind": "java", "common": { "beforeLaunch": [task] } }),
        json!({ "id": "t", "name": "temp", "kind": "java", "temporary": true }),
    ];
    let context = MenuContext { project: &project, builds: &[], triggers: &triggers, configurations: &configurations };
    let rows = rows_of(&project, None, &ids(&["/@shop/lifecycle", "/@shop/modules"]));
    let head = rows.iter().find(|row| row.label == "Shop").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::maven_menu(head, &context));
    let labels: Vec<&str> = menu.as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap()).collect();
    assert_eq!(
        labels,
        [
            "Reload Project",
            "Unlink Maven Projects",
            "Ignore Projects",
            "Create settings.xml",
            "Download Sources and Documentation",
            "Show Effective POM",
            "Analyze Dependencies"
        ]
    );
    assert_eq!(menu[1]["params"], json!({ "pom": "/p/pom.xml" }));
    assert_eq!(menu[5]["command"], EFFECTIVE_POM);
    assert_eq!(menu[6]["command"], crate::ide::dependency_analyzer::OPEN);
    assert_eq!(menu[6]["params"], json!({ "system": "maven", "module": "demo:shop" }));

    let core = rows.iter().find(|row| row.label == "core").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::maven_menu(core, &context));
    let analyze = menu.as_array().unwrap().iter().find(|item| item["label"] == "Analyze Dependencies").unwrap();
    assert_eq!(analyze["params"], json!({ "system": "maven", "module": "demo:core" }), "a module opens on itself");

    let compile = rows.iter().find(|row| row.label == "compile").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::maven_menu(compile, &context));
    let at = |label: &str| menu.as_array().unwrap().iter().find(|item| item["label"] == label).cloned().unwrap();
    assert_eq!(at("Run 'compile'")["command"], "maven.goal.run");
    assert_eq!(at("Debug 'compile'")["command"], "maven.goal.debug");
    assert_eq!(at("Create Run Configuration")["params"], json!({ "goals": "compile", "pom": "pom.xml" }));
    assert_eq!(at("Execute Before Build")["checked"], false);
    let after = at("Execute After Build");
    assert_eq!((after["checked"].clone(), after["params"]["on"].clone()), (json!(true), json!(false)));
    let before_run = at("Execute Before Run/Debug");
    assert_eq!(before_run["items"].as_array().unwrap().len(), 1, "temporary configurations are left out");
    assert_eq!(before_run["items"][0]["checked"], true);
    let shortcut = at("Assign Shortcut…");
    assert_eq!(shortcut["params"]["action"], r#"maven.goal.run {"goals":"compile","pom":"pom.xml"}"#);
}

#[test]
fn a_project_outside_the_build_offers_no_analysis_of_the_build() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut project = maven();
    project["linked"] = json!([{ "pom": "/x/tools/pom.xml", "groupId": "x", "artifactId": "tools", "name": "tools" }]);
    let context = MenuContext { project: &project, builds: &[], triggers: &[], configurations: &[] };
    let rows = rows_of(&project, None, &BTreeSet::new());
    let tools = rows.iter().find(|row| row.label == "tools").unwrap();
    let menu = tool_menu::menu_state(&tool_menu::maven_menu(tools, &context));
    assert!(menu.as_array().unwrap().iter().all(|item| item["label"] != "Analyze Dependencies"), "{menu}");
    assert!(menu.as_array().unwrap().iter().any(|item| item["label"] == "Show Effective POM"), "{menu}");
}

#[test]
fn speed_search_moves_to_the_next_row_holding_the_text() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let rows = rows_of(&maven(), None, &ids(&["/@shop/lifecycle", "/@shop/plugins"]));
    assert_eq!(tool_menu::search_target(&rows, "CLE", None).as_deref(), Some("/@shop/lifecycle"));
    let from = Some("/@shop/lifecycle");
    assert_eq!(tool_menu::search_target(&rows, "cle", from).as_deref(), Some("/@shop/lifecycle"));
    assert_eq!(tool_menu::search_target(&rows, "clean", from).as_deref(), Some("/@shop/lifecycle/clean"));
    assert_eq!(tool_menu::search_target(&rows, "nothing", None), None);
}

#[test]
fn the_error_shown_to_people_has_no_command_id() {
    let failed = zen_core::DispatchError::Failed {
        id: zen_core::CommandId::new("maven.project.reload").unwrap(),
        error: zen_core::CommandError::new("Maven is not found."),
    };
    assert_eq!(failed.text(), "Maven is not found.");
    assert!(failed.to_string().contains("maven.project.reload"), "logs and agents still get it");
}

#[test]
fn the_banner_is_the_projects_reason_with_its_fixes() {
    assert_eq!(banner(&json!({})), None, "a project that loads shows no banner");
    let blocked = json!({ "blocked": {
        "kind": "notTrusted", "text": "This project is not trusted.",
        "fixes": [{ "title": "Trust Project", "command": "project.trust.grant", "params": { "path": "/p" }, "reload": true }]
    } });
    let shown = banner(&blocked).unwrap();
    assert_eq!(
        shown,
        Banner {
            text: "This project is not trusted.".to_owned(),
            serious: false,
            fixes: vec![Fix {
                title: "Trust Project".to_owned(),
                command: "project.trust.grant".to_owned(),
                params: json!({ "path": "/p" }),
                reload: true,
            }],
        }
    );
    let missing = json!({ "blocked": { "kind": "toolNotFound", "text": "Maven is not found.", "fixes": [] } });
    assert!(banner(&missing).unwrap().serious);
}
