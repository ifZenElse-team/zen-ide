pub mod bytecode;
pub mod class_file;
pub mod console;
pub mod coroutines;
pub mod debug;
pub mod jacoco;
pub mod junit;
pub(crate) mod members;
pub mod parser;
pub mod processing;
pub mod refactoring;
pub mod scan;
pub mod stream;

pub use debug::{JavaProgram, application_plan};
pub use junit::{JUNIT, JavaTestRun, TESTNG, TestOptions};

#[cfg(all(test, unix))]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use zen_core::{Registry, Session};

    pub(crate) fn executable(path: &Path, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub(crate) fn session(jdk: &Path, project: &Path) -> (Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(crate::jdk::Jdks::new(crate::jdk::Store::with_sources(crate::jdk::Sources::default())));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, project);
        let env =
            zen_run::RunEnv { tool: PathBuf::new(), env: vec![("JAVA_HOME".to_owned(), jdk.display().to_string())] };
        session.state_mut::<crate::run::Runs>().set_providers(Vec::new(), env);
        (registry, session)
    }
}
