use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::keymap::EXTERNAL_TOOL_ACTION;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::macros::Macro;
use zen_workbench::external_tools::{ExternalTool, ExternalTools};

use super::agent::ActionError;
use super::{IdePalette, IdeView, dialog_frame, menu_label};
use crate::theme;

static STORES: Mutex<Vec<ExternalTools>> = Mutex::new(Vec::new());

pub(crate) fn load(tools: ExternalTools) {
    STORES.lock().unwrap_or_else(PoisonError::into_inner).push(tools);
}

pub(super) fn title(action: &str) -> Option<String> {
    let id = action.strip_prefix(EXTERNAL_TOOL_ACTION)?;
    let stores = STORES.lock().unwrap_or_else(PoisonError::into_inner);
    Some(stores.iter().rev().find_map(|store| store.get(id)).map_or_else(|| id.to_owned(), |tool| tool.name))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MenuGroup {
    pub(super) name: Option<String>,
    pub(super) tools: Vec<(String, String)>,
}

pub(super) fn menu(tools: &[ExternalTool]) -> Vec<MenuGroup> {
    let mut groups: Vec<MenuGroup> = Vec::new();
    for tool in tools.iter().filter(|tool| tool.enabled) {
        let name = (!tool.in_default_group()).then(|| tool.group.trim().to_owned());
        let at = match groups.iter().position(|group| group.name == name) {
            Some(at) => at,
            None => {
                groups.push(MenuGroup { name: name.clone(), tools: Vec::new() });
                groups.len() - 1
            }
        };
        groups[at].tools.push((tool.id.clone(), tool.name.clone()));
    }
    groups
}

pub(super) fn context_submenus(ui: &mut Ui, tools: &[ExternalTool]) -> Option<String> {
    let groups = menu(tools);
    if groups.is_empty() {
        return None;
    }
    let mut chosen = None;
    for group in &groups {
        let name = group.name.clone().unwrap_or_else(|| t!("menu-tools-external-tools"));
        ui.menu_button(menu_label(name), |ui| {
            ui.set_min_width(200.0);
            for (id, name) in &group.tools {
                if ui.button(name).clicked() {
                    chosen = Some(id.clone());
                    ui.close();
                }
            }
        });
    }
    chosen
}

#[derive(Debug)]
pub(super) struct ToolPrompt {
    command: &'static str,
    base: Value,
    name: String,
    text: Option<String>,
    chosen_file: Option<String>,
    focus: bool,
}

impl ToolPrompt {
    pub(super) fn new(command: &'static str, base: Value, name: String, text: bool, file: bool) -> Self {
        Self { command, base, name, text: text.then(String::new), chosen_file: file.then(String::new), focus: true }
    }

    fn params(&self) -> Value {
        let mut params = self.base.clone();
        if let Some(text) = &self.text {
            params["prompt"] = json!(text);
        }
        if let Some(file) = self.chosen_file.as_deref().map(str::trim).filter(|file| !file.is_empty()) {
            params["filePrompt"] = json!(file);
        }
        params
    }
}

enum Choice {
    Run,
    Cancel,
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_file(start: Option<&std::path::Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_file(_start: Option<&std::path::Path>) -> Option<PathBuf> {
    None
}

fn show(ui: &mut Ui, p: &IdePalette, prompt: &mut ToolPrompt, project: Option<&std::path::Path>) -> Option<Choice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("external-tool-prompt")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(egui::RichText::new(prompt.name.clone()).font(theme::semibold(15.0)).color(p.text));
            let focus = std::mem::take(&mut prompt.focus);
            if let Some(text) = prompt.text.as_mut() {
                ui.label(egui::RichText::new(t!("ide-external-tools-prompt")).font(theme::regular(13.0)).color(p.text));
                let field = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()));
                if focus {
                    field.request_focus();
                }
            }
            if let Some(file) = prompt.chosen_file.as_mut() {
                ui.label(
                    egui::RichText::new(t!("ide-external-tools-file-prompt")).font(theme::regular(13.0)).color(p.text),
                );
                ui.horizontal(|ui| {
                    let browse = t!("ide-external-tools-browse");
                    let field = ui.add(egui::TextEdit::singleline(file).desired_width(ui.available_width() - 90.0));
                    if focus && prompt.text.is_none() {
                        field.request_focus();
                    }
                    if ui.button(browse).clicked()
                        && let Some(path) = pick_file(project)
                    {
                        *file = path.display().to_string();
                    }
                });
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = egui::Button::new(
                    egui::RichText::new(t!("ide-external-tools-run")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(ok).clicked() {
                    choice = Some(Choice::Run);
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Run);
            }
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

impl IdeView {
    pub(super) fn run_external_tool(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<Value, ActionError> {
        let id = params.get("toolId").and_then(Value::as_str).unwrap_or_default().to_owned();
        let Some(tool) = session.state::<ExternalTools>().and_then(|tools| tools.get(&id)) else {
            return Err(ActionError::Refused(format!("there is no external tool `{id}`; see run.externalTools.list")));
        };
        let file = params.get("file").and_then(Value::as_str).map(PathBuf::from);
        let prompts = tool.prompts();
        let given = |key: &str| params.get(key).is_some_and(|value| !value.is_null());
        let ask_text = prompts.contains(&Macro::Prompt) && !given("prompt");
        let ask_file = prompts.contains(&Macro::FilePrompt) && !given("filePrompt");
        if tool.enabled && (ask_text || ask_file) {
            let mut base = json!({ "id": id, "file": file });
            for key in ["prompt", "filePrompt"] {
                if let Some(value) = params.get(key).filter(|value| !value.is_null()) {
                    base[key] = value.clone();
                }
            }
            self.external_tool_prompt =
                Some(ToolPrompt::new("run.externalTools.run", base, tool.name, ask_text, ask_file));
            return Ok(json!({ "asking": "externalToolPrompt" }));
        }
        let mut run = json!({ "id": id, "file": file });
        for key in ["prompt", "filePrompt"] {
            if let Some(value) = params.get(key) {
                run[key] = value.clone();
            }
        }
        registry.dispatch(session, "run.externalTools.run", run).map_err(ActionError::Dispatch)
    }

    pub(super) fn external_tool_prompt_open(&self) -> bool {
        self.external_tool_prompt.is_some()
    }

    pub(super) fn external_tool_prompt(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let project = session
            .state::<zen_workbench::Workbench>()
            .and_then(zen_workbench::Workbench::project)
            .map(|path| path.to_owned());
        let Some(prompt) = &mut self.external_tool_prompt else { return };
        match show(ui, &p, prompt, project.as_deref()) {
            Some(Choice::Cancel) => self.external_tool_prompt = None,
            Some(Choice::Run) => {
                let (command, params) = (prompt.command, prompt.params());
                self.external_tool_prompt = None;
                if let Err(error) = registry.dispatch(session, command, params) {
                    self.report(ui, error);
                }
            }
            None => {}
        }
    }

    pub(super) fn run_chosen_tool(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        id: &str,
        file: Option<PathBuf>,
    ) {
        let params = json!({ "toolId": id, "file": file });
        if let Err(ActionError::Dispatch(error)) = self.run_external_tool(registry, session, &params) {
            self.report(ui, error);
        }
    }
}

#[cfg(test)]
mod tests {
    use zen_workbench::external_tools::DEFAULT_GROUP;

    use super::*;

    fn tool(id: &str, group: &str, enabled: bool) -> ExternalTool {
        ExternalTool {
            id: id.to_owned(),
            name: id.to_uppercase(),
            group: group.to_owned(),
            enabled,
            ..ExternalTool::default()
        }
    }

    #[test]
    fn each_group_is_a_submenu_in_order_and_what_is_off_is_left_out() {
        let tools = [
            tool("lint", "Linters", true),
            tool("fmt", DEFAULT_GROUP, true),
            tool("vet", "Linters", true),
            tool("off", "Off", false),
            tool("bare", "", true),
        ];
        let groups = menu(&tools);
        assert_eq!(
            groups,
            [
                MenuGroup {
                    name: Some("Linters".to_owned()),
                    tools: vec![("lint".into(), "LINT".into()), ("vet".into(), "VET".into())]
                },
                MenuGroup { name: None, tools: vec![("fmt".into(), "FMT".into()), ("bare".into(), "BARE".into())] },
            ]
        );
        assert!(menu(&[tool("off", "", false)]).is_empty(), "no menu while no tool is on");
        assert!(menu(&[]).is_empty());
    }

    #[test]
    fn actions_are_named_after_their_tools() {
        let tools = ExternalTools::load(None);
        let id = tools
            .save_tool(ExternalTool {
                name: "Title Test Tool".to_owned(),
                program: "/usr/bin/true".to_owned(),
                ..ExternalTool::default()
            })
            .unwrap();
        load(tools);
        assert_eq!(title(&format!("{EXTERNAL_TOOL_ACTION}{id}")).as_deref(), Some("Title Test Tool"));
        assert_eq!(title(&format!("{EXTERNAL_TOOL_ACTION}gone-tool")).as_deref(), Some("gone-tool"));
        assert_eq!(title("run.process.start"), None);
    }
}
