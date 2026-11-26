use std::ops::Range;

use egui::{Align2, CursorIcon, FontId, Id, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use zen_i18n::t;

use super::{IdePalette, customization, theme};
use crate::accessibility;

const ROW: f32 = 22.0;
const DENSE_ROW: f32 = 18.0;
const HEADER: f32 = 26.0;
const BAR: f32 = 10.0;
const MIN_WIDTH: f32 = 40.0;
const MAX_FIRST_WIDTH: f32 = 320.0;
const PADDING: f32 = 6.0;
const ROWS_AROUND: u64 = 40;
const COLUMNS_AROUND: usize = 6;
const SAMPLED_ROWS: u64 = 40;
const LINK_ZONE: f32 = 18.0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GridColumn {
    pub(super) name: String,
    pub(super) type_name: String,
    pub(super) sort: Option<(bool, usize)>,
    pub(super) editable: bool,
    pub(super) filtered: bool,
    pub(super) not_null: bool,
    pub(super) primary: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum GridRowMark {
    #[default]
    Clean,
    Changed,
    Inserted,
    Deleted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GridFeatures {
    pub(super) editable: bool,
    pub(super) filterable: bool,
    pub(super) sortable: bool,
    pub(super) generate: bool,
    pub(super) clipboard: bool,
    pub(super) result: bool,
    pub(super) value_panel: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum GridAction {
    Commit { row: u64, column: usize, text: String },
    Revert { row: u64, column: usize },
    SetNull { row: u64, column: usize },
    SetDefault { row: u64, column: usize },
    CopyAbove { row: u64 },
    AddRow,
    Duplicate { row: u64 },
    Delete { rows: Vec<u64> },
    Restore { rows: Vec<u64> },
    Filter { row: u64, column: usize, operator: &'static str },
    FilterIn { rows: Vec<u64>, column: usize },
    FilterClipboard { column: usize, operator: &'static str },
    FilterCustom { column: usize },
    FilterValues { column: usize },
    ResetColumn { column: usize },
    Generate { kind: &'static str, rows: Range<u64> },
    Save,
    Preview,
    CopyAs { format: &'static str, rows: Range<u64>, columns: Range<usize> },
    CopyAdvanced { rows: Range<u64>, columns: Range<usize> },
    Paste { row: u64, column: usize, text: Option<String>, insert: bool },
    PasteAdvanced { row: u64, column: usize },
    FollowKey { row: u64, column: usize },
    OpenReferences { row: u64, reference: usize },
    Lookup { row: u64, column: usize },
    SortBy { column: usize, direction: &'static str },
    GoToRow,
    FetchNext,
    FetchAll,
    ToggleValuePanel,
    View { mode: &'static str },
    Export,
}

struct CellEditor {
    row: u64,
    column: usize,
    text: String,
    focused: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GridCell {
    pub(super) text: String,
    pub(super) null: bool,
    pub(super) number: bool,
    pub(super) boolean: Option<bool>,
    pub(super) cut: bool,
    pub(super) changed: bool,
    pub(super) default: bool,
}

pub(super) trait GridSource {
    fn columns(&self) -> &[GridColumn];
    fn row_count(&self) -> u64;
    fn revision(&self) -> u64;
    fn cells(&self, rows: Range<u64>, columns: Range<usize>) -> Vec<Vec<GridCell>>;
    fn marks(&self, _rows: Range<u64>) -> Vec<GridRowMark> {
        Vec::new()
    }
    fn full_text(&self, _row: u64, _column: usize) -> Option<String> {
        None
    }
    fn link(&self, _column: usize) -> Option<&str> {
        None
    }
    fn references(&self) -> &[String] {
        &[]
    }
    fn refusal(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionKind {
    Cells,
    Rows,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GridSelection {
    pub(super) anchor: (u64, usize),
    pub(super) head: (u64, usize),
    pub(super) kind: SelectionKind,
}

impl GridSelection {
    pub(super) fn bounds(&self, rows: u64, columns: usize) -> (Range<u64>, Range<usize>) {
        let (top, bottom) = (self.anchor.0.min(self.head.0), self.anchor.0.max(self.head.0));
        let (left, right) = (self.anchor.1.min(self.head.1), self.anchor.1.max(self.head.1));
        match self.kind {
            SelectionKind::Cells => (top..bottom + 1, left..right + 1),
            SelectionKind::Rows => (top..bottom + 1, 0..columns),
            SelectionKind::Columns => (0..rows, left..right + 1),
        }
    }

    fn contains(&self, row: u64, column: usize, rows: u64, columns: usize) -> bool {
        let (rows, columns) = self.bounds(rows, columns);
        rows.contains(&row) && columns.contains(&column)
    }
}

struct Cache {
    revision: u64,
    rows: Range<u64>,
    columns: Range<usize>,
    cells: Vec<Vec<GridCell>>,
}

#[derive(Default)]
pub(super) struct GridState {
    top: u64,
    fraction: f32,
    left: f32,
    widths: Vec<f32>,
    selection: Option<GridSelection>,
    cache: Option<Cache>,
    resizing: Option<(usize, f32, f32)>,
    thumb: Option<(f32, u64)>,
    dense: bool,
    fetched: usize,
    editor: Option<CellEditor>,
    column_menu: Option<(usize, Rect, bool)>,
    visible: u64,
    fit_last: bool,
}

impl GridState {
    pub(super) fn top(&self) -> u64 {
        self.top
    }

    pub(super) fn scroll_to(&mut self, row: u64) {
        self.top = row;
        self.fraction = 0.0;
    }

    pub(super) fn fit_last(&mut self) {
        self.fit_last = true;
    }

    pub(super) fn selection(&self) -> Option<GridSelection> {
        self.selection
    }

    pub(super) fn select(&mut self, row: u64, column: usize) {
        self.selection = Some(GridSelection { anchor: (row, column), head: (row, column), kind: SelectionKind::Cells });
    }

    pub(super) fn go_to(&mut self, row: u64, rows: u64) {
        if rows == 0 {
            return;
        }
        let row = row.min(rows - 1);
        let column = self.selection.map_or(0, |selection| selection.head.1);
        self.select(row, column);
        let visible = self.visible.max(1);
        if row < self.top || row >= self.top + visible {
            self.top = row.saturating_sub(visible / 2);
            self.fraction = 0.0;
        }
    }

    pub(super) fn page_rows(&self) -> u64 {
        self.visible.max(1)
    }

    pub(super) fn current_row(&self) -> Option<u64> {
        self.selection.map(|selection| selection.head.0)
    }

    pub(super) fn edit(&mut self, row: u64, column: usize, text: String) {
        self.select(row, column);
        self.editor = Some(CellEditor { row, column, text, focused: false });
    }

    #[cfg(test)]
    pub(super) fn widths(&self) -> &[f32] {
        &self.widths
    }

    #[cfg(test)]
    pub(super) fn fetched(&self) -> usize {
        self.fetched
    }

    fn row_height(&self) -> f32 {
        if self.dense { DENSE_ROW } else { ROW }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GridOutcome {
    pub(super) copy: Option<(Range<u64>, Range<usize>, bool)>,
    pub(super) near_end: bool,
    pub(super) selected: bool,
    pub(super) sort: Option<(usize, bool)>,
    pub(super) actions: Vec<GridAction>,
    pub(super) refused: Option<String>,
}

pub(super) fn chevron(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let points = vec![
        pos2(center.x - 3.5, center.y - 1.5),
        pos2(center.x + 3.5, center.y - 1.5),
        pos2(center.x, center.y + 2.5),
    ];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn cell_font() -> FontId {
    theme::regular(12.5)
}

fn type_mark(type_name: &str) -> &'static str {
    let name = type_name.to_ascii_lowercase();
    let name = name.trim_start_matches('_');
    if name.starts_with("int")
        || name.starts_with("float")
        || name.contains("numeric")
        || name.contains("decimal")
        || matches!(name, "oid" | "money" | "double" | "real" | "bigint" | "smallint" | "serial")
    {
        "123"
    } else if name.starts_with("bool") {
        "t/f"
    } else if name.contains("time") || name.contains("date") || name == "interval" {
        "dt"
    } else if name.contains("json") || name == "document" || name == "hash" || name == "map" {
        "{}"
    } else if name.starts_with("bytea") || name.contains("blob") || name.contains("binary") {
        "01"
    } else if type_name.starts_with('_') || name.ends_with("[]") || name == "array" || name == "list" {
        "[]"
    } else {
        "abc"
    }
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color()).size().x)
}

fn first_widths(ui: &Ui, source: &dyn GridSource) -> Vec<f32> {
    let columns = source.columns();
    let sample = source.cells(0..SAMPLED_ROWS.min(source.row_count()), 0..columns.len());
    let font = cell_font();
    let header = theme::semibold(12.5);
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let marks = if column.primary || source.link(index).is_some() { KEY_MARK } else { 0.0 };
            let named = text_width(ui, &column.name, &header) + 46.0 + MENU_ZONE + marks;
            let widest = sample
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| if cell.null { 40.0 } else { text_width(ui, &cell.text, &font) })
                .fold(0.0_f32, f32::max);
            (widest + 2.0 * PADDING).max(named).clamp(MIN_WIDTH + 20.0, MAX_FIRST_WIDTH)
        })
        .collect()
}

fn numbers_width(ui: &Ui, rows: u64) -> f32 {
    let digits = rows.max(1).to_string().len().max(2);
    text_width(ui, &"8".repeat(digits), &cell_font()) + 2.0 * PADDING + 4.0
}

fn cached<'a>(state: &'a mut GridState, source: &dyn GridSource, rows: Range<u64>, columns: Range<usize>) -> &'a Cache {
    let revision = source.revision();
    let fits = state.cache.as_ref().is_some_and(|cache| {
        cache.revision == revision
            && cache.rows.start <= rows.start
            && rows.end <= cache.rows.end
            && cache.columns.start <= columns.start
            && columns.end <= cache.columns.end
    });
    if !fits {
        let total = source.row_count();
        let wide = rows.start.saturating_sub(ROWS_AROUND)..(rows.end + ROWS_AROUND).min(total);
        let count = source.columns().len();
        let across = columns.start.saturating_sub(COLUMNS_AROUND)..(columns.end + COLUMNS_AROUND).min(count);
        let cells = source.cells(wide.clone(), across.clone());
        state.fetched += 1;
        state.cache = Some(Cache { revision, rows: wide, columns: across, cells });
    }
    state.cache.as_ref().expect("cached")
}

fn cell_at(cache: &Cache, row: u64, column: usize) -> Option<&GridCell> {
    let row = usize::try_from(row.checked_sub(cache.rows.start)?).ok()?;
    cache.cells.get(row)?.get(column.checked_sub(cache.columns.start)?)
}

pub(super) fn show(ui: &mut Ui, p: &IdePalette, source: &dyn GridSource, state: &mut GridState, id: Id) -> GridOutcome {
    show_with(ui, p, source, state, id, GridFeatures::default())
}

const SORT_ZONE: f32 = 18.0;
const MENU_ZONE: f32 = 14.0;
const KEY_MARK: f32 = 13.0;

fn key_mark(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let ring = pos2(center.x - 2.5, center.y);
    painter.circle_stroke(ring, 2.5, Stroke::new(1.2, color));
    painter.line_segment([pos2(ring.x + 2.5, center.y), pos2(center.x + 4.5, center.y)], Stroke::new(1.2, color));
    painter
        .line_segment([pos2(center.x + 3.5, center.y), pos2(center.x + 3.5, center.y + 2.5)], Stroke::new(1.2, color));
    painter
        .line_segment([pos2(center.x + 1.5, center.y), pos2(center.x + 1.5, center.y + 2.0)], Stroke::new(1.2, color));
}

fn link_mark(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let stroke = Stroke::new(1.2, color);
    painter.rect_stroke(
        Rect::from_center_size(pos2(center.x - 2.0, center.y + 1.0), vec2(5.0, 3.5)),
        1.5,
        stroke,
        egui::StrokeKind::Middle,
    );
    painter.rect_stroke(
        Rect::from_center_size(pos2(center.x + 2.0, center.y - 1.0), vec2(5.0, 3.5)),
        1.5,
        stroke,
        egui::StrokeKind::Middle,
    );
}

fn sort_mark(painter: &egui::Painter, center: egui::Pos2, descending: Option<bool>, color: egui::Color32) {
    let up = |y: f32| vec![pos2(center.x - 3.5, y + 2.0), pos2(center.x + 3.5, y + 2.0), pos2(center.x, y - 2.0)];
    let down = |y: f32| vec![pos2(center.x - 3.5, y - 2.0), pos2(center.x + 3.5, y - 2.0), pos2(center.x, y + 2.0)];
    match descending {
        Some(false) => painter.add(egui::Shape::convex_polygon(up(center.y), color, Stroke::NONE)),
        Some(true) => painter.add(egui::Shape::convex_polygon(down(center.y), color, Stroke::NONE)),
        None => {
            painter.add(egui::Shape::convex_polygon(up(center.y - 3.0), color, Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(down(center.y + 3.0), color, Stroke::NONE))
        }
    };
}

pub(super) fn show_with(
    ui: &mut Ui,
    p: &IdePalette,
    source: &dyn GridSource,
    state: &mut GridState,
    id: Id,
    features: GridFeatures,
) -> GridOutcome {
    let mut outcome = GridOutcome::default();
    let rect = ui.available_rect_before_wrap();
    ui.advance_cursor_after_rect(rect);
    let columns = source.columns();
    let rows = source.row_count();
    let numbers = numbers_width(ui, rows);
    if state.widths.len() != columns.len() {
        state.widths = first_widths(ui, source);
        state.cache = None;
        if state.fit_last
            && let Some((last, others)) = state.widths.split_last_mut()
        {
            let room = rect.width() - numbers - BAR - others.iter().sum::<f32>();
            *last = last.min(room).max(MIN_WIDTH + 20.0);
        }
    }
    let row_height = state.row_height();
    let total_width: f32 = state.widths.iter().sum();
    let body = Rect::from_min_max(
        pos2(rect.left() + numbers, rect.top() + HEADER),
        pos2(rect.right() - BAR, rect.bottom() - BAR),
    );
    let visible = ((body.height() / row_height).floor() as u64).max(1);
    state.visible = visible;
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let label = t!("ide-database-grid-name", rows = rows, columns = columns.len());
    accessibility::name_with(&response, egui::Role::Table, || label.clone());
    if response.clicked() || response.drag_started() {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let focused = ui.memory(|memory| memory.has_focus(id));
    if focused {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: features.result,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            );
        });
    }

    if response.hovered() {
        let delta = ui.input(|input| input.smooth_scroll_delta);
        if delta.y != 0.0 {
            state.fraction -= delta.y / row_height;
            let whole = state.fraction.trunc();
            state.fraction -= whole;
            if whole > 0.0 {
                state.top = state.top.saturating_add(whole as u64);
            } else if whole < 0.0 {
                state.top = state.top.saturating_sub((-whole) as u64);
            }
        }
        state.left -= delta.x;
    }
    let last_top = rows.saturating_sub(visible);
    if focused && state.editor.is_none() {
        keys(ui, state, (rows, columns, visible, body.width()), source, features, &mut outcome);
    }
    state.top = state.top.min(last_top);
    state.left = state.left.clamp(0.0, (total_width - body.width()).max(0.0));

    let mut starts = Vec::with_capacity(columns.len());
    let mut x = 0.0;
    for width in &state.widths {
        starts.push(x);
        x += width;
    }
    let first_column = starts.iter().rposition(|start| *start <= state.left).unwrap_or(0);
    let last_column = starts.iter().position(|start| *start >= state.left + body.width()).unwrap_or(columns.len());
    let shown_rows = state.top..(state.top + visible + 1).min(rows);
    let shown_columns = first_column..last_column.max(first_column);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, p.editor_background);
    let header = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + HEADER));
    painter.rect_filled(header, 0, p.island);
    let numbers_rect = Rect::from_min_max(pos2(rect.left(), body.top()), pos2(rect.left() + numbers, body.bottom()));
    painter.rect_filled(numbers_rect, 0, p.island);
    let scrolled = state.left;
    let x_of = |column: usize| body.left() + starts[column] - scrolled;
    let selection = state.selection;
    let line = Stroke::new(1.0, p.frame);

    let pointer = ui.input(|input| input.pointer.interact_pos());
    let hover = ui.input(|input| input.pointer.hover_pos());
    let column_at = |x: f32| -> Option<usize> {
        let offset = x - body.left() + scrolled;
        (offset >= 0.0).then(|| starts.iter().rposition(|start| *start <= offset)).flatten()
    };
    let first_row = state.top;
    let row_at = |y: f32| -> Option<u64> {
        let offset = ((y - body.top()) / row_height).floor();
        (offset >= 0.0).then(|| first_row + offset as u64).filter(|row| *row < rows)
    };

    let header_font = theme::semibold(12.5);
    let small = theme::regular(10.5);
    let mut edge_hovered = None;
    for column in shown_columns.clone() {
        let left = x_of(column);
        let width = state.widths[column];
        let cell = Rect::from_min_size(pos2(left, header.top()), vec2(width, HEADER));
        let clipped = painter.with_clip_rect(
            cell.intersect(Rect::from_min_max(pos2(body.left(), header.top()), pos2(body.right(), header.bottom()))),
        );
        let hidden = (body.left() - left).clamp(0.0, (width - SORT_ZONE - 2.0 * PADDING).max(0.0));
        let (left, width) = (left + hidden, width - hidden);
        let typed = !columns[column].type_name.is_empty();
        let mark = if typed { type_mark(&columns[column].type_name) } else { "" };
        let mark_color = if columns[column].filtered { p.accent } else { p.text_secondary };
        clipped.text(pos2(left + PADDING, cell.center().y), Align2::LEFT_CENTER, mark, small.clone(), mark_color);
        let mut mark_width = if typed { text_width(ui, mark, &small) + 5.0 } else { 0.0 };
        let keyed = columns[column].primary || source.link(column).is_some();
        if keyed {
            let center = pos2(left + PADDING + mark_width + KEY_MARK / 2.0 - 1.0, cell.center().y);
            if columns[column].primary {
                key_mark(&clipped, center, p.accent);
            } else {
                link_mark(&clipped, center, p.link);
            }
            mark_width += KEY_MARK;
        }
        let sort = columns[column].sort;
        let order_width = if sort.is_some_and(|(_, order)| order > 1) { 8.0 } else { 0.0 };
        let menu_width = if features.sortable { MENU_ZONE } else { 0.0 };
        let name = super::vcs::truncated(
            &columns[column].name,
            header_font.clone(),
            p.text,
            width - mark_width - 2.0 * PADDING - 10.0 - order_width - menu_width,
        );
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(name));
        clipped.galley(pos2(left + PADDING + mark_width, cell.center().y - galley.size().y / 2.0), galley, p.text);
        let zone = Rect::from_min_max(pos2(cell.right() - SORT_ZONE, cell.top()), cell.max);
        let over_zone = hover.is_some_and(|at| zone.contains(at));
        if features.sortable {
            let menu = Rect::from_min_max(pos2(zone.left() - MENU_ZONE, cell.top()), pos2(zone.left(), cell.bottom()));
            let over_menu = hover.is_some_and(|at| menu.contains(at));
            if over_menu {
                clipped.rect_filled(menu.shrink2(vec2(0.0, 4.0)), 3, p.hover);
            }
            chevron(&clipped, menu.center(), if over_menu { p.text } else { p.text_secondary });
            match sort {
                Some((descending, order)) => {
                    sort_mark(&clipped, pos2(zone.center().x, cell.center().y), Some(descending), p.accent);
                    if order > 1 {
                        clipped.text(
                            pos2(zone.left() - 1.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            order.to_string(),
                            small.clone(),
                            p.accent,
                        );
                    }
                }
                None => {
                    let color = if over_zone { p.text } else { p.text_secondary.gamma_multiply(0.6) };
                    sort_mark(&clipped, pos2(zone.center().x, cell.center().y), None, color);
                }
            }
        } else {
            chevron(&clipped, pos2(cell.right() - PADDING - 3.0, cell.center().y), p.text_secondary);
        }
        painter.vline(cell.right(), header.y_range(), line);
        if hover.is_some_and(|at| at.y <= header.bottom() && (at.x - cell.right()).abs() <= 4.0) {
            edge_hovered = Some(column);
        }
        if columns.len() < 200 && hover.is_some_and(|at| cell.contains(at)) {
            let mut tip = format!("{} : {}", columns[column].name, columns[column].type_name);
            if columns[column].primary {
                tip = format!("{tip}\n{}", t!("ide-database-grid-primary-key"));
            }
            if let Some(table) = source.link(column) {
                tip = format!("{tip}\n{}", t!("ide-database-grid-foreign-key", table = table));
            }
            response.clone().on_hover_text_at_pointer(tip);
        }
    }
    painter.hline(rect.x_range(), header.bottom(), line);
    painter.vline(numbers_rect.right(), rect.y_range(), line);

    if let Some(column) = edge_hovered.or(state.resizing.map(|(column, ..)| column)) {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        if response.double_clicked() && state.resizing.is_none() {
            let cache = cached(state, source, shown_rows.clone(), shown_columns.clone());
            let font = cell_font();
            let widest = (cache.rows.clone())
                .filter_map(|row| cell_at(cache, row, column))
                .map(|cell| text_width(ui, &cell.text, &font))
                .fold(text_width(ui, &columns[column].name, &header_font) + 40.0, f32::max);
            let fitted = (widest + 2.0 * PADDING).max(MIN_WIDTH);
            state.widths[column] = fitted;
        }
    }
    if response.drag_started()
        && let Some(column) = edge_hovered
    {
        state.resizing = Some((column, pointer.map_or(0.0, |at| at.x), state.widths[column]));
    }
    if let Some((column, from, width)) = state.resizing {
        if let Some(at) = pointer {
            state.widths[column] = (width + at.x - from).max(MIN_WIDTH);
        }
        if !response.dragged() {
            state.resizing = None;
        }
    }

    cached(state, source, shown_rows.clone(), shown_columns.clone());
    let top_row = state.top;
    let widths = state.widths.clone();
    let cache = state.cache.as_ref().expect("cached");
    let font = cell_font();
    let clip = Rect::from_min_max(pos2(body.left(), body.top()), pos2(body.right(), body.bottom()));
    let cells_painter = painter.with_clip_rect(clip);
    let mut hovered_cell = None;
    let mut link_hovered: Option<(u64, usize, bool)> = None;
    let marks = source.marks(shown_rows.clone());
    let content_right = (body.left() + total_width - state.left).min(body.right());
    let mark_of = |row: u64| {
        usize::try_from(row - shown_rows.start).ok().and_then(|at| marks.get(at).copied()).unwrap_or_default()
    };
    for row in shown_rows.clone() {
        let top = body.top() + (row - top_row) as f32 * row_height;
        let line_rect = Rect::from_min_size(pos2(rect.left(), top), vec2(numbers, row_height));
        let selected_row = selection.is_some_and(|selection| {
            let (rows_in, _) = selection.bounds(rows, columns.len());
            rows_in.contains(&row)
        });
        if selected_row {
            painter.rect_filled(line_rect, 0, p.hover);
        }
        let mark = mark_of(row);
        let (row_fill, gutter) = match mark {
            GridRowMark::Clean => (None, None),
            GridRowMark::Changed => (None, Some(p.gutter_modified)),
            GridRowMark::Inserted => (Some(p.diff_inserted), Some(p.gutter_added)),
            GridRowMark::Deleted => (Some(p.diff_deleted), Some(p.gutter_deleted)),
        };
        if let Some(gutter) = gutter {
            painter.rect_filled(
                Rect::from_min_size(pos2(line_rect.left() + 1.0, top + 2.0), vec2(3.0, row_height - 4.0)),
                1,
                gutter,
            );
        }
        if let Some(fill) = row_fill {
            cells_painter.rect_filled(
                Rect::from_min_max(pos2(body.left(), top), pos2(content_right, top + row_height)),
                0,
                fill,
            );
        }
        painter.text(
            pos2(line_rect.right() - PADDING, line_rect.center().y),
            Align2::RIGHT_CENTER,
            (row + 1).to_string(),
            font.clone(),
            p.gutter_text,
        );
        for column in shown_columns.clone() {
            let left = x_of(column);
            let width = widths[column];
            let cell_rect = Rect::from_min_size(pos2(left, top), vec2(width, row_height));
            let changed = cell_at(cache, row, column).is_some_and(|cell| cell.changed);
            if changed && mark != GridRowMark::Inserted {
                cells_painter.rect_filled(cell_rect, 0, p.diff_modified);
            }
            if selection.is_some_and(|selection| selection.contains(row, column, rows, columns.len())) {
                cells_painter.rect_filled(cell_rect, 0, p.editor_selection);
            }
            if changed {
                cells_painter.rect_filled(
                    Rect::from_min_size(cell_rect.min + vec2(1.0, 2.0), vec2(2.0, row_height - 4.0)),
                    0,
                    p.gutter_modified,
                );
            }
            let Some(cell) = cell_at(cache, row, column) else { continue };
            let inner = cell_rect.shrink2(vec2(PADDING, 0.0));
            let one = cells_painter.with_clip_rect(inner.intersect(clip));
            if cell.default || cell.null {
                let placeholder = if cell.default { "[DEFAULT]" } else { "[NULL]" };
                let job = super::vcs::truncated(placeholder, font.clone(), p.text_secondary, inner.width());
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                one.galley(pos2(inner.left(), inner.center().y - galley.size().y / 2.0), galley, p.text_secondary);
            } else if let Some(value) = cell.boolean {
                let box_rect = Rect::from_center_size(pos2(inner.left() + 7.0, inner.center().y), vec2(12.0, 12.0));
                one.rect_stroke(box_rect, 2, Stroke::new(1.0, p.text_secondary), egui::StrokeKind::Inside);
                if value {
                    super::glyphs::glyph_check(&one, box_rect.shrink(1.0), p.accent);
                }
                one.text(
                    pos2(box_rect.right() + 5.0, inner.center().y),
                    Align2::LEFT_CENTER,
                    &cell.text,
                    font.clone(),
                    p.text,
                );
            } else {
                let linked = source.link(column).is_some();
                let color = if linked {
                    p.link
                } else if cell.number {
                    p.number
                } else {
                    p.text
                };
                let job = super::vcs::truncated(&cell.text, font.clone(), color, inner.width());
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                let x = if cell.number { inner.right() - galley.size().x } else { inner.left() };
                one.galley(pos2(x, inner.center().y - galley.size().y / 2.0), galley, p.text);
            }
            if source.link(column).is_some()
                && !cell.null
                && !cell.default
                && let Some(at) = hover.filter(|at| cell_rect.contains(*at) && clip.contains(*at))
            {
                let zone = Rect::from_min_max(pos2(cell_rect.right() - LINK_ZONE, cell_rect.top()), cell_rect.max);
                let over = zone.contains(at);
                cells_painter.rect_filled(zone.shrink(2.0), 3, if over { p.hover } else { p.editor_background });
                super::glyphs::glyph_forward(
                    &cells_painter,
                    Rect::from_center_size(zone.center(), vec2(9.0, 8.0)),
                    p.link,
                );
                link_hovered = Some((row, column, over));
            }
            if hover.is_some_and(|at| cell_rect.contains(at) && clip.contains(at)) {
                hovered_cell = Some((
                    row,
                    column,
                    cell.cut || text_width(ui, &cell.text, &font) > inner.width(),
                    cell.text.clone(),
                ));
            }
        }
        if mark == GridRowMark::Deleted {
            cells_painter.hline(body.left()..=content_right, top + row_height / 2.0, Stroke::new(1.0, p.vcs_deleted));
        }
        cells_painter.hline(
            body.left()..=body.right(),
            top + row_height,
            Stroke::new(1.0, p.frame.gamma_multiply(0.5)),
        );
    }
    for column in shown_columns.clone() {
        let right = x_of(column) + widths[column];
        cells_painter.vline(right, body.y_range(), Stroke::new(1.0, p.frame.gamma_multiply(0.5)));
    }
    if let Some((_, _, true, text)) = &hovered_cell
        && state.resizing.is_none()
    {
        let shown = if text.chars().count() > 1000 {
            format!("{}…", text.chars().take(1000).collect::<String>())
        } else {
            text.clone()
        };
        response.clone().on_hover_text_at_pointer(shown);
    }

    let pressed = ui.input(|input| input.pointer.primary_pressed());
    let shift = ui.input(|input| {
        input
            .events
            .iter()
            .find_map(|event| match event {
                egui::Event::PointerButton { pressed: true, modifiers, .. } => Some(modifiers.shift),
                _ => None,
            })
            .unwrap_or(input.modifiers.shift)
    });
    let on_top = |at: egui::Pos2| ui.ctx().layer_id_at(at).is_none_or(|layer| layer == ui.layer_id());
    let command = ui.input(|input| input.modifiers.command);
    if let Some((row, column, over)) = link_hovered
        && state.resizing.is_none()
    {
        if over || command {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if pressed && (over || command) && pointer.is_some_and(on_top) {
            outcome.actions.push(GridAction::FollowKey { row, column });
        }
    }
    if pressed
        && edge_hovered.is_none()
        && let Some(at) = pointer.filter(|at| rect.contains(*at) && on_top(*at))
    {
        let in_header = at.y < header.bottom();
        let in_numbers = at.x < numbers_rect.right() && at.y >= body.top();
        let sort_zone = in_header
            && features.sortable
            && column_at(at.x).is_some_and(|column| at.x >= x_of(column) + state.widths[column] - SORT_ZONE);
        let menu_zone = in_header
            && features.sortable
            && !sort_zone
            && column_at(at.x)
                .is_some_and(|column| at.x >= x_of(column) + state.widths[column] - SORT_ZONE - MENU_ZONE);
        let target = match (in_header, in_numbers) {
            _ if menu_zone => {
                if let Some(column) = column_at(at.x) {
                    let right = x_of(column) + state.widths[column] - SORT_ZONE;
                    let zone = Rect::from_min_max(pos2(right - MENU_ZONE, header.top()), pos2(right, header.bottom()));
                    state.column_menu = Some((column, zone, true));
                }
                None
            }
            _ if sort_zone => {
                outcome.sort = column_at(at.x).map(|column| (column, shift));
                None
            }
            (true, false) => column_at(at.x).map(|column| ((state.top, column), SelectionKind::Columns)),
            (false, true) => row_at(at.y).map(|row| ((row, 0), SelectionKind::Rows)),
            (false, false) if body.contains(at) => {
                row_at(at.y).zip(column_at(at.x)).map(|cell| (cell, SelectionKind::Cells))
            }
            _ => None,
        };
        if let Some((cell, kind)) = target {
            let anchor = match state.selection {
                Some(selection) if shift && selection.kind == kind => selection.anchor,
                _ => cell,
            };
            state.selection = Some(GridSelection { anchor, head: cell, kind });
            outcome.selected = true;
        }
    } else if response.dragged()
        && state.resizing.is_none()
        && let Some(at) = pointer
        && let Some(selection) = state.selection.as_mut()
        && body.contains(at)
        && let Some(cell) = row_at(at.y).zip(column_at(at.x))
    {
        let head = match selection.kind {
            SelectionKind::Cells => cell,
            SelectionKind::Rows => (cell.0, selection.head.1),
            SelectionKind::Columns => (selection.head.0, cell.1),
        };
        if head != selection.head {
            selection.head = head;
            outcome.selected = true;
        }
    }
    if response.secondary_clicked()
        && let Some(cell) = pointer.filter(|at| body.contains(*at)).and_then(|at| row_at(at.y).zip(column_at(at.x)))
        && !state.selection.is_some_and(|selection| selection.contains(cell.0, cell.1, rows, columns.len()))
    {
        state.select(cell.0, cell.1);
        outcome.selected = true;
    }
    if response.double_clicked()
        && edge_hovered.is_none()
        && let Some((row, column)) =
            pointer.filter(|at| body.contains(*at)).and_then(|at| row_at(at.y).zip(column_at(at.x)))
        && mark_of(row) != GridRowMark::Deleted
    {
        match refusal(source, features, column) {
            None => {
                let text = source.full_text(row, column).unwrap_or_default();
                state.edit(row, column, text);
            }
            reason => outcome.refused = reason,
        }
    }
    let menu_selection = state.selection;
    let menu_rows = rows;
    response.context_menu(|ui| {
        let bounds = menu_selection.map(|selection| selection.bounds(rows, columns.len()));
        let head = menu_selection.map(|selection| selection.head);
        let has_bounds = bounds.is_some();
        let editable = (features.editable || source.refusal().is_some()) && head.is_some();
        let linked = head.is_some_and(|(_, column)| source.link(column).is_some());
        let shown = |slot: &str| match slot {
            "ide-database-grid-menu-advanced-copy" => features.clipboard,
            "ide-database-grid-copy-header" => !features.clipboard,
            customization::GRID_PASTE => features.clipboard && features.editable,
            "ide-database-grid-menu-edit" => editable,
            "ide-database-grid-filter" => features.filterable && head.is_some(),
            "ide-database-grid-menu-order" => features.sortable && head.is_some(),
            "ide-database-grid-menu-navigate" => features.result || linked,
            "ide-database-grid-menu-panels" | "ide-database-grid-menu-layout" | "ide-database-export" => {
                features.result
            }
            "ide-database-grid-generate" => features.generate && has_bounds,
            _ => true,
        };
        customization::popup(ui, customization::DATA_GRID_POPUP, shown, |ui, slot| match slot {
            "ide-database-grid-copy" => {
                if ui.add(egui::Button::new(t!("ide-database-grid-copy")).shortcut_text("⌘C")).clicked() {
                    outcome.copy = bounds.clone().map(|(rows_in, columns_in)| (rows_in, columns_in, false));
                    ui.close();
                }
            }
            "ide-database-grid-menu-advanced-copy" => {
                ui.menu_button(t!("ide-database-grid-menu-advanced-copy"), |ui| {
                    if ui.button(t!("ide-database-grid-copy-header")).clicked() {
                        outcome.copy = bounds.clone().map(|(rows_in, columns_in)| (rows_in, columns_in, true));
                        ui.close();
                    }
                    copy_menu(ui, &bounds, columns.len(), &mut outcome);
                });
            }
            "ide-database-grid-copy-header" => {
                if ui.button(t!("ide-database-grid-copy-header")).clicked() {
                    outcome.copy = bounds.clone().map(|(rows_in, columns_in)| (rows_in, columns_in, true));
                    ui.close();
                }
            }
            customization::GRID_PASTE => paste_menu(ui, &bounds, features, &mut outcome),
            "ide-database-grid-menu-edit" => {
                let Some((row, column)) = head else { return };
                ui.menu_button(t!("ide-database-grid-menu-edit"), |ui| {
                    let rows_in: Vec<u64> = bounds.clone().map(|(rows_in, _)| rows_in.collect()).unwrap_or_default();
                    let deleted = mark_of(row) == GridRowMark::Deleted;
                    let cell_refused = refusal(source, features, column);
                    let grid_refused = (!features.editable).then(|| source.refusal().unwrap_or_default().to_owned());
                    let item = |ui: &mut Ui, label: String, shortcut: &str, refused: Option<&String>, enabled: bool| {
                        let response = ui.add_enabled(
                            enabled && refused.is_none(),
                            egui::Button::new(label).shortcut_text(shortcut),
                        );
                        match refused {
                            Some(reason) => response.on_disabled_hover_text(reason.as_str()),
                            None => response,
                        }
                        .clicked()
                    };
                    if item(ui, t!("ide-database-grid-edit-cell"), "F2", cell_refused.as_ref(), !deleted) {
                        let text = source.full_text(row, column).unwrap_or_default();
                        state.edit(row, column, text);
                        ui.close();
                    }
                    if source.link(column).is_some()
                        && item(ui, t!("ide-database-grid-choose-value"), "⇧↩", cell_refused.as_ref(), !deleted)
                    {
                        outcome.actions.push(GridAction::Lookup { row, column });
                        ui.close();
                    }
                    let null_refused = cell_refused
                        .clone()
                        .or_else(|| columns[column].not_null.then(|| t!("ide-database-grid-not-null")));
                    if item(ui, t!("ide-database-grid-set-null"), "", null_refused.as_ref(), !deleted) {
                        outcome.actions.push(GridAction::SetNull { row, column });
                        ui.close();
                    }
                    if item(ui, t!("ide-database-grid-set-default"), "", cell_refused.as_ref(), !deleted) {
                        outcome.actions.push(GridAction::SetDefault { row, column });
                        ui.close();
                    }
                    if item(ui, t!("ide-database-grid-copy-above"), "", grid_refused.as_ref(), row > 0 && !deleted) {
                        outcome.actions.push(GridAction::CopyAbove { row });
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, t!("ide-database-grid-add-row"), "⌘N", grid_refused.as_ref(), true) {
                        outcome.actions.push(GridAction::AddRow);
                        ui.close();
                    }
                    if item(ui, t!("ide-database-grid-duplicate-row"), "⌘D", grid_refused.as_ref(), true) {
                        outcome.actions.push(GridAction::Duplicate { row });
                        ui.close();
                    }
                    if deleted {
                        if item(ui, t!("ide-database-grid-restore-rows"), "", grid_refused.as_ref(), true) {
                            outcome.actions.push(GridAction::Restore { rows: rows_in });
                            ui.close();
                        }
                    } else if item(ui, t!("ide-database-grid-delete-rows"), "⌘⌫", grid_refused.as_ref(), true) {
                        outcome.actions.push(GridAction::Delete { rows: rows_in });
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, t!("ide-database-grid-preview"), "", grid_refused.as_ref(), true) {
                        outcome.actions.push(GridAction::Preview);
                        ui.close();
                    }
                });
            }
            "ide-database-grid-filter" => {
                let Some((row, column)) = head else { return };
                ui.menu_button(t!("ide-database-grid-filter"), |ui| {
                    let operators = [
                        ("equal", "= …"),
                        ("notEqual", "<> …"),
                        ("greater", "> …"),
                        ("less", "< …"),
                        ("contains", "LIKE '%…%'"),
                        ("isNull", "IS NULL"),
                        ("isNotNull", "IS NOT NULL"),
                    ];
                    for (operator, label) in operators {
                        if ui.button(format!("{} {label}", columns[column].name)).clicked() {
                            outcome.actions.push(GridAction::Filter { row, column, operator });
                            ui.close();
                        }
                    }
                    if let Some((rows_in, _)) = bounds.clone().filter(|(rows_in, _)| rows_in.end - rows_in.start > 1)
                        && ui.button(t!("ide-database-grid-filter-in")).clicked()
                    {
                        outcome.actions.push(GridAction::FilterIn { rows: rows_in.collect(), column });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("ide-database-grid-filter-clipboard")).clicked() {
                        outcome.actions.push(GridAction::FilterClipboard { column, operator: "equal" });
                        ui.close();
                    }
                    if ui.button(t!("ide-database-grid-filter-custom")).clicked() {
                        outcome.actions.push(GridAction::FilterCustom { column });
                        ui.close();
                    }
                    if ui.button(t!("ide-database-grid-filter-values")).clicked() {
                        outcome.actions.push(GridAction::FilterValues { column });
                        ui.close();
                    }
                    if columns[column].filtered && ui.button(t!("ide-database-grid-filter-reset-column")).clicked() {
                        outcome.actions.push(GridAction::ResetColumn { column });
                        ui.close();
                    }
                });
            }
            "ide-database-grid-menu-order" => {
                let Some((_, column)) = head else { return };
                ui.menu_button(t!("ide-database-grid-menu-order"), |ui| {
                    order_menu(ui, column, columns[column].sort.is_some(), &mut outcome);
                });
            }
            "ide-database-grid-menu-navigate" => {
                ui.menu_button(t!("ide-database-grid-menu-navigate"), |ui| {
                    if let Some((row, column)) = head {
                        relations_menu(ui, source, (row, column), &mut outcome);
                    }
                    if features.result {
                        navigate_menu(ui, state, menu_rows, &mut outcome);
                    }
                });
            }
            "ide-database-grid-menu-view" => {
                ui.menu_button(t!("ide-database-grid-menu-view"), |ui| {
                    if ui.checkbox(&mut state.dense, t!("ide-database-grid-dense")).changed() {
                        ui.close();
                    }
                });
            }
            "ide-database-grid-menu-panels" => {
                ui.menu_button(t!("ide-database-grid-menu-panels"), |ui| {
                    let shortcut = crate::keymap::shortcut("db.result.toggleValuePanel").unwrap_or_default();
                    let mut shown = features.value_panel;
                    if ui.add(egui::Checkbox::new(&mut shown, t!("ide-database-value-panel"))).clicked() {
                        outcome.actions.push(GridAction::ToggleValuePanel);
                        ui.close();
                    }
                    if !shortcut.is_empty() {
                        ui.label(egui::RichText::new(shortcut).weak());
                    }
                });
            }
            "ide-database-grid-menu-layout" => {
                ui.menu_button(t!("ide-database-grid-menu-layout"), |ui| {
                    for (mode, label, key) in [
                        ("grid", t!("ide-database-view-grid"), "⇥"),
                        ("text", t!("ide-database-view-text"), ""),
                        ("record", t!("ide-database-view-record"), "⇥"),
                    ] {
                        if ui.add(egui::Button::new(label).shortcut_text(key)).clicked() {
                            outcome.actions.push(GridAction::View { mode });
                            ui.close();
                        }
                    }
                });
            }
            "ide-database-grid-generate" => {
                let Some((rows_in, _)) = bounds.clone() else { return };
                ui.menu_button(t!("ide-database-grid-generate"), |ui| {
                    for kind in ["select", "insert", "update", "delete"] {
                        if ui.button(kind.to_uppercase()).clicked() {
                            outcome.actions.push(GridAction::Generate { kind, rows: rows_in.clone() });
                            ui.close();
                        }
                    }
                });
            }
            "ide-database-export" if ui.button(t!("ide-database-export")).clicked() => {
                outcome.actions.push(GridAction::Export);
                ui.close();
            }
            _ => {}
        });
    });
    if let Some((column, zone, opening)) = state.column_menu {
        state.column_menu = Some((column, zone, false));
        let shown = egui::Popup::new(
            id.with("column-menu"),
            ui.ctx().clone(),
            egui::PopupAnchor::ParentRect(zone),
            ui.layer_id(),
        )
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .open_memory(opening.then_some(egui::SetOpenCommand::Bool(true)))
        .show(|ui| {
            ui.set_min_width(200.0);
            column_menu(ui, columns, column, features, &mut outcome)
        });
        match shown.map(|shown| shown.inner) {
            None => state.column_menu = None,
            Some(ColumnChoice::Select) => {
                state.selection = Some(GridSelection {
                    anchor: (state.top, column),
                    head: (state.top, column),
                    kind: SelectionKind::Columns,
                });
                outcome.selected = true;
            }
            Some(ColumnChoice::Fit) => {
                let font = cell_font();
                let cache = cached(state, source, shown_rows.clone(), shown_columns.clone());
                let widest = (cache.rows.clone())
                    .filter_map(|row| cell_at(cache, row, column))
                    .map(|cell| text_width(ui, &cell.text, &font))
                    .fold(text_width(ui, &columns[column].name, &header_font) + 60.0, f32::max);
                state.widths[column] = (widest + 2.0 * PADDING).max(MIN_WIDTH);
            }
            Some(ColumnChoice::Nothing) => {}
        }
    }
    editor(ui, p, state, (body, row_height, &starts), id, &mut outcome);

    scrollbars(ui, p, state, (rect, body), (rows, visible), total_width, id);
    if state.top + visible >= rows {
        outcome.near_end = true;
    }
    if let Some(selection) = state.selection
        && focused
    {
        let (row, column) = selection.head;
        if let Some(cell) = cell_at(state.cache.as_ref().expect("cached"), row, column) {
            let text = if cell.null { "NULL".to_owned() } else { cell.text.clone() };
            let name = columns.get(column).map_or("", |column| column.name.as_str());
            let spoken = t!("ide-database-grid-cell", row = row + 1, column = name, value = text);
            accessibility::describe(&response, &spoken);
        }
    }
    outcome
}

fn refusal(source: &dyn GridSource, features: GridFeatures, column: usize) -> Option<String> {
    if !features.editable {
        return Some(source.refusal().map_or_else(|| t!("ide-database-grid-read-only"), str::to_owned));
    }
    let editable = source.columns().get(column).is_some_and(|column| column.editable);
    (!editable).then(|| t!("ide-database-grid-column-read-only"))
}

fn editor(
    ui: &mut Ui,
    p: &IdePalette,
    state: &mut GridState,
    (body, row_height, starts): (Rect, f32, &[f32]),
    id: Id,
    outcome: &mut GridOutcome,
) {
    let Some(editor) = state.editor.as_mut() else { return };
    if editor.row < state.top || editor.column >= starts.len() {
        state.editor = None;
        return;
    }
    let top = body.top() + (editor.row - state.top) as f32 * row_height;
    let left = body.left() + starts[editor.column] - state.left;
    let width = state.widths.get(editor.column).copied().unwrap_or(MIN_WIDTH);
    let cell = Rect::from_min_size(pos2(left, top), vec2(width, row_height)).intersect(body.expand(1.0));
    if cell.height() <= 0.0 || cell.width() <= 0.0 {
        return;
    }
    ui.painter().rect_filled(cell, 0, p.text_field);
    ui.painter().rect_stroke(cell, 0, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
    let editor_id = id.with("cell-editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell.shrink2(vec2(3.0, 1.0))));
    let (enter, escape) = ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
    let edit = child.add(
        egui::TextEdit::singleline(&mut editor.text)
            .id(editor_id)
            .font(cell_font())
            .frame(egui::Frame::NONE)
            .text_color(p.text)
            .desired_width(cell.width() - 6.0),
    );
    accessibility::labelled(&edit, &t!("ide-database-grid-cell-editor"));
    if !editor.focused {
        edit.request_focus();
        editor.focused = true;
        return;
    }
    let (row, column) = (editor.row, editor.column);
    if escape {
        state.editor = None;
        ui.memory_mut(|memory| memory.request_focus(id));
    } else if enter || edit.lost_focus() {
        let text = std::mem::take(&mut editor.text);
        state.editor = None;
        outcome.actions.push(GridAction::Commit { row, column, text });
        if enter {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
    }
}

fn keys(
    ui: &Ui,
    state: &mut GridState,
    (rows, grid_columns, visible, shown): (u64, &[GridColumn], u64, f32),
    source: &dyn GridSource,
    features: GridFeatures,
    outcome: &mut GridOutcome,
) {
    let columns = grid_columns.len();
    let events = ui.input(|input| input.events.clone());
    for event in events {
        let anchor = || {
            state.selection.map_or((0, 0), |selection| {
                let (rows_in, columns_in) = selection.bounds(rows, columns);
                (rows_in.start, columns_in.start)
            })
        };
        let (key, modifiers) = match event {
            egui::Event::Key { key, pressed: true, modifiers, .. } => (key, modifiers),
            egui::Event::Copy if rows > 0 && columns > 0 => {
                outcome.copy = state.selection.map(|selection| {
                    let (rows_in, columns_in) = selection.bounds(rows, columns);
                    (rows_in, columns_in, false)
                });
                continue;
            }
            egui::Event::Paste(text) if features.editable && features.clipboard => {
                let (row, column) = anchor();
                outcome.actions.push(GridAction::Paste { row, column, text: Some(text), insert: false });
                continue;
            }
            egui::Event::Text(text) if !text.chars().any(char::is_control) && !text.is_empty() => {
                if start_editing(state, source, features, outcome, Some(text)) {
                    break;
                }
                continue;
            }
            _ => continue,
        };
        if features.result && key == Key::Tab && !modifiers.any() {
            outcome.actions.push(GridAction::View { mode: "record" });
            continue;
        }
        if features.result && key == Key::G && modifiers.ctrl && !modifiers.command && !modifiers.shift {
            outcome.actions.push(GridAction::GoToRow);
            continue;
        }
        if (key == Key::F2 || key == Key::Enter) && !modifiers.any() {
            if start_editing(state, source, features, outcome, None) {
                break;
            }
            continue;
        }
        if features.clipboard
            && features.editable
            && key == Key::V
            && modifiers.command
            && modifiers.alt
            && modifiers.shift
        {
            let (row, column) = anchor();
            outcome.actions.push(GridAction::PasteAdvanced { row, column });
            continue;
        }
        if features.editable {
            let head = state.selection.map(|selection| selection.head);
            let rows_in = || -> Vec<u64> {
                state.selection.map(|selection| selection.bounds(rows, columns).0.collect()).unwrap_or_default()
            };
            match key {
                Key::N if modifiers.command => {
                    outcome.actions.push(GridAction::AddRow);
                    continue;
                }
                Key::D if modifiers.command => {
                    if let Some((row, _)) = head {
                        outcome.actions.push(GridAction::Duplicate { row });
                    }
                    continue;
                }
                Key::Backspace | Key::Delete if modifiers.command => {
                    outcome.actions.push(GridAction::Delete { rows: rows_in() });
                    continue;
                }
                Key::S if modifiers.command => {
                    outcome.actions.push(GridAction::Save);
                    continue;
                }
                Key::Enter if modifiers.shift_only() => {
                    if let Some((row, column)) = head
                        && grid_columns.get(column).is_some_and(|column| column.editable)
                        && source.link(column).is_some()
                    {
                        outcome.actions.push(GridAction::Lookup { row, column });
                    }
                    continue;
                }
                Key::Escape if !modifiers.any() => {
                    if let Some((row, column)) = head {
                        outcome.actions.push(GridAction::Revert { row, column });
                    }
                    continue;
                }
                _ => {}
            }
        }
        if rows == 0 || columns == 0 {
            continue;
        }
        if key == Key::B && modifiers.command && !modifiers.shift && !modifiers.alt {
            if let Some((row, column)) = state.selection.map(|selection| selection.head)
                && source.link(column).is_some()
            {
                outcome.actions.push(GridAction::FollowKey { row, column });
            }
            continue;
        }
        if modifiers.command && modifiers.shift && key == Key::C && features.clipboard {
            if let Some(selection) = state.selection {
                let (rows_in, columns_in) = selection.bounds(rows, columns);
                outcome.actions.push(GridAction::CopyAdvanced { rows: rows_in, columns: columns_in });
            }
            continue;
        }
        if modifiers.command && key == Key::C {
            outcome.copy = state.selection.map(|selection| {
                let (rows_in, columns_in) = selection.bounds(rows, columns);
                (rows_in, columns_in, modifiers.shift)
            });
            continue;
        }
        if modifiers.command && key == Key::A {
            state.selection =
                Some(GridSelection { anchor: (0, 0), head: (rows - 1, columns - 1), kind: SelectionKind::Cells });
            outcome.selected = true;
            continue;
        }
        let current = state.selection.map_or((state.top, 0), |selection| selection.head);
        let (row, column) = current;
        let moved = match key {
            Key::ArrowUp if !modifiers.command => (row.saturating_sub(1), column),
            Key::ArrowDown if !modifiers.command => ((row + 1).min(rows - 1), column),
            Key::ArrowLeft => (row, column.saturating_sub(1)),
            Key::ArrowRight => (row, (column + 1).min(columns - 1)),
            Key::PageUp => (row.saturating_sub(visible), column),
            Key::PageDown => ((row + visible).min(rows - 1), column),
            Key::Home | Key::ArrowUp if modifiers.command => (0, column),
            Key::End | Key::ArrowDown if modifiers.command => (rows - 1, column),
            Key::Home => (row, 0),
            Key::End => (row, columns - 1),
            _ => continue,
        };
        let anchor = match state.selection {
            Some(selection) if modifiers.shift => selection.anchor,
            _ => moved,
        };
        state.selection = Some(GridSelection { anchor, head: moved, kind: SelectionKind::Cells });
        outcome.selected = true;
        if moved.0 < state.top {
            state.top = moved.0;
        } else if moved.0 >= state.top + visible {
            state.top = moved.0 + 1 - visible;
        }
        let start: f32 = state.widths[..moved.1].iter().sum();
        let width = state.widths[moved.1];
        if start < state.left {
            state.left = start;
        }
        let shown = shown.max(1.0);
        if start + width > state.left + shown {
            state.left = start + width - shown;
        }
    }
}

fn start_editing(
    state: &mut GridState,
    source: &dyn GridSource,
    features: GridFeatures,
    outcome: &mut GridOutcome,
    typed: Option<String>,
) -> bool {
    let Some((row, column)) = state.selection.map(|selection| selection.head) else { return false };
    if row >= source.row_count() {
        return false;
    }
    if let Some(reason) = refusal(source, features, column) {
        outcome.refused = Some(reason);
        return false;
    }
    if source.marks(row..row + 1).first() == Some(&GridRowMark::Deleted) {
        return false;
    }
    let text = typed.unwrap_or_else(|| source.full_text(row, column).unwrap_or_default());
    state.edit(row, column, text);
    true
}

fn relations_menu(ui: &mut Ui, source: &dyn GridSource, (row, column): (u64, usize), outcome: &mut GridOutcome) {
    let link = source.link(column);
    let references = source.references();
    if link.is_none() && references.is_empty() {
        return;
    }
    ui.separator();
    if let Some(table) = link
        && ui
            .add(egui::Button::new(t!("ide-database-grid-open-referenced", table = table)).shortcut_text("⌘B"))
            .clicked()
    {
        outcome.actions.push(GridAction::FollowKey { row, column });
        ui.close();
    }
    if !references.is_empty() {
        ui.menu_button(t!("ide-database-grid-references"), |ui| {
            for (reference, label) in references.iter().enumerate() {
                if ui.button(label).clicked() {
                    outcome.actions.push(GridAction::OpenReferences { row, reference });
                    ui.close();
                }
            }
        });
    }
}

fn copy_menu(ui: &mut Ui, bounds: &Option<(Range<u64>, Range<usize>)>, width: usize, outcome: &mut GridOutcome) {
    let Some((rows_in, columns_in)) = bounds.clone() else { return };
    if ui.button(t!("ide-database-grid-copy-names")).clicked() {
        let (rows, columns) = (rows_in.clone(), columns_in.clone());
        outcome.actions.push(GridAction::CopyAs { format: "columnNames", rows, columns });
        ui.close();
    }
    if ui.button(t!("ide-database-grid-copy-row-json")).clicked() {
        outcome.actions.push(GridAction::CopyAs { format: "json", rows: rows_in.clone(), columns: 0..width });
        ui.close();
    }
    ui.menu_button(t!("ide-database-grid-copy-as"), |ui| {
        for (at, format) in zen_db::clip::CopyFormat::ALL.iter().enumerate() {
            if matches!(at, 6 | 9) {
                ui.separator();
            }
            if ui.button(format.name()).clicked() {
                let (rows, columns) = (rows_in.clone(), columns_in.clone());
                outcome.actions.push(GridAction::CopyAs { format: format.id(), rows, columns });
                ui.close();
            }
        }
    });
    let shortcut = crate::keymap::shortcut("view.database.copyAdvanced").unwrap_or_default();
    if ui.add(egui::Button::new(t!("ide-database-grid-copy-advanced")).shortcut_text(shortcut)).clicked() {
        outcome.actions.push(GridAction::CopyAdvanced { rows: rows_in, columns: columns_in });
        ui.close();
    }
}

fn paste_menu(
    ui: &mut Ui,
    bounds: &Option<(Range<u64>, Range<usize>)>,
    features: GridFeatures,
    outcome: &mut GridOutcome,
) {
    if !features.editable {
        return;
    }
    let (row, column) = bounds.as_ref().map_or((0, 0), |(rows_in, columns_in)| (rows_in.start, columns_in.start));
    if ui.add(egui::Button::new(t!("ide-database-grid-paste")).shortcut_text("⌘V")).clicked() {
        outcome.actions.push(GridAction::Paste { row, column, text: None, insert: false });
        ui.close();
    }
    ui.menu_button(t!("ide-database-grid-menu-advanced-paste"), |ui| {
        if ui.button(t!("ide-database-grid-paste-rows")).clicked() {
            outcome.actions.push(GridAction::Paste { row, column, text: None, insert: true });
            ui.close();
        }
        let shortcut = crate::keymap::shortcut("view.database.pasteAdvanced").unwrap_or_default();
        if ui.add(egui::Button::new(t!("ide-database-grid-paste-advanced")).shortcut_text(shortcut)).clicked() {
            outcome.actions.push(GridAction::PasteAdvanced { row, column });
            ui.close();
        }
    });
}

fn order_menu(ui: &mut Ui, column: usize, sorted: bool, outcome: &mut GridOutcome) {
    for (direction, label) in [("asc", t!("ide-database-grid-sort-asc")), ("desc", t!("ide-database-grid-sort-desc"))] {
        if ui.button(label).clicked() {
            outcome.actions.push(GridAction::SortBy { column, direction });
            ui.close();
        }
    }
    if sorted && ui.button(t!("ide-database-grid-sort-none")).clicked() {
        outcome.actions.push(GridAction::SortBy { column, direction: "none" });
        ui.close();
    }
}

fn navigate_menu(ui: &mut Ui, state: &mut GridState, rows: u64, outcome: &mut GridOutcome) {
    if ui.add(egui::Button::new(t!("ide-database-grid-go-to-row")).shortcut_text("⌃G")).clicked() {
        outcome.actions.push(GridAction::GoToRow);
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(rows > 0, egui::Button::new(t!("ide-database-grid-first-row")).shortcut_text("⌘↑")).clicked()
    {
        state.go_to(0, rows);
        outcome.selected = true;
        ui.close();
    }
    if ui.add_enabled(rows > 0, egui::Button::new(t!("ide-database-grid-last-row")).shortcut_text("⌘↓")).clicked() {
        state.go_to(rows.saturating_sub(1), rows);
        outcome.selected = true;
        ui.close();
    }
    ui.separator();
    if ui.button(t!("ide-database-fetch-next")).clicked() {
        outcome.actions.push(GridAction::FetchNext);
        ui.close();
    }
    if ui.button(t!("ide-database-fetch-all")).clicked() {
        outcome.actions.push(GridAction::FetchAll);
        ui.close();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnChoice {
    Nothing,
    Select,
    Fit,
}

fn column_menu(
    ui: &mut Ui,
    columns: &[GridColumn],
    column: usize,
    features: GridFeatures,
    outcome: &mut GridOutcome,
) -> ColumnChoice {
    let mut choice = ColumnChoice::Nothing;
    let Some(shown) = columns.get(column) else { return choice };
    ui.label(egui::RichText::new(format!("{} : {}", shown.name, shown.type_name)).weak());
    ui.separator();
    order_menu(ui, column, shown.sort.is_some(), outcome);
    if features.filterable {
        ui.separator();
        if ui.button(t!("ide-database-grid-filter-custom")).clicked() {
            outcome.actions.push(GridAction::FilterCustom { column });
            ui.close();
        }
        if ui.button(t!("ide-database-grid-filter-values")).clicked() {
            outcome.actions.push(GridAction::FilterValues { column });
            ui.close();
        }
        if shown.filtered && ui.button(t!("ide-database-grid-filter-reset-column")).clicked() {
            outcome.actions.push(GridAction::ResetColumn { column });
            ui.close();
        }
    }
    ui.separator();
    if ui.button(t!("ide-database-grid-select-column")).clicked() {
        choice = ColumnChoice::Select;
        ui.close();
    }
    if ui.button(t!("ide-database-grid-fit-column")).clicked() {
        choice = ColumnChoice::Fit;
        ui.close();
    }
    if ui.button(t!("ide-database-grid-copy-column-name")).clicked() {
        ui.ctx().copy_text(shown.name.clone());
        ui.close();
    }
    choice
}

fn scrollbars(
    ui: &Ui,
    p: &IdePalette,
    state: &mut GridState,
    (rect, body): (Rect, Rect),
    (rows, visible): (u64, u64),
    total_width: f32,
    id: Id,
) {
    let painter = ui.painter();
    let track = Rect::from_min_max(pos2(rect.right() - BAR, body.top()), pos2(rect.right(), body.bottom()));
    if rows > visible {
        let share = (visible as f64 / rows as f64).clamp(0.02, 1.0) as f32;
        let thumb_height = (track.height() * share).max(20.0);
        let last_top = rows - visible;
        let position = (state.top as f64 / last_top as f64) as f32;
        let thumb = Rect::from_min_size(
            pos2(track.left() + 2.0, track.top() + (track.height() - thumb_height) * position),
            vec2(BAR - 4.0, thumb_height),
        );
        let response = ui.interact(track, id.with("vertical"), Sense::click_and_drag());
        let color = if response.hovered() || response.dragged() { p.text_secondary } else { p.frame };
        painter.rect_filled(thumb, 3, color);
        if response.drag_started() {
            state.thumb = Some((response.interact_pointer_pos().map_or(0.0, |at| at.y), state.top));
        }
        if let (Some((from, top)), Some(at)) = (state.thumb, response.interact_pointer_pos()) {
            let moved = (at.y - from) / (track.height() - thumb_height).max(1.0);
            let rows_moved = (f64::from(moved) * last_top as f64).round() as i64;
            state.top =
                (i64::try_from(top).unwrap_or(i64::MAX).saturating_add(rows_moved)).clamp(0, last_top as i64) as u64;
            state.fraction = 0.0;
        } else if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let share = ((at.y - track.top()) / track.height()).clamp(0.0, 1.0);
            state.top = (f64::from(share) * last_top as f64) as u64;
        }
        if !response.dragged() {
            state.thumb = None;
        }
    }
    let shown = body.width();
    if total_width > shown {
        let track = Rect::from_min_max(pos2(body.left(), rect.bottom() - BAR), pos2(body.right(), rect.bottom()));
        let thumb_width = (track.width() * shown / total_width).max(20.0);
        let position = state.left / (total_width - shown);
        let thumb = Rect::from_min_size(
            pos2(track.left() + (track.width() - thumb_width) * position, track.top() + 2.0),
            vec2(thumb_width, BAR - 4.0),
        );
        let response = ui.interact(track, id.with("horizontal"), Sense::click_and_drag());
        let color = if response.hovered() || response.dragged() { p.text_secondary } else { p.frame };
        painter.rect_filled(thumb, 3, color);
        if response.dragged() {
            let moved = response.drag_delta().x / (track.width() - thumb_width).max(1.0);
            state.left = (state.left + moved * (total_width - shown)).clamp(0.0, total_width - shown);
        }
    }
}

#[cfg(test)]
mod tests;
