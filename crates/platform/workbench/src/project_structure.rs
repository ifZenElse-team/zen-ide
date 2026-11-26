use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::model::{Library, LibraryKind, Module, ProjectModel};

use crate::Workbench;
use crate::jdk::{Jdk, Jdks, ProjectJdk};
use crate::project_model::ProjectModels;

/// Where a value shown in Project Structure comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Read from the build file; change it there.
    Build,
    /// The build system's convention, not read from the build, which may change it.
    Convention,
    /// Run configurations compile into a temporary folder of their own.
    Temporary,
    /// Kept by the IDE for this project; changed here.
    Ide,
    /// Not known.
    #[default]
    Unknown,
}

/// An SDK the project uses or can use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Sdk {
    /// Its name: `JDK`, `Go`, `Rust`.
    pub name: String,
    pub version: String,
    /// Its folder: a JDK's home, else the folder the language module reads.
    pub home: PathBuf,
}

/// The compiled code's folders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompilerOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_path: Option<PathBuf>,
    pub origin: Origin,
}

/// What `project.structure.project` answers: the Project section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSection {
    /// The project's name: its folder's name.
    pub name: String,
    pub root: PathBuf,
    /// Whether it is a Java or Kotlin project, whose SDK and language level are chosen here.
    pub jvm: bool,
    /// The project's JDK and language level (a JVM project), set with `toolchain.jdk.choose` and
    /// `toolchain.jdk.languageLevel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jdk: Option<ProjectJdk>,
    /// The JDKs to choose from (a JVM project).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jdks: Vec<Jdk>,
    /// The SDK the language module found, for a project that is not a JVM project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk: Option<Sdk>,
    /// The language level the build asks for, which the build's compiler uses whatever is chosen here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_language_level: Option<String>,
    /// The build file of the project's root, if the project has a build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_file: Option<PathBuf>,
    pub compiler_output: CompilerOutput,
}

/// What `project.structure.modules` answers: the Modules section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModulesSection {
    /// The modules the build files give: their roots, dependencies, output and language level, read-only
    /// here (change them in `file`). Folder marks kept by the IDE are in `project.structure.get`.
    pub modules: Vec<Module>,
    pub revision: u64,
}

/// A library with the modules that use it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    #[serde(flatten)]
    pub library: Library,
    /// The modules whose build declares it; empty when it comes in through another library.
    pub used_by: Vec<String>,
}

/// What `project.structure.libraries` answers: the Libraries section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibrariesSection {
    pub libraries: Vec<LibraryEntry>,
    pub revision: u64,
}

/// What `project.structure.sdks` answers: the SDKs section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SdksSection {
    /// The JDKs on this computer, as `toolchain.jdk.list` gives them.
    pub jdks: Vec<Jdk>,
    /// The SDKs of other languages the language modules found for the project.
    pub others: Vec<Sdk>,
}

/// The section a problem belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Project,
    Modules,
    Libraries,
    Sdks,
}

/// A problem with how the project is put together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructureProblem {
    pub section: Section,
    pub message: String,
    /// The file to fix it in, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// Whether the project works all the same.
    pub warning: bool,
}

/// What `project.structure.problems` answers: the Problems section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemsSection {
    pub problems: Vec<StructureProblem>,
}

fn require_project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn model(session: &mut Session, root: &Path) -> (ProjectModel, u64) {
    let models = session.state_mut::<ProjectModels>();
    models.ensure(Some(root));
    models.recheck_libraries();
    (models.model().clone(), models.revision())
}

fn root_module<'a>(model: &'a ProjectModel, root: &Path) -> Option<&'a Module> {
    model.modules.iter().find(|module| module.root == root).or_else(|| model.modules.first())
}

fn is_jvm(model: &ProjectModel, root: &Path) -> bool {
    crate::jdk::is_jvm_project(root) || model.modules.iter().any(|module| module.details.get("build").is_some())
}

fn other_sdks(model: &ProjectModel) -> Vec<Sdk> {
    model
        .libraries
        .iter()
        .filter(|library| library.kind == LibraryKind::Sdk && library.name != "JDK")
        .map(|library| Sdk {
            name: library.name.clone(),
            version: library.version.clone().unwrap_or_default(),
            home: library.root.clone(),
        })
        .collect()
}

pub fn project_section(session: &mut Session, root: &Path) -> ProjectSection {
    let (model, _) = model(session, root);
    let jvm = is_jvm(&model, root);
    let (jdk, jdks) = if jvm {
        let jdks = session.state_mut::<Jdks>().clone();
        let mut store = jdks.store();
        (store.project(root), store.jdks())
    } else {
        (None, Vec::new())
    };
    let module = root_module(&model, root);
    let build_file = module.map(|module| module.file.clone()).filter(|file| !file.as_os_str().is_empty());
    let compiler_output = match module {
        Some(module) if module.output.is_some() => CompilerOutput {
            path: module.output.clone(),
            test_path: module.test_output.clone(),
            origin: if module.assumed { Origin::Convention } else { Origin::Build },
        },
        _ if jvm => CompilerOutput { origin: Origin::Temporary, ..CompilerOutput::default() },
        _ => CompilerOutput::default(),
    };
    ProjectSection {
        name: root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        root: root.to_owned(),
        jvm,
        jdk,
        jdks,
        sdk: if jvm { None } else { other_sdks(&model).into_iter().next() },
        build_language_level: jvm.then(|| module.and_then(|module| module.language_version.clone())).flatten(),
        build_file,
        compiler_output,
    }
}

pub fn libraries_section(model: &ProjectModel, revision: u64) -> LibrariesSection {
    let libraries = model
        .libraries
        .iter()
        .filter(|library| library.kind == LibraryKind::Dependency)
        .map(|library| LibraryEntry {
            library: library.clone(),
            used_by: model
                .modules
                .iter()
                .filter(|module| module.dependencies.iter().any(|dependency| dependency.name == library.name))
                .map(|module| module.name.clone())
                .collect(),
        })
        .collect();
    LibrariesSection { libraries, revision }
}

pub fn problems_section(session: &mut Session, root: &Path) -> ProblemsSection {
    let project = project_section(session, root);
    let (model, _) = model(session, root);
    let mut problems = Vec::new();
    let mut add = |section, message: String, file: Option<PathBuf>, warning| {
        problems.push(StructureProblem { section, message, file, warning });
    };
    if project.jvm {
        match &project.jdk {
            None => add(Section::Project, t!("structure-problem-no-jdk"), None, false),
            Some(jdk) if jdk.language_level > jdk.jdk.major => add(
                Section::Project,
                t!("structure-problem-level-above-jdk", level = jdk.language_level, jdk = jdk.jdk.major),
                None,
                false,
            ),
            Some(_) => {}
        }
        if let (Some(jdk), Some(wanted)) = (&project.jdk, &project.build_language_level)
            && let Some(major) = crate::jdk::detect::major_of(wanted)
            && major > jdk.jdk.major
        {
            add(
                Section::Project,
                t!("structure-problem-build-above-jdk", level = wanted.as_str(), jdk = jdk.jdk.major),
                project.build_file.clone(),
                false,
            );
        }
    }
    for module in &model.modules {
        if let Some(error) = module.details.get("error").and_then(serde_json::Value::as_str) {
            add(
                Section::Modules,
                t!("structure-problem-module-unread", module = module.name.as_str(), error = error),
                Some(module.file.clone()),
                false,
            );
        }
    }
    for problem in &model.problems {
        add(Section::Modules, problem.message.clone(), Some(problem.file.clone()), problem.warning);
    }
    for library in model.libraries.iter().filter(|library| is_missing(library)) {
        let (section, message) = match library.kind {
            LibraryKind::Sdk => {
                let name = format!("{} {}", library.name, library.version.as_deref().unwrap_or_default());
                (Section::Sdks, t!("structure-problem-sdk-missing", name = name.trim()))
            }
            LibraryKind::Dependency => {
                let name = library.label.clone().unwrap_or_else(|| library.name.clone());
                (Section::Libraries, t!("structure-problem-library-missing", name = name))
            }
        };
        add(section, message, None, true);
    }
    ProblemsSection { problems }
}

pub fn is_missing(library: &Library) -> bool {
    library.root.as_os_str().is_empty() || (!library.exists && !crate::jvm_libraries::is_unpacking(&library.root))
}

pub struct ProjectSettings;

impl Command for ProjectSettings {
    type Params = NoParams;
    type Output = ProjectSection;
    const ID: &'static str = "project.structure.project";
    const TITLE: &'static str = "Project Structure: Project";
    const DESCRIPTION: &'static str = "The Project section of Project Structure: the project's name, its SDK (the \
         project JDK of a Java or Kotlin project, with the JDKs to choose from; else the SDK the language module \
         found), the language level, the one the build asks for, and the compiler output with where it comes from \
         (build, convention, temporary, unknown). Change the JDK and level with toolchain.jdk.choose and \
         toolchain.jdk.languageLevel; what the build gives, in the build file.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProjectSection, CommandError> {
        let root = require_project(session)?;
        Ok(project_section(session, &root))
    }
}

pub struct ModulesCommand;

impl Command for ModulesCommand {
    type Params = NoParams;
    type Output = ModulesSection;
    const ID: &'static str = "project.structure.modules";
    const TITLE: &'static str = "Project Structure: Modules";
    const DESCRIPTION: &'static str = "The Modules section of Project Structure: every module the build files give \
         (Maven, Gradle, go.mod, Cargo.toml...) with its build file, source, test and resource roots, excluded \
         folders, compiler output (`assumed`: the build system's convention, not read from the build), language \
         level and declared dependencies with their scopes. These come from the build and change in its file \
         (project.structure.openBuildFile); folder marks kept by the IDE are in project.structure.get.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ModulesSection, CommandError> {
        let root = require_project(session)?;
        let (model, revision) = model(session, &root);
        Ok(ModulesSection { modules: model.modules, revision })
    }
}

pub struct LibrariesCommand;

impl Command for LibrariesCommand {
    type Params = NoParams;
    type Output = LibrariesSection;
    const ID: &'static str = "project.structure.libraries";
    const TITLE: &'static str = "Project Structure: Libraries";
    const DESCRIPTION: &'static str = "The Libraries section of Project Structure: the libraries the build resolves, \
         each with its version, its folder, whether it is downloaded, and the modules whose build declares it. They \
         come from the build and change in its file.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LibrariesSection, CommandError> {
        let root = require_project(session)?;
        let (model, revision) = model(session, &root);
        Ok(libraries_section(&model, revision))
    }
}

pub struct SdksCommand;

impl Command for SdksCommand {
    type Params = NoParams;
    type Output = SdksSection;
    const ID: &'static str = "project.structure.sdks";
    const TITLE: &'static str = "Project Structure: SDKs";
    const DESCRIPTION: &'static str = "The SDKs section of Project Structure: the JDKs on this computer (add, \
         download or remove them with toolchain.jdk.*) and the SDKs of other languages the project's language \
         modules found (Go, Rust).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SdksSection, CommandError> {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let others = match &root {
            Some(root) => other_sdks(&model(session, root).0),
            None => Vec::new(),
        };
        let jdks = session.state_mut::<Jdks>().clone().store().jdks();
        Ok(SdksSection { jdks, others })
    }
}

pub struct ProblemsCommand;

impl Command for ProblemsCommand {
    type Params = NoParams;
    type Output = ProblemsSection;
    const ID: &'static str = "project.structure.problems";
    const TITLE: &'static str = "Project Structure: Problems";
    const DESCRIPTION: &'static str = "The Problems section of Project Structure: no JDK for a Java or Kotlin \
         project, a language level above the JDK, build files that do not read, problems of the project model, \
         and libraries or SDKs that are not downloaded; each with its section and the file to fix it in.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProblemsSection, CommandError> {
        let root = require_project(session)?;
        Ok(problems_section(session, &root))
    }
}

/// Which module's build file to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildFileParams {
    /// The module's name from project.structure.modules; the project's root module when absent.
    #[serde(default)]
    pub module: Option<String>,
}

/// What `project.structure.openBuildFile` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildFileAnswer {
    /// The build file opened in the editor.
    pub path: PathBuf,
}

pub struct OpenBuildFile;

impl Command for OpenBuildFile {
    type Params = BuildFileParams;
    type Output = BuildFileAnswer;
    const ID: &'static str = "project.structure.openBuildFile";
    const TITLE: &'static str = "Open Build File";
    const DESCRIPTION: &'static str = "Opens the build file of a module (pom.xml, build.gradle, go.mod, \
         Cargo.toml...), where what Project Structure shows as coming from the build is changed.";

    fn run(&self, session: &mut Session, params: BuildFileParams) -> Result<BuildFileAnswer, CommandError> {
        let root = require_project(session)?;
        let path = build_file(session, &root, params.module.as_deref())
            .ok_or_else(|| CommandError::new(t!("structure-error-no-build-file")))?;
        crate::open_document(session, &path)?;
        Ok(BuildFileAnswer { path })
    }
}

pub fn build_file(session: &mut Session, root: &Path, module: Option<&str>) -> Option<PathBuf> {
    let (model, _) = model(session, root);
    let found = match module {
        Some(name) => model.modules.iter().find(|module| module.name == name),
        None => root_module(&model, root),
    };
    found.map(|module| module.file.clone()).filter(|file| file.is_file())
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ProjectSettings)?;
    registry.register(ModulesCommand)?;
    registry.register(LibrariesCommand)?;
    registry.register(SdksCommand)?;
    registry.register(ProblemsCommand)?;
    registry.register(OpenBuildFile)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use zen_project::model::{FolderKind, ModelEnv};

    use super::*;
    use crate::build_tools::BuildEnv;
    use crate::jdk::{Sources, Store};
    use crate::jvm_libraries::{Declared, JvmLibraries, Places};

    const POM: &str = r#"<project>
  <groupId>demo</groupId><artifactId>shop</artifactId><version>1.0</version>
  <properties><maven.compiler.release>17</maven.compiler.release></properties>
  <dependencies>
    <dependency><groupId>org.demo</groupId><artifactId>lib</artifactId><version>1.0</version></dependency>
    <dependency><groupId>org.junit</groupId><artifactId>junit</artifactId><version>5.0</version><scope>test</scope></dependency>
  </dependencies>
  <build><testOutputDirectory>out/tests</testOutputDirectory></build>
</project>"#;

    fn fake_jdk(home: &Path, version: &str) {
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("bin/java"), "").unwrap();
        std::fs::write(home.join("bin/javac"), "").unwrap();
        std::fs::write(home.join("release"), format!("IMPLEMENTOR=\"Vendor\"\nJAVA_VERSION=\"{version}\"\n")).unwrap();
    }

    struct Fixture {
        registry: Registry,
        session: Session,
        root: PathBuf,
        jvm: PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            zen_i18n::register(crate::LOCALES);
            zen_i18n::set_language(Some("en"));
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap().join("shop");
            for (path, text) in files {
                let file = root.join(path);
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, text).unwrap();
            }
            let jvm = dir.path().join("jvm");
            fake_jdk(&jvm.join("a-17/Contents/Home"), "17.0.12");
            fake_jdk(&jvm.join("b-21/Contents/Home"), "21.0.5");
            let mut store = Store::with_sources(Sources { system_folders: vec![jvm.clone()], ..Sources::default() });
            store.set_data_dir(dir.path().join("data"));
            let home = dir.path().join("home");
            let env = BuildEnv::default();
            env.set_home(Some(home.clone()));
            env.set_jdk(Some(Arc::new(|_: &Path| None)));
            let places = Places {
                maven: home.join(".m2/repository"),
                gradle: home.join(".gradle/files"),
                cache: dir.path().join("cache"),
            };
            let provider = JvmLibraries::with_places(env, places, vec![Box::new(Declared)]);
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Jdks::new(store));
            session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(provider)], ModelEnv::default());
            registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
            Self { registry, session, root, jvm, _dir: dir }
        }

        fn run(&mut self, command: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, command, params).unwrap()
        }
    }

    #[test]
    fn a_maven_project_shows_its_build_model_and_the_project_jdk() {
        let mut fixture = Fixture::new(&[
            ("pom.xml", POM),
            ("src/main/java/demo/App.java", "class App {}"),
            ("src/main/resources/app.properties", ""),
            ("src/test/java/demo/AppTest.java", "class AppTest {}"),
        ]);
        let root = fixture.root.clone();
        let project: ProjectSection =
            serde_json::from_value(fixture.run("project.structure.project", json!({}))).unwrap();
        assert!(project.jvm);
        assert_eq!(project.jdk.as_ref().map(|jdk| jdk.jdk.major), Some(17), "the JDK of the build's level");
        assert_eq!(project.jdks.len(), 2);
        assert_eq!(project.build_language_level.as_deref(), Some("17"));
        assert_eq!(project.build_file, Some(root.join("pom.xml")));
        assert_eq!(
            project.compiler_output,
            CompilerOutput {
                path: Some(root.join("target/classes")),
                test_path: Some(root.join("out/tests")),
                origin: Origin::Build
            }
        );

        let modules: ModulesSection =
            serde_json::from_value(fixture.run("project.structure.modules", json!({}))).unwrap();
        let module = &modules.modules[0];
        assert_eq!((module.name.as_str(), module.assumed), ("shop", false));
        let folders: Vec<(PathBuf, FolderKind)> =
            module.folders.iter().map(|folder| (folder.path.clone(), folder.kind)).collect();
        assert_eq!(
            folders,
            [
                (root.join("src/main/java"), FolderKind::Sources),
                (root.join("src/test/java"), FolderKind::TestSources),
                (root.join("src/main/resources"), FolderKind::Resources),
                (root.join("target"), FolderKind::Excluded),
            ]
        );
        let scopes: Vec<(&str, Option<&str>)> = module
            .dependencies
            .iter()
            .map(|dependency| (dependency.name.as_str(), dependency.scope.as_deref()))
            .collect();
        assert_eq!(scopes, [("org.demo:lib", Some("compile")), ("org.junit:junit", Some("test"))]);

        let libraries: LibrariesSection =
            serde_json::from_value(fixture.run("project.structure.libraries", json!({}))).unwrap();
        let used: Vec<(&str, &[String])> =
            libraries.libraries.iter().map(|entry| (entry.library.name.as_str(), entry.used_by.as_slice())).collect();
        assert_eq!(used, [("org.demo:lib", &["shop".to_owned()][..]), ("org.junit:junit", &["shop".to_owned()][..])]);

        let sdks: SdksSection = serde_json::from_value(fixture.run("project.structure.sdks", json!({}))).unwrap();
        assert_eq!(sdks.jdks.iter().map(|jdk| jdk.major).collect::<Vec<_>>(), [21, 17]);

        let seventeen = fixture.jvm.join("a-17/Contents/Home");
        fixture.run("toolchain.jdk.choose", json!({ "home": seventeen }));
        fixture.run("toolchain.jdk.languageLevel", json!({ "level": "21" }));
        let problems: ProblemsSection =
            serde_json::from_value(fixture.run("project.structure.problems", json!({}))).unwrap();
        let found: Vec<(Section, &str, bool)> = problems
            .problems
            .iter()
            .map(|problem| (problem.section, problem.message.as_str(), problem.warning))
            .collect();
        assert_eq!(
            found,
            [
                (Section::Project, "The language level 21 is above the project JDK 17.", false),
                (Section::Libraries, "Maven: org.demo:lib:1.0 is not downloaded.", true),
                (Section::Libraries, "Maven: org.junit:junit:5.0 is not downloaded.", true),
            ]
        );

        let opened = fixture.run("project.structure.openBuildFile", json!({ "module": "shop" }));
        assert_eq!(opened["path"], json!(root.join("pom.xml")));
        let active =
            fixture.session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
        assert_eq!(active, Some(root.join("pom.xml")));
    }

    #[test]
    fn a_gradle_project_marks_its_output_as_the_convention() {
        let mut fixture = Fixture::new(&[
            ("settings.gradle.kts", "rootProject.name = \"shop\"\ninclude(\"app\")\n"),
            (
                "app/build.gradle.kts",
                "java { toolchain { languageVersion = JavaLanguageVersion.of(21) } }\n\
                 dependencies {\n    implementation(\"org.demo:lib:1.0\")\n    testImplementation(\"org.junit:junit:5.0\")\n}\n",
            ),
            ("app/src/main/kotlin/App.kt", ""),
        ]);
        let root = fixture.root.clone();
        let modules: ModulesSection =
            serde_json::from_value(fixture.run("project.structure.modules", json!({}))).unwrap();
        let app = modules.modules.iter().find(|module| module.name == "app").unwrap();
        assert!(app.assumed);
        assert_eq!(app.language_version.as_deref(), Some("21"));
        assert_eq!(app.output, Some(root.join("app/build/classes/java/main")));
        assert_eq!(app.folders[0].path, root.join("app/src/main/kotlin"));
        let scopes: Vec<Option<&str>> = app.dependencies.iter().map(|dependency| dependency.scope.as_deref()).collect();
        assert_eq!(scopes, [Some("implementation"), Some("testImplementation")]);
        let project: ProjectSection =
            serde_json::from_value(fixture.run("project.structure.project", json!({}))).unwrap();
        assert_eq!(project.compiler_output.origin, Origin::Convention);
        assert_eq!(project.build_file, Some(root.join("settings.gradle.kts")), "the root has a settings script only");
    }

    #[test]
    fn gradle_language_levels_read_in_every_spelling() {
        for (text, level) in [
            ("java { toolchain { languageVersion.set(JavaLanguageVersion.of(17)) } }", Some("17")),
            ("kotlin { jvmToolchain(21) }", Some("21")),
            ("java { sourceCompatibility = JavaVersion.VERSION_1_8 }", Some("1.8")),
            ("sourceCompatibility = '11'", Some("11")),
            ("plugins { id 'java' }", None),
        ] {
            assert_eq!(crate::jvm_modules::gradle_language_level(text).as_deref(), level, "{text}");
        }
    }

    #[test]
    fn a_project_without_a_build_says_its_output_is_not_known() {
        let mut fixture = Fixture::new(&[("README.md", "")]);
        let project: ProjectSection =
            serde_json::from_value(fixture.run("project.structure.project", json!({}))).unwrap();
        assert!(!project.jvm);
        assert_eq!(project.compiler_output.origin, Origin::Unknown);
        assert!(fixture.registry.dispatch(&mut fixture.session, "project.structure.openBuildFile", json!({})).is_err());
        let modules: ModulesSection =
            serde_json::from_value(fixture.run("project.structure.modules", json!({}))).unwrap();
        assert!(modules.modules.is_empty());
    }
}
