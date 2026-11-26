use serde_json::{Value, json};
use zen_core::{CommandError, Session};
use zen_i18n::t;
use zen_run::{Configuration, KindSpec, RunConfigurations, provider_for};

use crate::debug::SessionStarted;
use crate::run::{
    KindInfo, Launch, Prompts, RERUN_CONFIRMATION, RunStarted, Runs, project_root, read_file, run_context, start_with,
};

pub const KIND: &str = "compound";

pub const PROVIDER: &str = "run";

pub const MEMBERS: &str = "configurations";

pub const FOREIGN_TYPE: &str = "CompoundRunConfigurationType";

pub fn kind() -> KindInfo {
    KindInfo {
        provider: PROVIDER.to_owned(),
        spec: KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-run-kind-compound".to_owned(),
            fields: Vec::new(),
            defaults: json!({ MEMBERS: [] }),
        },
    }
}

pub fn is_compound(configuration: &Configuration) -> bool {
    configuration.kind == KIND
}

pub fn members(configuration: &Configuration) -> Vec<String> {
    configuration
        .options
        .get(MEMBERS)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub fn set_members(configuration: &mut Configuration, ids: &[String]) {
    if !configuration.options.is_object() {
        configuration.options = json!({});
    }
    if let Some(options) = configuration.options.as_object_mut() {
        options.insert(MEMBERS.to_owned(), json!(ids));
    }
}

pub fn cycle(configurations: &[Configuration], start: &Configuration) -> Option<String> {
    fn visit<'a>(
        configurations: &'a [Configuration],
        start: &'a Configuration,
        current: &'a Configuration,
        path: &mut Vec<&'a str>,
    ) -> Option<String> {
        if !is_compound(current) {
            return None;
        }
        if path.contains(&current.id.as_str()) {
            return Some(current.name.clone());
        }
        path.push(&current.id);
        for id in members(current) {
            let member = if id == start.id && !start.id.is_empty() {
                Some(start)
            } else {
                configurations.iter().find(|kept| kept.id == id)
            };
            if let Some(found) = member.and_then(|member| visit(configurations, start, member, path)) {
                return Some(found);
            }
        }
        path.pop();
        None
    }
    visit(configurations, start, start, &mut Vec::new())
}

pub fn check(file: &RunConfigurations, configuration: &Configuration) -> Result<(), CommandError> {
    match cycle(&file.configurations, configuration) {
        Some(name) => Err(CommandError::new(t!(
            "workbench-run-compound-cycle",
            name = configuration.name.as_str(),
            member = name
        ))),
        None => Ok(()),
    }
}

pub fn expand(file: &RunConfigurations, compound: &Configuration) -> Result<Vec<Configuration>, CommandError> {
    check(file, compound)?;
    let mut expanded: Vec<Configuration> = Vec::new();
    let mut pending: Vec<(String, String)> =
        members(compound).into_iter().rev().map(|id| (compound.name.clone(), id)).collect();
    while let Some((owner, id)) = pending.pop() {
        let member = file.get(&id).cloned().ok_or_else(|| {
            CommandError::new(t!("workbench-run-compound-missing", name = owner.as_str(), id = id.as_str()))
        })?;
        if is_compound(&member) {
            pending.extend(members(&member).into_iter().rev().map(|inner| (member.name.clone(), inner)));
        } else if !expanded.iter().any(|kept| kept.id == member.id) {
            expanded.push(member);
        }
    }
    if expanded.is_empty() {
        return Err(CommandError::new(t!("workbench-run-compound-empty", name = compound.name.as_str())));
    }
    Ok(expanded)
}

fn expand_in_session(session: &mut Session, compound: &Configuration) -> Result<Vec<Configuration>, CommandError> {
    let root = project_root(session)?;
    let file = read_file(session, &root)?;
    expand(&file, compound)
}

fn guard_members(session: &mut Session, members: &[Configuration]) -> Result<(), CommandError> {
    let root = project_root(session)?;
    for member in members {
        crate::trust::guard_run(session, &root, member)?;
    }
    Ok(())
}

pub(crate) fn start(
    session: &mut Session,
    compound: Configuration,
    launch: Launch,
    chain: &[String],
) -> Result<RunStarted, CommandError> {
    let members = expand_in_session(session, &compound)?;
    guard_members(session, &members)?;
    if launch.confirm_rerun {
        let runs = session.state_mut::<Runs>();
        let busy = members
            .iter()
            .any(|member| !member.common.allow_multiple_instances && !runs.instances(&member.id).is_empty());
        if busy {
            return Err(CommandError::with_code(
                RERUN_CONFIRMATION,
                t!("workbench-run-already-running", name = compound.name.as_str()),
            ));
        }
    }
    let mut children = Vec::new();
    let mut errors = Vec::new();
    for member in members {
        let own = Launch { prompts: launch.prompts.clone(), ..Launch::default() };
        match start_with(session, member, own, chain) {
            Ok(started) => {
                session.state_mut::<Runs>().set_compound(started.run_id, &compound.id);
                children.push(started);
            }
            Err(error) => errors.push(error.message),
        }
    }
    let Some(first) = children.first() else {
        return Err(CommandError::new(errors.join("\n")));
    };
    Ok(RunStarted { run_id: first.run_id, summary: None, children, errors })
}

fn debuggable(session: &mut Session, member: &Configuration) -> bool {
    let Ok(root) = project_root(session) else { return false };
    let context = run_context(session, &root);
    let providers = session.state::<Runs>().map(|runs| runs.providers().to_vec()).unwrap_or_default();
    provider_for(&providers, &member.kind).is_some_and(|provider| provider.debug(member, &context).is_some())
}

pub(crate) fn debug(
    session: &mut Session,
    compound: Configuration,
    prompts: &Prompts,
) -> Result<SessionStarted, CommandError> {
    let members = expand_in_session(session, &compound)?;
    guard_members(session, &members)?;
    let mut first: Option<SessionStarted> = None;
    let mut runs = Vec::new();
    let mut errors = Vec::new();
    let (debugged, ran): (Vec<Configuration>, Vec<Configuration>) =
        members.into_iter().partition(|member| debuggable(session, member));
    if debugged.is_empty() {
        return Err(CommandError::new(t!("workbench-run-compound-no-debug", name = compound.name.as_str())));
    }
    for member in debugged {
        match crate::debug::start(session, member, prompts) {
            Ok(started) => {
                session.state_mut::<Runs>().set_compound(started.run_id, &compound.id);
                runs.push(started.run_id);
                first.get_or_insert(started);
            }
            Err(error) => errors.push(error.message),
        }
    }
    for member in ran {
        let launch = Launch { prompts: prompts.clone(), ..Launch::default() };
        match start_with(session, member, launch, &[]) {
            Ok(started) => {
                session.state_mut::<Runs>().set_compound(started.run_id, &compound.id);
                runs.push(started.run_id);
            }
            Err(error) => errors.push(error.message),
        }
    }
    match first {
        Some(started) => Ok(SessionStarted { runs, errors, ..started }),
        None => Err(CommandError::new(errors.join("\n"))),
    }
}

#[cfg(test)]
mod tests;
