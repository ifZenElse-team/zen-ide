use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Key, Modifiers, Rect, Sense, Ui, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::debug_window::offset_of;
use super::run_window::call;
use super::{IdePalette, metrics};
use crate::{accessibility, theme};

pub(super) const SMART_STEP_INTO: &str = "view.smartStepInto.open";

const PENDING_POLL: Duration = Duration::from_millis(30);

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Target {
    pub id: i64,
    pub label: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
}

#[derive(Debug)]
struct Popup {
    stops: Value,
    place: Option<(PathBuf, u32)>,
    targets: Option<Vec<Target>>,
    selected: usize,
    anchor: Rect,
    rows: Vec<Rect>,
}

#[derive(Debug, Default)]
pub(super) struct SmartStep {
    popup: Option<Popup>,
}

fn stopped_state(registry: &Registry, session: &mut Session) -> Option<Value> {
    call(registry, session, "debug.session.get", json!({})).ok().filter(|state| state["state"] == "stopped")
}

impl SmartStep {
    pub(super) fn name(&self) -> Option<&'static str> {
        self.popup.as_ref().map(|_| "smartStepInto")
    }

    pub(super) fn open(&mut self, registry: &Registry, session: &mut Session, anchor: Rect) -> Result<(), String> {
        let Some(state) = stopped_state(registry, session) else { return Err(t!("ide-smart-step-not-stopped")) };
        if state["support"]["smartStepInto"] != true {
            return Err(t!("ide-smart-step-unsupported"));
        }
        let place = state["frame"]["path"]
            .as_str()
            .map(PathBuf::from)
            .zip(state["frame"]["line"].as_u64().and_then(|line| u32::try_from(line).ok()));
        self.popup =
            Some(Popup { stops: state["stops"].clone(), place, targets: None, selected: 0, anchor, rows: Vec::new() });
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn shown(&self) -> Option<(Vec<String>, usize)> {
        let popup = self.popup.as_ref()?;
        let labels = popup.targets.iter().flatten().map(|target| target.label.clone()).collect();
        Some((labels, popup.selected))
    }

    #[cfg(test)]
    pub(super) fn row(&self, index: usize) -> Option<Rect> {
        self.popup.as_ref()?.rows.get(index).copied()
    }

    pub(super) fn highlight(&self, session: &Session) -> Option<(PathBuf, Range<usize>)> {
        let popup = self.popup.as_ref()?;
        let target = popup.targets.as_ref()?.get(popup.selected)?;
        let (path, line) = popup.place.clone()?;
        let range = target_range(session, &path, line, target)?;
        Some((path, range))
    }

    pub(super) fn poll(&mut self, registry: &Registry, session: &mut Session) -> Option<(&'static str, Value)> {
        let popup = self.popup.as_mut()?;
        let still = stopped_state(registry, session).is_some_and(|state| state["stops"] == popup.stops);
        if !still {
            self.popup = None;
            return None;
        }
        if popup.targets.is_some() {
            return None;
        }
        match call(registry, session, "debug.execution.stepInTargets", json!({ "waitMs": 0 })) {
            Ok(answer) if answer["pending"] == true => None,
            Ok(answer) => {
                let targets: Vec<Target> = serde_json::from_value(answer["targets"].clone()).unwrap_or_default();
                match targets.as_slice() {
                    [] => {
                        self.popup = None;
                        Some(("debug.execution.stepInto", json!({})))
                    }
                    [only] => {
                        let id = only.id;
                        self.popup = None;
                        Some(("debug.execution.smartStepInto", json!({ "targetId": id })))
                    }
                    _ => {
                        popup.targets = Some(targets);
                        None
                    }
                }
            }
            Err(_) => {
                self.popup = None;
                Some(("debug.execution.stepInto", json!({})))
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Option<(&'static str, Value)> {
        let popup = self.popup.as_mut()?;
        let Some(targets) = popup.targets.clone() else {
            ui.ctx().request_repaint_after(PENDING_POLL);
            return None;
        };
        let count = targets.len();
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            popup.selected = (popup.selected + count - 1) % count;
        }
        if down {
            popup.selected = (popup.selected + 1) % count;
        }
        let mut chosen = enter.then_some(popup.selected);
        let at = popup.anchor.left_bottom() + vec2(0.0, 2.0);
        let id = ui.make_persistent_id("smart-step-into");
        let mut rows = Vec::with_capacity(count);
        let shown = egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(200.0);
                ui.label(
                    egui::RichText::new(t!("ide-smart-step-title")).font(theme::semibold(12.0)).color(p.text_secondary),
                );
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, target) in targets.iter().enumerate() {
                    let width = ui.available_width().max(200.0);
                    let (rect, response) = ui.allocate_exact_size(vec2(width, metrics::TREE_ROW), Sense::click());
                    let selected = index == popup.selected;
                    accessibility::item(&response, egui::Role::ListBoxOption, selected, &target.label);
                    if response.hovered() && ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO) {
                        popup.selected = index;
                    }
                    if index == popup.selected {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    }
                    ui.painter().with_clip_rect(rect).text(
                        rect.left_center() + vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &target.label,
                        theme::regular(13.0),
                        p.text,
                    );
                    if response.clicked() {
                        chosen = Some(index);
                    }
                    rows.push(rect);
                }
            })
        });
        accessibility::name(&shown.response, egui::Role::ListBox, &t!("ide-smart-step-title"));
        let rect = shown.response.rect;
        popup.rows = rows;
        let clicked_away = ui.input(|input| {
            input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(|pointer| !rect.contains(pointer))
        });
        if let Some(index) = chosen {
            let id = targets[index].id;
            self.popup = None;
            return Some(("debug.execution.smartStepInto", json!({ "targetId": id })));
        }
        if escape || clicked_away {
            self.popup = None;
        }
        None
    }
}

fn target_range(session: &Session, path: &Path, frame_line: u32, target: &Target) -> Option<Range<usize>> {
    let column = target.column?;
    let line = target.line.unwrap_or(frame_line);
    let start = offset_of(session, path, line, column);
    let end = match target.end_column {
        Some(end_column) => offset_of(session, path, target.end_line.unwrap_or(line), end_column),
        None => {
            let text = line_of(session, path, line);
            let word = text
                .chars()
                .skip(column as usize)
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .count();
            start + word
        }
    };
    (end > start).then_some(start..end)
}

fn line_of(session: &Session, path: &Path, line: u32) -> String {
    let open = session
        .state::<zen_workbench::Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned());
    let text = open.or_else(|| std::fs::read_to_string(path).ok()).unwrap_or_default();
    text.split('\n').nth(line as usize).unwrap_or_default().to_owned()
}
