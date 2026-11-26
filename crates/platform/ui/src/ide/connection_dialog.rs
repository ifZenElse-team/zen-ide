use egui::{Align, Key, Layout, RichText, Role, Sense, Ui, vec2};
use serde_json::{Map, Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.dataSource.open";

const OPEN_FILE: &str = "db.databaseFile.open";

const WIDTH: f32 = 820.0;
const DRIVERS_WIDTH: f32 = 200.0;
const ROW_HEIGHT: f32 = 24.0;
const PAGE_HEIGHT: f32 = 380.0;
const LABEL_WIDTH: f32 = 150.0;
const KEEP_ALIVE: &str = "keepAlive";
const SSL_MODE: &str = "sslmode";
const TLS_MODES: [&str; 5] = ["disable", "prefer", "require", "verify-ca", "verify-full"];
const VIEWS: [&str; 3] = ["simple", "advanced", "custom"];

#[derive(Default)]
pub(super) struct ConnectionDialogRequest(pub(super) Option<Value>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Main,
    SshSsl,
    Advanced,
    General,
}

impl Page {
    const ALL: [Page; 4] = [Page::Main, Page::SshSsl, Page::Advanced, Page::General];

    fn title(self) -> String {
        match self {
            Page::Main => t!("ide-dbc-page-main"),
            Page::SshSsl => t!("ide-dbc-page-ssh"),
            Page::Advanced => t!("ide-dbc-page-advanced"),
            Page::General => t!("ide-dbc-page-general"),
        }
    }

    fn of(name: &str) -> Option<Page> {
        match name {
            "main" => Some(Page::Main),
            "ssh" | "sshSsl" | "ssl" => Some(Page::SshSsl),
            "advanced" => Some(Page::Advanced),
            "general" => Some(Page::General),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FileAction {
    Open,
    Create,
    Setting(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Test {
    Running(u64),
    Passed(String),
    Failed(String),
}

#[derive(Clone, Debug)]
struct Profile {
    id: String,
    name: String,
    driver: String,
    family: String,
    port: Option<u16>,
    database: Option<String>,
    user: Option<String>,
    tls: String,
    settings: Vec<String>,
    capabilities: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Family(usize),
    Driver(usize),
    Profile(usize),
}

#[derive(Clone, Debug)]
struct Driver {
    id: String,
    name: String,
    family: String,
    port: Option<u16>,
    capabilities: Value,
}

impl Driver {
    fn has(&self, capability: &str) -> bool {
        self.capabilities[capability].as_bool().unwrap_or(false)
    }

    fn opens_files(&self) -> bool {
        self.has("sharedConnection")
    }

    fn settings(&self) -> Vec<Value> {
        self.capabilities["settings"].as_array().cloned().unwrap_or_default()
    }

    fn isolation_levels(&self) -> Vec<String> {
        self.capabilities["isolationLevels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|level| level.as_str().map(str::to_owned))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(super) struct ConnectionDialog {
    editing: Option<String>,
    detection: Option<String>,
    detected: Vec<Value>,
    pick: Option<String>,
    drivers: Vec<Driver>,
    profiles: Vec<Profile>,
    profile: Option<usize>,
    shown: Option<Driver>,
    detected_profile: Option<String>,
    types: Vec<(String, String)>,
    pub(super) driver: usize,
    search: String,
    pub(super) page: Page,
    by_url: bool,
    url: String,
    host: String,
    port: String,
    database: String,
    show_all_databases: bool,
    no_auth: bool,
    user: String,
    password: String,
    save_password: bool,
    has_password: bool,
    ssh: bool,
    ssh_host: String,
    ssh_user: String,
    ssh_port: String,
    ssh_jump: String,
    tls_mode: String,
    ca_file: String,
    client_cert: String,
    client_key: String,
    direct_tls: bool,
    settings: Map<String, Value>,
    properties: Vec<(String, String)>,
    removed: Vec<String>,
    name: String,
    connection_type: String,
    folder: String,
    project: bool,
    has_project: bool,
    read_only: bool,
    navigator_view: String,
    show_system_objects: bool,
    include_schemas: String,
    exclude_schemas: String,
    auto_commit: Option<bool>,
    isolation: Option<String>,
    default_schema: String,
    bootstrap: String,
    overrides: Value,
    test: Option<Test>,
    error: Option<String>,
    file_action: Option<FileAction>,
    connect_after: bool,
    fresh: bool,
    reveal: bool,
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

fn masks(list: &str) -> Vec<String> {
    list.split(',').map(str::trim).filter(|mask| !mask.is_empty()).map(str::to_owned).collect()
}

impl ConnectionDialog {
    pub(super) fn open(registry: &Registry, session: &mut Session, params: &Value) -> Result<Self, String> {
        let listed = registry.dispatch(session, "db.driver.list", json!({})).map_err(message)?;
        let drivers: Vec<Driver> = listed["drivers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|driver| Driver {
                id: text(&driver["id"]),
                name: text(&driver["name"]),
                family: driver["family"].as_str().map_or_else(|| text(&driver["id"]), str::to_owned),
                port: driver["defaultPort"].as_u64().and_then(|port| u16::try_from(port).ok()),
                capabilities: driver["capabilities"].clone(),
            })
            .collect();
        let profiles: Vec<Profile> = listed["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|profile| Profile {
                id: text(&profile["id"]),
                name: text(&profile["name"]),
                driver: text(&profile["driver"]),
                family: text(&profile["family"]),
                port: profile["defaultPort"].as_u64().and_then(|port| u16::try_from(port).ok()),
                database: profile["defaultDatabase"].as_str().map(str::to_owned),
                user: profile["defaultUser"].as_str().map(str::to_owned),
                tls: profile["tls"].as_str().unwrap_or("prefer").to_owned(),
                settings: profile["settings"].as_array().into_iter().flatten().map(text).collect(),
                capabilities: profile["capabilities"].clone(),
            })
            .collect();
        let types = registry.dispatch(session, "db.connectionType.list", json!({})).map_err(message)?;
        let types = types["types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|kind| (text(&kind["id"]), text(&kind["name"])))
            .collect();
        let has_project =
            session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project).is_some();
        let mut dialog = Self::blank(drivers, profiles, types, has_project);
        if let Some(id) = params["id"].as_str().filter(|id| !id.is_empty()) {
            let entry = registry.dispatch(session, "db.dataSource.get", json!({ "id": id })).map_err(message)?;
            dialog.load(&entry);
        }
        dialog.prefill(params);
        if let Some(id) = params["detection"].as_str().filter(|id| !id.is_empty()) {
            dialog.take_detection(session, id)?;
        } else if dialog.editing.is_none() && registry.info("db.detect.list").is_some() {
            let listed = registry.dispatch(session, "db.detect.list", json!({ "limit": 5 })).unwrap_or_default();
            dialog.detected = listed["detections"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|offer| offer["supported"] == true)
                .cloned()
                .collect();
        }
        Ok(dialog)
    }

    pub(super) fn take_detection(&mut self, session: &mut Session, id: &str) -> Result<(), String> {
        let finding = zen_workbench::database::detect::offer(session, id).ok_or_else(|| t!("ide-dbc-no-detection"))?;
        if let Some(index) = self.drivers.iter().position(|driver| driver.id == finding.driver) {
            self.choose_driver(index);
        }
        self.by_url = false;
        self.url.clear();
        self.name = finding.name();
        self.host = finding.host.clone().unwrap_or_default();
        self.port = finding.port.map(|port| port.to_string()).unwrap_or_default();
        self.database = finding.database.clone().unwrap_or_default();
        self.user = finding.user.clone().unwrap_or_default();
        self.password = finding.password.as_ref().map(|secret| secret.expose().to_owned()).unwrap_or_default();
        self.detection = Some(id.to_owned());
        Ok(())
    }

    fn blank(drivers: Vec<Driver>, profiles: Vec<Profile>, types: Vec<(String, String)>, has_project: bool) -> Self {
        let mut dialog = Self {
            editing: None,
            detection: None,
            detected: Vec::new(),
            pick: None,
            drivers,
            profiles,
            profile: None,
            shown: None,
            detected_profile: None,
            types,
            driver: 0,
            search: String::new(),
            page: Page::Main,
            by_url: false,
            url: String::new(),
            host: "localhost".to_owned(),
            port: String::new(),
            database: String::new(),
            show_all_databases: true,
            no_auth: false,
            user: String::new(),
            password: String::new(),
            save_password: true,
            has_password: false,
            ssh: false,
            ssh_host: String::new(),
            ssh_user: String::new(),
            ssh_port: String::new(),
            ssh_jump: String::new(),
            tls_mode: "prefer".to_owned(),
            ca_file: String::new(),
            client_cert: String::new(),
            client_key: String::new(),
            direct_tls: false,
            settings: Map::new(),
            properties: Vec::new(),
            removed: Vec::new(),
            name: String::new(),
            connection_type: zen_db::types::DEVELOPMENT.to_owned(),
            folder: String::new(),
            project: false,
            has_project,
            read_only: false,
            navigator_view: "advanced".to_owned(),
            show_system_objects: false,
            include_schemas: String::new(),
            exclude_schemas: String::new(),
            auto_commit: None,
            isolation: None,
            default_schema: String::new(),
            bootstrap: String::new(),
            overrides: json!({}),
            test: None,
            error: None,
            file_action: None,
            connect_after: true,
            fresh: true,
            reveal: true,
        };
        dialog.choose_driver(0);
        dialog
    }

    fn current(&self) -> Option<&Driver> {
        self.shown.as_ref()
    }

    fn current_profile(&self) -> Option<&Profile> {
        self.profile.and_then(|index| self.profiles.get(index))
    }

    fn refresh_current(&mut self) {
        let profile = self.current_profile().cloned();
        self.shown = self.drivers.get(self.driver).cloned().map(|mut driver| {
            if let Some(profile) = profile {
                driver.name = profile.name;
                driver.port = profile.port.or(driver.port);
                if !profile.capabilities.is_null() {
                    driver.capabilities = profile.capabilities;
                }
            }
            driver
        });
    }

    fn set_profile(&mut self, id: Option<&str>) {
        self.profile = id.and_then(|id| self.profiles.iter().position(|profile| profile.id == id));
        self.detected_profile = None;
        self.refresh_current();
    }

    fn choose_profile(&mut self, index: usize) {
        let Some(profile) = self.profiles.get(index).cloned() else { return };
        let Some(driver) = self.drivers.iter().position(|driver| driver.id == profile.driver) else { return };
        self.choose_driver(driver);
        self.profile = Some(index);
        self.port = profile.port.map(|port| port.to_string()).unwrap_or_default();
        self.database = profile.database.unwrap_or_default();
        self.user = profile.user.unwrap_or_default();
        self.tls_mode = profile.tls;
        for setting in profile.settings {
            self.settings.insert(setting, Value::Bool(true));
        }
        self.refresh_current();
    }

    fn rows(&self) -> Vec<Row> {
        let needle = self.search.trim().to_lowercase();
        let found =
            |id: &str, name: &str| needle.is_empty() || name.to_lowercase().contains(&needle) || id.contains(&needle);
        let mut families: Vec<&str> = Vec::new();
        for family in self.drivers.iter().map(|driver| driver.family.as_str()) {
            if !families.contains(&family) {
                families.push(family);
            }
        }
        let mut rows = Vec::new();
        for (number, family) in families.iter().enumerate() {
            let mut group: Vec<Row> = self
                .drivers
                .iter()
                .enumerate()
                .filter(|(_, driver)| driver.family == *family && found(&driver.id, &driver.name))
                .map(|(index, _)| Row::Driver(index))
                .collect();
            group.extend(
                self.profiles
                    .iter()
                    .enumerate()
                    .filter(|(_, profile)| profile.family == *family && found(&profile.id, &profile.name))
                    .filter(|(_, profile)| self.drivers.iter().any(|driver| driver.id == profile.driver))
                    .map(|(index, _)| Row::Profile(index)),
            );
            if !group.is_empty() {
                rows.push(Row::Family(number));
                rows.extend(group);
            }
        }
        rows
    }

    fn family_title(&self, number: usize) -> String {
        let mut families: Vec<&str> = Vec::new();
        for family in self.drivers.iter().map(|driver| driver.family.as_str()) {
            if !families.contains(&family) {
                families.push(family);
            }
        }
        let family = families.get(number).copied().unwrap_or_default();
        let name = self.drivers.iter().find(|driver| driver.id == family).map_or(family, |driver| driver.name.as_str());
        t!("ide-dbc-family", name = name)
    }

    fn choose_driver(&mut self, index: usize) {
        self.driver = index;
        self.profile = None;
        self.detected_profile = None;
        self.refresh_current();
        let Some(driver) = self.drivers.get(index).cloned() else { return };
        self.port = driver.port.map(|port| port.to_string()).unwrap_or_default();
        if driver.id == zen_db::postgres::ID {
            self.database = "postgres".to_owned();
            self.user = "postgres".to_owned();
        } else if driver.id == zen_db::mysql::ID || driver.id == zen_db::mysql::MARIADB_ID {
            self.database.clear();
            "root".clone_into(&mut self.user);
        } else {
            self.database.clear();
            self.user.clear();
        }
        if self.editing.is_none() {
            self.no_auth = !driver.has("password")
                || zen_db::registry::passwordless_by_default(&driver.id, Some(self.user.as_str()));
        }
        self.show_all_databases = driver.has("catalogs");
        self.settings = Map::new();
        for setting in driver.settings() {
            let key = text(&setting["key"]);
            if key == SSL_MODE {
                continue;
            }
            let default = text(&setting["default"]);
            let value = match setting["kind"].as_str() {
                Some("bool") => Value::Bool(default == "true"),
                _ => Value::String(default),
            };
            self.settings.insert(key, value);
        }
    }

    fn load(&mut self, entry: &Value) {
        self.editing = entry["id"].as_str().map(str::to_owned);
        if let Some(index) = self.drivers.iter().position(|driver| driver.id == text(&entry["driver"])) {
            self.choose_driver(index);
        }
        self.set_profile(entry["profile"].as_str());
        self.name = text(&entry["name"]);
        self.folder = text(&entry["folder"]);
        self.project = entry["scope"] == "project";
        self.connection_type = text(&entry["type"]);
        self.host = text(&entry["host"]);
        self.port = text(&entry["port"]);
        self.database = text(&entry["database"]);
        self.user = text(&entry["user"]);
        self.url = text(&entry["url"]);
        self.no_auth = entry["auth"] == "none";
        self.save_password = entry["savePassword"].as_bool().unwrap_or(true);
        self.has_password = entry["hasPassword"].as_bool().unwrap_or(false);
        self.read_only = entry["readOnly"].as_bool().unwrap_or(false);
        let tls = &entry["tls"];
        self.tls_mode = tls["mode"].as_str().unwrap_or("prefer").to_owned();
        self.ca_file = text(&tls["caFile"]);
        self.client_cert = text(&tls["clientCert"]);
        self.client_key = text(&tls["clientKey"]);
        self.direct_tls = tls["direct"].as_bool().unwrap_or(false);
        let ssh = &entry["ssh"];
        self.ssh = ssh.is_object();
        self.ssh_host = text(&ssh["host"]);
        self.ssh_user = text(&ssh["user"]);
        self.ssh_port = text(&ssh["port"]);
        self.ssh_jump = text(&ssh["jump"]);
        let navigator = &entry["navigator"];
        self.navigator_view = navigator["view"].as_str().unwrap_or("advanced").to_owned();
        self.show_all_databases = navigator["showAllDatabases"].as_bool().unwrap_or(true);
        self.show_system_objects = navigator["showSystemObjects"].as_bool().unwrap_or(false);
        for filter in entry["filters"].as_array().into_iter().flatten() {
            if filter["kind"] == "schema" {
                let join =
                    |list: &Value| list.as_array().into_iter().flatten().map(text).collect::<Vec<_>>().join(", ");
                self.include_schemas = join(&filter["include"]);
                self.exclude_schemas = join(&filter["exclude"]);
            }
        }
        let bootstrap = &entry["bootstrap"];
        self.default_schema = text(&bootstrap["defaultSchema"]);
        self.isolation = bootstrap["isolation"].as_str().map(str::to_owned);
        self.bootstrap =
            bootstrap["initStatements"].as_array().into_iter().flatten().map(text).collect::<Vec<_>>().join("\n");
        self.overrides = entry["overrides"].clone();
        self.auto_commit = self.overrides["autoCommit"].as_bool();
        let declared: Vec<String> = self
            .current()
            .map(Driver::settings)
            .unwrap_or_default()
            .iter()
            .map(|setting| text(&setting["key"]))
            .collect();
        for (key, value) in entry["settings"].as_object().into_iter().flatten() {
            if declared.contains(key) {
                self.settings.insert(key.clone(), value.clone());
            } else {
                self.properties.push((key.clone(), text(value)));
            }
        }
    }

    fn prefill(&mut self, params: &Value) {
        if self.editing.is_none()
            && let Some(driver) = params["driver"].as_str()
            && let Some(index) = self.drivers.iter().position(|known| known.id == driver)
        {
            self.choose_driver(index);
        }
        if self.editing.is_none()
            && let Some(profile) = params["profile"].as_str()
            && let Some(index) = self.profiles.iter().position(|known| known.id == profile)
        {
            self.choose_profile(index);
        }
        if let Some(url) = params["url"].as_str() {
            self.by_url = true;
            self.url = url.to_owned();
            self.fill_from_url();
        }
        let set = |key: &str, field: &mut String| {
            if let Some(value) = params.get(key).filter(|value| !value.is_null()) {
                *field = text(value);
            }
        };
        set("name", &mut self.name);
        set("host", &mut self.host);
        set("port", &mut self.port);
        set("database", &mut self.database);
        set("user", &mut self.user);
        set("folder", &mut self.folder);
        set("type", &mut self.connection_type);
        if self.editing.is_none() && self.has_project {
            self.project = params["scope"] == "project";
        }
        if let Some(page) = params["page"].as_str().and_then(Page::of) {
            self.page = page;
        }
    }

    pub(super) fn fill_from_url(&mut self) {
        let Some(parsed) = zen_db::registry::parse_uri(self.url.trim()) else {
            self.error = Some(t!("ide-dbc-bad-url"));
            return;
        };
        self.error = None;
        let base = parsed.scheme.strip_suffix("+srv").unwrap_or(&parsed.scheme);
        let base = if base == "rediss" { "redis" } else { base };
        let by_scheme = self.drivers.iter().position(|driver| {
            driver.capabilities["uriSchemes"]
                .as_array()
                .is_some_and(|schemes| schemes.iter().any(|scheme| scheme == parsed.scheme.as_str() || scheme == base))
        });
        let scheme_profile = zen_db::profile::for_scheme(&parsed.scheme);
        let driver_id = by_scheme
            .and_then(|index| self.drivers.get(index))
            .map(|driver| driver.id.clone())
            .or_else(|| scheme_profile.map(|(profile, _)| profile.driver.to_owned()));
        let profile = zen_db::profile::for_url(&parsed.scheme, driver_id.as_deref(), parsed.host.as_deref())
            .and_then(|found| self.profiles.iter().position(|known| known.id == found.id));
        if self.editing.is_none() {
            if let Some(index) = profile {
                if self.profile != Some(index) {
                    self.choose_profile(index);
                }
            } else if let Some(index) = driver_id.and_then(|id| self.drivers.iter().position(|driver| driver.id == id))
                && index != self.driver
            {
                self.choose_driver(index);
            }
        }
        if let Some(host) = parsed.host {
            self.host = host;
        }
        self.port = parsed.port.map(|port| port.to_string()).unwrap_or_else(|| {
            self.current().and_then(|driver| driver.port).map(|port| port.to_string()).unwrap_or_default()
        });
        if parsed.port.is_none() && self.host.contains(',') {
            self.port.clear();
        }
        self.database = parsed.database.unwrap_or_default();
        if let Some(user) = parsed.user {
            self.user = user;
        }
        if let Some(password) = parsed.password {
            self.password = password;
            self.url = parsed.without_password;
        }
        if self.editing.is_none() {
            let driver = self.current().map(|driver| driver.id.clone()).unwrap_or_default();
            let asks = self.current().is_some_and(|driver| driver.has("password"));
            self.no_auth = self.password.is_empty()
                && (!asks || zen_db::registry::passwordless_by_default(&driver, Some(self.user.as_str())));
        }
        if parsed.scheme == "rediss" || scheme_profile.is_some_and(|(_, tls)| tls) {
            self.tls_mode = "require".to_owned();
        }
        for (key, value) in parsed.params {
            match key.as_str() {
                key if zen_db::registry::TLS_MODE_KEYS.contains(&key) => {
                    if let Some(mode) = zen_db::registry::tls_mode_named(&value) {
                        mode.clone_into(&mut self.tls_mode);
                    }
                }
                "sslnegotiation" => self.direct_tls = value == "direct",
                "sslrootcert" => self.ca_file = value,
                "sslcert" => self.client_cert = value,
                "sslkey" => self.client_key = value,
                "password" | "application_name" => {}
                _ => {
                    if self.settings.contains_key(&key) {
                        let value = match self.settings[&key] {
                            Value::Bool(_) => Value::Bool(value == "true" || value == "1"),
                            _ => Value::String(value),
                        };
                        self.settings.insert(key, value);
                    } else if let Some(row) = self.properties.iter_mut().find(|(known, _)| *known == key) {
                        row.1 = value;
                    } else {
                        self.properties.push((key, value));
                    }
                }
            }
        }
    }

    fn name_or_default(&self) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            return name.to_owned();
        }
        if self.current().is_some_and(Driver::opens_files) {
            let path = std::path::Path::new(self.database.trim());
            if let Some(file) = path.file_name() {
                return file.to_string_lossy().into_owned();
            }
        }
        let driver = self.current().map(|driver| driver.name.clone()).unwrap_or_default();
        match self.host.trim() {
            "" => driver,
            host => format!("{driver} - {host}"),
        }
    }

    pub(super) fn fields(&self) -> Value {
        let driver = self.current();
        let mut fields = json!({
            "name": self.name_or_default(),
            "folder": self.folder.trim(),
            "host": self.host.trim(),
            "database": self.database.trim(),
            "user": self.user.trim(),
            "auth": if self.no_auth { "none" } else { "password" },
            "type": self.connection_type,
            "readOnly": self.read_only,
            "savePassword": self.save_password,
            "tls": {
                "mode": self.tls_mode,
                "direct": self.direct_tls,
            },
            "ssh": {
                "host": if self.ssh { self.ssh_host.trim() } else { "" },
            },
            "navigator": {
                "view": self.navigator_view,
                "showAllDatabases": self.show_all_databases,
                "showSystemObjects": self.show_system_objects,
            },
            "bootstrap": {
                "initStatements": self.bootstrap.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>(),
            },
        });
        fields["profile"] = json!(self.current_profile().map(|profile| profile.id.as_str()).unwrap_or_default());
        if let Some(driver) = driver {
            fields["driver"] = json!(driver.id);
            if driver.opens_files() {
                fields["host"] = json!("");
            }
        }
        if self.by_url && !self.url.trim().is_empty() {
            fields["url"] = json!(self.url.trim());
        }
        if let Ok(port) = self.port.trim().parse::<u16>()
            && !driver.is_some_and(Driver::opens_files)
        {
            fields["port"] = json!(port);
        }
        if !self.no_auth && !self.password.is_empty() {
            fields["password"] = json!(self.password);
        }
        for (key, path) in
            [("caFile", &self.ca_file), ("clientCert", &self.client_cert), ("clientKey", &self.client_key)]
        {
            if !path.trim().is_empty() {
                fields["tls"][key] = json!(path.trim());
            }
        }
        if self.ssh {
            if !self.ssh_user.trim().is_empty() {
                fields["ssh"]["user"] = json!(self.ssh_user.trim());
            }
            if let Ok(port) = self.ssh_port.trim().parse::<u16>() {
                fields["ssh"]["port"] = json!(port);
            }
            if !self.ssh_jump.trim().is_empty() {
                fields["ssh"]["jump"] = json!(self.ssh_jump.trim());
            }
        }
        let (include, exclude) = (masks(&self.include_schemas), masks(&self.exclude_schemas));
        fields["filters"] = if include.is_empty() && exclude.is_empty() {
            json!([])
        } else {
            json!([{ "kind": "schema", "include": include, "exclude": exclude }])
        };
        if !self.default_schema.trim().is_empty() {
            fields["bootstrap"]["defaultSchema"] = json!(self.default_schema.trim());
        }
        if let Some(isolation) = &self.isolation {
            fields["bootstrap"]["isolation"] = json!(isolation);
        }
        let mut overrides = self.overrides.as_object().cloned().unwrap_or_default();
        match self.auto_commit {
            Some(on) => overrides.insert("autoCommit".to_owned(), json!(on)),
            None => overrides.remove("autoCommit"),
        };
        fields["overrides"] = Value::Object(overrides);
        let mut settings = self.settings.clone();
        for (key, value) in &self.properties {
            if !key.trim().is_empty() {
                settings.insert(key.trim().to_owned(), json!(value));
            }
        }
        for key in &self.removed {
            settings.entry(key.clone()).or_insert(Value::Null);
        }
        fields["settings"] = Value::Object(settings);
        fields
    }

    pub(super) fn save_call(&self) -> (&'static str, Value) {
        let mut fields = self.fields();
        if self.editing.is_none()
            && let Some(id) = &self.detection
        {
            fields["id"] = json!(id);
            fields["scope"] = json!(if self.project { "project" } else { "global" });
            return ("db.detect.accept", fields);
        }
        match &self.editing {
            Some(id) => {
                fields["id"] = json!(id);
                ("db.dataSource.update", fields)
            }
            None => {
                fields["scope"] = json!(if self.project { "project" } else { "global" });
                ("db.dataSource.add", fields)
            }
        }
    }

    pub(super) fn connect_call(&self, saved: &Value) -> Option<Value> {
        if self.editing.is_some() || !self.connect_after {
            return None;
        }
        let id = saved["id"].as_str().or_else(|| saved["connection"]["id"].as_str())?;
        let mut params = json!({ "connection": id });
        if !self.no_auth && !self.password.is_empty() {
            params["password"] = json!(self.password);
        }
        Some(params)
    }

    pub(super) fn test_call(&self) -> Value {
        let mut fields = self.fields();
        match &self.editing {
            Some(id) => fields["connection"] = json!(id),
            None => fields["scope"] = json!(if self.project { "project" } else { "global" }),
        }
        fields
    }

    pub(super) fn set_file(&mut self, path: &str) {
        self.database = path.to_owned();
        self.error = None;
    }

    fn choose_file(&mut self, registry: &Registry, session: &mut Session, action: FileAction) {
        let extensions = zen_workbench::database::files::EXTENSIONS;
        let databases = || {
            rfd::FileDialog::new()
                .add_filter(t!("ide-dbc-path-filter"), &extensions)
                .add_filter(t!("ide-dbc-path-all-files"), &["*"])
        };
        match action {
            FileAction::Open => {
                if let Some(path) = databases().set_title(t!("ide-dbc-path-open-title")).pick_file() {
                    self.set_file(&path.display().to_string());
                }
            }
            FileAction::Create => {
                let Some(path) =
                    databases().set_title(t!("ide-dbc-path-create-title")).set_file_name("database.db").save_file()
                else {
                    return;
                };
                match registry.dispatch(session, "db.databaseFile.create", json!({ "path": path })) {
                    Ok(created) => self.set_file(created["path"].as_str().unwrap_or_default()),
                    Err(error) => self.error = Some(message(error)),
                }
            }
            FileAction::Setting(key) => {
                if let Some(path) = rfd::FileDialog::new().set_title(setting_title(&key)).pick_file() {
                    self.settings.insert(key, json!(path.display().to_string()));
                }
            }
        }
    }

    fn start_test(&mut self, registry: &Registry, session: &mut Session) {
        self.test = match registry.dispatch(session, "db.dataSource.test", self.test_call()) {
            Ok(started) => started["operation"].as_u64().map(Test::Running),
            Err(error) => Some(Test::Failed(message(error))),
        };
    }

    fn cancel_test(&mut self, registry: &Registry, session: &mut Session) {
        if let Some(Test::Running(operation)) = self.test {
            let _ = registry.dispatch(session, "db.connection.cancel", json!({ "operation": operation }));
            self.test = None;
        }
    }

    fn poll_test(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let Some(Test::Running(operation)) = self.test else { return false };
        let Ok(state) = registry.dispatch(session, "db.operation.state", json!({ "operation": operation })) else {
            self.test = None;
            return false;
        };
        match state["status"].as_str() {
            Some("running") => return true,
            Some("done") => {
                let report = &state["test"];
                self.detected_profile = report["profile"]
                    .as_str()
                    .filter(|found| self.current_profile().is_none_or(|profile| profile.id != *found))
                    .filter(|found| self.profiles.iter().any(|profile| profile.id == *found))
                    .map(str::to_owned);
                let server = report["serverVersion"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| t!("ide-dbc-test-server-unknown"));
                self.test = Some(Test::Passed(t!(
                    "ide-dbc-test-passed",
                    server = server.as_str(),
                    roundtrip = report["roundTripMs"].as_u64().unwrap_or(0),
                    connect = report["connectMs"].as_u64().unwrap_or(0)
                )));
            }
            Some("cancelled") => self.test = None,
            _ => self.test = Some(Test::Failed(state["message"].as_str().unwrap_or_default().to_owned())),
        }
        false
    }
}

enum Choice {
    Ok,
    Cancel,
    Test,
    CancelTest,
}

impl IdeView {
    pub(super) fn open_connection_dialog(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<(), String> {
        if registry.info("db.dataSource.add").is_none() {
            return Err(t!("ide-dbc-no-module"));
        }
        if let Some(dialog) = self.connection_dialog.as_mut() {
            dialog.cancel_test(registry, session);
        }
        self.connection_dialog = Some(ConnectionDialog::open(registry, session, params)?);
        Ok(())
    }

    pub(super) fn open_database_file(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
    ) -> bool {
        if registry.info(OPEN_FILE).is_none() || !zen_workbench::database::files::is_database_file(path) {
            return false;
        }
        let Ok(opened) = registry.dispatch(session, OPEN_FILE, json!({ "path": path })) else { return false };
        if opened["offer"].is_object() {
            let mut params = opened["offer"].clone();
            params["page"] = json!("main");
            if let Err(error) = self.open_connection_dialog(registry, session, &params) {
                self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                ));
            }
        }
        true
    }

    pub(super) fn connection_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(params) = session.state_mut::<ConnectionDialogRequest>().0.take()
            && let Err(error) = self.open_connection_dialog(registry, session, &params)
        {
            self.notify(zen_workbench::notifications::NewNotification::message(
                zen_workbench::notifications::Group::General,
                zen_workbench::notifications::Kind::Error,
                error,
            ));
        }
        let Some(dialog) = &mut self.connection_dialog else { return };
        if let Some(id) = dialog.pick.take()
            && let Err(error) = dialog.take_detection(session, &id)
        {
            dialog.error = Some(error);
        }
        if dialog.poll_test(registry, session) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(action) = dialog.file_action.take() {
            dialog.choose_file(registry, session, action);
        }
        let p = self.palette;
        match show(ui, &p, dialog) {
            Some(Choice::Cancel) => {
                dialog.cancel_test(registry, session);
                self.connection_dialog = None;
            }
            Some(Choice::Test) => dialog.start_test(registry, session),
            Some(Choice::CancelTest) => dialog.cancel_test(registry, session),
            Some(Choice::Ok) => {
                let (command, params) = dialog.save_call();
                match registry.dispatch(session, command, params) {
                    Ok(saved) => {
                        dialog.cancel_test(registry, session);
                        if let Some(connect) = dialog.connect_call(&saved) {
                            let _ = registry.dispatch(session, "db.connection.connect", connect);
                        }
                        self.connection_dialog = None;
                    }
                    Err(error) => dialog.error = Some(message(error)),
                }
            }
            None => {}
        }
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.allocate_ui_with_layout(vec2(LABEL_WIDTH, 28.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(LABEL_WIDTH);
        ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
    });
}

fn field(ui: &mut Ui, p: &IdePalette, name: String, value: &mut String, width: f32) -> egui::Response {
    ui.horizontal(|ui| {
        label(ui, p, name.clone());
        let response = input(ui, p, egui::TextEdit::singleline(value).desired_width(width));
        accessibility::name(&response, Role::TextInput, &name);
        response
    })
    .inner
}

pub(super) fn input(ui: &mut Ui, p: &IdePalette, edit: egui::TextEdit<'_>) -> egui::Response {
    egui::Frame::new()
        .fill(p.text_field)
        .stroke(egui::Stroke::new(1.0, p.text_field_border))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| ui.add(edit.frame(egui::Frame::NONE)))
        .inner
}

fn hint(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
}

fn check(ui: &mut Ui, p: &IdePalette, value: &mut bool, name: String) {
    super::form::check(ui, p, value, &name);
}

fn combo(ui: &mut Ui, p: &IdePalette, id: &str, name: String, value: &mut String, choices: &[(String, String)]) {
    ui.horizontal(|ui| {
        label(ui, p, name.clone());
        let shown =
            choices.iter().find(|(key, _)| key == value).map_or_else(|| value.clone(), |(_, title)| title.clone());
        super::form::combo(ui, p, ("dbc", id), &name, &shown, 236.0, |ui| {
            for (key, title) in choices {
                ui.selectable_value(value, key.clone(), title);
            }
        });
    });
}

fn section(ui: &mut Ui, p: &IdePalette, title: String) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).font(theme::semibold(13.0)).color(p.text));
}

fn detected(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    if dialog.detected.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        label(ui, p, t!("ide-dbc-detected"));
        for offer in &dialog.detected {
            let id = offer["id"].as_str().unwrap_or_default();
            let chosen = dialog.detection.as_deref() == Some(id);
            let place = match (offer["host"].as_str(), offer["port"].as_u64()) {
                (Some(host), Some(port)) => format!("{host}:{port}"),
                (Some(host), None) => host.to_owned(),
                _ => String::new(),
            };
            let text = format!(
                "{} {place} ({})",
                offer["product"].as_str().unwrap_or_default(),
                offer["file"].as_str().unwrap_or_default()
            );
            let color = if chosen { p.text } else { p.link };
            let link = ui.link(RichText::new(text.trim()).font(theme::regular(12.0)).color(color));
            if link.clicked() {
                dialog.pick = Some(id.to_owned());
            }
        }
    });
}

fn file_page(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    ui.horizontal(|ui| {
        label(ui, p, t!("ide-dbc-path"));
        let response = input(
            ui,
            p,
            egui::TextEdit::singleline(&mut dialog.database).hint_text(t!("ide-dbc-path-hint")).desired_width(250.0),
        );
        accessibility::name(&response, Role::TextInput, &t!("ide-dbc-path"));
        if super::vcs::secondary_button(ui, p, &t!("ide-dbc-path-open")).clicked() {
            dialog.file_action = Some(FileAction::Open);
        }
        if super::vcs::secondary_button(ui, p, &t!("ide-dbc-path-create")).clicked() {
            dialog.file_action = Some(FileAction::Create);
        }
    });
    hint(ui, p, t!("ide-dbc-path-help"));
}

fn main_page(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    detected(ui, p, dialog);
    let Some(driver) = dialog.current().cloned() else { return };
    if driver.opens_files() {
        file_page(ui, p, dialog);
        return;
    }
    ui.horizontal(|ui| {
        label(ui, p, t!("ide-dbc-connect-by"));
        super::form::radio(ui, p, &mut dialog.by_url, false, &t!("ide-dbc-by-host"));
        super::form::radio(ui, p, &mut dialog.by_url, true, &t!("ide-dbc-by-url"));
    });
    if dialog.by_url {
        let response = field(ui, p, t!("ide-dbc-url"), &mut dialog.url, 420.0);
        if response.changed() && dialog.url.contains("://") {
            dialog.fill_from_url();
        }
        let schemes = driver.capabilities["uriSchemes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|scheme| format!("{scheme}://"))
            .collect::<Vec<_>>()
            .join(", ");
        hint(ui, p, t!("ide-dbc-url-hint", schemes = schemes.as_str()));
    }
    ui.add_enabled_ui(!dialog.by_url, |ui| {
        ui.horizontal(|ui| {
            label(ui, p, t!("ide-dbc-host"));
            let host = input(ui, p, egui::TextEdit::singleline(&mut dialog.host).desired_width(250.0));
            accessibility::name(&host, Role::TextInput, &t!("ide-dbc-host"));
            if host.changed() && dialog.host.contains("://") {
                dialog.url = std::mem::take(&mut dialog.host);
                dialog.fill_from_url();
            }
            ui.label(RichText::new(t!("ide-dbc-port")).font(theme::regular(13.0)).color(p.text));
            let hint = driver.port.map(|port| port.to_string()).unwrap_or_default();
            let port = input(ui, p, egui::TextEdit::singleline(&mut dialog.port).hint_text(hint).desired_width(70.0));
            accessibility::name(&port, Role::TextInput, &t!("ide-dbc-port"));
        });
        field(ui, p, t!("ide-dbc-database"), &mut dialog.database, 250.0);
    });
    if driver.has("catalogs") {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_WIDTH + 8.0);
            check(ui, p, &mut dialog.show_all_databases, t!("ide-dbc-show-all-databases"));
        });
    }
    section(ui, p, t!("ide-dbc-authentication"));
    if driver.has("password") {
        let mut kind = if dialog.no_auth { "none".to_owned() } else { "password".to_owned() };
        combo(
            ui,
            p,
            "auth",
            t!("ide-dbc-auth"),
            &mut kind,
            &[("password".to_owned(), t!("ide-dbc-auth-password")), ("none".to_owned(), t!("ide-dbc-auth-none"))],
        );
        dialog.no_auth = kind == "none";
    }
    field(ui, p, t!("ide-dbc-user"), &mut dialog.user, 250.0);
    if !dialog.no_auth {
        ui.horizontal(|ui| {
            label(ui, p, t!("ide-dbc-password"));
            let hint = if dialog.has_password { t!("ide-dbc-password-saved") } else { String::new() };
            let response = input(
                ui,
                p,
                egui::TextEdit::singleline(&mut dialog.password).password(true).hint_text(hint).desired_width(250.0),
            );
            accessibility::name(&response, Role::PasswordInput, &t!("ide-dbc-password"));
            check(ui, p, &mut dialog.save_password, t!("ide-dbc-save-password"));
        });
    }
}

fn ssh_page(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    section(ui, p, t!("ide-dbc-ssh"));
    check(ui, p, &mut dialog.ssh, t!("ide-dbc-ssh-use"));
    ui.add_enabled_ui(dialog.ssh, |ui| {
        field(ui, p, t!("ide-dbc-ssh-host"), &mut dialog.ssh_host, 250.0);
        field(ui, p, t!("ide-dbc-ssh-user"), &mut dialog.ssh_user, 160.0);
        field(ui, p, t!("ide-dbc-ssh-port"), &mut dialog.ssh_port, 70.0);
        field(ui, p, t!("ide-dbc-ssh-jump"), &mut dialog.ssh_jump, 250.0);
    });
    hint(ui, p, t!("ide-dbc-ssh-hint"));
    ui.add_space(6.0);
    section(ui, p, t!("ide-dbc-ssl"));
    let modes: Vec<(String, String)> = TLS_MODES.iter().map(|mode| ((*mode).to_owned(), (*mode).to_owned())).collect();
    combo(ui, p, "sslmode", t!("ide-dbc-ssl-mode"), &mut dialog.tls_mode, &modes);
    field(ui, p, t!("ide-dbc-ssl-ca"), &mut dialog.ca_file, 380.0);
    field(ui, p, t!("ide-dbc-ssl-cert"), &mut dialog.client_cert, 380.0);
    field(ui, p, t!("ide-dbc-ssl-key"), &mut dialog.client_key, 380.0);
    if dialog.current().is_some_and(|driver| driver.id == zen_db::postgres::ID) {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_WIDTH + 8.0);
            check(ui, p, &mut dialog.direct_tls, t!("ide-dbc-ssl-direct"));
        });
    }
}

fn setting_title(key: &str) -> String {
    let message = format!("ide-dbc-setting-{key}");
    let title = t!(&message);
    if title == message { key.to_owned() } else { title }
}

fn advanced_page(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    let Some(driver) = dialog.current().cloned() else { return };
    section(ui, p, t!("ide-dbc-driver-settings", driver = driver.name.as_str()));
    let settings: Vec<Value> = driver
        .settings()
        .into_iter()
        .filter(|setting| !matches!(setting["key"].as_str(), Some(SSL_MODE | KEEP_ALIVE)))
        .collect();
    if settings.is_empty() {
        hint(ui, p, t!("ide-dbc-no-settings"));
    }
    for setting in settings {
        let key = text(&setting["key"]);
        let title = setting_title(&key);
        match setting["kind"].as_str() {
            Some("bool") => {
                let mut on = dialog.settings.get(&key).and_then(Value::as_bool).unwrap_or(false);
                ui.horizontal(|ui| {
                    ui.add_space(LABEL_WIDTH + 8.0);
                    check(ui, p, &mut on, title);
                });
                dialog.settings.insert(key, Value::Bool(on));
            }
            Some("choice") => {
                let choices: Vec<(String, String)> = setting["choices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|choice| (text(choice), text(choice)))
                    .collect();
                let mut value = dialog.settings.get(&key).map(text).unwrap_or_default();
                combo(ui, p, &key, title, &mut value, &choices);
                dialog.settings.insert(key, Value::String(value));
            }
            Some("path") => {
                let mut value = dialog.settings.get(&key).map(text).unwrap_or_default();
                ui.horizontal(|ui| {
                    field(ui, p, title, &mut value, 250.0);
                    if super::vcs::secondary_button(ui, p, &t!("ide-dbc-browse")).clicked() {
                        dialog.file_action = Some(FileAction::Setting(key.clone()));
                    }
                });
                dialog.settings.insert(key, Value::String(value));
            }
            _ => {
                let mut value = dialog.settings.get(&key).map(text).unwrap_or_default();
                field(ui, p, title, &mut value, 250.0);
                dialog.settings.insert(key, Value::String(value));
            }
        }
    }
    ui.add_space(6.0);
    section(ui, p, t!("ide-dbc-properties"));
    hint(ui, p, t!("ide-dbc-properties-hint"));
    let mut remove = None;
    egui::Grid::new("dbc-properties").num_columns(3).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label(RichText::new(t!("ide-dbc-property-name")).font(theme::regular(12.0)).color(p.text_secondary));
        ui.label(RichText::new(t!("ide-dbc-property-value")).font(theme::regular(12.0)).color(p.text_secondary));
        ui.end_row();
        for (index, (key, value)) in dialog.properties.iter_mut().enumerate() {
            let name = input(ui, p, egui::TextEdit::singleline(key).desired_width(200.0));
            accessibility::name(&name, Role::TextInput, &t!("ide-dbc-property-name"));
            let edited = input(ui, p, egui::TextEdit::singleline(value).desired_width(240.0));
            accessibility::name(&edited, Role::TextInput, &t!("ide-dbc-property-value"));
            if super::vcs::secondary_button(ui, p, &t!("ide-dbc-property-remove")).clicked() {
                remove = Some(index);
            }
            ui.end_row();
        }
    });
    if let Some(index) = remove {
        let (key, _) = dialog.properties.remove(index);
        if !key.trim().is_empty() {
            dialog.removed.push(key.trim().to_owned());
        }
    }
    if super::vcs::secondary_button(ui, p, &t!("ide-dbc-property-add")).clicked() {
        dialog.properties.push((String::new(), String::new()));
    }
}

fn general_page(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    let Some(driver) = dialog.current().cloned() else { return };
    field(ui, p, t!("ide-dbc-name"), &mut dialog.name, 300.0);
    let types = dialog.types.clone();
    combo(ui, p, "type", t!("ide-dbc-type"), &mut dialog.connection_type, &types);
    field(ui, p, t!("ide-dbc-folder"), &mut dialog.folder, 300.0);
    ui.horizontal(|ui| {
        label(ui, p, t!("ide-dbc-scope"));
        ui.add_enabled_ui(dialog.editing.is_none(), |ui| {
            super::form::radio(ui, p, &mut dialog.project, false, &t!("ide-dbc-scope-global"));
            ui.add_enabled_ui(dialog.has_project, |ui| {
                super::form::radio(ui, p, &mut dialog.project, true, &t!("ide-dbc-scope-project"));
            });
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(LABEL_WIDTH + 8.0);
        check(ui, p, &mut dialog.read_only, t!("ide-dbc-read-only"));
    });
    section(ui, p, t!("ide-dbc-navigator"));
    let views: Vec<(String, String)> =
        VIEWS.iter().map(|view| ((*view).to_owned(), t!(&format!("ide-dbc-view-{view}")))).collect();
    combo(ui, p, "view", t!("ide-dbc-navigator-view"), &mut dialog.navigator_view, &views);
    ui.horizontal(|ui| {
        ui.add_space(LABEL_WIDTH + 8.0);
        check(ui, p, &mut dialog.show_system_objects, t!("ide-dbc-show-system"));
    });
    if driver.has("schemas") {
        field(ui, p, t!("ide-dbc-filter-include"), &mut dialog.include_schemas, 300.0);
        field(ui, p, t!("ide-dbc-filter-exclude"), &mut dialog.exclude_schemas, 300.0);
    }
    section(ui, p, t!("ide-dbc-init"));
    if driver.has("transactions") {
        let mut commit = match dialog.auto_commit {
            None => "type".to_owned(),
            Some(true) => "on".to_owned(),
            Some(false) => "off".to_owned(),
        };
        combo(
            ui,
            p,
            "autocommit",
            t!("ide-dbc-auto-commit"),
            &mut commit,
            &[
                ("type".to_owned(), t!("ide-dbc-auto-commit-type")),
                ("on".to_owned(), t!("ide-dbc-on")),
                ("off".to_owned(), t!("ide-dbc-off")),
            ],
        );
        dialog.auto_commit = match commit.as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
    }
    let levels = driver.isolation_levels();
    if !levels.is_empty() {
        let mut choices = vec![(String::new(), t!("ide-dbc-isolation-default"))];
        choices.extend(levels.iter().map(|level| (level.clone(), level.clone())));
        let mut isolation = dialog.isolation.clone().unwrap_or_default();
        combo(ui, p, "isolation", t!("ide-dbc-isolation"), &mut isolation, &choices);
        dialog.isolation = (!isolation.is_empty()).then_some(isolation);
    }
    if driver.has("schemas") {
        field(ui, p, t!("ide-dbc-default-schema"), &mut dialog.default_schema, 250.0);
    }
    if driver.settings().iter().any(|setting| setting["key"] == KEEP_ALIVE) {
        let mut seconds = dialog.settings.get(KEEP_ALIVE).map(text).unwrap_or_default();
        field(ui, p, t!("ide-dbc-keep-alive"), &mut seconds, 70.0);
        dialog.settings.insert(KEEP_ALIVE.to_owned(), Value::String(seconds));
    }
    ui.horizontal_top(|ui| {
        label(ui, p, t!("ide-dbc-bootstrap"));
        let response = input(
            ui,
            p,
            egui::TextEdit::multiline(&mut dialog.bootstrap)
                .hint_text(t!("ide-dbc-bootstrap-hint"))
                .desired_rows(3)
                .desired_width(420.0)
                .code_editor(),
        );
        accessibility::name(&response, Role::MultilineTextInput, &t!("ide-dbc-bootstrap"));
    });
}

fn family_glyph(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, key_value: bool) {
    if !key_value {
        super::glyphs::glyph_database(painter, rect, color);
        return;
    }
    let stroke = egui::Stroke::new(1.2, color);
    let width = rect.width();
    for step in 0..3 {
        let y = rect.top() + rect.height() * (0.2 + 0.3 * step as f32);
        painter.circle_filled(egui::pos2(rect.left() + width * 0.15, y), width * 0.09, color);
        painter.line_segment([egui::pos2(rect.left() + width * 0.38, y), egui::pos2(rect.right(), y)], stroke);
    }
}

fn drivers(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    ui.vertical(|ui| {
        ui.set_width(DRIVERS_WIDTH);
        let search = input(
            ui,
            p,
            egui::TextEdit::singleline(&mut dialog.search).hint_text(t!("ide-dbc-search")).desired_width(DRIVERS_WIDTH),
        );
        accessibility::name(&search, Role::SearchInput, &t!("ide-dbc-search"));
        ui.add_space(6.0);
        let mut chosen = None;
        let rows = dialog.rows();
        egui::ScrollArea::vertical().id_salt("dbc-drivers").max_height(PAGE_HEIGHT).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if rows.is_empty() {
                    hint(ui, p, t!("ide-dbc-search-nothing"));
                }
                for row in rows {
                    let (title, selected, key_value) = match row {
                        Row::Family(number) => {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(dialog.family_title(number))
                                    .font(theme::semibold(11.0))
                                    .color(p.text_secondary),
                            );
                            continue;
                        }
                        Row::Driver(index) => {
                            let driver = &dialog.drivers[index];
                            let key_value = driver.capabilities["models"][0] == "keyValue";
                            (driver.name.clone(), index == dialog.driver && dialog.profile.is_none(), key_value)
                        }
                        Row::Profile(index) => {
                            let profile = &dialog.profiles[index];
                            let key_value = profile.family == "redis";
                            (profile.name.clone(), dialog.profile == Some(index), key_value)
                        }
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(DRIVERS_WIDTH - 8.0, ROW_HEIGHT), Sense::click());
                    if selected {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.hover);
                    }
                    let color = if dialog.editing.is_some() && !selected { p.text_secondary } else { p.text };
                    let icon = egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(12.0, 12.0));
                    family_glyph(ui.painter(), icon, p.text_secondary, key_value);
                    ui.painter().text(
                        rect.left_center() + vec2(28.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &title,
                        theme::regular(13.0),
                        color,
                    );
                    accessibility::item(&response, Role::ListItem, selected, &title);
                    if selected && std::mem::take(&mut dialog.reveal) {
                        response.scroll_to_me(Some(Align::Center));
                    }
                    if response.clicked() && dialog.editing.is_none() && !selected {
                        chosen = Some(row);
                    }
                }
            },
        );
        match chosen {
            Some(Row::Driver(index)) => dialog.choose_driver(index),
            Some(Row::Profile(index)) => dialog.choose_profile(index),
            _ => {}
        }
    });
}

fn tabs(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) {
    let files = dialog.current().is_some_and(Driver::opens_files);
    if files && dialog.page == Page::SshSsl {
        dialog.page = Page::Main;
    }
    ui.horizontal(|ui| {
        for page in Page::ALL.into_iter().filter(|page| !(files && *page == Page::SshSsl)) {
            let selected = dialog.page == page;
            let title = page.title();
            let color = if selected { p.text } else { p.text_secondary };
            let galley = ui.painter().layout_no_wrap(title.clone(), theme::regular(13.0), color);
            let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 28.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
            if selected {
                let line = egui::Rect::from_min_max(rect.left_bottom() - vec2(0.0, 2.0), rect.right_bottom());
                ui.painter().rect_filled(line, 0, p.accent);
            }
            accessibility::item(&response, Role::Tab, selected, &title);
            if response.clicked() {
                dialog.page = page;
            }
        }
    });
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut ConnectionDialog) -> Option<Choice> {
    let mut choice = None;
    let title = if dialog.editing.is_some() { t!("ide-dbc-title-edit") } else { t!("ide-dbc-title-new") };
    let modal = egui::Modal::new(ui.scope_id().with("connection-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            drivers(ui, p, dialog);
            ui.add_space(8.0);
            let (line, _) = ui.allocate_exact_size(vec2(1.0, PAGE_HEIGHT + 34.0), Sense::hover());
            ui.painter().rect_filled(line, 0, p.popup_border);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                tabs(ui, p, dialog);
                ui.add_space(4.0);
                super::form::solid_scroll_bars(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("dbc-page", dialog.page as u8))
                        .max_height(PAGE_HEIGHT)
                        .min_scrolled_height(PAGE_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_height(PAGE_HEIGHT);
                            ui.set_width(WIDTH - DRIVERS_WIDTH - 52.0);
                            match dialog.page {
                                Page::Main => main_page(ui, p, dialog),
                                Page::SshSsl => ssh_page(ui, p, dialog),
                                Page::Advanced => advanced_page(ui, p, dialog),
                                Page::General => general_page(ui, p, dialog),
                            }
                        });
                });
            });
        });
        if let Some(error) = &dialog.error {
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        if let Some(found) = dialog.detected_profile.clone()
            && let Some(profile) = dialog.profiles.iter().find(|profile| profile.id == found).cloned()
        {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!("ide-dbc-detected-profile", profile = profile.name.as_str()))
                        .font(theme::regular(12.0))
                        .color(p.text_secondary),
                );
                let link = ui.link(
                    RichText::new(t!("ide-dbc-use-profile", profile = profile.name.as_str()))
                        .font(theme::regular(12.0))
                        .color(p.link),
                );
                if link.clicked() {
                    dialog.set_profile(Some(&found));
                }
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let running = matches!(dialog.test, Some(Test::Running(_)));
            let test = super::vcs::secondary_button(ui, p, &t!("ide-dbc-test"));
            if test.clicked() && !running {
                choice = Some(Choice::Test);
            }
            match &dialog.test {
                Some(Test::Running(_)) => {
                    ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                    ui.label(RichText::new(t!("ide-dbc-testing")).font(theme::regular(12.0)).color(p.text_secondary));
                    let stop =
                        ui.link(RichText::new(t!("ide-dbc-test-cancel")).font(theme::regular(12.0)).color(p.link));
                    if stop.clicked() {
                        choice = Some(Choice::CancelTest);
                    }
                }
                Some(Test::Passed(text)) => {
                    ui.add(
                        egui::Label::new(RichText::new(text.clone()).font(theme::regular(12.0)).color(p.vcs_added))
                            .truncate(),
                    );
                }
                Some(Test::Failed(text)) => {
                    ui.add(
                        egui::Label::new(RichText::new(text.clone()).font(theme::regular(12.0)).color(p.error_text))
                            .wrap(),
                    );
                }
                None => {}
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if super::vcs::primary_button(ui, p, &t!("ide-dbc-ok")).clicked() {
                    choice = Some(Choice::Ok);
                }
                if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    choice = Some(Choice::Cancel);
                }
                if dialog.editing.is_none() {
                    ui.add_space(8.0);
                    check(ui, p, &mut dialog.connect_after, t!("ide-dbc-connect-after"));
                }
            });
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
            choice = Some(Choice::Ok);
        }
    });
    let fresh = std::mem::replace(&mut dialog.fresh, false);
    choice.or_else(|| (modal.should_close() && !fresh).then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests;
