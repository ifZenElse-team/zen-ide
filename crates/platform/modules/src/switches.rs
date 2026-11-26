use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::ModuleDescriptor;

#[derive(Clone, Copy, Debug)]
pub struct BuiltinModule {
    pub descriptor: &'static ModuleDescriptor,
    pub on_by_default: bool,
}

impl BuiltinModule {
    pub const fn on(descriptor: &'static ModuleDescriptor) -> Self {
        Self { descriptor, on_by_default: true }
    }

    pub const fn off(descriptor: &'static ModuleDescriptor) -> Self {
        Self { descriptor, on_by_default: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    enabled: Vec<String>,
    disabled: Vec<String>,
}

pub struct Modules {
    product: String,
    modules: Vec<BuiltinModule>,
    file: Option<PathBuf>,
    saved: Saved,
    active: Vec<&'static str>,
}

pub type SharedModules = Arc<Mutex<Modules>>;

/// One module as Settings → Plugins → Built-in and `plugins.modules.list` show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleEntry {
    /// The module's id, e.g. `go`.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What it gives.
    pub description: String,
    /// Whether it is on from the next start (the user's choice, else the product's default).
    pub enabled: bool,
    /// Whether this run of the IDE has it.
    pub active: bool,
    /// Whether the product has it on by default.
    pub on_by_default: bool,
    /// Whether the IDE must restart for the choice to apply (`enabled` differs from `active`).
    pub restart_required: bool,
}

/// The answer of `plugins.modules.list`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleList {
    /// The product, e.g. `GoGarden`, whose defaults `onByDefault` gives.
    pub product: String,
    /// The modules matching the query, in the product's order.
    pub modules: Vec<ModuleEntry>,
    /// The names of the modules switched on or off since the IDE started; a restart applies them.
    pub restart_required: Vec<String>,
}

impl Modules {
    pub fn load(product: &str, modules: Vec<BuiltinModule>, file: Option<PathBuf>) -> Self {
        let saved = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut this = Self { product: product.to_owned(), modules, file, saved, active: Vec::new() };
        this.active =
            this.modules.iter().filter(|module| this.enabled(module)).map(|module| module.descriptor.id).collect();
        this
    }

    pub fn register_locales(&self) {
        for module in &self.modules {
            zen_i18n::register(module.descriptor.locales);
        }
    }

    pub fn active(&self) -> Vec<&'static ModuleDescriptor> {
        self.modules
            .iter()
            .filter(|module| self.active.contains(&module.descriptor.id))
            .map(|module| module.descriptor)
            .collect()
    }

    fn enabled(&self, module: &BuiltinModule) -> bool {
        let id = module.descriptor.id;
        if module.on_by_default {
            !self.saved.disabled.iter().any(|saved| saved == id)
        } else {
            self.saved.enabled.iter().any(|saved| saved == id)
        }
    }

    fn entry(&self, module: &BuiltinModule) -> ModuleEntry {
        let enabled = self.enabled(module);
        let active = self.active.contains(&module.descriptor.id);
        ModuleEntry {
            id: module.descriptor.id.to_owned(),
            name: t!(module.descriptor.name),
            description: t!(module.descriptor.description),
            enabled,
            active,
            on_by_default: module.on_by_default,
            restart_required: enabled != active,
        }
    }

    pub fn module(&self, id: &str) -> Option<ModuleEntry> {
        self.modules.iter().find(|module| module.descriptor.id == id).map(|module| self.entry(module))
    }

    pub fn list(&self, query: &str) -> ModuleList {
        let query = query.trim().to_lowercase();
        let entries: Vec<ModuleEntry> = self.modules.iter().map(|module| self.entry(module)).collect();
        ModuleList {
            product: self.product.clone(),
            restart_required: entries
                .iter()
                .filter(|entry| entry.restart_required)
                .map(|entry| entry.name.clone())
                .collect(),
            modules: entries
                .into_iter()
                .filter(|entry| {
                    query.is_empty()
                        || [&entry.id, &entry.name, &entry.description]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query))
                })
                .collect(),
        }
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) -> Result<ModuleEntry, String> {
        let module = *self
            .modules
            .iter()
            .find(|module| module.descriptor.id == id)
            .ok_or_else(|| t!("modules-error-unknown", id = id))?;
        self.saved.enabled.retain(|saved| saved != id);
        self.saved.disabled.retain(|saved| saved != id);
        if on && !module.on_by_default {
            self.saved.enabled.push(id.to_owned());
        } else if !on && module.on_by_default {
            self.saved.disabled.push(id.to_owned());
        }
        self.save()?;
        Ok(self.entry(&module))
    }

    fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let text = serde_json::to_vec_pretty(&self.saved).unwrap_or_default();
        std::fs::write(file, text)
            .map_err(|error| t!("modules-error-save", file = file.display().to_string(), error = error.to_string()))
    }
}

fn lock(modules: &SharedModules) -> MutexGuard<'_, Modules> {
    modules.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The parameters of `plugins.modules.list`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListParams {
    /// Only the modules whose id, name or description contains this (any case).
    pub query: String,
}

/// The parameters of `plugins.module.enable` and `plugins.module.disable`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SwitchParams {
    /// The module's id, as `plugins.modules.list` gives it, e.g. `go`.
    pub id: String,
}

pub struct ModulesList(pub SharedModules);

impl Command for ModulesList {
    type Params = ListParams;
    type Output = ModuleList;
    const ID: &'static str = "plugins.modules.list";
    const TITLE: &'static str = "Built-in Plugins";
    const DESCRIPTION: &'static str = "The plug-ins built into every Zen IDE as native modules (Go, Rust...): whether \
         each is on from the next start (enabled), whether this run has it (active), whether this product has it on \
         by default, and which wait for a restart.";

    fn run(&self, _: &mut Session, params: ListParams) -> Result<ModuleList, CommandError> {
        Ok(lock(&self.0).list(&params.query))
    }
}

pub struct Enable(pub SharedModules);

impl Command for Enable {
    type Params = SwitchParams;
    type Output = ModuleEntry;
    const ID: &'static str = "plugins.module.enable";
    const TITLE: &'static str = "Enable Built-in Plugin";
    const DESCRIPTION: &'static str = "Switches a built-in plug-in on (by id, e.g. go) and saves the choice; it takes \
         effect when the IDE restarts (restartRequired says so).";

    fn run(&self, _: &mut Session, params: SwitchParams) -> Result<ModuleEntry, CommandError> {
        lock(&self.0).set_enabled(&params.id, true).map_err(CommandError::new)
    }
}

pub struct Disable(pub SharedModules);

impl Command for Disable {
    type Params = SwitchParams;
    type Output = ModuleEntry;
    const ID: &'static str = "plugins.module.disable";
    const TITLE: &'static str = "Disable Built-in Plugin";
    const DESCRIPTION: &'static str = "Switches a built-in plug-in off (by id, e.g. go) and saves the choice; it is \
         removed when the IDE restarts (restartRequired says so).";

    fn run(&self, _: &mut Session, params: SwitchParams) -> Result<ModuleEntry, CommandError> {
        lock(&self.0).set_enabled(&params.id, false).map_err(CommandError::new)
    }
}

pub fn register(registry: &mut Registry, modules: &SharedModules) -> Result<(), RegistryError> {
    registry.register(ModulesList(Arc::clone(modules)))?;
    registry.register(Enable(Arc::clone(modules)))?;
    registry.register(Disable(Arc::clone(modules)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ModuleHost;

    const GO_EN: &str = "module-go-name = Go\nmodule-go-description = Go support.\n";
    const RUST_EN: &str = "module-rust-name = Rust\nmodule-rust-description = Rust support.\n";

    fn go(host: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
        host.notification_group("toolchain.dlv.", "debugger");
        Ok(())
    }

    fn nothing(_: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
        Ok(())
    }

    static GO: ModuleDescriptor = ModuleDescriptor {
        id: "go",
        name: "module-go-name",
        description: "module-go-description",
        locales: &[("en", GO_EN)],
        register: go,
    };
    static RUST: ModuleDescriptor = ModuleDescriptor {
        id: "rust",
        name: "module-rust-name",
        description: "module-rust-description",
        locales: &[("en", RUST_EN)],
        register: nothing,
    };

    fn modules(file: Option<PathBuf>) -> Modules {
        let modules = Modules::load("RustRonin", vec![BuiltinModule::off(&GO), BuiltinModule::on(&RUST)], file);
        modules.register_locales();
        zen_i18n::register(crate::LOCALES);
        modules
    }

    #[test]
    fn the_product_decides_until_the_user_chooses_and_a_restart_applies_it() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("modules.json");
        let mut first = modules(Some(file.clone()));
        let ids: Vec<_> = first.active().iter().map(|module| module.id).collect();
        assert_eq!(ids, ["rust"], "only what the product has on runs");
        let list = first.list("");
        assert_eq!(list.product, "RustRonin");
        assert_eq!(list.modules[0].name, "Go");
        assert!(!list.modules[0].enabled && !list.modules[0].on_by_default && !list.modules[0].restart_required);
        assert!(list.restart_required.is_empty());

        let go = first.set_enabled("go", true).unwrap();
        assert!(go.enabled && !go.active && go.restart_required, "{go:?}");
        assert_eq!(first.list("").restart_required, ["Go"]);
        first.set_enabled("rust", false).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved, json!({ "enabled": ["go"], "disabled": ["rust"] }));

        let restarted = modules(Some(file.clone()));
        let ids: Vec<_> = restarted.active().iter().map(|module| module.id).collect();
        assert_eq!(ids, ["go"], "after a restart the choice applies");
        assert!(restarted.list("").restart_required.is_empty());

        let mut back = modules(Some(file.clone()));
        back.set_enabled("go", false).unwrap();
        back.set_enabled("rust", true).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved, json!({ "enabled": [], "disabled": [] }), "the defaults are not written down");
    }

    #[test]
    fn the_commands_list_search_and_switch() {
        let data = tempfile::tempdir().unwrap();
        let shared: SharedModules = Arc::new(Mutex::new(modules(Some(data.path().join("modules.json")))));
        let mut registry = Registry::new();
        register(&mut registry, &shared).unwrap();
        let mut session = Session::new();
        let listed = registry.dispatch(&mut session, "plugins.modules.list", json!({ "query": "rust supp" })).unwrap();
        assert_eq!(listed["modules"].as_array().unwrap().len(), 1);
        assert_eq!(listed["modules"][0]["id"], "rust");
        assert_eq!(listed["modules"][0]["onByDefault"], true);
        let enabled = registry.dispatch(&mut session, "plugins.module.enable", json!({ "id": "go" })).unwrap();
        assert_eq!(enabled["restartRequired"], true);
        let all = registry.dispatch(&mut session, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(all["restartRequired"], json!(["Go"]));
        let error = registry.dispatch(&mut session, "plugins.module.disable", json!({ "id": "cobol" })).unwrap_err();
        assert!(error.to_string().contains("cobol"), "{error}");
    }

    #[test]
    fn the_host_collects_what_a_module_gives() {
        let mut registry = Registry::new();
        let mut host = ModuleHost::new(&mut registry, None);
        host.run(&GO).unwrap();
        let given = host.finish();
        assert_eq!(given.notification_groups, [("toolchain.dlv.", "debugger")]);
        assert!(given.toolchain.is_none() && given.highlighters.is_empty());
    }
}
