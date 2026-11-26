use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::generators::{
    GeneratedFile, GeneratorForm, GeneratorInfo, GeneratorRequest, GeneratorSection, ModuleBuild, ModuleEntry,
    ProjectGenerator, Values, write_files,
};

use super::*;
use crate::build_tools::BuildEnv;

struct Fake {
    id: &'static str,
    section: GeneratorSection,
    fail: bool,
    forms: Mutex<Vec<Result<GeneratorForm, String>>>,
    gate: Option<Arc<(Mutex<bool>, Condvar)>>,
    folder: bool,
    no_files: bool,
    previews: AtomicUsize,
}

impl Fake {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            section: GeneratorSection::Types,
            fail: false,
            forms: Mutex::new(Vec::new()),
            gate: None,
            folder: false,
            no_files: false,
            previews: AtomicUsize::new(0),
        }
    }
}

impl ProjectGenerator for Fake {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo {
            id: self.id.to_owned(),
            name: self.id.to_owned(),
            section: self.section,
            ..GeneratorInfo::default()
        }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        if let Some(gate) = &self.gate {
            let (open, changed) = &**gate;
            let mut open = open.lock().unwrap();
            while !*open {
                open = changed.wait(open).unwrap();
            }
        }
        self.forms.lock().unwrap().pop().unwrap_or_else(|| {
            serde_json::from_value(json!({ "fields": [
                { "id": "greeting", "label": "Greeting", "type": "text", "default": "hello ${name}" },
            ] }))
            .map_err(|error| error.to_string())
        })
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        self.previews.fetch_add(1, Ordering::Relaxed);
        if self.no_files {
            return Err("cannot".to_owned());
        }
        let mut files = vec![
            GeneratedFile::text("README.md", request.values["greeting"].as_str().unwrap_or_default()),
            GeneratedFile::text("src/main.txt", "main"),
        ];
        if self.folder {
            files.push(GeneratedFile::text("src/static/", ""));
        }
        Ok(files)
    }

    fn generate(&self, request: &GeneratorRequest, folder: &Path, _stop: &AtomicBool) -> Result<(), String> {
        write_files(folder, &self.files(request)?)?;
        if self.fail { Err("the generator broke".to_owned()) } else { Ok(()) }
    }
}

struct Setup {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Setup {
    fn new(generators: Vec<Arc<dyn ProjectGenerator>>) -> Self {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(BuildEnv::default());
        let projects = NewProjects::default();
        for generator in generators {
            projects.declare(generator);
        }
        session.set_state(projects);
        Self { registry, session, dir: tempfile::tempdir().unwrap() }
    }

    fn dispatch(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        self.registry.dispatch(&mut self.session, command, params)
    }

    fn call(&mut self, command: &str, params: Value) -> Value {
        self.dispatch(command, params).unwrap()
    }

    fn location(&self, name: &str) -> String {
        std::fs::canonicalize(self.dir.path()).unwrap().join(name).to_string_lossy().into_owned()
    }

    fn folder_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn code(error: zen_core::DispatchError) -> Option<String> {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.code.map(|code| code.to_string()),
        _ => None,
    }
}

#[test]
fn the_list_has_the_modules_types_then_empty_project_then_the_plugins_in_their_order() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("go"))]);
    setup.session.state::<NewProjects>().unwrap().clone().set_plugins(Arc::new(|_| {
        let plugin =
            |id| Arc::new(Fake { section: GeneratorSection::Generators, ..Fake::new(id) }) as Arc<dyn ProjectGenerator>;
        vec![plugin("vite"), plugin("spring")]
    }));
    let list = setup.call("project.new.generators", json!({}));
    let ids: Vec<&str> = list["generators"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["go", EMPTY, "vite", "spring"]);
    assert_eq!(list["generators"][2]["section"], "generators");
}

#[test]
fn a_language_s_plug_in_type_stands_above_empty_project_and_others_under_generators() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("go"))]);
    setup.session.state::<NewProjects>().unwrap().clone().set_plugins(Arc::new(|_| {
        let generator = Fake { section: GeneratorSection::Generators, ..Fake::new("spring") };
        vec![Arc::new(generator) as Arc<dyn ProjectGenerator>, Arc::new(Fake::new("java"))]
    }));
    let list = setup.call("project.new.generators", json!({}));
    let ids: Vec<&str> = list["generators"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["go", "java", EMPTY, "spring"]);
}

struct JvmType;

impl ProjectGenerator for JvmType {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo { id: "java".to_owned(), name: "Java".to_owned(), ..GeneratorInfo::default() }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        serde_json::from_value(json!({ "fields": [
            { "id": "jdk", "label": "JDK:", "type": "jdk", "required": true },
        ] }))
        .map_err(|error| error.to_string())
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        Ok(vec![GeneratedFile::text("release.txt", request.values["jdkRelease"].to_string())])
    }
}

fn fake_jdk(home: &Path, version: &str) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/java"), "").unwrap();
    std::fs::write(home.join("bin/javac"), "").unwrap();
    std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{version}\"\n")).unwrap();
}

fn with_jdks(setup: &mut Setup, versions: &[&str]) -> Vec<String> {
    crate::jdk::register(&mut setup.registry).unwrap();
    let jvm = std::fs::canonicalize(setup.dir.path()).unwrap().join(".jvm");
    std::fs::create_dir_all(&jvm).unwrap();
    let homes = versions.iter().map(|version| {
        let home = jvm.join(format!("{version}.jdk/Contents/Home"));
        fake_jdk(&home, version);
        home.to_string_lossy().into_owned()
    });
    let homes = homes.collect();
    let sources = crate::jdk::Sources { system_folders: vec![jvm], ..crate::jdk::Sources::default() };
    let mut store = crate::jdk::Store::with_sources(sources);
    store.set_data_dir(setup.dir.path().join(".data"));
    setup.session.set_state(Jdks::new(store));
    homes
}

fn jdk_form(setup: &mut Setup) -> Value {
    setup.call("project.new.form", json!({ "generator": "java", "retry": true, "waitMs": 60_000 }))["form"]["fields"][0]
        .clone()
}

#[test]
fn a_jdk_field_lists_the_jdks_of_the_computer_and_starts_on_the_newest() {
    let mut setup = Setup::new(vec![Arc::new(JvmType)]);
    let homes = with_jdks(&mut setup, &["17.0.12", "21.0.5"]);
    let field = jdk_form(&mut setup);
    let listed: Vec<&str> = field["options"].as_array().unwrap().iter().map(|o| o["value"].as_str().unwrap()).collect();
    assert_eq!(listed.len(), 2, "{field}");
    assert!(homes.iter().all(|home| listed.contains(&home.as_str())), "{field}");
    assert_eq!(field["default"], homes[1]);
}

#[test]
fn a_jdk_downloaded_or_added_shows_when_the_form_loads_again() {
    let mut setup = Setup::new(vec![Arc::new(JvmType)]);
    with_jdks(&mut setup, &[]);
    assert_eq!(jdk_form(&mut setup)["options"], Value::Null);
    let elsewhere = setup.dir.path().join(".other");
    fake_jdk(&elsewhere, "25.0.1");
    setup.call("toolchain.jdk.add", json!({ "home": elsewhere }));
    assert_eq!(jdk_form(&mut setup)["default"], elsewhere.to_string_lossy().as_ref());
}

#[test]
fn no_jdk_refuses_create_with_the_reason_and_leaves_nothing() {
    let mut setup = Setup::new(vec![Arc::new(JvmType)]);
    with_jdks(&mut setup, &[]);
    let location = setup.location("shop");
    let params = json!({ "generator": "java", "location": location, "waitMs": 60_000 });
    let answer = setup.call("project.new.create", params);
    assert_eq!(answer["state"], "failed");
    assert_eq!(answer["error"], t!("workbench-new-project-no-jdk"));
    assert!(!Path::new(&location).exists());
}

#[test]
fn the_generator_gets_the_chosen_jdk_s_release_and_the_project_keeps_that_jdk() {
    let mut setup = Setup::new(vec![Arc::new(JvmType)]);
    let homes = with_jdks(&mut setup, &["17.0.12", "21.0.5"]);
    let location = setup.location("shop");
    let params = json!({ "generator": "java", "location": location, "values": { "jdk": homes[0] }, "waitMs": 60_000 });
    let answer = setup.call("project.new.create", params);
    assert_eq!(answer["state"], "ready", "{answer}");
    assert_eq!(std::fs::read_to_string(Path::new(&location).join("release.txt")).unwrap(), "17");
    let project = setup.call("toolchain.jdk.project", json!({ "project": location }));
    assert_eq!(project["jdk"]["home"], homes[0]);
    assert_eq!(project["chosen"], true);
}

#[test]
fn the_default_location_is_free_and_in_the_default_project_directory() {
    let mut setup = Setup::new(Vec::new());
    let directory = setup.dir.path().to_owned();
    setup.session.settings_mut().set(DEFAULT_DIRECTORY, &directory).unwrap();
    std::fs::create_dir(directory.join("untitled")).unwrap();
    let list = setup.call("project.new.generators", json!({}));
    assert_eq!(list["defaultDirectory"], json!(directory));
    assert_eq!(list["suggested"], json!(directory.join("untitled1")));
}

#[test]
fn a_form_loads_in_the_background_and_says_so_until_it_is_there() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let fake = Fake { gate: Some(Arc::clone(&gate)), ..Fake::new("spring") };
    let mut setup = Setup::new(vec![Arc::new(fake)]);
    let first = setup.call("project.new.form", json!({ "generator": "spring" }));
    assert_eq!(first["state"], "running", "{first}");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let ready = setup.call("project.new.form", json!({ "generator": "spring", "waitMs": 10000 }));
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["form"]["fields"][0]["id"], "greeting");
}

#[test]
fn a_form_that_failed_says_why_and_retry_loads_it_again() {
    let fake = Fake::new("spring");
    fake.forms.lock().unwrap().push(Err("the service does not answer".to_owned()));
    let mut setup = Setup::new(vec![Arc::new(fake)]);
    let failed = setup.call("project.new.form", json!({ "generator": "spring", "waitMs": 10000 }));
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["error"], "the service does not answer");
    let again = setup.call("project.new.form", json!({ "generator": "spring", "waitMs": 10000 }));
    assert_eq!(again["state"], "failed", "kept until asked to retry");
    let retried = setup.call("project.new.form", json!({ "generator": "spring", "retry": true, "waitMs": 10000 }));
    assert_eq!(retried["state"], "ready", "{retried}");
}

#[test]
fn an_unknown_generator_is_refused() {
    let mut setup = Setup::new(Vec::new());
    let refused = setup.dispatch("project.new.form", json!({ "generator": "nope" })).unwrap_err();
    assert_eq!(code(refused).as_deref(), Some(UNKNOWN_GENERATOR));
}

#[test]
fn a_preview_shows_the_files_in_path_order_and_writes_none() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("spring"))]);
    let params = json!({ "generator": "spring", "name": "shop", "waitMs": 10000 });
    let preview = setup.call("project.new.preview", params);
    assert_eq!(preview["state"], "ready", "{preview}");
    assert_eq!(preview["files"][0]["path"], "README.md");
    assert_eq!(preview["files"][0]["text"], "hello shop");
    assert_eq!(preview["files"][1]["path"], "src/main.txt");
    assert!(setup.folder_names().is_empty());
}

#[test]
fn locations_that_cannot_hold_a_project_give_the_reason() {
    let mut setup = Setup::new(Vec::new());
    let file = setup.location("file.txt");
    std::fs::write(&file, "x").unwrap();
    let problem = |setup: &mut Setup, location: &str| {
        setup.call("project.new.check", json!({ "location": location }))["problem"].as_str().map(str::to_owned)
    };
    assert!(problem(&mut setup, "").is_some(), "empty");
    assert!(problem(&mut setup, "relative/path").is_some(), "relative");
    assert!(problem(&mut setup, &file).is_some(), "a file");
    assert!(problem(&mut setup, &format!("{file}/inside")).is_some(), "below a file");
    let wild = setup.location("na?me");
    assert!(problem(&mut setup, &wild).is_some(), "a bad name");
    let fine = setup.location("deep/er/shop");
    assert_eq!(problem(&mut setup, &fine), None);
    let check = setup.call("project.new.check", json!({ "location": fine }));
    assert_eq!(check["name"], "shop");
    assert_eq!(check["notEmpty"], false);
}

#[test]
fn creating_writes_the_files_and_a_person_s_project_is_trusted() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("spring"))]);
    let location = setup.location("shop");
    let params = json!({ "generator": "spring", "location": location, "waitMs": 10000 });
    let created = setup.call("project.new.create", params);
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["path"], json!(location));
    assert_eq!(created["trusted"], true, "a project the person creates opens trusted");
    assert_eq!(std::fs::read_to_string(Path::new(&location).join("README.md")).unwrap(), "hello shop");
    assert_eq!(setup.folder_names(), ["shop"], "no staging folder is left");
    assert!(!Path::new(&location).join(".git").exists());
}

#[test]
fn an_agent_s_project_is_not_trusted() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("spring"))]);
    let location = setup.location("shop");
    let params = json!({ "generator": "spring", "location": location, "waitMs": 10000 });
    let Setup { registry, session, .. } = &mut setup;
    let created =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "project.new.create", params))
            .unwrap();
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["trusted"], false);
    assert!(!crate::trust::trusted(session, Path::new(&location)));
}

#[test]
fn create_git_repository_initialises_one_with_the_ide_s_git() {
    let mut setup = Setup::new(Vec::new());
    let location = setup.location("empty");
    let params = json!({ "generator": EMPTY, "location": location, "git": true, "waitMs": 10000 });
    let created = setup.call("project.new.create", params);
    assert_eq!(created["state"], "ready", "{created}");
    assert!(zen_vcs::git::run(Path::new(&location), &["rev-parse", "--git-dir"], None).is_ok());
    assert!(Path::new(&location).join(".git").is_dir());
}

#[test]
fn a_failing_generator_reports_its_error_and_leaves_nothing() {
    let mut setup = Setup::new(vec![Arc::new(Fake { fail: true, ..Fake::new("spring") })]);
    let location = setup.location("deep/shop");
    let params = json!({ "generator": "spring", "location": location, "waitMs": 10000 });
    let failed = setup.call("project.new.create", params);
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["error"], "the generator broke");
    assert!(setup.folder_names().is_empty(), "{:?}", setup.folder_names());
}

#[test]
fn a_failing_generator_leaves_an_existing_folder_as_it_was() {
    let mut setup = Setup::new(vec![Arc::new(Fake { fail: true, ..Fake::new("spring") })]);
    let location = setup.location("shop");
    std::fs::create_dir(&location).unwrap();
    std::fs::write(Path::new(&location).join("mine.txt"), "mine").unwrap();
    let params = json!({ "generator": "spring", "location": location, "reuse": true, "waitMs": 10000 });
    assert_eq!(setup.call("project.new.create", params)["state"], "failed");
    assert_eq!(setup.folder_names(), ["shop"]);
    let names: Vec<_> = std::fs::read_dir(&location).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(names, ["mine.txt"]);
}

#[test]
fn a_folder_that_holds_something_is_used_only_when_asked_and_its_files_are_kept() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("spring"))]);
    let location = setup.location("shop");
    std::fs::create_dir_all(Path::new(&location).join("src")).unwrap();
    std::fs::write(Path::new(&location).join("README.md"), "mine").unwrap();
    let params = json!({ "generator": "spring", "location": location, "waitMs": 10000 });
    let refused = setup.dispatch("project.new.create", params.clone()).unwrap_err();
    assert_eq!(code(refused).as_deref(), Some(FOLDER_NOT_EMPTY));
    let mut reuse = params;
    reuse["reuse"] = json!(true);
    let created = setup.call("project.new.create", reuse);
    assert_eq!(created["state"], "ready", "{created}");
    assert_eq!(created["kept"], json!(["README.md"]));
    let root = Path::new(&location);
    assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(), "mine");
    assert!(root.join("src/main.txt").is_file(), "a folder both have takes the new files");
    assert_eq!(setup.folder_names(), ["shop"]);
}

#[test]
fn an_invalid_location_is_refused_by_create() {
    let mut setup = Setup::new(Vec::new());
    let refused = setup.dispatch("project.new.create", json!({ "generator": EMPTY, "location": "x/y" })).unwrap_err();
    assert_eq!(code(refused).as_deref(), Some(INVALID_LOCATION));
}

#[test]
fn a_cancelled_creation_removes_what_was_written() {
    let setup = Setup::new(Vec::new());
    let location = setup.location("shop");
    let request =
        GeneratorRequest { name: "shop".to_owned(), values: Values::from([("greeting".to_owned(), json!("hi"))]) };
    let stop = AtomicBool::new(true);
    let output = JobOutput::default();
    let error =
        create_project(&Fake::new("spring"), &request, Path::new(&location), false, &stop, &output, None).unwrap_err();
    assert_eq!(error, t!("workbench-new-project-cancelled"));
    assert!(setup.folder_names().is_empty());
}

#[test]
fn cancelling_a_job_nobody_knows_says_so() {
    let mut setup = Setup::new(Vec::new());
    assert_eq!(setup.call("project.new.cancel", json!({ "job": 99 }))["cancelled"], false);
}

#[test]
fn a_generator_that_panics_fails_its_job_instead_of_hanging() {
    struct Panics;
    impl ProjectGenerator for Panics {
        fn info(&self) -> GeneratorInfo {
            GeneratorInfo { id: "panics".to_owned(), ..GeneratorInfo::default() }
        }

        fn form(&self) -> Result<GeneratorForm, String> {
            panic!("broken")
        }

        fn files(&self, _request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
            Ok(Vec::new())
        }
    }
    let mut setup = Setup::new(vec![Arc::new(Panics)]);
    let failed = setup.call("project.new.form", json!({ "generator": "panics", "waitMs": 10000 }));
    assert_eq!(failed["state"], "failed", "{failed}");
}

#[test]
fn a_tilde_is_the_home_folder() {
    assert_eq!(expanded("~/work"), home().join("work"));
    assert_eq!(expanded("~"), home());
    assert_eq!(expanded("/abs/~"), PathBuf::from("/abs/~"));
}

#[test]
fn a_failed_preview_is_tried_again() {
    let generator = Arc::new(Fake { no_files: true, ..Fake::new("flaky") });
    let mut setup = Setup::new(vec![Arc::clone(&generator) as _]);
    let params = json!({ "generator": "flaky", "name": "shop", "waitMs": 5000 });
    assert_eq!(setup.call("project.new.preview", params.clone())["state"], "failed");
    assert_eq!(generator.previews.load(Ordering::Relaxed), 1);
    assert_eq!(setup.call("project.new.preview", params)["state"], "failed");
    assert_eq!(generator.previews.load(Ordering::Relaxed), 2);
}

#[test]
fn folders_a_generator_makes_are_not_listed_as_files() {
    let generator = Arc::new(Fake { folder: true, ..Fake::new("tree") });
    let mut setup = Setup::new(vec![generator as _]);
    let answer = setup.call("project.new.preview", json!({ "generator": "tree", "name": "shop", "waitMs": 5000 }));
    let paths: Vec<&str> = answer["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["README.md", "src/main.txt"]);
}

#[test]
fn the_settings_reach_the_plugins_and_a_change_loads_the_form_again() {
    let mut setup = Setup::new(Vec::new());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    setup.session.state::<NewProjects>().unwrap().clone().set_plugins(Arc::new(move |environment| {
        recorded.lock().unwrap().push(environment.clone());
        let plugin = Fake { section: GeneratorSection::Generators, ..Fake::new("spring") };
        vec![Arc::new(plugin) as Arc<dyn ProjectGenerator>]
    }));
    let list = setup.call("project.new.generators", json!({}));
    assert_eq!(list["networkAllowed"], true);
    assert!(seen.lock().unwrap().last().unwrap().network, "on by default");
    setup.call("project.new.form", json!({ "generator": "spring", "waitMs": 5000 }));
    let projects = setup.session.state::<NewProjects>().unwrap().clone();
    assert_eq!(projects.lock().forms.len(), 1);
    setup.session.settings_mut().set(GENERATORS_NETWORK, false).unwrap();
    setup.session.settings_mut().set(GENERATOR_SETTINGS, json!({ "spring.url": "https://own.example" })).unwrap();
    let list = setup.call("project.new.generators", json!({}));
    assert_eq!(list["networkAllowed"], false);
    let environment = seen.lock().unwrap().last().unwrap().clone();
    assert!(!environment.network);
    assert_eq!(environment.settings["spring.url"], "https://own.example");
    assert!(projects.lock().forms.is_empty(), "what was loaded with the old settings is dropped");
}

fn git_in(folder: &Path, args: &[&str]) {
    zen_vcs::git::run(folder, args, None).unwrap();
}

fn repository(folder: &Path) -> String {
    std::fs::create_dir_all(folder).unwrap();
    git_in(folder, &["init", "-q"]);
    git_in(folder, &["config", "user.email", "a@b.c"]);
    git_in(folder, &["config", "user.name", "A"]);
    for number in 0..3 {
        std::fs::write(folder.join("file.txt"), number.to_string()).unwrap();
        git_in(folder, &["add", "-A"]);
        git_in(folder, &["commit", "-q", "-m", "change"]);
    }
    format!("file://{}", folder.display())
}

#[test]
fn a_clone_lands_in_the_directory_is_never_trusted_and_is_remembered_without_its_password() {
    let mut setup = Setup::new(Vec::new());
    let data = setup.dir.path().join("data");
    setup.session.state::<NewProjects>().unwrap().set_data_dir(data.clone());
    let url = repository(&setup.dir.path().join("origin"));
    let target = setup.location("shop");
    let answer = setup.call("project.fromVcs.clone", json!({ "url": url, "directory": target, "waitMs": 30000 }));
    assert_eq!(answer["state"], "ready", "{answer}");
    assert_eq!(answer["trusted"], false);
    assert_eq!(answer["path"], target);
    assert!(Path::new(&target).join("file.txt").is_file());
    assert_eq!(setup.folder_names(), ["data", "origin", "shop"], "no hidden folder stays");
    assert_eq!(setup.call("project.fromVcs.defaults", json!({}))["urls"], json!([url]));
    let reopened = NewProjects::default();
    reopened.set_data_dir(data);
    assert_eq!(reopened.lock().kept.clone_urls, std::slice::from_ref(&url), "the history survives a restart");
    reopened.remember_url("https://me:secret@host/a.git");
    assert_eq!(reopened.lock().kept.clone_urls[0], "https://host/a.git");
}

#[test]
fn a_shallow_clone_keeps_the_commits_asked_for() {
    let mut setup = Setup::new(Vec::new());
    let url = repository(&setup.dir.path().join("origin"));
    let target = setup.location("shop");
    let params = json!({ "url": url, "directory": target, "depth": 2, "waitMs": 30000 });
    assert_eq!(setup.call("project.fromVcs.clone", params)["state"], "ready");
    let count = zen_vcs::git::run(Path::new(&target), &["rev-list", "--count", "HEAD"], None).unwrap();
    assert_eq!(count.stdout.trim(), "2");
}

#[test]
fn a_failed_clone_leaves_nothing() {
    let mut setup = Setup::new(Vec::new());
    let url = format!("file://{}", setup.dir.path().join("nothing-here").display());
    let target = setup.location("shop");
    let answer = setup.call("project.fromVcs.clone", json!({ "url": url, "directory": target, "waitMs": 30000 }));
    assert_eq!(answer["state"], "failed", "{answer}");
    assert!(setup.folder_names().is_empty(), "{:?}", setup.folder_names());
    assert!(setup.session.state::<NewProjects>().unwrap().lock().kept.clone_urls.is_empty());
}

#[test]
fn a_cancelled_clone_leaves_nothing() {
    let mut setup = Setup::new(Vec::new());
    let url = repository(&setup.dir.path().join("origin"));
    let target = setup.location("shop");
    let started = setup.call("project.fromVcs.clone", json!({ "url": url, "directory": target }));
    let job = started["job"].as_u64().unwrap();
    assert_eq!(setup.call("project.new.cancel", json!({ "job": job }))["cancelled"], true);
    let answer = setup.call("project.new.status", json!({ "job": job, "waitMs": 30000 }));
    assert_eq!(answer["state"], "failed", "{answer}");
    assert_eq!(setup.folder_names(), ["origin"]);
}

#[test]
fn a_clone_needs_an_address_and_a_free_directory() {
    let mut setup = Setup::new(Vec::new());
    assert!(setup.call("project.fromVcs.check", json!({ "url": "" }))["problem"].is_string());
    let suggested = setup.call("project.fromVcs.check", json!({ "url": "https://host/team/shop.git" }));
    assert!(suggested["suggested"].as_str().unwrap().ends_with("/shop"), "{suggested}");
    let taken = setup.location("taken");
    std::fs::create_dir(&taken).unwrap();
    std::fs::write(Path::new(&taken).join("a"), "").unwrap();
    let check = setup.call("project.fromVcs.check", json!({ "url": "https://host/a.git", "directory": taken }));
    assert!(check["problem"].is_string(), "{check}");
    let refused = setup.dispatch("project.fromVcs.clone", json!({ "url": "https://host/a.git", "directory": taken }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(INVALID_LOCATION));
    let refused =
        setup.dispatch("project.fromVcs.clone", json!({ "url": "ext::sh", "directory": setup.location("x") }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(INVALID_LOCATION));
}

#[derive(Default)]
struct FakeBuild {
    fail: bool,
    asked: Mutex<Vec<ModuleEntry>>,
}

impl ModuleBuild for FakeBuild {
    fn plugin(&self) -> String {
        "maven".to_owned()
    }

    fn module_files(&self) -> Vec<String> {
        vec!["pom.xml".to_owned()]
    }

    fn reload(&self) -> String {
        "maven.project.reload".to_owned()
    }

    fn add_module(&self, entry: &ModuleEntry) -> Result<Vec<GeneratedFile>, String> {
        self.asked.lock().unwrap().push(entry.clone());
        if self.fail {
            return Err("the parent cannot be read".to_owned());
        }
        if entry.text.contains(&entry.path) {
            return Ok(Vec::new());
        }
        Ok(vec![GeneratedFile::text(&entry.file, format!("{}{}\n", entry.text, entry.path))])
    }
}

struct ModuleSetup {
    setup: Setup,
    root: PathBuf,
    build: Arc<FakeBuild>,
}

fn module_setup(build: FakeBuild) -> ModuleSetup {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("go"))]);
    let root = std::fs::canonicalize(setup.dir.path()).unwrap().join("shop");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("pom.xml"), "parent\n").unwrap();
    setup.registry = Registry::new();
    crate::register(&mut setup.registry).unwrap();
    setup.call("file.project.open", json!({ "path": root }));
    let build = Arc::new(build);
    let shared: Arc<dyn ModuleBuild> = Arc::clone(&build) as Arc<dyn ModuleBuild>;
    setup.session.state::<NewProjects>().unwrap().set_builds(Arc::new(move || vec![Arc::clone(&shared)]));
    ModuleSetup { setup, root, build }
}

#[test]
fn new_module_offers_the_types_without_empty_project_from_the_project_s_root_and_its_build() {
    let mut module = module_setup(FakeBuild::default());
    let options = module.setup.call("project.module.options", json!({}));
    let ids: Vec<&str> = options["generators"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["go"]);
    assert_eq!(options["location"], module.root.to_string_lossy().as_ref());
    assert_eq!(options["build"]["file"], "pom.xml");
    assert_eq!(options["values"]["build"], "maven");
}

#[test]
fn a_new_module_is_listed_in_the_build_and_says_what_loads_it() {
    let mut module = module_setup(FakeBuild::default());
    let params = json!({ "generator": "go", "name": "api", "waitMs": 30000 });
    let answer = module.setup.call("project.module.new", params);
    assert_eq!(answer["state"], "ready", "{answer}");
    assert_eq!(answer["registered"], "pom.xml");
    assert_eq!(answer["reload"], "maven.project.reload");
    assert!(module.root.join("api/README.md").is_file());
    assert_eq!(std::fs::read_to_string(module.root.join("pom.xml")).unwrap(), "parent\napi\n");
    assert_eq!(module.build.asked.lock().unwrap()[0].project, "shop");
}

#[test]
fn a_module_in_a_folder_of_the_project_has_a_path_from_the_root() {
    let mut module = module_setup(FakeBuild::default());
    let location = module.root.join("services");
    let params = json!({ "generator": "go", "name": "api", "location": location, "waitMs": 30000 });
    assert_eq!(module.setup.call("project.module.new", params)["state"], "ready");
    assert_eq!(module.build.asked.lock().unwrap()[0].path, "services/api");
}

#[test]
fn a_module_the_build_refuses_leaves_nothing() {
    let mut module = module_setup(FakeBuild { fail: true, ..FakeBuild::default() });
    let params = json!({ "generator": "go", "name": "api", "waitMs": 30000 });
    let answer = module.setup.call("project.module.new", params);
    assert_eq!(answer["state"], "failed", "{answer}");
    assert!(!module.root.join("api").exists());
    assert_eq!(std::fs::read_to_string(module.root.join("pom.xml")).unwrap(), "parent\n");
    let names: Vec<_> = std::fs::read_dir(&module.root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn a_module_the_build_leaves_out_says_nothing_was_registered() {
    let mut module = module_setup(FakeBuild::default());
    std::fs::write(module.root.join("pom.xml"), "api\n").unwrap();
    let params = json!({ "generator": "go", "name": "api", "waitMs": 30000 });
    let answer = module.setup.call("project.module.new", params);
    assert_eq!(answer["state"], "ready", "{answer}");
    assert!(answer["registered"].is_null(), "{answer}");
    assert!(answer["reload"].is_null(), "{answer}");
}

#[test]
fn a_module_without_a_build_is_a_plain_folder() {
    let mut module = module_setup(FakeBuild::default());
    std::fs::remove_file(module.root.join("pom.xml")).unwrap();
    assert!(module.setup.call("project.module.options", json!({}))["build"].is_null());
    let params = json!({ "generator": "go", "name": "api", "waitMs": 30000 });
    let answer = module.setup.call("project.module.new", params);
    assert_eq!(answer["state"], "ready", "{answer}");
    assert!(answer["registered"].is_null());
}

#[test]
fn empty_project_is_no_module_and_a_taken_folder_needs_reuse() {
    let mut module = module_setup(FakeBuild::default());
    let refused = module.setup.dispatch("project.module.new", json!({ "generator": EMPTY, "name": "x" }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(UNKNOWN_GENERATOR));
    std::fs::create_dir(module.root.join("api")).unwrap();
    std::fs::write(module.root.join("api/a"), "").unwrap();
    let refused = module.setup.dispatch("project.module.new", json!({ "generator": "go", "name": "api" }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(FOLDER_NOT_EMPTY));
}

#[test]
fn existing_sources_are_entered_once_in_the_build() {
    let mut module = module_setup(FakeBuild::default());
    let folder = module.root.join("legacy");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("pom.xml"), "").unwrap();
    let answer = module.setup.call("project.module.fromSources", json!({ "path": folder.join("pom.xml") }));
    assert_eq!(answer["registered"], "pom.xml");
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["reload"], "maven.project.reload");
    assert!(module.build.asked.lock().unwrap()[0].has_build);
    assert_eq!(std::fs::read_to_string(module.root.join("pom.xml")).unwrap(), "parent\nlegacy\n");
    let again = module.setup.call("project.module.fromSources", json!({ "path": folder }));
    assert_eq!(again["changed"], false);
    assert_eq!(std::fs::read_to_string(module.root.join("pom.xml")).unwrap(), "parent\nlegacy\n");
}

#[test]
fn the_project_itself_and_a_project_without_a_build_take_no_existing_sources() {
    let mut module = module_setup(FakeBuild::default());
    let root = module.root.clone();
    let refused = module.setup.dispatch("project.module.fromSources", json!({ "path": root }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(INVALID_LOCATION));
    std::fs::remove_file(root.join("pom.xml")).unwrap();
    std::fs::create_dir(root.join("legacy")).unwrap();
    let refused = module.setup.dispatch("project.module.fromSources", json!({ "path": root.join("legacy") }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(NO_BUILD));
}

#[test]
fn project_from_existing_sources_opens_nothing_by_itself_and_names_the_build() {
    let mut setup = Setup::new(Vec::new());
    let folder = setup.location("shop");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(Path::new(&folder).join("pom.xml"), "").unwrap();
    let found = setup.call("project.fromSources.detect", json!({ "path": folder }));
    assert_eq!(found["build"], "maven");
    assert_eq!(found["root"], folder);
    let refused = setup.dispatch("project.fromSources.detect", json!({ "path": "/no/such/place" }));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(INVALID_LOCATION));
}

struct ToolGenerator;

impl ProjectGenerator for ToolGenerator {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo { id: "archetype".to_owned(), tool: Some("maven".to_owned()), ..GeneratorInfo::default() }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        Ok(GeneratorForm::default())
    }

    fn files(&self, _request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        Ok(Vec::new())
    }

    fn generate_showing(
        &self,
        _request: &GeneratorRequest,
        folder: &Path,
        _stop: &AtomicBool,
        output: &dyn Fn(&str),
    ) -> Result<(), String> {
        std::fs::write(folder.join("half.txt"), "").unwrap();
        output("[ERROR] the archetype failed");
        Err("Maven failed".to_owned())
    }
}

#[test]
fn a_tool_s_failure_leaves_nothing_and_its_output_stays_visible() {
    let mut setup = Setup::new(vec![Arc::new(ToolGenerator)]);
    let target = setup.location("shop");
    let answer =
        setup.call("project.new.create", json!({ "generator": "archetype", "location": target, "waitMs": 30000 }));
    assert_eq!(answer["state"], "failed", "{answer}");
    assert_eq!(answer["output"], json!(["[ERROR] the archetype failed"]));
    assert!(setup.folder_names().is_empty(), "{:?}", setup.folder_names());
}

#[test]
fn a_generator_s_tool_waits_while_the_window_shows_a_project_not_trusted() {
    let mut setup = Setup::new(vec![Arc::new(ToolGenerator)]);
    let project = tempfile::tempdir().unwrap();
    setup.registry = Registry::new();
    crate::register(&mut setup.registry).unwrap();
    setup.call("file.project.open", json!({ "path": project.path() }));
    let params = json!({ "generator": "archetype", "location": setup.location("shop") });
    let refused = setup.dispatch("project.new.create", params.clone()).unwrap_err();
    assert_eq!(code(refused).as_deref(), Some(crate::trust::NOT_TRUSTED));
    assert!(setup.folder_names().is_empty());
    crate::trust::trust_session_in_tests(&mut setup.session, project.path());
    setup.dispatch("project.new.create", params).expect("the person's Create, in a trusted window");
}

#[test]
fn a_folder_that_held_something_is_not_trusted_by_creating_in_it() {
    let mut setup = Setup::new(vec![Arc::new(Fake::new("spring"))]);
    let location = setup.location("shop");
    std::fs::create_dir(&location).unwrap();
    std::fs::write(Path::new(&location).join("pom.xml"), "<project/>").unwrap();
    let params = json!({ "generator": "spring", "location": location, "reuse": true, "waitMs": 10000 });
    let created = setup.call("project.new.create", params);
    assert_eq!((created["state"].as_str(), created["trusted"].as_bool()), (Some("ready"), Some(false)), "{created}");
    let fresh = setup.location("fresh");
    let params = json!({ "generator": "spring", "location": fresh, "waitMs": 10000 });
    assert_eq!(setup.call("project.new.create", params)["trusted"], true);
}

#[test]
fn an_agent_s_clone_is_not_trusted_either() {
    let mut setup = Setup::new(Vec::new());
    let url = repository(&setup.dir.path().join("origin"));
    let params = json!({ "url": url, "directory": setup.location("shop"), "waitMs": 30000 });
    let Setup { registry, session, .. } = &mut setup;
    let answer =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "project.fromVcs.clone", params))
            .unwrap();
    assert_eq!((answer["state"].as_str(), answer["trusted"].as_bool()), (Some("ready"), Some(false)), "{answer}");
}

#[test]
fn an_agent_cannot_add_a_catalog_a_person_would_create_from() {
    let mut setup = Setup::new(Vec::new());
    let params = json!({ "generator": "archetype", "list": "catalogs", "rows": [{ "url": "https://evil.example" }] });
    let Setup { registry, session, .. } = &mut setup;
    let refused = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "project.newLists.set", params.clone())
    });
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(crate::trust::AGENT_NOT_ALLOWED));
    assert_eq!(setup.call("project.newLists.set", params)["rows"][0]["url"], "https://evil.example");
}

#[test]
fn an_agent_cannot_run_a_generator_s_tool() {
    let mut setup = Setup::new(vec![Arc::new(ToolGenerator)]);
    let params = json!({ "generator": "archetype", "location": setup.location("shop") });
    let (registry, session) = (&setup.registry, &mut setup.session);
    let refused =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "project.new.create", params));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some(crate::trust::NOT_TRUSTED));
    assert!(setup.folder_names().is_empty());
}

struct ListGenerator {
    seen: Mutex<Vec<Values>>,
}

impl ProjectGenerator for ListGenerator {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo {
            id: "lists".to_owned(),
            form_depends_on: vec!["catalog".to_owned()],
            ..GeneratorInfo::default()
        }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        Ok(GeneratorForm::default())
    }

    fn form_with(&self, values: &Values) -> Result<GeneratorForm, String> {
        self.seen.lock().unwrap().push(values.clone());
        Ok(GeneratorForm::default())
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        Ok(vec![GeneratedFile::text("lists.json", request.values["lists"].to_string())])
    }
}

#[test]
fn a_form_is_loaded_again_for_the_fields_it_depends_on_and_the_lists_kept_for_its_type() {
    let generator = Arc::new(ListGenerator { seen: Mutex::default() });
    let mut setup = Setup::new(vec![Arc::clone(&generator) as Arc<dyn ProjectGenerator>]);
    let form = |values: Value| json!({ "generator": "lists", "values": values, "waitMs": 30000 });
    setup.call("project.new.form", form(json!({ "catalog": "a", "other": "x" })));
    setup.call("project.new.form", form(json!({ "catalog": "a", "other": "y" })));
    assert_eq!(generator.seen.lock().unwrap().len(), 1, "another field does not reload it");
    setup.call("project.new.form", form(json!({ "catalog": "b" })));
    assert_eq!(generator.seen.lock().unwrap()[1]["catalog"], "b");
    let rows = json!([{ "name": "Ours", "url": "https://repo.example/c.xml" }]);
    setup.call("project.newLists.set", json!({ "generator": "lists", "list": "catalogs", "rows": rows }));
    assert_eq!(setup.call("project.newLists.get", json!({ "generator": "lists", "list": "catalogs" }))["rows"], rows);
    setup.call("project.new.form", form(json!({ "catalog": "b" })));
    assert_eq!(generator.seen.lock().unwrap()[2]["lists"]["catalogs"], rows, "a new list reloads it");
    let target = setup.location("shop");
    let params = json!({ "generator": "lists", "location": target, "values": { "catalog": "b" }, "waitMs": 30000 });
    assert_eq!(setup.call("project.new.create", params)["state"], "ready");
    let written = std::fs::read_to_string(Path::new(&target).join("lists.json")).unwrap();
    assert!(written.contains("Ours"), "{written}");
}
