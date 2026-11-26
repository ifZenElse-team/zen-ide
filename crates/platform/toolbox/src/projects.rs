use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_project::recent::{self, RecentProjects};

use crate::{Tool, Tools, catalog, launch, not_in_demo, project_folder, tools};

/// A project folder that a Zen IDE opened, as the Projects tab lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// The project's root folder, an absolute path.
    pub path: PathBuf,
    /// The folder name, shown as the project name.
    pub name: String,
    /// The id of the IDE that opened the project most recently, e.g. `gogarden`.
    pub tool_id: String,
    /// The name of that IDE, e.g. `GoGarden`.
    pub tool_name: String,
    /// When that IDE last opened the project, in seconds since the Unix epoch.
    pub opened: u64,
    /// Whether the folder exists now; a missing project is still listed.
    pub exists: bool,
    /// The ids of the other IDEs that also list the project, most recent first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_in: Vec<String>,
}

pub fn read_recent(tool: &Tool, data_dir: Option<&Path>) -> RecentProjects {
    match data_dir {
        Some(data_dir) => RecentProjects::load(&recent::file_in(data_dir, &tool.bundle_id)),
        None => recent::read(&tool.bundle_id),
    }
}

pub fn collect(tools: &[Tool], mut recent: impl FnMut(&Tool) -> RecentProjects) -> Vec<Project> {
    let mut entries: Vec<(&Tool, recent::RecentProject)> = Vec::new();
    for tool in tools {
        entries.extend(recent(tool).projects().iter().cloned().map(|project| (tool, project)));
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1.opened));
    let mut projects: Vec<Project> = Vec::new();
    for (tool, entry) in entries {
        if let Some(project) = projects.iter_mut().find(|project| project.path == entry.path) {
            if project.tool_id != tool.id && !project.also_in.contains(&tool.id) {
                project.also_in.push(tool.id.clone());
            }
            continue;
        }
        projects.push(Project {
            name: entry.name(),
            exists: entry.path.is_dir(),
            path: entry.path,
            tool_id: tool.id.clone(),
            tool_name: tool.name.clone(),
            opened: entry.opened,
            also_in: Vec::new(),
        });
    }
    projects
}

pub fn scan(tools: &[Tool], data_dir: Option<&Path>) -> Vec<Project> {
    collect(tools, |tool| read_recent(tool, data_dir))
}

pub fn filter(projects: Vec<Project>, query: &str) -> Vec<Project> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return projects;
    }
    projects
        .into_iter()
        .filter(|project| {
            project.name.to_lowercase().contains(&query)
                || project.path.to_string_lossy().to_lowercase().contains(&query)
        })
        .collect()
}

pub fn demo() -> Vec<Project> {
    type Row = (&'static str, &'static str, &'static str, u64, bool, &'static [&'static str]);
    const PROJECTS: &[Row] = &[
        ("/Users/ada/src/garden", "gogarden", "GoGarden", 1_792_224_000, true, &["rustronin"]),
        ("/Users/ada/src/ronin-cli", "rustronin", "RustRonin", 1_792_180_000, true, &[]),
        ("/Users/ada/src/notes", "gogarden", "GoGarden", 1_792_137_600, true, &[]),
        ("/Volumes/Backup/old-site", "pypetal", "PyPetal", 1_791_900_000, false, &[]),
        ("/Users/ada/src/petal-scripts", "pypetal", "PyPetal", 1_791_500_000, true, &[]),
    ];
    PROJECTS
        .iter()
        .map(|&(path, tool_id, tool_name, opened, exists, also_in)| {
            let path = PathBuf::from(path);
            Project {
                name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                path,
                tool_id: tool_id.to_owned(),
                tool_name: tool_name.to_owned(),
                opened,
                exists,
                also_in: also_in.iter().map(|&id| id.to_owned()).collect(),
            }
        })
        .collect()
}

impl Tools {
    pub fn projects(&self) -> Vec<Project> {
        let (demo, data_dir) = {
            let shared = self.lock();
            (shared.demo, shared.config.data_dir.clone())
        };
        if demo { self::demo() } else { scan(&catalog(), data_dir.as_deref()) }
    }
}

/// Parameters of `toolbox.projects.list`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListProjectsParams {
    /// Only projects whose name or path contains this text, ignoring case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

pub struct ListProjects;

impl Command for ListProjects {
    type Params = ListProjectsParams;
    type Output = Vec<Project>;
    const ID: &'static str = "toolbox.projects.list";
    const TITLE: &'static str = "List Projects";
    const DESCRIPTION: &'static str = "Lists the projects every Zen IDE opened recently, one per folder, newest first, \
         with the IDE that opened each last and whether its folder still exists. `query` keeps the projects whose \
         name or path contains it, ignoring case.";

    fn run(&self, session: &mut Session, params: ListProjectsParams) -> Result<Vec<Project>, CommandError> {
        let projects = tools(session)?.projects();
        Ok(filter(projects, params.query.as_deref().unwrap_or_default()))
    }
}

/// Parameters of `toolbox.project.open`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectParams {
    /// The project folder to open.
    pub path: PathBuf,
    /// The id of the IDE to open it in, e.g. `gogarden`; without it, the IDE that opened it last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
}

pub struct OpenProject;

impl Command for OpenProject {
    type Params = OpenProjectParams;
    type Output = ();
    const ID: &'static str = "toolbox.project.open";
    const TITLE: &'static str = "Open Project";
    const DESCRIPTION: &'static str = "Opens a project folder in an installed Zen IDE: in `toolId` when given, \
         otherwise in the IDE that opened the folder most recently (see `toolbox.projects.list`).";

    fn run(&self, session: &mut Session, params: OpenProjectParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let folder = project_folder(&params.path)?;
        let id = match params.tool_id {
            Some(id) => id,
            None => owner(&tools.projects(), &params.path, &folder).ok_or_else(|| {
                CommandError::new(t!("toolbox-error-unknown-project", path = params.path.display().to_string()))
            })?,
        };
        launch(&tools, &id, Some(&params.path))
    }
}

fn owner(projects: &[Project], path: &Path, folder: &Path) -> Option<String> {
    projects
        .iter()
        .find(|project| project.path == path || std::fs::canonicalize(&project.path).is_ok_and(|known| known == folder))
        .map(|project| project.tool_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::installed;

    fn write_recent(data_dir: &Path, tool: &str, projects: &[(&Path, u64)]) {
        let mut recent = RecentProjects::default();
        for &(path, opened) in projects.iter().rev() {
            recent.opened(path, opened);
        }
        recent.save(&recent::file_in(data_dir, &format!("space.ifzenelse.{tool}"))).unwrap();
    }

    fn folder(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn scanning_merges_every_ide_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let (garden, shared, notes) =
            (folder(dir.path(), "garden"), folder(dir.path(), "Shared"), folder(dir.path(), "notes"));
        let gone = dir.path().join("gone");
        write_recent(&data, "gogarden", &[(&shared, 50), (&garden, 30), (&gone, 10)]);
        write_recent(&data, "rustronin", &[(&notes, 40), (&shared, 20)]);
        write_recent(&data, "pypetal", &[(&shared, 60)]);

        let projects = scan(&catalog(), Some(&data));
        let rows: Vec<(&str, &str, u64, bool)> = projects
            .iter()
            .map(|project| (project.name.as_str(), project.tool_id.as_str(), project.opened, project.exists))
            .collect();
        assert_eq!(
            rows,
            [
                ("Shared", "pypetal", 60, true),
                ("notes", "rustronin", 40, true),
                ("garden", "gogarden", 30, true),
                ("gone", "gogarden", 10, false),
            ]
        );
        assert_eq!(projects[0].also_in, ["gogarden", "rustronin"]);
        assert_eq!(projects[0].tool_name, "PyPetal");
        assert!(projects[1].also_in.is_empty());
        assert!(scan(&catalog(), Some(&dir.path().join("nothing"))).is_empty());
    }

    #[test]
    fn the_query_matches_names_and_paths_ignoring_case() {
        let projects = demo();
        let names = |query: &str| -> Vec<String> {
            filter(projects.clone(), query).into_iter().map(|project| project.name).collect()
        };
        assert_eq!(names("  ").len(), projects.len());
        assert_eq!(names("GARDEN"), ["garden"]);
        assert_eq!(names("volumes/backup"), ["old-site"]);
        assert_eq!(names("petal"), ["petal-scripts"]);
        assert!(names("nothing like it").is_empty());
    }

    #[test]
    fn projects_serialise_in_camel_case() {
        let value = serde_json::to_value(&demo()[0]).unwrap();
        assert_eq!(value["toolId"], "gogarden");
        assert_eq!(value["toolName"], "GoGarden");
        assert_eq!(value["alsoIn"], serde_json::json!(["rustronin"]));
        assert!(serde_json::to_value(&demo()[1]).unwrap().get("alsoIn").is_none());
        let schema = serde_json::to_value(schemars::schema_for!(Project)).unwrap();
        assert!(schema["properties"]["exists"]["description"].as_str().unwrap().contains("exists"));
        let schema = serde_json::to_value(schemars::schema_for!(OpenProjectParams)).unwrap();
        assert_eq!(schema["required"], serde_json::json!(["path"]));
        assert!(schema["properties"].get("toolId").is_some());
    }

    #[test]
    fn listing_projects_through_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, _) = installed(dir.path(), &[]);
        let (garden, notes) = (folder(dir.path(), "garden"), folder(dir.path(), "notes"));
        write_recent(&dir.path().join("data"), "gogarden", &[(&garden, 20), (&notes, 10)]);

        let all = registry.dispatch(&mut session, "toolbox.projects.list", serde_json::Value::Null).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 2);
        assert_eq!(all[0]["name"], "garden");
        assert_eq!(all[0]["exists"], true);
        let some =
            registry.dispatch(&mut session, "toolbox.projects.list", serde_json::json!({ "query": "NOT" })).unwrap();
        assert_eq!(some.as_array().unwrap().len(), 1);
        assert_eq!(some[0]["name"], "notes");

        let mut demo = Session::new();
        demo.set_state(Tools::demo());
        let listed = registry.dispatch(&mut demo, "toolbox.projects.list", serde_json::json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), self::demo().len());
        assert!(listed.as_array().unwrap().iter().any(|project| project["exists"] == false));
    }

    #[test]
    fn opening_a_project_launches_its_most_recent_ide() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, launched) = installed(dir.path(), &["gogarden", "rustronin"]);
        let garden = folder(dir.path(), "garden");
        write_recent(&dir.path().join("data"), "gogarden", &[(&garden, 10)]);
        write_recent(&dir.path().join("data"), "rustronin", &[(&garden, 20)]);

        registry.dispatch(&mut session, "toolbox.project.open", serde_json::json!({ "path": garden })).unwrap();
        registry
            .dispatch(&mut session, "toolbox.project.open", serde_json::json!({ "path": garden, "toolId": "gogarden" }))
            .unwrap();
        let launched = launched.lock().unwrap();
        let canonical = std::fs::canonicalize(&garden).unwrap().into_os_string();
        let programs: Vec<String> = launched
            .iter()
            .map(|line| line.program.file_name().unwrap().to_string_lossy().into_owned())
            .map(|name| name.trim_end_matches(".exe").to_owned())
            .collect();
        assert_eq!(programs, ["rustronin", "gogarden"]);
        assert!(launched.iter().all(|line| line.args == [canonical.clone()]));
    }

    #[test]
    fn opening_a_project_reports_what_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, launched) = installed(dir.path(), &["gogarden"]);
        let (garden, stranger) = (folder(dir.path(), "garden"), folder(dir.path(), "stranger"));
        write_recent(&dir.path().join("data"), "rustronin", &[(&garden, 10)]);
        let mut open = |params: serde_json::Value| {
            registry.dispatch(&mut session, "toolbox.project.open", params).unwrap_err().to_string()
        };

        let error = open(serde_json::json!({ "path": dir.path().join("gone") }));
        assert!(error.contains("There is no folder") && error.contains("gone"), "{error}");
        let error = open(serde_json::json!({ "path": stranger }));
        assert!(error.contains("No Zen IDE has opened") && error.contains("stranger"), "{error}");
        let error = open(serde_json::json!({ "path": garden }));
        assert!(error.contains("RustRonin is not installed"), "{error}");
        let error = open(serde_json::json!({ "path": stranger, "toolId": "pypetal" }));
        assert!(error.contains("PyPetal is not installed"), "{error}");
        let error = open(serde_json::json!({ "path": stranger, "toolId": "nope" }));
        assert!(error.contains("There is no tool"), "{error}");
        assert!(launched.lock().unwrap().is_empty());

        let mut demo = Session::new();
        demo.set_state(Tools::demo());
        let error = registry
            .dispatch(&mut demo, "toolbox.project.open", serde_json::json!({ "path": garden }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("demo data"), "{error}");
    }
}
