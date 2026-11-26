use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Session;

/// One recorded command of a macro: its id and the parameters it was dispatched with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MacroStep {
    /// The command id, such as `editor.edit.type`.
    pub command: String,
    /// The parameters it ran with.
    #[serde(default)]
    pub params: Value,
}

impl MacroStep {
    pub fn new(command: impl Into<String>, params: Value) -> Self {
        Self { command: command.into(), params }
    }
}

pub type Finish = Box<dyn FnOnce(&mut Session) + Send>;

struct Playback {
    steps: Vec<MacroStep>,
    finish: Option<Finish>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Window,
    Agent,
}

#[derive(Default)]
pub struct Recorder {
    recording: Option<Vec<MacroStep>>,
    owner: Source,
    source: Source,
    playback: Option<Playback>,
    playing: bool,
}

impl Recorder {
    pub fn start(&mut self) {
        self.recording = Some(Vec::new());
        self.owner = self.source;
    }

    pub fn owner(&self) -> Source {
        self.owner
    }

    pub fn set_source(&mut self, source: Source) -> Source {
        std::mem::replace(&mut self.source, source)
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn steps(&self) -> &[MacroStep] {
        self.recording.as_deref().unwrap_or_default()
    }

    pub fn stop(&mut self) -> Option<Vec<MacroStep>> {
        self.recording.take()
    }

    pub fn record(&mut self, command: &str, params: Value) {
        if self.source != self.owner {
            return;
        }
        let Some(steps) = &mut self.recording else { return };
        if let Some(last) = steps.last_mut()
            && command == TYPE
            && last.command == TYPE
            && let (Some(before), Some(typed)) = (last.params["text"].as_str(), params["text"].as_str())
            && plain(before)
            && plain(typed)
        {
            last.params = serde_json::json!({ "text": format!("{before}{typed}") });
            return;
        }
        steps.push(MacroStep::new(command, params));
    }

    pub fn play(&mut self, steps: Vec<MacroStep>, finish: Option<Finish>) {
        self.playback = Some(Playback { steps, finish });
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn has_playback(&self) -> bool {
        self.playback.is_some()
    }

    pub(crate) fn take_playback(&mut self) -> Option<(Vec<MacroStep>, Option<Finish>)> {
        self.playback.take().map(|playback| (playback.steps, playback.finish))
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }
}

const TYPE: &str = "editor.edit.type";

pub const STEP_FAILED: &str = "macroStepFailed";

fn plain(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '_')
}

pub fn is_recorded(id: &str) -> bool {
    match id {
        "editor.caret.goTo" | "editor.caret.toggle" | "editor.selection.get" | "editor.selection.column" => false,
        "editor.code.reformat"
        | "editor.imports.optimize"
        | "editor.brackets.goToMatching"
        | "file.document.save"
        | "file.document.saveAll" => true,
        _ => ["editor.edit.", "editor.caret.", "editor.selection.", "editor.line.", "editor.comment."]
            .iter()
            .any(|prefix| id.starts_with(prefix)),
    }
}

pub fn is_recording(session: &Session) -> bool {
    session.state::<Recorder>().is_some_and(Recorder::is_recording)
}

pub fn by_agent(session: &Session) -> bool {
    session.state::<Recorder>().is_some_and(|recorder| recorder.source == Source::Agent)
}

pub fn as_agent<T>(session: &mut Session, run: impl FnOnce(&mut Session) -> T) -> T {
    let before = session.state_mut::<Recorder>().set_source(Source::Agent);
    let result = run(session);
    session.state_mut::<Recorder>().set_source(before);
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn editor_commands_are_recorded_and_the_rest_is_not() {
        for id in [
            "editor.edit.type",
            "editor.edit.backspace",
            "editor.caret.move",
            "editor.selection.set",
            "editor.line.duplicate",
            "editor.comment.line",
            "editor.code.reformat",
            "file.document.saveAll",
        ] {
            assert!(is_recorded(id), "{id}");
        }
        for id in [
            "editor.caret.goTo",
            "editor.selection.get",
            "editor.completion.list",
            "view.settings.open",
            "editor.macro.play",
            "file.document.open",
            "file.document.autosave",
            "editor.fold.toggle",
        ] {
            assert!(!is_recorded(id), "{id}");
        }
    }

    #[test]
    fn plain_typing_is_one_step_and_brackets_stay_apart() {
        let mut recorder = Recorder::default();
        recorder.record(TYPE, json!({ "text": "a" }));
        assert!(recorder.steps().is_empty(), "nothing is recorded before Start");
        recorder.start();
        for text in ["a", "b", " ", "c", "(", "d"] {
            recorder.record(TYPE, json!({ "text": text }));
        }
        recorder.record("editor.line.duplicate", json!({}));
        recorder.record(TYPE, json!({ "text": "e" }));
        let texts: Vec<String> = recorder
            .steps()
            .iter()
            .map(|step| format!("{} {}", step.command, step.params["text"].as_str().unwrap_or_default()))
            .collect();
        assert_eq!(
            texts,
            [
                "editor.edit.type ab c",
                "editor.edit.type (",
                "editor.edit.type d",
                "editor.line.duplicate ",
                "editor.edit.type e"
            ]
        );
        assert_eq!(recorder.stop().map(|steps| steps.len()), Some(5));
        assert!(!recorder.is_recording());
    }

    #[test]
    fn a_recording_keeps_only_what_its_own_source_dispatches() {
        let mut recorder = Recorder::default();
        recorder.start();
        assert_eq!(recorder.owner(), Source::Window);
        recorder.record(TYPE, json!({ "text": "(" }));
        assert_eq!(recorder.set_source(Source::Agent), Source::Window);
        recorder.record(TYPE, json!({ "text": "agent" }));
        recorder.set_source(Source::Window);
        recorder.record(TYPE, json!({ "text": "[" }));
        assert_eq!(
            recorder.stop().unwrap(),
            [MacroStep::new(TYPE, json!({ "text": "(" })), MacroStep::new(TYPE, json!({ "text": "[" }))]
        );

        recorder.set_source(Source::Agent);
        recorder.start();
        recorder.record(TYPE, json!({ "text": "(" }));
        recorder.set_source(Source::Window);
        recorder.record(TYPE, json!({ "text": "person" }));
        assert_eq!(recorder.owner(), Source::Agent);
        assert_eq!(recorder.stop().unwrap(), [MacroStep::new(TYPE, json!({ "text": "(" }))]);
    }
}
