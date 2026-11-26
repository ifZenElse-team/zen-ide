use serde_json::{Value, json};
use zen_i18n::t;

use super::{Row, RowAction, RowIcon, Tool, goal_shortcut_action, task_shortcut_action, text};

pub(in crate::ide) const EFFECTIVE_POM: &str = "view.maven.effectivePom";
pub(in crate::ide) const BEFORE_RUN: &str = "view.maven.beforeRun";
pub(in crate::ide) const ASSIGN_SHORTCUT: &str = "view.buildTool.assignShortcut";
pub(in crate::ide) const EXECUTE_GOAL: &str = "view.maven.executeGoal";
pub(in crate::ide) const EXECUTE_TASK: &str = "view.gradle.executeTask";
pub(in crate::ide) const TASK_BEFORE_RUN: &str = "view.gradle.beforeRun";
pub(in crate::ide) const OPEN_SETTINGS: &str = "view.settings.open";
pub(in crate::ide) const MENU: &str = "view.buildTool.menu";
pub(in crate::ide) const SEARCH: &str = "view.buildTool.search";

#[derive(Clone, Debug, PartialEq)]
pub(super) enum MenuItem {
    Command { label: String, command: String, params: Value, checked: Option<bool> },
    Submenu { label: String, items: Vec<MenuItem> },
    Separator,
}

fn item(label: String, command: &str, params: Value) -> MenuItem {
    MenuItem::Command { label, command: command.to_owned(), params, checked: None }
}

fn ticked(label: String, command: &str, params: Value, checked: bool) -> MenuItem {
    MenuItem::Command { label, command: command.to_owned(), params, checked: Some(checked) }
}

pub(super) struct MenuContext<'a> {
    pub(super) project: &'a Value,
    pub(super) builds: &'a [Value],
    pub(super) triggers: &'a [Value],
    pub(super) configurations: &'a [Value],
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().map(text).collect()
}

pub(super) fn goal_task(pom: &str, goals: &str) -> Value {
    json!({ "type": "maven.goal", "pom": pom, "goals": goals })
}

pub(super) fn gradle_task(project_path: &str, tasks: &str) -> Value {
    json!({ "type": "gradle.task", "projectPath": project_path, "tasks": tasks })
}

pub(super) fn has_menu(row: &Row) -> bool {
    matches!(
        (&row.action, row.icon),
        (Some(RowAction::Open(_)), RowIcon::Project)
            | (Some(RowAction::Goals { .. } | RowAction::Tasks { .. } | RowAction::Configuration(_)), _)
    )
}

pub(super) fn maven_menu(row: &Row, context: &MenuContext<'_>) -> Vec<MenuItem> {
    match (&row.action, row.icon) {
        (Some(RowAction::Open(pom)), RowIcon::Project) => project_menu(&pom.to_string_lossy(), context),
        (Some(RowAction::Goals { goals, pom }), _) => goal_menu(goals, pom, context),
        (Some(RowAction::Configuration(id)), _) => configuration_menu(id),
        _ => Vec::new(),
    }
}

pub(super) fn gradle_menu(row: &Row, context: &MenuContext<'_>) -> Vec<MenuItem> {
    match (&row.action, row.icon) {
        (Some(RowAction::Tasks { tasks, project_path }), _) => gradle_task_menu(tasks, project_path, context),
        (Some(RowAction::Configuration(id)), _) => configuration_menu(id),
        (_, RowIcon::Project) => gradle_project_menu(row, context),
        _ => Vec::new(),
    }
}

fn configuration_menu(id: &str) -> Vec<MenuItem> {
    vec![
        item(t!("ide-build-menu-run"), "run.process.start", json!({ "configurationId": id })),
        item(t!("ide-build-menu-debug"), "debug.session.start", json!({ "configurationId": id })),
    ]
}

fn gradle_project_menu(row: &Row, context: &MenuContext<'_>) -> Vec<MenuItem> {
    let mut items = vec![item(t!("ide-gradle-reload"), "gradle.project.reload", json!({}))];
    if let Some(build) = super::linked_index(&row.id).and_then(|index| context.builds.get(index)) {
        items.push(item(t!("ide-gradle-menu-unlink"), "gradle.project.unlink", json!({ "path": build["folder"] })));
    }
    items.push(MenuItem::Separator);
    items.push(item(t!("ide-build-tool-settings"), OPEN_SETTINGS, json!({ "page": "gradle" })));
    items
}

struct TaskItems {
    run: MenuItem,
    debug: MenuItem,
    create: MenuItem,
    task: Value,
    before_run: &'static str,
    shortcut: Value,
    around_sync: bool,
}

fn task_items(entry: TaskItems, context: &MenuContext<'_>) -> Vec<MenuItem> {
    let TaskItems { run, debug, create, task, before_run, shortcut, around_sync } = entry;
    let trigger = |when: &str, label: String| {
        let on = context.triggers.iter().any(|trigger| trigger["when"] == when && trigger["task"] == task);
        ticked(label, "build.triggers.set", json!({ "when": when, "task": task, "on": !on }), on)
    };
    let runs: Vec<MenuItem> = context
        .configurations
        .iter()
        .filter(|configuration| configuration["temporary"] != true)
        .map(|configuration| {
            let id = text(&configuration["id"]);
            let on =
                configuration["common"]["beforeLaunch"].as_array().into_iter().flatten().any(|known| *known == task);
            ticked(text(&configuration["name"]), before_run, json!({ "configurationId": id, "task": task }), on)
        })
        .collect();
    let mut items = vec![run, debug, create, MenuItem::Separator];
    if around_sync {
        items.push(trigger("beforeSync", t!("ide-build-menu-before-sync")));
        items.push(trigger("afterSync", t!("ide-build-menu-after-sync")));
    }
    items.extend([
        trigger("beforeBuild", t!("ide-build-menu-before-build")),
        trigger("afterBuild", t!("ide-build-menu-after-build")),
        trigger("beforeRebuild", t!("ide-build-menu-before-rebuild")),
        trigger("afterRebuild", t!("ide-build-menu-after-rebuild")),
    ]);
    if !runs.is_empty() {
        items.push(MenuItem::Submenu { label: t!("ide-build-menu-before-run"), items: runs });
    }
    items.push(MenuItem::Separator);
    items.push(item(t!("ide-build-menu-assign-shortcut"), ASSIGN_SHORTCUT, shortcut));
    items
}

fn project_menu(pom: &str, context: &MenuContext<'_>) -> Vec<MenuItem> {
    let project = context.project;
    let own = text(&project["project"]["pom"]) == pom && project["rootLinked"] != false;
    let linked = project["linked"].as_array().into_iter().flatten().any(|linked| text(&linked["pom"]) == pom);
    let ignored = strings(&project["ignored"]).iter().any(|known| known == pom);
    let mut items = vec![item(t!("ide-maven-menu-reload"), "maven.project.reload", json!({}))];
    if own || linked {
        items.push(item(t!("ide-maven-menu-unlink"), "maven.project.unlink", json!({ "pom": pom })));
    }
    let ignore = if ignored { t!("ide-maven-menu-unignore") } else { t!("ide-maven-menu-ignore") };
    items.push(item(ignore, "maven.project.ignore", json!({ "pom": pom, "ignored": !ignored })));
    items.push(MenuItem::Separator);
    let settings = if project["userSettingsExists"] == true {
        t!("ide-maven-menu-open-settings")
    } else {
        t!("ide-maven-menu-create-settings")
    };
    items.push(item(settings, "maven.settingsFile.open", json!({})));
    items.push(item(t!("ide-maven-menu-download"), "maven.sources.download", json!({})));
    items.push(item(t!("ide-maven-menu-effective-pom"), EFFECTIVE_POM, json!({ "pom": pom })));
    if let Some(module) = build_module(&project["project"], pom, "").filter(|_| project["rootLinked"] != false) {
        let mut params = json!({ "system": "maven" });
        if !module.starts_with(':') {
            params["module"] = json!(module);
        }
        items.push(item(t!("ide-dependency-analyze"), crate::ide::dependency_analyzer::OPEN, params));
    }
    items
}

fn build_module(project: &Value, pom: &str, parent_group: &str) -> Option<String> {
    let group = project["groupId"].as_str().filter(|group| !group.is_empty()).unwrap_or(parent_group);
    if text(&project["pom"]) == pom {
        return Some(format!("{group}:{}", text(&project["artifactId"])));
    }
    project["modules"].as_array()?.iter().find_map(|module| build_module(module, pom, group))
}

fn goal_menu(goals: &str, pom: &str, context: &MenuContext<'_>) -> Vec<MenuItem> {
    let params = json!({ "goals": goals, "pom": pom });
    let title = t!("ide-maven-shortcut-title", goals = goals);
    task_items(
        TaskItems {
            run: item(t!("ide-maven-menu-run", goals = goals), "maven.goal.run", params.clone()),
            debug: item(t!("ide-maven-menu-debug", goals = goals), "maven.goal.debug", params.clone()),
            create: item(t!("ide-build-menu-create-configuration"), "maven.runConfiguration.create", params),
            task: goal_task(pom, goals),
            before_run: BEFORE_RUN,
            shortcut: json!({ "action": goal_shortcut_action(pom, goals), "title": title, "tool": "maven" }),
            around_sync: false,
        },
        context,
    )
}

fn gradle_task_menu(tasks: &str, project_path: &str, context: &MenuContext<'_>) -> Vec<MenuItem> {
    let params = json!({ "tasks": tasks, "projectPath": project_path });
    let title = t!("ide-gradle-shortcut-title", tasks = tasks);
    task_items(
        TaskItems {
            run: item(t!("ide-gradle-menu-run", tasks = tasks), "gradle.task.run", params.clone()),
            debug: item(t!("ide-gradle-menu-debug", tasks = tasks), "gradle.task.debug", params.clone()),
            create: item(t!("ide-build-menu-create-configuration"), "gradle.runConfiguration.create", params),
            task: gradle_task(project_path, tasks),
            before_run: TASK_BEFORE_RUN,
            shortcut: json!({
                "action": task_shortcut_action(project_path, tasks),
                "title": title,
                "tool": "gradle"
            }),
            around_sync: true,
        },
        context,
    )
}

pub(super) fn menu_state(items: &[MenuItem]) -> Value {
    Value::Array(
        items
            .iter()
            .filter_map(|entry| match entry {
                MenuItem::Command { label, command, params, checked } => {
                    let mut state = json!({ "label": label, "command": command, "params": params });
                    if let Some(checked) = checked {
                        state["checked"] = json!(checked);
                    }
                    Some(state)
                }
                MenuItem::Submenu { label, items } => Some(json!({ "label": label, "items": menu_state(items) })),
                MenuItem::Separator => None,
            })
            .collect(),
    )
}

pub(super) fn draw(ui: &mut egui::Ui, items: &[MenuItem], picked: &mut Option<(String, Value)>) {
    for entry in items {
        match entry {
            MenuItem::Command { label, command, params, checked } => {
                let clicked = match checked {
                    Some(on) => ui.add(egui::Button::new(label.as_str()).selected(*on)).clicked(),
                    None => ui.button(label.as_str()).clicked(),
                };
                if clicked {
                    *picked = Some((command.clone(), params.clone()));
                    ui.close();
                }
            }
            MenuItem::Submenu { label, items } => {
                ui.menu_button(label.as_str(), |ui| draw(ui, items, picked));
            }
            MenuItem::Separator => {
                ui.separator();
            }
        }
    }
}

pub(super) fn matches(label: &str, query: &str) -> bool {
    !query.is_empty() && label.to_lowercase().contains(&query.to_lowercase())
}

pub(super) fn search_target(rows: &[Row], query: &str, from: Option<&str>) -> Option<String> {
    let start = from.and_then(|id| rows.iter().position(|row| row.id == id)).unwrap_or(0);
    rows[start..].iter().chain(rows[..start].iter()).find(|row| matches(&row.label, query)).map(|row| row.id.clone())
}

#[derive(Clone, Debug)]
pub(super) struct ExecuteGoal {
    pub(super) tool: Tool,
    pub(super) text: String,
    pub(super) place: String,
    pub(super) project: String,
    pub(super) selected: Option<usize>,
    pub(super) suggestions: Vec<String>,
    pub(super) asked: Option<String>,
    pub(super) focus: bool,
}

impl ExecuteGoal {
    pub(super) fn new(tool: Tool, place: String, project: String) -> Self {
        Self {
            tool,
            text: String::new(),
            place,
            project,
            selected: None,
            suggestions: Vec::new(),
            asked: None,
            focus: true,
        }
    }

    fn place_key(&self) -> &'static str {
        match self.tool {
            Tool::Maven => "pom",
            Tool::Gradle => "projectPath",
        }
    }

    pub(super) fn complete_params(&self) -> Value {
        match self.tool {
            Tool::Maven => json!({ "text": self.text }),
            Tool::Gradle => json!({ "text": self.text, "projectPath": self.place }),
        }
    }

    pub(super) fn complete_command(&self) -> &'static str {
        match self.tool {
            Tool::Maven => "maven.goal.complete",
            Tool::Gradle => "gradle.task.complete",
        }
    }

    pub(super) fn run_command(&self) -> (&'static str, Value) {
        let line = self.text.trim();
        match self.tool {
            Tool::Maven => ("maven.goal.run", json!({ "goals": line, "pom": self.place })),
            Tool::Gradle => ("gradle.task.run", json!({ "tasks": line, "projectPath": self.place })),
        }
    }

    pub(super) fn state(&self) -> Value {
        let mut state = json!({ "text": self.text, "suggestions": self.suggestions, "selected": self.selected });
        state[self.place_key()] = json!(self.place);
        state
    }
}
