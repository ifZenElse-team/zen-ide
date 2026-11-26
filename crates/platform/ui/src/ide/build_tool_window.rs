use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::glyphs::*;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

mod tool_menu;

pub(super) use tool_menu::{
    ASSIGN_SHORTCUT, BEFORE_RUN, EFFECTIVE_POM, EXECUTE_GOAL, EXECUTE_TASK, MENU, SEARCH, TASK_BEFORE_RUN,
};
use tool_menu::{ExecuteGoal, MenuContext, MenuItem, OPEN_SETTINGS};

const READ_EVERY: f64 = 2.0;
const CHECK_EVERY: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Maven,
    Gradle,
}

impl Tool {
    pub(super) fn id(self) -> &'static str {
        match self {
            Tool::Maven => "maven",
            Tool::Gradle => "gradle",
        }
    }

    fn title(self) -> String {
        match self {
            Tool::Maven => t!("ide-maven-title"),
            Tool::Gradle => t!("ide-gradle-title"),
        }
    }

    fn reload(self) -> &'static str {
        match self {
            Tool::Maven => "maven.project.reload",
            Tool::Gradle => "gradle.project.reload",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Fix {
    pub(super) title: String,
    pub(super) command: String,
    pub(super) params: Value,
    pub(super) reload: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Banner {
    pub(super) text: String,
    pub(super) serious: bool,
    pub(super) fixes: Vec<Fix>,
}

pub(super) fn banner(model: &Value) -> Option<Banner> {
    let blocked = model.get("blocked")?;
    let fixes = blocked["fixes"].as_array().into_iter().flatten();
    Some(Banner {
        text: text(&blocked["text"]),
        serious: blocked["kind"] == "toolNotFound",
        fixes: fixes
            .map(|fix| Fix {
                title: text(&fix["title"]),
                command: text(&fix["command"]),
                params: fix["params"].clone(),
                reload: fix["reload"] == true,
            })
            .collect(),
    })
}

fn paint_banner(ui: &mut Ui, p: &IdePalette, banner: &Banner) -> Option<Fix> {
    let mut pressed = None;
    let color = if banner.serious { p.error_text } else { p.warning_text };
    egui::Frame::new().fill(p.hover).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add(egui::Label::new(egui::RichText::new(&banner.text).color(color).font(theme::regular(12.0))).wrap());
        ui.horizontal_wrapped(|ui| {
            for fix in &banner.fixes {
                if ui.button(&fix.title).clicked() {
                    pressed = Some(fix.clone());
                }
            }
        });
    });
    pressed
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RowAction {
    Goals { goals: String, pom: String },
    Tasks { tasks: String, project_path: String },
    Dependency { group: String, artifact: String, version: String },
    Profile(String),
    Open(PathBuf),
    Configuration(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowIcon {
    Project,
    Folder,
    Run,
    Plugin,
    Dependency,
    Profile,
    Hint,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) id: String,
    pub(super) depth: usize,
    pub(super) label: String,
    pub(super) detail: String,
    pub(super) icon: RowIcon,
    pub(super) parent: bool,
    pub(super) action: Option<RowAction>,
    pub(super) checked: Option<bool>,
    pub(super) muted: bool,
}

pub(super) fn row(id: String, depth: usize, label: impl Into<String>, icon: RowIcon) -> Row {
    Row {
        id,
        depth,
        label: label.into(),
        detail: String::new(),
        icon,
        parent: false,
        action: None,
        checked: None,
        muted: false,
    }
}

#[derive(Clone, Copy)]
pub(super) struct MavenInputs<'a> {
    pub(super) project: &'a Value,
    pub(super) tree: Option<&'a Value>,
    pub(super) root: &'a Path,
    pub(super) configurations: &'a [Value],
    pub(super) shortcuts: &'a BTreeMap<String, String>,
}

struct MavenWalk<'a> {
    inputs: MavenInputs<'a>,
    lifecycle: Vec<String>,
    roots: Vec<Value>,
    ignored: Vec<String>,
    group: bool,
    show_ignored: bool,
}

struct Rows<'a> {
    rows: Vec<Row>,
    expanded: &'a BTreeSet<String>,
}

impl Rows<'_> {
    fn parent(&mut self, mut row: Row) -> bool {
        row.parent = true;
        let open = self.expanded.contains(&row.id) != open_by_default(&row.id);
        self.rows.push(row);
        open
    }

    fn leaf(&mut self, row: Row) {
        self.rows.push(row);
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

pub(super) fn maven_rows(inputs: MavenInputs<'_>, expanded: &BTreeSet<String>) -> Vec<Row> {
    let project = inputs.project;
    let mut rows = Rows { rows: Vec::new(), expanded };
    let selected: Vec<String> = project["selectedProfiles"].as_array().into_iter().flatten().map(text).collect();
    let mut profiles = all_profiles(&project["project"]);
    for linked in project["linked"].as_array().into_iter().flatten() {
        for profile in all_profiles(linked) {
            if !profiles.contains(&profile) {
                profiles.push(profile);
            }
        }
    }
    if !profiles.is_empty() && rows.parent(row("profiles".to_owned(), 0, t!("ide-maven-profiles"), RowIcon::Folder)) {
        for profile in profiles {
            let mut leaf = row(format!("profiles/{profile}"), 1, profile.clone(), RowIcon::Profile);
            leaf.checked = Some(selected.contains(&profile));
            leaf.action = Some(RowAction::Profile(profile));
            rows.leaf(leaf);
        }
    }
    let view = &project["view"];
    let walk = MavenWalk {
        inputs,
        lifecycle: project["lifecycle"].as_array().into_iter().flatten().map(text).collect(),
        roots: inputs.tree.and_then(|tree| tree["roots"].as_array().cloned()).unwrap_or_default(),
        ignored: project["ignored"].as_array().into_iter().flatten().map(text).collect(),
        group: view["groupModules"] != false,
        show_ignored: view["showIgnored"] != false,
    };
    let mut tops: Vec<(String, &Value)> = Vec::new();
    if project["rootLinked"] != false {
        tops.push((String::new(), &project["project"]));
    }
    for (index, linked) in project["linked"].as_array().into_iter().flatten().enumerate() {
        tops.push((format!("linked{index}"), linked));
    }
    for (parent, top) in tops {
        if walk.group {
            maven_project(&mut rows, &walk, top, 0, &parent);
        } else {
            let mut all = Vec::new();
            flatten(top, &mut all);
            for one in all {
                maven_project(&mut rows, &walk, one, 0, &parent);
            }
        }
    }
    rows.rows
}

fn flatten<'a>(project: &'a Value, into: &mut Vec<&'a Value>) {
    into.push(project);
    for module in project["modules"].as_array().into_iter().flatten() {
        flatten(module, into);
    }
}

pub(super) fn goal_shortcut_action(pom: &str, goals: &str) -> String {
    format!("maven.goal.run {}", json!({ "goals": goals, "pom": pom }))
}

pub(super) fn task_shortcut_action(project_path: &str, tasks: &str) -> String {
    format!("gradle.task.run {}", json!({ "tasks": tasks, "projectPath": project_path }))
}

fn configuration_pom(configuration: &Value) -> String {
    match text(&configuration["options"]["pom"]) {
        pom if pom.trim().is_empty() => "pom.xml".to_owned(),
        pom => pom.trim().to_owned(),
    }
}

fn all_profiles(project: &Value) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![project];
    while let Some(project) = stack.pop() {
        for profile in project["profiles"].as_array().into_iter().flatten() {
            let id = text(&profile["id"]);
            if !id.is_empty() && !found.contains(&id) {
                found.push(id);
            }
        }
        stack.extend(project["modules"].as_array().into_iter().flatten());
    }
    found
}

fn relative(root: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(root)
        .map_or_else(|_| path.to_owned(), |inside| inside.to_string_lossy().replace('\\', "/"))
}

fn maven_project(rows: &mut Rows<'_>, walk: &MavenWalk<'_>, project: &Value, depth: usize, parent_id: &str) {
    let root = walk.inputs.root;
    let ignored = walk.ignored.contains(&text(&project["pom"]));
    if ignored && !walk.show_ignored {
        return;
    }
    let artifact = text(&project["artifactId"]);
    let name = match text(&project["name"]) {
        name if name.is_empty() => artifact.clone(),
        name => name,
    };
    let pom = text(&project["pom"]);
    let id = format!("{parent_id}/@{artifact}");
    let mut head = row(id.clone(), depth, name, RowIcon::Project);
    head.action = Some(RowAction::Open(PathBuf::from(&pom)));
    head.muted = ignored;
    if ignored {
        head.detail = t!("ide-maven-ignored");
    }
    if let Some(error) = project["error"].as_str() {
        head.detail = error.lines().next().unwrap_or_default().to_owned();
        rows.leaf(head);
        return;
    }
    if !rows.parent(head) {
        return;
    }
    let pom = relative(root, &pom);
    let inner = depth + 1;
    let shortcut =
        |goals: &str| walk.inputs.shortcuts.get(&goal_shortcut_action(&pom, goals)).cloned().unwrap_or_default();
    if rows.parent(row(format!("{id}/lifecycle"), inner, t!("ide-maven-lifecycle"), RowIcon::Folder)) {
        for phase in &walk.lifecycle {
            let mut leaf = row(format!("{id}/lifecycle/{phase}"), inner + 1, phase.clone(), RowIcon::Run);
            leaf.detail = shortcut(phase);
            leaf.action = Some(RowAction::Goals { goals: phase.clone(), pom: pom.clone() });
            rows.leaf(leaf);
        }
    }
    let plugins = project["plugins"].as_array().cloned().unwrap_or_default();
    if !plugins.is_empty() && rows.parent(row(format!("{id}/plugins"), inner, t!("ide-maven-plugins"), RowIcon::Folder))
    {
        for plugin in &plugins {
            let prefix = text(&plugin["prefix"]);
            let plugin_id = format!("{id}/plugins/{prefix}");
            let mut node = row(plugin_id.clone(), inner + 1, prefix.clone(), RowIcon::Plugin);
            node.detail = format!(
                "({}:{}{})",
                text(&plugin["groupId"]),
                text(&plugin["artifactId"]),
                match text(&plugin["version"]) {
                    version if version.is_empty() => String::new(),
                    version => format!(":{version}"),
                }
            );
            if rows.parent(node) {
                for goal in plugin["goals"].as_array().into_iter().flatten().map(text) {
                    let goals = format!("{prefix}:{goal}");
                    let mut leaf = row(format!("{plugin_id}/{goal}"), inner + 2, goals.clone(), RowIcon::Run);
                    leaf.detail = shortcut(&goals);
                    leaf.action = Some(RowAction::Goals { goals, pom: pom.clone() });
                    rows.leaf(leaf);
                }
            }
        }
    }
    let resolved = walk
        .roots
        .iter()
        .find(|node| text(&node["artifactId"]) == artifact && text(&node["groupId"]) == text(&project["groupId"]));
    let declared = project["dependencies"].as_array().cloned().unwrap_or_default();
    let children = resolved.and_then(|node| node["children"].as_array().cloned()).unwrap_or_default();
    if (resolved.is_some() && !children.is_empty()) || (resolved.is_none() && !declared.is_empty()) {
        let open = rows.parent(row(format!("{id}/dependencies"), inner, t!("ide-maven-dependencies"), RowIcon::Folder));
        if open {
            match resolved {
                Some(_) => dependency_rows(rows, &children, inner + 1, &format!("{id}/dependencies"), Tool::Maven),
                None => {
                    for dependency in &declared {
                        let coordinates =
                            format!("{}:{}", text(&dependency["groupId"]), text(&dependency["artifactId"]));
                        let mut leaf = row(
                            format!("{id}/dependencies/{coordinates}"),
                            inner + 1,
                            coordinates,
                            RowIcon::Dependency,
                        );
                        let (version, scope) = (text(&dependency["version"]), text(&dependency["scope"]));
                        leaf.detail =
                            [version.clone(), if scope == "compile" { String::new() } else { format!("({scope})") }]
                                .into_iter()
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ");
                        leaf.action = Some(RowAction::Dependency {
                            group: text(&dependency["groupId"]),
                            artifact: text(&dependency["artifactId"]),
                            version,
                        });
                        rows.leaf(leaf);
                    }
                }
            }
        }
    }
    let configurations: Vec<&Value> = walk
        .inputs
        .configurations
        .iter()
        .filter(|configuration| configuration["kind"] == "maven" && configuration_pom(configuration) == pom)
        .collect();
    if !configurations.is_empty()
        && rows.parent(row(format!("{id}/configurations"), inner, t!("ide-build-run-configurations"), RowIcon::Folder))
    {
        for configuration in configurations {
            let configuration_id = text(&configuration["id"]);
            let mut leaf = row(
                format!("{id}/configurations/{configuration_id}"),
                inner + 1,
                text(&configuration["name"]),
                RowIcon::Run,
            );
            leaf.action = Some(RowAction::Configuration(configuration_id));
            rows.leaf(leaf);
        }
    }
    let modules = project["modules"].as_array().cloned().unwrap_or_default();
    if walk.group
        && !modules.is_empty()
        && rows.parent(row(format!("{id}/modules"), inner, t!("ide-maven-modules"), RowIcon::Folder))
    {
        for module in &modules {
            maven_project(rows, walk, module, inner + 1, &format!("{id}/modules"));
        }
    }
}

fn dependency_label(node: &Value) -> String {
    match (text(&node["groupId"]), text(&node["project"])) {
        (group, _) if !group.is_empty() => format!("{group}:{}", text(&node["artifactId"])),
        (_, project) if !project.is_empty() => t!("ide-gradle-project-dependency", project = project.as_str()),
        _ => text(&node["requested"]),
    }
}

fn dependency_detail(node: &Value) -> String {
    let scope = text(&node["scope"]);
    let requested = text(&node["requested"]);
    let selected = node["groupId"].is_string() && !requested.is_empty();
    [
        text(&node["version"]),
        if scope == "compile" || scope.is_empty() { String::new() } else { format!("({scope})") },
        text(&node["note"]),
        if selected { t!("ide-gradle-dependency-requested", requested = requested.as_str()) } else { String::new() },
        if node["unresolved"].is_string() { t!("ide-gradle-dependency-unresolved") } else { String::new() },
        if node["repeated"] == true { "(*)".to_owned() } else { String::new() },
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn dependency_rows(rows: &mut Rows<'_>, nodes: &[Value], depth: usize, parent: &str, tool: Tool) {
    for node in nodes {
        let label = dependency_label(node);
        let id = format!("{parent}/{label}");
        let mut entry = row(id.clone(), depth, label, RowIcon::Dependency);
        entry.detail = dependency_detail(node);
        if tool == Tool::Maven {
            entry.action = Some(RowAction::Dependency {
                group: text(&node["groupId"]),
                artifact: text(&node["artifactId"]),
                version: text(&node["version"]),
            });
        }
        let children = node["children"].as_array().cloned().unwrap_or_default();
        if children.is_empty() {
            rows.leaf(entry);
        } else if rows.parent(entry) {
            dependency_rows(rows, &children, depth + 1, &id, tool);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GradleInputs<'a> {
    pub(super) model: &'a Value,
    pub(super) configurations: &'a [Value],
    pub(super) shortcuts: &'a BTreeMap<String, String>,
}

pub(super) fn linked_index(id: &str) -> Option<usize> {
    let rest = id.strip_prefix("linked")?;
    rest.split('/').next()?.parse::<usize>().ok().map(|index| index + 1)
}

fn build_prefix(index: usize) -> String {
    if index == 0 { String::new() } else { format!("linked{}/", index - 1) }
}

fn task_groups(tasks: &[Value]) -> Vec<String> {
    let groups: BTreeSet<String> = tasks.iter().map(|task| text(&task["group"])).collect();
    let (other, mut named): (Vec<String>, Vec<String>) = groups.into_iter().partition(|group| group == "other");
    named.extend(other);
    named
}

fn task_path(project: &str, name: &str) -> String {
    if project == ":" { format!(":{name}") } else { format!("{project}:{name}") }
}

fn gradle_project(
    rows: &mut Rows<'_>,
    inputs: GradleInputs<'_>,
    project: &Value,
    project_path: &str,
    id: &str,
    depth: usize,
) -> bool {
    let path = text(&project["path"]);
    let build_root = project["build"] == path.as_str();
    let mut head = row(id.to_owned(), depth, text(&project["name"]), RowIcon::Project);
    if let Some(file) = project["buildFile"].as_str() {
        head.action = Some(RowAction::Open(PathBuf::from(file)));
    }
    if build_root && path != ":" && path != ":buildSrc" {
        head.detail = t!("ide-gradle-included-build");
    }
    if !rows.parent(head) {
        return false;
    }
    let inner = depth + 1;
    let tasks = project["tasks"].as_array().cloned().unwrap_or_default();
    if !tasks.is_empty() && rows.parent(row(format!("{id}/tasks"), inner, t!("ide-gradle-tasks"), RowIcon::Folder)) {
        for group in task_groups(&tasks) {
            let group_id = format!("{id}/tasks/{group}");
            if !rows.parent(row(group_id.clone(), inner + 1, group.clone(), RowIcon::Folder)) {
                continue;
            }
            for task in tasks.iter().filter(|task| text(&task["group"]) == group) {
                let name = text(&task["name"]);
                let full = task_path(&path, &name);
                let shortcut = inputs.shortcuts.get(&task_shortcut_action(project_path, &full));
                let mut leaf = row(format!("{group_id}/{name}"), inner + 2, name, RowIcon::Run);
                leaf.detail = shortcut.cloned().unwrap_or_else(|| text(&task["description"]));
                leaf.action = Some(RowAction::Tasks { tasks: full, project_path: project_path.to_owned() });
                rows.leaf(leaf);
            }
        }
    }
    let configurations: Vec<&Value> = project["configurations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|configuration| {
            configuration["error"].is_string()
                || configuration["dependencies"].as_array().is_some_and(|nodes| !nodes.is_empty())
        })
        .collect();
    let dependencies = format!("{id}/dependencies");
    let label = t!("ide-maven-dependencies");
    if !configurations.is_empty() && rows.parent(row(dependencies.clone(), inner, label, RowIcon::Folder)) {
        for configuration in configurations {
            let name = text(&configuration["name"]);
            let node_id = format!("{dependencies}/{name}");
            let mut node = row(node_id.clone(), inner + 1, name, RowIcon::Folder);
            if let Some(error) = configuration["error"].as_str() {
                node.detail = error.lines().next().unwrap_or_default().to_owned();
            }
            if rows.parent(node) {
                let nodes = configuration["dependencies"].as_array().cloned().unwrap_or_default();
                dependency_rows(rows, &nodes, inner + 2, &node_id, Tool::Gradle);
            }
        }
    }
    let saved: Vec<&Value> = inputs
        .configurations
        .iter()
        .filter(|configuration| {
            let own = match text(&configuration["options"]["projectPath"]) {
                folder if folder.trim().is_empty() => ".".to_owned(),
                folder => folder,
            };
            path == ":" && configuration["kind"] == "gradle" && own == project_path
        })
        .collect();
    let label = t!("ide-build-run-configurations");
    if !saved.is_empty() && rows.parent(row(format!("{id}/configurations"), inner, label, RowIcon::Folder)) {
        for configuration in saved {
            let configuration_id = text(&configuration["id"]);
            let entry = format!("{id}/configurations/{configuration_id}");
            let mut leaf = row(entry, inner + 1, text(&configuration["name"]), RowIcon::Run);
            leaf.action = Some(RowAction::Configuration(configuration_id));
            rows.leaf(leaf);
        }
    }
    true
}

pub(super) fn gradle_rows(inputs: GradleInputs<'_>, expanded: &BTreeSet<String>) -> Vec<Row> {
    let mut rows = Rows { rows: Vec::new(), expanded };
    let builds = inputs.model["builds"].as_array().cloned().unwrap_or_default();
    for (index, build) in builds.iter().enumerate() {
        let prefix = build_prefix(index);
        let project_path = text(&build["projectPath"]);
        let projects = build["projects"].as_array().cloned().unwrap_or_default();
        let empty = projects.iter().all(|project| project["tasks"].as_array().is_none_or(Vec::is_empty));
        let mut open = BTreeMap::new();
        for project in &projects {
            let path = text(&project["path"]);
            let owner = text(&project["build"]);
            let id = format!("{prefix}@{path}");
            if path == owner {
                let shown = gradle_project(&mut rows, inputs, project, &project_path, &id, 0);
                if shown && path == ":" && empty {
                    rows.leaf(row(format!("{id}/hint"), 1, t!("ide-gradle-reload-hint"), RowIcon::Hint));
                }
                open.insert(owner, shown);
            } else if open.get(&owner) == Some(&true) {
                gradle_project(&mut rows, inputs, project, &project_path, &id, 1);
            }
        }
    }
    rows.rows
}

#[derive(Default)]
pub(super) struct BuildToolWindow {
    pub(super) maven: bool,
    pub(super) gradle: bool,
    pub(super) ant: bool,
    pub(super) npm: bool,
    pub(super) endpoints: bool,
    pub(super) dependencies: bool,
    checked: Option<(PathBuf, u64, f64)>,
    maven_project: Option<Value>,
    maven_tree: Option<Value>,
    gradle_build: Option<Value>,
    gradle_model: Option<Value>,
    gradle_seen: String,
    gradle_imported: Option<PathBuf>,
    maven_error: Option<String>,
    gradle_error: Option<String>,
    read: [f64; 2],
    expanded: [BTreeSet<String>; 2],
    selected: [Option<String>; 2],
    configurations: Vec<Value>,
    triggers: Vec<Value>,
    search: [String; 2],
    execute: Option<ExecuteGoal>,
    assigning: Option<(String, String, Option<zen_core::keymap::Keystroke>)>,
    pending_effective: Option<String>,
    focus_next: bool,
}

impl BuildToolWindow {
    pub(super) fn available(&self, id: &str) -> bool {
        match id {
            "maven" => self.maven,
            "gradle" => self.gradle,
            "build" => self.maven || self.gradle,
            "ant" => self.ant,
            "npm" => self.npm,
            "endpoints" => self.endpoints,
            "dependencies" => self.dependencies,
            _ => true,
        }
    }

    fn slot(tool: Tool) -> usize {
        match tool {
            Tool::Maven => 0,
            Tool::Gradle => 1,
        }
    }

    pub(super) fn stale(&mut self, tool: Tool) {
        self.read[Self::slot(tool)] = f64::NEG_INFINITY;
    }

    pub(super) fn read_now(&mut self, tool: Tool, registry: &Registry, session: &mut Session) {
        self.stale(tool);
        self.read(tool, registry, session, f64::MAX);
        self.stale(tool);
    }

    fn read(&mut self, tool: Tool, registry: &Registry, session: &mut Session, now: f64) {
        let slot = Self::slot(tool);
        if now - self.read[slot] < READ_EVERY {
            return;
        }
        self.read[slot] = now;
        self.configurations = registry
            .dispatch(session, "run.configuration.list", json!({}))
            .ok()
            .and_then(|list| list["configurations"].as_array().cloned())
            .unwrap_or_default();
        self.triggers = registry
            .dispatch(session, "build.triggers.list", json!({}))
            .ok()
            .and_then(|list| list["triggers"].as_array().cloned())
            .unwrap_or_default();
        match tool {
            Tool::Maven => match registry.dispatch(session, "maven.project.get", json!({})) {
                Ok(project) => {
                    if let Some(pom) = self.pending_effective.clone() {
                        match registry.dispatch(
                            session,
                            "maven.effectivePom.show",
                            json!({ "pom": pom, "start": false }),
                        ) {
                            Ok(shown) if shown["state"] == "loading" => {}
                            Ok(shown) => {
                                self.pending_effective = None;
                                self.maven_error = shown["error"].as_str().map(str::to_owned);
                            }
                            Err(error) => {
                                self.pending_effective = None;
                                self.maven_error = Some(error.to_string());
                            }
                        }
                    }
                    let loaded = project["dependencyTree"]["state"] == "loaded";
                    self.maven_tree =
                        loaded.then(|| registry.dispatch(session, "maven.dependencies.tree", json!({})).ok()).flatten();
                    self.maven_project = Some(project);
                    if self.pending_effective.is_none() {
                        self.maven_error = None;
                    }
                }
                Err(error) => self.maven_error = Some(error.text()),
            },
            Tool::Gradle => match registry.dispatch(session, "gradle.project.get", json!({})) {
                Ok(build) => {
                    let seen = format!("{} {} {}", build["modelHash"], build["linked"], build["modelState"]);
                    if seen != self.gradle_seen || self.gradle_model.is_none() {
                        self.gradle_model = registry.dispatch(session, "gradle.model.get", json!({})).ok();
                        self.gradle_seen = seen;
                    }
                    self.gradle_build = Some(build);
                    self.gradle_error = None;
                }
                Err(error) => self.gradle_error = Some(error.text()),
            },
        }
    }

    fn rows(&self, tool: Tool, root: &Path) -> Vec<Row> {
        let expanded = &self.expanded[Self::slot(tool)];
        match tool {
            Tool::Maven => {
                let shortcuts = goal_shortcuts();
                self.maven_project
                    .as_ref()
                    .map(|project| {
                        let inputs = MavenInputs {
                            project,
                            tree: self.maven_tree.as_ref(),
                            root,
                            configurations: &self.configurations,
                            shortcuts: &shortcuts,
                        };
                        maven_rows(inputs, expanded)
                    })
                    .unwrap_or_default()
            }
            Tool::Gradle => {
                let shortcuts = goal_shortcuts();
                self.gradle_model
                    .as_ref()
                    .map(|model| {
                        let inputs =
                            GradleInputs { model, configurations: &self.configurations, shortcuts: &shortcuts };
                        gradle_rows(inputs, expanded)
                    })
                    .unwrap_or_default()
            }
        }
    }

    fn menu(&self, tool: Tool, row: &Row) -> Vec<MenuItem> {
        let empty = Value::Null;
        let builds = self.gradle_model.as_ref().and_then(|model| model["builds"].as_array());
        let context = MenuContext {
            project: self.maven_project.as_ref().unwrap_or(&empty),
            builds: builds.map_or(&[], Vec::as_slice),
            triggers: &self.triggers,
            configurations: &self.configurations,
        };
        match tool {
            Tool::Maven => tool_menu::maven_menu(row, &context),
            Tool::Gradle => tool_menu::gradle_menu(row, &context),
        }
    }

    fn loading(&self, tool: Tool) -> bool {
        match tool {
            Tool::Maven => self.maven_project.as_ref().is_some_and(|p| p["dependencyTree"]["state"] == "loading"),
            Tool::Gradle => {
                let states = self.gradle_build.as_ref().and_then(|build| build["modelState"].as_array());
                states.is_some_and(|states| states.iter().any(|state| state["state"] == "loading"))
            }
        }
    }

    fn model(&self, tool: Tool) -> Option<&Value> {
        match tool {
            Tool::Maven => self.maven_project.as_ref(),
            Tool::Gradle => self.gradle_build.as_ref(),
        }
    }

    fn failure(&self, tool: Tool) -> Option<String> {
        let value = match tool {
            Tool::Maven => self.maven_project.as_ref().map(|p| &p["dependencyTree"]["error"]),
            Tool::Gradle => self.gradle_build.as_ref().and_then(|build| {
                build["modelState"].as_array()?.iter().map(|state| &state["error"]).find(|error| error.is_string())
            }),
        };
        value.and_then(Value::as_str).map(|error| error.lines().next().unwrap_or_default().to_owned())
    }

    pub(super) fn state(&self, root: &Path, shown: impl Fn(&str) -> bool) -> Value {
        let one = |tool: Tool, available: bool| {
            let slot = Self::slot(tool);
            let mut state = json!({ "available": available, "selected": self.selected[slot] });
            if available && shown(tool.id()) {
                let rows: Vec<Value> =
                    self.rows(tool, root).iter().map(|row| row_state(row, &self.expanded[slot])).collect();
                state["rows"] = Value::Array(rows);
                state["loading"] = json!(self.loading(tool));
                if !self.search[slot].is_empty() {
                    state["search"] = json!(self.search[slot]);
                }
                if let Some(execute) = self.execute.as_ref().filter(|execute| execute.tool == tool) {
                    state[if tool == Tool::Maven { "executeGoal" } else { "executeTask" }] = execute.state();
                }
                if let Some(found) = self.model(tool).and_then(|model| tool_line(tool, model)) {
                    state["found"] = json!(found);
                }
                if let Some(blocked) = self.model(tool).and_then(|model| model.get("blocked")) {
                    state["blocked"] = blocked.clone();
                }
            }
            state
        };
        json!({ "maven": one(Tool::Maven, self.maven), "gradle": one(Tool::Gradle, self.gradle) })
    }
}

pub(super) fn tool_line(tool: Tool, model: &Value) -> Option<String> {
    let executable = model.get("executable")?;
    let source = match executable["source"].as_str()? {
        "projectWrapper" => t!("ide-build-source-wrapper"),
        "wrapperDistribution" => t!("ide-build-source-distribution"),
        "sdkman" => t!("ide-build-source-sdkman"),
        "downloaded" => t!("ide-build-source-downloaded"),
        "custom" => t!("ide-build-source-custom"),
        _ => t!("ide-build-source-path"),
    };
    let name = match tool {
        Tool::Maven => t!("ide-maven-title"),
        Tool::Gradle => t!("ide-gradle-title"),
    };
    Some(match executable["version"].as_str() {
        Some(version) => t!("ide-build-found-version", name = name, version = version, source = source),
        None => t!("ide-build-found", name = name, source = source),
    })
}

fn row_state(row: &Row, expanded: &BTreeSet<String>) -> Value {
    let runs = row.action.as_ref().map(|action| match action {
        RowAction::Goals { .. } => "goals",
        RowAction::Tasks { .. } => "tasks",
        RowAction::Dependency { .. } => "dependency",
        RowAction::Profile(_) => "profile",
        RowAction::Open(_) => "open",
        RowAction::Configuration(_) => "configuration",
    });
    let mut state = json!({ "id": row.id, "depth": row.depth, "label": row.label });
    if !row.detail.is_empty() {
        state["detail"] = json!(row.detail);
    }
    if row.parent {
        state["open"] = json!(expanded.contains(&row.id) != open_by_default(&row.id));
    }
    if let Some(runs) = runs {
        state["runs"] = json!(runs);
    }
    if let Some(checked) = row.checked {
        state["checked"] = json!(checked);
    }
    if row.muted {
        state["muted"] = json!(true);
    }
    state
}

fn goal_shortcuts() -> BTreeMap<String, String> {
    crate::keymap::current()
        .user_changes()
        .iter()
        .filter(|(action, keys)| {
            (action.starts_with("maven.goal.run ") || action.starts_with("gradle.task.run ")) && !keys.is_empty()
        })
        .map(|(action, keys)| (action.clone(), keys.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")))
        .collect()
}

fn project_pom(row: &Row) -> Option<String> {
    match (&row.action, row.icon) {
        (Some(RowAction::Open(pom)), RowIcon::Project) => Some(pom.to_string_lossy().into_owned()),
        (Some(RowAction::Goals { pom, .. }), _) => Some(pom.clone()),
        _ => None,
    }
}

pub(super) fn tool_param(params: &Value) -> Result<Tool, String> {
    match params.get("tool").and_then(Value::as_str) {
        Some("maven") => Ok(Tool::Maven),
        Some("gradle") => Ok(Tool::Gradle),
        _ => Err("`tool` is maven or gradle".to_owned()),
    }
}

fn run_action(registry: &Registry, session: &mut Session, action: &RowAction) -> Result<(), String> {
    let (command, params) = match action {
        RowAction::Goals { goals, pom } => ("maven.goal.run", json!({ "goals": goals, "pom": pom })),
        RowAction::Tasks { tasks, project_path } => {
            ("gradle.task.run", json!({ "tasks": tasks, "projectPath": project_path }))
        }
        RowAction::Dependency { group, artifact, version } => {
            ("maven.dependency.open", json!({ "groupId": group, "artifactId": artifact, "version": version }))
        }
        RowAction::Profile(profile) => {
            let project = registry.dispatch(session, "maven.project.get", json!({})).map_err(|e| e.text())?;
            let mut chosen: Vec<String> =
                project["selectedProfiles"].as_array().into_iter().flatten().map(text).collect();
            match chosen.iter().position(|known| known == profile) {
                Some(at) => {
                    chosen.remove(at);
                }
                None => chosen.push(profile.clone()),
            }
            ("maven.options.set", json!({ "profiles": chosen }))
        }
        RowAction::Open(path) => ("file.document.open", json!({ "path": path })),
        RowAction::Configuration(id) => ("run.process.start", json!({ "configurationId": id })),
    };
    registry.dispatch(session, command, params).map(|_| ()).map_err(|error| error.text())
}

impl IdeView {
    pub(super) fn build_tool_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        let tool = tool_param(params)?;
        if !self.build_tools.available(tool.id()) {
            return Err(match tool {
                Tool::Maven => t!("ide-maven-not-available"),
                Tool::Gradle => t!("ide-gradle-not-available"),
            });
        }
        if !self.tool_visible(tool.id()) {
            self.show_tool(tool.id());
        }
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        self.build_tools.read_now(tool, registry, session);
        let slot = BuildToolWindow::slot(tool);
        let rows = self.build_tools.rows(tool, &root);
        let named = params.get("row").and_then(Value::as_str).map(str::to_owned);
        let id = match (action, named) {
            (_, Some(id)) => id,
            ("view.buildTool.run", None) => {
                self.build_tools.selected[slot].clone().ok_or("no row is selected; give `row`")?
            }
            _ => return Err("`row` is required: an id of get_window_state's buildTools rows".to_owned()),
        };
        let row = rows
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .ok_or_else(|| format!("no row `{id}` in the {} tool window (open its parent first)", tool.id()))?;
        self.build_tools.selected[slot] = Some(row.id.clone());
        match action {
            "view.buildTool.select" => {}
            "view.buildTool.toggle" if row.parent => self.toggle_row(tool, &row),
            "view.buildTool.toggle" => return Err(format!("`{id}` has no rows under it")),
            _ => {
                let run = row.action.as_ref().ok_or_else(|| format!("`{id}` runs nothing"))?;
                run_action(registry, session, run)?;
                self.build_tools.stale(tool);
            }
        }
        Ok(json!({ "selected": row.id }))
    }

    pub(super) fn tool_window_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        let tool = if action.starts_with("view.maven.") {
            Tool::Maven
        } else if action.starts_with("view.gradle.") {
            Tool::Gradle
        } else {
            tool_param(params)?
        };
        if !self.build_tools.available(tool.id()) {
            return Err(match tool {
                Tool::Maven => t!("ide-maven-not-available"),
                Tool::Gradle => t!("ide-gradle-not-available"),
            });
        }
        if !self.tool_visible(tool.id()) {
            self.show_tool(tool.id());
        }
        self.build_tools.read_now(tool, registry, session);
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let slot = BuildToolWindow::slot(tool);
        let text_param = |key: &str| params.get(key).and_then(Value::as_str).map(str::to_owned);
        match action {
            MENU => {
                let id = text_param("row").ok_or("`row` is required: an id of get_window_state's buildTools rows")?;
                let rows = self.build_tools.rows(tool, &root);
                let row = rows.iter().find(|row| row.id == id).ok_or_else(|| format!("no row `{id}`"))?;
                let items = self.build_tools.menu(tool, row);
                Ok(json!({ "row": id, "items": tool_menu::menu_state(&items) }))
            }
            SEARCH => {
                let query = text_param("text").unwrap_or_default();
                let rows = self.build_tools.rows(tool, &root);
                let found = tool_menu::search_target(&rows, &query, None);
                if let Some(found) = &found {
                    self.build_tools.selected[slot] = Some(found.clone());
                }
                self.build_tools.search[slot] = query;
                Ok(json!({ "selected": found }))
            }
            EXECUTE_GOAL => match text_param("goals") {
                Some(goals) => {
                    let pom = text_param("pom").unwrap_or_else(|| self.selected_pom());
                    let started = registry
                        .dispatch(session, "maven.goal.run", json!({ "goals": goals, "pom": pom }))
                        .map_err(|error| error.to_string())?;
                    self.build_tools.execute = None;
                    Ok(started)
                }
                None => {
                    self.open_execute(Tool::Maven);
                    Ok(json!({ "executeGoal": self.build_tools.execute.as_ref().map(ExecuteGoal::state) }))
                }
            },
            EXECUTE_TASK => match text_param("tasks") {
                Some(tasks) => {
                    let project_path = text_param("projectPath").unwrap_or_else(|| self.selected_build());
                    let run = json!({ "tasks": tasks, "projectPath": project_path });
                    let started =
                        registry.dispatch(session, "gradle.task.run", run).map_err(|error| error.to_string())?;
                    self.build_tools.execute = None;
                    Ok(started)
                }
                None => {
                    self.open_execute(Tool::Gradle);
                    Ok(json!({ "executeTask": self.build_tools.execute.as_ref().map(ExecuteGoal::state) }))
                }
            },
            EFFECTIVE_POM => {
                let pom = text_param("pom").unwrap_or_else(|| self.selected_pom());
                let shown = registry
                    .dispatch(session, "maven.effectivePom.show", json!({ "pom": pom }))
                    .map_err(|error| error.to_string())?;
                if shown["state"] == "loading" {
                    self.build_tools.pending_effective = Some(pom);
                }
                if let Some(error) = shown["error"].as_str() {
                    return Err(error.to_owned());
                }
                Ok(shown)
            }
            BEFORE_RUN | TASK_BEFORE_RUN => {
                let id = text_param("configurationId").ok_or("`configurationId` is required")?;
                let task = params.get("task").cloned().ok_or("`task` is required")?;
                let list = registry
                    .dispatch(session, "run.beforeLaunch.list", json!({ "configurationId": id }))
                    .map_err(|error| error.to_string())?;
                let at = list["tasks"].as_array().into_iter().flatten().position(|known| {
                    let mut known = known.clone();
                    if let Some(fields) = known.as_object_mut() {
                        fields.remove("index");
                        fields.remove("label");
                    }
                    known == task
                });
                let answer = match at {
                    Some(index) => registry.dispatch(
                        session,
                        "run.beforeLaunch.remove",
                        json!({ "configurationId": id, "index": index }),
                    ),
                    None => registry.dispatch(
                        session,
                        "run.beforeLaunch.add",
                        json!({ "configurationId": id, "task": task }),
                    ),
                };
                self.build_tools.stale(tool);
                answer.map_err(|error| error.to_string())
            }
            ASSIGN_SHORTCUT => {
                let shortcut = text_param("action").ok_or("`action` is required")?;
                let title = text_param("title").unwrap_or_else(|| shortcut.clone());
                self.build_tools.assigning = Some((shortcut.clone(), title, None));
                Ok(json!({ "assigning": shortcut }))
            }
            _ => Err(format!("unknown action `{action}`")),
        }
    }

    fn selected_pom(&self) -> String {
        let root = PathBuf::new();
        let rows = self.build_tools.rows(Tool::Maven, &root);
        let selected = self.build_tools.selected[0].as_ref();
        selected
            .and_then(|id| rows.iter().find(|row| &row.id == id))
            .and_then(project_pom)
            .unwrap_or_else(|| "pom.xml".to_owned())
    }

    fn selected_build(&self) -> String {
        let root = PathBuf::new();
        let rows = self.build_tools.rows(Tool::Gradle, &root);
        let selected = self.build_tools.selected[1].as_ref();
        let index = selected.and_then(|id| rows.iter().find(|row| &row.id == id)).and_then(|row| linked_index(&row.id));
        let builds = self.build_tools.gradle_model.as_ref().and_then(|model| model["builds"].as_array());
        let build = builds.and_then(|builds| builds.get(index.unwrap_or(0)));
        build.map_or_else(|| ".".to_owned(), |build| text(&build["projectPath"]))
    }

    fn open_execute(&mut self, tool: Tool) {
        let opened = match tool {
            Tool::Maven => {
                let project = self.build_tools.maven_project.as_ref();
                let name = project.map(|project| text(&project["project"]["artifactId"]));
                ExecuteGoal::new(tool, self.selected_pom(), name.unwrap_or_default())
            }
            Tool::Gradle => {
                let project = self.build_tools.gradle_build.as_ref();
                let name = project.map(|build| text(&build["projects"][0]["name"]));
                ExecuteGoal::new(tool, self.selected_build(), name.unwrap_or_default())
            }
        };
        self.build_tools.execute = Some(opened);
    }

    fn menu_command(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        command: &str,
        params: &Value,
    ) -> Result<(), String> {
        match command {
            EFFECTIVE_POM | BEFORE_RUN | TASK_BEFORE_RUN | ASSIGN_SHORTCUT => {
                self.tool_window_action(registry, session, command, params).map(|_| ())
            }
            OPEN_SETTINGS => self
                .perform_action(ctx, registry, session, command, params.clone(), false)
                .map(|_| ())
                .map_err(|error| match error {
                    super::agent::ActionError::Refused(message) => message,
                    super::agent::ActionError::Dispatch(error) => error.to_string(),
                }),
            _ => registry.dispatch(session, command, params.clone()).map(|_| ()).map_err(|error| error.to_string()),
        }
    }

    pub(super) fn follow_build_tools(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        for tool in [Tool::Maven, Tool::Gradle] {
            if self.build_tools.available(tool.id()) && self.tool_visible(tool.id()) {
                self.build_tools.read(tool, registry, session, now);
            }
        }
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            self.build_tools.maven = false;
            self.build_tools.gradle = false;
            return;
        };
        let generation = self.plugins.generation();
        let fresh = self.build_tools.checked.as_ref().is_some_and(|(seen, at_generation, at)| {
            *seen == root && *at_generation == generation && now - at < CHECK_EVERY
        });
        if fresh {
            return;
        }
        self.build_tools.checked = Some((root.clone(), generation, now));
        let supporting: Vec<String> = session
            .state::<zen_workbench::plugin_projects::PluginProjects>()
            .map(|projects| projects.supporting(&root).into_iter().map(|plugin| plugin.id).collect())
            .unwrap_or_default();
        self.build_tools.maven = supporting.iter().any(|id| id == "maven") && root.join("pom.xml").is_file();
        self.build_tools.gradle =
            supporting.iter().any(|id| id == "gradle") && zen_workbench::gradle::model::is_build(&root);
        for id in ["maven", "gradle", "build"] {
            if !self.build_tools.available(id) && self.tool_visible(id) {
                self.hide_tool(id);
            }
        }
        if !self.build_tools.gradle {
            self.build_tools.gradle_imported = None;
        } else if self.build_tools.gradle_imported.as_ref() != Some(&root) {
            let answer = registry.dispatch(session, "gradle.model.get", json!({ "load": true }));
            if answer.is_err() || answer.is_ok_and(|answer| answer["trusted"] == true) {
                self.build_tools.gradle_imported = Some(root);
                self.build_tools.stale(Tool::Gradle);
            }
        }
    }

    pub(super) fn toggle_build_tool(&mut self, tool: Tool) -> Result<(), String> {
        if !self.build_tools.available(tool.id()) {
            return Err(match tool {
                Tool::Maven => t!("ide-maven-not-available"),
                Tool::Gradle => t!("ide-gradle-not-available"),
            });
        }
        self.toggle_tool(tool.id());
        Ok(())
    }

    pub(super) fn build_tool_window(&mut self, ui: &mut Ui, tool: Tool, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        self.build_tools.read(tool, registry, session, now);
        let slot = BuildToolWindow::slot(tool);
        let loading = self.build_tools.loading(tool);
        if loading {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        let rows = self.build_tools.rows(tool, &root);
        let selected = self.build_tools.selected[slot].clone();
        let selected_action = selected
            .as_ref()
            .and_then(|id| rows.iter().find(|row| &row.id == id))
            .and_then(|row| row.action.clone())
            .filter(|action| matches!(action, RowAction::Goals { .. } | RowAction::Tasks { .. }));

        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            tool.title(),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with(("build-tool-title", tool.id())), title, || tool.title());
        let mut x = title.right() + 14.0;
        let mut button =
            |ui: &mut Ui, key: &str, glyph: Glyph, label: String, on: Option<bool>, enabled: bool| -> bool {
                let rect = Rect::from_center_size(pos2(x + 12.0, header.center().y), Vec2::splat(24.0));
                x += 25.0;
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("build-tool", tool.id(), key)),
                    if enabled { Sense::click() } else { Sense::hover() },
                );
                if on == Some(true) {
                    ui.painter().rect_filled(rect, 5, p.hover);
                }
                if enabled {
                    hover(ui, &response, &p);
                }
                let color = if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) };
                glyph(ui.painter(), rect.shrink(6.0), color);
                hint(ui, &response, &label, None, Side::Below);
                accessibility::button(&response, &label);
                if let Some(on) = on {
                    accessibility::pressed(&response, on);
                }
                if !enabled {
                    accessibility::disabled(&response);
                }
                enabled && response.clicked()
            };
        let mut actions: Vec<(&str, Value)> = Vec::new();
        let mut run_selected = false;
        let mut add_project = false;
        let mut execute = false;
        let mut settings = false;
        match tool {
            Tool::Maven => {
                if button(ui, "reload", glyph_refresh, t!("ide-maven-reload"), None, !loading) {
                    actions.push(("maven.project.reload", json!({})));
                }
                if button(ui, "run", glyph_run, t!("ide-build-run-selected"), None, selected_action.is_some()) {
                    run_selected = true;
                }
                let project = self.build_tools.maven_project.clone().unwrap_or_default();
                let offline = project["offline"] == true;
                if button(ui, "offline", glyph_cloud, t!("ide-maven-offline"), Some(offline), true) {
                    actions.push(("maven.options.set", json!({ "offline": !offline })));
                }
                let skip = project["skipTests"] == true;
                if button(ui, "skip-tests", glyph_test_skipped, t!("ide-maven-skip-tests"), Some(skip), true) {
                    actions.push(("maven.options.set", json!({ "skipTests": !skip })));
                }
                if button(ui, "execute", glyph_maven, t!("ide-maven-execute-goal"), None, true) {
                    execute = true;
                }
                if button(ui, "add", glyph_plus, t!("ide-maven-add-project"), None, true) {
                    add_project = true;
                }
                if button(ui, "dependencies", glyph_tree, t!("ide-maven-show-dependencies"), None, !loading) {
                    actions.push(("maven.dependencies.tree", json!({ "load": true })));
                }
                if button(ui, "analyze", glyph_hierarchy, t!("ide-dependency-analyze"), None, true) {
                    actions.push((super::dependency_analyzer::OPEN, json!({ "system": "maven" })));
                }
            }
            Tool::Gradle => {
                if button(ui, "reload", glyph_refresh, t!("ide-gradle-reload"), None, !loading) {
                    actions.push(("gradle.project.reload", json!({})));
                }
                if button(ui, "run", glyph_run, t!("ide-build-run-selected"), None, selected_action.is_some()) {
                    run_selected = true;
                }
                if button(ui, "analyze", glyph_hierarchy, t!("ide-dependency-analyze"), None, true) {
                    actions.push((super::dependency_analyzer::OPEN, json!({ "system": "gradle" })));
                }
                if button(ui, "add", glyph_plus, t!("ide-gradle-add-project"), None, true) {
                    add_project = true;
                }
                if button(ui, "execute", glyph_gradle, t!("ide-gradle-execute-task"), None, true) {
                    execute = true;
                }
                let offline = self.build_tools.gradle_build.as_ref().is_some_and(|build| build["offline"] == true);
                if button(ui, "offline", glyph_cloud, t!("ide-maven-offline"), Some(offline), true) {
                    actions.push(("gradle.options.set", json!({ "offline": !offline })));
                }
                if button(ui, "settings", glyph_settings, t!("ide-build-tool-settings"), None, true) {
                    settings = true;
                }
            }
        }
        let mut collapse = tool == Tool::Gradle
            && button(ui, "collapse", glyph_collapse_all, t!("ide-build-collapse-all"), None, true);
        if tool == Tool::Maven {
            let rect = Rect::from_center_size(pos2(x + 12.0, header.center().y), Vec2::splat(24.0));
            let response = ui.interact(rect, ui.scope_id().with(("build-tool", tool.id(), "view")), Sense::click());
            hover(ui, &response, &p);
            glyph_eye(ui.painter(), rect.shrink(6.0), p.text_secondary);
            hint(ui, &response, &t!("ide-maven-view-options"), None, Side::Below);
            accessibility::button(&response, &t!("ide-maven-view-options"));
            let view =
                self.build_tools.maven_project.as_ref().map(|project| project["view"].clone()).unwrap_or_default();
            crate::components::popup_menu(&response, |ui| {
                ui.set_min_width(220.0);
                for (key, label) in [
                    ("groupModules", t!("ide-maven-view-group-modules")),
                    ("showIgnored", t!("ide-maven-view-show-ignored")),
                    ("basicPhases", t!("ide-maven-view-basic-phases")),
                ] {
                    let on = view[key] != false;
                    if ui.add(egui::Button::new(label).selected(on)).clicked() {
                        let mut change = json!({});
                        change[key] = json!(!on);
                        actions.push(("maven.view.set", change));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t!("ide-maven-menu-download")).clicked() {
                    actions.push(("maven.sources.download", json!({})));
                    ui.close();
                }
                if ui.button(t!("ide-build-collapse-all")).clicked() {
                    collapse = true;
                    ui.close();
                }
            });
        }
        if collapse {
            let open: Vec<String> =
                rows.iter().filter(|row| row.parent && open_by_default(&row.id)).map(|row| row.id.clone()).collect();
            self.build_tools.expanded[slot] = open.into_iter().collect();
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with(("build-tool-hide", tool.id())), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(tool.id());
        }

        if let Some(model) = self.build_tools.model(tool)
            && let Some(found) = tool_line(tool, model)
        {
            let path = model["executable"]["path"].as_str().unwrap_or_default().to_owned();
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(found).color(p.text_secondary).font(theme::regular(12.0)))
                        .truncate(),
                )
                .on_hover_text(path);
            });
        }
        let status = if loading {
            Some((t!("ide-build-loading"), false))
        } else if let Some(error) = match tool {
            Tool::Maven => self.build_tools.maven_error.clone(),
            Tool::Gradle => self.build_tools.gradle_error.clone(),
        } {
            Some((error, true))
        } else {
            self.build_tools.failure(tool).map(|error| (error, true))
        };
        let blocked = self.build_tools.model(tool).and_then(banner);
        let mut fixed = None;
        if let Some(blocked) = &blocked {
            fixed = paint_banner(ui, &p, blocked);
        }
        if let Some((message, error)) = status {
            let color = if error { p.error_text } else { p.text_secondary };
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(message).color(color).font(theme::regular(12.0)));
            });
        }

        let mut clicked: Option<(String, bool, bool)> = None;
        let mut picked: Option<(String, Value)> = None;
        let tree_focus = ui.make_persistent_id(("build-tool-tree-focus", tool.id()));
        let tree_area = ui.interact(ui.available_rect_before_wrap(), tree_focus, Sense::click());
        accessibility::name_with(&tree_area, egui::Role::Tree, || tool.title());
        if tree_area.clicked() || std::mem::take(&mut self.build_tools.focus_next) {
            ui.memory_mut(|memory| memory.request_focus(tree_focus));
        }
        let focused = ui.memory(|memory| memory.has_focus(tree_focus));
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    tree_focus,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }
        let query = self.build_tools.search[slot].clone();
        egui::ScrollArea::vertical().id_salt(("build-tool-tree", tool.id())).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &rows {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                    let is_selected = selected.as_deref() == Some(row.id.as_str());
                    let expanded = row.parent.then(|| self.row_open(tool, row));
                    accessibility::tree_item(&response, row.depth + 1, expanded, is_selected, &row.label);
                    let highlight = Rect::from_min_max(
                        pos2(rect.left() + 12.0, rect.top()),
                        pos2(rect.right() - 6.0, rect.bottom()),
                    );
                    if is_selected {
                        ui.painter().rect_filled(highlight, 5, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(highlight, 5, p.hover);
                    }
                    let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                    let chevron =
                        Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                    match expanded {
                        Some(true) => crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary),
                        Some(false) => crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary),
                        None => {}
                    }
                    if tool_menu::matches(&row.label, &query) {
                        paint_match(ui, &p, rect, x, row, &query);
                    }
                    paint_row(ui, &p, rect, x, row);
                    if tool_menu::has_menu(row) || (tool == Tool::Gradle && row.icon == RowIcon::Project) {
                        response.context_menu(|ui| {
                            ui.set_min_width(240.0);
                            let items = self.build_tools.menu(tool, row);
                            tool_menu::draw(ui, &items, &mut picked);
                        });
                        if response.secondary_clicked() {
                            self.build_tools.selected[slot] = Some(row.id.clone());
                        }
                    }
                    if response.clicked() || response.double_clicked() {
                        let on_chevron = row.parent
                            && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                        let on_check = row.checked.is_some()
                            && response.interact_pointer_pos().is_some_and(|at| at.x < x + metrics::TREE_TEXT);
                        clicked = Some((row.id.clone(), response.double_clicked() || on_check, on_chevron));
                    }
                }
                if rows.is_empty() && !loading {
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(t!("ide-build-empty"))
                                .color(p.text_secondary)
                                .font(theme::regular(12.5)),
                        );
                    });
                }
            },
        );
        self.build_tools.focus_next |= clicked.is_some();
        if focused {
            let typed = speed_search_keys(ui, &mut self.build_tools.search[slot]);
            if typed.changed {
                let query = self.build_tools.search[slot].clone();
                let from = self.build_tools.selected[slot].clone();
                if let Some(found) = tool_menu::search_target(&rows, &query, from.as_deref()) {
                    self.build_tools.selected[slot] = Some(found);
                }
            }
            if let Some(step) = typed.step {
                let at = selected.as_ref().and_then(|id| rows.iter().position(|row| &row.id == id));
                let next = match at {
                    Some(at) => at.saturating_add_signed(step).min(rows.len().saturating_sub(1)),
                    None => 0,
                };
                if let Some(row) = rows.get(next) {
                    self.build_tools.selected[slot] = Some(row.id.clone());
                }
            }
            if typed.enter
                && let Some(id) = selected.clone()
            {
                clicked = Some((id, true, false));
            }
        } else if !self.build_tools.search[slot].is_empty() {
            self.build_tools.search[slot].clear();
        }
        if let Some((command, params)) = picked
            && let Err(error) = self.menu_command(ui.ctx(), registry, session, &command, &params)
        {
            self.post(ui, error);
        }
        if let Some((id, activate, on_chevron)) = clicked {
            self.build_tools.selected[slot] = Some(id.clone());
            let found = rows.iter().find(|row| row.id == id).cloned();
            if let Some(row) = found {
                if on_chevron || (activate && row.parent && row.action.is_none()) {
                    self.toggle_row(tool, &row);
                } else if activate && let Some(action) = &row.action {
                    if let Err(error) = run_action(registry, session, action) {
                        self.post(ui, error);
                    }
                    self.build_tools.stale(tool);
                }
            }
        }
        if run_selected
            && let Some(action) = selected_action
            && let Err(error) = run_action(registry, session, &action)
        {
            self.post(ui, error);
        }
        let acted = !actions.is_empty() || fixed.is_some();
        if let Some(fix) = fixed {
            if fix.command.starts_with("view.") {
                self.run_action(ui, registry, session, &fix.command, fix.params);
            } else {
                self.dispatch(ui, registry, session, &fix.command, fix.params);
            }
            if fix.reload {
                actions.push((tool.reload(), json!({})));
            }
        }
        for (command, params) in actions {
            if command.starts_with("view.") {
                self.run_action(ui, registry, session, command, params);
            } else if let Err(error) = registry.dispatch(session, command, params) {
                self.post(ui, error.text());
            }
        }
        if acted {
            self.build_tools.stale(tool);
        }
        if add_project && let Some((command, params)) = pick_project(tool, &root) {
            if let Err(error) = registry.dispatch(session, command, params) {
                self.post(ui, error.to_string());
            }
            self.build_tools.stale(tool);
        }
        if settings {
            let page = json!({ "page": "gradle" });
            if let Err(error) = self.menu_command(ui.ctx(), registry, session, OPEN_SETTINGS, &page) {
                self.post(ui, error);
            }
        }
        if execute {
            self.open_execute(tool);
        }
        if self.build_tools.execute.as_ref().is_some_and(|open| open.tool == tool) {
            self.execute_popup(ui, registry, session);
        }
        self.assign_goal_shortcut(ui, registry, session);
        if tool == Tool::Maven && self.build_tools.pending_effective.is_some() {
            self.build_tools.stale(tool);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    fn execute_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(mut execute) = self.build_tools.execute.take() else { return };
        if execute.asked.as_deref() != Some(execute.text.as_str()) {
            execute.suggestions = registry
                .dispatch(session, execute.complete_command(), execute.complete_params())
                .ok()
                .and_then(|answer| answer["suggestions"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .map(|suggestion| text(&suggestion["text"]))
                .collect();
            execute.asked = Some(execute.text.clone());
            execute.selected = None;
        }
        let (mut run, mut close) = (false, false);
        let (title, place, hint_text) = match execute.tool {
            Tool::Maven => (
                t!("ide-maven-execute-goal"),
                t!("ide-maven-execute-in", project = execute.project.as_str(), pom = execute.place.as_str()),
                t!("ide-maven-execute-hint"),
            ),
            Tool::Gradle => (
                t!("ide-gradle-execute-task"),
                t!("ide-gradle-execute-in", project = execute.project.as_str()),
                t!("ide-gradle-execute-hint"),
            ),
        };
        let modal = egui::Modal::new(ui.scope_id().with("build-tool-execute")).frame(super::dialog_frame(&p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(title.as_str()).font(theme::semibold(14.0)).color(p.text));
                ui.label(egui::RichText::new(place).font(theme::regular(12.0)).color(p.text_secondary));
                ui.add_space(8.0);
                let (down, up, enter, tab) = ui.input_mut(|input| {
                    (
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                    )
                });
                let field = ui.add(
                    egui::TextEdit::singleline(&mut execute.text)
                        .hint_text(hint_text)
                        .desired_width(f32::INFINITY)
                        .font(theme::regular(13.0)),
                );
                accessibility::labelled(&field, &title);
                if execute.focus {
                    field.request_focus();
                    execute.focus = false;
                }
                let count = execute.suggestions.len();
                if down && count > 0 {
                    execute.selected = Some(execute.selected.map_or(0, |at| (at + 1).min(count - 1)));
                }
                if up {
                    execute.selected = execute.selected.and_then(|at| at.checked_sub(1));
                }
                let take = |execute: &mut ExecuteGoal, at: usize| {
                    if let Some(chosen) = execute.suggestions.get(at).cloned() {
                        execute.text = chosen;
                        execute.selected = None;
                        execute.focus = true;
                    }
                };
                if tab && let Some(at) = execute.selected.or((count > 0).then_some(0)) {
                    take(&mut execute, at);
                }
                if enter {
                    match execute.selected {
                        Some(at) => take(&mut execute, at),
                        None => run = true,
                    }
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    let mut chosen = None;
                    for (index, suggestion) in execute.suggestions.iter().enumerate() {
                        let on = execute.selected == Some(index);
                        if ui.add(egui::Button::new(suggestion.as_str()).selected(on).frame(on)).clicked() {
                            chosen = Some(index);
                        }
                    }
                    if let Some(at) = chosen {
                        take(&mut execute, at);
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("settings-cancel")).clicked() {
                        close = true;
                    }
                    let label = egui::RichText::new(t!("ide-build-menu-run")).color(p.accent_text);
                    if ui
                        .add_enabled(!execute.text.trim().is_empty(), egui::Button::new(label).fill(p.accent))
                        .clicked()
                    {
                        run = true;
                    }
                });
            },
        );
        if modal.should_close() {
            close = true;
        }
        if run && !execute.text.trim().is_empty() {
            let (command, params) = execute.run_command();
            if let Err(error) = registry.dispatch(session, command, params) {
                self.post(ui, error.to_string());
            }
            self.build_tools.stale(execute.tool);
            return;
        }
        if !close {
            self.build_tools.execute = Some(execute);
        }
    }

    fn assign_goal_shortcut(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        use super::keymap_page::{Recorded, shortcut_dialog, take_keystroke};
        let Some((action, title, mut pressed)) = self.build_tools.assigning.take() else { return };
        if take_keystroke(ui, &mut pressed) {
            return;
        }
        let keymap = crate::keymap::current();
        match shortcut_dialog(ui, &self.palette, &title, &action, pressed, &keymap) {
            Recorded::Open(pressed) => self.build_tools.assigning = Some((action, title, pressed)),
            Recorded::Cancelled => {}
            Recorded::Done { key, take_from } => {
                let set = |session: &mut Session, action: &str, keys: Vec<zen_core::keymap::Keystroke>| {
                    let keystrokes: Vec<String> = keys.iter().map(ToString::to_string).collect();
                    registry.dispatch(
                        session,
                        "ui.keymap.setShortcut",
                        json!({ "action": action, "keystrokes": keystrokes }),
                    )
                };
                for other in take_from {
                    let rest = keymap.keystrokes(&other).into_iter().filter(|stroke| *stroke != key).collect();
                    let _ = set(session, &other, rest);
                }
                let mut keys = keymap.keystrokes(&action);
                if !keys.contains(&key) {
                    keys.push(key);
                }
                if let Err(error) = set(session, &action, keys) {
                    self.post(ui, error.to_string());
                }
            }
        }
    }

    fn row_open(&self, tool: Tool, row: &Row) -> bool {
        let flipped = self.build_tools.expanded[BuildToolWindow::slot(tool)].contains(&row.id);
        open_by_default(&row.id) != flipped
    }

    fn toggle_row(&mut self, tool: Tool, row: &Row) {
        let expanded = &mut self.build_tools.expanded[BuildToolWindow::slot(tool)];
        if !expanded.remove(&row.id) {
            expanded.insert(row.id.clone());
        }
    }
}

#[derive(Default)]
struct Typed {
    changed: bool,
    step: Option<isize>,
    enter: bool,
}

fn speed_search_keys(ui: &mut Ui, query: &mut String) -> Typed {
    let mut typed = Typed::default();
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            egui::Event::Text(text) if !text.chars().any(char::is_control) => {
                query.push_str(text);
                typed.changed = true;
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, modifiers, .. }
                if modifiers.is_none() && !query.is_empty() =>
            {
                query.pop();
                typed.changed = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } if !query.is_empty() => {
                query.clear();
                typed.changed = true;
                false
            }
            egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, modifiers, .. } if modifiers.is_none() => {
                typed.step = Some(1);
                false
            }
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, modifiers, .. } if modifiers.is_none() => {
                typed.step = Some(-1);
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none() => {
                typed.enter = true;
                false
            }
            _ => true,
        });
    });
    typed
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_project(tool: Tool, root: &Path) -> Option<(&'static str, Value)> {
    let dialog = rfd::FileDialog::new().set_directory(root);
    match tool {
        Tool::Maven => {
            let pom = dialog.add_filter("pom.xml", &["xml"]).pick_file()?;
            Some(("maven.project.add", json!({ "pom": pom })))
        }
        Tool::Gradle => {
            let script = dialog.add_filter("Gradle", &["gradle", "kts"]).pick_file()?;
            Some(("gradle.project.add", json!({ "path": script })))
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn pick_project(_tool: Tool, _root: &Path) -> Option<(&'static str, Value)> {
    None
}

fn paint_match(ui: &Ui, p: &IdePalette, rect: Rect, x: f32, row: &Row, query: &str) {
    let Some(start) = row.label.to_lowercase().find(&query.to_lowercase()) else { return };
    let end = (start + query.len()).min(row.label.len());
    let (Some(before), Some(found)) = (row.label.get(..start), row.label.get(start..end)) else { return };
    let font = theme::regular(13.0);
    let width = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), p.text).size().x;
    let left = x + metrics::TREE_TEXT + width(before);
    let mark = Rect::from_min_max(pos2(left, rect.top() + 3.0), pos2(left + width(found), rect.bottom() - 3.0));
    ui.painter().rect_filled(mark, 3, p.search_highlight);
}

pub(super) fn open_by_default(id: &str) -> bool {
    let last = id.rsplit('/').next().unwrap_or(id);
    id == "profiles" || id == "@:" || (last.starts_with('@') && !id.starts_with("@:"))
}

pub(super) fn paint_row(ui: &Ui, p: &IdePalette, rect: Rect, x: f32, row: &Row) {
    let icon = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(14.0));
    let painter = ui.painter();
    match (row.icon, row.checked) {
        (_, Some(checked)) => {
            let check = Rect::from_center_size(icon.center(), Vec2::splat(12.0));
            painter.rect_stroke(check, 3, egui::Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            if checked {
                painter.rect_filled(check, 3, p.accent);
                glyph_check(painter, check.shrink(2.0), p.accent_text);
            }
        }
        (RowIcon::Project, _) => glyph_build_project(painter, icon, p.text_secondary),
        (RowIcon::Folder, _) => glyph_folder(painter, icon, p.text_secondary),
        (RowIcon::Run, _) => glyph_run(painter, icon.shrink(2.0), p.run),
        (RowIcon::Plugin, _) => glyph_settings(painter, icon, p.text_secondary),
        (RowIcon::Dependency, _) => glyph_database(painter, icon, p.text_secondary),
        (RowIcon::Profile, _) => glyph_label(painter, icon, p.text_secondary, p.frame),
        (RowIcon::Hint, _) => {}
    }
    let painter = painter.with_clip_rect(rect);
    let color = if row.icon == RowIcon::Hint || row.muted { p.text_secondary } else { p.text };
    let start = if row.icon == RowIcon::Hint { x + metrics::TREE_ARROW } else { x + metrics::TREE_TEXT };
    let name = painter.text(pos2(start, rect.center().y), Align2::LEFT_CENTER, &row.label, theme::regular(13.0), color);
    if !row.detail.is_empty() {
        painter.text(
            pos2(name.right() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            &row.detail,
            theme::regular(12.5),
            p.text_secondary,
        );
    }
}

#[cfg(test)]
mod tests;
