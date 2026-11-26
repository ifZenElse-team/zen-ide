use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{Common, Configuration, Template};

use crate::run::{KindInfo, Runs, check_options, project_root, read_file, write_file};

/// A kind's template, as the Edit Configuration Templates page shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    /// The kind and its provider.
    #[serde(flatten)]
    pub kind: KindInfo,
    /// Whether the project keeps a template of its own for the kind; otherwise `template` is the kind's defaults.
    pub stored: bool,
    /// The settings a new configuration of the kind starts with.
    pub template: Template,
}

/// What `run.template.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateList {
    /// One per kind the IDE can run, in the order of `run.configuration.list`'s kinds.
    pub templates: Vec<TemplateInfo>,
}

pub fn defaults(kind: &KindInfo) -> Template {
    Template { kind: kind.spec.id.clone(), common: Common::default(), options: kind.spec.defaults.clone() }
}

fn templated_kinds(session: &Session) -> Vec<KindInfo> {
    let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
    kinds.into_iter().filter(|kind| kind.spec.id != crate::compound::KIND).collect()
}

fn known_kind(session: &Session, kind: &str) -> Result<KindInfo, CommandError> {
    templated_kinds(session)
        .into_iter()
        .find(|known| known.spec.id == kind)
        .ok_or_else(|| CommandError::new(t!("workbench-run-template-unknown-kind", kind = kind)))
}

pub struct ListTemplates;

impl Command for ListTemplates {
    type Params = NoParams;
    type Output = TemplateList;
    const ID: &'static str = "run.template.list";
    const TITLE: &'static str = "Run Configuration Templates";
    const DESCRIPTION: &'static str = "The template of every kind of run configuration (Edit Configuration \
         Templates): the settings a new configuration of the kind, and a temporary one made by running a context, \
         start from. The project's own are kept in .zen/runConfigurations.json under templates; a kind without one \
         answers its defaults with stored: false.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TemplateList, CommandError> {
        let root = project_root(session)?;
        let file = read_file(session, &root)?;
        let templates = templated_kinds(session)
            .into_iter()
            .map(|kind| {
                let stored = file.template(&kind.spec.id).cloned();
                let template = stored.clone().unwrap_or_else(|| defaults(&kind));
                TemplateInfo { kind, stored: stored.is_some(), template }
            })
            .collect();
        Ok(TemplateList { templates })
    }
}

/// The parameters of `run.template.update`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTemplateParams {
    /// The template, replacing the project's template of its kind.
    pub template: Template,
}

pub struct UpdateTemplate;

impl Command for UpdateTemplate {
    type Params = UpdateTemplateParams;
    type Output = Template;
    const ID: &'static str = "run.template.update";
    const TITLE: &'static str = "Change Run Configuration Template";
    const DESCRIPTION: &'static str = "Sets the project's template of a kind of run configuration and saves the \
         file; configurations made afterwards start from it, the existing ones do not change.";

    fn run(&self, session: &mut Session, params: UpdateTemplateParams) -> Result<Template, CommandError> {
        let root = project_root(session)?;
        known_kind(session, &params.template.kind)?;
        let probe = Configuration {
            kind: params.template.kind.clone(),
            options: params.template.options.clone(),
            ..Configuration::default()
        };
        check_options(session, &probe)?;
        let mut file = read_file(session, &root)?;
        file.set_template(params.template.clone());
        write_file(session, &root, &file)?;
        Ok(params.template)
    }
}

/// A kind of run configuration.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KindParams {
    /// The kind, e.g. `go.test`.
    pub kind: String,
}

pub struct ResetTemplate;

impl Command for ResetTemplate {
    type Params = KindParams;
    type Output = Template;
    const ID: &'static str = "run.template.reset";
    const TITLE: &'static str = "Reset Run Configuration Template";
    const DESCRIPTION: &'static str = "Removes the project's template of a kind, so that new configurations of it \
         start from the kind's defaults again; answers those defaults.";

    fn run(&self, session: &mut Session, params: KindParams) -> Result<Template, CommandError> {
        let root = project_root(session)?;
        let kind = known_kind(session, &params.kind)?;
        let mut file = read_file(session, &root)?;
        if file.template(&params.kind).is_some() {
            file.templates.retain(|kept| kept.kind != params.kind);
            write_file(session, &root, &file)?;
        }
        Ok(defaults(&kind))
    }
}

/// The parameters of `run.configuration.new`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewParams {
    /// The kind, e.g. `go.test`.
    pub kind: String,
    /// The name; empty when absent.
    #[serde(default)]
    pub name: String,
}

pub struct NewConfiguration;

impl Command for NewConfiguration {
    type Params = NewParams;
    type Output = Configuration;
    const ID: &'static str = "run.configuration.new";
    const TITLE: &'static str = "New Run Configuration";
    const DESCRIPTION: &'static str = "A configuration of a kind as the + of the Run/Debug Configurations dialog \
         makes it: the kind's template, or its defaults, without an id and not saved; save it with \
         run.configuration.create.";

    fn run(&self, session: &mut Session, params: NewParams) -> Result<Configuration, CommandError> {
        let root = project_root(session)?;
        let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
        let kind = kinds
            .into_iter()
            .find(|known| known.spec.id == params.kind)
            .ok_or_else(|| CommandError::new(t!("workbench-run-template-unknown-kind", kind = params.kind.as_str())))?;
        let file = read_file(session, &root)?;
        let mut options = kind.spec.defaults.as_object().cloned().unwrap_or_default();
        let mut common = Common::default();
        if let Some(template) = file.template(&kind.spec.id) {
            options.extend(template.options.as_object().cloned().unwrap_or_default());
            common = template.common.clone();
        }
        Ok(Configuration {
            name: params.name,
            kind: kind.spec.id.clone(),
            common,
            options: serde_json::Value::Object(options),
            ..Configuration::default()
        })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTemplates)?;
    registry.register(UpdateTemplate)?;
    registry.register(ResetTemplate)?;
    registry.register(NewConfiguration)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use zen_run::fake::{FakeProvider, KIND};
    use zen_run::{RunConfigurations, RunEnv};

    use super::*;

    #[test]
    fn a_project_not_trusted_lends_no_template_to_what_runs_from_the_caret() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let mut file = RunConfigurations::default();
        file.set_template(Template {
            kind: KIND.to_owned(),
            common: Common { tool_arguments: "-toolexec=evil".to_owned(), ..Default::default() },
            options: json!({}),
        });
        file.write(dir.path()).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let fake = FakeProvider::new();
        fake.set_context(Some(FakeProvider::configuration("context")));
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake)], RunEnv::default());
        let main = dir.path().join("main.fake");
        std::fs::write(&main, "run me").unwrap();
        let context = registry.dispatch(&mut session, "run.context.get", json!({ "path": main })).unwrap();
        assert_eq!(context["configuration"]["common"]["toolArguments"], "", "{context}");
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let context = registry.dispatch(&mut session, "run.context.get", json!({ "path": main })).unwrap();
        assert_eq!(context["configuration"]["common"]["toolArguments"], "-toolexec=evil", "once trusted");
    }

    #[test]
    fn templates_are_kept_in_the_file_and_new_and_temporary_configurations_start_from_them() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let fake = FakeProvider::new();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());

        let listed = registry.dispatch(&mut session, "run.template.list", json!({})).unwrap();
        let templates = listed["templates"].as_array().unwrap();
        assert_eq!(templates.len(), 1, "a compound has no template: {listed}");
        assert_eq!((templates[0]["id"].as_str(), templates[0]["stored"].as_bool()), (Some(KIND), Some(false)));
        assert_eq!(templates[0]["template"]["options"]["target"], "");

        let template = json!({ "kind": KIND, "common": { "toolArguments": "-race" }, "options": { "target": "tmpl" } });
        registry.dispatch(&mut session, "run.template.update", json!({ "template": template })).unwrap();
        let file = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(file.template(KIND).map(|kept| kept.common.tool_arguments.as_str()), Some("-race"));
        let wrong = json!({ "template": { "kind": KIND, "options": { "nope": 1 } } });
        assert!(registry.dispatch(&mut session, "run.template.update", wrong).is_err());
        let unknown = json!({ "template": { "kind": "nope", "options": {} } });
        assert!(registry.dispatch(&mut session, "run.template.update", unknown).is_err());

        let made =
            registry.dispatch(&mut session, "run.configuration.new", json!({ "kind": KIND, "name": "n" })).unwrap();
        assert_eq!((made["name"].as_str(), made["options"]["target"].as_str()), (Some("n"), Some("tmpl")));
        assert_eq!(made["common"]["toolArguments"], "-race");
        assert_eq!(made["options"]["fail"], json!(null), "the kind's other defaults stay");

        let bare = json!({ "configuration": { "name": "bare", "kind": KIND } });
        let created = registry.dispatch(&mut session, "run.configuration.create", bare).unwrap();
        assert_eq!(created["common"]["toolArguments"], "-race");
        let plain = json!({ "configuration": { "name": "plain", "kind": KIND }, "template": false });
        let created = registry.dispatch(&mut session, "run.configuration.create", plain).unwrap();
        assert_eq!(created["common"]["toolArguments"], "");

        let main = dir.path().join("main.fake");
        std::fs::write(&main, "run me").unwrap();
        fake.set_context(Some(FakeProvider::configuration("context")));
        let context = registry.dispatch(&mut session, "run.context.get", json!({ "path": main })).unwrap();
        assert_eq!(context["configuration"]["common"]["toolArguments"], "-race");
        assert_eq!(context["configuration"]["options"]["target"], "context", "what the context sets wins");
        registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).unwrap();
        registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).unwrap();
        let temporary = session.state::<Runs>().unwrap().temporary().to_vec();
        assert_eq!(temporary.len(), 1, "the temporary one made from the template is reused");
        assert_eq!(temporary[0].common.tool_arguments, "-race");

        let reset = registry.dispatch(&mut session, "run.template.reset", json!({ "kind": KIND })).unwrap();
        assert_eq!(reset["options"]["target"], "");
        let text = std::fs::read_to_string(dir.path().join(zen_project::run::FILE)).unwrap();
        assert!(!text.contains("templates"), "{text}");
    }
}
