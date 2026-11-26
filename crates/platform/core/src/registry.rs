use std::any::Any;
use std::collections::BTreeMap;

use crate::macros::{self, Recorder};
use crate::{Command, CommandError, CommandId, CommandInfo, DispatchError, RegistryError, Session};

trait ErasedCommand: Send + Sync {
    fn is_available(&self, session: &Session) -> bool;
    fn dispatch(&self, session: &mut Session, params: serde_json::Value) -> Result<serde_json::Value, DispatchError>;
    fn as_any(&self) -> &dyn Any;
}

struct Entry<C: Command> {
    id: CommandId,
    command: C,
}

impl<C: Command> ErasedCommand for Entry<C> {
    fn is_available(&self, session: &Session) -> bool {
        self.command.is_available(session)
    }

    fn dispatch(&self, session: &mut Session, params: serde_json::Value) -> Result<serde_json::Value, DispatchError> {
        let params = if params.is_null() { serde_json::Value::Object(Default::default()) } else { params };
        let params: C::Params = serde_json::from_value(params)
            .map_err(|error| DispatchError::InvalidParams { id: self.id.clone(), message: error.to_string() })?;
        let output = self
            .command
            .run(session, params)
            .map_err(|error: CommandError| DispatchError::Failed { id: self.id.clone(), error })?;
        serde_json::to_value(output)
            .map_err(|error| DispatchError::InvalidOutput { id: self.id.clone(), message: error.to_string() })
    }

    fn as_any(&self) -> &dyn Any {
        &self.command
    }
}

struct Registered {
    info: CommandInfo,
    command: Box<dyn ErasedCommand>,
}

#[derive(Default)]
pub struct Registry {
    commands: BTreeMap<CommandId, Registered>,
    aliases: BTreeMap<CommandId, CommandId>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<C: Command>(&mut self, command: C) -> Result<(), RegistryError> {
        let id = CommandId::new(C::ID)?;
        if self.commands.contains_key(&id) || self.aliases.contains_key(&id) {
            return Err(RegistryError::Duplicate(id));
        }
        let mut schema = serde_json::to_value(schemars::schema_for!(C::Params)).unwrap_or(serde_json::Value::Null);
        join_description_lines(&mut schema);
        let info = CommandInfo {
            id: id.clone(),
            title: C::TITLE.to_owned(),
            description: C::DESCRIPTION.to_owned(),
            params_schema: schema,
            aliases: Vec::new(),
        };
        self.commands.insert(id.clone(), Registered { info, command: Box::new(Entry { id, command }) });
        Ok(())
    }

    pub fn alias(&mut self, old: &str, target: &str) -> Result<(), RegistryError> {
        let old = CommandId::new(old)?;
        let target = CommandId::new(target)?;
        if self.commands.contains_key(&old) || self.aliases.contains_key(&old) {
            return Err(RegistryError::Duplicate(old));
        }
        let Some(registered) = self.commands.get_mut(&target) else {
            return Err(RegistryError::UnknownTarget(target));
        };
        registered.info.aliases.push(old.clone());
        self.aliases.insert(old, target);
        Ok(())
    }

    pub fn resolve(&self, id: &str) -> Option<&CommandId> {
        let id = CommandId::new(id).ok()?;
        if let Some((registered, _)) = self.commands.get_key_value(&id) {
            return Some(registered);
        }
        self.aliases.get(&id)
    }

    pub fn is_deprecated(&self, id: &str) -> bool {
        CommandId::new(id).is_ok_and(|id| self.aliases.contains_key(&id))
    }

    pub fn commands(&self) -> impl Iterator<Item = &CommandInfo> {
        self.commands.values().map(|registered| &registered.info)
    }

    pub fn info(&self, id: &str) -> Option<&CommandInfo> {
        let id = self.resolve(id)?;
        self.commands.get(id).map(|registered| &registered.info)
    }

    pub fn is_available(&self, session: &Session, id: &str) -> bool {
        self.resolve(id).and_then(|id| self.commands.get(id)).is_some_and(|r| r.command.is_available(session))
    }

    pub fn dispatch(
        &self,
        session: &mut Session,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DispatchError> {
        let registered = self.lookup(id)?;
        let recorded =
            (macros::is_recorded(registered.info.id.as_str()) && macros::is_recording(session)).then(|| params.clone());
        let output = self.run_registered(registered, session, params)?;
        if let Some(params) = recorded {
            session.state_mut::<Recorder>().record(registered.info.id.as_str(), params);
        }
        self.play_queued(session, &registered.info.id)?;
        Ok(output)
    }

    pub fn dispatch_unrecorded(
        &self,
        session: &mut Session,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DispatchError> {
        let registered = self.lookup(id)?;
        let output = self.run_registered(registered, session, params)?;
        self.play_queued(session, &registered.info.id)?;
        Ok(output)
    }

    fn run_registered(
        &self,
        registered: &Registered,
        session: &mut Session,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DispatchError> {
        if !registered.command.is_available(session) {
            return Err(DispatchError::Unavailable { id: registered.info.id.clone() });
        }
        registered.command.dispatch(session, params)
    }

    fn play_queued(&self, session: &mut Session, id: &CommandId) -> Result<(), DispatchError> {
        if !session.state::<Recorder>().is_some_and(Recorder::has_playback) {
            return Ok(());
        }
        let Some((steps, finish)) = session.state_mut::<Recorder>().take_playback() else { return Ok(()) };
        session.state_mut::<Recorder>().set_playing(true);
        let mut failed = None;
        for (index, step) in steps.iter().enumerate() {
            if let Err(error) = self.dispatch_unrecorded(session, &step.command, step.params.clone()) {
                failed = Some((index, step, error));
                break;
            }
        }
        session.state_mut::<Recorder>().set_playing(false);
        if let Some(finish) = finish {
            finish(session);
        }
        let Some((index, step, error)) = failed else { return Ok(()) };
        let message = match error {
            DispatchError::Failed { error, .. } => error.message,
            other => other.to_string(),
        };
        let error = CommandError::with_code(macros::STEP_FAILED, message)
            .with_data(serde_json::json!({ "step": index, "command": step.command }));
        Err(DispatchError::Failed { id: id.clone(), error })
    }

    pub fn run<C: Command>(&self, session: &mut Session, params: C::Params) -> Result<C::Output, DispatchError> {
        let registered = self.lookup(C::ID)?;
        let id = registered.info.id.clone();
        let Some(command) = registered.command.as_any().downcast_ref::<C>() else {
            return Err(DispatchError::UnknownCommand { id: C::ID.to_owned() });
        };
        if !command.is_available(session) {
            return Err(DispatchError::Unavailable { id });
        }
        command.run(session, params).map_err(|error| DispatchError::Failed { id, error })
    }

    fn lookup(&self, id: &str) -> Result<&Registered, DispatchError> {
        self.resolve(id)
            .and_then(|resolved| self.commands.get(resolved))
            .ok_or_else(|| DispatchError::UnknownCommand { id: id.to_owned() })
    }
}

fn join_description_lines(schema: &mut serde_json::Value) {
    match schema {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                match value {
                    serde_json::Value::String(text) if key == "description" => {
                        *text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    }
                    _ => join_description_lines(value),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(join_description_lines),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::NoParams;

    #[derive(Default)]
    struct Counter(i64);

    struct Add;

    #[derive(Deserialize, JsonSchema)]
    struct AddParams {
        amount: i64,
    }

    #[derive(Serialize, Debug, PartialEq)]
    struct Total {
        total: i64,
    }

    impl Command for Add {
        type Params = AddParams;
        type Output = Total;
        const ID: &'static str = "test.counter.add";
        const TITLE: &'static str = "Add";
        const DESCRIPTION: &'static str = "Adds an amount to the counter.";

        fn run(&self, session: &mut Session, params: AddParams) -> Result<Total, CommandError> {
            if params.amount < 0 {
                return Err(CommandError::new("the amount must not be negative"));
            }
            let counter = session.state_mut::<Counter>();
            counter.0 += params.amount;
            Ok(Total { total: counter.0 })
        }
    }

    struct Reset;

    impl Command for Reset {
        type Params = NoParams;
        type Output = ();
        const ID: &'static str = "test.counter.reset";
        const TITLE: &'static str = "Reset";
        const DESCRIPTION: &'static str = "Resets the counter; available only when it is not zero.";

        fn is_available(&self, session: &Session) -> bool {
            session.state::<Counter>().is_some_and(|counter| counter.0 != 0)
        }

        fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
            session.state_mut::<Counter>().0 = 0;
            Ok(())
        }
    }

    struct BadId;

    impl Command for BadId {
        type Params = NoParams;
        type Output = ();
        const ID: &'static str = "Bad.id";
        const TITLE: &'static str = "";
        const DESCRIPTION: &'static str = "";

        fn run(&self, _: &mut Session, _: NoParams) -> Result<(), CommandError> {
            Ok(())
        }
    }

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register(Add).unwrap();
        registry.register(Reset).unwrap();
        registry
    }

    #[test]
    fn dispatch_by_id_with_json() {
        let registry = registry();
        let mut session = Session::new();
        assert_eq!(
            registry.dispatch(&mut session, "test.counter.add", json!({ "amount": 2 })),
            Ok(json!({ "total": 2 }))
        );
        assert_eq!(
            registry.dispatch(&mut session, "test.counter.add", json!({ "amount": 3 })),
            Ok(json!({ "total": 5 }))
        );
    }

    #[test]
    fn typed_run() {
        let registry = registry();
        let mut session = Session::new();
        assert_eq!(registry.run::<Add>(&mut session, AddParams { amount: 4 }), Ok(Total { total: 4 }));
    }

    #[test]
    fn structured_errors() {
        let registry = registry();
        let mut session = Session::new();
        assert_eq!(
            registry.dispatch(&mut session, "test.counter.nope", json!({})),
            Err(DispatchError::UnknownCommand { id: "test.counter.nope".into() })
        );
        assert!(matches!(
            registry.dispatch(&mut session, "test.counter.add", json!({ "amount": "x" })),
            Err(DispatchError::InvalidParams { .. })
        ));
        assert_eq!(
            registry.dispatch(&mut session, "test.counter.add", json!({ "amount": -1 })),
            Err(DispatchError::Failed {
                id: CommandId::new("test.counter.add").unwrap(),
                error: CommandError::new("the amount must not be negative"),
            })
        );
        let error = registry.dispatch(&mut session, "test.counter.nope", json!({})).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({ "kind": "unknownCommand", "id": "test.counter.nope" })
        );
    }

    #[test]
    fn availability_is_checked() {
        let registry = registry();
        let mut session = Session::new();
        assert!(!registry.is_available(&session, "test.counter.reset"));
        assert!(matches!(
            registry.dispatch(&mut session, "test.counter.reset", json!(null)),
            Err(DispatchError::Unavailable { .. })
        ));
        registry.dispatch(&mut session, "test.counter.add", json!({ "amount": 1 })).unwrap();
        assert!(registry.is_available(&session, "test.counter.reset"));
        assert_eq!(registry.dispatch(&mut session, "test.counter.reset", json!(null)), Ok(json!(null)));
    }

    #[test]
    fn duplicates_and_bad_ids_are_rejected() {
        let mut registry = registry();
        assert_eq!(registry.register(Add), Err(RegistryError::Duplicate(CommandId::new("test.counter.add").unwrap())));
        assert!(matches!(registry.register(BadId), Err(RegistryError::InvalidId(_))));
    }

    #[test]
    fn deprecated_aliases_resolve_to_their_target() {
        let mut registry = registry();
        registry.alias("test.counter.increase", "test.counter.add").unwrap();
        let mut session = Session::new();
        assert_eq!(
            registry.dispatch(&mut session, "test.counter.increase", json!({ "amount": 1 })),
            Ok(json!({ "total": 1 }))
        );
        assert!(registry.is_deprecated("test.counter.increase"));
        assert!(!registry.is_deprecated("test.counter.add"));
        assert_eq!(
            registry.info("test.counter.add").unwrap().aliases,
            vec![CommandId::new("test.counter.increase").unwrap()]
        );
        assert_eq!(
            registry.alias("test.counter.add", "test.counter.reset"),
            Err(RegistryError::Duplicate(CommandId::new("test.counter.add").unwrap()))
        );
        assert_eq!(
            registry.alias("test.counter.old", "test.counter.missing"),
            Err(RegistryError::UnknownTarget(CommandId::new("test.counter.missing").unwrap()))
        );
    }

    #[test]
    fn commands_are_listed_with_their_schema() {
        let registry = registry();
        let ids: Vec<&str> = registry.commands().map(|info| info.id.as_str()).collect();
        assert_eq!(ids, ["test.counter.add", "test.counter.reset"]);
        let schema = &registry.info("test.counter.add").unwrap().params_schema;
        assert_eq!(schema["properties"]["amount"]["type"], json!("integer"));
        assert_eq!(schema["required"], json!(["amount"]));
    }

    #[derive(Default)]
    struct Typed(String);

    struct Insert;

    #[derive(Deserialize, JsonSchema)]
    struct InsertParams {
        text: String,
    }

    impl Command for Insert {
        type Params = InsertParams;
        type Output = ();
        const ID: &'static str = "editor.edit.insert";
        const TITLE: &'static str = "Insert";
        const DESCRIPTION: &'static str = "Appends text; fails on `!`.";

        fn run(&self, session: &mut Session, params: InsertParams) -> Result<(), CommandError> {
            if params.text == "!" {
                return Err(CommandError::new("no exclamations"));
            }
            session.state_mut::<Typed>().0.push_str(&params.text);
            Ok(())
        }
    }

    struct Play;

    #[derive(Default)]
    struct Finished(usize);

    impl Command for Play {
        type Params = Vec<macros::MacroStep>;
        type Output = usize;
        const ID: &'static str = "test.macro.play";
        const TITLE: &'static str = "Play";
        const DESCRIPTION: &'static str = "Plays the steps given.";

        fn run(&self, session: &mut Session, steps: Vec<macros::MacroStep>) -> Result<usize, CommandError> {
            let count = steps.len();
            let finish: macros::Finish = Box::new(|session: &mut Session| session.state_mut::<Finished>().0 += 1);
            session.state_mut::<Recorder>().play(steps, Some(finish));
            Ok(count)
        }
    }

    #[test]
    fn recorded_commands_are_kept_while_recording_and_played_back_after_the_command() {
        let mut registry = registry();
        registry.register(Insert).unwrap();
        registry.register(Play).unwrap();
        let mut session = Session::new();
        let insert = |text: &str| json!({ "text": text });
        registry.dispatch(&mut session, "editor.edit.insert", insert("a")).unwrap();
        session.state_mut::<Recorder>().start();
        registry.dispatch(&mut session, "editor.edit.insert", insert("b")).unwrap();
        registry.dispatch(&mut session, "test.counter.add", json!({ "amount": 1 })).unwrap();
        registry.dispatch_unrecorded(&mut session, "editor.edit.insert", insert("c")).unwrap();
        assert!(registry.dispatch(&mut session, "editor.edit.insert", insert("!")).is_err());
        let steps = session.state_mut::<Recorder>().stop().unwrap();
        assert_eq!(steps, vec![macros::MacroStep::new("editor.edit.insert", insert("b"))]);
        assert_eq!(session.state::<Typed>().unwrap().0, "abc");

        let twice = json!([steps[0], steps[0]]);
        assert_eq!(registry.dispatch(&mut session, "test.macro.play", twice), Ok(json!(2)));
        assert_eq!(session.state::<Typed>().unwrap().0, "abcbb");
        assert_eq!(session.state::<Finished>().unwrap().0, 1);
        assert!(!session.state::<Recorder>().unwrap().is_playing());

        let failing = json!([{ "command": "editor.edit.insert", "params": { "text": "x" } },
            { "command": "editor.edit.insert", "params": { "text": "!" } },
            { "command": "editor.edit.insert", "params": { "text": "y" } }]);
        let Err(DispatchError::Failed { id, error }) = registry.dispatch(&mut session, "test.macro.play", failing)
        else {
            panic!("the failed step stops the playback");
        };
        assert_eq!(id.as_str(), "test.macro.play");
        assert_eq!(error.message, "no exclamations");
        assert_eq!(error.code.as_deref(), Some(macros::STEP_FAILED));
        assert_eq!(error.data.unwrap()["step"], json!(1));
        assert_eq!(session.state::<Typed>().unwrap().0, "abcbbx");
        assert_eq!(session.state::<Finished>().unwrap().0, 2, "what ran is finished too");
    }
}
