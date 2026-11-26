use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::workspace::{GitWorkspace, RefKind, RefTarget};
use zen_workbench::vcs::workspace_columns::{ColumnSettings, GraphColumn};
use zen_workbench::vcs::workspace_view::{GATHERED_WIDTH, GraphFit, GraphStage, LANES_MARGIN};

use super::super::glyphs::{glyph_branch, glyph_check, glyph_cloud, glyph_graph};
use super::super::{IdePalette, Side, hint};
use super::{AUTHOR, Ask, DATE, LANE, MESSAGE, NARROW_REFS, REFS, SHA, look};
use crate::theme;

pub(super) struct Columns {
    pub(super) refs: (f32, f32),
    pub(super) graph: (f32, f32),
    pub(super) message: (f32, f32),
    pub(super) author: Option<(f32, f32)>,
    pub(super) date: Option<(f32, f32)>,
    pub(super) sha: Option<(f32, f32)>,
    pub(super) shown: Vec<(GraphColumn, (f32, f32))>,
    pub(super) fit: GraphFit,
}

impl Columns {
    pub(super) fn new(rect: Rect, settings: &ColumnSettings, fit: GraphFit) -> Self {
        let graph_width = fit.width;
        let refs_width = settings
            .widths
            .get(&GraphColumn::Refs)
            .copied()
            .unwrap_or(if rect.width() < REFS + graph_width + MESSAGE + DATE { NARROW_REFS } else { REFS });
        let own = |column: GraphColumn, fallback: f32| settings.width(column).unwrap_or(fallback);
        let mut room = rect.width() - refs_width - graph_width - MESSAGE;
        let mut kept = Vec::new();
        for (column, fallback) in [(GraphColumn::Date, DATE), (GraphColumn::Author, AUTHOR), (GraphColumn::Sha, SHA)] {
            let width = own(column, fallback);
            if settings.shown(column) && room >= width {
                room -= width;
                kept.push(column);
            }
        }
        let width = |column: GraphColumn| match column {
            GraphColumn::Refs => Some(refs_width),
            GraphColumn::Graph => Some(graph_width),
            GraphColumn::Message => None,
            GraphColumn::Author => Some(own(column, AUTHOR)),
            GraphColumn::Date => Some(own(column, DATE)),
            GraphColumn::Sha => Some(own(column, SHA)),
        };
        let order: Vec<GraphColumn> =
            settings.order.iter().copied().filter(|column| !column.optional() || kept.contains(column)).collect();
        let fixed: f32 = order.iter().filter_map(|&column| width(column)).sum();
        let message = (rect.width() - fixed).max(0.0);
        let mut x = rect.left();
        let shown: Vec<(GraphColumn, (f32, f32))> = order
            .iter()
            .map(|&column| {
                let left = x;
                x += width(column).unwrap_or(message);
                (column, (left, x))
            })
            .collect();
        let span = |wanted: GraphColumn| shown.iter().find(|(column, _)| *column == wanted).map(|(_, span)| *span);
        Columns {
            refs: span(GraphColumn::Refs).unwrap_or_default(),
            graph: span(GraphColumn::Graph).unwrap_or_default(),
            message: span(GraphColumn::Message).unwrap_or_default(),
            author: span(GraphColumn::Author),
            date: span(GraphColumn::Date),
            sha: span(GraphColumn::Sha),
            shown,
            fit,
        }
    }

    pub(super) fn lane_x(&self, lane: usize) -> f32 {
        self.graph.0 + LANES_MARGIN + lane as f32 * LANE - self.fit.scroll
    }

    pub(super) fn gathered_x(&self) -> f32 {
        match self.fit.stage {
            GraphStage::Dots => (self.graph.0 + self.graph.1) / 2.0,
            _ => self.graph.1 - GATHERED_WIDTH / 2.0,
        }
    }

    pub(super) fn node_x(&self, lane: usize) -> f32 {
        match self.fit.stage {
            GraphStage::Lanes => self.lane_x(lane),
            _ if self.fit.shows(lane) => self.lane_x(lane),
            _ => self.gathered_x(),
        }
    }

    pub(super) fn lanes_room(&self) -> Option<(f32, f32)> {
        match self.fit.stage {
            GraphStage::Lanes => Some(self.graph),
            GraphStage::Gathered => Some((self.graph.0, self.graph.1 - GATHERED_WIDTH)),
            GraphStage::Dots => None,
        }
    }

    pub(super) fn lines(&self) -> Option<(f32, f32)> {
        let fit = &self.fit;
        match fit.stage {
            GraphStage::Lanes => Some(self.graph),
            GraphStage::Gathered if fit.shown_lanes > 0 => {
                let (left, right) = self.lanes_room()?;
                let first = self.lane_x(fit.first_lane) - LANE / 2.0;
                let last = self.lane_x(fit.first_lane + fit.shown_lanes - 1) + LANE / 2.0;
                Some((first.max(left), last.min(right)))
            }
            _ => None,
        }
    }

    pub(super) fn edge(
        &self,
        from: usize,
        to: usize,
        lower: bool,
        rect: Rect,
    ) -> Option<(egui::Pos2, egui::Pos2, (f32, f32))> {
        let lines = self.lines()?;
        let middle = rect.center().y;
        let (from_y, to_y) = if lower { (middle, rect.bottom()) } else { (rect.top(), middle) };
        let (start, end) = (pos2(self.lane_x(from), from_y), pos2(self.lane_x(to), to_y));
        if self.fit.stage == GraphStage::Lanes {
            return Some((start, end, lines));
        }
        let gathered = pos2(self.gathered_x(), middle);
        match (self.fit.shows(from), self.fit.shows(to)) {
            (true, true) => Some((start, end, lines)),
            (false, false) => None,
            (true, false) => Some((start, gathered, (self.graph.0, gathered.x))),
            (false, true) => Some((gathered, end, (self.graph.0, gathered.x))),
        }
    }

    pub(super) fn band(&self, node: f32, rect: Rect) -> (f32, f32) {
        if self.graph.0 <= self.message.0 { (node.min(self.graph.1), rect.right()) } else { (rect.left(), node) }
    }
}

#[derive(Default)]
pub(super) struct ColumnsView {
    dragged: Option<GraphColumn>,
    menu_at: Option<egui::Pos2>,
    pub(super) hover: Option<Hovered>,
    sent: Option<Hovered>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Hovered {
    Branch(RefTarget),
    Commit(String),
}

pub(super) fn header(
    ui: &mut Ui,
    p: &IdePalette,
    header: Rect,
    columns: &Columns,
    view: &mut ColumnsView,
    asks: &mut Vec<Ask>,
) {
    let painter = ui.painter().clone();
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let message_left = columns.message.0;
    for (index, &(column, (left, right))) in columns.shown.iter().enumerate() {
        let title = Rect::from_x_y_ranges(left + 4.0..=(right - 4.0).max(left + 4.0), header.y_range());
        let response = ui.interact(title, ui.scope_id().with(("gw-column-title", column)), Sense::click_and_drag());
        let text = t!(title_key(column));
        let dragged = view.dragged == Some(column);
        let color = if dragged { p.text_secondary.gamma_multiply(0.45) } else { p.text_secondary };
        let whole = painter.layout_job(look::small_caps(&text, 10.5, color)).size().x;
        let iconic = column == GraphColumn::Graph
            && (columns.fit.stage == GraphStage::Dots || whole > title_room((left, right)));
        if iconic {
            let icon = Rect::from_center_size(pos2(columns.gathered_x(), header.center().y), Vec2::splat(13.0));
            let icon = if columns.fit.stage == GraphStage::Dots {
                icon
            } else {
                Rect::from_center_size(pos2((left + right) / 2.0, header.center().y), Vec2::splat(13.0))
            };
            glyph_graph(&painter, icon, color);
            response.clone().on_hover_text(t!(title_key(column)));
        } else {
            column_title(ui, &response, header, (left, right), &text, color);
        }
        if response.drag_started() {
            view.dragged = Some(column);
        }
        if response.secondary_clicked() {
            view.menu_at = response.interact_pointer_pos();
            asks.push(Ask::Command("vcs.workspace.columnsMenu", json!({ "open": true })));
        }
        if column == GraphColumn::Message {
            continue;
        }
        let edge = if left < message_left { right } else { left };
        if edge <= header.left() + 1.0 || (edge >= header.right() - 1.0 && index + 1 == columns.shown.len()) {
            continue;
        }
        let handle = Rect::from_center_size(pos2(edge, header.center().y), vec2(8.0, header.height()));
        let border = ui.interact(handle, ui.scope_id().with(("gw-column-edge", column)), Sense::click_and_drag());
        let active = border.hovered() || border.dragged();
        if active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        painter.vline(edge, header.y_range().shrink(5.0), Stroke::new(1.0, if active { p.accent } else { p.frame }));
        if border.double_clicked() {
            asks.push(Ask::Command("vcs.workspaceColumns.set", json!({ "resetWidths": true })));
        } else if border.dragged()
            && let Some(at) = border.interact_pointer_pos()
        {
            let width = if left < message_left { at.x - left } else { right - at.x };
            asks.push(Ask::Command(
                "vcs.workspaceColumns.set",
                json!({ "widths": { column_name(column): width.round() } }),
            ));
        }
    }
    if let Some(column) = view.dragged {
        let released = ui.input(|input| !input.pointer.any_down());
        if let Some(at) = pointer.or_else(|| ui.input(|input| input.pointer.latest_pos())) {
            let place = drop_place(columns, column, at.x);
            let x = place_x(columns, column, place);
            painter.vline(x, header.y_range(), Stroke::new(2.0, p.accent));
            let galley = painter.layout_job(look::small_caps(&t!(title_key(column)), 10.5, p.text));
            let plate = Rect::from_center_size(pos2(at.x, header.center().y), galley.size() + vec2(16.0, 8.0));
            let layer = egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("gw-column-drag"));
            let floating = ui.ctx().layer_painter(layer);
            floating.rect(plate, 4, p.popup, Stroke::new(1.0, p.popup_border), egui::StrokeKind::Inside);
            floating.galley(plate.center() - galley.size() / 2.0, galley, p.text);
            if released {
                view.dragged = None;
                let order = moved(columns, column, place);
                if let Some(order) = order {
                    asks.push(Ask::Command("vcs.workspaceColumns.set", json!({ "order": order })));
                }
            }
        } else if released {
            view.dragged = None;
        }
    }
    painter.hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0, p.frame));
    if view.menu_at.is_none() {
        view.menu_at = Some(pos2(header.left() + 12.0, header.bottom() + 2.0));
    }
}

pub(super) fn column_title(
    ui: &Ui,
    response: &egui::Response,
    header: Rect,
    (left, right): (f32, f32),
    title: &str,
    color: Color32,
) -> bool {
    let (galley, shortened) = look::small_caps_fitted(ui.painter(), title, 10.5, color, title_room((left, right)));
    ui.painter().with_clip_rect(Rect::from_x_y_ranges(left..=(right - 4.0).max(left), header.y_range())).galley(
        pos2(left + 10.0, header.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if shortened {
        hint(ui, response, &title.to_uppercase(), None, Side::Below);
    }
    shortened
}

fn title_room((left, right): (f32, f32)) -> f32 {
    (right - left - 14.0).max(0.0)
}

pub(super) fn graph_border(ui: &Ui, p: &IdePalette, rows: Rect, columns: &Columns, asks: &mut Vec<Ask>) {
    let (left, right) = columns.graph;
    let outer = left >= columns.message.0;
    let edge = if outer { left } else { right };
    if edge <= rows.left() + 1.0 || edge >= rows.right() - 1.0 {
        return;
    }
    let handle = Rect::from_x_y_ranges(edge - 3.0..=edge + 3.0, rows.y_range());
    let border = ui.interact(handle, ui.scope_id().with("gw-graph-border"), Sense::click_and_drag());
    if border.hovered() || border.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().vline(edge, rows.y_range(), Stroke::new(1.0, p.accent));
    }
    if border.double_clicked() {
        asks.push(Ask::Command("vcs.workspace.view", json!({ "graphStage": "lanes" })));
    } else if border.dragged()
        && let Some(at) = border.interact_pointer_pos()
    {
        let width = if outer { right - at.x } else { at.x - left };
        asks.push(Ask::Command("vcs.workspace.view", json!({ "graphWidth": width.round() })));
    }
}

pub(super) fn graph_scroll(ui: &Ui, p: &IdePalette, rows: Rect, columns: &Columns, asks: &mut Vec<Ask>) {
    let fit = columns.fit;
    let Some((left, right)) = columns.lanes_room().filter(|_| fit.max_scroll > 0.0) else { return };
    let lanes = Rect::from_x_y_ranges(left..=right + 10.0, rows.y_range());
    let mut scroll = fit.scroll;
    super::super::diff_view::scroll_bars(ui, p, &[lanes], &mut scroll, right - left, fit.max_scroll, "gw-graph-hbar");
    let over = ui
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|at| Rect::from_x_y_ranges(columns.graph.0..=columns.graph.1, rows.y_range()).contains(at));
    if over && ui.ui_contains_pointer() {
        let sideways = ui.input(|input| input.smooth_scroll_delta.x);
        scroll = (scroll - sideways).clamp(0.0, fit.max_scroll);
    }
    if (scroll - fit.scroll).abs() >= 0.5 {
        asks.push(Ask::Command("vcs.workspace.view", json!({ "graphScroll": scroll })));
    }
}

fn drop_place(columns: &Columns, dragged: GraphColumn, x: f32) -> usize {
    columns
        .shown
        .iter()
        .filter(|(column, _)| *column != dragged)
        .filter(|(_, (left, right))| (left + right) / 2.0 < x)
        .count()
}

fn place_x(columns: &Columns, dragged: GraphColumn, place: usize) -> f32 {
    let others: Vec<(f32, f32)> =
        columns.shown.iter().filter(|(column, _)| *column != dragged).map(|(_, span)| *span).collect();
    match others.get(place) {
        Some((left, _)) => *left,
        None => others.last().map_or(0.0, |(_, right)| *right),
    }
}

fn moved(columns: &Columns, dragged: GraphColumn, place: usize) -> Option<Vec<GraphColumn>> {
    let mut order: Vec<GraphColumn> =
        columns.shown.iter().map(|(column, _)| *column).filter(|column| *column != dragged).collect();
    order.insert(place.min(order.len()), dragged);
    let now: Vec<GraphColumn> = columns.shown.iter().map(|(column, _)| *column).collect();
    (order != now).then_some(order)
}

fn column_name(column: GraphColumn) -> String {
    serde_json::to_value(column).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

pub(super) fn title_key(column: GraphColumn) -> &'static str {
    match column {
        GraphColumn::Refs => "vcs-workspace-column-refs",
        GraphColumn::Graph => "vcs-workspace-column-graph",
        GraphColumn::Message => "vcs-workspace-column-message",
        GraphColumn::Author => "vcs-workspace-column-author",
        GraphColumn::Date => "vcs-workspace-column-date",
        GraphColumn::Sha => "vcs-workspace-column-sha",
    }
}

pub(super) fn columns_menu(
    ui: &Ui,
    p: &IdePalette,
    settings: &ColumnSettings,
    view: &mut ColumnsView,
    asks: &mut Vec<Ask>,
) {
    let at = view.menu_at.unwrap_or_else(|| ui.max_rect().left_top());
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-columns-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let labels = ["vcs-workspace-columns-highlight", "vcs-workspace-columns-reset", "vcs-workspace-column-author"];
    let widest = labels
        .iter()
        .map(|key| ui.painter().layout_no_wrap(t!(key), theme::regular(13.0), p.text).size().x)
        .fold(0.0, f32::max);
    let width = (widest + 52.0).max(200.0);
    popup.show(|ui| {
        ui.set_width(width);
        for column in [GraphColumn::Author, GraphColumn::Date, GraphColumn::Sha] {
            let shown = settings.shown(column);
            if menu_row(ui, p, width, &t!(title_key(column)), Some(shown)).clicked() {
                let key = if shown { "hide" } else { "show" };
                asks.push(Ask::Command("vcs.workspaceColumns.set", json!({ key: [column] })));
                ui.close();
            }
        }
        ui.separator();
        let on = settings.highlight_on_hover;
        if menu_row(ui, p, width, &t!("vcs-workspace-columns-highlight"), Some(on)).clicked() {
            asks.push(Ask::Command("vcs.workspaceColumns.set", json!({ "highlightOnHover": !on })));
            ui.close();
        }
        ui.separator();
        if menu_row(ui, p, width, &t!("vcs-workspace-columns-reset"), None).clicked() {
            asks.push(Ask::Command("vcs.workspaceColumns.set", json!({ "reset": true })));
            ui.close();
        }
    });
    if !open {
        view.menu_at = None;
        asks.push(Ask::Command("vcs.workspace.columnsMenu", json!({ "open": false })));
    }
}

pub(super) fn menu_row(ui: &mut Ui, p: &IdePalette, width: f32, label: &str, checked: Option<bool>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5, p.selection);
    }
    let check = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(11.0));
    if checked == Some(true) {
        glyph_check(ui.painter(), check, p.text);
    }
    let left = if checked.is_some() { rect.left() + 30.0 } else { rect.left() + 12.0 };
    ui.painter().text(pos2(left, rect.center().y), Align2::LEFT_CENTER, label, theme::regular(13.0), p.text);
    response
}

pub(super) fn follow_hover(
    view: &mut ColumnsView,
    registry: &Registry,
    session: &mut Session,
    hovered: Option<Hovered>,
    graph: u64,
) {
    let current = session.state::<GitWorkspace>().and_then(GitWorkspace::hover).map(|hover| hover.info.graph_revision);
    let stale = hovered.is_some() && current != Some(graph);
    if hovered == view.sent && !stale {
        return;
    }
    view.sent = hovered.clone();
    let params = match hovered {
        Some(Hovered::Branch(branch)) => json!({ "branch": branch }),
        Some(Hovered::Commit(id)) => json!({ "revision": id }),
        None => json!({}),
    };
    let _ = registry.dispatch(session, "vcs.workspace.hover", params);
}

pub(super) fn dim(painter: &egui::Painter, p: &IdePalette, rect: Rect) {
    painter.rect_filled(rect, 0, p.editor_background.gamma_multiply(0.62));
}

pub(super) fn draw_nearest(
    painter: &egui::Painter,
    p: &IdePalette,
    refs: (f32, f32),
    middle: f32,
    branch: &RefTarget,
    lane: Color32,
) {
    const ICON: f32 = 12.0;
    let left = refs.0 + 8.0;
    let room = (refs.1 - 8.0 - left).max(24.0);
    let font = theme::regular(12.0);
    let galley =
        painter.layout_job(super::super::vcs::truncated(&branch.name, font, p.text_secondary, room - ICON - 24.0));
    let width = (galley.size().x + ICON + 24.0).min(room);
    let rect = Rect::from_min_size(pos2(left, middle - 10.0), vec2(width, 20.0));
    painter.rect(rect, 4, p.editor_background, Stroke::new(1.0, lane), egui::StrokeKind::Inside);
    let icon = Rect::from_center_size(pos2(rect.left() + 6.0 + ICON / 2.0, middle), Vec2::splat(ICON));
    match branch.kind {
        RefKind::Remote => glyph_cloud(painter, icon, p.text_secondary),
        _ => glyph_branch(painter, icon, p.text_secondary),
    }
    let size = galley.size();
    painter.with_clip_rect(rect.shrink2(vec2(4.0, 0.0))).galley(
        pos2(icon.right() + 5.0, middle - size.y / 2.0),
        galley,
        p.text_secondary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER_HEIGHT: f32 = 26.0;

    fn rect(width: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 28.0))
    }

    fn fit(width: f32) -> GraphFit {
        GraphFit { width, lanes: 4, shown_lanes: 4, ..GraphFit::default() }
    }

    #[test]
    fn nodes_of_lanes_that_do_not_show_gather_into_one_line_and_lines_stop_before_it() {
        let settings = ColumnSettings::default();
        let width = |w: f32, scroll: f32| Columns::new(rect(1200.0), &settings, GraphFit::new(Some(w), 8, scroll));
        let lanes = width(400.0, 0.0);
        assert_eq!(lanes.node_x(7), lanes.lane_x(7));
        assert_eq!(lanes.lines(), Some(lanes.graph));
        let gathered = width(120.0, 0.0);
        assert_eq!(gathered.node_x(3), gathered.lane_x(3));
        assert_eq!(gathered.node_x(4), gathered.graph.1 - GATHERED_WIDTH / 2.0);
        assert_eq!(gathered.node_x(7), gathered.node_x(4), "one line");
        let (_, lines_end) = gathered.lines().unwrap();
        assert!(gathered.lane_x(3) + super::super::NODE <= lines_end, "the last lane's node is whole");
        assert!(gathered.gathered_x() - GATHERED_WIDTH / 2.0 >= lines_end);
        let scrolled = width(120.0, 20.0);
        assert_eq!(scrolled.node_x(0), scrolled.gathered_x());
        assert_eq!(scrolled.node_x(4), scrolled.lane_x(4));
        assert_eq!(scrolled.lane_x(1), scrolled.graph.0 + LANES_MARGIN, "lane 1 is where lane 0 was");
        let dots = width(40.0, 0.0);
        let middle = (dots.graph.0 + dots.graph.1) / 2.0;
        assert_eq!((dots.node_x(0), dots.node_x(5), dots.lines()), (middle, middle, None));
    }

    #[test]
    fn nodes_stay_on_their_lines_at_every_width_and_scroll() {
        let settings = ColumnSettings::default();
        for lanes in [3, 8, 20] {
            for step in 0..=800 {
                let width = 40.0 + step as f32 * 0.5;
                let max_scroll = GraphFit::new(Some(width), lanes, 0.0).max_scroll;
                for scroll in (0..=(max_scroll as usize)).step_by(5) {
                    let fit = GraphFit::new(Some(width), lanes, scroll as f32);
                    let columns = Columns::new(rect(1200.0), &settings, fit);
                    let lines = columns.lines();
                    for lane in 0..lanes {
                        let x = columns.lane_x(lane);
                        let line_shows = lines.is_some_and(|(left, right)| left <= x && x <= right);
                        let at = || format!("lane {lane} of {lanes}, width {width}, scroll {scroll}");
                        assert_eq!(line_shows, fit.stage == GraphStage::Lanes || fit.shows(lane), "{}", at());
                        let node = columns.node_x(lane);
                        if line_shows {
                            assert_eq!(node, x, "the node is on its line: {}", at());
                        } else {
                            assert_eq!(node, columns.gathered_x(), "the node is in the line of dots: {}", at());
                        }
                    }
                    let row = rect(1200.0);
                    let pairs = if lanes > 8 { 0 } else { lanes };
                    for (from, to, lower) in
                        (0..pairs).flat_map(|a| (0..pairs).flat_map(move |b| [(a, b, false), (a, b, true)]))
                    {
                        let at =
                            || format!("{from} -> {to} (lower {lower}) of {lanes}, width {width}, scroll {scroll}");
                        let Some((start, end, (left, right))) = columns.edge(from, to, lower, row) else {
                            assert!(fit.stage == GraphStage::Dots || !(fit.shows(from) || fit.shows(to)), "{}", at());
                            continue;
                        };
                        for (point, lane) in [(start, from), (end, to)] {
                            if fit.stage == GraphStage::Lanes || fit.shows(lane) {
                                assert_eq!(point.x, columns.lane_x(lane), "{}", at());
                            } else {
                                assert_eq!(
                                    point,
                                    pos2(columns.gathered_x(), row.center().y),
                                    "into the dots: {}",
                                    at()
                                );
                            }
                            assert!(left <= point.x && point.x <= right, "the line is drawn to its end: {}", at());
                        }
                    }
                    if let (Some((_, right)), GraphStage::Gathered) = (lines, fit.stage) {
                        assert!(right <= columns.gathered_x() - GATHERED_WIDTH / 2.0, "width {width}");
                    }
                }
            }
        }
    }

    #[test]
    fn columns_give_the_message_what_is_left() {
        let settings = ColumnSettings::default();
        let columns = Columns::new(rect(1200.0), &settings, fit(100.0));
        assert_eq!(columns.refs, (0.0, REFS));
        assert_eq!(columns.graph, (REFS, REFS + 100.0));
        assert_eq!(columns.sha, Some((1200.0 - SHA, 1200.0)));
        assert_eq!(columns.date, Some((1200.0 - SHA - DATE, 1200.0 - SHA)));
        assert_eq!(columns.message, (REFS + 100.0, 1200.0 - SHA - DATE - AUTHOR));
        assert!(columns.lane_x(1) - columns.lane_x(0) == LANE);
        let medium = Columns::new(rect(800.0), &settings, fit(100.0));
        assert_eq!((medium.sha, medium.author.is_some(), medium.date.is_some()), (None, true, true));
        let narrow = Columns::new(rect(460.0), &settings, fit(100.0));
        assert_eq!((narrow.author, narrow.date, narrow.sha), (None, None, None));
        assert_eq!(narrow.refs, (0.0, NARROW_REFS), "the labels narrow too");
        assert_eq!(narrow.message.1, 460.0);
        let mut set = ColumnSettings::default();
        set.widths.extend([(GraphColumn::Refs, 70.0), (GraphColumn::Graph, 50.0)]);
        let resized = Columns::new(rect(1200.0), &set, fit(50.0));
        assert_eq!((resized.refs, resized.graph), ((0.0, 70.0), (70.0, 120.0)));
        assert_eq!(Columns::new(rect(460.0), &set, fit(50.0)).refs.1, 70.0);
    }

    #[test]
    fn columns_follow_their_order_and_leave_out_those_switched_off() {
        use GraphColumn::*;
        let settings = ColumnSettings {
            order: vec![Graph, Refs, Sha, Message, Author, Date],
            hidden: vec![Author],
            ..ColumnSettings::default()
        };
        let columns = Columns::new(rect(1200.0), &settings, fit(100.0));
        let order: Vec<GraphColumn> = columns.shown.iter().map(|(column, _)| *column).collect();
        assert_eq!(order, [Graph, Refs, Sha, Message, Date]);
        assert_eq!(columns.graph, (0.0, 100.0));
        assert_eq!(columns.refs, (100.0, 100.0 + REFS));
        assert_eq!(columns.sha, Some((100.0 + REFS, 100.0 + REFS + SHA)));
        assert_eq!(columns.author, None);
        assert_eq!(columns.message, (100.0 + REFS + SHA, 1200.0 - DATE));
        for pair in columns.shown.windows(2) {
            assert_eq!(pair[0].1.1, pair[1].1.0);
        }
        assert!(Columns::new(rect(800.0), &settings, fit(100.0)).sha.is_some());
        assert_eq!(columns.band(20.0, rect(1200.0)), (20.0, 1200.0));
        let message_first = ColumnSettings { order: vec![Message, Graph], ..ColumnSettings::default() };
        let columns = Columns::new(rect(1200.0), &message_first, fit(100.0));
        assert_eq!(columns.band(columns.lane_x(0), rect(1200.0)), (0.0, columns.lane_x(0)));
    }

    fn ftl_title(source: &str, key: &str) -> String {
        source
            .lines()
            .find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))
            .unwrap_or_else(|| panic!("{key}"))
            .trim()
            .to_owned()
    }

    #[test]
    fn a_title_that_does_not_fit_ends_in_an_ellipsis_with_the_whole_title_in_its_hint() {
        let languages = [include_str!("../../../locales/en/ui.ftl"), include_str!("../../../locales/ru/ui.ftl")];
        let mut screen = crate::offscreen::Offscreen::new(vec2(600.0, HEADER_HEIGHT), 1.0);
        let mut seen = Vec::new();
        for source in languages {
            for (key, wide) in [
                ("vcs-workspace-column-refs", NARROW_REFS),
                ("vcs-workspace-column-message", MESSAGE),
                ("vcs-workspace-column-author", AUTHOR),
                ("vcs-workspace-column-date", DATE),
                ("vcs-workspace-column-sha", SHA),
                ("vcs-rebase-column-action", 120.0),
            ] {
                let title = ftl_title(source, key);
                for width in [wide, 30.0] {
                    let span = (20.0, 20.0 + width);
                    let mut drawn = None;
                    let tree = screen.accessibility_tree(|ui| {
                        let header = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, HEADER_HEIGHT));
                        let rect = Rect::from_x_y_ranges(span.0..=span.1, header.y_range());
                        let response = ui.interact(rect, ui.scope_id().with("title"), Sense::hover());
                        let shortened = column_title(ui, &response, header, span, &title, Color32::WHITE);
                        let (galley, _) =
                            look::small_caps_fitted(ui.painter(), &title, 10.5, Color32::WHITE, title_room(span));
                        let last = galley.rows.iter().flat_map(|row| row.glyphs.iter()).last().map(|glyph| glyph.chr);
                        drawn = Some((shortened, last, galley.size().x));
                    });
                    let (shortened, last, drawn_width) = drawn.unwrap();
                    let hinted = tree.nodes.iter().any(|(_, node)| node.label() == Some(title.to_uppercase().as_str()));
                    seen.push((title.clone(), width, shortened, last, drawn_width, hinted));
                }
            }
        }
        for (title, width, shortened, last, drawn_width, hinted) in &seen {
            assert!(*drawn_width <= title_room((0.0, *width)) + 0.5, "{title} in {width}: {drawn_width}");
            if *width == 30.0 {
                assert!(*shortened, "{title} is cut in {width} points");
            }
            if *shortened {
                assert_eq!(*last, Some('…'), "{title} in {width} ends in an ellipsis");
                assert!(*hinted, "{title} in {width}: the hint has it whole");
            } else {
                assert!(!*hinted, "{title} in {width}: no hint when whole");
            }
        }
        let whole: Vec<_> = seen.iter().filter(|seen| seen.1 != 30.0 && seen.2).collect();
        assert!(whole.is_empty(), "titles cut at their own widths: {whole:?}");
    }

    #[test]
    fn a_title_dragged_moves_its_column() {
        use GraphColumn::*;
        let columns = Columns::new(rect(1200.0), &ColumnSettings::default(), fit(100.0));
        assert_eq!(drop_place(&columns, Sha, 5.0), 0);
        assert_eq!(moved(&columns, Sha, 0), Some(vec![Sha, Refs, Graph, Message, Author, Date]));
        let place = drop_place(&columns, Graph, columns.graph.0 + 10.0);
        assert_eq!(moved(&columns, Graph, place), None);
        let place = drop_place(&columns, Refs, (columns.graph.0 + columns.graph.1) / 2.0 + 1.0);
        assert_eq!(moved(&columns, Refs, place), Some(vec![Graph, Refs, Message, Author, Date, Sha]));
        assert_eq!(place_x(&columns, Refs, place), columns.message.0);
        assert_eq!(column_name(Sha), "sha");
    }
}
