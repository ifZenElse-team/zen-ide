use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::SharedModules;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AreaPlugin {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub restart_required: bool,
}

pub trait AreaPlugins: Send + Sync {
    fn plugin(&self, id: &str) -> Option<AreaPlugin>;
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String>;
}

/// What an area is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AreaKind {
    /// A built-in module (Settings → Plugins → Built-in); its change applies at a restart.
    Module,
    /// A WebAssembly plug-in bundled with the IDE (Settings → Plugins → Installed); its change
    /// applies at once.
    Plugin,
}

/// One area as the choice and `plugins.areas.list` show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AreaEntry {
    /// The plug-in's id, e.g. `api-tests`.
    pub id: String,
    /// A built-in module or a bundled plug-in.
    pub kind: AreaKind,
    /// Its name.
    pub name: String,
    /// What it gives, in a sentence.
    pub description: String,
    /// Whether it is on (for a module: from the next start).
    pub enabled: bool,
    /// Whether the IDE must restart for the choice to apply.
    pub restart_required: bool,
}

/// The answer of the `plugins.areas.*` commands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AreaList {
    /// The product, e.g. `TestTotami`.
    pub product: String,
    /// Whether the first-start choice was made or skipped; until then the IDE offers it.
    pub offered: bool,
    /// Whether the choice dialog is open now.
    pub dialog_open: bool,
    /// The product's areas, in its order; empty when its bundle has none.
    pub areas: Vec<AreaEntry>,
    /// The names of the areas whose change waits for a restart.
    pub restart_required: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    offered: bool,
}

pub struct Areas {
    product: String,
    ids: Vec<String>,
    modules: SharedModules,
    plugins: Option<Arc<dyn AreaPlugins>>,
    file: Option<PathBuf>,
    saved: Saved,
    generation: u64,
    show: bool,
    open: bool,
}

pub type SharedAreas = Arc<Mutex<Areas>>;

impl Areas {
    pub fn load(
        product: &str,
        ids: Vec<String>,
        modules: SharedModules,
        plugins: Option<Arc<dyn AreaPlugins>>,
        file: Option<PathBuf>,
    ) -> Self {
        let saved = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            product: product.to_owned(),
            ids,
            modules,
            plugins,
            file,
            saved,
            generation: 0,
            show: false,
            open: false,
        }
    }

    fn modules(&self) -> MutexGuard<'_, crate::Modules> {
        self.modules.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn entry(&self, id: &str) -> Option<AreaEntry> {
        if let Some(module) = self.modules().module(id) {
            return Some(AreaEntry {
                id: module.id,
                kind: AreaKind::Module,
                name: module.name,
                description: module.description,
                enabled: module.enabled,
                restart_required: module.restart_required,
            });
        }
        let plugin = self.plugins.as_ref()?.plugin(id)?;
        Some(AreaEntry {
            id: id.to_owned(),
            kind: AreaKind::Plugin,
            name: plugin.name,
            description: plugin.description,
            enabled: plugin.enabled,
            restart_required: plugin.restart_required,
        })
    }

    pub fn list(&self) -> AreaList {
        let areas: Vec<AreaEntry> = self.ids.iter().filter_map(|id| self.entry(id)).collect();
        AreaList {
            product: self.product.clone(),
            offered: self.saved.offered,
            dialog_open: self.open,
            restart_required: areas.iter().filter(|area| area.restart_required).map(|area| area.name.clone()).collect(),
            areas,
        }
    }

    pub fn should_offer(&self) -> bool {
        !self.saved.offered && !self.list().areas.is_empty()
    }

    pub fn choose(&mut self, chosen: &[String]) -> Result<AreaList, String> {
        let areas = self.list().areas;
        if let Some(unknown) = chosen.iter().find(|id| !areas.iter().any(|area| &area.id == *id)) {
            let ids: Vec<&str> = areas.iter().map(|area| area.id.as_str()).collect();
            return Err(if ids.is_empty() {
                t!("areas-error-none", product = self.product.as_str())
            } else {
                t!("areas-error-unknown", id = unknown.as_str(), ids = ids.join(", "))
            });
        }
        for area in &areas {
            let on = chosen.contains(&area.id);
            if area.enabled == on {
                continue;
            }
            match area.kind {
                AreaKind::Module => self.modules().set_enabled(&area.id, on).map(|_| ())?,
                AreaKind::Plugin => match &self.plugins {
                    Some(plugins) => plugins.set_enabled(&area.id, on)?,
                    None => continue,
                },
            }
        }
        self.mark_offered()?;
        Ok(self.list())
    }

    pub fn skip(&mut self) -> Result<AreaList, String> {
        self.mark_offered()?;
        Ok(self.list())
    }

    fn mark_offered(&mut self) -> Result<(), String> {
        self.saved.offered = true;
        self.generation += 1;
        self.open = false;
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let text = serde_json::to_vec_pretty(&self.saved).unwrap_or_default();
        std::fs::write(file, text)
            .map_err(|error| t!("modules-error-save", file = file.display().to_string(), error = error.to_string()))
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn request_show(&mut self) -> Result<AreaList, String> {
        if self.list().areas.is_empty() {
            return Err(t!("areas-error-none", product = self.product.as_str()));
        }
        self.show = true;
        Ok(self.list())
    }

    pub fn take_show(&mut self) -> bool {
        std::mem::take(&mut self.show)
    }

    pub fn lock(shared: &SharedAreas) -> MutexGuard<'_, Areas> {
        shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }
}

fn lock(areas: &SharedAreas) -> MutexGuard<'_, Areas> {
    Areas::lock(areas)
}

/// The parameters of `plugins.areas.choose`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChooseParams {
    /// The ids of the areas to switch on, as `plugins.areas.list` gives them; every other area is
    /// switched off. Empty switches every area off.
    pub areas: Vec<String>,
}

pub struct AreasList(pub SharedAreas);

impl Command for AreasList {
    type Params = NoParams;
    type Output = AreaList;
    const ID: &'static str = "plugins.areas.list";
    const TITLE: &'static str = "Areas of Work";
    const DESCRIPTION: &'static str = "The optional plug-ins of this product's bundle (areas): plug-ins it ships but \
         keeps off until the user chooses the areas they work in, each a built-in module or a bundled plug-in, with \
         whether it is on, whether the first-start choice was made (offered), and which wait for a restart. Empty \
         when the product has no areas.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<AreaList, CommandError> {
        Ok(lock(&self.0).list())
    }
}

pub struct AreasChoose(pub SharedAreas);

impl Command for AreasChoose {
    type Params = ChooseParams;
    type Output = AreaList;
    const ID: &'static str = "plugins.areas.choose";
    const TITLE: &'static str = "Choose Areas of Work";
    const DESCRIPTION: &'static str = "Switches on exactly the given areas (ids from plugins.areas.list) and off the \
         other areas of this product, and records that the first-start choice was made. A bundled plug-in changes at \
         once; a built-in module at the next start (restartRequired says so; view.ide.restart restarts).";

    fn run(&self, _: &mut Session, params: ChooseParams) -> Result<AreaList, CommandError> {
        lock(&self.0).choose(&params.areas).map_err(CommandError::new)
    }
}

pub struct AreasSkip(pub SharedAreas);

impl Command for AreasSkip {
    type Params = NoParams;
    type Output = AreaList;
    const ID: &'static str = "plugins.areas.skip";
    const TITLE: &'static str = "Skip Choosing Areas";
    const DESCRIPTION: &'static str = "Leaves every area as it is (off, on a first start) and records that the \
         first-start choice was offered, as the dialog's Skip does; the choice stays in Settings → Plugins → Built-in.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<AreaList, CommandError> {
        lock(&self.0).skip().map_err(CommandError::new)
    }
}

pub struct AreasShow(pub SharedAreas);

impl Command for AreasShow {
    type Params = NoParams;
    type Output = AreaList;
    const ID: &'static str = "plugins.areas.show";
    const TITLE: &'static str = "Choose Areas…";
    const DESCRIPTION: &'static str = "Shows the dialog that offers this product's areas (the one of the first start) \
         over the main window; fails when the product has none.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<AreaList, CommandError> {
        lock(&self.0).request_show().map_err(CommandError::new)
    }
}

pub fn register(registry: &mut Registry, areas: &SharedAreas) -> Result<(), RegistryError> {
    registry.register(AreasList(Arc::clone(areas)))?;
    registry.register(AreasChoose(Arc::clone(areas)))?;
    registry.register(AreasSkip(Arc::clone(areas)))?;
    registry.register(AreasShow(Arc::clone(areas)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::{BuiltinModule, ModuleDescriptor, ModuleHost, Modules};

    const QA_EN: &str = "module-qa-name = Load Testing\nmodule-qa-description = Runs k6 scripts.\n";

    fn nothing(_: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
        Ok(())
    }

    static LOAD: ModuleDescriptor = ModuleDescriptor {
        id: "load-tests",
        name: "module-qa-name",
        description: "module-qa-description",
        locales: &[("en", QA_EN)],
        register: nothing,
    };

    #[derive(Default)]
    struct Bundle(Mutex<BTreeMap<String, (String, bool)>>);

    impl AreaPlugins for Bundle {
        fn plugin(&self, id: &str) -> Option<AreaPlugin> {
            let plugins = self.0.lock().unwrap();
            let (name, enabled) = plugins.get(id)?.clone();
            Some(AreaPlugin { name, description: format!("{id} plug-in"), enabled, restart_required: false })
        }

        fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
            let mut plugins = self.0.lock().unwrap();
            let plugin = plugins.get_mut(id).ok_or("not installed")?;
            plugin.1 = enabled;
            Ok(())
        }
    }

    fn test_totami(data: &std::path::Path) -> (SharedAreas, Arc<Bundle>) {
        zen_i18n::register(crate::LOCALES);
        let modules = Modules::load("TestTotami", vec![BuiltinModule::off(&LOAD)], Some(data.join("modules.json")));
        modules.register_locales();
        let bundle = Arc::new(Bundle::default());
        for (id, name) in [("api-tests", "API Test Collections"), ("e2e-tests", "E2E and UI Tests")] {
            bundle.0.lock().unwrap().insert(id.to_owned(), (name.to_owned(), false));
        }
        let ids = ["e2e-tests", "api-tests", "load-tests", "test-management"].map(str::to_owned).to_vec();
        let areas = Areas::load(
            "TestTotami",
            ids,
            Arc::new(Mutex::new(modules)),
            Some(Arc::clone(&bundle) as Arc<dyn AreaPlugins>),
            Some(data.join("areas.json")),
        );
        (Arc::new(Mutex::new(areas)), bundle)
    }

    #[test]
    fn the_first_start_offers_the_areas_and_the_choice_switches_them() {
        let data = tempfile::tempdir().unwrap();
        let (areas, bundle) = test_totami(data.path());
        let list = lock(&areas).list();
        let ids: Vec<_> = list.areas.iter().map(|area| area.id.as_str()).collect();
        assert_eq!(ids, ["e2e-tests", "api-tests", "load-tests"], "an area the IDE lacks is left out");
        assert_eq!(list.areas[2].kind, AreaKind::Module);
        assert_eq!(list.areas[2].name, "Load Testing");
        assert!(list.areas.iter().all(|area| !area.enabled), "areas stay off until chosen");
        assert!(lock(&areas).should_offer());

        let chosen = lock(&areas).choose(&["api-tests".to_owned(), "load-tests".to_owned()]).unwrap();
        assert!(chosen.offered);
        let on: Vec<_> = chosen.areas.iter().filter(|area| area.enabled).map(|area| area.id.as_str()).collect();
        assert_eq!(on, ["api-tests", "load-tests"]);
        assert_eq!(chosen.restart_required, ["Load Testing"], "a module applies at a restart");
        assert!(bundle.0.lock().unwrap()["api-tests"].1, "a bundled plug-in is switched at once");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(data.path().join("modules.json")).unwrap()).unwrap();
        assert_eq!(saved["enabled"], json!(["load-tests"]));

        let (again, _) = test_totami(data.path());
        assert!(!lock(&again).should_offer());
        let list = lock(&again).list();
        assert!(list.areas.iter().find(|area| area.id == "load-tests").unwrap().enabled);
        assert!(list.restart_required.is_empty(), "the restart applied it");

        let changed = lock(&again).choose(&["e2e-tests".to_owned()]).unwrap();
        let on: Vec<_> = changed.areas.iter().filter(|area| area.enabled).map(|area| area.id.as_str()).collect();
        assert_eq!(on, ["e2e-tests"]);
    }

    #[test]
    fn skip_keeps_everything_off_and_a_product_without_areas_offers_nothing() {
        let data = tempfile::tempdir().unwrap();
        let (areas, _) = test_totami(data.path());
        let skipped = lock(&areas).skip().unwrap();
        assert!(skipped.offered && skipped.areas.iter().all(|area| !area.enabled));
        assert!(!lock(&areas).should_offer());

        let modules = Modules::load("GoGarden", Vec::new(), None);
        let none = Areas::load("GoGarden", Vec::new(), Arc::new(Mutex::new(modules)), None, None);
        assert!(!none.should_offer());
        assert!(none.list().areas.is_empty());
    }

    #[test]
    fn the_commands_list_choose_skip_and_show() {
        let data = tempfile::tempdir().unwrap();
        let (areas, _) = test_totami(data.path());
        let mut registry = Registry::new();
        register(&mut registry, &areas).unwrap();
        let mut session = Session::new();
        let listed = registry.dispatch(&mut session, "plugins.areas.list", json!({})).unwrap();
        assert_eq!(listed["offered"], false);
        assert_eq!(listed["areas"][0]["kind"], "plugin");
        let shown = registry.dispatch(&mut session, "plugins.areas.show", json!({})).unwrap();
        assert_eq!(shown["areas"].as_array().unwrap().len(), 3);
        assert!(lock(&areas).take_show() && !lock(&areas).take_show());
        let before = lock(&areas).generation();
        let chosen =
            registry.dispatch(&mut session, "plugins.areas.choose", json!({ "areas": ["e2e-tests"] })).unwrap();
        assert_eq!(chosen["offered"], true);
        assert_eq!(chosen["areas"][0]["enabled"], true);
        assert!(lock(&areas).generation() > before, "an open dialog sees the choice");
        let error = registry.dispatch(&mut session, "plugins.areas.choose", json!({ "areas": ["cobol"] })).unwrap_err();
        assert!(error.to_string().contains("cobol") && error.to_string().contains("api-tests"), "{error}");
        registry.dispatch(&mut session, "plugins.areas.skip", json!({})).unwrap();

        let modules = Modules::load("GoGarden", Vec::new(), None);
        let none: SharedAreas =
            Arc::new(Mutex::new(Areas::load("GoGarden", Vec::new(), Arc::new(Mutex::new(modules)), None, None)));
        let mut registry = Registry::new();
        register(&mut registry, &none).unwrap();
        let error = registry.dispatch(&mut session, "plugins.areas.show", json!({})).unwrap_err();
        assert!(error.to_string().contains("GoGarden"), "{error}");
    }
}
