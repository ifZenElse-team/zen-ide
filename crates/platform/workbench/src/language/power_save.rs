use std::collections::BTreeMap;

use serde_json::Value;
use zen_lsp::Client;

use super::{Entry, Languages, Server, inlay_hints, pull};
use crate::code_style_options::ServerStyles;

fn own_section(entry: &Entry) -> Option<&str> {
    let hints = entry.declaration.inlay_hints.as_ref().map(|hints| hints.section.as_str());
    hints.or_else(|| entry.declaration.power_save.as_ref().map(|declared| declared.section.as_str()))
}

fn style_of<'a>(entry: &Entry, style: &'a ServerStyles) -> Vec<(&'a String, &'a Value)> {
    entry.declaration.languages.iter().filter_map(|language| style.get(language)).flatten().collect()
}

fn object(value: Option<&Value>) -> Value {
    value.filter(|value| value.is_object()).cloned().unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}

pub(crate) type AnalyzerSwitches = BTreeMap<String, bool>;

fn analyzer_section(entry: &Entry, analyzers: &AnalyzerSwitches, sections: &mut Vec<(String, Value)>, base: &Value) {
    let Some(declared) = &entry.declaration.inspection_settings else { return };
    let switches = declared_switches(entry, analyzers);
    match sections.iter_mut().find(|(section, _)| *section == declared.section) {
        Some((_, settings)) => *settings = declared.settings(settings, &switches),
        None => sections.push((declared.section.clone(), declared.settings(base, &switches))),
    }
}

fn declared_switches<'a>(entry: &Entry, analyzers: &'a AnalyzerSwitches) -> BTreeMap<&'a str, bool> {
    analyzers
        .iter()
        .filter(|(id, _)| entry.declaration.inspections.iter().any(|inspection| inspection.id == **id))
        .map(|(id, on)| (id.as_str(), *on))
        .collect()
}

pub(super) fn sections(
    entry: &Entry,
    base: &Value,
    switches: &inlay_hints::HintSwitches,
    style: &ServerStyles,
    analyzers: &AnalyzerSwitches,
    power_save: bool,
) -> Vec<(String, Value)> {
    let own = own_section(entry);
    let styled = style_of(entry, style);
    let mut own_base = object(Some(base));
    for (section, over) in &styled {
        if Some(section.as_str()) == own {
            zen_project::code_style::merge(&mut own_base, over);
        }
    }
    let base = if own_base == object(Some(base)) { base } else { &own_base };
    let mut sections: Vec<(String, Value)> =
        inlay_hints::section(entry.declaration.inlay_hints.as_ref(), base, switches).into_iter().collect();
    analyzer_section(entry, analyzers, &mut sections, base);
    if let Some(declared) = &entry.declaration.power_save {
        let at = sections.iter().position(|(section, _)| *section == declared.section);
        let current = at.map_or_else(|| base.clone(), |at| sections[at].1.clone());
        let settings = if power_save { declared.over(&current) } else { current };
        match at {
            Some(at) => sections[at].1 = settings,
            None => sections.push((declared.section.clone(), settings)),
        }
    }
    for (section, over) in styled.into_iter().filter(|(section, _)| Some(section.as_str()) != own) {
        match sections.iter_mut().find(|(known, _)| known == section) {
            Some((_, settings)) => zen_project::code_style::merge(settings, over),
            None => {
                let mut settings = object(base.get("settings").and_then(|settings| settings.get(section)));
                zen_project::code_style::merge(&mut settings, over);
                sections.push((section.clone(), settings));
            }
        }
    }
    sections
}

pub(super) fn options(
    entry: &Entry,
    base: &Value,
    switches: &inlay_hints::HintSwitches,
    analyzers: &AnalyzerSwitches,
    power_save: bool,
) -> Value {
    let mut options = inlay_hints::with_switches(entry.declaration.inlay_hints.as_ref(), base, switches);
    if let Some(declared) = &entry.declaration.inspection_settings {
        options = declared.settings(&options, &declared_switches(entry, analyzers));
    }
    match &entry.declaration.power_save {
        Some(declared) if power_save => declared.over(&options),
        _ => options,
    }
}

pub(super) fn configure(
    client: &mut Client,
    entry: &Entry,
    switches: &inlay_hints::HintSwitches,
    style: &ServerStyles,
    analyzers: &AnalyzerSwitches,
    power_save: bool,
) {
    let base = match &entry.declaration.inspection_settings {
        Some(declared) => declared.without(client.initialization_options()),
        None => client.initialization_options().clone(),
    };
    let base = match &entry.declaration.power_save {
        Some(declared) => strip(&base, &declared.settings),
        None => base,
    };
    for (section, settings) in sections(entry, &base, switches, style, analyzers, power_save) {
        if client.configuration(&section) != Some(&settings) {
            let _ = client.set_configuration(&section, settings);
        }
    }
}

fn strip(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            let mut kept = base.clone();
            for (key, value) in over {
                match (kept.get(key), value) {
                    (Some(inner), Value::Object(_)) if inner.is_object() => {
                        let stripped = strip(inner, value);
                        if stripped.as_object().is_some_and(serde_json::Map::is_empty) {
                            kept.remove(key);
                        } else {
                            kept.insert(key.clone(), stripped);
                        }
                    }
                    (Some(inner), _) if inner == value => {
                        kept.remove(key);
                    }
                    _ => {}
                }
            }
            Value::Object(kept)
        }
        _ => base.clone(),
    }
}

impl Languages {
    pub(super) fn set_analyzer_switches(&mut self, mut switches: AnalyzerSwitches) {
        for id in self.analyzer_switches.keys() {
            let declared = self.entries.iter().flat_map(|entry| &entry.declaration.inspections);
            let default = declared.filter(|inspection| inspection.id == *id).map(|inspection| !inspection.off).next();
            if let Some(default) = default {
                switches.entry(id.clone()).or_insert(default);
            }
        }
        self.analyzer_switches = switches;
    }

    pub fn set_power_save(&mut self, on: bool) {
        if self.power_save == on {
            return;
        }
        self.power_save = on;
        if on {
            return;
        }
        for server in self.servers.values_mut() {
            let Server::Running { client, files } = server else { continue };
            if pull::supported(client) {
                continue;
            }
            for synced in files.values() {
                let _ = client.reopen(&synced.uri);
            }
        }
        pull::refresh(&mut self.pulls);
        for pulls in self.alongside_pulls.values_mut() {
            pull::refresh(pulls);
        }
    }

    pub fn power_save(&self) -> bool {
        self.power_save
    }

    pub fn servers_told_to_pause(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter_map(|(id, server)| {
                let Server::Running { client, .. } = server else { return None };
                let entry = self.entries.iter().find(|entry| entry.id() == id)?;
                let declared = entry.declaration.power_save.as_ref()?;
                let told = client.configuration(&declared.section)?;
                (declared.over(told) == *told).then(|| id.clone())
            })
            .collect()
    }

    pub fn server_processes(&self) -> Vec<(String, u32)> {
        self.servers
            .iter()
            .filter_map(|(id, server)| match server {
                Server::Running { client, .. } => Some((id.clone(), client.pid()?)),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::*;
    use crate::language::testing;

    const MAIN: &str = "package main\n\nfunc main() {\n\tx := 1\n}\n";

    fn set_power_save(fixture: &mut testing::Fixture, on: bool) {
        fixture.session.settings_mut().set(crate::power_save::ENABLED, on).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    }

    fn problems(fixture: &mut testing::Fixture, name: &str) -> Value {
        let path = fixture.path(name);
        fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap()
    }

    fn wait_until(fixture: &mut testing::Fixture, mut done: impl FnMut(&mut testing::Fixture) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            fixture.dispatch("language.servers.sync", json!({})).unwrap();
            if done(fixture) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn declare(fixture: &mut testing::Fixture) {
        fixture.with(|languages, _| {
            languages.entries[0].declaration.power_save = Some(zen_lsp::servers::PowerSaveSettings {
                section: "gopls".to_owned(),
                settings: json!({ "diagnosticsTrigger": "Save" }),
            });
        });
    }

    #[test]
    fn a_profiles_analyzer_switches_reach_the_server_and_go_back_to_the_defaults() {
        let mut fixture = testing::fixture(&[("main.go", "package main\n")]);
        fixture.with(|languages, _| {
            let declaration = &mut languages.entries[0].declaration;
            declaration.inspection_settings =
                Some(zen_lsp::servers::InspectionSettings { section: "gopls".to_owned(), path: "analyses".to_owned() });
            declaration.inspections =
                vec![zen_lsp::servers::ServerInspection { id: "shadow".to_owned(), off: true, ..Default::default() }];
        });
        fixture.open("main.go");
        fixture.dispatch_once_running("language.servers.sync", json!({})).unwrap();
        let set = json!({ "inspections": { "shadow": { "enabled": true }, "todo": { "enabled": false } } });
        fixture.dispatch("inspect.profile.set", set).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let told = fixture.server.wait_for("workspace/didChangeConfiguration", 1, Duration::from_secs(5));
        assert_eq!(told[0]["settings"]["gopls"]["analyses"], json!({ "shadow": true }), "{told:?}");
        fixture.dispatch("inspect.profile.restore", json!({})).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let told = fixture.server.wait_for("workspace/didChangeConfiguration", 2, Duration::from_secs(5));
        assert_eq!(
            told[1]["settings"]["gopls"]["analyses"],
            json!({ "shadow": false }),
            "a server keeps what it is not told again"
        );
    }

    fn unused_variable(params: &Value) -> Value {
        json!({
            "uri": params["textDocument"]["uri"],
            "diagnostics": [{
                "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 2 } },
                "severity": 1,
                "message": "declared and not used: x",
            }],
        })
    }

    #[test]
    fn power_save_tells_a_declaring_server_to_pause_and_brings_the_problems_back_when_off() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        declare(&mut fixture);
        fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", unused_variable);
        fixture.open("main.go");
        assert!(wait_until(&mut fixture, |fixture| problems(fixture, "main.go")
            .as_array()
            .is_some_and(|list| list.len() == 1)));
        assert!(!crate::power_save::status(&fixture.session).on);

        set_power_save(&mut fixture, true);
        let told = fixture.server.wait_for("workspace/didChangeConfiguration", 1, Duration::from_secs(5));
        assert_eq!(told.last().unwrap()["settings"]["gopls"]["diagnosticsTrigger"], "Save", "{told:?}");
        let answer = problems(&mut fixture, "main.go");
        assert_eq!(answer["reason"], "powerSave", "{answer}");
        assert_eq!(answer["problems"], json!([]), "no highlighting while it is on");
        let now = crate::power_save::status(&fixture.session);
        assert!(now.on);
        assert_eq!(now.paused, ["diagnostics", "completionPopup", "networkChecks", "reindexing"]);
        assert_eq!(now.servers_paused, ["go"]);
        let opens = fixture.server.requests("textDocument/didOpen").len();

        set_power_save(&mut fixture, false);
        let told = fixture.server.wait_for("workspace/didChangeConfiguration", 2, Duration::from_secs(5));
        assert!(told.last().unwrap()["settings"]["gopls"].get("diagnosticsTrigger").is_none(), "{told:?}");
        assert!(fixture.server.wait_for("textDocument/didOpen", opens + 1, Duration::from_secs(5)).len() > opens);
        assert!(
            wait_until(&mut fixture, |fixture| problems(fixture, "main.go")
                .as_array()
                .is_some_and(|list| list.len() == 1)),
            "the problem is back without reopening the file"
        );
        assert_eq!(fixture.open_files().len(), 1);
        assert!(crate::power_save::status(&fixture.session).servers_paused.is_empty());
    }

    #[test]
    fn a_server_started_in_power_save_mode_starts_paused() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        declare(&mut fixture);
        fixture.session.settings_mut().set(crate::power_save::ENABLED, true).unwrap();
        fixture.open("main.go");
        let initialize = &fixture.server.requests("initialize")[0];
        assert_eq!(initialize["initializationOptions"]["diagnosticsTrigger"], "Save");
        assert_eq!(crate::power_save::status(&fixture.session).servers_paused, ["go"]);
    }

    #[test]
    fn code_style_server_settings_reach_a_running_server_and_leave_at_their_default() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        declare(&mut fixture);
        let declaration = zen_project::code_style::CodeStyleDeclaration::parse(
            r#"{ "language": "go", "title": "t", "options": [ { "id": "gofumpt", "label": "l", "tab": "spaces",
                 "kind": { "type": "boolean" }, "default": false,
                 "serverSetting": { "section": "gopls", "path": "gofumpt" } } ] }"#,
        )
        .unwrap();
        fixture.session.set_state(crate::code_style_options::CodeStyles::new(vec![declaration], Vec::new()));
        fixture.open("main.go");
        fixture.session.settings_mut().set("editor.codeStyle.go.gofumpt", true).unwrap();
        let told = wait_until(&mut fixture, |fixture| {
            fixture
                .server
                .requests("workspace/didChangeConfiguration")
                .iter()
                .any(|told| told["settings"]["gopls"]["gofumpt"] == true)
        });
        assert!(told, "a changed option is told to the running server");
        fixture.session.settings_mut().set("editor.codeStyle.go.gofumpt", false).unwrap();
        let told =
            wait_until(&mut fixture, |fixture| {
                fixture.server.requests("workspace/didChangeConfiguration").iter().any(|told| {
                    told["settings"]["gopls"].is_object() && told["settings"]["gopls"].get("gofumpt").is_none()
                })
            });
        assert!(told, "back at the default, the setting is left to the server");
    }

    #[test]
    fn a_server_that_declares_no_section_gets_the_code_style_section_on_its_initial_settings() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        let entry = fixture.with(|languages, _| languages.entries[0].clone());
        let base = json!({ "settings": { "java": { "import": { "maven": true } } } });
        let style = ServerStyles::from([(
            "go".to_owned(),
            BTreeMap::from([("java".to_owned(), json!({ "format": { "comments": false } }))]),
        )]);
        let found =
            sections(&entry, &base, &inlay_hints::HintSwitches::default(), &style, &AnalyzerSwitches::default(), false);
        assert_eq!(
            found,
            [("java".to_owned(), json!({ "import": { "maven": true }, "format": { "comments": false } }))]
        );
        let other = ServerStyles::from([("rust".to_owned(), BTreeMap::new())]);
        assert!(
            sections(&entry, &base, &inlay_hints::HintSwitches::default(), &other, &AnalyzerSwitches::default(), false)
                .is_empty()
        );
    }

    #[test]
    fn pulled_diagnostics_wait_while_power_save_is_on() {
        let mut fixture = testing::fixture(&[("main.go", MAIN)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "diagnosticProvider": { "interFileDependencies": false, "workspaceDiagnostics": false },
        }));
        fixture.server.on("textDocument/diagnostic", json!({ "kind": "full", "items": [] }));
        fixture.session.settings_mut().set(crate::power_save::ENABLED, true).unwrap();
        fixture.open("main.go");
        for _ in 0..20 {
            fixture.dispatch("language.servers.sync", json!({})).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(fixture.server.requests("textDocument/diagnostic").is_empty(), "nothing is pulled while it is on");
        set_power_save(&mut fixture, false);
        assert!(!fixture.server.wait_for("textDocument/diagnostic", 1, Duration::from_secs(5)).is_empty());
    }

    #[test]
    fn options_started_in_power_save_mode_lose_its_settings_again() {
        let over = json!({ "diagnosticsTrigger": "Save", "ui": { "x": 1 } });
        let started = json!({ "semanticTokens": true, "diagnosticsTrigger": "Save", "ui": { "x": 1, "y": 2 } });
        assert_eq!(strip(&started, &over), json!({ "semanticTokens": true, "ui": { "y": 2 } }));
        let only = json!({ "diagnosticsTrigger": "Save", "ui": { "x": 1 } });
        assert_eq!(strip(&only, &over), json!({}));
        let other = json!({ "diagnosticsTrigger": "Edit" });
        assert_eq!(strip(&other, &over), other, "a value of the user's stays");
    }
}
