use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::glyphs::*;
use super::{IdePalette, IdeView, Side, dependency_analyzer, hint, hover, metrics};
use crate::{accessibility, theme};

pub(super) const LOAD_CHANGES: &str = "view.buildChanges.load";
const CHECK_EVERY: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    Running,
    Done,
    Warning,
    Failed,
    Info,
}

impl Mark {
    fn of(state: &Value) -> Mark {
        match state.as_str().unwrap_or_default() {
            "running" => Mark::Running,
            "warning" => Mark::Warning,
            "failed" | "error" => Mark::Failed,
            "info" => Mark::Info,
            _ => Mark::Done,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mark::Running => "running",
            Mark::Done => "done",
            Mark::Warning => "warning",
            Mark::Failed => "failed",
            Mark::Info => "info",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Fix {
    pub(super) title: String,
    pub(super) command: String,
    pub(super) params: Value,
    pub(super) reload: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SyncRow {
    pub(super) id: String,
    pub(super) depth: usize,
    pub(super) label: String,
    pub(super) detail: String,
    pub(super) mark: Mark,
    pub(super) parent: bool,
    pub(super) open: bool,
    pub(super) target: Option<(PathBuf, Option<u64>)>,
    pub(super) fix: Option<Fix>,
}

fn took(ms: &Value) -> String {
    match ms.as_u64() {
        None => String::new(),
        Some(ms) if ms < 1000 => t!("ide-sync-took-ms", ms = ms),
        Some(ms) => t!("ide-sync-took-s", seconds = format!("{:.1}", ms as f64 / 1000.0)),
    }
}

fn place(message: &Value) -> String {
    let Some(file) = message["file"].as_str() else { return String::new() };
    let name = Path::new(file).file_name().map_or_else(|| file.to_owned(), |name| name.to_string_lossy().into_owned());
    match (message["line"].as_u64(), message["column"].as_u64()) {
        (Some(line), Some(column)) => format!("{name}:{line}:{column}"),
        (Some(line), None) => format!("{name}:{line}"),
        _ => name,
    }
}

pub(super) fn sync_rows(syncs: &[Value], flipped: &BTreeSet<String>) -> Vec<SyncRow> {
    let mut rows = Vec::new();
    for (index, sync) in syncs.iter().enumerate() {
        let id = format!("sync/{}", sync["id"]);
        let mark = Mark::of(&sync["state"]);
        let open = (index == 0) != flipped.contains(&id);
        let state = match mark {
            Mark::Running => t!("ide-sync-running"),
            Mark::Failed => t!("ide-sync-failed"),
            _ => t!("ide-sync-finished"),
        };
        let mut detail = took(&sync["tookMs"]);
        let (errors, warnings) = (sync["errors"].as_u64().unwrap_or(0), sync["warnings"].as_u64().unwrap_or(0));
        if errors + warnings > 0 {
            detail = format!("{}  {detail}", t!("ide-sync-counts", errors = errors, warnings = warnings));
        }
        rows.push(SyncRow {
            id: id.clone(),
            depth: 0,
            label: t!("ide-sync-title", name = sync["name"].as_str().unwrap_or_default(), state = state),
            detail,
            mark,
            parent: true,
            open,
            target: None,
            fix: None,
        });
        if !open {
            continue;
        }
        for (at, step) in sync["steps"].as_array().into_iter().flatten().enumerate() {
            let step_id = format!("{id}/{at}");
            let messages = step["messages"].as_array().cloned().unwrap_or_default();
            let step_open = (!messages.is_empty()) != flipped.contains(&step_id);
            rows.push(SyncRow {
                id: step_id.clone(),
                depth: 1,
                label: step["title"].as_str().unwrap_or_default().to_owned(),
                detail: took(&step["tookMs"]),
                mark: Mark::of(&step["state"]),
                parent: !messages.is_empty(),
                open: step_open,
                target: None,
                fix: None,
            });
            if !step_open {
                continue;
            }
            for (number, message) in messages.iter().enumerate() {
                let fix = message.get("fix").filter(|fix| fix.is_object()).map(|fix| Fix {
                    title: fix["title"].as_str().unwrap_or_default().to_owned(),
                    command: fix["command"].as_str().unwrap_or_default().to_owned(),
                    params: fix["params"].clone(),
                    reload: (fix["reload"] == true).then(|| sync["system"].as_str().unwrap_or_default().to_owned()),
                });
                rows.push(SyncRow {
                    id: format!("{step_id}/{number}"),
                    depth: 2,
                    label: message["text"].as_str().unwrap_or_default().lines().next().unwrap_or_default().to_owned(),
                    detail: place(message),
                    mark: Mark::of(&message["severity"]),
                    parent: false,
                    open: false,
                    target: message["file"].as_str().map(|file| (PathBuf::from(file), message["offset"].as_u64())),
                    fix,
                });
            }
        }
    }
    rows
}

#[derive(Default)]
pub(super) struct BuildSyncUi {
    checked: f64,
    pending: Vec<Value>,
    syncs: Vec<Value>,
    seen_failure: u64,
    flipped: BTreeSet<String>,
    selected: Option<String>,
}

impl BuildSyncUi {
    pub(super) fn state(&self, shown: bool) -> Value {
        let mut state = json!({ "pending": self.pending, "selected": self.selected });
        if shown {
            let rows: Vec<Value> = sync_rows(&self.syncs, &self.flipped)
                .iter()
                .map(|row| {
                    let mut value =
                        json!({ "id": row.id, "depth": row.depth, "label": row.label, "state": row.mark.name() });
                    if !row.detail.is_empty() {
                        value["detail"] = json!(row.detail);
                    }
                    if row.parent {
                        value["open"] = json!(row.open);
                    }
                    if let Some(fix) = &row.fix {
                        value["fix"] = json!({ "title": fix.title, "command": fix.command, "params": fix.params });
                    }
                    value
                })
                .collect();
            state["rows"] = Value::Array(rows);
        }
        state
    }

    fn pending_for(&self, path: &Path) -> Option<&Value> {
        let name = path.file_name()?.to_str()?;
        self.pending.iter().find(|pending| {
            pending["autoReload"] != true
                && pending["files"].as_array().is_some_and(|files| files.iter().any(|f| f == name))
        })
    }
}

impl IdeView {
    #[cfg(test)]
    pub(super) fn build_sync_button_for(&self, path: &Path) -> bool {
        self.build_sync.pending_for(path).is_some()
    }

    pub(super) fn follow_build_sync(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if now - self.build_sync.checked < CHECK_EVERY {
            return;
        }
        self.build_sync.checked = now;
        self.refresh_build_sync(registry, session, true);
    }

    fn refresh_build_sync(&mut self, registry: &Registry, session: &mut Session, auto: bool) {
        let pending = registry
            .dispatch(session, "build.changes.get", json!({}))
            .ok()
            .and_then(|changes| changes["pending"].as_array().cloned())
            .unwrap_or_default();
        let (auto_reload, shown): (Vec<Value>, Vec<Value>) =
            pending.into_iter().partition(|pending| auto && pending["autoReload"] == true);
        self.build_sync.pending = shown;
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        for pending in auto_reload {
            let (Some(command), Some(id), Some(root)) = (pending["reload"].as_str(), pending["id"].as_str(), &root)
            else {
                continue;
            };
            if registry.dispatch(session, command, json!({})).is_err() {
                zen_workbench::build_sync::mark_loaded(session, id, root);
            }
        }
        self.build_sync.syncs = registry
            .dispatch(session, "build.sync.get", json!({}))
            .ok()
            .and_then(|list| list["syncs"].as_array().cloned())
            .unwrap_or_default();
        let failed = self
            .build_sync
            .syncs
            .iter()
            .filter(|sync| sync["state"] == "failed")
            .filter_map(|sync| sync["id"].as_u64())
            .max()
            .unwrap_or(0);
        if failed > self.build_sync.seen_failure {
            self.build_sync.seen_failure = failed;
            if self.build_tools.available("build") && !self.tool_visible("build") {
                self.show_tool("build");
            }
        }
    }

    pub(super) fn load_build_changes(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<Value, String> {
        let changes = registry.dispatch(session, "build.changes.get", json!({})).map_err(|error| error.text())?;
        let named = params.get("system").and_then(Value::as_str);
        let chosen: Vec<Value> = match named {
            Some(id) => changes["systems"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|system| system["id"] == id)
                .cloned()
                .collect(),
            None => changes["pending"].as_array().cloned().unwrap_or_default(),
        };
        if let (Some(id), true) = (named, chosen.is_empty()) {
            return Err(t!("ide-sync-no-system", system = id));
        }
        if !chosen.is_empty() {
            registry.dispatch(session, "file.document.saveAll", json!({})).map_err(|error| error.text())?;
        }
        let mut loaded = Vec::new();
        let mut failure = None;
        for system in &chosen {
            let Some(command) = system["reload"].as_str() else { continue };
            match registry.dispatch(session, command, json!({})) {
                Ok(_) => loaded.push(system["id"].clone()),
                Err(error) => failure = Some(error.text()),
            }
        }
        self.build_sync.checked = f64::NEG_INFINITY;
        self.refresh_build_sync(registry, session, false);
        match failure {
            Some(error) if loaded.is_empty() => Err(error),
            _ => Ok(json!({ "loaded": loaded })),
        }
    }

    pub(super) fn build_changes_button(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, area: Rect) {
        let Some(path) = session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned())
        else {
            return;
        };
        let Some(pending) = self.build_sync.pending_for(&path) else { return };
        let label = t!("ide-sync-load-changes", name = pending["name"].as_str().unwrap_or_default());
        let p = self.palette;
        let rect = Rect::from_min_size(pos2(area.right() - 52.0, area.top() + 8.0), Vec2::splat(28.0));
        let response = ui
            .interact(rect, ui.scope_id().with("build-load-changes"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter_at(area);
        painter.rect(rect, 6, p.popup, egui::Stroke::new(1.0, p.popup_border), egui::StrokeKind::Inside);
        if response.hovered() {
            painter.rect_filled(rect.shrink(2.0), 5, p.hover);
        }
        glyph_refresh(&painter, rect.shrink(7.0), p.accent);
        accessibility::button(&response, &label);
        hint(ui, &response, &label, crate::keymap::shortcut(LOAD_CHANGES).as_deref(), Side::Below);
        if response.clicked() {
            self.run_action(ui, registry, session, LOAD_CHANGES, json!({}));
        }
    }

    pub(super) fn build_sync_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if self.build_sync.syncs.iter().any(|sync| sync["state"] == "running") {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-build"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("build-sync-title"), title, || t!("ide-tool-build"));
        let analyzer = self.dependency_analyzer.tab;
        let mut right = title.right() + 14.0;
        for (shows_analyzer, name) in [(false, t!("ide-sync-tab")), (true, t!("ide-dependency-tab"))] {
            let tab = ui.painter().layout_no_wrap(name.clone(), theme::regular(13.0), p.text);
            let rect = Rect::from_min_size(pos2(right, header.center().y - 11.0), vec2(tab.size().x + 16.0, 22.0));
            let response = ui.interact(rect, ui.scope_id().with(("build-tab", shows_analyzer)), Sense::click());
            if shows_analyzer == analyzer {
                ui.painter().rect_filled(rect, 5, p.selection);
            } else {
                hover(ui, &response, &p);
            }
            ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - tab.size().y / 2.0), tab, p.text);
            accessibility::item(&response, egui::Role::Tab, shows_analyzer == analyzer, &name);
            if response.clicked() && shows_analyzer != analyzer {
                let action = if shows_analyzer { dependency_analyzer::OPEN } else { dependency_analyzer::SET };
                self.run_action(ui, registry, session, action, json!({ "shown": shows_analyzer }));
            }
            right = rect.right() + 4.0;
        }
        let reload = Rect::from_center_size(pos2(right + 16.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(
            reload,
            ui.scope_id().with("build-sync-reload"),
            if analyzer { Sense::hover() } else { Sense::click() },
        );
        if !analyzer {
            hover(ui, &response, &p);
            glyph_refresh(ui.painter(), reload.shrink(6.0), p.text_secondary);
            let reload_label = t!("ide-sync-reload");
            hint(ui, &response, &reload_label, None, Side::Below);
            accessibility::button(&response, &reload_label);
        }
        let mut act: Option<(String, Value, Option<String>)> = None;
        if response.clicked() {
            let systems: Vec<String> = self
                .build_sync
                .syncs
                .iter()
                .filter_map(|sync| sync["system"].as_str().map(str::to_owned))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            for system in systems {
                if let Err(error) = self.load_build_changes(registry, session, &json!({ "system": system })) {
                    self.post(ui, error);
                }
            }
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("build-sync-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool("build");
        }
        if analyzer {
            self.dependency_analyzer_body(ui, registry, session);
            return;
        }

        let rows = sync_rows(&self.build_sync.syncs, &self.build_sync.flipped);
        let mut toggled = None;
        let mut go = None;
        egui::ScrollArea::vertical().id_salt("build-sync-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if rows.is_empty() {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(t!("ide-sync-empty")).color(p.text_secondary).font(theme::regular(12.5)),
                    );
                });
            }
            for row in &rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let selected = self.build_sync.selected.as_deref() == Some(row.id.as_str());
                accessibility::tree_item(
                    &response,
                    row.depth + 1,
                    row.parent.then_some(row.open),
                    selected,
                    &row.label,
                );
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if selected {
                    ui.painter().rect_filled(highlight, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                if row.parent {
                    if row.open {
                        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                    } else {
                        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                    }
                }
                let icon = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(12.0));
                paint_mark(ui, &p, icon, row.mark);
                let painter = ui.painter().with_clip_rect(rect);
                let mut right = rect.right() - 12.0;
                if let Some(fix) = &row.fix {
                    let galley = painter.layout_no_wrap(fix.title.clone(), theme::regular(12.5), p.link);
                    let link = Rect::from_min_size(
                        pos2(right - galley.size().x, rect.center().y - galley.size().y / 2.0),
                        galley.size(),
                    );
                    right = link.left() - 12.0;
                    let clicked = ui
                        .interact(link, ui.scope_id().with(("build-sync-fix", &row.id)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    accessibility::name(&clicked, egui::Role::Link, &fix.title);
                    if clicked.hovered() {
                        painter.line_segment([link.left_bottom(), link.right_bottom()], egui::Stroke::new(1.0, p.link));
                    }
                    painter.galley(link.min, galley, p.link);
                    if clicked.clicked() {
                        act = Some((fix.command.clone(), fix.params.clone(), fix.reload.clone()));
                    }
                }
                let name = painter.text(
                    pos2(x + metrics::TREE_TEXT, rect.center().y),
                    Align2::LEFT_CENTER,
                    &row.label,
                    theme::regular(13.0),
                    p.text,
                );
                if !row.detail.is_empty() && name.right() + 12.0 < right {
                    painter.text(
                        pos2(name.right() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &row.detail,
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                }
                if response.clicked() || response.double_clicked() {
                    self.build_sync.selected = Some(row.id.clone());
                    let on_chevron = response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    if row.parent && (on_chevron || response.double_clicked()) {
                        toggled = Some(row.id.clone());
                    } else if let Some(target) = &row.target {
                        go = Some(target.clone());
                    }
                }
            }
        });
        if let Some(id) = toggled
            && !self.build_sync.flipped.remove(&id)
        {
            self.build_sync.flipped.insert(id);
        }
        if let Some((path, offset)) = go {
            let (command, params) = match offset {
                Some(offset) => ("navigate.place.goTo", json!({ "path": path, "offset": offset })),
                None => ("file.document.open", json!({ "path": path })),
            };
            self.dispatch(ui, registry, session, command, params);
        }
        if let Some((command, params, reload)) = act {
            if command.starts_with("view.") {
                self.run_action(ui, registry, session, &command, params);
            } else {
                self.dispatch(ui, registry, session, &command, params);
            }
            if let Some(system) = reload
                && let Err(error) = self.load_build_changes(registry, session, &json!({ "system": system }))
            {
                self.post(ui, error);
            }
        }
    }
}

fn paint_mark(ui: &Ui, p: &IdePalette, rect: Rect, mark: Mark) {
    let painter = ui.painter();
    match mark {
        Mark::Running => glyph_refresh(painter, rect, p.text_secondary),
        Mark::Done => glyph_check(painter, rect, p.run),
        Mark::Warning => {
            painter.circle_filled(rect.center(), rect.width() / 2.0, p.problem("warning"));
            painter.text(rect.center(), Align2::CENTER_CENTER, "!", theme::semibold(10.0), p.editor_background);
        }
        Mark::Failed => {
            painter.circle_filled(rect.center(), rect.width() / 2.0, p.problem("error"));
            glyph_close(painter, rect.shrink(3.0), p.editor_background);
        }
        Mark::Info => {
            painter.circle_filled(rect.center(), rect.width() / 2.0, p.problem("information"));
            painter.text(rect.center(), Align2::CENTER_CENTER, "i", theme::semibold(10.0), p.editor_background);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syncs() -> Vec<Value> {
        vec![
            json!({ "id": 2, "system": "maven", "name": "Maven", "state": "failed", "tookMs": 3140, "errors": 1, "warnings": 0,
                "steps": [
                    { "title": "Read pom.xml files", "state": "done", "tookMs": 4 },
                    { "title": "Resolve dependencies", "state": "failed", "tookMs": 3100, "messages": [{
                        "severity": "error", "text": "Non-resolvable parent POM\nmore", "file": "/p/pom.xml", "line": 3,
                        "column": 9, "offset": 120,
                        "fix": { "title": "Work Offline", "command": "maven.options.set", "params": { "offline": true }, "reload": true }
                    }] }
                ] }),
            json!({ "id": 1, "system": "maven", "name": "Maven", "state": "done", "tookMs": 800, "errors": 0, "warnings": 0,
                "steps": [{ "title": "Read pom.xml files", "state": "done", "tookMs": 3 }] }),
        ]
    }

    #[test]
    fn the_newest_sync_opens_with_its_failed_steps_and_their_fixes() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let rows = sync_rows(&syncs(), &BTreeSet::new());
        let shown: Vec<(usize, &str, &str)> =
            rows.iter().map(|row| (row.depth, row.label.as_str(), row.detail.as_str())).collect();
        assert_eq!(
            shown,
            [
                (0, "Maven: failed", "1 error, 0 warnings  3.1 s"),
                (1, "Read pom.xml files", "4 ms"),
                (1, "Resolve dependencies", "3.1 s"),
                (2, "Non-resolvable parent POM", "pom.xml:3:9"),
                (0, "Maven: finished", "800 ms"),
            ]
        );
        let error = &rows[3];
        assert_eq!((error.mark, error.target.clone()), (Mark::Failed, Some((PathBuf::from("/p/pom.xml"), Some(120)))));
        let fix = error.fix.clone().unwrap();
        assert_eq!((fix.command.as_str(), fix.reload.as_deref()), ("maven.options.set", Some("maven")));
        let flipped: BTreeSet<String> = ["sync/2".to_owned(), "sync/1".to_owned()].into();
        let rows = sync_rows(&syncs(), &flipped);
        assert_eq!(rows.iter().map(|row| row.depth).collect::<Vec<_>>(), [0, 0, 1], "closed, and the older one open");
    }
}
