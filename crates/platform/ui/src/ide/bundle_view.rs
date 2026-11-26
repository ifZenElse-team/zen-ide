use std::path::{Path, PathBuf};

use egui::{Align2, Key, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::resource_bundles::{BundleKey, BundleTable};

use super::{IdePalette, IdeView, Side, hint, hover};
use crate::theme;

const ROW: f32 = 24.0;
const BAR: f32 = 34.0;
const KEY_SHARE: f32 = 0.3;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Input {
    NewKey(String),
    Rename(String, String),
    Value { key: String, locale: String, text: String },
}

pub(super) struct BundleTab {
    pub(super) path: PathBuf,
    table: Option<BundleTable>,
    error: Option<String>,
    stamp: Option<(u64, u64)>,
    selected: Option<String>,
    filter: String,
    missing_only: bool,
    input: Option<Input>,
    focus_input: bool,
}

#[derive(Debug, Default)]
pub(super) struct BundleOutcome {
    pub(super) ask: Option<(&'static str, Value)>,
}

pub(super) fn locale_label(locale: &str) -> String {
    if locale.is_empty() { t!("bundle-default-locale") } else { locale.to_owned() }
}

const TREE_MARK: &str = "\0bundle ";

pub(super) fn tree_path(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{TREE_MARK}{name}"))
}

pub(super) fn bundle_at(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?.strip_prefix(TREE_MARK)?;
    Some(path.parent()?.join(format!("{name}.{}", zen_workbench::resource_bundles::EXTENSION)))
}

pub(super) fn bundle_icon(painter: &egui::Painter, rect: Rect, p: &IdePalette) {
    let stroke = Stroke::new(1.2, p.text_secondary);
    let back = Rect::from_min_size(rect.min + vec2(rect.width() * 0.3, rect.height() * 0.08), rect.size() * 0.62);
    painter.rect_stroke(back, 1.5, stroke, egui::StrokeKind::Inside);
    let front = Rect::from_min_size(rect.min + vec2(rect.width() * 0.08, rect.height() * 0.3), rect.size() * 0.62);
    painter.rect_filled(front, 1.5, p.island);
    painter.rect_stroke(front, 1.5, Stroke::new(1.2, p.accent), egui::StrokeKind::Inside);
    for row in [0.38, 0.62] {
        let y = front.top() + front.height() * row;
        painter.line_segment([pos2(front.left() + 2.5, y), pos2(front.right() - 2.5, y)], Stroke::new(1.0, p.accent));
    }
}

fn shown_value(value: &str) -> String {
    value.replace('\n', "⏎").replace('\t', "⇥")
}

impl BundleTab {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            table: None,
            error: None,
            stamp: None,
            selected: None,
            filter: String::new(),
            missing_only: false,
            input: None,
            focus_input: false,
        }
    }

    pub(super) fn title(&self) -> String {
        let name = self.table.as_ref().map_or_else(
            || self.path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
            |table| table.name.clone(),
        );
        t!("bundle-tab-title", name = name.as_str())
    }

    pub(super) fn table(&self) -> Option<&BundleTable> {
        self.table.as_ref()
    }

    pub(super) fn set_table(&mut self, answer: Result<Value, String>) {
        match answer.and_then(|value| serde_json::from_value::<BundleTable>(value).map_err(|error| error.to_string())) {
            Ok(table) => {
                if let Some(selected) = &self.selected
                    && !table.keys.iter().any(|key| &key.key == selected)
                {
                    self.selected = None;
                }
                self.path.clone_from(&table.path);
                self.table = Some(table);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn needs_reload(&mut self, stamp: (u64, u64)) -> bool {
        let stale = self.stamp != Some(stamp);
        self.stamp = Some(stamp);
        stale
    }

    pub(super) fn select(&mut self, key: Option<String>) {
        self.selected = key;
    }

    pub(super) fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub(super) fn start_new_key(&mut self) {
        self.input = Some(Input::NewKey(String::new()));
        self.focus_input = true;
    }

    pub(super) fn start_rename(&mut self) {
        if let Some(key) = self.selected.clone() {
            self.input = Some(Input::Rename(key.clone(), key));
            self.focus_input = true;
        }
    }

    pub(super) fn set_filter(&mut self, filter: &str, missing_only: bool) {
        filter.clone_into(&mut self.filter);
        self.missing_only = missing_only;
    }

    pub(super) fn rows(&self) -> Vec<&BundleKey> {
        let filter = self.filter.to_lowercase();
        self.table
            .as_ref()
            .map(|table| {
                table
                    .keys
                    .iter()
                    .filter(|key| !self.missing_only || !key.missing.is_empty())
                    .filter(|key| {
                        filter.is_empty()
                            || key.key.to_lowercase().contains(&filter)
                            || key.values.iter().flatten().any(|value| value.to_lowercase().contains(&filter))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn button(&self, ui: &mut Ui, p: &IdePalette, rect: Rect, id: &str, label: &str, enabled: bool) -> bool {
        let response = ui.interact(rect, ui.scope_id().with(("bundle-button", id)), Sense::click());
        crate::accessibility::button(&response, label);
        if enabled {
            hover(ui, &response, p);
        } else {
            crate::accessibility::disabled(&response);
        }
        hint(ui, &response, label, None, Side::Below);
        enabled && response.clicked()
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, outcome: &mut BundleOutcome) {
        let bar = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), BAR));
        let y = bar.center().y;
        let mut x = bar.left() + 8.0;
        let selected = self.selected.clone();
        let mut next = |width: f32| {
            let rect = Rect::from_min_size(pos2(x, y - 12.0), Vec2::splat(width));
            x += width + 4.0;
            rect
        };
        let add = next(24.0);
        if self.button(ui, p, add, "add", &t!("bundle-add-key"), self.table.is_some()) {
            self.start_new_key();
        }
        super::glyphs::glyph_plus(ui.painter(), add.shrink(5.0), p.text_secondary);
        let rename = next(24.0);
        if self.button(ui, p, rename, "rename", &t!("bundle-rename-key"), selected.is_some()) {
            self.start_rename();
        }
        super::glyphs::glyph_pencil(ui.painter(), rename.shrink(5.0), p.text_secondary);
        let remove = next(24.0);
        if self.button(ui, p, remove, "remove", &t!("bundle-remove-key"), selected.is_some())
            && let Some(key) = &selected
        {
            outcome.ask = Some(("properties.bundle.removeKey", json!({ "path": self.path, "key": key })));
        }
        super::glyphs::glyph_trash(ui.painter(), remove.shrink(5.0), p.text_secondary);
        let undo = next(24.0);
        if self.button(ui, p, undo, "undo", &t!("bundle-undo"), self.table.is_some()) {
            outcome.ask = Some(("properties.bundle.undo", json!({ "path": self.path })));
        }
        super::glyphs::glyph_undo(ui.painter(), undo.shrink(5.0), p.text_secondary);
        let field = Rect::from_min_size(pos2(x + 8.0, y - 12.0), vec2(200.0, 24.0));
        let edit = egui::TextEdit::singleline(&mut self.filter)
            .hint_text(t!("bundle-filter"))
            .font(theme::regular(12.5))
            .desired_width(field.width());
        let response = ui.put(field, edit);
        crate::accessibility::labelled(&response, &t!("bundle-filter"));
        let label = t!("bundle-missing-only");
        let label_width =
            ui.painter().layout_no_wrap(label.clone(), egui::TextStyle::Body.resolve(ui.style()), p.text).size().x;
        let toggle = Rect::from_min_size(pos2(field.right() + 12.0, y - 11.0), vec2(label_width + 30.0, 22.0));
        let mut missing_only = self.missing_only;
        ui.put(toggle, egui::Checkbox::new(&mut missing_only, label));
        self.missing_only = missing_only;
        if let Some(table) = &self.table {
            let summary = t!("bundle-summary", keys = table.keys.len(), incomplete = table.incomplete);
            let color = if table.incomplete > 0 { p.warning_text } else { p.text_secondary };
            let galley = ui.painter().layout_no_wrap(summary, theme::regular(12.0), color);
            if bar.right() - 10.0 - galley.size().x > toggle.right() + 8.0 {
                ui.painter().galley(
                    pos2(bar.right() - 10.0 - galley.size().x, y - galley.size().y / 2.0),
                    galley,
                    color,
                );
            }
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        ui.advance_cursor_after_rect(bar);
    }

    fn input_row(&mut self, ui: &mut Ui, p: &IdePalette, outcome: &mut BundleOutcome) {
        let Some(input) = &mut self.input else { return };
        let label = match input {
            Input::NewKey(_) => t!("bundle-new-key"),
            Input::Rename(old, _) => t!("bundle-rename-to", key = old.as_str()),
            Input::Value { .. } => return,
        };
        let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), BAR));
        let caption = ui.painter().text(
            pos2(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label.clone(),
            theme::regular(12.5),
            p.text,
        );
        let field = Rect::from_min_max(
            pos2(caption.right() + 8.0, row.top() + 5.0),
            pos2(row.right() - 10.0, row.bottom() - 5.0),
        );
        let text = match input {
            Input::NewKey(text) | Input::Rename(_, text) => text,
            Input::Value { .. } => return,
        };
        let response =
            ui.put(field, egui::TextEdit::singleline(text).font(theme::regular(12.5)).desired_width(field.width()));
        crate::accessibility::labelled(&response, &label);
        if std::mem::take(&mut self.focus_input) {
            response.request_focus();
        }
        let enter = response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        let escape = ui.input(|input| input.key_pressed(Key::Escape));
        ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        ui.advance_cursor_after_rect(row);
        if escape {
            self.input = None;
            return;
        }
        if enter {
            let ask = match self.input.take() {
                Some(Input::NewKey(key)) if !key.is_empty() => {
                    self.selected = Some(key.clone());
                    Some(("properties.bundle.addKey", json!({ "path": self.path, "key": key })))
                }
                Some(Input::Rename(old, new)) if !new.is_empty() && new != old => {
                    self.selected = Some(new.clone());
                    Some(("properties.bundle.renameKey", json!({ "path": self.path, "key": old, "newKey": new })))
                }
                _ => None,
            };
            outcome.ask = ask;
        }
    }

    fn header(&self, ui: &mut Ui, p: &IdePalette, columns: &[(f32, f32)]) {
        let Some(table) = &self.table else { return };
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        ui.painter().rect_filled(header, 0, p.frame);
        let titles =
            std::iter::once(t!("bundle-key")).chain(table.locales.iter().map(|file| locale_label(&file.locale)));
        for ((left, width), title) in columns.iter().zip(titles) {
            let clip =
                ui.painter().with_clip_rect(Rect::from_min_size(pos2(*left, header.top()), vec2(*width - 6.0, ROW)));
            clip.text(
                pos2(left + 8.0, header.center().y),
                Align2::LEFT_CENTER,
                title,
                theme::semibold(12.0),
                p.text_secondary,
            );
        }
        ui.painter().hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, keys: bool) -> BundleOutcome {
        let mut outcome = BundleOutcome::default();
        self.toolbar(ui, p, &mut outcome);
        self.input_row(ui, p, &mut outcome);
        if let Some(error) = &self.error {
            let body = ui.available_rect_before_wrap();
            ui.painter().text(body.center(), Align2::CENTER_CENTER, error, theme::regular(13.0), p.error_text);
            return outcome;
        }
        let Some(table) = &self.table else { return outcome };
        let (locales, no_keys): (Vec<String>, bool) =
            (table.locales.iter().map(|file| file.locale.clone()).collect(), table.keys.is_empty());
        let width = ui.available_width();
        let left = ui.available_rect_before_wrap().left();
        let key_width = (width * KEY_SHARE).max(120.0);
        let rest = ((width - key_width) / locales.len().max(1) as f32).max(80.0);
        let mut columns = vec![(left, key_width)];
        for index in 0..locales.len() {
            columns.push((left + key_width + index as f32 * rest, rest));
        }
        self.header(ui, p, &columns);
        if keys && self.input.is_none() {
            if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z)) {
                outcome.ask = Some(("properties.bundle.undo", json!({ "path": self.path })));
            }
            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
                && let Some(key) = &self.selected
            {
                outcome.ask = Some(("properties.bundle.removeKey", json!({ "path": self.path, "key": key })));
            }
            if ui.input_mut(|input| input.consume_key(Modifiers::SHIFT, Key::F6)) {
                self.start_rename();
            }
        }
        let rows: Vec<BundleKey> = self.rows().into_iter().cloned().collect();
        if rows.is_empty() {
            let body = ui.available_rect_before_wrap();
            let message = if no_keys { t!("bundle-empty") } else { t!("bundle-nothing-shown") };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, message, theme::regular(13.0), p.text_secondary);
            return outcome;
        }
        let mut clicked = None;
        let mut started = None;
        let area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("bundle-rows");
        area.show_rows(ui, ROW, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let selected = self.selected.as_deref() == Some(row.key.as_str());
                crate::accessibility::item(&response, egui::Role::Row, selected, &row.key);
                if selected {
                    ui.painter().rect_filled(rect, 0, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 0, p.hover);
                }
                if response.clicked() || response.double_clicked() {
                    clicked = Some(row.key.clone());
                }
                let key_color = if row.missing.is_empty() { p.text } else { p.warning_text };
                let (key_left, key_width) = columns[0];
                let clip = ui.painter().with_clip_rect(Rect::from_min_size(pos2(key_left, rect.top()), vec2(key_width - 6.0, ROW)));
                clip.text(pos2(key_left + 8.0, rect.center().y), Align2::LEFT_CENTER, &row.key, theme::regular(12.5), key_color);
                for (index, (value, locale)) in row.values.iter().zip(&locales).enumerate() {
                    let (cell_left, cell_width) = columns[index + 1];
                    let cell = Rect::from_min_size(pos2(cell_left, rect.top()), vec2(cell_width, ROW));
                    ui.painter().vline(cell.left(), cell.y_range(), Stroke::new(1.0, p.tab_border));
                    let editing = matches!(&self.input, Some(Input::Value { key, locale: at, .. }) if *key == row.key && at == locale);
                    if editing {
                        if let Some(Input::Value { text, .. }) = &mut self.input {
                            let edit = egui::TextEdit::singleline(text).font(theme::regular(12.5)).desired_width(cell.width() - 8.0);
                            let enter = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                            let response = ui.put(cell.shrink2(vec2(4.0, 2.0)), edit);
                            crate::accessibility::labelled(&response, &format!("{} {}", row.key, locale_label(locale)));
                            if std::mem::take(&mut self.focus_input) {
                                response.request_focus();
                            }
                            if ui.input(|input| input.key_pressed(Key::Escape)) {
                                started = Some(None);
                            } else if enter || response.lost_focus() {
                                started = Some(Some((row.key.clone(), locale.clone(), text.clone())));
                            }
                        }
                        continue;
                    }
                    let missing = value.as_deref().is_none_or(str::is_empty);
                    if missing {
                        ui.painter().rect_filled(cell.shrink(1.0), 0, p.diagnostic_warning.gamma_multiply(0.12));
                    }
                    let clip = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
                    let (text, color) = match value {
                        Some(value) if !value.is_empty() => (shown_value(value), p.string),
                        Some(_) => (t!("bundle-empty-value"), p.text_secondary),
                        None => (t!("bundle-missing"), p.warning_text),
                    };
                    clip.text(pos2(cell.left() + 8.0, cell.center().y), Align2::LEFT_CENTER, text, theme::regular(12.5), color);
                    let cell_response = ui.interact(cell, ui.scope_id().with(("bundle-cell", &row.key, locale)), Sense::click());
                    if cell_response.clicked() {
                        clicked = Some(row.key.clone());
                    }
                    if cell_response.double_clicked() {
                        self.input = Some(Input::Value {
                            key: row.key.clone(),
                            locale: locale.clone(),
                            text: value.clone().unwrap_or_default(),
                        });
                        self.focus_input = true;
                    }
                }
            }
        });
        if let Some(key) = clicked {
            self.selected = Some(key);
        }
        match started {
            Some(None) => self.input = None,
            Some(Some((key, locale, value))) => {
                self.input = None;
                let unchanged = self
                    .table
                    .as_ref()
                    .and_then(|table| {
                        let column = table.locales.iter().position(|file| file.locale == locale)?;
                        table.keys.iter().find(|row| row.key == key)?.values.get(column).cloned()
                    })
                    .flatten()
                    .is_some_and(|before| before == value);
                if !unchanged {
                    outcome.ask = Some((
                        "properties.bundle.setValue",
                        json!({ "path": self.path, "key": key, "locale": locale, "value": value }),
                    ));
                }
            }
            None => {}
        }
        outcome
    }

    pub(super) fn agent_state(&self) -> Value {
        json!({
            "path": self.path,
            "selected": self.selected,
            "filter": self.filter,
            "missingOnly": self.missing_only,
            "rows": self.rows().iter().map(|key| key.key.as_str()).collect::<Vec<_>>(),
            "error": self.error,
        })
    }
}

impl IdeView {
    pub(super) fn bundle_row(
        &mut self,
        ui: &mut Ui,
        bundle: &zen_workbench::resource_bundles::Bundle,
        depth: usize,
        active: Option<&Path>,
        frame: &mut super::TreeFrame,
        mode: super::libraries_tree::RowMode<'_>,
    ) {
        let path = tree_path(&bundle.folder, &bundle.name);
        let label = t!("bundle-tree-label", name = bundle.name.as_str());
        let entry = zen_vfs::Entry { path: path.clone(), name: label.clone(), is_dir: true, is_symlink: false };
        let row = super::libraries_tree::RowMode {
            label: Some(&label),
            kind: super::libraries_tree::RowKind::Bundle,
            ..mode
        };
        self.tree_row(ui, &entry, depth, active, frame, row);
        if self.expanded.contains(&path) {
            for file in &bundle.files {
                let name = file.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let child = zen_vfs::Entry { path: file.path.clone(), name, is_dir: false, is_symlink: false };
                self.tree_row(ui, &child, depth + 1, active, frame, mode);
            }
        }
    }

    pub(super) fn open_bundle(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
    ) -> Result<Value, String> {
        let answer = registry
            .dispatch(session, "properties.bundle.get", json!({ "path": path }))
            .map_err(|error| error.to_string())?;
        let mut tab = BundleTab::new(path.to_owned());
        tab.set_table(Ok(answer.clone()));
        tab.stamp = Some(Self::bundle_stamp(session, &tab));
        self.bundle = Some(tab);
        self.bring_front(super::front::FrontKind::Bundle, session);
        self.tree_focused = false;
        self.close_text_dialogs();
        Ok(answer)
    }

    fn bundle_stamp(session: &Session, tab: &BundleTab) -> (u64, u64) {
        let Some(workbench) = session.state::<Workbench>() else { return (0, 0) };
        let files: Vec<&Path> =
            tab.table().map(|table| table.locales.iter().map(|file| file.path.as_path()).collect()).unwrap_or_default();
        let versions = workbench
            .documents()
            .iter()
            .filter(|document| files.contains(&document.path()))
            .map(zen_workbench::Document::version)
            .sum::<u64>();
        let modified = files
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok()?.modified().ok())
            .filter_map(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_millis() as u64)
            .sum::<u64>();
        (workbench.revision().wrapping_add(versions), modified)
    }

    pub(super) fn bundle_frame(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let keys = !self.tree_focused && !egui::Popup::is_any_open(ui.ctx());
        let Some(mut tab) = self.bundle.take() else { return };
        let stamp = Self::bundle_stamp(session, &tab);
        if tab.needs_reload(stamp) {
            let answer = registry.dispatch(session, "properties.bundle.get", json!({ "path": tab.path }));
            tab.set_table(answer.map_err(|error| error.to_string()));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
        let outcome = tab.show(ui, &p, keys);
        if let Some((command, params)) = outcome.ask {
            match registry.dispatch(session, command, params) {
                Ok(answer) => {
                    tab.set_table(Ok(answer));
                    tab.stamp = Some(Self::bundle_stamp(session, &tab));
                }
                Err(error) => self.report(ui, error),
            }
        }
        self.bundle = Some(tab);
    }

    pub(super) fn bundle_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, super::agent::ActionError> {
        use super::agent::ActionError::Refused;
        let path = params.get("path").and_then(Value::as_str).map(PathBuf::from);
        if action == "view.resourceBundle.open" {
            let path =
                path.ok_or_else(|| Refused("view.resourceBundle.open needs `path`: a file of the bundle".to_owned()))?;
            let path = session.state_mut::<Workbench>().resolve(&path);
            self.open_bundle(registry, session, &path).map_err(Refused)?;
            return Ok(self.bundle.as_ref().map(BundleTab::agent_state).unwrap_or_default());
        }
        let Some(tab) = &mut self.bundle else {
            return Err(Refused("no resource bundle is open: run view.resourceBundle.open first".to_owned()));
        };
        match action {
            "view.resourceBundle.select" => {
                tab.select(params.get("key").and_then(Value::as_str).map(str::to_owned));
            }
            "view.resourceBundle.filter" => {
                let text = params.get("text").and_then(Value::as_str).unwrap_or_default();
                let missing = params.get("missingOnly").and_then(Value::as_bool).unwrap_or(false);
                tab.set_filter(text, missing);
            }
            "view.resourceBundle.addKey" => tab.start_new_key(),
            "view.resourceBundle.renameKey" => {
                if tab.selected().is_none() {
                    return Err(Refused("select a key first (view.resourceBundle.select)".to_owned()));
                }
                tab.start_rename();
            }
            _ => return Err(Refused(format!("{action} is not an action of the Resource Bundle editor"))),
        }
        Ok(tab.agent_state())
    }
}
