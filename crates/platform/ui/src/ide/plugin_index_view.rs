use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::glyphs::*;
use super::plugin_tool_window::WindowInfo;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const FETCH_EVERY: f64 = 1.0;
const STATE_EVERY: f64 = 30.0;
const STATE_BACKOFF: f64 = 120.0;
const DETAILS: f32 = 128.0;
const BOX: Vec2 = vec2(170.0, 38.0);
const GAP: Vec2 = vec2(64.0, 14.0);
const ROW: f32 = 24.0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GraphNode {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) namespace: String,
    pub(super) path: String,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) detail: String,
    pub(super) problem: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Cluster {
    pub(super) from: String,
    pub(super) columns: Vec<(String, String)>,
    pub(super) errors: Vec<String>,
    pub(super) rows: BTreeMap<String, (String, Vec<String>)>,
    pub(super) failed: bool,
}

pub(super) fn cluster_of(answer: &Value) -> Cluster {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let cluster = &answer["cluster"];
    let rows = answer["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["cluster"].is_object())
        .map(|row| {
            let columns = row["cluster"]["columns"].as_array().into_iter().flatten().map(text).collect();
            (text(&row["id"]), (text(&row["cluster"]["state"]), columns))
        })
        .collect();
    Cluster {
        from: text(&cluster["from"]),
        columns: cluster["columns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|column| (text(&column["id"]), text(&column["title"])))
            .collect(),
        errors: cluster["errors"].as_array().into_iter().flatten().map(text).collect(),
        rows,
        failed: cluster["retryAfterMs"].is_u64(),
    }
}

fn state_color(p: &IdePalette, state: &str) -> Option<Color32> {
    match state {
        "error" => Some(p.error_text),
        "progressing" => Some(p.warning_text),
        "ok" => Some(p.run),
        "unknown" => Some(p.text_secondary),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GraphEdge {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Graph {
    pub(super) nodes: Vec<GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
    pub(super) kinds: Vec<String>,
    pub(super) indexing: bool,
    pub(super) truncated: bool,
}

const TREE_EDGES: [&str; 7] = ["path", "child", "resource", "source", "chart", "project", "dependsOn"];

const CONTAINED_BY: [&str; 3] = ["path", "child", "resource"];

pub(super) fn graph_of(answer: &Value) -> Graph {
    let index = &answer["indexes"][0];
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let problems: Vec<(String, usize, usize, bool)> = index["problems"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|problem| {
            (
                text(&problem["path"]),
                problem["start"].as_u64().unwrap_or(0) as usize,
                problem["end"].as_u64().unwrap_or(0) as usize,
                problem["severity"] == "error",
            )
        })
        .collect();
    let nodes = index["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|node| node["inGraph"] != false)
        .map(|node| {
            let (path, start, end) = (
                text(&node["path"]),
                node["start"].as_u64().unwrap_or(0) as usize,
                node["end"].as_u64().unwrap_or(0) as usize,
            );
            let problem = problems
                .iter()
                .filter(|(file, from, to, _)| *file == path && start <= *from && *to <= end && end > start)
                .map(|(_, _, _, error)| if *error { 2 } else { 1 })
                .max()
                .unwrap_or(0);
            GraphNode {
                id: text(&node["id"]),
                kind: text(&node["kind"]),
                name: text(&node["name"]),
                namespace: text(&node["namespace"]),
                path,
                start,
                end,
                detail: text(&node["detail"]),
                problem,
            }
        })
        .collect();
    let edges = index["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|edge| GraphEdge { from: text(&edge["from"]), to: text(&edge["to"]), kind: text(&edge["kind"]) })
        .collect();
    let kinds = index["kinds"].as_array().into_iter().flatten().map(text).collect();
    Graph { nodes, edges, kinds, indexing: answer["indexing"] == true, truncated: index["truncated"] == true }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TreeRow {
    pub(super) id: String,
    pub(super) depth: usize,
    pub(super) children: bool,
    pub(super) edge: Option<String>,
}

fn matches(node: &GraphNode, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || [&node.name, &node.kind, &node.path, &node.namespace]
            .iter()
            .any(|field| field.to_lowercase().contains(&filter))
}

pub(super) fn tree_rows(graph: &Graph, expanded: &BTreeSet<String>, filter: &str) -> Vec<TreeRow> {
    let known: BTreeSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    if !filter.trim().is_empty() {
        return graph
            .nodes
            .iter()
            .filter(|node| matches(node, filter))
            .map(|node| TreeRow { id: node.id.clone(), depth: 0, children: false, edge: None })
            .collect();
    }
    let children = |id: &str| -> Vec<(&str, &str)> {
        let mut found: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.from == id && known.contains(edge.to.as_str()) && TREE_EDGES.contains(&edge.kind.as_str())
            })
            .map(|edge| (edge.to.as_str(), edge.kind.as_str()))
            .collect();
        found.sort_by_key(|(_, kind)| TREE_EDGES.iter().position(|known| known == kind));
        found
    };
    let contained: BTreeSet<&str> = graph
        .edges
        .iter()
        .filter(|edge| CONTAINED_BY.contains(&edge.kind.as_str()) && known.contains(edge.from.as_str()))
        .map(|edge| edge.to.as_str())
        .collect();
    let mut roots: Vec<&GraphNode> = graph.nodes.iter().filter(|node| !contained.contains(node.id.as_str())).collect();
    if roots.is_empty() {
        roots = graph.nodes.iter().collect();
    }
    roots.sort_by_key(|node| {
        (node.kind != "Kustomization" && node.kind != "Application", node.kind.clone(), node.name.clone())
    });
    let mut rows = Vec::new();
    fn walk<'a>(
        id: &'a str,
        edge: Option<&str>,
        depth: usize,
        stack: &mut Vec<&'a str>,
        rows: &mut Vec<TreeRow>,
        expanded: &BTreeSet<String>,
        children: &dyn Fn(&str) -> Vec<(&'a str, &'a str)>,
    ) {
        let below: Vec<(&str, &str)> = children(id).into_iter().filter(|(child, _)| !stack.contains(child)).collect();
        rows.push(TreeRow { id: id.to_owned(), depth, children: !below.is_empty(), edge: edge.map(str::to_owned) });
        if rows.len() > 20_000 || depth > 32 || !expanded.contains(id) {
            return;
        }
        stack.push(id);
        for (child, kind) in below {
            walk(child, Some(kind), depth + 1, stack, rows, expanded, children);
        }
        stack.pop();
    }
    for root in roots {
        let mut stack = Vec::new();
        walk(root.id.as_str(), None, 0, &mut stack, &mut rows, expanded, &children);
    }
    rows
}

fn flows(edge: &GraphEdge) -> (&str, &str) {
    match edge.kind.as_str() {
        "source" | "chart" | "project" | "dependsOn" => (edge.to.as_str(), edge.from.as_str()),
        _ => (edge.from.as_str(), edge.to.as_str()),
    }
}

pub(super) fn layers(graph: &Graph) -> BTreeMap<String, (usize, usize)> {
    let known: BTreeSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let mut after: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &graph.edges {
        let (from, to) = flows(edge);
        if known.contains(from) && known.contains(to) && from != to {
            after.entry(to).or_default().push(from);
        }
    }
    let mut layer: BTreeMap<&str, usize> = BTreeMap::new();
    fn depth<'a>(
        id: &'a str,
        after: &BTreeMap<&'a str, Vec<&'a str>>,
        layer: &mut BTreeMap<&'a str, usize>,
        visiting: &mut BTreeSet<&'a str>,
    ) -> usize {
        if let Some(known) = layer.get(id) {
            return *known;
        }
        if !visiting.insert(id) || visiting.len() > 64 {
            return 0;
        }
        let found = after
            .get(id)
            .map(|before| before.iter().map(|before| depth(before, after, layer, visiting) + 1).max().unwrap_or(0))
            .unwrap_or(0);
        visiting.remove(id);
        layer.insert(id, found);
        found
    }
    for node in &graph.nodes {
        let mut visiting = BTreeSet::new();
        depth(node.id.as_str(), &after, &mut layer, &mut visiting);
    }
    let mut columns: BTreeMap<usize, Vec<&GraphNode>> = BTreeMap::new();
    for node in &graph.nodes {
        columns.entry(layer.get(node.id.as_str()).copied().unwrap_or(0)).or_default().push(node);
    }
    let mut placed = BTreeMap::new();
    for (column, mut nodes) in columns {
        nodes.sort_by_key(|node| (node.kind.clone(), node.namespace.clone(), node.name.clone()));
        for (order, node) in nodes.into_iter().enumerate() {
            placed.insert(node.id.clone(), (column, order));
        }
    }
    placed
}

pub(super) fn connected(graph: &Graph, id: &str) -> BTreeSet<String> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.from == id || edge.to == id)
        .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Tree,
    Graph,
}

#[derive(Default)]
pub(super) struct IndexView {
    graph: Graph,
    next: f64,
    mode: Mode,
    selected: Option<String>,
    expanded: BTreeSet<String>,
    seeded: bool,
    filter: String,
    pan: Vec2,
    zoom: f32,
    fitted: bool,
    error: Option<String>,
    cluster: Cluster,
    next_state: f64,
}

impl IdeView {
    pub(super) fn follow_index_section(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        window: &WindowInfo,
        now: f64,
    ) {
        let view = self.index_views.entry(window.tool).or_default();
        if now < view.next {
            return;
        }
        view.next = now + FETCH_EVERY;
        if let Some(section) = window.sections.iter().find(|section| section.index && section.state) {
            let refresh = now >= view.next_state;
            let params = json!({
                "plugin": window.plugin, "window": window.window, "section": section.id,
                "refresh": refresh, "waitMs": 0,
            });
            if let Ok(answer) = registry.dispatch(session, "plugins.toolWindow.rows", params) {
                view.cluster = cluster_of(&answer);
            }
            if refresh {
                view.next_state = now + if view.cluster.failed { STATE_BACKOFF } else { STATE_EVERY };
            }
        }
        match registry.dispatch(session, "plugins.index.get", json!({ "plugin": window.plugin })) {
            Ok(answer) => {
                let graph = graph_of(&answer);
                if graph.nodes != view.graph.nodes || graph.edges != view.graph.edges {
                    view.fitted = false;
                }
                if !view.seeded && !graph.nodes.is_empty() {
                    view.seeded = true;
                    view.expanded = tree_rows(&graph, &BTreeSet::new(), "").into_iter().map(|row| row.id).collect();
                }
                view.graph = graph;
                view.error = None;
            }
            Err(error) => view.error = Some(error.to_string()),
        }
    }

    fn open_index_node(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, node: &GraphNode) {
        let Some(root) = session
            .state::<zen_workbench::Workbench>()
            .and_then(zen_workbench::Workbench::project)
            .map(|root| root.to_path_buf())
        else {
            return;
        };
        let path = root.join(&node.path);
        if node.start == 0 && node.end == 0 {
            let target = (super::select_in::Target::ProjectView, path.as_path());
            if let Err(super::agent::ActionError::Refused(message)) =
                self.select_in_target(ctx, registry, session, target, &json!({}), false)
            {
                self.post_at(ctx, message);
            }
            return;
        }
        let offset =
            std::fs::read_to_string(&path).map(|text| text[..node.start.min(text.len())].chars().count()).unwrap_or(0);
        if let Err(error) = registry.dispatch(session, "navigate.place.goTo", json!({ "path": path, "offset": offset }))
        {
            self.post_at(ctx, error.to_string());
        }
    }

    pub(super) fn plugin_index_section(
        &mut self,
        ui: &mut Ui,
        window: &WindowInfo,
        registry: &Registry,
        session: &mut Session,
    ) {
        let p = self.palette;
        let tool = window.tool;
        let mut opened: Option<GraphNode> = None;
        let mut acted: Option<(String, String)> = None;
        let state_section = window.sections.iter().find(|section| section.index && section.state).cloned();
        let refresh;
        let hide;
        {
            let view = self.index_views.entry(tool).or_default();
            let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
            let title = ui.painter().text(
                pos2(header.left() + 12.0, header.center().y),
                Align2::LEFT_CENTER,
                &window.title,
                theme::semibold(13.0),
                p.text,
            );
            accessibility::heading(ui, ui.scope_id().with(("index-title", tool)), title, || window.title.clone());
            let mut x = title.right() + 14.0;
            for (mode, label) in [(Mode::Tree, t!("ide-index-tree")), (Mode::Graph, t!("ide-index-graph"))] {
                let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(12.5), p.text);
                let rect = Rect::from_min_size(pos2(x, header.center().y - 11.0), vec2(galley.size().x + 14.0, 22.0));
                x = rect.right() + 4.0;
                let response =
                    ui.interact(rect, ui.scope_id().with(("index-mode", tool, label.as_str())), Sense::click());
                if view.mode == mode {
                    ui.painter().rect_filled(rect, 5, p.selection);
                } else {
                    hover(ui, &response, &p);
                }
                ui.painter().galley(pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
                accessibility::button(&response, &label);
                accessibility::pressed(&response, view.mode == mode);
                if response.clicked() {
                    view.mode = mode;
                }
            }
            let rect = Rect::from_center_size(pos2(x + 16.0, header.center().y), Vec2::splat(24.0));
            let response = ui.interact(rect, ui.scope_id().with(("index-refresh", tool)), Sense::click());
            hover(ui, &response, &p);
            glyph_refresh(ui.painter(), rect.shrink(6.0), p.text_secondary);
            hint(ui, &response, &t!("ide-plugin-tool-refresh"), None, Side::Below);
            accessibility::button(&response, &t!("ide-plugin-tool-refresh"));
            refresh = response.clicked();
            let rect = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
            let response = ui.interact(rect, ui.scope_id().with(("index-hide", tool)), Sense::click());
            hover(ui, &response, &p);
            glyph_minus(ui.painter(), rect.shrink(6.0), p.text_secondary);
            hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
            accessibility::button(&response, &t!("ide-run-hide"));
            hide = response.clicked();

            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            ui.scope_builder(egui::UiBuilder::new().max_rect(line.shrink2(vec2(12.0, 3.0))), |ui| {
                let field = egui::TextEdit::singleline(&mut view.filter)
                    .hint_text(t!("ide-index-filter"))
                    .desired_width(ui.available_width());
                let response = ui.add(field);
                accessibility::edit(&response, |node| node.set_label(t!("ide-index-filter")));
            });
            let note = if let Some(error) = &view.error {
                Some((error.clone(), p.error_text))
            } else if view.graph.nodes.is_empty() && view.graph.indexing {
                Some((t!("ide-index-indexing"), p.text_secondary))
            } else if view.graph.nodes.is_empty() {
                Some((t!("ide-index-empty"), p.text_secondary))
            } else if view.graph.truncated {
                Some((t!("ide-index-truncated"), p.warning_text))
            } else {
                None
            };
            if let Some((note, color)) = note {
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                ui.painter().with_clip_rect(line).text(
                    pos2(line.left() + 12.0, line.center().y),
                    Align2::LEFT_CENTER,
                    note,
                    theme::regular(12.0),
                    color,
                );
            }
            let mut body = ui.available_rect_before_wrap();
            ui.allocate_rect(body, Sense::hover());
            if let Some(section) = &state_section {
                let details = Rect::from_min_max(pos2(body.left(), body.bottom() - DETAILS), body.max);
                body.max.y = details.top();
                acted = cluster_details(ui, &p, tool, view, section, details);
            }
            let picked = match view.mode {
                Mode::Tree => tree(ui, &p, tool, view, body),
                Mode::Graph => graph(ui, &p, tool, view, body),
            };
            if let Some(id) = picked {
                opened = view.graph.nodes.iter().find(|node| node.id == id).cloned();
            }
        }
        if refresh {
            let _ = registry.dispatch(session, "plugins.index.refresh", json!({ "plugin": window.plugin }));
            if let Some(section) = window.sections.iter().find(|section| section.index && section.state) {
                let params = json!({
                    "plugin": window.plugin, "window": window.window, "section": section.id,
                    "refresh": true, "force": true, "waitMs": 0,
                });
                let _ = registry.dispatch(session, "plugins.toolWindow.rows", params);
            }
            let view = self.index_views.entry(tool).or_default();
            view.next = 0.0;
        }
        if let Some((action, id)) = acted {
            self.act_on_plugin_rows(ui, registry, session, (tool, action, vec![id]), false);
        }
        if hide {
            self.hide_tool(tool);
        }
        if let Some(node) = opened {
            self.open_index_node(ui.ctx(), registry, session, &node);
        }
    }
}

fn disclosure(painter: &egui::Painter, rect: Rect, open: bool, color: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.28;
    let points = if open {
        vec![pos2(c.x - r, c.y - r * 0.5), pos2(c.x + r, c.y - r * 0.5), pos2(c.x, c.y + r * 0.7)]
    } else {
        vec![pos2(c.x - r * 0.5, c.y - r), pos2(c.x + r * 0.7, c.y), pos2(c.x - r * 0.5, c.y + r)]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn problem_color(p: &IdePalette, problem: u8) -> Option<Color32> {
    match problem {
        2 => Some(p.error_text),
        1 => Some(p.warning_text),
        _ => None,
    }
}

fn cluster_details(
    ui: &mut Ui,
    p: &IdePalette,
    tool: &str,
    view: &IndexView,
    section: &super::plugin_tool_window::SectionInfo,
    rect: Rect,
) -> Option<(String, String)> {
    let painter = ui.painter().with_clip_rect(rect);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, p.button_border));
    let mut y = rect.top() + 14.0;
    let line = |y: &mut f32, text: String, color: Color32| {
        painter.text(pos2(rect.left() + 12.0, *y), Align2::LEFT_CENTER, text, theme::regular(12.0), color);
        *y += 18.0;
    };
    let cluster = &view.cluster;
    let from = if cluster.from.is_empty() { t!("ide-index-state-none") } else { cluster.from.clone() };
    line(&mut y, from, p.text_secondary);
    for error in &cluster.errors {
        line(&mut y, error.lines().next().unwrap_or_default().to_owned(), p.error_text);
    }
    let selected = view.selected.as_deref()?;
    let node = view.graph.nodes.iter().find(|node| node.id == selected)?;
    let Some((state, values)) = cluster.rows.get(selected) else {
        if !cluster.from.is_empty() {
            line(&mut y, t!("ide-index-state-missing", kind = node.kind.as_str(), name = node.name.as_str()), p.text);
        }
        return None;
    };
    let mut parts = Vec::new();
    for ((_, title), value) in cluster.columns.iter().zip(values) {
        if !title.is_empty() && !value.is_empty() {
            parts.push(format!("{title}: {value}"));
        }
    }
    let color = state_color(p, state).unwrap_or(p.text);
    line(&mut y, format!("{} {} · {}", node.kind, node.name, parts.join(" · ")), color);
    let mut x = rect.left() + 12.0;
    let mut acted = None;
    for action in section.actions.iter().filter(|action| action.rows) {
        let galley = ui.painter().layout_no_wrap(action.title.clone(), theme::regular(12.0), p.link);
        let button = Rect::from_min_size(pos2(x, y - 9.0), vec2(galley.size().x + 12.0, 20.0));
        x = button.right() + 6.0;
        let response = ui.interact(button, ui.scope_id().with(("index-act", tool, action.id.as_str())), Sense::click());
        hover(ui, &response, p);
        painter.galley(pos2(button.left() + 6.0, button.center().y - galley.size().y / 2.0), galley, p.link);
        accessibility::button(&response, &action.title);
        if response.clicked() {
            acted = Some((action.id.clone(), selected.to_owned()));
        }
    }
    acted
}

fn tree(ui: &mut Ui, p: &IdePalette, tool: &str, view: &mut IndexView, body: Rect) -> Option<String> {
    let rows = tree_rows(&view.graph, &view.expanded, &view.filter);
    let by_id: BTreeMap<&str, &GraphNode> = view.graph.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let near = view.selected.as_deref().map(|id| connected(&view.graph, id)).unwrap_or_default();
    let mut picked = None;
    let mut toggle = None;
    let mut select = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical().id_salt(("index-tree", tool)).auto_shrink([false, false]).show_rows(
            ui,
            ROW,
            rows.len(),
            |ui, range| {
                for row in &rows[range] {
                    let Some(node) = by_id.get(row.id.as_str()) else { continue };
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let selected = view.selected.as_deref() == Some(row.id.as_str());
                    if selected {
                        ui.painter().rect_filled(rect, 0, p.selection);
                    } else if near.contains(&row.id) {
                        ui.painter().rect_filled(rect, 0, p.symbol_highlight);
                    } else {
                        hover(ui, &response, p);
                    }
                    let left = rect.left() + 10.0 + row.depth as f32 * 16.0;
                    if row.children {
                        let arrow = Rect::from_center_size(pos2(left + 6.0, rect.center().y), Vec2::splat(12.0));
                        disclosure(ui.painter(), arrow, view.expanded.contains(&row.id), p.text_secondary);
                        if response.clicked()
                            && ui.input(|input| input.pointer.interact_pos()).is_some_and(|at| at.x < left + 14.0)
                        {
                            toggle = Some(row.id.clone());
                        }
                    }
                    let mut x = left + 18.0;
                    if let Some(color) = problem_color(p, node.problem) {
                        ui.painter().circle_filled(pos2(x + 3.0, rect.center().y), 3.5, color);
                        x += 10.0;
                    }
                    if let Some(color) = view.cluster.rows.get(&row.id).and_then(|(state, _)| state_color(p, state)) {
                        ui.painter().circle_stroke(pos2(x + 3.0, rect.center().y), 3.5, Stroke::new(1.5, color));
                        x += 10.0;
                    }
                    let edge = row.edge.as_deref().map(|edge| format!("{edge} → ")).unwrap_or_default();
                    let kind = ui.painter().text(
                        pos2(x, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("{edge}{}", node.kind),
                        theme::regular(12.0),
                        p.text_secondary,
                    );
                    let name = if node.namespace.is_empty() {
                        node.name.clone()
                    } else {
                        format!("{}/{}", node.namespace, node.name)
                    };
                    ui.painter().text(
                        pos2(kind.right() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &name,
                        theme::regular(13.0),
                        p.text,
                    );
                    accessibility::item(&response, egui::Role::TreeItem, selected, &format!("{} {name}", node.kind));
                    if response.double_clicked() {
                        picked = Some(row.id.clone());
                    } else if response.clicked() && toggle.is_none() {
                        select = Some(row.id.clone());
                    }
                    if !node.path.is_empty() {
                        response.on_hover_text(if node.detail.is_empty() {
                            node.path.clone()
                        } else {
                            format!("{}\n{}", node.path, node.detail)
                        });
                    }
                }
            },
        );
    });
    if let Some(id) = toggle
        && !view.expanded.remove(&id)
    {
        view.expanded.insert(id);
    }
    if let Some(id) = select {
        view.selected = Some(id);
    }
    if ui.input(|input| input.key_pressed(egui::Key::Enter)) && ui.rect_contains_pointer(body) {
        picked = picked.or_else(|| view.selected.clone());
    }
    picked
}

fn graph(ui: &mut Ui, p: &IdePalette, tool: &str, view: &mut IndexView, body: Rect) -> Option<String> {
    let placed = layers(&view.graph);
    let world = |(column, order): (usize, usize)| pos2(column as f32 * (BOX.x + GAP.x), order as f32 * (BOX.y + GAP.y));
    let bounds = placed.values().fold(Rect::NOTHING, |bounds, at| bounds.union(Rect::from_min_size(world(*at), BOX)));
    if !view.fitted && bounds.is_positive() {
        let zoom =
            (body.width() / (bounds.width() + 40.0)).min(body.height() / (bounds.height() + 40.0)).clamp(0.25, 1.0);
        view.zoom = zoom;
        view.pan = vec2(20.0, 20.0) - bounds.min.to_vec2() * zoom;
        view.fitted = true;
    }
    let zoom = if view.zoom > 0.0 { view.zoom } else { 1.0 };
    let response = ui.interact(body, ui.scope_id().with(("index-graph", tool)), Sense::click_and_drag());
    if response.dragged() {
        view.pan += response.drag_delta();
    }
    if ui.rect_contains_pointer(body) {
        let (scroll, factor) = ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta()));
        view.pan += scroll;
        if factor != 1.0
            && let Some(at) = ui.input(|input| input.pointer.hover_pos())
        {
            let before = (at - body.min - view.pan) / zoom;
            view.zoom = (zoom * factor).clamp(0.2, 2.5);
            view.pan = at - body.min - before * view.zoom;
        }
    }
    let zoom = if view.zoom > 0.0 { view.zoom } else { 1.0 };
    let screen = |at: (usize, usize)| body.min + view.pan + world(at).to_vec2() * zoom;
    let painter = ui.painter().with_clip_rect(body);
    let near = view.selected.as_deref().map(|id| connected(&view.graph, id)).unwrap_or_default();
    for edge in &view.graph.edges {
        let (from, to) = flows(edge);
        let (Some(a), Some(b)) = (placed.get(from), placed.get(to)) else { continue };
        let start = screen(*a) + vec2(BOX.x * zoom, BOX.y * zoom / 2.0);
        let end = screen(*b) + vec2(0.0, BOX.y * zoom / 2.0);
        let lit = view.selected.as_deref().is_some_and(|id| edge.from == id || edge.to == id);
        let color = if lit { p.accent } else { p.text_secondary.gamma_multiply(0.5) };
        let stroke = Stroke::new(if lit { 2.0 } else { 1.0 }, color);
        if edge.kind == "dependsOn" {
            painter.add(egui::Shape::dashed_line(&[start, end], stroke, 6.0, 4.0));
        } else {
            painter.line_segment([start, end], stroke);
        }
        painter.circle_filled(end, 2.5 * zoom.max(0.6), color);
    }
    let mut picked = None;
    let mut clicked_node = false;
    for node in &view.graph.nodes {
        let Some(at) = placed.get(&node.id) else { continue };
        let rect = Rect::from_min_size(screen(*at), BOX * zoom);
        if !rect.intersects(body) {
            continue;
        }
        let id = ui.scope_id().with(("index-node", tool, node.id.as_str()));
        let response = ui.interact(rect.intersect(body), id, Sense::click());
        let selected = view.selected.as_deref() == Some(node.id.as_str());
        let fill = if selected {
            p.selection
        } else if near.contains(&node.id) {
            p.symbol_highlight
        } else {
            p.island
        };
        painter.rect_filled(rect, 6.0 * zoom, fill);
        let border = problem_color(p, node.problem).unwrap_or(if selected { p.accent } else { p.button_border });
        painter.rect_stroke(rect, 6.0 * zoom, Stroke::new(1.0, border), egui::StrokeKind::Inside);
        if let Some(color) = view.cluster.rows.get(&node.id).and_then(|(state, _)| state_color(p, state)) {
            let dot = pos2(rect.right() - 9.0 * zoom, rect.top() + 9.0 * zoom);
            painter.circle_filled(dot, 4.0 * zoom.max(0.6), color);
        }
        if zoom >= 0.45 {
            let inner = rect.shrink2(vec2(8.0 * zoom, 4.0 * zoom));
            painter.with_clip_rect(rect.intersect(body)).text(
                pos2(inner.left(), inner.top()),
                Align2::LEFT_TOP,
                &node.kind,
                theme::regular(10.5 * zoom),
                p.text_secondary,
            );
            painter.with_clip_rect(rect.intersect(body)).text(
                pos2(inner.left(), inner.bottom()),
                Align2::LEFT_BOTTOM,
                &node.name,
                theme::semibold(12.5 * zoom),
                p.text,
            );
        }
        accessibility::item(&response, egui::Role::ListItem, selected, &format!("{} {}", node.kind, node.name));
        if response.double_clicked() {
            picked = Some(node.id.clone());
            clicked_node = true;
        } else if response.clicked() {
            view.selected = Some(node.id.clone());
            clicked_node = true;
        }
        let path = node.path.clone();
        response.on_hover_text(if node.detail.is_empty() { path } else { format!("{path}\n{}", node.detail) });
    }
    if response.clicked() && !clicked_node {
        view.selected = None;
    }
    if ui.input(|input| input.key_pressed(egui::Key::Enter)) && ui.rect_contains_pointer(body) {
        picked = picked.or_else(|| view.selected.clone());
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Graph {
        let answer = json!({
            "indexing": false,
            "indexes": [{
                "kinds": ["flux"],
                "nodes": [
                    { "id": "repo", "kind": "GitRepository", "name": "flux-system", "path": "sync.yaml", "start": 0, "end": 40 },
                    { "id": "root", "kind": "Kustomization", "name": "flux-system", "path": "sync.yaml", "start": 41, "end": 90 },
                    { "id": "apps", "kind": "Kustomization", "name": "apps", "path": "apps.yaml", "start": 0, "end": 50 },
                    { "id": "infra", "kind": "Kustomization", "name": "infra", "path": "apps.yaml", "start": 51, "end": 99 },
                    { "id": "path:apps", "kind": "Folder", "name": "apps", "path": "apps", "start": 0, "end": 0 },
                    { "id": "release", "kind": "HelmRelease", "name": "web", "path": "apps/r.yaml", "start": 0, "end": 30 },
                    { "id": "value:replicas", "kind": "Value", "name": "replicas", "path": "apps/values.yaml", "start": 0, "end": 8, "inGraph": false }
                ],
                "edges": [
                    { "from": "root", "to": "repo", "kind": "source" },
                    { "from": "apps", "to": "repo", "kind": "source" },
                    { "from": "apps", "to": "infra", "kind": "dependsOn" },
                    { "from": "apps", "to": "path:apps", "kind": "path" },
                    { "from": "path:apps", "to": "release", "kind": "resource" },
                    { "from": "infra", "to": "infra-gone", "kind": "dependsOn" }
                ],
                "problems": [{ "path": "apps.yaml", "start": 60, "end": 70, "severity": "error", "message": "x" }]
            }]
        });
        graph_of(&answer)
    }

    #[test]
    fn a_rows_answer_gives_each_node_its_cluster_state_and_where_it_came_from() {
        let answer = json!({
            "rows": [
                { "id": "apps", "columns": [], "cluster": { "state": "error", "columns": ["False", "boom"] } },
                { "id": "infra", "columns": [] }
            ],
            "cluster": {
                "from": "State from prod", "values": { "kubernetes.context": "prod" },
                "columns": [{ "id": "ready", "title": "Ready" }, { "id": "message", "title": "Message" }],
                "errors": ["no Argo CD"]
            }
        });
        let cluster = cluster_of(&answer);
        assert_eq!(cluster.from, "State from prod");
        assert_eq!(cluster.rows.get("apps"), Some(&("error".to_owned(), vec!["False".to_owned(), "boom".to_owned()])));
        assert!(!cluster.rows.contains_key("infra"));
        assert_eq!(cluster.errors, ["no Argo CD"]);
        assert!(!cluster.failed);
        assert!(cluster_of(&json!({ "cluster": { "retryAfterMs": 90000 } })).failed);
        let p = IdePalette::zen_dark();
        assert_eq!(state_color(&p, "error"), Some(p.error_text));
        assert_eq!(state_color(&p, "ok"), Some(p.run));
        assert_eq!(state_color(&p, ""), None);
    }

    #[test]
    fn an_index_answer_becomes_nodes_with_their_worst_problem_and_edges() {
        let graph = sample();
        assert_eq!(graph.nodes.len(), 6, "a node that is not in the graph is left out");
        let infra = graph.nodes.iter().find(|node| node.id == "infra").unwrap();
        assert_eq!(infra.problem, 2, "the error inside its span");
        assert_eq!(graph.nodes.iter().find(|node| node.id == "apps").unwrap().problem, 0);
        assert_eq!(graph.edges.len(), 6);
        assert_eq!(graph.kinds, ["flux"]);
    }

    #[test]
    fn the_tree_starts_from_what_nothing_contains_and_expands_into_its_connections() {
        let graph = sample();
        let collapsed = tree_rows(&graph, &BTreeSet::new(), "");
        let roots: Vec<&str> = collapsed.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            roots,
            ["apps", "root", "infra", "repo"],
            "Kustomizations first, by name; the folder and release are inside"
        );
        assert!(collapsed[0].children && !collapsed[3].children);
        let expanded: BTreeSet<String> = ["apps".to_owned(), "path:apps".to_owned()].into_iter().collect();
        let rows = tree_rows(&graph, &expanded, "");
        let under: Vec<(&str, usize, Option<&str>)> =
            rows.iter().take(5).map(|row| (row.id.as_str(), row.depth, row.edge.as_deref())).collect();
        assert_eq!(
            under,
            [
                ("apps", 0, None),
                ("path:apps", 1, Some("path")),
                ("release", 2, Some("resource")),
                ("repo", 1, Some("source")),
                ("infra", 1, Some("dependsOn")),
            ]
        );
        let found: Vec<String> = tree_rows(&graph, &expanded, "WEB").into_iter().map(|row| row.id).collect();
        assert_eq!(found, ["release"], "the filter lists matches flat");
    }

    #[test]
    fn the_graph_puts_sources_left_of_what_they_feed_and_survives_cycles() {
        let mut graph = sample();
        let placed = layers(&graph);
        let column = |id: &str| placed[id].0;
        assert!(column("repo") < column("apps"), "a source feeds its Kustomization");
        assert!(column("infra") < column("apps"), "a dependency first");
        assert!(column("apps") < column("path:apps") && column("path:apps") < column("release"));
        graph.edges.push(GraphEdge { from: "infra".to_owned(), to: "apps".to_owned(), kind: "dependsOn".to_owned() });
        let placed = layers(&graph);
        assert_eq!(placed.len(), graph.nodes.len(), "a cycle still places every node");
        assert_eq!(connected(&graph, "release"), ["path:apps".to_owned(), "release".to_owned()].into_iter().collect());
    }
}
