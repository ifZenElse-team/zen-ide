use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::keymap::macro_action;
use zen_core::macros::{Finish, MacroStep, Recorder};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::keymaps::{self, SharedKeymaps};

pub const FORMAT_VERSION: u32 = 1;

/// A recorded macro: its name and its steps, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Macro {
    /// The name; empty for the temporary macro kept by a recording stopped without one.
    pub name: String,
    /// The commands it runs, with their parameters.
    pub steps: Vec<MacroStep>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MacroFile {
    format_version: u32,
    #[serde(default)]
    macros: Vec<Macro>,
}

#[derive(Default)]
struct Inner {
    file: Option<PathBuf>,
    saved: Vec<Macro>,
    last: Option<Macro>,
    generation: u64,
    notice: Option<String>,
    keymaps: Option<SharedKeymaps>,
}

impl Inner {
    fn find(&self, name: &str) -> Result<usize, CommandError> {
        self.saved
            .iter()
            .position(|saved| saved.name == name)
            .ok_or_else(|| CommandError::new(t!("workbench-macro-unknown", name = name)))
    }

    fn taken(&self, name: &str) -> Result<(), CommandError> {
        match self.saved.iter().any(|saved| saved.name == name) {
            true => Err(CommandError::new(t!("workbench-macro-exists", name = name))),
            false => Ok(()),
        }
    }

    fn save(&mut self) -> Result<(), CommandError> {
        self.generation += 1;
        if let Some(keymaps) = &self.keymaps {
            keymaps::lock(keymaps).set_macros(self.saved.iter().map(|saved| saved.name.clone()).collect());
        }
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let saved = MacroFile { format_version: FORMAT_VERSION, macros: self.saved.clone() };
        let json = serde_json::to_vec_pretty(&saved).unwrap_or_default();
        zen_vfs::write_atomic(file, &json)
            .map_err(|error| CommandError::new(t!("workbench-macro-save-failed", detail = error.to_string())))
    }

    fn move_shortcuts(&self, from: &str, to: Option<&str>) {
        if let Some(keymaps) = &self.keymaps {
            let to = to.map(macro_action);
            let _ = keymaps::lock(keymaps).move_action(&macro_action(from), to.as_deref());
        }
    }
}

#[derive(Clone, Default)]
pub struct Macros {
    inner: Arc<Mutex<Inner>>,
}

impl Macros {
    pub fn load(file: Option<PathBuf>) -> Self {
        let (saved, notice) = file.as_deref().map_or_else(|| (Vec::new(), None), read);
        Self { inner: Arc::new(Mutex::new(Inner { file, saved, notice, ..Inner::default() })) }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn take_notice(&self) -> Option<String> {
        self.inner().notice.take()
    }

    pub fn set_keymaps(&self, keymaps: SharedKeymaps) {
        let mut inner = self.inner();
        keymaps::lock(&keymaps).set_macros(inner.saved.iter().map(|saved| saved.name.clone()).collect());
        inner.keymaps = Some(keymaps);
    }

    pub fn generation(&self) -> u64 {
        self.inner().generation
    }

    pub fn list(&self) -> Vec<Macro> {
        self.inner().saved.clone()
    }

    pub fn get(&self, name: &str) -> Option<Macro> {
        self.inner().saved.iter().find(|saved| saved.name == name).cloned()
    }

    pub fn last(&self) -> Option<Macro> {
        self.inner().last.clone()
    }

    pub fn keep(&self, name: &str, steps: Vec<MacroStep>) -> Result<Macro, CommandError> {
        let name = name.trim();
        let mut inner = self.inner();
        let kept = Macro { name: name.to_owned(), steps };
        if !name.is_empty() {
            inner.taken(name)?;
            inner.saved.push(kept.clone());
            inner.save()?;
        }
        inner.last = Some(kept.clone());
        inner.generation += 1;
        Ok(kept)
    }

    pub fn check_name(&self, name: &str) -> Result<(), CommandError> {
        let name = name.trim();
        if name.is_empty() { Ok(()) } else { self.inner().taken(name) }
    }

    pub fn rename(&self, name: &str, new_name: &str) -> Result<(), CommandError> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(CommandError::new(t!("workbench-macro-empty-name")));
        }
        let mut inner = self.inner();
        let index = inner.find(name)?;
        if new_name == name {
            return Ok(());
        }
        inner.taken(new_name)?;
        new_name.clone_into(&mut inner.saved[index].name);
        if let Some(last) = inner.last.as_mut().filter(|last| last.name == name) {
            new_name.clone_into(&mut last.name);
        }
        inner.save()?;
        inner.move_shortcuts(name, Some(new_name));
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<(), CommandError> {
        let mut inner = self.inner();
        let index = inner.find(name)?;
        inner.saved.remove(index);
        inner.save()?;
        inner.move_shortcuts(name, None);
        Ok(())
    }

    pub fn remove_step(&self, name: &str, index: usize) -> Result<Macro, CommandError> {
        let mut inner = self.inner();
        let found = inner.find(name)?;
        if index >= inner.saved[found].steps.len() {
            return Err(CommandError::new(t!("workbench-macro-no-step", name = name, step = index)));
        }
        inner.saved[found].steps.remove(index);
        let changed = inner.saved[found].clone();
        if let Some(last) = inner.last.as_mut().filter(|last| last.name == name) {
            *last = changed.clone();
        }
        inner.save()?;
        Ok(changed)
    }

    fn played(&self, played: &Macro) {
        let mut inner = self.inner();
        inner.last = Some(played.clone());
        inner.generation += 1;
    }
}

fn read(file: &Path) -> (Vec<Macro>, Option<String>) {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(error) => return (Vec::new(), Some(t!("workbench-macro-unreadable", detail = error.to_string()))),
    };
    match serde_json::from_slice::<MacroFile>(&bytes) {
        Ok(saved) => (saved.macros, None),
        Err(error) => {
            let kept = file.with_extension("json.corrupt");
            let kept = if std::fs::rename(file, &kept).is_ok() { kept } else { file.to_owned() };
            let notice = t!("workbench-macro-corrupt", detail = error.to_string(), file = kept.display().to_string());
            (Vec::new(), Some(notice))
        }
    }
}

fn macros(session: &Session) -> Macros {
    session.state::<Macros>().cloned().unwrap_or_default()
}

fn recording(session: &Session) -> bool {
    zen_core::macros::is_recording(session)
}

fn playable(session: &Session) -> bool {
    !recording(session) && !session.state::<Recorder>().is_some_and(Recorder::is_playing)
}

fn play(session: &mut Session, played: &Macro) -> Result<Played, CommandError> {
    if let Some((index, step)) =
        played.steps.iter().enumerate().find(|(_, step)| !zen_core::macros::is_recorded(&step.command))
    {
        let message = t!("workbench-macro-step-refused", step = index, command = step.command.as_str());
        return Err(CommandError::with_code(zen_core::macros::STEP_FAILED, message)
            .with_data(serde_json::json!({ "step": index, "command": step.command })));
    }
    let has_workbench = session.state::<Workbench>().is_some();
    let before =
        has_workbench.then(|| session.state_mut::<Workbench>().active_document_mut()).flatten().map(|document| {
            document.close_undo_group();
            (document.path().to_owned(), document.undo_depth())
        });
    let undoes =
        played.steps.iter().any(|step| matches!(step.command.as_str(), "editor.edit.undo" | "editor.edit.redo"));
    let finish = before.map(|(path, depth)| {
        Box::new(move |session: &mut Session| {
            if session.state::<Workbench>().is_none() {
                return;
            }
            if let Some(document) =
                session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path)
            {
                let now = document.undo_depth();
                if !undoes && now > depth + 1 {
                    document.merge_undo_steps(now - depth);
                }
                document.close_undo_group();
            }
        }) as Finish
    });
    session.state_mut::<Recorder>().play(played.steps.clone(), finish);
    Ok(Played { name: played.name.clone(), steps: played.steps.len() })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroState {
    pub recording: bool,
    pub recorded: Vec<MacroStep>,
    pub last: Option<Macro>,
    pub macros: Vec<Macro>,
}

fn state(session: &Session) -> MacroState {
    let store = macros(session);
    let recorder = session.state::<Recorder>();
    MacroState {
        recording: recorder.is_some_and(Recorder::is_recording),
        recorded: recorder.map(|recorder| recorder.steps().to_vec()).unwrap_or_default(),
        last: store.last(),
        macros: store.list(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Played {
    pub name: String,
    pub steps: usize,
}

pub struct StartRecording;

impl Command for StartRecording {
    type Params = NoParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.startRecording";
    const TITLE: &'static str = "Start Macro Recording";
    const DESCRIPTION: &'static str = "Starts recording a macro: from now on the editor commands run on the active \
         document (typing, caret moves, selection, line actions, comments, reformatting) and saving are kept, with \
         their parameters, until editor.macro.stopRecording.";

    fn is_available(&self, session: &Session) -> bool {
        playable(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<MacroState, CommandError> {
        session.state_mut::<Recorder>().start();
        Ok(state(session))
    }
}

/// The parameters of `editor.macro.stopRecording`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StopParams {
    /// The name to save the macro under; empty or absent keeps it only as the last macro.
    pub name: Option<String>,
}

pub struct StopRecording;

impl Command for StopRecording {
    type Params = StopParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.stopRecording";
    const TITLE: &'static str = "Stop Macro Recording";
    const DESCRIPTION: &'static str = "Stops recording and keeps the macro: saved under `name` (unique among the saved \
         macros), or without a name only as the last macro for editor.macro.playLast. A recording without steps is \
         dropped.";

    fn is_available(&self, session: &Session) -> bool {
        recording(session)
    }

    fn run(&self, session: &mut Session, params: StopParams) -> Result<MacroState, CommandError> {
        let store = macros(session);
        let name = params.name.unwrap_or_default();
        store.check_name(&name)?;
        let steps = session.state_mut::<Recorder>().stop().unwrap_or_default();
        if !steps.is_empty() {
            store.keep(&name, steps)?;
        }
        Ok(state(session))
    }
}

pub struct PlayLast;

impl Command for PlayLast {
    type Params = NoParams;
    type Output = Played;
    const ID: &'static str = "editor.macro.playLast";
    const TITLE: &'static str = "Play Back Last Macro";
    const DESCRIPTION: &'static str = "Plays the last macro recorded or played on the active document, as one undo \
         step; a step that fails stops it.";

    fn is_available(&self, session: &Session) -> bool {
        playable(session) && session.state::<Macros>().is_some_and(|store| store.last().is_some())
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Played, CommandError> {
        let last = macros(session).last().ok_or_else(|| CommandError::new(t!("workbench-macro-no-last")))?;
        play(session, &last)
    }
}

/// A saved macro, by name.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct NameParams {
    /// The macro's name.
    pub name: String,
}

pub struct Play;

impl Command for Play {
    type Params = NameParams;
    type Output = Played;
    const ID: &'static str = "editor.macro.play";
    const TITLE: &'static str = "Play Saved Macro";
    const DESCRIPTION: &'static str = "Plays the saved macro `name` on the active document, as one undo step, and \
         makes it the last macro; a step that fails stops it.";

    fn is_available(&self, session: &Session) -> bool {
        playable(session)
    }

    fn run(&self, session: &mut Session, params: NameParams) -> Result<Played, CommandError> {
        let store = macros(session);
        let saved = store
            .get(&params.name)
            .ok_or_else(|| CommandError::new(t!("workbench-macro-unknown", name = params.name.as_str())))?;
        let played = play(session, &saved)?;
        store.played(&saved);
        Ok(played)
    }
}

pub struct ListMacros;

impl Command for ListMacros {
    type Params = NoParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.list";
    const TITLE: &'static str = "Macros";
    const DESCRIPTION: &'static str = "Lists the saved macros with their steps, the last macro, and whether a macro is \
         being recorded with the steps recorded so far.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<MacroState, CommandError> {
        Ok(state(session))
    }
}

/// The parameters of `editor.macro.rename`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The macro's name.
    pub name: String,
    /// Its new name, not taken by another macro.
    pub new_name: String,
}

pub struct RenameMacro;

impl Command for RenameMacro {
    type Params = RenameParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.rename";
    const TITLE: &'static str = "Rename Macro";
    const DESCRIPTION: &'static str = "Renames a saved macro; its shortcuts go with it.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<MacroState, CommandError> {
        macros(session).rename(&params.name, &params.new_name)?;
        Ok(state(session))
    }
}

pub struct RemoveMacro;

impl Command for RemoveMacro {
    type Params = NameParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.remove";
    const TITLE: &'static str = "Remove Macro";
    const DESCRIPTION: &'static str = "Removes a saved macro and its shortcuts.";

    fn run(&self, session: &mut Session, params: NameParams) -> Result<MacroState, CommandError> {
        macros(session).remove(&params.name)?;
        Ok(state(session))
    }
}

/// The parameters of `editor.macro.removeStep`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct StepParams {
    /// The macro's name.
    pub name: String,
    /// The step's index, from 0.
    pub index: usize,
}

pub struct RemoveStep;

impl Command for RemoveStep {
    type Params = StepParams;
    type Output = MacroState;
    const ID: &'static str = "editor.macro.removeStep";
    const TITLE: &'static str = "Remove Macro Action";
    const DESCRIPTION: &'static str = "Removes the step at `index` (from 0) of a saved macro.";

    fn run(&self, session: &mut Session, params: StepParams) -> Result<MacroState, CommandError> {
        macros(session).remove_step(&params.name, params.index)?;
        Ok(state(session))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(StartRecording)?;
    registry.register(StopRecording)?;
    registry.register(PlayLast)?;
    registry.register(Play)?;
    registry.register(ListMacros)?;
    registry.register(RenameMacro)?;
    registry.register(RemoveMacro)?;
    registry.register(RemoveStep)?;
    Ok(())
}

#[cfg(test)]
mod tests;
