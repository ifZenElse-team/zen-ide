use serde_json::json;
use zen_core::{Registry, Session};
use zen_modules::WindowToolchain;
use zen_workbench::project_model::ProjectModels;
use zen_workbench::run::Runs;

#[derive(Default)]
pub(super) struct ToolchainWatch {
    seen: Option<Vec<u64>>,
}

impl ToolchainWatch {
    pub(super) fn step(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let Some(WindowToolchain(toolchain)) = session.state::<WindowToolchain>().cloned() else { return false };
        let generations = toolchain.generations();
        let now: Vec<u64> = generations.iter().map(|(generation, _)| *generation).collect();
        let changed: Vec<&Vec<String>> = match &self.seen {
            Some(seen) if seen.len() == now.len() => generations
                .iter()
                .zip(seen)
                .filter(|((generation, _), seen)| generation != *seen)
                .map(|((_, languages), _)| languages)
                .collect(),
            Some(_) => generations.iter().map(|(_, languages)| languages).collect(),
            None => Vec::new(),
        };
        self.seen = Some(now);
        if changed.is_empty() {
            return false;
        }
        for language in changed.into_iter().flatten() {
            let _ = registry.dispatch(session, "language.servers.restart", json!({ "language": language }));
        }
        let runs = session.state_mut::<Runs>();
        runs.set_env(super::app::toolchain_run_env(Some(&toolchain)));
        runs.set_debugger(super::app::toolchain_debugger(Some(&toolchain)));
        session.state_mut::<ProjectModels>().set_env(super::app::toolchain_model_env(Some(&toolchain)));
        let _ = registry.dispatch(session, "project.model.refresh", json!({}));
        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use zen_lsp::fake::FakeServer;
    use zen_workbench::language::{Languages, ServerSpec};

    use super::*;

    #[derive(Default)]
    struct FakeToolchain {
        state: Mutex<(u64, PathBuf, Option<PathBuf>)>,
    }

    impl FakeToolchain {
        fn switch(&self, sdk: &Path, debugger: Option<&Path>) {
            let mut state = self.state.lock().unwrap();
            *state = (state.0 + 1, sdk.to_owned(), debugger.map(Path::to_owned));
        }
    }

    impl zen_lsp::ServerTools for FakeToolchain {}

    impl zen_modules::LanguageToolchain for FakeToolchain {
        fn generation(&self) -> u64 {
            self.state.lock().unwrap().0
        }

        fn model_env(&self) -> zen_workbench::model::ModelEnv {
            let sdk = self.state.lock().unwrap().1.clone();
            zen_workbench::model::ModelEnv {
                sdk_root: Some(sdk).filter(|sdk| !sdk.as_os_str().is_empty()),
                ..Default::default()
            }
        }

        fn run_env(&self) -> zen_run::RunEnv {
            zen_run::RunEnv { tool: self.state.lock().unwrap().1.join("bin/go"), env: Vec::new() }
        }

        fn debugger(&self) -> Option<PathBuf> {
            self.state.lock().unwrap().2.clone()
        }

        fn languages(&self) -> Vec<String> {
            vec!["go".to_owned()]
        }
    }

    fn sync_until(registry: &Registry, session: &mut Session, done: impl Fn(&Session) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            registry.dispatch(session, "language.servers.sync", json!({})).unwrap();
            if done(session) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the language server did not get the file");
    }

    #[test]
    fn a_new_debugger_reaches_the_runs() {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let toolchain = Arc::new(FakeToolchain::default());
        session.set_state(WindowToolchain(Arc::clone(&toolchain) as _));
        let mut watch = ToolchainWatch::default();
        assert!(!watch.step(&registry, &mut session));
        toolchain.switch(Path::new("/sdk"), Some(Path::new("/tools/dlv")));
        assert!(watch.step(&registry, &mut session));
        assert_eq!(session.state::<Runs>().map(|runs| runs.debugger().to_owned()), Some(PathBuf::from("/tools/dlv")));
        assert!(!watch.step(&registry, &mut session));
    }

    #[test]
    fn a_new_sdk_restarts_the_language_server_and_reaches_the_model_and_the_runs() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let main = project.join("main.go");
        std::fs::write(&main, "package main\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let toolchain = Arc::new(FakeToolchain::default());
        toolchain.switch(Path::new("/sdks/a"), None);
        session.set_state(WindowToolchain(Arc::clone(&toolchain) as _));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        let server = FakeServer::new();
        let launched = server.clone();
        session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        let has_main = |session: &Session| {
            session.state::<Languages>().is_some_and(|languages| languages.synced_text(&main).is_some())
        };
        let mut watch = ToolchainWatch::default();
        assert!(!watch.step(&registry, &mut session), "the first look only remembers");
        sync_until(&registry, &mut session, has_main);
        assert_eq!(server.launches(), 1);
        assert!(!watch.step(&registry, &mut session), "nothing changed");

        toolchain.switch(Path::new("/sdks/b"), None);
        assert!(watch.step(&registry, &mut session));
        assert_eq!(session.state::<Runs>().map(|runs| runs.env().tool.clone()), Some(PathBuf::from("/sdks/b/bin/go")));
        let models = session.state::<ProjectModels>().unwrap();
        assert_eq!(models.env().sdk_root.as_deref(), Some(Path::new("/sdks/b")));
        assert!(!has_main(&session), "the old server is stopped");
        sync_until(&registry, &mut session, has_main);
        assert_eq!(server.launches(), 2);
        assert!(!watch.step(&registry, &mut session));
    }
}
