use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, CursorIcon, Event, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2,
    vec2,
};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::database::WORKSPACE;
use zen_workbench::database::navigator::{self, Row, RowKey, View};
use zen_workbench::database::structure::Structure;

use super::glyphs::{
    Glyph, glyph_collapse_all, glyph_database, glyph_folder, glyph_locate, glyph_lock, glyph_more, glyph_plus_menu,
    glyph_refresh, glyph_rerun, glyph_run, glyph_search, glyph_stop, stroke,
};
use super::{IdePalette, Side, customization, hint, hover, metrics, theme};
use crate::accessibility;

const TOOLBAR: f32 = 30.0;
const FILTER: f32 = 30.0;
const DEFAULT_WIDTH: f32 = 280.0;
const MIN_WIDTH: f32 = 180.0;
const MAX_WIDTH: f32 = 640.0;
const COLLAPSED_WIDTH: f32 = 30.0;
const GRIP: f32 = 5.0;
const STALE: Duration = Duration::from_millis(500);
const SIZE_BOX: f32 = 44.0;
const SIZE_ROOM: f32 = 64.0;
const SCRIPTS: usize = 30;
const CODE_WAIT: Duration = Duration::from_secs(10);
const CODE_RETRY: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Command(&'static str, Value),
    Select(RowKey),
    Toggle(RowKey, bool),
    Open(RowKey, Option<&'static str>),
    Edit(String),
    NewConnection(Option<String>),
    Copy(RowKey, bool),
    Console(String, Option<String>),
    Generate(RowKey, Generate),
    Code(RowKey, &'static str),
    ClientCommand(String),
    Filter(String),
    Collapse(bool),
    Export(RowKey),
    Import(RowKey),
    Backup(RowKey, bool),
    Transfer(RowKey),
    Scripts(String),
    OpenScript(PathBuf),
    NewScript(String),
    FindUsages(RowKey),
    Rename(RowKey),
    Delete(RowKey),
    Truncate(RowKey),
}

#[derive(Clone, Debug, PartialEq)]
struct Deletion {
    command: &'static str,
    key: RowKey,
    title: String,
    loss: String,
    statements: Vec<String>,
    can_cascade: bool,
    cascade: bool,
    token: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    Rename { key: RowKey, name: String, error: Option<String>, focus: bool },
    Delete(Deletion),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Generate {
    Ddl,
    Select,
    Insert,
}

pub(super) struct NavigatorPanel {
    width: f32,
    collapsed: bool,
    rows: Vec<Row>,
    read: Option<(Instant, u64, u64)>,
    filter: String,
    focus_filter: bool,
    reveal: bool,
    error: Option<String>,
    tree: Option<egui::Id>,
    drivers: Vec<(String, String)>,
    scripts: (String, Vec<Value>),
    code: Option<(RowKey, &'static str, Instant, Instant)>,
    dialog: Option<Dialog>,
    seen: Option<RowKey>,
    watching: Vec<String>,
}

impl Default for NavigatorPanel {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            collapsed: false,
            rows: Vec::new(),
            read: None,
            filter: String::new(),
            focus_filter: false,
            reveal: false,
            error: None,
            tree: None,
            drivers: Vec::new(),
            scripts: (String::new(), Vec::new()),
            code: None,
            dialog: None,
            seen: None,
            watching: Vec::new(),
        }
    }
}

fn message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.to_string(),
        other => other.to_string(),
    }
}

fn row_params(key: &RowKey) -> Value {
    json!({ "connection": key.connection, "folder": key.folder, "path": key.path })
}

pub(super) fn type_color(p: &IdePalette, color: &str) -> Option<Color32> {
    match color {
        "green" => Some(p.vcs_added),
        "red" => Some(p.error_text),
        "orange" => Some(p.warning_text),
        "blue" => Some(p.link),
        "violet" => Some(p.keyword),
        _ => None,
    }
}

fn glyph_table(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.08);
    painter.rect_stroke(r, 1.5, s, StrokeKind::Middle);
    let header = r.top() + r.height() * 0.32;
    painter.hline(r.x_range(), header, s);
    painter.hline(r.x_range(), r.top() + r.height() * 0.66, s);
    painter.vline(r.center().x, header..=r.bottom(), s);
}

fn glyph_view(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.08);
    painter.rect_stroke(r, 1.5, s, StrokeKind::Middle);
    let eye = Rect::from_center_size(
        pos2(r.center().x, r.top() + r.height() * 0.62),
        vec2(r.width() * 0.62, r.height() * 0.34),
    );
    painter.rect_stroke(eye, eye.height() / 2.0, s, StrokeKind::Middle);
    painter.circle_filled(eye.center(), eye.height() * 0.22, color);
    painter.hline(r.x_range(), r.top() + r.height() * 0.28, s);
}

fn glyph_column(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.16);
    let column = Rect::from_center_size(r.center(), vec2(r.width() * 0.46, r.height()));
    painter.rect_stroke(column, 1.5, s, StrokeKind::Middle);
    painter.hline(column.x_range(), column.top() + column.height() * 0.3, s);
}

fn glyph_key(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.1);
    let ring = pos2(r.left() + r.width() * 0.28, r.center().y);
    painter.circle_stroke(ring, r.width() * 0.22, s);
    let start = ring.x + r.width() * 0.22;
    painter.hline(start..=r.right(), ring.y, s);
    painter.vline(r.right() - r.width() * 0.04, ring.y..=ring.y + r.height() * 0.22, s);
    painter.vline(r.right() - r.width() * 0.24, ring.y..=ring.y + r.height() * 0.16, s);
}

fn glyph_foreign_key(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_key(painter, Rect::from_min_size(rect.min, rect.size() * 0.8), color);
    let s = stroke(rect, color);
    let tip = pos2(rect.right(), rect.bottom() - rect.height() * 0.12);
    let from = pos2(rect.center().x, tip.y);
    painter.line_segment([from, tip], s);
    painter.line_segment([tip, tip - vec2(rect.width() * 0.16, rect.height() * 0.12)], s);
    painter.line_segment([tip, tip - vec2(rect.width() * 0.16, -rect.height() * 0.12)], s);
}

fn glyph_index(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    for (at, width) in [(0.15, 1.0), (0.5, 0.7), (0.85, 0.45)] {
        let y = r.top() + r.height() * at;
        painter.hline(r.left()..=r.left() + r.width() * width, y, s);
    }
}

fn glyph_letters(painter: &egui::Painter, rect: Rect, color: Color32, letters: &str) {
    painter.text(rect.center(), Align2::CENTER_CENTER, letters, theme::semibold(rect.height() * 0.72), color);
}

fn glyph_function(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_letters(painter, rect, color, "ƒ");
}

fn glyph_sequence(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_letters(painter, rect.expand(rect.width() * 0.1), color, "123");
}

fn glyph_type(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_letters(painter, rect, color, "T");
}

fn glyph_trigger(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.12);
    let points = vec![
        pos2(r.left() + r.width() * 0.62, r.top()),
        pos2(r.left() + r.width() * 0.2, r.top() + r.height() * 0.58),
        pos2(r.center().x, r.top() + r.height() * 0.58),
        pos2(r.left() + r.width() * 0.38, r.bottom()),
        pos2(r.right() - r.width() * 0.2, r.top() + r.height() * 0.4),
        pos2(r.center().x, r.top() + r.height() * 0.4),
    ];
    painter.add(egui::Shape::closed_line(points, stroke(rect, color)));
}

fn glyph_schema(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.1);
    let cell = vec2(r.width() * 0.4, r.height() * 0.4);
    for corner in
        [r.left_top(), pos2(r.right() - cell.x, r.top()), pos2(r.left(), r.bottom() - cell.y), r.right_bottom() - cell]
    {
        painter.rect_stroke(Rect::from_min_size(corner, cell), 1.0, s, StrokeKind::Middle);
    }
}

fn glyph_role(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    painter.circle_stroke(pos2(r.center().x, r.top() + r.height() * 0.3), r.width() * 0.22, s);
    let body = Rect::from_min_max(pos2(r.left() + r.width() * 0.1, r.top() + r.height() * 0.62), r.right_bottom());
    let body = Rect::from_min_max(body.min, pos2(r.right() - r.width() * 0.1, body.max.y));
    painter.rect_stroke(body, egui::CornerRadius { nw: 5, ne: 5, sw: 1, se: 1 }, s, StrokeKind::Middle);
}

fn glyph_extension(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.14);
    let block =
        Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.25), pos2(r.right() - r.width() * 0.25, r.bottom()));
    painter.rect_stroke(block, 1.5, s, StrokeKind::Middle);
    painter.circle_stroke(pos2(block.center().x, block.top()), r.width() * 0.13, s);
    painter.circle_stroke(pos2(block.right(), block.center().y), r.width() * 0.13, s);
}

fn icon(kind: &str) -> Option<Glyph> {
    Some(match kind {
        "connection" | "catalog" | "node" => glyph_database,
        "connectionFolder" | "folder" | "keyPrefix" => glyph_folder,
        "schema" => glyph_schema,
        "table" | "foreignTable" | "partition" | "dictionary" => glyph_table,
        "view" | "materializedView" => glyph_view,
        "column" => glyph_column,
        "constraint" => glyph_key,
        "foreignKey" | "reference" => glyph_foreign_key,
        "index" => glyph_index,
        "function" | "procedure" | "package" => glyph_function,
        "sequence" => glyph_sequence,
        "type" => glyph_type,
        "trigger" | "policy" | "event" | "eventTrigger" => glyph_trigger,
        "tablespace" => glyph_database,
        "role" => glyph_role,
        "extension" => glyph_extension,
        "key" | "collection" | "searchIndex" => glyph_key,
        _ => return None,
    })
}

fn icon_color(p: &IdePalette, kind: &str) -> Color32 {
    match kind {
        "connection" | "catalog" | "node" => p.accent,
        "table" | "foreignTable" | "view" | "materializedView" | "partition" | "dictionary" => p.link,
        "constraint" | "foreignKey" | "reference" => p.warning_text,
        "function" | "procedure" | "package" | "trigger" | "event" | "eventTrigger" => p.function,
        "extension" | "role" => p.type_name,
        "sequence" | "type" => p.type_name,
        _ => p.text_secondary,
    }
}

pub(super) fn paint_found_icon(painter: &egui::Painter, rect: Rect, kind: &str, p: &IdePalette) {
    match kind {
        "script" | "console" => crate::file_icons::file_icon_at(painter, rect, Path::new("script.sql")),
        "history" => super::database_view::glyph_history(painter, rect.shrink(1.0), p.text_secondary),
        _ => {
            if let Some(glyph) = icon(kind) {
                glyph(painter, rect.shrink(1.0), icon_color(p, kind));
            }
        }
    }
}

fn openable(row: &Row) -> bool {
    let relational = matches!(
        row.kind.as_str(),
        "table"
            | "foreignTable"
            | "view"
            | "materializedView"
            | "function"
            | "procedure"
            | "sequence"
            | "type"
            | "event"
    ) && row.key.path.len() == 4;
    let own_tree = matches!(row.kind.as_str(), "table" | "view" | "dictionary") && row.key.path.len() == 3;
    relational || own_tree || row.kind == "key" || leaf_item(row)
}

fn leaf_item(row: &Row) -> bool {
    let path = &row.key.path;
    let index = row.kind == "index" && path.len() == 4 && path[2] == zen_db::schema::SCHEMA_INDEXES;
    let database = matches!(row.kind.as_str(), "extension" | "eventTrigger" | "tablespace" | "role")
        && path.len() == 3
        && path[1].starts_with('#');
    index || database
}

fn in_schema(row: &Row) -> bool {
    row.key.path.len() >= 2 && !row.key.path[1].starts_with('#')
}

fn has_usages(row: &Row) -> bool {
    let object =
        matches!(row.kind.as_str(), "table" | "foreignTable" | "view" | "materializedView" | "function" | "procedure")
            && row.key.path.len() == 4;
    let column = row.kind == "column" && row.key.path.len() == 6;
    (object || column) && !key_value(row)
}

fn key_value(row: &Row) -> bool {
    matches!(row.kind.as_str(), "key" | "keyPrefix" | "node" | "more")
}

fn is_schema(row: &Row) -> bool {
    row.kind == "schema" && row.key.path.len() == 2
}

fn backs_up(driver: Option<&str>) -> bool {
    driver.and_then(zen_db::backup::Family::of).is_some()
}

fn connected(row: &Row) -> bool {
    matches!(row.state, Some(zen_db::engine::ConnectionState::Connected | zen_db::engine::ConnectionState::Closed))
}

fn open_action(row: &Row) -> Action {
    let page = (!key_value(row) && row.key.path.len() == 4).then_some("properties");
    Action::Open(row.key.clone(), page)
}

fn object_kind(row: &Row) -> bool {
    row.key.path.get(2).and_then(|folder| zen_db::schema::ObjectKind::from_folder(folder)).is_some()
}

fn deletable(row: &Row) -> bool {
    if row.kind == "connection" {
        return true;
    }
    if key_value(row) || row.key.connection.is_none() {
        return false;
    }
    match row.key.path.len() {
        2 => is_schema(row),
        4 => object_kind(row),
        6 => matches!(row.key.path[4].as_str(), "columns" | "indexes" | "constraints" | "foreignKeys"),
        _ => false,
    }
}

fn renamable(row: &Row) -> bool {
    if row.kind == "connection" {
        return true;
    }
    if key_value(row) || row.key.connection.is_none() {
        return false;
    }
    match row.key.path.len() {
        2 => is_schema(row),
        4 => object_kind(row),
        6 => row.key.path[4] == "columns",
        _ => false,
    }
}

fn truncatable(row: &Row) -> bool {
    row.kind == "table" && row.key.path.len() == 4 && row.key.path[2] == "tables"
}

pub(super) fn short_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit > 0 && value < 10.0 && (value * 10.0).round() % 10.0 != 0.0 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

impl NavigatorPanel {
    pub(super) fn width(&self) -> f32 {
        if self.collapsed { COLLAPSED_WIDTH } else { self.width }
    }

    pub(super) fn sizes(&self) -> [(String, f32); 2] {
        [
            ("navigator".to_owned(), self.width),
            ("navigatorCollapsed".to_owned(), if self.collapsed { 1.0 } else { 0.0 }),
        ]
    }

    pub(super) fn restore(&mut self, sizes: &std::collections::BTreeMap<String, f32>) {
        if let Some(width) = sizes.get("navigator") {
            self.width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        }
        if let Some(collapsed) = sizes.get("navigatorCollapsed") {
            self.collapsed = *collapsed > 0.5;
        }
    }

    fn refresh_rows(&mut self, session: &mut Session) {
        let navigator_revision = navigator::navigator(session).revision();
        let structure_revision = session.state::<Structure>().map_or(0, Structure::revision);
        let fresh = self.read.is_some_and(|(at, nav, structure)| {
            at.elapsed() < STALE && nav == navigator_revision && structure == structure_revision
        });
        if fresh {
            return;
        }
        self.rows = navigator::rows(session);
        let navigator_revision = navigator::navigator(session).revision();
        let structure_revision = session.state::<Structure>().map_or(0, Structure::revision);
        self.read = Some((Instant::now(), navigator_revision, structure_revision));
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0, p.island);
        let mut actions = Vec::new();
        if self.collapsed {
            let button = Rect::from_min_size(area.min + vec2(2.0, 3.0), Vec2::splat(TOOLBAR - 6.0));
            let response = ui.interact(button, ui.scope_id().with("navigator-expand"), Sense::click());
            hover(ui, &response, p);
            hint(ui, &response, &t!("ide-database-navigator-show"), None, Side::Right);
            glyph_database(ui.painter(), button.shrink(5.0), p.text_secondary);
            if response.clicked() {
                self.collapsed = false;
            }
            return;
        }
        self.refresh_rows(session);
        if self.drivers.is_empty()
            && let Ok(listed) = registry.dispatch(session, "db.driver.list", json!({}))
        {
            self.drivers = listed["drivers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|driver| Some((driver["id"].as_str()?.to_owned(), driver["name"].as_str()?.to_owned())))
                .collect();
        }
        let grip = Rect::from_min_max(pos2(area.right() - GRIP, area.top()), area.right_bottom());
        let resize = ui.interact(grip, ui.scope_id().with("navigator-grip"), Sense::drag());
        if resize.hovered() || resize.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if resize.dragged() {
            self.width = (self.width + resize.drag_delta().x).clamp(MIN_WIDTH, MAX_WIDTH);
        }
        let bar = Rect::from_min_size(area.min, vec2(area.width() - GRIP, TOOLBAR));
        self.toolbar(ui, p, bar, session, &mut actions);
        let filter = Rect::from_min_size(pos2(area.left(), bar.bottom()), vec2(area.width() - GRIP, FILTER));
        self.filter_box(ui, p, filter, &mut actions);
        let body = Rect::from_min_max(pos2(area.left(), filter.bottom()), pos2(area.right() - GRIP, area.bottom()));
        let error_height = if self.error.is_some() { 40.0 } else { 0.0 };
        let tree = Rect::from_min_max(body.min, pos2(body.right(), body.bottom() - error_height));
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-navigator-tree").max_rect(tree));
        self.tree(&mut child, p, &mut actions);
        if let Some((key, language, until, next)) = self.code.clone() {
            let now = Instant::now();
            if now >= until {
                self.code = None;
            } else if now >= next {
                actions.push(Action::Code(key, language));
            } else {
                ui.ctx().request_repaint_after(next - now);
            }
        }
        if let Some(error) = &self.error {
            let line =
                Rect::from_min_max(pos2(body.left() + 8.0, tree.bottom()), pos2(body.right() - 8.0, body.bottom()));
            let galley = ui.painter().layout(error.clone(), theme::regular(12.0), p.error_text, line.width());
            ui.painter().galley(line.min + vec2(0.0, 4.0), galley, p.error_text);
            accessibility::announce(ui, ui.scope_id().with("navigator-error"), line, || error.clone());
        }
        self.run(ui, registry, session, actions);
        self.follow_runs(ui, session);
        self.dialogs(ui, p, registry, session);
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, bar: Rect, session: &mut Session, actions: &mut Vec<Action>) {
        let selected = navigator::navigator(session).selected().cloned();
        let connection = selected.as_ref().and_then(|key| key.connection.clone());
        let state = connection.as_ref().and_then(|id| {
            self.rows
                .iter()
                .find(|row| row.kind == "connection" && row.key.connection.as_ref() == Some(id))
                .and_then(|row| row.state)
        });
        let is_connected =
            matches!(state, Some(zen_db::engine::ConnectionState::Connected | zen_db::engine::ConnectionState::Closed));
        let size = TOOLBAR - 6.0;
        let mut x = bar.left() + 4.0;
        let new = Rect::from_min_size(pos2(x, bar.top() + 3.0), Vec2::splat(size));
        let new_response = ui.interact(new, ui.scope_id().with("navigator-new"), Sense::click());
        hover(ui, &new_response, p);
        hint(ui, &new_response, &t!("ide-database-new-connection"), None, Side::Below);
        glyph_plus_menu(ui.painter(), new.shrink(5.0), p.text_secondary);
        egui::Popup::menu(&new_response).show(|ui| {
            ui.set_min_width(200.0);
            for (id, name) in &self.drivers {
                if ui.button(name).clicked() {
                    actions.push(Action::NewConnection(Some(id.clone())));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(t!("ide-database-navigator-new-other")).clicked() {
                actions.push(Action::NewConnection(None));
                ui.close();
            }
        });
        x += size + 2.0;
        let id = connection.clone().unwrap_or_default();
        let top = bar.top() + 3.0;
        let limit = bar.right() - size - 8.0;
        if tool_button(
            ui,
            p,
            (&mut x, top, size, limit),
            "navigator-connect",
            glyph_run,
            t!("ide-database-connect"),
            connection.is_some() && !is_connected,
        ) {
            actions.push(Action::Command("db.connection.connect", json!({ "connection": id })));
        }
        if tool_button(
            ui,
            p,
            (&mut x, top, size, limit),
            "navigator-reconnect",
            glyph_rerun,
            t!("ide-database-reconnect"),
            is_connected,
        ) {
            actions.push(Action::Command("db.connection.invalidate", json!({ "connection": id })));
        }
        if tool_button(
            ui,
            p,
            (&mut x, top, size, limit),
            "navigator-disconnect",
            glyph_stop,
            t!("ide-database-disconnect"),
            is_connected,
        ) {
            actions.push(Action::Command("db.connection.disconnect", json!({ "connection": id })));
        }
        if tool_button(
            ui,
            p,
            (&mut x, top, size, limit),
            "navigator-refresh",
            glyph_refresh,
            t!("ide-database-refresh"),
            selected.is_some(),
        ) {
            actions.push(Action::Command("db.navigator.refresh", json!({})));
        }
        if tool_button(
            ui,
            p,
            (&mut x, top, size, limit),
            "navigator-collapse",
            glyph_collapse_all,
            t!("ide-database-collapse-all"),
            true,
        ) {
            actions.push(Action::Collapse(true));
        }
        let options = navigator::navigator(session).options().clone();
        if x + size <= limit {
            let link = Rect::from_min_size(pos2(x, bar.top() + 3.0), Vec2::splat(size));
            let link_response = ui.interact(link, ui.scope_id().with("navigator-link"), Sense::click());
            if options.link_with_editor {
                ui.painter().rect_filled(link, 6, p.selection);
            }
            hover(ui, &link_response, p);
            hint(ui, &link_response, &t!("ide-database-link-with-editor"), None, Side::Below);
            accessibility::pressed(&link_response, options.link_with_editor);
            glyph_locate(ui.painter(), link.shrink(5.0), p.text_secondary);
            if link_response.clicked() {
                actions.push(Action::Command(
                    "db.navigator.options",
                    json!({ "linkWithEditor": !options.link_with_editor }),
                ));
            }
        }
        let more = Rect::from_min_size(pos2(bar.right() - size - 4.0, bar.top() + 3.0), Vec2::splat(size));
        let more_response = ui.interact(more, ui.scope_id().with("navigator-more"), Sense::click());
        hover(ui, &more_response, p);
        if !egui::Popup::is_any_open(ui.ctx()) {
            hint(ui, &more_response, &t!("ide-database-navigator-options"), None, Side::Below);
        }
        accessibility::button_with(&more_response, || t!("ide-database-navigator-options"));
        glyph_more(ui.painter(), more.shrink(5.0), p.text_secondary);
        egui::Popup::menu(&more_response).show(|ui| {
            ui.set_min_width(240.0);
            ui.label(egui::RichText::new(t!("ide-database-navigator-view")).color(p.text_secondary));
            for (view, label) in [
                (View::Simple, t!("ide-database-view-simple")),
                (View::Advanced, t!("ide-database-view-advanced")),
                (View::Custom, t!("ide-database-view-custom")),
            ] {
                if ui.radio(options.view == view, label).clicked() {
                    actions.push(Action::Command("db.navigator.options", json!({ "view": view })));
                    ui.close();
                }
            }
            ui.separator();
            let mut toggle = |ui: &mut Ui, on: bool, label: String, key: &str| {
                let mut value = on;
                if ui.checkbox(&mut value, label).changed() {
                    let mut params = serde_json::Map::new();
                    params.insert(key.to_owned(), Value::Bool(value));
                    let command = if key == "connectedOnly" { "db.navigator.filter" } else { "db.navigator.options" };
                    actions.push(Action::Command(command, Value::Object(params)));
                }
            };
            toggle(ui, options.show_system_objects, t!("ide-database-show-system"), "showSystemObjects");
            toggle(ui, options.show_host, t!("ide-database-show-host"), "showHost");
            toggle(ui, options.show_descriptions, t!("ide-database-show-descriptions"), "showDescriptions");
            toggle(ui, options.show_statistics, t!("ide-database-show-statistics"), "showStatistics");
            toggle(ui, options.connected_only, t!("ide-database-connected-only"), "connectedOnly");
            ui.separator();
            if ui.button(t!("ide-database-navigator-hide")).clicked() {
                self.collapsed = true;
                ui.close();
            }
        });
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
    }

    fn filter_box(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<Action>) {
        let field = rect.shrink2(vec2(6.0, 3.0));
        ui.painter().rect_filled(field, 5, p.text_field);
        ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.text_field_border), StrokeKind::Inside);
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(field.left() + 12.0, field.center().y), Vec2::splat(12.0)),
            p.text_secondary,
        );
        let text =
            Rect::from_min_max(pos2(field.left() + 24.0, field.top()), pos2(field.right() - 6.0, field.bottom()));
        let hint_text = t!("ide-database-navigator-filter");
        let response = ui.put(
            text,
            egui::TextEdit::singleline(&mut self.filter)
                .id_salt("database-navigator-filter")
                .frame(egui::Frame::NONE)
                .font(theme::regular(13.0))
                .align(Align2::LEFT_CENTER)
                .hint_text(hint_text.clone())
                .desired_width(text.width()),
        );
        accessibility::labelled(&response, &hint_text);
        if std::mem::take(&mut self.focus_filter) {
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(self.filter.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), response.id);
            }
        }
        if response.changed() {
            actions.push(Action::Filter(self.filter.clone()));
        }
        if response.has_focus() && ui.input(|input| input.key_pressed(Key::Escape)) {
            self.filter.clear();
            actions.push(Action::Filter(String::new()));
            if let Some(tree) = self.tree {
                ui.memory_mut(|memory| memory.request_focus(tree));
            }
        }
        if response.has_focus()
            && ui.input(|input| input.key_pressed(Key::ArrowDown))
            && let Some(tree) = self.tree
        {
            ui.memory_mut(|memory| memory.request_focus(tree));
        }
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<Action>) {
        let id = ui.make_persistent_id("database-navigator");
        self.tree = Some(id);
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-database-navigator"));
        if area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        area.context_menu(|ui| {
            customization::popup(
                ui,
                customization::DATABASE_AREA_POPUP,
                |_| true,
                |ui, slot| {
                    if ui.button(t!(slot)).clicked() {
                        actions.push(Action::NewConnection(None));
                        ui.close();
                    }
                },
            );
        });
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused && self.dialog.is_none() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.keys(ui, actions);
        }
        let rows = &self.rows;
        let scripts = self.scripts.clone();
        let mut clicked: Option<(RowKey, bool, bool)> = None;
        egui::ScrollArea::vertical().id_salt("database-navigator-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let height = metrics::TREE_ROW;
            let top = ui.cursor().top();
            let clip = ui.clip_rect();
            let first = (((clip.top() - top) / height).floor().max(0.0) as usize).min(rows.len());
            let last = ((((clip.bottom() - top) / height).ceil().max(0.0) as usize) + 1).clamp(first, rows.len());
            let selected_at = rows.iter().position(|row| row.selected);
            let selected = selected_at.map(|at| rows[at].key.clone());
            if selected.is_some() && selected != self.seen {
                self.reveal = true;
            }
            self.seen = selected;
            if self.reveal
                && let Some(at) = selected_at.filter(|at| *at < first || *at >= last)
            {
                let rect = Rect::from_min_size(
                    pos2(ui.max_rect().left(), top + at as f32 * height),
                    vec2(ui.available_width(), height),
                );
                ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
                self.reveal = false;
            }
            ui.add_space(first as f32 * height);
            for row in &rows[first..last] {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
                let expanded = row.has_children.then_some(row.expanded);
                accessibility::tree_item(&response, row.depth + 1, expanded, row.selected, &row.label);
                let description = [row.detail.clone(), row.error.clone(), row.comment.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ");
                if !description.is_empty() {
                    accessibility::describe(&response, &description);
                }
                let size = paint_row(ui, p, rect, row, focused, response.hovered());
                if let Some(size) = size
                    && response.hover_pos().is_some_and(|at| size.contains(at))
                {
                    let text = size_tip(row);
                    response.clone().on_hover_text(text);
                }
                if row.selected && std::mem::take(&mut self.reveal) {
                    response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                }
                if response.clicked() || response.double_clicked() {
                    let x = rect.left() + 8.0 + row.depth as f32 * metrics::TREE_INDENT;
                    let on_chevron = row.has_children
                        && response.interact_pointer_pos().is_some_and(|at| at.x < x + metrics::TREE_ARROW * 2.0 + 2.0);
                    clicked = Some((row.key.clone(), response.double_clicked(), on_chevron));
                }
                let row = row.clone();
                let scripts = scripts_of(&scripts, row.key.connection.as_deref());
                response.context_menu(|ui| row_menu(ui, &row, scripts, actions));
                if response.secondary_clicked() {
                    actions.push(Action::Select(row.key.clone()));
                    if let Some(connection) = &row.key.connection {
                        actions.push(Action::Scripts(connection.clone()));
                    }
                }
            }
            ui.add_space((rows.len() - last) as f32 * height);
        });
        if let Some((key, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            let row = self.rows.iter().find(|row| row.key == key).cloned();
            actions.push(Action::Select(key.clone()));
            if let Some(row) = row {
                if on_chevron {
                    actions.push(Action::Toggle(key, !row.expanded));
                } else if double {
                    if row.kind == "connection" && !connected(&row) {
                        actions.push(Action::Toggle(key, true));
                    } else if openable(&row) {
                        actions.push(open_action(&row));
                    } else if row.has_children {
                        actions.push(Action::Toggle(key, !row.expanded));
                    }
                }
            }
        }
        if self.rows.is_empty() {
            let text = if self.filter.is_empty() {
                t!("ide-database-navigator-empty")
            } else {
                t!("ide-database-navigator-nothing-found")
            };
            let rect = ui.max_rect();
            let galley = ui.painter().layout(text, theme::regular(12.5), p.text_secondary, rect.width() - 24.0);
            ui.painter().galley(rect.min + vec2(12.0, 12.0), galley, p.text_secondary);
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let none = Modifiers::NONE;
        let mut typed = String::new();
        let (rename, delete) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::F2),
                input.consume_key(none, Key::Delete) || input.consume_key(Modifiers::COMMAND, Key::Backspace),
            )
        });
        let (up, down, left, right, enter, f4, f5, refresh, escape, home, end, new, usages) = ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Text(text) if !text.chars().any(char::is_control) => {
                    typed.push_str(text);
                    false
                }
                _ => true,
            });
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter) || input.consume_key(Modifiers::COMMAND, Key::ArrowDown),
                input.consume_key(none, Key::F4),
                input.consume_key(none, Key::F5),
                input.consume_key(Modifiers::COMMAND, Key::R),
                input.consume_key(none, Key::Escape),
                input.consume_key(none, Key::Home),
                input.consume_key(none, Key::End),
                input.consume_key(Modifiers::COMMAND, Key::N),
                input.consume_key(Modifiers::ALT, Key::F7),
            )
        });
        if new {
            actions.push(Action::NewConnection(None));
            return;
        }
        if !typed.is_empty() {
            self.filter.push_str(&typed);
            self.focus_filter = true;
            actions.push(Action::Filter(self.filter.clone()));
            return;
        }
        if escape && !self.filter.is_empty() {
            self.filter.clear();
            actions.push(Action::Filter(String::new()));
        }
        let rows = &self.rows;
        let at = rows.iter().position(|row| row.selected);
        let target = |index: usize| rows.get(index).map(|row| row.key.clone());
        let mut select = None;
        if up || down {
            select = match at {
                None => target(0),
                Some(at) if up => target(at.saturating_sub(1)),
                Some(at) => target((at + 1).min(rows.len().saturating_sub(1))),
            };
        }
        if home {
            select = target(0);
        }
        if end {
            select = target(rows.len().saturating_sub(1));
        }
        if let Some(key) = select {
            actions.push(Action::Select(key));
            self.reveal = true;
            return;
        }
        let Some(row) = at.and_then(|at| rows.get(at)).cloned() else { return };
        if left {
            if row.has_children && row.expanded {
                actions.push(Action::Toggle(row.key.clone(), false));
            } else if let Some(parent) = rows[..at.unwrap_or(0)].iter().rev().find(|other| other.depth < row.depth) {
                actions.push(Action::Select(parent.key.clone()));
                self.reveal = true;
            }
        }
        if right {
            if row.has_children && !row.expanded {
                actions.push(Action::Toggle(row.key.clone(), true));
            } else if let Some(child) = rows.get(at.unwrap_or(0) + 1).filter(|child| child.depth > row.depth) {
                actions.push(Action::Select(child.key.clone()));
                self.reveal = true;
            }
        }
        if f4 && row.kind == "connection" {
            if let Some(id) = &row.key.connection {
                actions.push(Action::Edit(id.clone()));
            }
        } else if f4 && is_schema(&row) {
            actions.push(Action::Open(row.key.clone(), None));
        } else if enter || f4 {
            if row.kind == "connection" && !connected(&row) {
                actions.push(Action::Toggle(row.key.clone(), true));
            } else if openable(&row) {
                actions.push(open_action(&row));
            } else if row.has_children {
                actions.push(Action::Toggle(row.key.clone(), !row.expanded));
            }
        }
        if f5 || refresh {
            actions.push(Action::Command("db.navigator.refresh", row_params(&row.key)));
        }
        if usages && has_usages(&row) {
            actions.push(Action::FindUsages(row.key.clone()));
        }
        if rename && renamable(&row) {
            actions.push(Action::Rename(row.key.clone()));
        }
        if delete && deletable(&row) {
            actions.push(Action::Delete(row.key.clone()));
        }
    }

    fn run(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, actions: Vec<Action>) {
        if actions.is_empty() {
            return;
        }
        self.read = None;
        for action in actions {
            let result = match action {
                Action::Command(command, params) => registry.dispatch(session, command, params).map(|_| ()),
                Action::Select(key) => registry.dispatch(session, "db.navigator.select", row_params(&key)).map(|_| ()),
                Action::Toggle(key, open) => {
                    let command = if open { "db.navigator.expand" } else { "db.navigator.collapse" };
                    let result = registry.dispatch(session, command, row_params(&key)).map(|_| ());
                    if let Err(zen_core::DispatchError::Failed { error, .. }) = &result
                        && error.code.as_deref() == Some("passwordNeeded")
                        && let Some(id) = &key.connection
                    {
                        open_dialog(session, json!({ "id": id }));
                    }
                    result
                }
                Action::Open(key, page) => {
                    let mut params = row_params(&key);
                    if let Some(page) = page {
                        params["page"] = Value::from(page);
                    }
                    registry.dispatch(session, "db.object.open", params).map(|_| ())
                }
                Action::Edit(id) => {
                    open_dialog(session, json!({ "id": id }));
                    Ok(())
                }
                Action::NewConnection(driver) => {
                    open_dialog(session, driver.map_or_else(|| json!({}), |driver| json!({ "driver": driver })));
                    Ok(())
                }
                Action::Copy(key, qualified) => {
                    let mut params = row_params(&key);
                    params["qualified"] = Value::Bool(qualified);
                    registry.dispatch(session, "db.object.copyName", params).map(|answer| {
                        if let Some(name) = answer["name"].as_str() {
                            ui.ctx().copy_text(name.to_owned());
                        }
                    })
                }
                Action::Console(connection, text) => registry
                    .dispatch(session, "db.console.new", json!({ "connection": connection, "text": text }))
                    .map(|_| ()),
                Action::Generate(key, kind) => self.generate(registry, session, &key, kind),
                Action::Code(key, language) => {
                    let mut params = row_params(&key);
                    params["language"] = Value::from(language);
                    registry.dispatch(session, "db.object.generateCode", params).map(|answer| {
                        let waiting = self.code.take().filter(|code| code.0 == key && code.1 == language);
                        if let Some(code) = answer["code"].as_str() {
                            ui.ctx().copy_text(code.to_owned());
                        } else if answer["loading"].as_bool() == Some(true) {
                            let now = Instant::now();
                            let until = waiting.map_or(now + CODE_WAIT, |code| code.2);
                            self.code = Some((key, language, until, now + CODE_RETRY));
                        }
                    })
                }
                Action::ClientCommand(connection) => registry
                    .dispatch(session, "db.connection.clientCommand", json!({ "connection": connection }))
                    .map(|answer| {
                        if let Some(line) = answer["commandLine"].as_str() {
                            ui.ctx().copy_text(line.to_owned());
                        }
                    }),
                Action::Filter(text) => {
                    registry.dispatch(session, "db.navigator.filter", json!({ "text": text })).map(|_| ())
                }
                Action::Export(key) => {
                    super::export_dialog::request(
                        session,
                        json!({ "connection": key.connection.clone().unwrap_or_default(), "path": key.path }),
                    );
                    Ok(())
                }
                Action::Import(key) => {
                    super::csv_import_dialog::request(
                        session,
                        json!({ "connection": key.connection.clone().unwrap_or_default(), "path": key.path }),
                    );
                    Ok(())
                }
                Action::Backup(key, restore) => {
                    let mode =
                        if restore { super::backup_dialog::Mode::Restore } else { super::backup_dialog::Mode::Backup };
                    super::backup_dialog::request(
                        session,
                        mode,
                        json!({ "connection": key.connection.clone().unwrap_or_default(), "path": key.path }),
                    );
                    Ok(())
                }
                Action::Transfer(key) => {
                    super::transfer_dialog::request(
                        session,
                        json!({ "connection": key.connection.clone().unwrap_or_default(), "path": key.path }),
                    );
                    Ok(())
                }
                Action::Collapse(all) => {
                    registry.dispatch(session, "db.navigator.collapse", json!({ "all": all })).map(|_| ())
                }
                Action::Scripts(connection) => {
                    let listed = registry.dispatch(
                        session,
                        "db.script.list",
                        json!({ "connection": connection, "limit": SCRIPTS }),
                    );
                    let scripts = listed.as_ref().ok().and_then(|listed| listed["scripts"].as_array().cloned());
                    self.scripts = (connection, scripts.unwrap_or_default());
                    listed.map(|_| ())
                }
                Action::OpenScript(path) => registry
                    .dispatch(session, "workspace.tab.open", json!({ "workspace": WORKSPACE, "path": path }))
                    .map(|_| ()),
                Action::NewScript(connection) => new_script(registry, session, connection),
                Action::FindUsages(key) => registry
                    .dispatch(
                        session,
                        "db.usages.find",
                        json!({ "connection": key.connection.clone().unwrap_or_default(), "path": key.path }),
                    )
                    .map(|_| ()),
                Action::Rename(key) => {
                    let name = self
                        .rows
                        .iter()
                        .find(|row| row.key == key)
                        .map(|row| row.label.split('(').next().unwrap_or_default().trim().to_owned())
                        .unwrap_or_default();
                    self.dialog = Some(Dialog::Rename { key, name, error: None, focus: true });
                    Ok(())
                }
                Action::Delete(key) => self.ask_delete(registry, session, key, "db.object.drop", false),
                Action::Truncate(key) => self.ask_delete(registry, session, key, "db.object.truncate", false),
            };
            self.error = result.err().filter(|error| !super::transaction_dialog::ask(session, error)).map(message);
        }
        ui.ctx().request_repaint();
    }

    fn label_of(&self, key: &RowKey) -> String {
        self.rows.iter().find(|row| row.key == *key).map(|row| row.label.clone()).unwrap_or_default()
    }

    fn watch(&mut self, key: &RowKey) {
        if let Some(connection) = &key.connection
            && !self.watching.contains(connection)
        {
            self.watching.push(connection.clone());
        }
    }

    fn follow_runs(&mut self, ui: &Ui, session: &Session) {
        let mut still = Vec::new();
        for connection in std::mem::take(&mut self.watching) {
            let key = zen_workbench::database::alter::navigator_key(&connection);
            let Some(alterations) = session.state::<zen_workbench::database::alter::Alterations>() else { continue };
            if alterations.running(&key) {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
                still.push(connection);
            } else if let Some(outcome) = alterations.outcome(&key)
                && outcome.status != zen_db::engine::OperationStatus::Done
            {
                self.error =
                    Some(outcome.message.clone().unwrap_or_else(|| t!("ide-database-navigator-change-failed")));
            }
        }
        self.watching = still;
    }

    fn ask_delete(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        key: RowKey,
        command: &'static str,
        cascade: bool,
    ) -> Result<(), zen_core::DispatchError> {
        let name = self.label_of(&key);
        let truncate = command == "db.object.truncate";
        let title = if truncate {
            t!("ide-database-truncate-title", name = name.as_str())
        } else {
            t!("ide-database-delete-title", name = name.as_str())
        };
        if key.path.is_empty() {
            self.dialog = Some(Dialog::Delete(Deletion {
                command: "db.dataSource.remove",
                key,
                title,
                loss: t!("ide-database-delete-connection-loss", name = name.as_str()),
                statements: Vec::new(),
                can_cascade: false,
                cascade: false,
                token: None,
                error: None,
            }));
            return Ok(());
        }
        let mut params = row_params(&key);
        params["cascade"] = Value::Bool(cascade);
        match registry.dispatch(session, command, params) {
            Ok(_) => {
                self.watch(&key);
                Ok(())
            }
            Err(zen_core::DispatchError::Failed { error, .. })
                if error.code.as_deref() == Some(zen_workbench::database::safety::CONFIRM_EXECUTE) =>
            {
                let data = Value::Object(error.data.clone().unwrap_or_default());
                let statements = data["statements"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|statement| statement["text"].as_str().map(str::to_owned))
                    .collect();
                self.dialog = Some(Dialog::Delete(Deletion {
                    command,
                    key,
                    title,
                    loss: data["loss"].as_str().map_or_else(|| error.message.clone(), str::to_owned),
                    statements,
                    can_cascade: data["cascade"] == true,
                    cascade,
                    token: data["token"].as_str().map(str::to_owned),
                    error: None,
                }));
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    fn confirm_delete(&mut self, registry: &Registry, session: &mut Session, deletion: Deletion) {
        let params = if deletion.command == "db.dataSource.remove" {
            json!({ "id": deletion.key.connection })
        } else {
            let mut params = row_params(&deletion.key);
            params["cascade"] = Value::Bool(deletion.cascade);
            params["confirmed"] = json!(deletion.token);
            params
        };
        match registry.dispatch(session, deletion.command, params) {
            Ok(_) => {
                if deletion.command != "db.dataSource.remove" {
                    self.watch(&deletion.key);
                }
                self.dialog = None;
            }
            Err(error) => {
                self.dialog = Some(Dialog::Delete(Deletion { error: Some(message(error)), ..deletion }));
            }
        }
        self.read = None;
    }

    fn confirm_rename(&mut self, registry: &Registry, session: &mut Session, key: RowKey, name: String) {
        let answer = if key.path.is_empty() {
            registry.dispatch(session, "db.dataSource.update", json!({ "id": key.connection, "name": name.trim() }))
        } else {
            let mut params = row_params(&key);
            params["to"] = Value::from(name.trim());
            registry.dispatch(session, "db.object.rename", params)
        };
        match answer {
            Ok(_) => {
                if !key.path.is_empty() {
                    self.watch(&key);
                }
                self.dialog = None;
            }
            Err(error) if super::transaction_dialog::ask(session, &error) => self.dialog = None,
            Err(error) => {
                self.dialog = Some(Dialog::Rename { key, name, error: Some(message(error)), focus: false });
            }
        }
        self.read = None;
    }

    fn dialogs(&mut self, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut kept = Some(dialog.clone());
        let mut confirmed = false;
        let mut recascade = None;
        let modal = egui::Modal::new(ui.scope_id().with("navigator-dialog")).frame(super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let wrap = |ui: &mut Ui, text: &str, color: Color32, font: egui::FontId| {
                    ui.add(egui::Label::new(egui::RichText::new(text).font(font).color(color)).wrap());
                };
                match kept.as_mut() {
                    Some(Dialog::Rename { name, error, focus, key }) => {
                        let title = if key.path.is_empty() {
                            t!("ide-database-rename-connection-title")
                        } else {
                            t!("ide-database-rename-title")
                        };
                        wrap(ui, &title, p.text, theme::semibold(15.0));
                        ui.add_space(12.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(name)
                                .id_salt("database-navigator-rename")
                                .font(theme::regular(13.0))
                                .desired_width(f32::INFINITY),
                        );
                        accessibility::labelled(&field, &title);
                        if std::mem::take(focus) {
                            field.request_focus();
                        }
                        if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                            confirmed = true;
                        }
                        if let Some(error) = error {
                            ui.add_space(8.0);
                            wrap(ui, error, p.error_text, theme::regular(13.0));
                        }
                        ui.add_space(18.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                                kept = None;
                            }
                            if super::vcs::primary_button(ui, p, &t!("ide-database-rename-action")).clicked() {
                                confirmed = true;
                            }
                        });
                    }
                    Some(Dialog::Delete(deletion)) => {
                        wrap(ui, &deletion.title, p.text, theme::semibold(15.0));
                        ui.add_space(10.0);
                        wrap(ui, &deletion.loss, p.text, theme::regular(13.0));
                        if !deletion.statements.is_empty() {
                            ui.add_space(10.0);
                            egui::Frame::new()
                                .fill(p.island)
                                .corner_radius(6)
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    egui::ScrollArea::vertical().max_height(140.0).auto_shrink([false, true]).show(
                                        ui,
                                        |ui| {
                                            let text = deletion.statements.join(";\n");
                                            wrap(ui, &text, p.text, egui::FontId::monospace(12.5));
                                        },
                                    );
                                });
                        }
                        if deletion.can_cascade {
                            ui.add_space(10.0);
                            let mut cascade = deletion.cascade;
                            let label = egui::RichText::new(t!("ide-database-cascade")).font(theme::regular(13.0));
                            if ui.checkbox(&mut cascade, label).changed() {
                                recascade = Some(cascade);
                            }
                        }
                        if let Some(error) = &deletion.error {
                            ui.add_space(8.0);
                            wrap(ui, error, p.error_text, theme::regular(13.0));
                        }
                        ui.add_space(18.0);
                        let action = if deletion.command == "db.object.truncate" {
                            t!("ide-database-truncate-action")
                        } else {
                            t!("ide-database-delete-action")
                        };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                                kept = None;
                            }
                            if super::execute_dialog::danger_button(ui, p, &action, true).clicked() {
                                confirmed = true;
                            }
                        });
                    }
                    None => {}
                }
                if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
                    kept = None;
                }
            },
        );
        if modal.should_close() {
            kept = None;
        }
        match (kept, confirmed, recascade) {
            (Some(Dialog::Delete(deletion)), _, Some(cascade)) => {
                let (key, command) = (deletion.key.clone(), deletion.command);
                if let Err(error) = self.ask_delete(registry, session, key, command, cascade) {
                    self.dialog = Some(Dialog::Delete(Deletion { error: Some(message(error)), cascade, ..deletion }));
                }
            }
            (Some(Dialog::Delete(deletion)), true, None) => self.confirm_delete(registry, session, deletion),
            (Some(Dialog::Rename { key, name, .. }), true, _) => self.confirm_rename(registry, session, key, name),
            (kept, _, _) => self.dialog = kept,
        }
    }

    fn generate(
        &self,
        registry: &Registry,
        session: &mut Session,
        key: &RowKey,
        kind: Generate,
    ) -> Result<(), zen_core::DispatchError> {
        let connection = key.connection.clone().unwrap_or_default();
        let text = match kind {
            Generate::Ddl => {
                let answer = registry.dispatch(session, "db.object.ddl", row_params(key))?;
                answer["ddl"].as_str().map(str::to_owned).unwrap_or_default()
            }
            Generate::Select | Generate::Insert => {
                let object = registry.dispatch(session, "db.object.get", row_params(key))?;
                let name = object["qualified"].as_str().unwrap_or_default().to_owned();
                let driver = object["driver"].as_str().unwrap_or_default();
                let columns: Vec<String> = object["object"]["columns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|column| column["name"].as_str().map(|name| zen_db::schema::quote_for(driver, name)))
                    .collect();
                if kind == Generate::Select {
                    let list = if columns.is_empty() { "*".to_owned() } else { columns.join(", ") };
                    format!("SELECT {list}\nFROM {name};\n")
                } else {
                    let values = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                    format!("INSERT INTO {name} ({})\nVALUES ({values});\n", columns.join(", "))
                }
            }
        };
        registry.dispatch(session, "db.console.new", json!({ "connection": connection, "text": text })).map(|_| ())
    }
}

fn tool_button(
    ui: &mut Ui,
    p: &IdePalette,
    (x, top, size, limit): (&mut f32, f32, f32, f32),
    salt: &str,
    glyph: Glyph,
    name: String,
    enabled: bool,
) -> bool {
    if *x + size > limit {
        return false;
    }
    let rect = Rect::from_min_size(pos2(*x, top), Vec2::splat(size));
    *x += size + 2.0;
    let response = ui.interact(rect, ui.scope_id().with(salt), if enabled { Sense::click() } else { Sense::hover() });
    if enabled {
        hover(ui, &response, p);
    }
    hint(ui, &response, &name, None, Side::Below);
    if !enabled {
        accessibility::disabled(&response);
    }
    glyph(ui.painter(), rect.shrink(5.0), if enabled { p.text_secondary } else { p.gutter_text });
    response.clicked() && enabled
}

fn open_dialog(session: &mut Session, params: Value) {
    session.state_mut::<super::connection_dialog::ConnectionDialogRequest>().0 = Some(params);
}

fn paint_row(ui: &Ui, p: &IdePalette, rect: Rect, row: &Row, focused: bool, hovered: bool) -> Option<Rect> {
    let highlight = Rect::from_min_max(pos2(rect.left() + 4.0, rect.top()), pos2(rect.right() - 4.0, rect.bottom()));
    let painter = ui.painter().with_clip_rect(highlight.intersect(ui.clip_rect()));
    if let Some(color) =
        row.color.as_deref().and_then(|color| type_color(p, color)).filter(|_| row.kind == "connection")
    {
        painter.rect_filled(highlight, 5, color.gamma_multiply(0.14));
    }
    if row.selected {
        painter.rect_filled(highlight, 5, if focused { p.selection } else { p.hover });
    } else if hovered {
        painter.rect_filled(highlight, 5, p.hover);
    }
    let x = highlight.left() + 4.0 + row.depth as f32 * metrics::TREE_INDENT;
    let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
    if row.has_children {
        if row.expanded {
            crate::icons::chevron_down(&painter, chevron, p.text_secondary);
        } else {
            crate::icons::chevron_right(&painter, chevron, p.text_secondary);
        }
    }
    let icon_rect = Rect::from_center_size(pos2(x + metrics::TREE_ICON - 4.0, rect.center().y), Vec2::splat(14.0));
    let dim = row.system || matches!(row.kind.as_str(), "loading" | "message");
    if let Some(glyph) = icon(&row.kind) {
        let color = if dim { p.text_secondary } else { icon_color(p, &row.kind) };
        glyph(&painter, icon_rect, color);
    }
    if row.kind == "connection" && !connected(row) {
        painter.circle_filled(icon_rect.right_bottom() - vec2(1.0, 1.0), 2.5, p.island);
    } else if row.kind == "connection" {
        painter.circle_filled(icon_rect.right_bottom() - vec2(1.0, 1.0), 2.5, p.vcs_added);
    }
    if row.read_only {
        glyph_lock(
            &painter,
            Rect::from_min_size(icon_rect.right_top() - vec2(4.0, 3.0), Vec2::splat(8.0)),
            p.text_secondary,
        );
    }
    if row.error.is_some() {
        painter.circle_filled(icon_rect.left_bottom() + vec2(2.0, -2.0), 3.0, p.error_text);
    }
    let text_x = x + metrics::TREE_TEXT - 2.0;
    let room = highlight.right() - 12.0 - SIZE_BOX - text_x >= SIZE_ROOM;
    let size = row.size.filter(|_| row.size_ratio.is_some() && room).map(|bytes| {
        let size =
            Rect::from_min_size(pos2(highlight.right() - 6.0 - SIZE_BOX, rect.center().y - 8.0), vec2(SIZE_BOX, 16.0));
        (size, bytes)
    });
    let limit = size.map_or(highlight.right() - 6.0, |(size, _)| size.left() - 6.0);
    let font = if row.kind == "loading" { egui::FontId::proportional(13.0) } else { theme::regular(13.0) };
    let label_color = if dim { p.text_secondary } else { p.text };
    let job = super::vcs::truncated(&row.label, font, label_color, (limit - text_x).max(1.0));
    let galley = painter.layout_job(job);
    let label_width = galley.size().x;
    painter.galley(pos2(text_x, rect.center().y - galley.size().y / 2.0), galley, label_color);
    let mut after = text_x + label_width + 6.0;
    let detail = match (&row.detail, row.kind.as_str()) {
        (Some(detail), "column") => Some(format!("({detail})")),
        (detail, _) => detail.clone(),
    };
    for (text, color) in
        [(detail, p.text_secondary), (row.comment.clone().map(|comment| format!("– {comment}")), p.comment)]
    {
        let Some(text) = text else { continue };
        if after + 12.0 >= limit {
            break;
        }
        let job = super::vcs::truncated(&text, theme::regular(12.0), color, limit - after);
        let galley = painter.layout_job(job);
        let width = galley.size().x;
        painter.galley(pos2(after, rect.center().y - galley.size().y / 2.0), galley, color);
        after += width + 6.0;
    }
    let (size, bytes) = size?;
    painter.rect_filled(size, 3, p.hover);
    let ratio = row.size_ratio.unwrap_or(0.0).clamp(0.0, 1.0);
    let filled = Rect::from_min_size(size.min, vec2((SIZE_BOX * ratio).max(1.0), size.height()));
    painter.rect_filled(filled, 3, p.accent.gamma_multiply(0.28));
    painter.text(
        pos2(size.right() - 4.0, size.center().y),
        Align2::RIGHT_CENTER,
        short_size(bytes),
        theme::regular(11.0),
        p.text_secondary,
    );
    Some(size)
}

fn size_tip(row: &Row) -> String {
    let size = zen_workbench::database::objects::size_text(row.size.unwrap_or(0));
    match row.rows.filter(|rows| *rows >= 0) {
        Some(rows) => t!("ide-database-navigator-size-rows", size = size, rows = rows),
        None => t!("ide-database-navigator-size", size = size),
    }
}

fn new_script(registry: &Registry, session: &mut Session, connection: String) -> Result<(), zen_core::DispatchError> {
    let project =
        session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project).map(Path::to_path_buf);
    let mut dialog = rfd::FileDialog::new().set_title(t!("ide-database-new-script-title")).set_file_name("script.sql");
    if let Some(project) = &project {
        dialog = dialog.set_directory(project);
    }
    let Some(chosen) = dialog.save_file() else { return Ok(()) };
    let name = chosen.file_name().map(|name| name.to_string_lossy().into_owned());
    let folder = chosen.parent().map(Path::to_path_buf);
    registry
        .dispatch(session, "db.script.new", json!({ "connection": connection, "folder": folder, "name": name }))
        .map(|_| ())
}

fn scripts_menu(ui: &mut Ui, id: &str, scripts: &[Value]) -> Option<Action> {
    let mut chosen = None;
    if ui.button(t!("ide-database-open-console")).clicked() {
        chosen = Some(Action::Console(id.to_owned(), None));
        ui.close();
    }
    if ui.button(t!("ide-database-new-script")).clicked() {
        chosen = Some(Action::NewScript(id.to_owned()));
        ui.close();
    }
    ui.menu_button(t!("ide-database-scripts"), |ui| {
        ui.set_min_width(240.0);
        if scripts.is_empty() {
            ui.add_enabled(false, egui::Button::new(t!("ide-database-no-scripts")));
        }
        for script in scripts {
            let (Some(name), Some(path)) = (script["name"].as_str(), script["path"].as_str()) else { continue };
            let place = script["place"].as_str().unwrap_or_default();
            let button = egui::Button::new(name).right_text(egui::RichText::new(place).weak());
            if ui.add_enabled(script["exists"] != false, button).clicked() {
                chosen = Some(Action::OpenScript(PathBuf::from(path)));
                ui.close();
            }
        }
    });
    chosen
}

fn scripts_of<'a>(scripts: &'a (String, Vec<Value>), connection: Option<&str>) -> &'a [Value] {
    match connection {
        Some(id) if scripts.0 == id => &scripts.1,
        _ => &[],
    }
}

fn edit_items(ui: &mut Ui, row: &Row, actions: &mut Vec<Action>) {
    for (shown, label, shortcut, action) in [
        (renamable(row), t!("ide-database-rename"), "F2", Action::Rename(row.key.clone())),
        (deletable(row), t!("ide-database-delete"), "⌦", Action::Delete(row.key.clone())),
    ] {
        if shown && ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked() {
            actions.push(action);
            ui.close();
        }
    }
}

fn create_menu(ui: &mut Ui, row: &Row, id: &str, actions: &mut Vec<Action>) {
    let path = &row.key.path;
    let in_table = path.len() >= 4 && path[2] == "tables" && !matches!(row.kind.as_str(), "folder") || truncatable(row);
    let mut kinds = vec![
        ("table", t!("ide-database-create-table")),
        ("view", t!("ide-database-create-view")),
        ("sequence", t!("ide-database-create-sequence")),
        ("function", t!("ide-database-create-function")),
        ("schema", t!("ide-database-create-schema")),
    ];
    if in_table {
        kinds.splice(0..0, [("column", t!("ide-database-create-column")), ("index", t!("ide-database-create-index"))]);
    }
    ui.menu_button(t!("ide-database-create"), |ui| {
        ui.set_min_width(180.0);
        for (kind, label) in kinds {
            if ui.button(label).clicked() {
                let params = json!({ "connection": id, "path": path, "kind": kind });
                actions.push(Action::Command("db.navigator.create", params));
                ui.close();
            }
        }
    });
}

fn row_menu(ui: &mut Ui, row: &Row, scripts: &[Value], actions: &mut Vec<Action>) {
    ui.set_min_width(220.0);
    let mut later = Vec::new();
    let mut item = |ui: &mut Ui, enabled: bool, label: String, action: Action| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            later.push(action);
            ui.close();
        }
    };
    let key = row.key.clone();
    let connection = key.connection.clone();
    if row.kind == "connectionFolder" {
        customization::popup(
            ui,
            customization::DATABASE_FOLDER_POPUP,
            |_| true,
            |ui, slot| {
                let action = match slot {
                    "ide-database-new-connection" => Action::NewConnection(None),
                    _ => Action::Command("db.navigator.refresh", row_params(&key)),
                };
                item(ui, true, t!(slot), action);
            },
        );
        actions.extend(later);
        return;
    }
    let Some(id) = connection else { return };
    let mut scripted = None;
    let editable = renamable(row) || deletable(row);
    if row.kind == "connection" {
        let shown = |slot: &str| match slot {
            "ide-dbsearch-menu" => !key_value(row),
            "ide-database-tools" => backs_up(row.driver.as_deref()),
            customization::EDIT_ITEMS => editable,
            _ => true,
        };
        customization::popup(ui, customization::DATABASE_CONNECTION_POPUP, shown, |ui, slot| match slot {
            "ide-database-sql-editor" => {
                let chosen = ui.menu_button(t!("ide-database-sql-editor"), |ui| scripts_menu(ui, &id, scripts));
                scripted = scripted.take().or(chosen.inner.flatten());
            }
            "ide-dbsearch-menu" => {
                item(ui, true, t!("ide-dbsearch-menu"), Action::Command("db.search.open", json!({ "connection": id })));
            }
            "ide-database-edit-connection" => {
                item(ui, true, t!("ide-database-edit-connection"), Action::Edit(id.clone()));
            }
            "ide-database-duplicate-connection" => item(
                ui,
                true,
                t!("ide-database-duplicate-connection"),
                Action::Command("db.dataSource.duplicate", json!({ "id": id })),
            ),
            "ide-database-connect" => {
                item(ui, !connected(row), t!("ide-database-connect"), Action::Toggle(key.clone(), true));
            }
            "ide-database-reconnect" => item(
                ui,
                connected(row),
                t!("ide-database-reconnect"),
                Action::Command("db.connection.invalidate", json!({ "connection": id })),
            ),
            "ide-database-disconnect" => item(
                ui,
                connected(row),
                t!("ide-database-disconnect"),
                Action::Command("db.connection.disconnect", json!({ "connection": id })),
            ),
            "ide-database-sessions" => {
                item(
                    ui,
                    true,
                    t!("ide-database-sessions"),
                    Action::Command("db.session.open", json!({ "connection": id })),
                );
            }
            "ide-database-migrations" => item(
                ui,
                true,
                t!("ide-database-migrations"),
                Action::Command("db.migration.open", json!({ "connection": id })),
            ),
            "ide-database-open-terminal" => item(
                ui,
                true,
                t!("ide-database-open-terminal"),
                Action::Command("db.connection.openTerminal", json!({ "connection": id })),
            ),
            "ide-database-copy-client-command" => {
                item(ui, true, t!("ide-database-copy-client-command"), Action::ClientCommand(id.clone()));
            }
            "ide-database-tools" => {
                ui.menu_button(t!("ide-database-tools"), |ui| {
                    item(ui, true, t!("ide-database-backup"), Action::Backup(key.clone(), false));
                    item(ui, true, t!("ide-database-restore"), Action::Backup(key.clone(), true));
                });
            }
            "ide-database-copy-name" => item(ui, true, t!("ide-database-copy-name"), Action::Copy(key.clone(), false)),
            customization::EDIT_ITEMS => edit_items(ui, row, actions),
            "ide-database-refresh" => {
                item(ui, true, t!("ide-database-refresh"), Action::Command("db.navigator.refresh", row_params(&key)));
            }
            _ => {}
        });
        actions.extend(later);
        actions.extend(scripted);
        return;
    }
    if matches!(row.kind.as_str(), "loading" | "message" | "more") {
        return;
    }
    let table_like =
        matches!(row.kind.as_str(), "table" | "foreignTable" | "view" | "materializedView") && key.path.len() == 4;
    let own_tree = openable(row) && row.kind != "key" && key.path.len() == 3;
    let tables_folder = row.kind == "folder" && key.path.len() == 3 && !key_value(row);
    let tables_only = key.path.len() != 3 || key.path.get(2).is_some_and(|folder| folder == "tables");
    let backup_tools = (is_schema(row) || row.kind == "table" && key.path.len() == 4) && !key_value(row);
    let shown = |slot: &str| match slot {
        "ide-database-view-data" => table_like || own_tree,
        "ide-database-open-object" => {
            openable(row) && !key_value(row) && key.path.len() == 4 || is_schema(row) || leaf_item(row)
        }
        "ide-database-view-diagram" => (table_like || is_schema(row)) && !key_value(row),
        "ide-database-open-value" => row.kind == "key",
        customization::CREATE_ITEMS => in_schema(row) && !key_value(row) && !leaf_item(row),
        "ide-database-generate-sql" => {
            key.path.len() >= 4
                && !key_value(row)
                && !matches!(row.kind.as_str(), "folder" | "column" | "reference" | "policy" | "dictionary")
                && !leaf_item(row)
        }
        "ide-database-generate-code" => table_like,
        "ide-database-find-usages" => has_usages(row),
        "ide-database-export-data" => table_like || tables_folder || is_schema(row),
        "ide-database-import-data" | "ide-database-transfer-data" => {
            row.kind == "table" || (tables_folder && tables_only) || is_schema(row)
        }
        "ide-database-tools" => backup_tools || truncatable(row),
        "ide-dbsearch-menu" => is_schema(row) && !key_value(row),
        "ide-database-copy-name" | "ide-database-copy-qualified-name" => row.kind != "folder",
        customization::EDIT_ITEMS => editable,
        _ => true,
    };
    customization::popup(ui, customization::DATABASE_OBJECT_POPUP, shown, |ui, slot| match slot {
        "ide-database-view-data" => {
            item(ui, true, t!("ide-database-view-data"), Action::Open(key.clone(), Some("data")))
        }
        "ide-database-open-object" => {
            item(ui, true, t!("ide-database-open-object"), Action::Open(key.clone(), Some("properties")));
        }
        "ide-database-view-diagram" => {
            let params = json!({ "connection": id, "path": key.path });
            item(ui, true, t!("ide-database-view-diagram"), Action::Command("db.diagram.open", params));
        }
        "ide-database-open-value" => item(ui, true, t!("ide-database-open-value"), Action::Open(key.clone(), None)),
        "ide-database-sql-editor" => {
            let chosen = ui.menu_button(t!("ide-database-sql-editor"), |ui| scripts_menu(ui, &id, scripts));
            scripted = scripted.take().or(chosen.inner.flatten());
        }
        customization::CREATE_ITEMS => create_menu(ui, row, &id, actions),
        "ide-database-generate-sql" => {
            ui.menu_button(t!("ide-database-generate-sql"), |ui| {
                item(ui, true, "DDL".to_owned(), Action::Generate(key.clone(), Generate::Ddl));
                if table_like {
                    item(ui, true, "SELECT".to_owned(), Action::Generate(key.clone(), Generate::Select));
                    item(ui, row.kind == "table", "INSERT".to_owned(), Action::Generate(key.clone(), Generate::Insert));
                }
            });
        }
        "ide-database-generate-code" => {
            ui.menu_button(t!("ide-database-generate-code"), |ui| {
                item(ui, true, t!("ide-database-code-go"), Action::Code(key.clone(), "go"));
                item(ui, true, t!("ide-database-code-rust"), Action::Code(key.clone(), "rust"));
                item(ui, true, t!("ide-database-code-typescript"), Action::Code(key.clone(), "typescript"));
            });
        }
        "ide-database-find-usages" => item(ui, true, t!("ide-database-find-usages"), Action::FindUsages(key.clone())),
        "ide-database-export-data" => item(ui, true, t!("ide-database-export-data"), Action::Export(key.clone())),
        "ide-database-import-data" => item(ui, true, t!("ide-database-import-data"), Action::Import(key.clone())),
        "ide-database-transfer-data" => item(ui, true, t!("ide-database-transfer-data"), Action::Transfer(key.clone())),
        "ide-database-tools" => {
            if backup_tools {
                ui.menu_button(t!("ide-database-tools"), |ui| {
                    item(ui, true, t!("ide-database-backup"), Action::Backup(key.clone(), false));
                    item(ui, true, t!("ide-database-restore"), Action::Backup(key.clone(), true));
                });
            }
            if truncatable(row) {
                ui.menu_button(t!("ide-database-tools"), |ui| {
                    item(ui, true, t!("ide-database-truncate"), Action::Truncate(key.clone()));
                });
            }
        }
        "ide-dbsearch-menu" => {
            let params = json!({ "connection": id, "schema": key.path[1] });
            item(ui, true, t!("ide-dbsearch-menu"), Action::Command("db.search.open", params));
        }
        "ide-database-copy-name" => item(ui, true, t!("ide-database-copy-name"), Action::Copy(key.clone(), false)),
        "ide-database-copy-qualified-name" => item(
            ui,
            key.path.len() >= 2 && !key_value(row),
            t!("ide-database-copy-qualified-name"),
            Action::Copy(key.clone(), true),
        ),
        customization::EDIT_ITEMS => edit_items(ui, row, actions),
        "ide-database-refresh" => {
            item(ui, true, t!("ide-database-refresh"), Action::Command("db.navigator.refresh", row_params(&key)));
        }
        _ => {}
    });
    actions.extend(later);
    actions.extend(scripted);
}

#[cfg(test)]
mod tests;
