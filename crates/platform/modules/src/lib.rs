mod areas;
pub mod assist;
pub mod file_references;
pub mod folding;
pub mod form;
pub mod generate;
pub mod search;
mod switches;
mod toolchains;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use areas::{
    AreaEntry, AreaKind, AreaList, AreaPlugin, AreaPlugins, Areas, AreasChoose, AreasList, AreasShow, AreasSkip,
    ChooseParams, SharedAreas, register as register_areas,
};
pub use switches::{
    BuiltinModule, Disable, Enable, ListParams, ModuleEntry, ModuleList, Modules, ModulesList, SharedModules,
    SwitchParams, register,
};
pub use toolchains::Toolchains;
use zen_core::{Registry, RegistryError, Session};

pub const LOCALES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en/modules.ftl")), ("ru", include_str!("../locales/ru/modules.ftl"))];

pub type AddImport = fn(&str, &str) -> Option<(Range<usize>, String)>;

pub type WindowState = Arc<dyn Fn(&mut Session) + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub struct ModuleDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub locales: &'static [(&'static str, &'static str)],
    pub register: fn(&mut ModuleHost<'_>) -> Result<(), RegistryError>,
}

pub struct ModuleHost<'a> {
    registry: &'a mut Registry,
    data_dir: Option<PathBuf>,
    contributions: Contributions,
    toolchains: Vec<Arc<dyn LanguageToolchain>>,
}

impl<'a> ModuleHost<'a> {
    pub fn new(registry: &'a mut Registry, data_dir: Option<&Path>) -> Self {
        Self {
            registry,
            data_dir: data_dir.map(Path::to_owned),
            contributions: Contributions::default(),
            toolchains: Vec::new(),
        }
    }

    pub fn run(&mut self, module: &ModuleDescriptor) -> Result<(), RegistryError> {
        (module.register)(self)
    }

    pub fn finish(self) -> Contributions {
        self.contributions
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    pub fn commands(&mut self) -> &mut Registry {
        self.registry
    }

    pub fn highlighter(&mut self, highlighter: Arc<dyn zen_syntax::Highlighter>) {
        self.contributions.highlighters.push(highlighter);
    }

    pub fn outliner(&mut self, outliner: Arc<dyn zen_syntax::Outliner>) {
        self.contributions.outliners.push(outliner);
    }

    pub fn project_model(&mut self, provider: Arc<dyn zen_project::model::ModelProvider>) {
        self.contributions.project_models.push(provider);
    }

    pub fn run_provider(&mut self, provider: Arc<dyn zen_run::RunProvider>) {
        self.contributions.run_providers.push(provider);
    }

    pub fn test_support(&mut self, support: Arc<dyn zen_run::test_support::TestSupport>) {
        self.contributions.window_states.push(Arc::new(move |session: &mut Session| {
            session.state_mut::<zen_run::test_support::TestSupports>().add(Arc::clone(&support));
        }));
    }

    pub fn inspector(&mut self, inspector: Arc<dyn zen_project::inspect::Inspector>) {
        self.contributions.inspectors.push(inspector);
    }

    pub fn language_servers(&mut self, servers: impl IntoIterator<Item = zen_lsp::ServerDeclaration>) {
        self.contributions.language_servers.extend(servers);
    }

    pub fn templates(&mut self, templates: TemplateSet) {
        self.contributions.templates.push(templates);
    }

    pub fn file_templates(&mut self, templates: FileTemplateSet) {
        self.contributions.file_templates.push(templates);
    }

    pub fn toolchain(&mut self, toolchain: Arc<dyn LanguageToolchain>) {
        self.toolchains.push(toolchain);
        self.contributions.toolchain = Toolchains::of(self.toolchains.clone());
    }

    pub fn debugger(&mut self, offer: DebuggerOffer) {
        if self.contributions.debugger.is_none() {
            self.contributions.debugger = Some(offer);
        }
    }

    pub fn window_state(&mut self, state: WindowState) {
        self.contributions.window_states.push(state);
    }

    pub fn settings_page(&mut self, page: SettingsPage) {
        self.contributions.settings_pages.push(page);
    }

    pub fn file_type(&mut self, file_type: FileTypeDeclaration) {
        self.contributions.file_types.push(file_type);
    }

    pub fn excluded_folder(&mut self, folder: ExcludedFolderDeclaration) {
        self.contributions.excluded_folders.push(folder);
    }

    pub fn code_style(&mut self, page: CodeStylePage) {
        self.contributions.code_styles.push(page);
    }

    pub fn code_style_options(&mut self, declaration: zen_project::code_style::CodeStyleDeclaration) {
        self.contributions.code_style_declarations.push(declaration);
    }

    pub fn arranger(&mut self, arranger: Arc<dyn zen_syntax::Arranger>) {
        self.contributions.arrangers.push(arranger);
    }

    pub fn folding_options(&mut self, options: folding::FoldingOptions) {
        self.contributions.folding_options.push(options);
    }

    pub fn search_contributor(&mut self, contributor: Arc<dyn search::SearchContributor>) {
        self.contributions.search_contributors.push(contributor);
    }

    pub fn code_generator(&mut self, generator: Arc<dyn generate::CodeGenerator>) {
        self.contributions.code_generators.push(generator);
    }

    pub fn file_references(&mut self, references: Arc<dyn file_references::FileReferences>) {
        self.contributions.file_references.push(references);
    }

    pub fn file_watcher_templates(&mut self, templates: FileWatcherTemplates) {
        self.contributions.file_watcher_templates.push(templates);
    }

    pub fn project_generator(&mut self, generator: Arc<dyn zen_project::generators::ProjectGenerator>) {
        self.contributions.project_generators.push(generator);
    }

    pub fn language_assist(&mut self, assist: Arc<dyn assist::LanguageAssist>) {
        self.contributions.language_assists.push(assist);
    }

    pub fn notification_group(&mut self, prefix: &'static str, group: &'static str) {
        self.contributions.notification_groups.push((prefix, group));
    }
}

#[derive(Default)]
pub struct Contributions {
    pub highlighters: Vec<Arc<dyn zen_syntax::Highlighter>>,
    pub outliners: Vec<Arc<dyn zen_syntax::Outliner>>,
    pub project_models: Vec<Arc<dyn zen_project::model::ModelProvider>>,
    pub run_providers: Vec<Arc<dyn zen_run::RunProvider>>,
    pub inspectors: Vec<Arc<dyn zen_project::inspect::Inspector>>,
    pub language_servers: Vec<zen_lsp::ServerDeclaration>,
    pub templates: Vec<TemplateSet>,
    pub file_templates: Vec<FileTemplateSet>,
    pub toolchain: Option<Arc<dyn LanguageToolchain>>,
    pub debugger: Option<DebuggerOffer>,
    pub window_states: Vec<WindowState>,
    pub settings_pages: Vec<SettingsPage>,
    pub code_styles: Vec<CodeStylePage>,
    pub code_style_declarations: Vec<zen_project::code_style::CodeStyleDeclaration>,
    pub arrangers: Vec<Arc<dyn zen_syntax::Arranger>>,
    pub folding_options: Vec<folding::FoldingOptions>,
    pub file_types: Vec<FileTypeDeclaration>,
    pub excluded_folders: Vec<ExcludedFolderDeclaration>,
    pub notification_groups: Vec<(&'static str, &'static str)>,
    pub search_contributors: Vec<Arc<dyn search::SearchContributor>>,
    pub code_generators: Vec<Arc<dyn generate::CodeGenerator>>,
    pub file_references: Vec<Arc<dyn file_references::FileReferences>>,
    pub file_watcher_templates: Vec<FileWatcherTemplates>,
    pub project_generators: Vec<Arc<dyn zen_project::generators::ProjectGenerator>>,
    pub language_assists: Vec<Arc<dyn assist::LanguageAssist>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileWatcherTemplates {
    pub origin: &'static str,
    pub templates: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileTypeDeclaration {
    pub id: &'static str,
    pub name: &'static str,
    pub patterns: &'static [&'static str],
    pub hashbangs: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludedFolderDeclaration {
    pub name: &'static str,
    pub beside: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct TemplateSet {
    pub language: &'static str,
    pub templates: &'static str,
    pub add_import: Option<AddImport>,
    pub surrounders: &'static str,
    pub postfix: &'static str,
    pub doc_comments: &'static str,
    pub statements: &'static str,
}

pub type TemplateValues = fn(&Path) -> Vec<(String, String)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateVariable {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct FileTemplateSet {
    pub origin: &'static str,
    pub templates: &'static str,
    pub variables: &'static [TemplateVariable],
    pub values: Option<TemplateValues>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsPage {
    pub id: &'static str,
    pub title: &'static str,
    pub search: &'static [&'static str],
    pub form: &'static str,
    pub commands: &'static [&'static str],
    pub parent: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeStylePage {
    pub language: &'static str,
    pub title: &'static str,
}

pub trait LanguageToolchain: zen_lsp::ServerTools {
    fn generation(&self) -> u64;

    fn model_env(&self) -> zen_project::model::ModelEnv;

    fn run_env(&self) -> zen_run::RunEnv;

    fn debugger(&self) -> Option<PathBuf>;

    fn languages(&self) -> Vec<String>;

    fn serves(&self, server: &zen_lsp::ServerDeclaration) -> bool {
        let languages = self.languages();
        server.languages.iter().any(|language| languages.contains(language))
    }

    fn generations(&self) -> Vec<(u64, Vec<String>)> {
        vec![(self.generation(), self.languages())]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebuggerOffer {
    pub title: &'static str,
    pub detail: &'static str,
    pub install: &'static str,
    pub installing: &'static str,
    pub command: &'static str,
}

#[derive(Clone)]
pub struct WindowToolchain(pub Arc<dyn LanguageToolchain>);
