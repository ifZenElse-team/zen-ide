use std::fmt;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const FILE: &str = ".zen/runConfigurations.json";

pub const VERSION: u32 = 1;

pub const TEMPORARY_LIMIT: usize = 5;

/// A named way to run or test something, of a kind a run provider knows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Configuration {
    /// The stable id, unique in the project.
    pub id: String,
    /// The name shown in the run widget and the Run tool window.
    pub name: String,
    /// The kind, as a provider declares it, e.g. `go.build` or `go.test`.
    pub kind: String,
    /// Whether it was made by running a context and is not saved by the user yet.
    pub temporary: bool,
    /// The settings every kind has.
    pub common: Common,
    /// The settings of the kind, in the provider's own shape: the keys of the kind's fields, as
    /// `run.configuration.list` gives them in `kinds` (a key the kind does not have is refused).
    pub options: serde_json::Value,
    /// The file of another IDE it is adopted from (`.idea/runConfigurations/*.xml`, `.run/*.run.xml`); such a configuration is read-only and never written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<PathBuf>,
    /// Its folder in the dialog's tree.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

impl Configuration {
    pub fn is_adopted(&self) -> bool {
        self.adopted_from.is_some()
    }
}

/// The settings every configuration has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Common {
    /// The folder the program runs in, relative to the project; the project's root when absent.
    /// Macros such as `$ProjectFileDir$` expand at launch.
    pub working_directory: Option<PathBuf>,
    /// Environment variables added for the program, in order, on top of the `envFiles`; `$NAME$`
    /// in a value is the IDE's own variable `NAME` (case-sensitive).
    pub env: Vec<(String, String)>,
    /// The program's arguments, as typed, split like a shell does; macros such as `$FilePath$`
    /// expand at launch, each value staying one argument.
    pub program_arguments: String,
    /// The tool's own arguments, e.g. `go build` flags, as typed; macros expand as in
    /// `programArguments`.
    pub tool_arguments: String,
    /// The tasks run in order before the program starts, each `{ type, ...its options }` with a type
    /// `run.beforeLaunch.types` lists; a failing one cancels the start.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before_launch: Vec<BeforeLaunchTask>,
    /// Whether the Run/Debug Configurations dialog opens on the configuration before each launch
    /// from the IDE's window (Show this page).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub show_page: bool,
    /// `.env` files read at launch, in order, relative to the project or absolute; a missing one
    /// fails the start.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_files: Vec<PathBuf>,
    /// Whether starting it while it runs starts another instance in a tab of its own instead of
    /// stopping the running one and starting it again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub allow_multiple_instances: bool,
    /// Whether the Run tool window opens when it starts.
    #[serde(skip_serializing_if = "is_true")]
    pub activate_tool_window: bool,
    /// Whether the Run tool window takes the keyboard when it starts.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub focus_tool_window: bool,
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Default for Common {
    fn default() -> Self {
        Self {
            working_directory: None,
            env: Vec::new(),
            program_arguments: String::new(),
            tool_arguments: String::new(),
            env_files: Vec::new(),
            allow_multiple_instances: false,
            activate_tool_window: true,
            focus_tool_window: false,
            before_launch: Vec::new(),
            show_page: false,
        }
    }
}

/// A task run before a configuration starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BeforeLaunchTask {
    /// The task's type, e.g. `go.command`, `runConfiguration` or `toolWindow`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The type's options, by the keys of its fields.
    #[serde(flatten)]
    pub options: serde_json::Map<String, serde_json::Value>,
}

impl BeforeLaunchTask {
    pub fn new(kind: &str, options: serde_json::Value) -> Self {
        let options = match options {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        Self { kind: kind.to_owned(), options }
    }

    pub fn text(&self, key: &str) -> &str {
        self.options.get(key).and_then(serde_json::Value::as_str).unwrap_or_default()
    }
}

impl Common {
    pub fn working_directory_in(&self, root: &Path) -> PathBuf {
        match &self.working_directory {
            Some(folder) if !folder.as_os_str().is_empty() => root.join(folder),
            _ => root.to_owned(),
        }
    }
}

/// The settings a new configuration of a kind starts with (Edit Configuration Templates).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Template {
    /// The kind it is the template of, e.g. `go.test`.
    pub kind: String,
    /// The settings every kind has.
    pub common: Common,
    /// The kind's own settings, by the keys of its fields.
    pub options: serde_json::Value,
}

impl Template {
    pub fn apply(&self, configuration: Configuration) -> Configuration {
        let defaults = Common::default();
        let given = configuration.common;
        let template = &self.common;
        let common = Common {
            working_directory: given.working_directory.or_else(|| template.working_directory.clone()),
            env: if given.env.is_empty() { template.env.clone() } else { given.env },
            program_arguments: if given.program_arguments.is_empty() {
                template.program_arguments.clone()
            } else {
                given.program_arguments
            },
            tool_arguments: if given.tool_arguments.is_empty() {
                template.tool_arguments.clone()
            } else {
                given.tool_arguments
            },
            before_launch: if given.before_launch.is_empty() {
                template.before_launch.clone()
            } else {
                given.before_launch
            },
            show_page: given.show_page || template.show_page,
            env_files: if given.env_files.is_empty() { template.env_files.clone() } else { given.env_files },
            allow_multiple_instances: given.allow_multiple_instances || template.allow_multiple_instances,
            activate_tool_window: if given.activate_tool_window == defaults.activate_tool_window {
                template.activate_tool_window
            } else {
                given.activate_tool_window
            },
            focus_tool_window: given.focus_tool_window || template.focus_tool_window,
        };
        let mut options = match &self.options {
            serde_json::Value::Object(map) => map.clone(),
            _ => serde_json::Map::new(),
        };
        if let serde_json::Value::Object(own) = configuration.options {
            options.extend(own);
        }
        Configuration { common, options: serde_json::Value::Object(options), ..configuration }
    }
}

/// The run configurations of a project, as kept in [`FILE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunConfigurations {
    /// The file format's version, [`VERSION`].
    pub version: u32,
    /// The id of the configuration the run widget shows; read from older files, never written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The configurations, in the order the user arranged them.
    pub configurations: Vec<Configuration>,
    /// The templates new configurations start from, one per kind at most.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
}

impl Default for RunConfigurations {
    fn default() -> Self {
        Self { version: VERSION, selected: None, configurations: Vec::new(), templates: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for FileError {}

pub fn file_in(root: &Path) -> PathBuf {
    root.join(FILE)
}

impl RunConfigurations {
    pub fn read(root: &Path) -> Result<Self, FileError> {
        let path = file_in(root);
        let error = |message: String| FileError { path: path.clone(), message };
        let text = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(other) => return Err(error(other.to_string())),
        };
        let mut file: Self = serde_json::from_slice(&text).map_err(|broken| error(broken.to_string()))?;
        if file.version != VERSION {
            return Err(error(format!("version {} is not supported", file.version)));
        }
        for configuration in &mut file.configurations {
            configuration.adopted_from = None;
        }
        Ok(file)
    }

    pub fn permanent(&self) -> Self {
        Self {
            version: VERSION,
            selected: None,
            configurations: self
                .configurations
                .iter()
                .filter(|kept| !kept.temporary && !kept.is_adopted())
                .cloned()
                .collect(),
            templates: self.templates.clone(),
        }
    }

    pub fn template(&self, kind: &str) -> Option<&Template> {
        self.templates.iter().find(|template| template.kind == kind)
    }

    pub fn set_template(&mut self, template: Template) {
        match self.templates.iter_mut().find(|kept| kept.kind == template.kind) {
            Some(kept) => *kept = template,
            None => self.templates.push(template),
        }
    }

    pub fn from_template(&self, configuration: Configuration) -> Configuration {
        match self.template(&configuration.kind) {
            Some(template) => template.apply(configuration),
            None => configuration,
        }
    }

    pub fn write(&self, root: &Path) -> Result<(), FileError> {
        let path = file_in(root);
        let error = |message: String| FileError { path: path.clone(), message };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|failed| error(failed.to_string()))?;
        }
        let mut file = self.permanent();
        for configuration in &mut file.configurations {
            if let Some(folder) = &configuration.common.working_directory
                && let Ok(relative) = folder.strip_prefix(root)
            {
                configuration.common.working_directory = Some(relative.to_owned());
            }
        }
        let mut bytes = serde_json::to_vec_pretty(&file).map_err(|failed| error(failed.to_string()))?;
        bytes.push(b'\n');
        zen_vfs::write_atomic(&path, &bytes).map_err(|failed| error(failed.to_string()))
    }

    pub fn get(&self, id: &str) -> Option<&Configuration> {
        self.configurations.iter().find(|configuration| configuration.id == id)
    }

    pub fn selected_configuration(&self) -> Option<&Configuration> {
        self.selected.as_deref().and_then(|id| self.get(id)).or_else(|| self.configurations.first())
    }

    pub fn new_id(&self, kind: &str) -> String {
        let base = if kind.is_empty() { "configuration" } else { kind };
        (1..).map(|number| format!("{base}-{number}")).find(|id| self.get(id).is_none()).unwrap_or_default()
    }

    pub fn add(&mut self, configuration: Configuration) -> Configuration {
        self.add_keeping(configuration, TEMPORARY_LIMIT)
    }

    pub fn add_keeping(&mut self, mut configuration: Configuration, temporary_limit: usize) -> Configuration {
        if configuration.id.is_empty() || self.get(&configuration.id).is_some() {
            configuration.id = self.new_id(&configuration.kind);
        }
        self.configurations.push(configuration.clone());
        let limit = temporary_limit.max(1);
        let temporary: Vec<String> =
            self.configurations.iter().filter(|kept| kept.temporary).map(|kept| kept.id.clone()).collect();
        if temporary.len() > limit {
            let dropped = &temporary[..temporary.len() - limit];
            self.configurations.retain(|kept| !dropped.contains(&kept.id));
            if self.selected.as_ref().is_some_and(|id| dropped.contains(id)) {
                self.selected = None;
            }
        }
        configuration
    }

    pub fn update(&mut self, configuration: Configuration) -> bool {
        match self.configurations.iter_mut().find(|kept| kept.id == configuration.id) {
            Some(kept) => {
                *kept = configuration;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.configurations.len();
        self.configurations.retain(|kept| kept.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = self.configurations.first().map(|first| first.id.clone());
        }
        self.configurations.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunConfigurations {
        RunConfigurations {
            version: VERSION,
            selected: Some("go.test-1".to_owned()),
            templates: Vec::new(),
            configurations: vec![
                Configuration {
                    id: "go.build-1".to_owned(),
                    name: "go build app".to_owned(),
                    kind: "go.build".to_owned(),
                    temporary: false,
                    common: Common {
                        working_directory: Some(PathBuf::from("cmd/app")),
                        env: vec![("GOFLAGS".to_owned(), "-mod=mod".to_owned())],
                        program_arguments: "--port 8080".to_owned(),
                        tool_arguments: "-race".to_owned(),
                        ..Common::default()
                    },
                    options: serde_json::json!({ "package": "./cmd/app" }),
                    ..Configuration::default()
                },
                Configuration {
                    id: "go.test-1".to_owned(),
                    name: "TestParse".to_owned(),
                    kind: "go.test".to_owned(),
                    temporary: true,
                    common: Common::default(),
                    options: serde_json::json!({ "pattern": "^TestParse$" }),
                    ..Configuration::default()
                },
            ],
        }
    }

    #[test]
    fn the_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let written = sample().permanent();
        written.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(".zen/runConfigurations.json")).unwrap();
        assert!(text.contains("\"workingDirectory\": \"cmd/app\""), "{text}");
        assert!(text.contains("\"programArguments\""), "{text}");
        assert_eq!(RunConfigurations::read(dir.path()).unwrap(), written);
    }

    #[test]
    fn adopted_configurations_are_never_written_nor_read_back_as_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = sample();
        file.configurations[0].temporary = false;
        file.configurations.push(Configuration {
            id: "jb:.run/Serve.run.xml#Serve".to_owned(),
            name: "Serve".to_owned(),
            kind: "go.build".to_owned(),
            adopted_from: Some(dir.path().join(".run/Serve.run.xml")),
            ..Configuration::default()
        });
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(!text.contains("Serve") && !text.contains("adoptedFrom"), "{text}");
        let handwritten =
            r#"{ "version": 1, "configurations": [{ "id": "a", "kind": "go.build", "adoptedFrom": "/x" }] }"#;
        std::fs::write(file_in(dir.path()), handwritten).unwrap();
        let read = RunConfigurations::read(dir.path()).unwrap();
        assert!(!read.configurations[0].is_adopted(), "only the IDE adopts");
    }

    #[test]
    fn the_selection_and_temporary_configurations_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        sample().write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(!text.contains("selected"), "{text}");
        assert!(!text.contains("go.test-1"), "{text}");
        let read = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(read.selected, None);
        assert_eq!(read.configurations.len(), 1);
        assert_eq!(read, sample().permanent());
    }

    #[test]
    fn older_files_with_a_selection_and_temporary_configurations_are_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(file_in(dir.path()), serde_json::to_vec(&sample()).unwrap()).unwrap();
        assert_eq!(RunConfigurations::read(dir.path()).unwrap(), sample());
    }

    #[test]
    fn before_launch_tasks_keep_their_type_and_options_and_are_left_out_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = sample().permanent();
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(!text.contains("beforeLaunch") && !text.contains("showPage"), "{text}");
        file.configurations[0].common.before_launch = vec![
            BeforeLaunchTask::new("go.command", serde_json::json!({ "command": "generate ./..." })),
            BeforeLaunchTask::new("saveFiles", serde_json::Value::Null),
        ];
        file.configurations[0].common.show_page = true;
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(text.contains("\"type\": \"go.command\""), "{text}");
        assert!(text.contains("\"command\": \"generate ./...\""), "{text}");
        assert!(text.contains("\"showPage\": true"), "{text}");
        let read = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(read, file);
        assert_eq!(read.configurations[0].common.before_launch[0].text("command"), "generate ./...");
    }

    #[test]
    fn a_missing_file_is_no_configurations() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(RunConfigurations::read(dir.path()).unwrap(), RunConfigurations::default());
    }

    #[test]
    fn a_broken_file_is_an_error_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(file_in(dir.path()), "{ \"configurations\": [").unwrap();
        let error = RunConfigurations::read(dir.path()).unwrap_err();
        assert_eq!(error.path, file_in(dir.path()));
        assert!(!error.message.is_empty());
        std::fs::write(file_in(dir.path()), "{ \"version\": 7 }").unwrap();
        assert!(RunConfigurations::read(dir.path()).unwrap_err().message.contains("version 7"));
    }

    #[test]
    fn absolute_folders_inside_the_project_are_written_relative() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = sample();
        file.configurations[0].common.working_directory = Some(dir.path().join("cmd"));
        file.configurations[1].common.working_directory = Some(PathBuf::from("/elsewhere"));
        file.configurations[1].temporary = false;
        file.write(dir.path()).unwrap();
        let read = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(read.configurations[0].common.working_directory, Some(PathBuf::from("cmd")));
        assert_eq!(read.configurations[1].common.working_directory, Some(PathBuf::from("/elsewhere")));
        assert_eq!(read.configurations[0].common.working_directory_in(dir.path()), dir.path().join("cmd"));
        assert_eq!(Common::default().working_directory_in(dir.path()), dir.path());
    }

    #[test]
    fn ids_are_unique_and_temporary_ones_are_limited() {
        let mut file = RunConfigurations::default();
        let first = file.add(Configuration { kind: "go.test".to_owned(), ..Configuration::default() });
        let second =
            file.add(Configuration { id: first.id.clone(), kind: "go.test".to_owned(), ..Configuration::default() });
        assert_eq!((first.id.as_str(), second.id.as_str()), ("go.test-1", "go.test-2"));
        for _ in 0..TEMPORARY_LIMIT {
            file.add(Configuration { kind: "go.run".to_owned(), temporary: true, ..Configuration::default() });
        }
        file.selected = Some("go.run-1".to_owned());
        file.add(Configuration { kind: "go.run".to_owned(), temporary: true, ..Configuration::default() });
        assert!(file.get("go.run-1").is_none(), "the oldest temporary configuration goes");
        assert_eq!(file.selected, None);
        assert_eq!(file.configurations.iter().filter(|kept| kept.temporary).count(), TEMPORARY_LIMIT);
        assert!(file.remove("go.test-1"));
        assert!(!file.remove("go.test-1"));
        assert_eq!(file.selected_configuration().map(|kept| kept.id.as_str()), Some("go.test-2"));
    }

    #[test]
    fn the_common_launch_options_are_written_only_when_they_differ_from_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = sample().permanent();
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        for key in ["envFiles", "allowMultipleInstances", "activateToolWindow", "focusToolWindow"] {
            assert!(!text.contains(key), "{key}: {text}");
        }
        let common = &mut file.configurations[0].common;
        common.env_files = vec![PathBuf::from(".env"), PathBuf::from("/etc/app.env")];
        common.allow_multiple_instances = true;
        common.activate_tool_window = false;
        common.focus_tool_window = true;
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(text.contains("\"envFiles\": [") && text.contains("\"allowMultipleInstances\": true"), "{text}");
        assert!(text.contains("\"activateToolWindow\": false") && text.contains("\"focusToolWindow\": true"), "{text}");
        assert_eq!(RunConfigurations::read(dir.path()).unwrap(), file);
        let old: Common = serde_json::from_str(r#"{ "programArguments": "x" }"#).unwrap();
        assert!(old.activate_tool_window && !old.focus_tool_window && !old.allow_multiple_instances);
    }

    #[test]
    fn the_temporary_limit_is_given_by_the_caller() {
        let mut file = RunConfigurations::default();
        let mut last = String::new();
        for _ in 0..4 {
            let temporary = Configuration { kind: "go.run".to_owned(), temporary: true, ..Configuration::default() };
            last = file.add_keeping(temporary, 2).id;
        }
        assert_eq!(file.configurations.len(), 2);
        assert_eq!(file.configurations.last().map(|kept| kept.id.as_str()), Some(last.as_str()));
        file.add_keeping(Configuration { kind: "go.run".to_owned(), temporary: true, ..Configuration::default() }, 0);
        assert_eq!(file.configurations.len(), 1, "at least one is kept");
    }

    #[test]
    fn templates_and_folders_are_kept_in_the_file_and_fill_what_a_new_configuration_leaves_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = sample().permanent();
        file.configurations[0].folder = "Servers".to_owned();
        file.set_template(Template {
            kind: "go.test".to_owned(),
            common: Common { tool_arguments: "-race".to_owned(), activate_tool_window: false, ..Common::default() },
            options: serde_json::json!({ "pattern": "^Test", "bench": "" }),
        });
        file.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(text.contains("\"folder\": \"Servers\"") && text.contains("\"templates\": ["), "{text}");
        let read = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(read, file);
        let fresh = Configuration {
            kind: "go.test".to_owned(),
            common: Common { program_arguments: "-v".to_owned(), ..Common::default() },
            options: serde_json::json!({ "pattern": "^TestA$" }),
            ..Configuration::default()
        };
        let made = read.from_template(fresh);
        assert_eq!(made.common.tool_arguments, "-race");
        assert_eq!(made.common.program_arguments, "-v", "what the configuration sets wins");
        assert!(!made.common.activate_tool_window);
        assert_eq!(made.options, serde_json::json!({ "pattern": "^TestA$", "bench": "" }));
        let other = Configuration { kind: "go.build".to_owned(), ..Configuration::default() };
        assert_eq!(read.from_template(other.clone()), other, "a kind without a template is left as it is");
        sample().permanent().write(dir.path()).unwrap();
        let text = std::fs::read_to_string(file_in(dir.path())).unwrap();
        assert!(!text.contains("templates") && !text.contains("folder"), "{text}");
    }
}
