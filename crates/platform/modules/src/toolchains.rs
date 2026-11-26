use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use zen_lsp::{NotInstallable, ServerDeclaration, ServerTools};

use crate::LanguageToolchain;

pub struct Toolchains(Vec<Arc<dyn LanguageToolchain>>);

impl Toolchains {
    pub fn of(mut toolchains: Vec<Arc<dyn LanguageToolchain>>) -> Option<Arc<dyn LanguageToolchain>> {
        match toolchains.len() {
            0 => None,
            1 => toolchains.pop(),
            _ => Some(Arc::new(Self(toolchains))),
        }
    }

    fn owner(&self, server: &ServerDeclaration) -> Option<&Arc<dyn LanguageToolchain>> {
        self.0.iter().find(|toolchain| toolchain.serves(server))
    }
}

impl ServerTools for Toolchains {
    fn locate(&self, server: &ServerDeclaration) -> Option<PathBuf> {
        match self.owner(server) {
            Some(toolchain) => toolchain.locate(server),
            None => zen_lsp::SystemTools.locate(server),
        }
    }

    fn environment(&self, server: &ServerDeclaration) -> Vec<(OsString, OsString)> {
        match self.owner(server) {
            Some(toolchain) => toolchain.environment(server),
            None => zen_lsp::SystemTools.environment(server),
        }
    }

    fn install_command(&self, server: &ServerDeclaration) -> Result<std::process::Command, NotInstallable> {
        match self.owner(server) {
            Some(toolchain) => toolchain.install_command(server),
            None => zen_lsp::SystemTools.install_command(server),
        }
    }

    fn install_command_into(
        &self,
        server: &ServerDeclaration,
        prefix: &std::path::Path,
    ) -> Result<std::process::Command, NotInstallable> {
        match self.owner(server) {
            Some(toolchain) => toolchain.install_command_into(server, prefix),
            None => zen_lsp::SystemTools.install_command_into(server, prefix),
        }
    }

    fn refresh(&self) {
        self.0.iter().for_each(|toolchain| toolchain.refresh());
    }

    fn initialization_options(&self, server: &ServerDeclaration, root: &std::path::Path) -> serde_json::Value {
        match self.owner(server) {
            Some(toolchain) => toolchain.initialization_options(server, root),
            None => zen_lsp::SystemTools.initialization_options(server, root),
        }
    }

    fn project_environment(&self, root: &std::path::Path) -> Vec<(String, String)> {
        self.0.iter().flat_map(|toolchain| toolchain.project_environment(root)).collect()
    }
}

impl LanguageToolchain for Toolchains {
    fn generation(&self) -> u64 {
        self.0.iter().fold(0u64, |sum, toolchain| sum.wrapping_add(toolchain.generation()))
    }

    fn generations(&self) -> Vec<(u64, Vec<String>)> {
        self.0.iter().flat_map(|toolchain| toolchain.generations()).collect()
    }

    fn model_env(&self) -> zen_project::model::ModelEnv {
        let default = zen_project::model::ModelEnv::default();
        self.0.iter().map(|toolchain| toolchain.model_env()).find(|env| *env != default).unwrap_or(default)
    }

    fn run_env(&self) -> zen_run::RunEnv {
        let default = zen_run::RunEnv::default();
        self.0.iter().map(|toolchain| toolchain.run_env()).find(|env| *env != default).unwrap_or(default)
    }

    fn debugger(&self) -> Option<PathBuf> {
        self.0.iter().find_map(|toolchain| toolchain.debugger())
    }

    fn languages(&self) -> Vec<String> {
        let mut languages: Vec<String> = Vec::new();
        for language in self.0.iter().flat_map(|toolchain| toolchain.languages()) {
            if !languages.contains(&language) {
                languages.push(language);
            }
        }
        languages
    }

    fn serves(&self, server: &ServerDeclaration) -> bool {
        self.owner(server).is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct Fake {
        language: &'static str,
        generation: AtomicU64,
        tool: Option<&'static str>,
    }

    impl ServerTools for Fake {
        fn locate(&self, _server: &ServerDeclaration) -> Option<PathBuf> {
            Some(PathBuf::from(format!("/{}/server", self.language)))
        }
    }

    impl LanguageToolchain for Fake {
        fn generation(&self) -> u64 {
            self.generation.load(Ordering::SeqCst)
        }

        fn model_env(&self) -> zen_project::model::ModelEnv {
            zen_project::model::ModelEnv {
                sdk_root: self.tool.map(PathBuf::from),
                ..zen_project::model::ModelEnv::default()
            }
        }

        fn run_env(&self) -> zen_run::RunEnv {
            zen_run::RunEnv { tool: self.tool.map(PathBuf::from).unwrap_or_default(), env: Vec::new() }
        }

        fn debugger(&self) -> Option<PathBuf> {
            None
        }

        fn languages(&self) -> Vec<String> {
            vec![self.language.to_owned()]
        }
    }

    fn server(language: &str) -> ServerDeclaration {
        ServerDeclaration::parse_list(&format!(
            r#"[{{ "id": "{language}-ls", "name": "{language}-ls", "languages": ["{language}"], "command": "{language}-ls" }}]"#
        ))
        .unwrap()
        .remove(0)
    }

    #[test]
    fn each_server_goes_to_its_languages_toolchain() {
        let rust = Arc::new(Fake { language: "rust", generation: AtomicU64::new(0), tool: None });
        let go = Arc::new(Fake { language: "go", generation: AtomicU64::new(5), tool: Some("/go/bin/go") });
        assert!(Toolchains::of(Vec::new()).is_none());
        let both = Toolchains::of(vec![rust.clone(), go.clone()]).unwrap();
        assert_eq!(both.locate(&server("rust")), Some(PathBuf::from("/rust/server")));
        assert_eq!(both.locate(&server("go")), Some(PathBuf::from("/go/server")));
        assert!(!both.serves(&server("yaml")));
        assert_eq!(both.languages(), ["rust", "go"]);
        assert_eq!(both.run_env().tool, PathBuf::from("/go/bin/go"));
        assert_eq!(both.model_env().sdk_root, Some(PathBuf::from("/go/bin/go")));
        let before = both.generations();
        go.generation.store(6, Ordering::SeqCst);
        let after = both.generations();
        let changed: Vec<&Vec<String>> =
            before.iter().zip(&after).filter(|(old, new)| old.0 != new.0).map(|(_, new)| &new.1).collect();
        assert_eq!(changed, [&vec!["go".to_owned()]]);
        assert_eq!(both.generation(), 6);
    }
}
