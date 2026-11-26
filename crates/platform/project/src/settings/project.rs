use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CodeStyle, CodeStyleOptions, FileCodeStyle, SchemeFile};

/// The settings that a project's own files give, as the IDE's settings providers read them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSettings {
    /// The code style the project sets, by language id.
    pub code_style: BTreeMap<String, CodeStyleOverride>,
    /// The options of the project's code style that belong to no language.
    pub code_style_options: CodeStyleOptions,
    /// The encodings of the project's files.
    pub encodings: Encodings,
    /// The folders excluded from the project, as absolute paths.
    pub excluded: Vec<PathBuf>,
    /// The name patterns of files and folders excluded everywhere in the project, e.g. `*.log`.
    pub exclude_patterns: Vec<String>,
    /// The content roots of the project's modules, as absolute paths.
    pub content_roots: Vec<PathBuf>,
    /// The folders marked as roots of a kind (resource roots, source roots), as absolute paths.
    pub source_folders: Vec<SourceFolder>,
    /// The inspection profiles the project keeps.
    pub inspection_profiles: Vec<InspectionProfile>,
    /// The named scopes the project keeps.
    pub scopes: Vec<Scope>,
    /// The run configurations another IDE keeps in the project's files.
    pub run_configurations: Vec<ForeignRunConfiguration>,
    /// What could not be read.
    pub problems: Vec<SettingsProblem>,
    /// Every file the settings were read from, as absolute paths.
    pub sources: Vec<PathBuf>,
}

impl ProjectSettings {
    pub fn resource_roots(&self) -> impl Iterator<Item = &Path> {
        self.source_folders.iter().filter(|folder| folder.kind.is_resource()).map(|folder| folder.path.as_path())
    }

    pub fn merge(&mut self, other: ProjectSettings) {
        for (language, style) in other.code_style {
            self.code_style.entry(language).or_insert(style);
        }
        self.code_style_options = self.code_style_options.clone().or(other.code_style_options);
        self.encodings.default = self.encodings.default.take().or(other.encodings.default);
        self.encodings.bom_for_new_files = self.encodings.bom_for_new_files.or(other.encodings.bom_for_new_files);
        self.encodings.files.extend(other.encodings.files);
        self.excluded.extend(other.excluded);
        self.exclude_patterns.extend(other.exclude_patterns);
        self.content_roots.extend(other.content_roots);
        self.source_folders.extend(other.source_folders);
        self.inspection_profiles.extend(other.inspection_profiles);
        self.scopes.extend(other.scopes);
        self.run_configurations.extend(other.run_configurations);
        self.problems.extend(other.problems);
        self.sources.extend(other.sources);
    }
}

/// A folder marked as a root of a kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceFolder {
    /// The folder, as an absolute path.
    pub path: PathBuf,
    /// What it is marked as.
    pub kind: SourceFolderKind,
}

/// What a folder is marked as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SourceFolderKind {
    /// Sources Root.
    #[default]
    Sources,
    /// Test Sources Root.
    TestSources,
    /// Resource Root.
    Resources,
    /// Test Resources Root.
    TestResources,
}

impl SourceFolderKind {
    pub fn is_resource(self) -> bool {
        matches!(self, Self::Resources | Self::TestResources)
    }
}

/// The parts of a language's code style that a project sets; the others come from the IDE.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeStyleOverride {
    /// Whether indentation is written with tab characters.
    pub use_tabs: Option<bool>,
    /// How many columns a tab character takes.
    pub tab_size: Option<u8>,
    /// How many columns one level of indentation takes.
    pub indent: Option<u8>,
    /// How many columns a continued line is indented by.
    pub continuation_indent: Option<u8>,
    /// The column of the right margin.
    pub right_margin: Option<u16>,
    /// The columns of the visual guides drawn besides the right margin.
    pub visual_guides: Option<Vec<u16>>,
}

impl CodeStyleOverride {
    pub fn apply_to(&self, base: CodeStyle) -> CodeStyle {
        self.apply_to_file(FileCodeStyle::from(base)).code_style()
    }

    pub fn apply_to_file(&self, base: FileCodeStyle) -> FileCodeStyle {
        FileCodeStyle {
            use_tabs: self.use_tabs.unwrap_or(base.use_tabs),
            tab_size: self.tab_size.filter(|size| *size > 0).unwrap_or(base.tab_size),
            indent: self.indent.filter(|size| *size > 0).unwrap_or(base.indent),
            continuation_indent: self.continuation_indent.or(base.continuation_indent),
            right_margin: self.right_margin.filter(|column| *column > 0).or(base.right_margin),
            visual_guides: base.visual_guides,
        }
    }

    pub fn or(self, fallback: Self) -> Self {
        Self {
            use_tabs: self.use_tabs.or(fallback.use_tabs),
            tab_size: self.tab_size.or(fallback.tab_size),
            indent: self.indent.or(fallback.indent),
            continuation_indent: self.continuation_indent.or(fallback.continuation_indent),
            right_margin: self.right_margin.or(fallback.right_margin),
            visual_guides: self.visual_guides.or(fallback.visual_guides),
        }
    }
}

/// The encodings a project sets for its files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Encodings {
    /// The encoding of files without an entry of their own, e.g. `UTF-8`.
    pub default: Option<String>,
    /// The files and folders with an encoding of their own.
    pub files: Vec<FileEncoding>,
    /// Whether new UTF-8 files start with a byte order mark.
    pub bom_for_new_files: Option<NewFileBom>,
}

/// Settings → Editor → File Encodings → Create UTF-8 files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NewFileBom {
    /// With a byte order mark.
    With,
    /// Without one.
    #[default]
    Without,
    /// With one under Windows, without one elsewhere.
    OnWindows,
}

impl NewFileBom {
    pub fn adds(self) -> bool {
        match self {
            Self::With => true,
            Self::Without => false,
            Self::OnWindows => cfg!(windows),
        }
    }
}

impl Encodings {
    pub fn charset_for(&self, path: &Path) -> Option<&str> {
        self.files
            .iter()
            .filter(|file| path.starts_with(&file.path))
            .max_by_key(|file| file.path.components().count())
            .map(|file| file.charset.as_str())
            .or(self.default.as_deref())
    }

    pub fn with_file(mut self, path: &Path, charset: Option<&str>) -> Self {
        self.files.retain(|file| file.path != path);
        if let Some(charset) = charset {
            self.files.push(FileEncoding { path: path.to_owned(), charset: charset.to_owned() });
            self.files.sort_by(|left, right| left.path.cmp(&right.path));
        }
        self
    }
}

pub fn is_utf8_compatible(charset: &str) -> bool {
    let name = charset.trim().to_ascii_lowercase().replace('_', "-");
    matches!(name.as_str(), "utf-8" | "utf8" | "us-ascii" | "ascii")
}

/// The encoding of one file or folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileEncoding {
    /// The file or folder, as an absolute path.
    pub path: PathBuf,
    /// The encoding's name, e.g. `windows-1251`.
    pub charset: String,
}

/// An inspection profile the project keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectionProfile {
    /// The profile's name.
    pub name: String,
    /// The file it is kept in.
    pub file: PathBuf,
    /// Whether the project uses it.
    pub is_project_profile: bool,
    /// The inspections it configures.
    pub tools: Vec<InspectionTool>,
}

/// One inspection as a profile configures it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectionTool {
    /// The inspection's id as the profile names it.
    pub class: String,
    /// Whether it runs.
    pub enabled: bool,
    /// How serious what it finds is, e.g. `WARNING`.
    pub level: String,
}

/// A named set of files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Scope {
    /// The scope's name.
    pub name: String,
    /// Which files it holds, in the pattern language of the IDE that wrote it.
    pub pattern: String,
}

/// A run configuration another IDE keeps in the project's files, as written there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ForeignRunConfiguration {
    /// The file it is kept in.
    pub source_file: PathBuf,
    /// Its name.
    pub name: String,
    /// The id of its type, e.g. `GoApplicationRunConfiguration`.
    pub type_id: String,
    /// The factory of its type, when the type has several.
    pub factory: Option<String>,
    /// Its options by name, as written.
    pub options: BTreeMap<String, String>,
    /// The environment variables it sets, in order.
    pub env: Vec<(String, String)>,
    /// Whether it is shared through the project's files rather than kept for one user.
    pub shared: bool,
    /// Its before-launch tasks, in order, as written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before_launch: Vec<ForeignTask>,
}

/// A before-launch task of an adopted run configuration file, as written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ForeignTask {
    /// The id of its type as the file names it, e.g. `RunConfigurationTask`.
    pub id: String,
    pub enabled: bool,
    /// Its attributes by name, macros expanded.
    pub options: BTreeMap<String, String>,
}

/// A problem found while reading a project's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsProblem {
    /// The file it is in.
    pub file: PathBuf,
    /// What is wrong.
    pub message: String,
}

pub trait ReadFiles {
    fn read(&self, relative: &Path) -> Option<String>;

    fn list(&self, relative_dir: &Path) -> Vec<PathBuf>;
}

/// What marking a folder or changing the excluded name patterns did to a project's files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExclusionChange {
    /// Whether a file was written.
    pub changed: bool,
    /// The files written or created, as absolute paths.
    pub files: Vec<PathBuf>,
}

pub trait SettingsProvider: Send + Sync {
    fn id(&self) -> &str;

    fn watches(&self, relative: &Path) -> bool;

    fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings>;

    fn set_excluded(&self, _root: &Path, _folder: &Path, _excluded: bool) -> Option<Result<ExclusionChange, String>> {
        None
    }

    fn write_code_style(&self, _root: &Path, _scheme: &SchemeFile) -> Option<Result<Vec<PathBuf>, String>> {
        None
    }

    fn code_style_text(&self, _scheme: &SchemeFile) -> Option<String> {
        None
    }

    fn parse_code_style(&self, _text: &str) -> Option<Result<SchemeFile, String>> {
        None
    }

    fn set_resource_root(
        &self,
        _root: &Path,
        _folder: &Path,
        _marked: bool,
    ) -> Option<Result<ExclusionChange, String>> {
        None
    }

    fn set_exclude_patterns(&self, _root: &Path, _patterns: &[String]) -> Option<Result<ExclusionChange, String>> {
        None
    }

    fn set_encodings(&self, _root: &Path, _encodings: &Encodings) -> Option<Result<(), String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProjectSettings {
        let mut code_style = BTreeMap::new();
        code_style.insert(
            "go".to_owned(),
            CodeStyleOverride {
                use_tabs: Some(true),
                tab_size: Some(8),
                right_margin: Some(100),
                ..Default::default()
            },
        );
        ProjectSettings {
            code_style,
            code_style_options: CodeStyleOptions::default(),
            encodings: Encodings {
                default: Some("UTF-8".to_owned()),
                files: vec![FileEncoding { path: PathBuf::from("/p/legacy"), charset: "windows-1251".to_owned() }],
                bom_for_new_files: Some(NewFileBom::Without),
            },
            excluded: vec![PathBuf::from("/p/build")],
            exclude_patterns: vec!["*.log".to_owned()],
            content_roots: vec![PathBuf::from("/p")],
            source_folders: vec![SourceFolder { path: PathBuf::from("/p/static"), kind: SourceFolderKind::Resources }],
            inspection_profiles: vec![InspectionProfile {
                name: "Project Default".to_owned(),
                file: PathBuf::from("/p/.idea/inspectionProfiles/Project_Default.xml"),
                is_project_profile: true,
                tools: vec![InspectionTool { class: "Unused".to_owned(), enabled: false, level: "WARNING".to_owned() }],
            }],
            scopes: vec![Scope { name: "Sources".to_owned(), pattern: "file:src//*".to_owned() }],
            run_configurations: vec![ForeignRunConfiguration {
                source_file: PathBuf::from("/p/.run/serve.run.xml"),
                name: "serve".to_owned(),
                type_id: "GoApplicationRunConfiguration".to_owned(),
                factory: Some("Go Application".to_owned()),
                options: BTreeMap::from([("kind".to_owned(), "PACKAGE".to_owned())]),
                env: vec![("PORT".to_owned(), "8080".to_owned())],
                shared: true,
                before_launch: vec![ForeignTask {
                    id: "RunConfigurationTask".to_owned(),
                    enabled: true,
                    options: BTreeMap::from([("run_configuration_name".to_owned(), "build".to_owned())]),
                }],
            }],
            problems: vec![SettingsProblem { file: PathBuf::from("/p/.idea/misc.xml"), message: "bad".to_owned() }],
            sources: vec![PathBuf::from("/p/.idea/misc.xml")],
        }
    }

    #[test]
    fn the_settings_survive_json_in_camel_case() {
        let settings = sample();
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["codeStyle"]["go"]["useTabs"], true);
        assert_eq!(json["codeStyle"]["go"]["rightMargin"], 100);
        assert_eq!(json["codeStyle"]["go"]["continuationIndent"], serde_json::Value::Null);
        assert_eq!(json["encodings"]["bomForNewFiles"], "without");
        assert_eq!(json["excludePatterns"][0], "*.log");
        assert_eq!(json["sourceFolders"][0]["kind"], "resources");
        assert_eq!(json["contentRoots"][0], "/p");
        assert_eq!(json["inspectionProfiles"][0]["isProjectProfile"], true);
        assert_eq!(json["runConfigurations"][0]["typeId"], "GoApplicationRunConfiguration");
        assert_eq!(json["runConfigurations"][0]["sourceFile"], "/p/.run/serve.run.xml");
        assert_eq!(json["runConfigurations"][0]["env"], serde_json::json!([["PORT", "8080"]]));
        assert_eq!(serde_json::from_value::<ProjectSettings>(json).unwrap(), settings);
    }

    #[test]
    fn missing_fields_take_their_defaults_and_there_is_a_schema() {
        assert_eq!(serde_json::from_str::<ProjectSettings>("{}").unwrap(), ProjectSettings::default());
        let partial: ProjectSettings = serde_json::from_str(r#"{ "scopes": [{ "name": "s" }] }"#).unwrap();
        assert_eq!(partial.scopes, vec![Scope { name: "s".to_owned(), pattern: String::new() }]);
        let schema = serde_json::to_value(schemars::schema_for!(ProjectSettings)).unwrap().to_string();
        assert!(schema.contains("runConfigurations") && schema.contains("bomForNewFiles"));
    }

    #[test]
    fn an_override_replaces_only_what_it_sets() {
        let go = CodeStyle::default_for("go");
        let style = CodeStyleOverride { tab_size: Some(2), ..Default::default() }.apply_to(go);
        assert_eq!(style, CodeStyle { tab_size: 2, ..go });
        assert_eq!(CodeStyleOverride::default().apply_to(go), go);
    }

    #[test]
    fn the_closest_listed_folder_or_the_default_gives_the_charset() {
        let encodings = Encodings {
            default: Some("UTF-8".to_owned()),
            files: vec![
                FileEncoding { path: PathBuf::from("/p/legacy/notes.txt"), charset: "KOI8-R".to_owned() },
                FileEncoding { path: PathBuf::from("/p/legacy"), charset: "windows-1251".to_owned() },
            ],
            bom_for_new_files: None,
        };
        assert_eq!(encodings.charset_for(Path::new("/p/legacy/notes.txt")), Some("KOI8-R"));
        assert_eq!(encodings.charset_for(Path::new("/p/legacy/sub/a.go")), Some("windows-1251"));
        assert_eq!(encodings.charset_for(Path::new("/p/legacy")), Some("windows-1251"));
        assert_eq!(encodings.charset_for(Path::new("/p/legacy-2/a.go")), Some("UTF-8"));
        assert_eq!(Encodings::default().charset_for(Path::new("/p/a.go")), None);
        for charset in ["UTF-8", "utf8", "US-ASCII", " utf_8 "] {
            assert!(is_utf8_compatible(charset), "{charset}");
        }
        for charset in ["windows-1251", "UTF-16", "ISO-8859-1", ""] {
            assert!(!is_utf8_compatible(charset), "{charset}");
        }
    }

    #[test]
    fn a_new_entry_replaces_the_files_old_one_and_none_removes_it() {
        let encodings = Encodings::default()
            .with_file(Path::new("/p/b"), Some("KOI8-R"))
            .with_file(Path::new("/p/a"), Some("UTF-8"))
            .with_file(Path::new("/p/b"), Some("windows-1251"));
        let listed: Vec<_> = encodings.files.iter().map(|file| file.charset.as_str()).collect();
        assert_eq!(listed, ["UTF-8", "windows-1251"]);
        assert_eq!(encodings.with_file(Path::new("/p/a"), None).files.len(), 1);
    }

    #[test]
    fn merging_keeps_what_was_read_first() {
        let mut first = sample();
        let mut second = sample();
        second.code_style.insert("go".to_owned(), CodeStyleOverride::default());
        second.code_style.insert("json".to_owned(), CodeStyleOverride { indent: Some(2), ..Default::default() });
        second.encodings.default = Some("ISO-8859-1".to_owned());
        second.sources = vec![PathBuf::from("/p/other.iml")];
        first.merge(second);
        assert_eq!(first.code_style["go"].tab_size, Some(8));
        assert_eq!(first.code_style["json"].indent, Some(2));
        assert_eq!(first.encodings.default.as_deref(), Some("UTF-8"));
        assert_eq!(first.encodings.files.len(), 2);
        assert_eq!(first.sources, vec![PathBuf::from("/p/.idea/misc.xml"), PathBuf::from("/p/other.iml")]);
    }
}
