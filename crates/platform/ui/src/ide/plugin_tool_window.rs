use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError, RwLock};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::ToolRow;

use super::glyphs::*;
use super::plugin_tool_inputs::{Field, InputDialog, InputInfo, input_dialog, inputs_of};
use super::tool_stripes::Kind;
use super::tool_windows::{Declared, Side as Place};
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const CHECK_EVERY: f64 = 2.0;
const REFRESH_EVERY: f64 = 5.0;
const POLL_EVERY: f64 = 0.3;
const MAX_DIFFS_OPENED: usize = 20;

type Request = (&'static str, String, Vec<String>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ActionInfo {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) kind: String,
    pub(super) rows: bool,
    pub(super) confirms: bool,
    pub(super) inputs: Vec<InputInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SectionInfo {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) columns: Vec<String>,
    pub(super) column_ids: Vec<String>,
    pub(super) actions: Vec<ActionInfo>,
    pub(super) index: bool,
    pub(super) state: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WindowInfo {
    pub(super) tool: &'static str,
    pub(super) plugin: String,
    pub(super) window: String,
    pub(super) title: String,
    pub(super) side: Place,
    pub(super) icon: bool,
    pub(super) choices: bool,
    pub(super) sections: Vec<SectionInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ChoiceView {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) value: Option<String>,
    pub(super) valid: bool,
    pub(super) options: Vec<(String, String)>,
    pub(super) source: Vec<(String, bool)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RowInfo {
    pub(super) id: String,
    pub(super) columns: Vec<String>,
    pub(super) state: Option<String>,
}

#[derive(Default)]
struct View {
    section: usize,
    rows: Vec<RowInfo>,
    error: Option<String>,
    busy: bool,
    listing: bool,
    next: f64,
    force: bool,
    selected: BTreeSet<String>,
    choices: Vec<ChoiceView>,
}

#[derive(Default)]
pub(super) struct PluginToolWindows {
    windows: Vec<WindowInfo>,
    checked: Option<f64>,
    views: BTreeMap<&'static str, View>,
    confirm: Option<Confirm>,
    asking: Option<InputDialog>,
}

#[derive(Clone, Debug)]
struct Confirm {
    tool: &'static str,
    action: String,
    rows: Vec<String>,
    title: String,
    question: Option<String>,
    typed: Option<Value>,
}

fn active() -> &'static RwLock<Vec<&'static Kind>> {
    static ACTIVE: RwLock<Vec<&'static Kind>> = RwLock::new(Vec::new());
    &ACTIVE
}

fn leaked() -> &'static Mutex<Vec<(&'static Kind, &'static Declared)>> {
    static LEAKED: Mutex<Vec<(&'static Kind, &'static Declared)>> = Mutex::new(Vec::new());
    &LEAKED
}

pub(super) fn kind(id: &str) -> Option<&'static Kind> {
    active().read().unwrap_or_else(PoisonError::into_inner).iter().find(|kind| kind.id == id).copied()
}

pub(super) fn declared(id: &str) -> Option<&'static Declared> {
    kind(id)?;
    leaked()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .rev()
        .find(|(kind, _)| kind.id == id)
        .map(|(_, d)| *d)
}

pub(super) fn side_of(name: &str) -> Place {
    match name {
        "left" => Place::LeftTop,
        "right" => Place::RightTop,
        _ => Place::BottomLeft,
    }
}

fn leak(id: &str, title: &str, side: Place) -> &'static Kind {
    let mut leaked = leaked().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((kind, _)) =
        leaked.iter().rev().find(|(kind, declared)| kind.id == id && kind.title == title && declared.side == side)
    {
        return kind;
    }
    let id: &'static str = leaked
        .iter()
        .find(|(kind, _)| kind.id == id)
        .map_or_else(|| &*Box::leak(id.to_owned().into_boxed_str()), |(kind, _)| kind.id);
    let action: &'static str = Box::leak(format!("view.toolWindow.{id}").into_boxed_str());
    let title: &'static str = Box::leak(title.to_owned().into_boxed_str());
    let kind: &'static Kind = Box::leak(Box::new(Kind { id, glyph: glyph_services, title, action }));
    let declared: &'static Declared = Box::leak(Box::new(Declared { id, side, shown: false, reopens: true }));
    leaked.push((kind, declared));
    kind
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

pub(super) fn tool_id(plugin: &str, window: &str) -> String {
    if super::tool_stripes::built_in(window) { format!("{plugin}-{window}") } else { window.to_owned() }
}

pub(super) fn windows_of(listed: &Value) -> Vec<WindowInfo> {
    listed["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|window| {
            let (plugin, id) = (text(&window["plugin"]), text(&window["window"]));
            let title = text(&window["title"]);
            let side = side_of(window["side"].as_str().unwrap_or_default());
            let tool = leak(&tool_id(&plugin, &id), if title.is_empty() { &id } else { &title }, side).id;
            let sections = window["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|section| SectionInfo {
                    id: text(&section["id"]),
                    title: text(&section["title"]),
                    columns: section["columnTitles"].as_array().into_iter().flatten().map(text).collect(),
                    column_ids: section["columns"].as_array().into_iter().flatten().map(text).collect(),
                    actions: section["actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|action| ActionInfo {
                            id: text(&action["id"]),
                            title: text(&action["title"]),
                            kind: text(&action["kind"]),
                            rows: action["rows"] == true,
                            confirms: action["confirms"] == true,
                            inputs: inputs_of(action),
                        })
                        .collect(),
                    index: section["rows"] == "index",
                    state: section["state"] == true,
                })
                .collect();
            let choices = window["choices"].as_array().is_some_and(|choices| !choices.is_empty());
            let icon = window["icon"] == true;
            WindowInfo { tool, plugin, window: id, title, side, icon, choices, sections }
        })
        .collect()
}

pub(super) fn choices_of(answer: &Value) -> Vec<ChoiceView> {
    answer["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|choice| ChoiceView {
            id: text(&choice["id"]),
            title: text(&choice["title"]),
            value: choice["value"].as_str().map(str::to_owned),
            valid: choice["valid"] == true,
            options: choice["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|option| (text(&option["id"]), text(&option["label"])))
                .collect(),
            source: choice["source"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|file| (text(&file["path"]), file["exists"] == true))
                .collect(),
        })
        .collect()
}

pub(super) fn rows_of(answer: &Value) -> Vec<RowInfo> {
    answer["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| RowInfo {
            id: text(&row["id"]),
            columns: row["columns"].as_array().into_iter().flatten().map(text).collect(),
            state: row["state"].as_str().map(str::to_owned),
        })
        .collect()
}

fn glyph_for(action: &ActionInfo) -> Glyph {
    match (action.id.as_str(), action.kind.as_str()) {
        (_, "destroy") => glyph_trash,
        (_, "terminal") => glyph_terminal,
        ("start", _) => glyph_run,
        ("stop", _) => glyph_stop,
        ("restart", _) => glyph_rerun,
        ("pull", _) => glyph_arrow_down,
        (_, "stream") => glyph_run_window,
        _ => glyph_run,
    }
}

fn state_color(p: &IdePalette, state: Option<&str>) -> Option<egui::Color32> {
    match state? {
        "running" | "healthy" | "up" => Some(p.run),
        "paused" | "restarting" | "starting" | "unhealthy" => Some(p.warning_text),
        "dead" => Some(p.error_text),
        _ => Some(p.text_secondary.gamma_multiply(0.6)),
    }
}

impl PluginToolWindows {
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn menu_items(&self) -> Vec<(String, String)> {
        self.windows.iter().map(|window| (format!("view.toolWindow.{}", window.tool), window.title.clone())).collect()
    }

    pub(super) fn window(&self, tool: &str) -> Option<&WindowInfo> {
        self.windows.iter().find(|window| window.tool == tool)
    }

    fn set_windows(&mut self, windows: Vec<WindowInfo>) {
        let kinds: Vec<&'static Kind> = windows.iter().filter_map(|window| leaked_kind(window.tool)).collect();
        let icons: BTreeMap<&'static str, String> = windows
            .iter()
            .filter(|window| window.icon)
            .filter_map(|window| {
                Some((window.tool, zen_workbench::plugin_tools::icon(&window.plugin, &window.window)?))
            })
            .collect();
        *svgs().lock().unwrap_or_else(PoisonError::into_inner) = icons;
        *active().write().unwrap_or_else(PoisonError::into_inner) = kinds;
        self.views.retain(|tool, _| windows.iter().any(|window| window.tool == *tool));
        self.windows = windows;
    }
}

fn svgs() -> &'static Mutex<BTreeMap<&'static str, String>> {
    static SVGS: Mutex<BTreeMap<&'static str, String>> = Mutex::new(BTreeMap::new());
    &SVGS
}

type Masks = BTreeMap<(String, u32), Option<egui::TextureHandle>>;

fn masks() -> &'static Mutex<Masks> {
    static MASKS: Mutex<Masks> = Mutex::new(BTreeMap::new());
    &MASKS
}

pub(super) fn icon_texture(ctx: &egui::Context, tool: &str, side: u32) -> Option<egui::TextureHandle> {
    let svg = svgs().lock().unwrap_or_else(PoisonError::into_inner).get(tool).cloned()?;
    let mut masks = masks().lock().unwrap_or_else(PoisonError::into_inner);
    masks
        .entry((svg.clone(), side))
        .or_insert_with(|| {
            let mask = zen_image::icon::mask(&svg, side)?;
            let rgba: Vec<u8> = mask.iter().flat_map(|alpha| [255, 255, 255, *alpha]).collect();
            let image = egui::ColorImage::from_rgba_unmultiplied([side as usize, side as usize], &rgba);
            Some(ctx.load_texture(format!("plugin-tool-icon-{tool}-{side}"), image, egui::TextureOptions::LINEAR))
        })
        .clone()
}

pub(super) fn paint_glyph(kind: &Kind, painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let side = (rect.width().max(rect.height()) * painter.ctx().pixels_per_point()).round().clamp(1.0, 256.0) as u32;
    match icon_texture(painter.ctx(), kind.id, side) {
        Some(texture) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, color);
        }
        None => (kind.glyph)(painter, rect, color),
    }
}

fn leaked_kind(id: &str) -> Option<&'static Kind> {
    leaked()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .rev()
        .find(|(kind, _)| kind.id == id)
        .map(|(k, _)| *k)
}

const NOTES: &str = "pluginToolWindowSides";

pub(super) fn moved(
    windows: &[WindowInfo],
    placed: &dyn Fn(&str) -> Option<Place>,
    noted: &BTreeSet<String>,
) -> Vec<String> {
    windows
        .iter()
        .filter(|window| window.side != Place::BottomLeft && !noted.contains(window.tool))
        .filter(|window| placed(window.tool).is_some_and(|side| side.anchor() != window.side.anchor()))
        .map(|window| window.tool.to_owned())
        .collect()
}

impl IdeView {
    fn note_moved_windows(&mut self, session: &mut Session, windows: &[WindowInfo]) {
        let Some(env) = session.state::<zen_workbench::build_tools::BuildEnv>().cloned() else { return };
        let mut noted: BTreeSet<String> = env.settings(NOTES);
        let layout = &self.tool_windows;
        let moved = moved(windows, &|tool| layout.side_of(tool), &noted);
        if moved.is_empty() {
            return;
        }
        let titles: Vec<String> = windows
            .iter()
            .filter(|window| moved.iter().any(|tool| tool == window.tool))
            .map(|window| window.title.clone())
            .collect();
        let side = windows.iter().find(|window| moved.iter().any(|tool| tool == window.tool)).map(|window| window.side);
        let names = match titles.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => t!("ide-plugin-tool-and", first = rest.join(", "), last = last.as_str()),
            [] => String::new(),
        };
        let side = match side.map(Place::anchor) {
            Some(super::tool_windows::Anchor::Left) => "left",
            Some(super::tool_windows::Anchor::Right) => "right",
            _ => "bottom",
        };
        let body = t!("ide-plugin-tool-moved", windows = names, count = titles.len(), side = side);
        let notice = zen_workbench::notifications::NewNotification::message(
            zen_workbench::notifications::Group::General,
            zen_workbench::notifications::Kind::Info,
            body,
        );
        zen_workbench::notifications::post(session, notice);
        noted.extend(moved);
        let _ = env.set_settings(NOTES, &noted);
    }

    pub(super) fn follow_plugin_tool_windows(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        now: f64,
    ) {
        self.agent_wants_values(session);
        if self.plugin_tools.checked.is_none_or(|at| now - at >= CHECK_EVERY) {
            self.plugin_tools.checked = Some(now);
            let listed = registry.dispatch(session, "plugins.toolWindow.list", json!({})).unwrap_or_default();
            let windows = windows_of(&listed);
            for window in &windows {
                self.tool_windows.ensure(window.tool, window.side);
            }
            if windows != self.plugin_tools.windows {
                self.note_moved_windows(session, &windows);
                let gone: Vec<&'static str> = self
                    .plugin_tools
                    .windows
                    .iter()
                    .map(|window| window.tool)
                    .filter(|tool| !windows.iter().any(|window| window.tool == *tool))
                    .collect();
                self.plugin_tools.set_windows(windows);
                for tool in gone {
                    if self.tool_visible(tool) {
                        self.hide_tool(tool);
                    }
                }
            }
        }
        let windows: Vec<WindowInfo> =
            self.plugin_tools.windows.iter().filter(|window| self.tool_visible(window.tool)).cloned().collect();
        for window in &windows {
            let current = self.plugin_tools.views.entry(window.tool).or_default().section;
            if window.sections.get(current).is_some_and(|section| section.index) {
                self.follow_index_section(registry, session, window, now);
                continue;
            }
            let view = self.plugin_tools.views.entry(window.tool).or_default();
            let Some(section) = window.sections.get(view.section) else { continue };
            if now < view.next {
                continue;
            }
            let params = json!({
                "plugin": window.plugin, "window": window.window, "section": section.id,
                "refresh": !view.listing, "force": view.force, "waitMs": 0,
            });
            view.force = false;
            match registry.dispatch(session, "plugins.toolWindow.rows", params) {
                Ok(answer) => {
                    view.rows = rows_of(&answer);
                    view.choices = choices_of(&answer);
                    view.error = answer["error"].as_str().map(str::to_owned);
                    view.busy = answer["busy"] == true;
                    view.listing = answer["listing"] == true;
                    let retry = answer["retryAfterMs"].as_u64().map(|ms| ms as f64 / 1000.0);
                    let wait = if view.listing || view.busy {
                        POLL_EVERY
                    } else {
                        retry.filter(|_| view.error.is_some()).unwrap_or(REFRESH_EVERY).max(POLL_EVERY)
                    };
                    view.next = now + wait;
                    view.selected.retain(|id| view.rows.iter().any(|row| &row.id == id));
                }
                Err(error) => {
                    view.rows.clear();
                    view.error = Some(error.to_string());
                    view.listing = false;
                    view.next = now + REFRESH_EVERY;
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((view.next - now).max(POLL_EVERY)));
        }
    }

    pub(super) fn follow_plugin_diffs(&mut self, registry: &Registry, session: &mut Session) {
        for result in zen_workbench::plugin_diff::take(session) {
            for object in result.objects.iter().take(MAX_DIFFS_OPENED) {
                let params = json!({
                    "left": object.left, "right": object.right,
                    "leftTitle": result.left_title(object), "rightTitle": result.right_title(object),
                });
                let _ = registry.dispatch(session, "file.compare.texts", params);
            }
        }
    }

    pub(super) fn act_on_plugin_rows(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        request: Request,
        confirmed: bool,
    ) {
        self.act_with(ui, registry, session, request, confirmed, None);
    }

    fn ask_for_values(&mut self, request: Request, token: Option<String>, question: Option<String>) {
        let (tool, action, rows) = request;
        let Some(window) = self.plugin_tools.window(tool).cloned() else { return };
        let section_index = self.plugin_tools.views.get(tool).map_or(0, |view| view.section);
        let Some(section) = window.sections.get(section_index) else { return };
        let Some(info) = section.actions.iter().find(|info| info.id == action) else { return };
        let row = self
            .plugin_tools
            .views
            .get(tool)
            .and_then(|view| view.rows.iter().find(|row| rows.first() == Some(&row.id)))
            .map(|row| ToolRow { id: row.id.clone(), columns: row.columns.clone(), state: row.state.clone() });
        let fields =
            info.inputs.iter().map(|input| Field::starting(input, &section.column_ids, row.as_ref())).collect();
        self.plugin_tools.asking =
            Some(InputDialog { tool, action, rows, title: info.title.clone(), fields, token, question });
    }

    fn act_with(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        request: Request,
        confirmed: bool,
        typed: Option<Value>,
    ) {
        let (tool, action, rows) = request;
        let action = action.as_str();
        let Some(window) = self.plugin_tools.window(tool).cloned() else { return };
        let Some(view) = self.plugin_tools.views.get_mut(tool) else { return };
        let Some(section) = window.sections.get(view.section) else { return };
        let Some(info) = section.actions.iter().find(|info| info.id == action) else { return };
        let title = info.title.clone();
        if !info.inputs.is_empty() && typed.is_none() {
            self.ask_for_values((tool, action.to_owned(), rows), None, None);
            return;
        }
        if info.kind == "destroy" && !info.confirms && !confirmed {
            let question = None;
            self.plugin_tools.confirm = Some(Confirm { tool, action: action.to_owned(), rows, title, question, typed });
            return;
        }
        let mut params = json!({
            "plugin": window.plugin, "window": window.window, "section": section.id,
            "action": action, "rows": if info.rows { rows.clone() } else { Vec::new() }, "confirmed": confirmed,
        });
        if let Some(typed) = &typed {
            params["inputs"] = typed.clone();
        }
        view.next = 0.0;
        match registry.dispatch(session, "plugins.toolWindow.act", params) {
            Ok(_) => {}
            Err(zen_core::DispatchError::Failed { error, .. })
                if !confirmed && error.code.as_deref() == Some("confirmationNeeded") =>
            {
                let question = Some(error.message);
                self.plugin_tools.confirm =
                    Some(Confirm { tool, action: action.to_owned(), rows, title, question, typed });
            }
            Err(error) => self.post(ui, error.to_string()),
        }
    }

    fn agent_wants_values(&mut self, session: &mut Session) {
        let Some(request) = zen_workbench::plugin_tools::take_entering(session) else { return };
        let Some(window) = self
            .plugin_tools
            .windows
            .iter()
            .find(|window| window.plugin == request.plugin && window.window == request.window)
        else {
            return;
        };
        let tool = window.tool;
        if let Some(index) = window.sections.iter().position(|section| section.id == request.section) {
            let view = self.plugin_tools.views.entry(tool).or_default();
            if view.section != index {
                *view = View { section: index, ..View::default() };
            }
        }
        self.show_tool(tool);
        self.ask_for_values((tool, request.action, request.rows), Some(request.token), request.question);
    }

    pub(super) fn plugin_tool_window(&mut self, ui: &mut Ui, tool: &str, registry: &Registry, session: &mut Session) {
        let Some(window) = self.plugin_tools.window(tool).cloned() else { return };
        let tool = window.tool;
        let p = self.palette;
        let view = self.plugin_tools.views.entry(tool).or_default();
        let current = view.section.min(window.sections.len().saturating_sub(1));
        let Some(section) = window.sections.get(current).cloned() else { return };
        if section.index {
            self.plugin_index_section(ui, &window, registry, session);
            return;
        }
        let selected: Vec<String> = view.selected.iter().cloned().collect();
        let (rows, error, busy) = (view.rows.clone(), view.error.clone(), view.busy);
        let choices = view.choices.clone();

        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            &window.title,
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with(("plugin-tool-title", tool)), title, || window.title.clone());
        let mut x = title.right() + 14.0;
        let mut picked_section = None;
        for (index, info) in window.sections.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(info.title.clone(), theme::regular(12.5), p.text);
            let rect = Rect::from_min_size(pos2(x, header.center().y - 11.0), vec2(galley.size().x + 14.0, 22.0));
            x = rect.right() + 4.0;
            let response = ui.interact(rect, ui.scope_id().with(("plugin-tool-section", tool, index)), Sense::click());
            if index == current {
                ui.painter().rect_filled(rect, 5, p.selection);
            } else {
                hover(ui, &response, &p);
            }
            ui.painter().galley(pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
            accessibility::button(&response, &info.title);
            accessibility::pressed(&response, index == current);
            if response.clicked() {
                picked_section = Some(index);
            }
        }
        x += 10.0;
        let mut button = |ui: &mut Ui, key: &str, glyph: Glyph, label: &str, enabled: bool| {
            let rect = Rect::from_center_size(pos2(x + 12.0, header.center().y), Vec2::splat(24.0));
            x += 26.0;
            let sense = if enabled { Sense::click() } else { Sense::hover() };
            let response = ui.interact(rect, ui.scope_id().with(("plugin-tool", tool, key)), sense);
            if enabled {
                hover(ui, &response, &p);
            }
            let color = if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) };
            glyph(ui.painter(), rect.shrink(6.0), color);
            hint(ui, &response, label, None, Side::Below);
            accessibility::button(&response, label);
            if !enabled {
                accessibility::disabled(&response);
            }
            enabled && response.clicked()
        };
        let mut chosen: Option<(String, Vec<String>)> = None;
        let refresh = button(ui, "refresh", glyph_refresh, &t!("ide-plugin-tool-refresh"), !busy);
        for info in &section.actions {
            let enabled = !busy && (!info.rows || !selected.is_empty());
            if button(ui, &info.id, glyph_for(info), &info.title, enabled) {
                chosen = Some((info.id.clone(), selected.clone()));
            }
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with(("plugin-tool-hide", tool)), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(tool);
        }

        let mut picked_choice: Option<(String, String)> = None;
        if window.choices && !choices.is_empty() {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            ui.scope_builder(egui::UiBuilder::new().max_rect(line.shrink2(vec2(12.0, 3.0))), |ui| {
                ui.horizontal_centered(|ui| {
                    for choice in &choices {
                        let label = t!("ide-plugin-tool-choice", choice = choice.title.as_str());
                        ui.label(egui::RichText::new(label).color(p.text_secondary).font(theme::regular(12.5)));
                        let shown = choice.value.clone().unwrap_or_else(|| t!("ide-plugin-tool-choice-none"));
                        let color = if choice.valid { p.text } else { p.error_text };
                        let combo = egui::ComboBox::from_id_salt(("plugin-tool-choice", tool, choice.id.as_str()))
                            .selected_text(egui::RichText::new(shown).color(color))
                            .show_ui(ui, |ui| {
                                for (id, label) in &choice.options {
                                    let current = choice.value.as_deref() == Some(id.as_str());
                                    if ui.selectable_label(current, label).clicked() && !current {
                                        picked_choice = Some((choice.id.clone(), id.clone()));
                                    }
                                }
                            });
                        accessibility::edit(&combo.response, |node| node.set_label(choice.title.clone()));
                        ui.add_space(14.0);
                    }
                });
            });
            for choice in choices.iter().filter(|choice| !choice.source.is_empty()) {
                let files: Vec<String> = choice
                    .source
                    .iter()
                    .map(|(path, exists)| {
                        if *exists { path.clone() } else { t!("ide-plugin-tool-choice-missing", path = path.as_str()) }
                    })
                    .collect();
                let note =
                    t!("ide-plugin-tool-choice-source", choice = choice.title.as_str(), files = files.join(", "));
                let (line, response) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                accessibility::name(&response, egui::Role::Label, &note);
                ui.painter().with_clip_rect(line).text(
                    pos2(line.left() + 12.0, line.center().y),
                    Align2::LEFT_CENTER,
                    &note,
                    theme::regular(11.5),
                    p.text_secondary,
                );
                hint(ui, &response, &note, None, Side::Below);
            }
        }

        let note = error.clone().or_else(|| busy.then(|| t!("ide-plugin-tool-busy")));
        if let Some(note) = note {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            let color = if error.is_some() { p.error_text } else { p.text_secondary };
            let first = note.lines().next().unwrap_or_default().to_owned();
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                first,
                theme::regular(12.0),
                color,
            );
        }

        let columns = section.columns.len().max(1);
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let start = head.left() + 30.0;
        let width = ((head.right() - start - 8.0) / columns as f32).max(40.0);
        for (index, column) in section.columns.iter().enumerate() {
            let at = pos2(start + index as f32 * width, head.center().y);
            ui.painter().with_clip_rect(head).text(
                at,
                Align2::LEFT_CENTER,
                column,
                theme::regular(12.0),
                p.text_secondary,
            );
        }
        let stream = section.actions.iter().find(|info| info.kind == "stream" && info.rows).map(|info| info.id.clone());
        let mut clicked: Option<(String, bool, bool)> = None;
        egui::ScrollArea::vertical().id_salt(("plugin-tool-rows", tool)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let is_selected = selected.contains(&row.id);
                let label = row.columns.first().cloned().unwrap_or_else(|| row.id.clone());
                accessibility::tree_item(&response, 1, None, is_selected, &label);
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if is_selected {
                    ui.painter().rect_filled(highlight, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                if let Some(color) = state_color(&p, row.state.as_deref()) {
                    ui.painter().circle_filled(pos2(rect.left() + 18.0, rect.center().y), 3.5, color);
                }
                for (index, value) in row.columns.iter().enumerate().take(columns) {
                    let cell = Rect::from_min_max(
                        pos2(start + index as f32 * width, rect.top()),
                        pos2(start + (index + 1) as f32 * width - 6.0, rect.bottom()),
                    );
                    let color = if index == 0 { p.text } else { p.text_secondary };
                    ui.painter().with_clip_rect(cell).text(
                        pos2(cell.left(), cell.center().y),
                        Align2::LEFT_CENTER,
                        value,
                        theme::regular(12.5),
                        color,
                    );
                }
                let toggle = ui.input(|input| input.modifiers.command || input.modifiers.shift);
                if response.clicked() || response.double_clicked() {
                    clicked = Some((row.id.clone(), response.double_clicked(), toggle));
                }
                response.context_menu(|ui| {
                    for info in &section.actions {
                        if ui.add_enabled(!busy, egui::Button::new(&info.title)).clicked() {
                            let rows = if is_selected { selected.clone() } else { vec![row.id.clone()] };
                            chosen = Some((info.id.clone(), rows));
                            ui.close();
                        }
                    }
                });
            }
            if rows.is_empty() && error.is_none() {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(t!("ide-plugin-tool-empty"))
                            .color(p.text_secondary)
                            .font(theme::regular(12.5)),
                    );
                });
            }
        });

        if let Some((choice, value)) = picked_choice {
            let params = json!({ "plugin": window.plugin, "window": window.window, "choice": choice, "value": value });
            match registry.dispatch(session, "plugins.toolWindow.choose", params) {
                Ok(_) => {
                    let view = self.plugin_tools.views.entry(tool).or_default();
                    view.rows.clear();
                    view.selected.clear();
                    view.next = 0.0;
                    view.force = true;
                }
                Err(error) => self.post(ui, error.to_string()),
            }
        }
        let view = self.plugin_tools.views.entry(tool).or_default();
        if let Some(index) = picked_section
            && index != view.section
        {
            *view = View { section: index, ..View::default() };
        }
        if refresh {
            view.next = 0.0;
            view.force = true;
        }
        if let Some((id, activate, toggle)) = clicked {
            if toggle && !view.selected.insert(id.clone()) {
                view.selected.remove(&id);
            } else if !toggle {
                view.selected = [id.clone()].into_iter().collect();
            }
            if activate && let Some(stream) = stream {
                chosen = Some((stream, vec![id]));
            }
        }
        if let Some((action, rows)) = chosen {
            self.act_on_plugin_rows(ui, registry, session, (tool, action, rows), false);
        }
        if let Some(Confirm { tool: confirm_tool, action, rows, title, question, typed }) =
            self.plugin_tools.confirm.clone()
            && confirm_tool == tool
        {
            match confirm_dialog(ui, &p, &title, question.as_deref(), rows.len()) {
                Some(true) => {
                    self.plugin_tools.confirm = None;
                    self.act_with(ui, registry, session, (tool, action, rows), true, typed);
                }
                Some(false) => self.plugin_tools.confirm = None,
                None => {}
            }
        }
        if self.plugin_tools.asking.as_ref().is_some_and(|dialog| dialog.tool == tool)
            && let Some(mut dialog) = self.plugin_tools.asking.take()
        {
            match input_dialog(ui, &p, &mut dialog) {
                Some(true) => {
                    let typed = dialog.values();
                    match dialog.token.clone() {
                        Some(token) => {
                            let params = json!({ "token": token, "inputs": typed });
                            if let Err(error) = registry.dispatch(session, "plugins.toolWindow.allow", params) {
                                self.post(ui, error.to_string());
                            }
                        }
                        None => self.act_with(ui, registry, session, (tool, dialog.action, dialog.rows), false, typed),
                    }
                }
                Some(false) => {}
                None => self.plugin_tools.asking = Some(dialog),
            }
        }
    }
}

fn confirm_dialog(ui: &mut Ui, p: &IdePalette, title: &str, asked: Option<&str>, count: usize) -> Option<bool> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("plugin-tool-confirm")).frame(super::dialog_frame(p)).show(
        ui.ctx(),
        |ui| {
            ui.set_width(400.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            let question = match asked {
                Some(asked) => asked.to_owned(),
                None if count == 0 => t!("ide-plugin-tool-confirm-section"),
                None => t!("ide-plugin-tool-confirm-rows", count = count),
            };
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(p.text)).wrap());
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let go = egui::Button::new(egui::RichText::new(title).font(theme::regular(13.0)).color(p.accent_text))
                    .min_size(vec2(96.0, 30.0))
                    .corner_radius(6)
                    .fill(p.accent);
                if ui.add(go).clicked() {
                    choice = Some(true);
                }
                if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    choice = Some(false);
                }
            });
        },
    );
    choice.or_else(|| modal.should_close().then_some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn listed() -> Value {
        json!({ "windows": [{
            "plugin": "dockerfile", "window": "docker", "title": "Docker",
            "sections": [{
                "id": "containers", "title": "Containers", "columns": ["name", "state"],
                "columnTitles": ["Name", "State"],
                "actions": [
                    { "id": "stop", "title": "Stop", "kind": "change", "rows": true },
                    { "id": "prune", "title": "Delete Stopped Containers", "kind": "destroy", "rows": false }
                ]
            }]
        }]})
    }

    #[test]
    fn a_plug_in_s_windows_become_tool_windows_once_they_are_active() {
        let windows = windows_of(&listed());
        assert_eq!(windows[0].tool, "docker");
        assert_eq!(windows[0].sections[0].columns, ["Name", "State"]);
        assert_eq!(windows[0].sections[0].actions[1].kind, "destroy");
        assert_eq!(windows_of(&listed())[0].tool.as_ptr(), windows[0].tool.as_ptr(), "an id is made once");
        let mut tools = PluginToolWindows::default();
        tools.set_windows(windows);
        assert_eq!(kind("docker").map(|kind| kind.title), Some("Docker"));
        assert_eq!(kind("docker").map(|kind| kind.action), Some("view.toolWindow.docker"));
        assert_eq!(declared("docker").map(|declared| declared.side), Some(Place::BottomLeft));
        tools.set_windows(Vec::new());
        assert!(kind("docker").is_none() && declared("docker").is_none(), "a disabled plug-in's window goes");
    }

    #[test]
    fn the_header_s_choices_and_a_confirming_action_are_read_from_the_answers() {
        let mut listed = listed();
        listed["windows"][0]["choices"] = json!([{ "id": "context", "title": "Context" }]);
        listed["windows"][0]["sections"][0]["actions"][0]["confirms"] = json!(true);
        let windows = windows_of(&listed);
        assert!(windows[0].choices && windows[0].sections[0].actions[0].confirms);
        assert!(!windows[0].sections[0].actions[1].confirms);
        let answer = json!({ "rows": [], "choices": [{
            "id": "context", "title": "Context", "value": "prod", "valid": false,
            "options": [{ "id": "dev", "label": "dev" }],
            "source": [{ "path": "/k/a.yaml", "exists": true }, { "path": "/k/gone.yaml", "exists": false }]
        }]});
        let choices = choices_of(&answer);
        assert_eq!(choices[0].value.as_deref(), Some("prod"));
        assert!(!choices[0].valid, "a context gone from the kubeconfig is shown as such");
        assert_eq!(choices[0].options, [("dev".to_owned(), "dev".to_owned())]);
        assert_eq!(choices[0].source, [("/k/a.yaml".to_owned(), true), ("/k/gone.yaml".to_owned(), false)]);
    }

    #[test]
    fn a_window_named_like_a_built_in_one_keeps_its_plug_in_in_its_id() {
        assert_eq!(tool_id("acme", "terminal"), "acme-terminal");
        assert_eq!(tool_id("dockerfile", "docker"), "docker");
    }

    #[test]
    fn rows_and_their_state_are_read_from_the_answer() {
        let rows = rows_of(
            &json!({ "rows": [{ "id": "c1", "columns": ["web", "running"], "state": "running" }, { "id": "c2", "columns": [] }] }),
        );
        assert_eq!(
            rows[0],
            RowInfo { id: "c1".into(), columns: vec!["web".into(), "running".into()], state: Some("running".into()) }
        );
        assert_eq!(rows[1].state, None);
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        assert_eq!(state_color(&p, Some("running")), Some(p.run));
        assert_eq!(state_color(&p, None), None);
    }

    #[test]
    fn a_window_opens_first_on_its_declared_side_and_then_where_the_person_put_it() {
        let mut listed = listed();
        listed["windows"][0]["window"] = json!("sided");
        listed["windows"][0]["side"] = json!("right");
        listed["windows"][0]["icon"] = json!(true);
        let windows = windows_of(&listed);
        assert_eq!((windows[0].side, windows[0].icon), (Place::RightTop, true));
        assert_eq!(side_of("left"), Place::LeftTop);
        assert_eq!(side_of("sideways"), Place::BottomLeft);
        let mut layout = super::super::tool_windows::ToolWindowLayout::default();
        layout.ensure(windows[0].tool, windows[0].side);
        assert_eq!(layout.side_of(windows[0].tool), Some(Place::RightTop));
        layout.move_to(windows[0].tool, Place::BottomLeft, None);
        layout.ensure(windows[0].tool, windows[0].side);
        assert_eq!(layout.side_of(windows[0].tool), Some(Place::BottomLeft), "the person's place is kept");
        let tools = PluginToolWindows { windows: windows.clone(), ..PluginToolWindows::default() };
        assert_eq!(tools.menu_items(), [("view.toolWindow.sided".to_owned(), "Docker".to_owned())]);
    }

    #[test]
    fn a_window_kept_elsewhere_is_noted_once_and_never_moved() {
        let mut answer = listed();
        answer["windows"][0]["window"] = json!("noted");
        answer["windows"][0]["side"] = json!("right");
        let windows = windows_of(&answer);
        let tool = windows[0].tool;
        let none = BTreeSet::new();
        assert_eq!(moved(&windows, &|_| Some(Place::BottomLeft), &none), [tool.to_owned()]);
        assert!(moved(&windows, &|_| Some(Place::RightBottom), &none).is_empty(), "already on the right");
        assert!(moved(&windows, &|_| None, &none).is_empty(), "a new window simply opens on its side");
        let noted: BTreeSet<String> = [tool.to_owned()].into_iter().collect();
        assert!(moved(&windows, &|_| Some(Place::BottomLeft), &noted).is_empty(), "said once");
        let bottom = windows_of(&listed());
        assert!(moved(&bottom, &|_| Some(Place::RightTop), &none).is_empty(), "no side declared, nothing to say");
    }

    #[test]
    fn an_icon_is_a_texture_of_its_shapes_and_a_hostile_one_falls_back_to_the_glyph() {
        let ctx = egui::Context::default();
        let good = r#"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="5"/></svg>"#;
        let hostile = format!(
            r#"<svg viewBox="0 0 16 16">{}<image href="file:///etc/passwd"/>{}</svg>"#,
            "<g>".repeat(40),
            "</g>".repeat(40)
        );
        {
            let mut svgs = svgs().lock().unwrap_or_else(PoisonError::into_inner);
            svgs.insert("icon-good", good.to_owned());
            svgs.insert("icon-hostile", hostile);
        }
        assert!(icon_texture(&ctx, "icon-good", 18).is_some_and(|texture| texture.size() == [18, 18]));
        assert!(icon_texture(&ctx, "icon-hostile", 18).is_none());
        assert!(icon_texture(&ctx, "icon-none", 18).is_none());
    }
}
