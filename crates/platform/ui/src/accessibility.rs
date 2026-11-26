use std::ops::Range;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use egui::accesskit::{Action, Live, Node, NodeId, Role, Toggled, TreeUpdate};
use egui::{Context, Id, Rect, Response, Sense, Ui, WidgetInfo};

const ANNOUNCE_EVERY: f64 = 1.0;
const ANNOUNCE_LINES: usize = 3;
const ANNOUNCE_CHARS: usize = 300;

pub fn reduce_motion() -> bool {
    if !cfg!(target_os = "macos") || cfg!(test) {
        return false;
    }
    static CACHE: OnceLock<Mutex<(bool, Option<Instant>, bool)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((false, None, false)));
    let mut state = cache.lock().unwrap_or_else(PoisonError::into_inner);
    let due = state.1.is_none_or(|read| read.elapsed() >= Duration::from_secs(60));
    if due && !state.2 {
        state.2 = true;
        let _ = std::thread::Builder::new().name("zen-reduce-motion".to_owned()).spawn(move || {
            let output = std::process::Command::new("/usr/bin/defaults")
                .args(["read", "com.apple.universalaccess", "reduceMotion"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            let reduce = output.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "1");
            let mut state = cache.lock().unwrap_or_else(PoisonError::into_inner);
            *state = (reduce, Some(Instant::now()), false);
        });
    }
    state.0
}

pub fn active(ctx: &Context) -> bool {
    ctx.accesskit_node_builder(egui::accesskit_root_id(), |_| ()).is_some()
}

pub fn name(response: &Response, role: Role, name: &str) {
    response.widget_info(|| WidgetInfo::labeled(role, response.enabled(), name));
}

pub fn name_with(response: &Response, role: Role, name: impl Fn() -> String) {
    response.widget_info(|| WidgetInfo::labeled(role, response.enabled(), name()));
}

pub fn button(response: &Response, label: &str) {
    name(response, Role::Button, label);
}

pub fn button_with(response: &Response, label: impl Fn() -> String) {
    name_with(response, Role::Button, label);
}

pub fn pressed(response: &Response, on: bool) {
    edit(response, |node| node.set_toggled(if on { Toggled::True } else { Toggled::False }));
}

pub fn disabled(response: &Response) {
    edit(response, Node::set_disabled);
}

pub fn item(response: &Response, role: Role, selected: bool, label: &str) {
    name(response, role, label);
    edit(response, |node| node.set_selected(selected));
}

pub fn tree_item(response: &Response, level: usize, expanded: Option<bool>, selected: bool, label: &str) {
    item(response, Role::TreeItem, selected, label);
    edit(response, |node| {
        node.set_level(level.max(1));
        if let Some(expanded) = expanded {
            node.set_expanded(expanded);
        }
    });
}

pub fn labelled(response: &Response, label: &str) {
    edit(response, |node| node.set_label(label));
}

pub fn describe(response: &Response, description: &str) {
    if !description.is_empty() {
        edit(response, |node| node.set_description(description.to_owned()));
    }
}

pub fn edit(response: &Response, change: impl FnOnce(&mut Node)) {
    edit_id(&response.ctx, response.id, change);
}

pub fn edit_id(ctx: &Context, id: Id, change: impl FnOnce(&mut Node)) {
    ctx.accesskit_node_builder(id, change);
}

pub fn label(ui: &Ui, id: Id, rect: Rect, text: impl FnOnce() -> String) {
    painted(ui, id, rect, Role::Label, text);
}

pub fn heading(ui: &Ui, id: Id, rect: Rect, text: impl FnOnce() -> String) {
    painted(ui, id, rect, Role::Heading, text);
}

pub fn announce(ui: &Ui, id: Id, rect: Rect, text: impl FnOnce() -> String) {
    if let Some(response) = painted(ui, id, rect, Role::Status, || brief(&text())) {
        edit(&response, |node| node.set_live(Live::Polite));
    }
}

#[derive(Debug, Default)]
pub struct Announcer {
    announced: usize,
    at: Option<f64>,
    text: String,
}

impl Announcer {
    pub fn show<'a>(
        &mut self,
        ui: &Ui,
        id: Id,
        rect: Rect,
        total: usize,
        lines: impl FnOnce(Range<usize>) -> Vec<&'a str>,
    ) {
        if !active(ui.ctx()) {
            self.announced = total;
            self.at = None;
            self.text.clear();
            return;
        }
        self.announced = self.announced.min(total);
        if self.announced < total {
            let now = ui.input(|input| input.time);
            let wait = self.at.map_or(0.0, |at| at + ANNOUNCE_EVERY - now);
            if wait <= 0.0 {
                let from = total.saturating_sub(ANNOUNCE_LINES).max(self.announced);
                self.text = lines(from..total).join("\n");
                self.announced = total;
                self.at = Some(now);
            } else {
                ui.ctx().request_repaint_after_secs(wait as f32);
            }
        }
        let text = &self.text;
        announce(ui, id, rect, || text.clone());
    }
}

fn brief(text: &str) -> String {
    match text.char_indices().nth(ANNOUNCE_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

pub fn unnamed(update: &TreeUpdate) -> Vec<String> {
    update
        .nodes
        .iter()
        .filter(|(_, node)| {
            egui::accessibility::is_input(node.role())
                || (node.supports_action(Action::Click) && node.role() != Role::ScrollBar)
        })
        .filter(|(_, node)| node.role() == Role::Unknown || accessible_name(node, &update.nodes).is_none())
        .map(|(id, node)| format!("{id:?}: {:?} at {:?}", node.role(), node.bounds()))
        .collect()
}

pub fn accessible_name(node: &Node, nodes: &[(NodeId, Node)]) -> Option<String> {
    let own = node.label().or_else(|| (node.role() == Role::Label).then(|| node.value()).flatten());
    if let Some(label) = own.filter(|label| !label.trim().is_empty()) {
        return Some(label.to_owned());
    }
    let by: Vec<&str> = node
        .labelled_by()
        .iter()
        .filter_map(|id| nodes.iter().find(|(other, _)| other == id))
        .filter_map(|(_, other)| other.label().or(other.value()))
        .filter(|text| !text.trim().is_empty())
        .collect();
    (!by.is_empty()).then(|| by.join(" "))
}

fn painted(ui: &Ui, id: Id, rect: Rect, role: Role, text: impl FnOnce() -> String) -> Option<Response> {
    if !active(ui.ctx()) {
        return None;
    }
    let text = text();
    if text.trim().is_empty() {
        return None;
    }
    let response = ui.interact(rect, id, Sense::hover());
    edit(&response, |node| {
        node.set_role(role);
        node.set_bounds(egui::accesskit::Rect {
            x0: rect.min.x.into(),
            y0: rect.min.y.into(),
            x1: rect.max.x.into(),
            y1: rect.max.y.into(),
        });
        if role == Role::Label {
            node.set_value(text);
        } else {
            node.set_label(text);
        }
    });
    Some(response)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) fn tree_of(ctx: &Context, size: egui::Vec2, ui: impl FnMut(&mut Ui)) -> TreeUpdate {
        ctx.enable_accesskit();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, ui);
        output.textures_delta.clear();
        output.platform_output.accesskit_update.unwrap_or_else(|| panic!("the tree is built once AccessKit is on"))
    }

    pub(crate) fn find<'a>(tree: &'a TreeUpdate, role: Role, name: &str) -> Option<&'a Node> {
        tree.nodes
            .iter()
            .find(|(_, node)| {
                node.role() == role && accessible_name(node, &tree.nodes).is_some_and(|found| found.contains(name))
            })
            .map(|(_, node)| node)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn run(ctx: &Context, mut ui: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(egui::RawInput::default(), |inner| ui(inner));
        output.textures_delta.clear();
        output
    }

    #[test]
    fn nothing_is_built_while_no_screen_reader_reads_the_window() {
        let ctx = Context::default();
        let calls = Cell::new(0);
        let frame = |ui: &mut Ui| {
            let response = ui.interact(
                Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0)),
                ui.scope_id().with("b"),
                Sense::click(),
            );
            button(&response, "Run");
            label(ui, ui.scope_id().with("l"), response.rect, || {
                calls.set(calls.get() + 1);
                "Ready".to_owned()
            });
            assert_eq!(active(ui.ctx()), ui.ctx().has_accesskit_node(response.id));
        };
        let output = run(&ctx, frame);
        assert!(output.platform_output.accesskit_update.is_none());
        assert_eq!(calls.get(), 0, "painted text is not even read without a screen reader");

        ctx.enable_accesskit();
        let output = run(&ctx, frame);
        assert_eq!(calls.get(), 1);
        let update = output.platform_output.accesskit_update.unwrap();
        let named = |role: Role, text: &str| {
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == role && (node.label() == Some(text) || node.value() == Some(text)))
        };
        assert!(named(Role::Button, "Run"));
        assert!(named(Role::Label, "Ready"));
    }

    #[test]
    fn a_growing_list_announces_only_its_last_new_lines_at_most_once_a_second() {
        let ctx = Context::default();
        let mut announcer = Announcer::default();
        let lines: Vec<String> = (0..20).map(|line| format!("line {line}")).collect();
        let mut frame = |time: f64, total: usize| {
            let input = egui::RawInput { time: Some(time), ..egui::RawInput::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
                announcer.show(ui, ui.scope_id().with("console"), rect, total, |range| {
                    lines[range].iter().map(String::as_str).collect()
                });
            });
            output.textures_delta.clear();
            output.platform_output.accesskit_update.and_then(|update| {
                update
                    .nodes
                    .into_iter()
                    .find(|(_, node)| node.role() == Role::Status)
                    .map(|(_, node)| (node.label().unwrap_or_default().to_owned(), node.live()))
            })
        };
        assert_eq!(frame(0.0, 5), None, "nothing without a screen reader");
        ctx.enable_accesskit();
        assert_eq!(frame(0.1, 5), None, "the lines before the screen reader came are not read");
        let announced = frame(0.2, 6).unwrap();
        assert_eq!(announced, ("line 5".to_owned(), Some(Live::Polite)));
        assert_eq!(frame(0.5, 12).unwrap().0, "line 5", "within a second the announcement waits");
        assert_eq!(frame(1.3, 12).unwrap().0, "line 9\nline 10\nline 11", "then the last new lines are read");
        assert_eq!(frame(2.5, 12).unwrap().0, "line 9\nline 10\nline 11");
    }

    #[test]
    fn a_long_announcement_is_cut() {
        let long = "x".repeat(1000);
        assert_eq!(brief(&long).chars().count(), ANNOUNCE_CHARS + 1);
        assert_eq!(brief("short"), "short");
    }

    #[test]
    fn items_carry_their_state() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let output = run(&ctx, |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0));
            let row = ui.interact(rect, ui.scope_id().with("row"), Sense::click());
            tree_item(&row, 2, Some(true), true, "src");
            let stripe =
                ui.interact(rect.translate(egui::vec2(0.0, 30.0)), ui.scope_id().with("stripe"), Sense::click());
            button(&stripe, "Project");
            pressed(&stripe, true);
        });
        let update = output.platform_output.accesskit_update.unwrap();
        let node = |label: &str| update.nodes.iter().find(|(_, node)| node.label() == Some(label)).unwrap().1.clone();
        let row = node("src");
        assert_eq!(
            (row.role(), row.level(), row.is_expanded(), row.is_selected()),
            (Role::TreeItem, Some(2), Some(true), Some(true))
        );
        assert_eq!(node("Project").toggled(), Some(Toggled::True));
    }
}
