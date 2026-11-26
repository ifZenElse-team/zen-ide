use egui::{Align2, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::terminal::{Direction, LinkAction, Node, Terminals};

use super::glyphs::{glyph_minus, glyph_more};
use super::terminal_link_dialog::{self, Choice, LinkDialog};
use super::{IdeView, Side, customization, hint, hover, metrics};
use crate::accessibility;
use crate::terminal::{LinkClick, PaneView};
use crate::theme;

const TAB_TEXT_MAX: f32 = 200.0;
const TAB_TEXT_MIN: f32 = 24.0;
const TAB_PADDING: f32 = 40.0;
const TAB_GAP: f32 = 4.0;
const TAB_BUTTONS: f32 = 52.0;

const WHEEL_STEP: f32 = 40.0;

#[derive(Clone, Debug)]
pub(super) struct TerminalHeader {
    pub(super) first: usize,
    wheel: f32,
    #[cfg_attr(not(test), expect(dead_code, reason = "the tests check that the tabs lie inside it"))]
    pub(super) strip: Rect,
    pub(super) tabs: Vec<(usize, Rect)>,
    pub(super) buttons: Vec<(&'static str, Rect)>,
}

impl Default for TerminalHeader {
    fn default() -> Self {
        Self { first: 0, wheel: 0.0, strip: Rect::NOTHING, tabs: Vec::new(), buttons: Vec::new() }
    }
}

fn span(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + TAB_GAP * widths.len().saturating_sub(1) as f32
}

fn visible_tabs(widths: &[f32], available: f32, active: Option<usize>, first: usize) -> std::ops::Range<usize> {
    let count = widths.len();
    if count == 0 {
        return 0..0;
    }
    let mut first = first.min(count - 1);
    if let Some(active) = active.filter(|active| *active < count) {
        first = first.min(active);
        while first < active && span(&widths[first..=active]) > available {
            first += 1;
        }
    }
    let fits = |from: usize| (from + 1..=count).take_while(|end| span(&widths[from..*end]) <= available).last();
    let mut end = fits(first).unwrap_or(first + 1);
    while end == count && first > 0 && span(&widths[first - 1..end]) <= available {
        first -= 1;
        end = fits(first).unwrap_or(first + 1);
    }
    first..end
}

enum Action {
    Select(usize),
    CloseTab(usize),
    New,
    Split(u64, Direction),
    Close(u64),
    StartRename(usize),
    Rename(usize, String),
    Focus(u64),
    Ratio(u64, f32),
    Hide,
    Link(u64, LinkClick),
    Copy(u64, bool),
    Paste(u64),
    SelectAll(u64),
    Rerun(u64),
    DismissRerun(u64),
}

const RERUN_BAR: f32 = 30.0;

impl IdeView {
    pub(super) fn follow_terminals(&mut self, session: &mut Session) {
        let shown = self.tool_visible("terminal");
        if !shown {
            self.terminal_focus = None;
            self.terminal_drawn.clear();
        }
        let restore = session.settings().get_or(zen_workbench::terminal::RESTORE, true);
        let terminals = session.state_mut::<Terminals>();
        terminals.set_restore(restore);
        let reveal = terminals.take_reveal();
        let ended = terminals.prune();
        let gone = ended && terminals.tabs().is_empty() && !terminals.has_pending();
        if reveal && !shown {
            self.show_tool("terminal");
        }
        if gone && self.tool_visible("terminal") {
            self.hide_tool("terminal");
        }
    }

    pub(super) fn follow_index_reveal(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let Some(path) = session.state_mut::<zen_workbench::project_index::Indexes>().take_reveal() else { return };
        let target = (super::select_in::Target::ProjectView, path.as_path());
        match self.select_in_target(ctx, registry, session, target, &json!({}), false) {
            Ok(_) => {}
            Err(super::agent::ActionError::Refused(message)) => self.post_at(ctx, message),
            Err(super::agent::ActionError::Dispatch(error)) => self.post_at(ctx, error.to_string()),
        }
    }

    pub(super) fn follow_terminal_links(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let Some(path) = session.state_mut::<Terminals>().take_reveal_in_project() else { return };
        let target = (super::select_in::Target::ProjectView, path.as_path());
        match self.select_in_target(ctx, registry, session, target, &json!({}), false) {
            Ok(_) => {}
            Err(super::agent::ActionError::Refused(message)) => self.post_at(ctx, message),
            Err(super::agent::ActionError::Dispatch(error)) => self.post_at(ctx, error.to_string()),
        }
    }

    fn open_terminal_link(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        id: u64,
        click: &LinkClick,
        action: LinkAction,
    ) {
        let params = json!({ "target": click.command_target(), "action": action, "id": id });
        self.dispatch(ui, registry, session, "terminal.link.open", params);
    }

    fn terminal_link_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = self.terminal_link.as_mut() else { return };
        let product = (self.product.id, self.product.name);
        let Some(choice) = terminal_link_dialog::show(ui, &self.palette, product, dialog) else { return };
        let Some(dialog) = self.terminal_link.take() else { return };
        let elsewhere =
            matches!(choice, Choice::Act { action: LinkAction::OpenInEditor | LinkAction::RevealInProject, .. });
        if !elsewhere && let Some(view) = self.terminal_panes.get_mut(&dialog.session) {
            view.request_focus();
        }
        if let Choice::Act { action, remember } = choice {
            if remember {
                let mut settings = self.overview.settings.clone();
                settings.terminal_links.set(dialog.click.kind, Some(action));
                self.overview.settings = settings.clone();
                self.requests.push(super::app::Request::Settings(Box::new(settings)));
            }
            self.open_terminal_link(ui, registry, session, dialog.session, &dialog.click, action);
        }
    }

    pub(super) fn terminal_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Err(error) = zen_workbench::terminal::restore_pending(session) {
            self.post(ui, error.message);
        }
        session.state_mut::<Terminals>().prune();
        if session.state::<Terminals>().is_none_or(|terminals| terminals.tabs().is_empty()) {
            if let Err(error) = registry.dispatch(session, "terminal.session.new", json!({})) {
                self.report(ui, error);
                self.hide_tool("terminal");
                return;
            }
            session.state_mut::<Terminals>().take_reveal();
        }
        let mut actions = Vec::new();
        self.terminal_drawn.clear();
        if let Some(terminals) = session.state::<Terminals>() {
            self.terminal_header(ui, terminals, &mut actions);
            if let Some(index) = terminals.active() {
                let tab = &terminals.tabs()[index];
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                let project = session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project);
                let project = project.map(std::path::Path::to_owned);
                self.terminal_node(ui, tab.root(), rect, (index, tab.focused(), project.as_deref()), &mut actions);
                self.terminal_focus = Some(tab.focused());
            }
            let live: Vec<u64> =
                terminals.tabs().iter().flat_map(|tab| tab.root().panes()).map(|pane| pane.id()).collect();
            self.terminal_panes.retain(|id, _| live.contains(id));
        }
        for action in actions {
            self.terminal_action(ui, registry, session, action);
        }
        self.terminal_link_dialog(ui, registry, session);
        if session.state::<Terminals>().is_none_or(|terminals| terminals.tabs().is_empty()) {
            self.hide_tool("terminal");
        }
    }

    fn terminal_action(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, action: Action) {
        let (command, params) = match action {
            Action::Select(tab) => ("terminal.tab.select", json!({ "tab": tab })),
            Action::CloseTab(tab) => ("terminal.tab.close", json!({ "tab": tab })),
            Action::New => ("terminal.session.new", json!({})),
            Action::Split(id, direction) => ("terminal.session.split", json!({ "id": id, "direction": direction })),
            Action::Close(id) => ("terminal.session.close", json!({ "id": id })),
            Action::Copy(id, plain) => ("terminal.selection.copy", json!({ "id": id, "plain": plain })),
            Action::Paste(id) => ("terminal.session.paste", json!({ "id": id })),
            Action::SelectAll(id) => ("terminal.selection.selectAll", json!({ "id": id })),
            Action::Rerun(id) => ("terminal.session.rerun", json!({ "id": id })),
            Action::DismissRerun(id) => ("terminal.session.dismissRerun", json!({ "id": id })),
            Action::Rename(tab, name) => ("terminal.tab.rename", json!({ "tab": tab, "name": name })),
            Action::Focus(id) => ("terminal.session.focus", json!({ "id": id })),
            Action::StartRename(tab) => {
                let title =
                    session.state::<Terminals>().and_then(|terminals| terminals.tabs().get(tab)).map(|tab| tab.title());
                self.terminal_rename = title.map(|title| (tab, title, false));
                return;
            }
            Action::Ratio(split, ratio) => {
                session.state_mut::<Terminals>().set_ratio(split, ratio);
                return;
            }
            Action::Hide => {
                self.hide_tool("terminal");
                return;
            }
            Action::Link(id, click) => {
                match self.overview.settings.terminal_links.get(click.kind) {
                    Some(action) => self.open_terminal_link(ui, registry, session, id, &click, action),
                    None => self.terminal_link = Some(LinkDialog { session: id, click, remember: false }),
                }
                return;
            }
        };
        self.dispatch(ui, registry, session, command, params);
    }

    fn terminal_header(&mut self, ui: &mut Ui, terminals: &Terminals, actions: &mut Vec<Action>) {
        let p = self.palette;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-terminal-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("terminal-title"), title, || t!("ide-terminal-title"));
        let hide = Rect::from_center_size(pos2(header.right() - 20.0, header.center().y), Vec2::splat(24.0));
        let more = Rect::from_center_size(pos2(hide.left() - 16.0, header.center().y), Vec2::splat(24.0));
        let strip_left = title.right() + 14.0;
        let available = (more.left() - 8.0 - TAB_BUTTONS - strip_left).max(0.0);
        let text_max = TAB_TEXT_MAX.min(available - TAB_PADDING).max(TAB_TEXT_MIN);
        let active = terminals.active();
        let font = theme::regular(13.0);
        let tabs: Vec<(String, std::sync::Arc<egui::Galley>, bool)> = terminals
            .tabs()
            .iter()
            .map(|tab| {
                let name = tab.title();
                let full = ui.painter().layout_no_wrap(name.clone(), font.clone(), p.text).size().x;
                let galley = ui.painter().layout_job(super::vcs::truncated(&name, font.clone(), p.text, text_max));
                (name, galley, full > text_max)
            })
            .collect();
        let widths: Vec<f32> = tabs
            .iter()
            .enumerate()
            .map(|(index, (_, galley, _))| {
                let renaming = self.terminal_rename.as_ref().is_some_and(|(at, ..)| *at == index);
                let text = if renaming { galley.size().x.max(120.0).min(text_max) } else { galley.size().x };
                text + TAB_PADDING
            })
            .collect();
        let strip = Rect::from_min_max(pos2(strip_left, header.top()), pos2(strip_left + available, header.bottom()));
        let mut first = self.terminal_header.first;
        let mut wheel = self.terminal_header.wheel;
        if ui.rect_contains_pointer(strip) {
            let delta = ui.input(|input| input.smooth_scroll_delta);
            wheel -= delta.x + delta.y;
            while wheel >= WHEEL_STEP {
                wheel -= WHEEL_STEP;
                first += 1;
            }
            while wheel <= -WHEEL_STEP {
                wheel += WHEEL_STEP;
                first = first.saturating_sub(1);
            }
        } else {
            wheel = 0.0;
        }
        let visible = visible_tabs(&widths, available, active, first);
        let mut drawn = TerminalHeader { first: visible.start, wheel, strip, ..TerminalHeader::default() };
        let mut x = strip.left();
        for (index, (name, galley, cut)) in tabs.into_iter().enumerate() {
            if !visible.contains(&index) {
                continue;
            }
            let rect = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(widths[index], 26.0));
            x = rect.right() + TAB_GAP;
            drawn.tabs.push((index, rect));
            let renaming = self.terminal_rename.as_ref().is_some_and(|(at, ..)| *at == index);
            if renaming {
                self.terminal_rename_field(ui, rect, actions);
                continue;
            }
            let painter = ui.painter().with_clip_rect(rect);
            let response = ui.interact(rect, ui.scope_id().with(("terminal-tab", index)), Sense::click());
            if cut {
                hint(ui, &response, &name, None, Side::Below);
            }
            accessibility::item(&response, egui::Role::Tab, Some(index) == active, &name);
            if Some(index) == active {
                painter.rect_filled(rect, 6, p.selection);
            } else if response.hovered() {
                painter.rect_filled(rect, 6, p.hover);
            }
            painter.galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
            let cross = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), Vec2::splat(14.0));
            let cross_response = ui.interact(cross, ui.scope_id().with(("terminal-close", index)), Sense::click());
            accessibility::button_with(&cross_response, || t!("a11y-close-tab", name = name.as_str()));
            let c = cross.shrink(3.5);
            let color = if cross_response.hovered() { p.text } else { p.text_secondary };
            painter.line_segment([c.left_top(), c.right_bottom()], Stroke::new(1.2, color));
            painter.line_segment([c.right_top(), c.left_bottom()], Stroke::new(1.2, color));
            if cross_response.clicked() {
                actions.push(Action::CloseTab(index));
            } else if response.double_clicked() {
                actions.push(Action::StartRename(index));
            } else if response.clicked() {
                actions.push(Action::Select(index));
            }
            response.context_menu(|ui| {
                customization::popup(
                    ui,
                    customization::TERMINAL_TAB_POPUP,
                    |_| true,
                    |ui, slot| {
                        let chosen = match slot {
                            "ide-terminal-rename" => Action::StartRename(index),
                            _ => Action::CloseTab(index),
                        };
                        if ui.button(t!(slot)).clicked() {
                            actions.push(chosen);
                            ui.close();
                        }
                    },
                );
            });
        }
        let used = if drawn.tabs.is_empty() { strip.left() - TAB_GAP } else { x };
        let plus = Rect::from_center_size(pos2(used - TAB_GAP + 14.0, header.center().y), Vec2::splat(24.0));
        let plus_response = ui.interact(plus, ui.scope_id().with("terminal-new"), Sense::click());
        accessibility::button_with(&plus_response, || t!("ide-terminal-new"));
        hover(ui, &plus_response, &p);
        let c = plus.shrink(7.0);
        ui.painter().line_segment([c.left_center(), c.right_center()], Stroke::new(1.3, p.text_secondary));
        ui.painter().line_segment([c.center_top(), c.center_bottom()], Stroke::new(1.3, p.text_secondary));
        hint(
            ui,
            &plus_response,
            &t!("ide-terminal-new"),
            crate::keymap::shortcut("terminal.session.new").as_deref(),
            Side::Below,
        );
        if plus_response.clicked() {
            actions.push(Action::New);
        }
        let chooser = Rect::from_center_size(pos2(plus.right() + 14.0, header.center().y), Vec2::splat(24.0));
        let chooser_response = ui.interact(chooser, ui.scope_id().with("terminal-tabs"), Sense::click());
        hover(ui, &chooser_response, &p);
        crate::icons::chevron_down(ui.painter(), chooser.shrink(7.0), p.text_secondary);
        hint(ui, &chooser_response, &t!("a11y-terminal-tabs"), None, Side::Below);
        crate::components::popup_menu(&chooser_response, |ui| {
            for (index, tab) in terminals.tabs().iter().enumerate() {
                if ui.add(egui::Button::selectable(Some(index) == active, tab.title())).clicked() {
                    actions.push(Action::Select(index));
                    ui.close();
                }
            }
        });
        let hide_response = ui.interact(hide, ui.scope_id().with("terminal-hide"), Sense::click());
        accessibility::button_with(&hide_response, || t!("a11y-hide"));
        hover(ui, &hide_response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        if hide_response.clicked() {
            actions.push(Action::Hide);
        }
        let more_response = ui.interact(more, ui.scope_id().with("terminal-more"), Sense::click());
        accessibility::button_with(&more_response, || t!("a11y-options"));
        hover(ui, &more_response, &p);
        glyph_more(ui.painter(), more.shrink(6.0), p.text_secondary);
        hint(ui, &more_response, &t!("ide-tool-options"), None, Side::Below);
        let session_shown = active.zip(terminals.focused()).map(|(index, focused)| {
            let selected = terminals.pane(focused).is_some_and(|pane| pane.session().has_selection());
            (index, focused, selected)
        });
        crate::components::popup_menu(&more_response, |ui| {
            if let Some(shown) = session_shown {
                session_menu(ui, shown, actions);
                ui.separator();
            }
            self.tool_window_menu(ui, "terminal", false);
        });
        drawn.buttons = vec![("new", plus), ("tabs", chooser), ("more", more), ("hide", hide)];
        self.terminal_header = drawn;
    }

    fn terminal_rename_field(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let Some((index, text, seen)) = self.terminal_rename.as_mut() else { return };
        let id = ui.scope_id().with("terminal-rename");
        let response = ui.put(rect, egui::TextEdit::singleline(text).id(id).font(theme::regular(13.0)));
        if !*seen {
            response.request_focus();
            *seen = true;
            return;
        }
        let (enter, escape) = ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
        if escape {
            self.terminal_rename = None;
        } else if enter || response.lost_focus() {
            actions.push(Action::Rename(*index, text.clone()));
            self.terminal_rename = None;
        }
    }

    fn terminal_node(
        &mut self,
        ui: &mut Ui,
        node: &Node,
        rect: Rect,
        (tab, focused, project): (usize, u64, Option<&std::path::Path>),
        actions: &mut Vec<Action>,
    ) {
        match node {
            Node::Pane(pane) => {
                let id = pane.id();
                let rect = match pane.rerun() {
                    Some(command) if rect.height() > RERUN_BAR * 2.0 => {
                        let bar = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RERUN_BAR));
                        self.rerun_bar(ui, bar, id, command, actions);
                        Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max)
                    }
                    _ => rect,
                };
                let palette = self.terminal_palette;
                let changed = self.terminal_focus != Some(focused);
                let view = self
                    .terminal_panes
                    .entry(id)
                    .or_insert_with(|| PaneView::new(egui::Id::unique(("zen-terminal-pane", id))));
                if changed && id == focused {
                    view.request_focus();
                }
                if self.terminal_link.is_some() {
                    let widget = view.id();
                    ui.memory_mut(|memory| memory.surrender_focus(widget));
                }
                view.set_link_bases(
                    std::iter::once(pane.cwd().to_owned()).chain(project.map(std::path::Path::to_owned)).collect(),
                );
                let widget = view.id();
                let response = view.show(ui, rect, pane.session(), &palette);
                if view.take_copy() {
                    actions.push(Action::Copy(id, false));
                }
                if let Some(click) = view.take_link_click() {
                    actions.push(Action::Link(id, click));
                }
                if (response.clicked() || response.secondary_clicked()) && id != focused {
                    actions.push(Action::Focus(id));
                }
                let selected = pane.session().has_selection();
                response.context_menu(|ui| session_menu(ui, (tab, id, selected), actions));
                self.terminal_drawn.push((id, widget, rect));
            }
            Node::Split(split) => {
                let p = self.palette;
                let right = split.direction == Direction::Right;
                let (first, second, line) = if right {
                    let x = (rect.left() + rect.width() * split.ratio).round();
                    let first = Rect::from_min_max(rect.min, pos2(x, rect.bottom()));
                    let second = Rect::from_min_max(pos2(x + 1.0, rect.top()), rect.max);
                    (first, second, Rect::from_min_max(pos2(x, rect.top()), pos2(x + 1.0, rect.bottom())))
                } else {
                    let y = (rect.top() + rect.height() * split.ratio).round();
                    let first = Rect::from_min_max(rect.min, pos2(rect.right(), y));
                    let second = Rect::from_min_max(pos2(rect.left(), y + 1.0), rect.max);
                    (first, second, Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), y + 1.0)))
                };
                self.terminal_node(ui, &split.first, first, (tab, focused, project), actions);
                self.terminal_node(ui, &split.second, second, (tab, focused, project), actions);
                ui.painter().rect_filled(line, 0.0, p.tab_border);
                let handle = line.expand2(if right { vec2(3.0, 0.0) } else { vec2(0.0, 3.0) });
                let response = ui.interact(handle, ui.scope_id().with(("terminal-split", split.id)), Sense::drag());
                if response.hovered() || response.dragged() {
                    let icon =
                        if right { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
                    ui.ctx().set_cursor_icon(icon);
                }
                if response.dragged()
                    && let Some(pointer) = response.interact_pointer_pos()
                {
                    let ratio = if right {
                        (pointer.x - rect.left()) / rect.width().max(1.0)
                    } else {
                        (pointer.y - rect.top()) / rect.height().max(1.0)
                    };
                    actions.push(Action::Ratio(split.id, ratio));
                }
            }
        }
    }
}

impl IdeView {
    fn rerun_bar(&self, ui: &mut Ui, bar: Rect, id: u64, command: &str, actions: &mut Vec<Action>) {
        let p = self.palette;
        ui.painter().rect_filled(bar, 0.0, p.banner);
        ui.painter().rect_filled(Rect::from_min_max(pos2(bar.left(), bar.bottom() - 1.0), bar.max), 0.0, p.tab_border);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 4.0))));
        child.horizontal_centered(|ui| {
            let shown: String = command.chars().take(120).collect();
            ui.label(egui::RichText::new(t!("terminal-rerun-offer", command = shown)).color(p.text));
            if ui.button(t!("terminal-rerun-run")).clicked() {
                actions.push(Action::Rerun(id));
            }
            if ui.button(t!("terminal-rerun-dismiss")).clicked() {
                actions.push(Action::DismissRerun(id));
            }
        });
    }
}

fn clipboard_keys(action: &str) -> &'static str {
    let mac = cfg!(target_os = "macos");
    match action {
        "terminal.selection.copy" if mac => "⌘C",
        "terminal.selection.copy" => "Ctrl+Shift+C",
        "terminal.session.paste" if mac => "⌘V",
        "terminal.session.paste" => "Ctrl+Shift+V",
        "terminal.selection.selectAll" if mac => "⌘A",
        _ => "",
    }
}

fn session_menu(ui: &mut Ui, (tab, id, selected): (usize, u64, bool), actions: &mut Vec<Action>) {
    let shortcut = |action: &str| crate::keymap::shortcut(action).unwrap_or_else(|| clipboard_keys(action).to_owned());
    let mut item = |ui: &mut Ui, label: String, action: &str, chosen: Action| {
        let enabled = selected || !matches!(chosen, Action::Copy(..));
        let button = egui::Button::new(label).shortcut_text(shortcut(action));
        if ui.add_enabled(enabled, button).clicked() {
            actions.push(chosen);
            ui.close();
        }
    };
    customization::popup(
        ui,
        customization::TERMINAL_POPUP,
        |_| true,
        |ui, slot| {
            let (action, chosen) = match slot {
                "ide-terminal-copy" => ("terminal.selection.copy", Action::Copy(id, false)),
                "ide-terminal-copy-plain" => ("", Action::Copy(id, true)),
                "ide-terminal-paste" => ("terminal.session.paste", Action::Paste(id)),
                "ide-terminal-select-all" => ("terminal.selection.selectAll", Action::SelectAll(id)),
                "ide-terminal-split-right" => ("terminal.session.split/right", Action::Split(id, Direction::Right)),
                "ide-terminal-split-down" => ("terminal.session.split/down", Action::Split(id, Direction::Down)),
                "ide-terminal-new" => ("terminal.session.new", Action::New),
                "ide-terminal-rename" => ("", Action::StartRename(tab)),
                _ => ("terminal.session.close", Action::Close(id)),
            };
            item(ui, t!(slot), action, chosen);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_that_fit_are_all_shown() {
        assert_eq!(visible_tabs(&[100.0, 80.0], 400.0, Some(1), 1), 0..2, "room for all: from the first");
        assert_eq!(visible_tabs(&[], 400.0, None, 3), 0..0);
    }

    #[test]
    fn only_whole_tabs_are_shown_and_the_active_one_always() {
        let widths = [240.0, 240.0, 240.0, 240.0];
        assert_eq!(visible_tabs(&widths, 300.0, Some(2), 0), 2..3, "moved to the active tab");
        assert_eq!(visible_tabs(&widths, 500.0, Some(2), 0), 1..3, "as little as needed: two fit");
        assert_eq!(visible_tabs(&widths, 500.0, Some(0), 2), 0..2, "back to an earlier tab");
        assert_eq!(visible_tabs(&widths, 500.0, Some(1), 1), 1..3, "a first tab that shows it is kept");
        assert_eq!(visible_tabs(&widths, 500.0, Some(3), 3), 2..4, "room after the last tab is filled");
        assert_eq!(visible_tabs(&widths, 500.0, Some(3), 9), 2..4, "never past the last tab");
        assert_eq!(visible_tabs(&widths, 100.0, Some(1), 0), 1..2, "a tab wider than the strip still shows");
    }
}
