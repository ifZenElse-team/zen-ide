use egui::{Align2, Color32, Painter, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::tasks::{TaskInfo, TaskState, Tasks};

use super::{IdePalette, IdeView, accessibility, glyphs, metrics};
use crate::theme;

const OUTCOME: u64 = 8;
const PANEL: Vec2 = vec2(600.0, 300.0);
const SHOWN_LINES: usize = 2000;

#[derive(Default)]
pub(super) struct TasksUi {
    seen: u64,
}

pub(super) fn progress_bar(ui: &Ui, p: &IdePalette, bar: Rect, fraction: Option<f32>) {
    ui.painter().rect_filled(bar, 2, p.hover);
    let filled = match fraction {
        Some(fraction) => Rect::from_min_size(bar.min, vec2(bar.width() * fraction.clamp(0.0, 1.0), bar.height())),
        None => {
            let time = ui.input(|input| input.time);
            let at = ((time * 0.8) % 1.0) as f32;
            Rect::from_min_size(pos2(bar.left() + (bar.width() - 30.0) * at, bar.top()), vec2(30.0, bar.height()))
        }
    };
    ui.painter().rect_filled(filled, 2, p.accent);
    if fraction.is_none() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn state_glyph(painter: &Painter, p: &IdePalette, rect: Rect, state: TaskState) {
    match state {
        TaskState::Running => glyphs::glyph_test_running(painter, rect, p.accent),
        TaskState::Succeeded => glyphs::glyph_test_passed(painter, rect, p.run),
        TaskState::Failed => glyphs::glyph_test_failed(painter, rect, p.error_text),
        TaskState::Cancelled => glyphs::glyph_stop(painter, rect, p.text_secondary),
    }
}

fn control(
    ui: &Ui,
    p: &IdePalette,
    at: Rect,
    salt: (&str, u64, &str),
    name: &str,
    glyph: impl Fn(&Painter, Rect, Color32),
) -> bool {
    let response = ui.interact(at, ui.scope_id().with(salt), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(at, 4, p.hover);
    }
    glyph(ui.painter(), at.shrink(4.0), if response.hovered() { p.text } else { p.text_secondary });
    super::hint(ui, &response, name, None, super::Side::Below);
    response.clicked()
}

fn controls(ui: &Ui, p: &IdePalette, task: &TaskInfo, right: f32, y: f32, place: &str) -> (Option<&'static str>, f32) {
    let mut left = right;
    let mut chosen = None;
    let mut next = || {
        let at = Rect::from_center_size(pos2(left - 8.0, y), Vec2::splat(16.0));
        left = at.left() - 4.0;
        at
    };
    if task.cancellable {
        let name = t!("ide-tasks-cancel");
        let clicked =
            control(ui, p, next(), ("background-task-cancel", task.id, place), &name, |painter, at, color| {
                super::find_bar::glyph_close(painter, at, color);
            });
        chosen = clicked.then_some("tasks.background.cancel").or(chosen);
    }
    if task.pausable {
        let (name, command) = if task.paused {
            (t!("ide-tasks-resume"), "tasks.background.resume")
        } else {
            (t!("ide-tasks-pause"), "tasks.background.pause")
        };
        let glyph = if task.paused { glyphs::glyph_resume } else { glyphs::glyph_pause };
        let clicked = control(ui, p, next(), ("background-task-pause", task.id, place), &name, glyph);
        chosen = clicked.then_some(command).or(chosen);
    }
    (chosen, left)
}

fn progress_text(task: &TaskInfo) -> String {
    let percent = task.fraction.map(|fraction| (fraction * 100.0).round() as u32);
    let detail = task.detail.as_str();
    match percent {
        Some(percent) if detail.is_empty() => t!("ide-tasks-percent", percent = percent),
        Some(percent) => t!("ide-tasks-detail-percent", detail = detail, percent = percent),
        None => task.detail.clone(),
    }
}

fn seconds_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl IdeView {
    pub(super) fn tasks_indicator(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        right: f32,
        y: f32,
    ) -> f32 {
        let Some(tasks) = session.state::<Tasks>() else { return right };
        let list = tasks.list();
        let shown = tasks.panel_shown();
        let running: Vec<&TaskInfo> = list.iter().filter(|task| task.state == TaskState::Running).collect();
        let task = match running.last() {
            Some(task) => *task,
            None => match list.last() {
                Some(task)
                    if (task.state == TaskState::Failed && task.id > self.tasks_ui.seen)
                        || task.ended.is_some_and(|ended| seconds_now().saturating_sub(ended) < OUTCOME) =>
                {
                    task
                }
                _ => return right,
            },
        };
        let p = self.palette;
        let mut left = right;
        let mut item = Rect::NOTHING;
        if task.state == TaskState::Running {
            let (chosen, start) = controls(ui, &p, task, right, y, "status-bar");
            if let Some(command) = chosen {
                self.dispatch(ui, registry, session, command, json!({ "id": task.id }));
            }
            left = if start < right { start - 2.0 } else { right };
        }
        if task.state == TaskState::Running {
            let bar = Rect::from_min_size(pos2(left - 120.0, y - 2.0), vec2(120.0, 4.0));
            progress_bar(ui, &p, bar, task.fraction);
            left = bar.left() - 8.0;
            item = Rect::from_min_max(pos2(bar.left(), y - 8.0), pos2(bar.right(), y + 8.0));
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        let mut text = task.title.clone();
        if task.state == TaskState::Running
            && !task.detail.is_empty()
            && matches!(
                task.source.as_str(),
                "vcs" | zen_workbench::exit::INDEX | zen_workbench::find_results::TASK_SOURCE
            )
        {
            text = format!("{text} {}", task.detail);
        } else if task.state == TaskState::Running && task.source == zen_workbench::exit::LANGUAGE_SERVER {
            text = format!("{text} {}", progress_text(task));
        }
        if task.paused {
            text = t!("ide-tasks-paused", title = text);
        }
        if running.len() > 1 {
            text = t!("ide-tasks-more", title = text, count = running.len() - 1);
        }
        let font = theme::regular(12.5);
        let galley = ui.painter().layout_no_wrap(text, font, p.text_secondary);
        let width = galley.size().x.min(360.0);
        let label = Rect::from_min_size(pos2(left - width, y - galley.size().y / 2.0), vec2(width, galley.size().y));
        ui.painter().with_clip_rect(label).galley(label.min, galley, p.text_secondary);
        item = item.union(label);
        if task.state != TaskState::Running {
            let mark = Rect::from_center_size(pos2(label.left() - 10.0, y), Vec2::splat(12.0));
            state_glyph(ui.painter(), &p, mark, task.state);
            item = item.union(mark);
        }
        let response = ui.interact(item.expand(4.0), ui.scope_id().with("background-tasks"), Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        accessibility::button(&response, &t!("ide-tasks-title"));
        let hint = if shown { t!("ide-tasks-hide") } else { t!("ide-tasks-show") };
        let response = response.on_hover_text(hint);
        if response.clicked() {
            self.tasks_ui.seen = self.tasks_ui.seen.max(task.id);
            let command = if shown { "tasks.panel.hide" } else { "tasks.panel.show" };
            self.dispatch(ui, registry, session, command, json!({}));
        }
        item.left() - 16.0
    }

    pub(super) fn tasks_panel(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(tasks) = session.state::<Tasks>() else { return };
        if !tasks.panel_shown() {
            return;
        }
        let list = tasks.list();
        let shown = tasks.selected().and_then(|selected| tasks.log(Some(selected), Some(SHOWN_LINES)));
        let Some((task, lines)) = shown else {
            self.empty_tasks_panel(ui, registry, session);
            return;
        };
        if task.state == TaskState::Failed {
            self.tasks_ui.seen = self.tasks_ui.seen.max(task.id);
        }
        let p = self.palette;
        let screen = ui.ctx().content_rect();
        let width = PANEL.x.min(screen.width() - 32.0);
        let at = pos2(screen.right() - metrics::STRIPE - 8.0, screen.bottom() - metrics::STATUS_BAR - 6.0);
        let mut picked = None;
        let mut hide = false;
        let mut chosen = None;
        egui::Area::new(ui.scope_id().with("background-tasks-panel"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(at)
            .pivot(Align2::RIGHT_BOTTOM)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(10).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                    ui.horizontal(|ui| {
                        let (mark, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        state_glyph(ui.painter(), &p, mark.shrink(1.0), task.state);
                        let title = egui::RichText::new(&task.title).font(theme::semibold(13.5)).color(p.text);
                        ui.add(egui::Label::new(title).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = egui::RichText::new(t!("ide-tasks-hide-button"))
                                .font(theme::regular(12.5))
                                .color(p.link);
                            let response = ui.add(egui::Label::new(label).sense(Sense::click()));
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                hide = true;
                            }
                        });
                    });
                    if list.len() > 1 {
                        ui.add_space(8.0);
                        egui::ScrollArea::horizontal().id_salt("background-task-tabs").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for item in list.iter().rev() {
                                    let galley = ui.painter().layout_no_wrap(
                                        item.title.clone(),
                                        theme::regular(12.0),
                                        if item.id == task.id { p.text } else { p.text_secondary },
                                    );
                                    let size = vec2(galley.size().x.min(200.0) + 30.0, 22.0);
                                    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                                    if item.id == task.id {
                                        ui.painter().rect_filled(rect, 5, p.selection);
                                    } else if response.hovered() {
                                        ui.painter().rect_filled(rect, 5, p.popup_hover);
                                    }
                                    let mark = Rect::from_center_size(
                                        pos2(rect.left() + 12.0, rect.center().y),
                                        Vec2::splat(11.0),
                                    );
                                    state_glyph(ui.painter(), &p, mark, item.state);
                                    let text = Rect::from_min_max(
                                        pos2(rect.left() + 22.0, rect.top()),
                                        pos2(rect.right() - 6.0, rect.bottom()),
                                    );
                                    ui.painter().with_clip_rect(text).galley(
                                        pos2(text.left(), rect.center().y - galley.size().y / 2.0),
                                        galley,
                                        p.text,
                                    );
                                    if response.clicked() {
                                        picked = Some(item.id);
                                    }
                                }
                            });
                        });
                    }
                    ui.add_space(8.0);
                    match task.state {
                        TaskState::Running => {
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
                            let bar = Rect::from_min_size(pos2(rect.left(), rect.center().y - 2.0), vec2(140.0, 4.0));
                            progress_bar(ui, &p, bar, task.fraction);
                            let (control, end) = controls(ui, &p, &task, rect.right(), rect.center().y, "panel");
                            chosen = control;
                            let text = Rect::from_min_max(
                                pos2(bar.right() + 12.0, rect.top()),
                                pos2(end - 4.0, rect.bottom()),
                            );
                            let detail = progress_text(&task);
                            let detail = if task.paused { t!("ide-tasks-paused", title = detail) } else { detail };
                            let galley = ui.painter().layout_no_wrap(detail, theme::regular(12.0), p.text_secondary);
                            let at = pos2(text.left(), rect.center().y - galley.size().y / 2.0);
                            ui.painter().with_clip_rect(text).galley(at, galley, p.text_secondary);
                        }
                        TaskState::Cancelled => {
                            let message = task.message.clone().unwrap_or_default();
                            let text = message.lines().next().unwrap_or_default().to_owned();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text).font(theme::regular(12.5)).color(p.text_secondary),
                                )
                                .truncate(),
                            );
                        }
                        TaskState::Succeeded | TaskState::Failed => {
                            let failed = task.state == TaskState::Failed;
                            let message = task.message.clone().unwrap_or_default();
                            let mut lines = message.lines().map(str::trim).filter(|line| !line.is_empty());
                            let first =
                                if failed { lines.next_back() } else { lines.next() }.unwrap_or_default().to_owned();
                            let text = if failed {
                                t!("ide-tasks-failed", error = first)
                            } else {
                                t!("ide-tasks-succeeded", message = first)
                            };
                            let color = if failed { p.error_text } else { p.run };
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).font(theme::regular(12.5)).color(color))
                                    .truncate(),
                            );
                        }
                    }
                    ui.add_space(8.0);
                    let log_frame = egui::Frame::new()
                        .fill(p.editor_background)
                        .stroke(Stroke::new(1.0, p.popup_border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 6));
                    log_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let font = egui::FontId::monospace(12.0);
                        let row = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 1.0;
                        egui::ScrollArea::both()
                            .id_salt(("background-task-log", task.id))
                            .max_height(PANEL.y - 120.0)
                            .min_scrolled_height(PANEL.y - 120.0)
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show_rows(ui, row, lines.len().max(1), |ui, range| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                if lines.is_empty() {
                                    ui.label(
                                        egui::RichText::new(t!("ide-tasks-no-output"))
                                            .font(font.clone())
                                            .color(p.text_secondary),
                                    );
                                }
                                for line in &lines[range.start.min(lines.len())..range.end.min(lines.len())] {
                                    let color = if line.starts_with("$ ") { p.text_secondary } else { p.text };
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(line).font(font.clone()).color(color))
                                            .extend(),
                                    );
                                }
                            });
                    });
                });
            });
        if let Some(id) = picked {
            self.dispatch(ui, registry, session, "tasks.panel.show", json!({ "id": id }));
        }
        if let Some(command) = chosen {
            self.dispatch(ui, registry, session, command, json!({ "id": task.id }));
        }
        if hide {
            self.dispatch(ui, registry, session, "tasks.panel.hide", json!({}));
        }
        if task.state == TaskState::Running {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn empty_tasks_panel(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let screen = ui.ctx().content_rect();
        let at = pos2(screen.right() - metrics::STRIPE - 8.0, screen.bottom() - metrics::STATUS_BAR - 6.0);
        let mut hide = false;
        egui::Area::new(ui.scope_id().with("background-tasks-panel"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(at)
            .pivot(Align2::RIGHT_BOTTOM)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(10).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        let none = egui::RichText::new(t!("ide-tasks-none"))
                            .font(theme::regular(12.5))
                            .color(p.text_secondary);
                        ui.add(egui::Label::new(none).selectable(false));
                        let label =
                            egui::RichText::new(t!("ide-tasks-hide-button")).font(theme::regular(12.5)).color(p.link);
                        let response = ui.add(egui::Label::new(label).sense(Sense::click()));
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        hide = response.clicked();
                    });
                });
            });
        if hide {
            self.dispatch(ui, registry, session, "tasks.panel.hide", json!({}));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, PointerButton, Pos2};
    use serde_json::json;
    use zen_core::{Registry, Session};
    use zen_workbench::tasks::Tasks;

    use super::super::{IdeProduct, IdeView};

    fn product() -> IdeProduct {
        IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        }
    }

    #[test]
    fn a_task_shows_its_progress_and_its_log_at_the_bottom_right() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        view.project_open = false;
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 560.0), 1.5);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let save = |name: &str, image: &crate::offscreen::Frame| {
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(name), image).unwrap();
            }
        };
        let task = session.state_mut::<Tasks>().start("Updating 3 dependencies", "inspections", true);
        task.log(
            "$ go get github.com/andybalholm/brotli@v1.2.6 golang.org/x/crypto@v0.43.0 github.com/google/uuid@v1.6.0",
        );
        task.log("go: downloading github.com/andybalholm/brotli v1.2.6\ngo: downloading golang.org/x/crypto v0.43.0");
        task.log("go: upgraded github.com/andybalholm/brotli v1.0.0 => v1.2.6");
        let mut frame = |events: Vec<Event>, view: &mut IdeView, session: &mut Session| {
            screen.frame(events, |ui| view.show(ui, &registry, session))
        };
        for _ in 0..3 {
            frame(Vec::new(), &mut view, &mut session);
        }
        let image = frame(Vec::new(), &mut view, &mut session);
        save("10-background-task-running.png", &image);
        assert!(session.state::<Tasks>().unwrap().panel_shown(), "the panel opens at once");

        registry.dispatch(&mut session, "tasks.panel.hide", json!({})).unwrap();
        frame(Vec::new(), &mut view, &mut session);
        assert!(!session.state::<Tasks>().unwrap().panel_shown());
        let at = Pos2::new(1100.0 - 14.0 - 60.0, 560.0 - super::metrics::STATUS_BAR / 2.0);
        let click = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![Event::PointerMoved(at), click(true)], &mut view, &mut session);
        frame(vec![click(false)], &mut view, &mut session);
        assert!(session.state::<Tasks>().unwrap().panel_shown(), "the status bar shows it again");

        task.finish(Err("go: golang.org/x/crypto@v0.43.0: unknown revision v0.43.0".to_owned()));
        frame(Vec::new(), &mut view, &mut session);
        let image = frame(Vec::new(), &mut view, &mut session);
        save("11-background-task-failed.png", &image);
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({})).unwrap();
        assert_eq!(log["task"]["state"], "failed");
    }

    fn tasks_window(size: egui::Vec2) -> (tempfile::TempDir, Registry, Session, IdeView, crate::offscreen::Offscreen) {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        view.project_open = false;
        (dir, registry, session, view, crate::offscreen::Offscreen::new(size, 1.0))
    }

    fn center_of(
        screen: &mut crate::offscreen::Offscreen,
        name: &str,
        mut show: impl FnMut(&mut egui::Ui),
    ) -> Option<Pos2> {
        let tree = screen.accessibility_tree(|ui| show(ui));
        tree.nodes.iter().find(|(_, node)| node.label().is_some_and(|label| label == name)).and_then(|(_, node)| {
            let bounds = node.bounds()?;
            Some(Pos2::new(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
        })
    }

    fn click_events(at: Pos2, pressed: bool) -> Vec<Event> {
        let button =
            Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        if pressed { vec![Event::PointerMoved(at), button] } else { vec![button] }
    }

    #[test]
    fn indexing_pauses_and_resumes_from_the_status_bar_and_has_no_cross() {
        let (_dir, registry, mut session, mut view, mut screen) = tasks_window(egui::vec2(1100.0, 560.0));
        let task = session.state_mut::<Tasks>().start("Indexing…", zen_workbench::exit::INDEX, false);
        let told = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&told);
        task.on_pause(move |paused| seen.lock().unwrap().push(paused));
        task.progress(Some(0.25), "10 of 40 files");
        for _ in 0..3 {
            screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        assert!(center_of(&mut screen, "Cancel", |ui| view.show(ui, &registry, &mut session)).is_none());
        let at =
            center_of(&mut screen, "Pause", |ui| view.show(ui, &registry, &mut session)).expect("the ‖ of indexing");
        screen.frame(click_events(at, true), |ui| view.show(ui, &registry, &mut session));
        screen.frame(click_events(at, false), |ui| view.show(ui, &registry, &mut session));
        assert_eq!(*told.lock().unwrap(), [true]);
        assert!(session.state::<Tasks>().unwrap().list().last().unwrap().paused);
        for _ in 0..2 {
            screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let at = center_of(&mut screen, "Resume", |ui| view.show(ui, &registry, &mut session)).expect("the ▷");
        screen.frame(click_events(at, true), |ui| view.show(ui, &registry, &mut session));
        screen.frame(click_events(at, false), |ui| view.show(ui, &registry, &mut session));
        assert_eq!(*told.lock().unwrap(), [true, false]);
    }

    #[test]
    fn the_panel_has_the_cross_of_the_task_it_shows_and_says_when_it_is_empty() {
        let (_dir, registry, mut session, mut view, mut screen) = tasks_window(egui::vec2(1100.0, 560.0));
        while session.state::<Tasks>().unwrap().running() {
            std::thread::yield_now();
        }
        registry.dispatch(&mut session, "tasks.finished.clear", json!({})).unwrap();
        let labelled =
            |screen: &mut crate::offscreen::Offscreen, view: &mut IdeView, session: &mut Session, name: &str| {
                for _ in 0..3 {
                    screen.frame(Vec::new(), |ui| view.show(ui, &registry, session));
                }
                let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, session));
                tree.nodes
                    .iter()
                    .filter(|(_, node)| {
                        node.label().is_some_and(|label| label == name)
                            || node.value().is_some_and(|value| value == name)
                    })
                    .count()
            };
        assert_eq!(labelled(&mut screen, &mut view, &mut session, "No background tasks"), 0);
        registry.dispatch(&mut session, "tasks.panel.toggle", json!({})).unwrap();
        assert_eq!(labelled(&mut screen, &mut view, &mut session, "No background tasks"), 1);

        let task = session.state_mut::<Tasks>().start("Fetching", "vcs", true);
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        task.cancel_with(Arc::clone(&flag));
        assert_eq!(labelled(&mut screen, &mut view, &mut session, "Cancel"), 2, "the status bar and the panel");
        registry.dispatch(&mut session, "tasks.background.cancel", json!({})).unwrap();
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(labelled(&mut screen, &mut view, &mut session, "Cancel"), 0, "only once");
        task.finish(Err("signal: interrupt".to_owned()));
        assert!(labelled(&mut screen, &mut view, &mut session, "Cancelled") > 0, "the panel says how it ended");
        assert_eq!(
            session.state::<Tasks>().unwrap().list().last().unwrap().state,
            zen_workbench::tasks::TaskState::Cancelled
        );
    }

    #[test]
    fn the_cross_of_a_cancellable_task_in_the_status_bar_stops_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        view.project_open = false;
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 560.0), 1.0);
        let task = session.state_mut::<Tasks>().start("Searching for 'Version'", "search", false);
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        task.cancel_with(Arc::clone(&flag));
        task.progress(None, "in kucoin/client.go…");
        for _ in 0..3 {
            screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
        let label = |text: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|label| label == text))
                .and_then(|(_, node)| node.bounds())
        };
        let cross = label("Cancel").expect("the × of the running search");
        let at = Pos2::new(((cross.x0 + cross.x1) / 2.0) as f32, ((cross.y0 + cross.y1) / 2.0) as f32);
        let click = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        screen.frame(vec![Event::PointerMoved(at), click(true)], |ui| view.show(ui, &registry, &mut session));
        screen.frame(vec![click(false)], |ui| view.show(ui, &registry, &mut session));
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed), "the click cancels the task");
    }
}
