use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_lsp::fake::FakeServer;

use super::{Languages, Server, ServerSpec, with_languages};
use crate::Workbench;

pub(crate) struct Fixture {
    pub(crate) registry: Registry,
    pub(crate) session: Session,
    pub(crate) server: FakeServer,
    dir: tempfile::TempDir,
}

pub(crate) fn fixture(files: &[(&str, &str)]) -> Fixture {
    fixture_for("go", files)
}

pub(crate) fn fixture_for(language: &str, files: &[(&str, &str)]) -> Fixture {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let server = FakeServer::new();
    let launched = server.clone();
    session.state_mut::<Languages>().set_specs(vec![ServerSpec {
        language_id: language.to_owned(),
        extensions: vec![language.to_owned()],
        launch: Arc::new(move |_| Ok(launched.launcher())),
        initialization_options: json!({}),
    }]);
    Fixture { registry, session, server, dir }
}

impl Fixture {
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub(crate) fn dispatch(&mut self, command: &str, params: Value) -> Result<Value, DispatchError> {
        self.registry.dispatch(&mut self.session, command, params)
    }

    pub(crate) fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.dispatch("file.document.open", json!({ "path": path })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            self.dispatch("language.servers.sync", json!({})).unwrap();
            if self.session.state::<Languages>().is_some_and(|languages| languages.server_version(&path).is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the server did not get {name}");
    }

    pub(crate) fn publish(&mut self, name: &str, diagnostics: Value) {
        let path = self.path(name);
        let params = json!({ "uri": zen_lsp::position::file_uri(&path), "diagnostics": diagnostics });
        self.with(|languages, _| {
            let id = languages.spec_for(&path).expect("no server for the file").id().to_owned();
            let server = languages.servers.remove(&id).expect("the server is not started");
            let Server::Running { files, .. } = &server else { panic!("the server is not running") };
            languages.publish(&params, files);
            languages.servers.insert(id, server);
        });
    }

    pub(crate) fn dispatch_once_running(&mut self, command: &str, params: Value) -> Result<Value, DispatchError> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let result = self.dispatch(command, params.clone());
            let starting = result.as_ref().is_err_and(|error| {
                error.to_string().contains(&zen_i18n::t!("workbench-language-not-running"))
                    || matches!(error, DispatchError::Failed { error, .. } if error.code.as_deref() == Some("starting"))
            });
            if !starting || Instant::now() >= deadline {
                return result;
            }
            self.dispatch("language.servers.sync", json!({})).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub(crate) fn open_files(&self) -> Vec<PathBuf> {
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().map(|document| document.path().to_owned()).collect()
    }

    pub(crate) fn suppresses(&mut self, suppression: zen_lsp::servers::SuppressionDeclaration) {
        self.with(|languages, _| languages.entries[0].declaration.suppression = Some(suppression));
    }

    pub(crate) fn with<T>(&mut self, f: impl FnOnce(&mut Languages, &mut Workbench) -> T) -> T {
        with_languages(&mut self.session, f)
    }
}
