use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{Catalog, ConstraintKind, Object, ObjectKind, Snapshot};

/// A column of an entity as the diagram shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    /// The column's name.
    pub name: String,
    /// Its type as the server names it.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Whether it is part of the primary key.
    pub primary: bool,
    /// Whether it is part of a foreign key.
    pub foreign: bool,
    /// Whether it is part of a unique key.
    pub unique: bool,
    /// Whether it may be NULL.
    pub nullable: bool,
}

/// A table, view or materialised view on the diagram.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    /// Its name.
    pub name: String,
    /// Its schema.
    pub schema: String,
    /// Its path in the connection's tree, `[catalog, schema, folder, name]`.
    pub path: Vec<String>,
    /// What it is: `table`, `view`, `materializedView` or `foreignTable`.
    pub kind: String,
    /// Whether it belongs to another schema than the diagram's and is shown for a relation.
    pub external: bool,
    /// Whether its columns and keys were read; false when only its name is known.
    pub detailed: bool,
    /// Its columns in their order.
    pub columns: Vec<Attribute>,
}

impl Entity {
    pub fn label(&self) -> String {
        if self.external { format!("{}.{}", self.schema, self.name) } else { self.name.clone() }
    }
}

/// A foreign key between two entities.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// The foreign key's name.
    pub name: String,
    /// The entity holding the foreign key, an index into the entities.
    pub from: usize,
    /// Its columns.
    pub from_columns: Vec<String>,
    /// The entity it references.
    pub to: usize,
    /// The referenced columns.
    pub to_columns: Vec<String>,
    /// Whether a row may reference nothing: one of the foreign key's columns may be NULL.
    pub optional: bool,
    /// Whether a referenced row is referenced at most once: the foreign key's columns are unique.
    pub unique: bool,
}

/// An ER diagram: entities sorted by schema and name, and the foreign keys between them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErDiagram {
    /// The entities.
    pub entities: Vec<Entity>,
    /// The relations.
    pub relations: Vec<Relation>,
}

impl ErDiagram {
    pub fn partial(&self) -> bool {
        self.entities.iter().any(|entity| !entity.detailed)
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.entities
            .iter()
            .position(|entity| entity.label() == name)
            .or_else(|| self.entities.iter().position(|entity| format!("{}.{}", entity.schema, entity.name) == name))
            .or_else(|| self.entities.iter().position(|entity| entity.name == name))
    }
}

fn shown(object: &Object) -> bool {
    matches!(
        object.kind,
        ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView | ObjectKind::ForeignTable
    ) && !object.system
        && object.partitioning.as_ref().is_none_or(|partitioning| partitioning.parent.is_none())
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::View => "view",
        ObjectKind::MaterializedView => "materializedView",
        ObjectKind::ForeignTable => "foreignTable",
        _ => "table",
    }
}

fn entity(catalog: &str, schema: &str, object: &Object, external: bool) -> Entity {
    let keyed = |kind: ConstraintKind, column: &str| {
        object
            .constraints
            .iter()
            .any(|constraint| constraint.kind == kind && constraint.columns.iter().any(|known| known == column))
    };
    let columns = object
        .columns
        .iter()
        .map(|column| Attribute {
            name: column.name.clone(),
            type_name: column.type_name.clone(),
            primary: keyed(ConstraintKind::PrimaryKey, &column.name),
            foreign: keyed(ConstraintKind::ForeignKey, &column.name),
            unique: keyed(ConstraintKind::Unique, &column.name),
            nullable: column.nullable,
        })
        .collect();
    Entity {
        name: object.name.clone(),
        schema: schema.to_owned(),
        path: vec![catalog.to_owned(), schema.to_owned(), object.kind.folder().to_owned(), object.key()],
        kind: kind_name(object.kind).to_owned(),
        external,
        detailed: object.detailed,
        columns,
    }
}

fn lookup<'a>(catalog: &'a Catalog, schema: &str, name: &str) -> Option<&'a Object> {
    catalog
        .schemas
        .iter()
        .find(|known| known.name == schema)?
        .objects
        .iter()
        .find(|object| object.name == name && shown(object))
}

fn unique_columns(object: &Object, columns: &[String]) -> bool {
    !columns.is_empty()
        && object.constraints.iter().any(|constraint| {
            matches!(constraint.kind, ConstraintKind::PrimaryKey | ConstraintKind::Unique)
                && constraint.columns.len() == columns.len()
                && constraint.columns.iter().all(|column| columns.contains(column))
        })
}

struct Builder<'a> {
    catalog: &'a Catalog,
    home: String,
    found: BTreeMap<(bool, String, String), &'a Object>,
}

impl<'a> Builder<'a> {
    fn add(&mut self, schema: &str, object: &'a Object) {
        let external = schema != self.home;
        self.found.entry((external, schema.to_owned(), object.name.clone())).or_insert(object);
    }

    fn finish(self, keep: impl Fn(&(bool, String, String), &(bool, String, String)) -> bool) -> ErDiagram {
        let keys: Vec<(bool, String, String)> = self.found.keys().cloned().collect();
        let index: HashMap<(String, String), usize> =
            keys.iter().enumerate().map(|(at, (_, schema, name))| ((schema.clone(), name.clone()), at)).collect();
        let entities: Vec<Entity> =
            keys.iter().map(|key| entity(&self.catalog.name, &key.1, self.found[key], key.0)).collect();
        let mut relations = Vec::new();
        for (from, key) in keys.iter().enumerate() {
            let object = self.found[key];
            for constraint in &object.constraints {
                let Some(reference) = constraint.references.as_ref() else { continue };
                if constraint.kind != ConstraintKind::ForeignKey {
                    continue;
                }
                let Some(&to) = index.get(&(reference.schema.clone(), reference.table.clone())) else { continue };
                if !keep(key, &keys[to]) {
                    continue;
                }
                let optional = constraint
                    .columns
                    .iter()
                    .any(|name| object.columns.iter().any(|column| column.name == *name && column.nullable));
                relations.push(Relation {
                    name: constraint.name.clone(),
                    from,
                    from_columns: constraint.columns.clone(),
                    to,
                    to_columns: reference.columns.clone(),
                    optional,
                    unique: unique_columns(object, &constraint.columns),
                });
            }
        }
        ErDiagram { entities, relations }
    }
}

pub fn of_schema(snapshot: &Snapshot, catalog: &str, schema: &str) -> Option<ErDiagram> {
    let catalog = snapshot.catalog(catalog)?;
    let home = catalog.schemas.iter().find(|known| known.name == schema)?;
    let mut builder = Builder { catalog, home: schema.to_owned(), found: BTreeMap::new() };
    for object in home.objects.iter().filter(|object| shown(object)) {
        builder.add(schema, object);
        for reference in object.constraints.iter().filter_map(|constraint| constraint.references.as_ref()) {
            if reference.schema != schema
                && let Some(target) = lookup(catalog, &reference.schema, &reference.table)
            {
                builder.add(&reference.schema, target);
            }
        }
    }
    Some(builder.finish(|_, _| true))
}

pub fn of_table(snapshot: &Snapshot, path: &[String]) -> Option<ErDiagram> {
    let found = crate::schema::find_object(snapshot, path)?;
    if !shown(found.object) {
        return None;
    }
    let schema = found.schema.name.clone();
    let mut builder = Builder { catalog: found.catalog, home: schema.clone(), found: BTreeMap::new() };
    builder.add(&schema, found.object);
    for reference in found.object.constraints.iter().filter_map(|constraint| constraint.references.as_ref()) {
        if let Some(target) = lookup(found.catalog, &reference.schema, &reference.table) {
            builder.add(&reference.schema, target);
        }
    }
    for other in &found.catalog.schemas {
        for object in other.objects.iter().filter(|object| shown(object)) {
            let points_here = object.constraints.iter().any(|constraint| {
                constraint
                    .references
                    .as_ref()
                    .is_some_and(|reference| reference.schema == schema && reference.table == found.object.name)
            });
            if points_here {
                builder.add(&other.name, object);
            }
        }
    }
    let center = (false, schema, found.object.name.clone());
    Some(builder.finish(|from, to| *from == center || *to == center))
}

/// The sizes the layout gives boxes, in the canvas's units (points at 100 %).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    /// The width of a character of a column row (a monospaced font).
    pub char_width: f32,
    /// The width of a character of a title, at most.
    pub title_char_width: f32,
    /// The height of the title bar.
    pub header: f32,
    /// The height of a column row.
    pub row: f32,
    /// The space left and right of a row's text, and for its key icon.
    pub padding: f32,
    /// The narrowest box.
    pub min_width: f32,
    /// The widest box.
    pub max_width: f32,
    /// The most column rows a box shows; one more row says how many are left out.
    pub max_rows: usize,
    /// The space between columns of boxes, where relations run.
    pub gap_x: f32,
    /// The space between boxes of a column.
    pub gap_y: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            char_width: 7.3,
            title_char_width: 7.8,
            header: 26.0,
            row: 19.0,
            padding: 8.0,
            min_width: 120.0,
            max_width: 420.0,
            max_rows: 40,
            gap_x: 90.0,
            gap_y: 28.0,
        }
    }
}

/// A rectangle of the diagram.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
}

impl Bounds {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Where everything of a diagram is: a box per entity, a polyline per relation from the referencing entity to the referenced one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// The box of each entity, in the entities' order.
    pub boxes: Vec<Bounds>,
    /// How many column rows each box shows.
    pub rows: Vec<usize>,
    /// The points of each relation, in the relations' order.
    pub edges: Vec<Vec<[f32; 2]>>,
    /// The width of the whole diagram.
    pub width: f32,
    /// The height of the whole diagram.
    pub height: f32,
    /// The metrics it was laid out with.
    pub metrics: Metrics,
}

fn box_size(entity: &Entity, metrics: &Metrics) -> (f32, f32, usize) {
    let total = entity.columns.len();
    let rows = if total > metrics.max_rows { metrics.max_rows - 1 } else { total };
    let longest = entity
        .columns
        .iter()
        .take(rows)
        .map(|column| column.name.chars().count() + 2 + column.type_name.chars().count())
        .max()
        .unwrap_or(0) as f32;
    let title = entity.label().chars().count() as f32 * metrics.title_char_width + 2.0 * metrics.padding;
    let width = (longest * metrics.char_width + 3.0 * metrics.padding + metrics.row)
        .max(title)
        .clamp(metrics.min_width, metrics.max_width);
    let shown_rows = if total > rows { rows + 1 } else { rows.max(1) };
    (width.ceil(), metrics.header + shown_rows as f32 * metrics.row + 6.0, rows)
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn ranks(nodes: &[usize], parents: &[Vec<usize>]) -> HashMap<usize, usize> {
    let mut rank: HashMap<usize, usize> = HashMap::new();
    let mut on_stack: HashMap<usize, bool> = HashMap::new();
    for &start in nodes {
        if rank.contains_key(&start) {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        on_stack.insert(start, true);
        while let Some((node, next)) = stack.pop() {
            if let Some(&parent) = parents[node].get(next) {
                stack.push((node, next + 1));
                if !rank.contains_key(&parent) && !on_stack.get(&parent).copied().unwrap_or(false) {
                    on_stack.insert(parent, true);
                    stack.push((parent, 0));
                }
                continue;
            }
            let own = parents[node].iter().filter_map(|parent| rank.get(parent)).map(|known| known + 1).max();
            rank.insert(node, own.unwrap_or(0));
            on_stack.insert(node, false);
        }
    }
    rank
}

fn barycenter(node: usize, neighbours: &[Vec<usize>], position: &HashMap<usize, f32>) -> Option<f32> {
    let known: Vec<f32> = neighbours[node].iter().filter_map(|other| position.get(other).copied()).collect();
    (!known.is_empty()).then(|| known.iter().sum::<f32>() / known.len() as f32)
}

fn order_layers(layers: &mut [Vec<usize>], neighbours: &[Vec<usize>]) {
    for sweep in 0..4 {
        let order: Vec<usize> = if sweep % 2 == 0 {
            (1..layers.len()).collect()
        } else {
            (0..layers.len().saturating_sub(1)).rev().collect()
        };
        for at in order {
            let beside = if sweep % 2 == 0 { at - 1 } else { at + 1 };
            let position: HashMap<usize, f32> =
                layers[beside].iter().enumerate().map(|(index, &node)| (node, index as f32)).collect();
            let current: Vec<(usize, f32)> = layers[at]
                .iter()
                .enumerate()
                .map(|(index, &node)| (node, barycenter(node, neighbours, &position).unwrap_or(index as f32)))
                .collect();
            let mut sorted = current;
            sorted.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            layers[at] = sorted.into_iter().map(|(node, _)| node).collect();
        }
    }
}

struct Placed {
    nodes: Vec<(usize, f32, f32)>,
    width: f32,
    height: f32,
}

fn place_component(
    nodes: &[usize],
    sizes: &[(f32, f32, usize)],
    parents: &[Vec<usize>],
    neighbours: &[Vec<usize>],
    metrics: &Metrics,
) -> Placed {
    let rank = ranks(nodes, parents);
    let depth = rank.values().copied().max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for &node in nodes {
        layers[rank[&node]].push(node);
    }
    order_layers(&mut layers, neighbours);
    let mut placed = Vec::new();
    let mut centers: HashMap<usize, f32> = HashMap::new();
    let mut x = 0.0f32;
    let mut height = 0.0f32;
    for (at, layer) in layers.iter().enumerate() {
        let width = layer.iter().map(|&node| sizes[node].0).fold(0.0, f32::max);
        let mut bottom = -metrics.gap_y;
        for &node in layer {
            let size = sizes[node];
            let wanted =
                if at == 0 { None } else { barycenter(node, neighbours, &centers).map(|center| center - size.1 / 2.0) };
            let y = wanted.unwrap_or(f32::MIN).max(bottom + metrics.gap_y).max(0.0);
            bottom = y + size.1;
            placed.push((node, x, y));
            centers.insert(node, y + size.1 / 2.0);
        }
        height = height.max(bottom);
        x += width + metrics.gap_x;
    }
    Placed { nodes: placed, width: (x - metrics.gap_x).max(0.0), height: height.max(0.0) }
}

fn place_grid(nodes: &[usize], sizes: &[(f32, f32, usize)], metrics: &Metrics) -> Placed {
    let columns = (nodes.len() as f32).sqrt().ceil().max(1.0) as usize;
    let mut placed = Vec::new();
    let mut widths = vec![0.0f32; columns];
    for (at, &node) in nodes.iter().enumerate() {
        widths[at % columns] = widths[at % columns].max(sizes[node].0);
    }
    let mut y = 0.0;
    let mut width = 0.0f32;
    for row in nodes.chunks(columns) {
        let mut x = 0.0;
        let mut tallest = 0.0f32;
        for (at, &node) in row.iter().enumerate() {
            placed.push((node, x, y));
            tallest = tallest.max(sizes[node].1);
            x += widths[at] + metrics.gap_y * 1.5;
        }
        width = width.max(x - metrics.gap_y * 1.5);
        y += tallest + metrics.gap_y;
    }
    Placed { nodes: placed, width, height: (y - metrics.gap_y).max(0.0) }
}

fn row_y(bounds: &Bounds, entity: &Entity, rows: usize, column: Option<&String>, metrics: &Metrics) -> f32 {
    let index = column.and_then(|name| entity.columns.iter().take(rows).position(|known| known.name == *name));
    match index {
        Some(index) => bounds.y + metrics.header + (index as f32 + 0.5) * metrics.row,
        None => bounds.y + metrics.header / 2.0,
    }
}

fn route(diagram: &ErDiagram, boxes: &[Bounds], rows: &[usize], metrics: &Metrics) -> Vec<Vec<[f32; 2]>> {
    let mut loops: HashMap<usize, usize> = HashMap::new();
    diagram
        .relations
        .iter()
        .map(|relation| {
            let (from, to) = (&boxes[relation.from], &boxes[relation.to]);
            let start_y = row_y(
                from,
                &diagram.entities[relation.from],
                rows[relation.from],
                relation.from_columns.first(),
                metrics,
            );
            let end_y =
                row_y(to, &diagram.entities[relation.to], rows[relation.to], relation.to_columns.first(), metrics);
            if to.right() <= from.x {
                let middle = (to.right() + from.x) / 2.0;
                vec![[from.x, start_y], [middle, start_y], [middle, end_y], [to.right(), end_y]]
            } else if from.right() <= to.x {
                let middle = (from.right() + to.x) / 2.0;
                vec![[from.right(), start_y], [middle, start_y], [middle, end_y], [to.x, end_y]]
            } else {
                let right = from.right().max(to.right());
                let count = loops.entry(relation.from.max(relation.to)).or_insert(0);
                *count += 1;
                let out = right + 14.0 + 8.0 * (*count as f32 - 1.0).min(6.0);
                let end_y = if relation.from == relation.to && (end_y - start_y).abs() < 1.0 {
                    to.y + metrics.header / 2.0
                } else {
                    end_y
                };
                vec![[from.right(), start_y], [out, start_y], [out, end_y], [to.right(), end_y]]
            }
        })
        .collect()
}

pub fn layout(diagram: &ErDiagram, metrics: &Metrics, cancel: &AtomicBool) -> Option<Layout> {
    let count = diagram.entities.len();
    let sizes: Vec<(f32, f32, usize)> = diagram.entities.iter().map(|entity| box_size(entity, metrics)).collect();
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); count];
    for relation in diagram.relations.iter().filter(|relation| relation.from != relation.to) {
        if !parents[relation.from].contains(&relation.to) {
            parents[relation.from].push(relation.to);
        }
        if !neighbours[relation.from].contains(&relation.to) {
            neighbours[relation.from].push(relation.to);
            neighbours[relation.to].push(relation.from);
        }
    }
    for list in parents.iter_mut().chain(neighbours.iter_mut()) {
        list.sort_unstable();
    }
    let mut component = vec![usize::MAX; count];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut alone = Vec::new();
    for start in 0..count {
        if component[start] != usize::MAX {
            continue;
        }
        if neighbours[start].is_empty() {
            component[start] = usize::MAX - 1;
            alone.push(start);
            continue;
        }
        let id = groups.len();
        let mut members = vec![start];
        component[start] = id;
        let mut at = 0;
        while at < members.len() {
            let node = members[at];
            for &next in &neighbours[node] {
                if component[next] == usize::MAX {
                    component[next] = id;
                    members.push(next);
                }
            }
            at += 1;
        }
        members.sort_unstable();
        groups.push(members);
    }
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    let mut parts = Vec::new();
    for group in &groups {
        if cancelled(cancel) {
            return None;
        }
        parts.push(place_component(group, &sizes, &parents, &neighbours, metrics));
    }
    if !alone.is_empty() {
        parts.push(place_grid(&alone, &sizes, metrics));
    }
    if cancelled(cancel) {
        return None;
    }
    let area: f32 = parts.iter().map(|part| (part.width + metrics.gap_x) * (part.height + metrics.gap_x)).sum();
    let widest = parts.iter().map(|part| part.width).fold(0.0, f32::max);
    let shelf = (area.sqrt() * 1.5).max(widest);
    let mut boxes = vec![Bounds::default(); count];
    let (mut x, mut y, mut tallest, mut width) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for part in &parts {
        if x > 0.0 && x + part.width > shelf {
            x = 0.0;
            y += tallest + metrics.gap_x;
            tallest = 0.0;
        }
        for &(node, left, top) in &part.nodes {
            boxes[node] = Bounds { x: x + left, y: y + top, width: sizes[node].0, height: sizes[node].1 };
        }
        width = width.max(x + part.width);
        tallest = tallest.max(part.height);
        x += part.width + metrics.gap_x;
    }
    let rows: Vec<usize> = sizes.iter().map(|size| size.2).collect();
    let edges = route(diagram, &boxes, &rows, metrics);
    let right = edges.iter().flatten().map(|point| point[0]).fold(width, f32::max);
    Some(Layout { boxes, rows, edges, width: right, height: y + tallest, metrics: *metrics })
}

/// The colours of an exported diagram, each `[r, g, b, a]`, taken from the theme by the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    /// The page behind the boxes.
    pub background: [u8; 4],
    /// The inside of a box.
    pub fill: [u8; 4],
    /// The title bar of a box.
    pub header: [u8; 4],
    /// A box's outline.
    pub border: [u8; 4],
    /// Names.
    pub text: [u8; 4],
    /// Types and other secondary text.
    pub secondary: [u8; 4],
    /// The relations' lines.
    pub line: [u8; 4],
    /// The primary key icon.
    pub key: [u8; 4],
    /// The foreign key icon.
    pub foreign: [u8; 4],
}

fn hex(color: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn opacity(color: [u8; 4]) -> String {
    if color[3] == 255 { String::new() } else { format!(" opacity=\"{:.3}\"", f32::from(color[3]) / 255.0) }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fit(text: &str, room: f32, char_width: f32) -> String {
    let most = (room / char_width).floor().max(1.0) as usize;
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

pub fn fit_row(name: &str, type_name: &str, room: f32, char_width: f32) -> (String, String) {
    let name_chars = name.chars().count().min(6) as f32;
    let type_room = room - (name_chars + 1.0) * char_width;
    let shown_type = fit(type_name, type_room, char_width);
    let used = (shown_type.chars().count() as f32 + 1.0) * char_width;
    (fit(name, room - used, char_width), shown_type)
}

const MARGIN: f32 = 16.0;

pub fn svg(diagram: &ErDiagram, layout: &Layout, colors: &Colors) -> String {
    let metrics = &layout.metrics;
    let (width, height) = (layout.width + 2.0 * MARGIN, layout.height + 2.0 * MARGIN);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"{:.0} {:.0} {width:.0} {height:.0}\" font-size=\"12\">",
        -MARGIN, -MARGIN
    );
    let _ = writeln!(
        out,
        "<rect x=\"{:.0}\" y=\"{:.0}\" width=\"{width:.0}\" height=\"{height:.0}\" fill=\"{}\"{}/>",
        -MARGIN,
        -MARGIN,
        hex(colors.background),
        opacity(colors.background)
    );
    let _ = writeln!(out, "<g fill=\"none\" stroke=\"{}\" stroke-width=\"1.2\">", hex(colors.line));
    for (relation, points) in diagram.relations.iter().zip(&layout.edges) {
        let path: Vec<String> = points.iter().map(|point| format!("{:.1},{:.1}", point[0], point[1])).collect();
        let _ = writeln!(
            out,
            "<polyline points=\"{}\"><title>{}</title></polyline>",
            path.join(" "),
            escape(&relation.name)
        );
        let ends = [
            (points.first(), points.get(1), !relation.unique, false),
            (points.last(), points.iter().rev().nth(1), false, relation.optional),
        ];
        for (tip, before, many, optional) in ends {
            let (Some(tip), Some(before)) = (tip, before) else { continue };
            let direction = if before[0] >= tip[0] { 1.0 } else { -1.0 };
            let x = tip[0] + direction * 8.0;
            if many {
                let _ = writeln!(
                    out,
                    "<path d=\"M{:.1},{:.1} L{x:.1},{:.1} M{:.1},{:.1} L{x:.1},{:.1} M{:.1},{:.1} L{x:.1},{:.1}\"/>",
                    tip[0],
                    tip[1] - 5.0,
                    tip[1],
                    tip[0],
                    tip[1] + 5.0,
                    tip[1],
                    tip[0],
                    tip[1],
                    tip[1]
                );
            } else {
                let _ = writeln!(out, "<path d=\"M{x:.1},{:.1} L{x:.1},{:.1}\"/>", tip[1] - 5.0, tip[1] + 5.0);
            }
            if optional {
                let _ = writeln!(out, "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\"/>", tip[0] + direction * 13.0, tip[1]);
            }
        }
    }
    out.push_str("</g>\n");
    for (at, entity) in diagram.entities.iter().enumerate() {
        let bounds = &layout.boxes[at];
        let rows = layout.rows[at];
        let dash = if entity.kind == "table" { "" } else { " stroke-dasharray=\"4 3\"" };
        let _ = writeln!(out, "<g><title>{}</title>", escape(&format!("{}.{}", entity.schema, entity.name)));
        let _ = writeln!(
            out,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4\" fill=\"{}\" stroke=\"{}\"{dash}/>",
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            hex(colors.fill),
            hex(colors.border)
        );
        let _ = writeln!(
            out,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4\" fill=\"{}\"/>",
            bounds.x + 0.5,
            bounds.y + 0.5,
            bounds.width - 1.0,
            metrics.header,
            hex(colors.header)
        );
        let title = fit(&entity.label(), bounds.width - 2.0 * metrics.padding, metrics.title_char_width);
        let _ = writeln!(
            out,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-weight=\"600\" fill=\"{}\">{}</text>",
            bounds.x + metrics.padding,
            bounds.y + metrics.header / 2.0 + 4.0,
            hex(colors.text),
            escape(&title)
        );
        for (row, column) in entity.columns.iter().take(rows).enumerate() {
            let y = bounds.y + metrics.header + (row as f32 + 0.5) * metrics.row;
            let icon = bounds.x + metrics.padding + metrics.row / 2.0 - 4.0;
            if column.primary || column.foreign {
                let color = if column.primary { colors.key } else { colors.foreign };
                let _ = writeln!(
                    out,
                    "<g fill=\"none\" stroke=\"{}\" stroke-width=\"1.4\"><circle cx=\"{:.1}\" cy=\"{y:.1}\" r=\"3\"/><path d=\"M{:.1},{y:.1} L{:.1},{y:.1} L{:.1},{:.1}\"/></g>",
                    hex(color),
                    icon - 2.0,
                    icon + 1.0,
                    icon + 8.0,
                    icon + 8.0,
                    y + 3.0
                );
            }
            let left = bounds.x + metrics.padding + metrics.row;
            let room = bounds.right() - metrics.padding - left;
            let (name, type_name) = fit_row(&column.name, &column.type_name, room, metrics.char_width);
            let _ = writeln!(
                out,
                "<text x=\"{left:.1}\" y=\"{:.1}\" font-family=\"monospace\" fill=\"{}\">{}</text>",
                y + 4.0,
                hex(colors.text),
                escape(&name)
            );
            let _ = writeln!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"monospace\" text-anchor=\"end\" fill=\"{}\">{}</text>",
                bounds.right() - metrics.padding,
                y + 4.0,
                hex(colors.secondary),
                escape(&type_name)
            );
        }
        if entity.columns.len() > rows {
            let y = bounds.y + metrics.header + (rows as f32 + 0.5) * metrics.row;
            let _ = writeln!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"monospace\" fill=\"{}\">+{}</text>",
                bounds.x + metrics.padding + metrics.row,
                y + 4.0,
                hex(colors.secondary),
                entity.columns.len() - rows
            );
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

fn mermaid_name(text: &str) -> String {
    let mut name: String =
        text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        name.insert(0, '_');
    }
    name
}

fn mermaid_type(text: &str) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '(' | ')' | '[' | ']') { c } else { '_' })
        .collect();
    if kept.is_empty() { "unknown".to_owned() } else { kept }
}

pub fn mermaid(diagram: &ErDiagram) -> String {
    let mut names: Vec<String> = Vec::with_capacity(diagram.entities.len());
    let mut taken: HashMap<String, usize> = HashMap::new();
    for entity in &diagram.entities {
        let base = mermaid_name(&entity.label());
        let count = taken.entry(base.clone()).or_insert(0);
        *count += 1;
        names.push(if *count == 1 { base } else { format!("{base}_{count}") });
    }
    let mut out = String::from("erDiagram\n");
    for (entity, name) in diagram.entities.iter().zip(&names) {
        let label = entity.label();
        if *name == label {
            let _ = write!(out, "    {name}");
        } else {
            let _ = write!(out, "    {name}[\"{}\"]", label.replace('"', "'"));
        }
        if entity.columns.is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(" {\n");
        for column in &entity.columns {
            let keys: Vec<&str> =
                [(column.primary, "PK"), (column.foreign, "FK"), (column.unique && !column.primary, "UK")]
                    .into_iter()
                    .filter_map(|(on, key)| on.then_some(key))
                    .collect();
            let _ = write!(out, "        {} {}", mermaid_type(&column.type_name), mermaid_name(&column.name));
            if !keys.is_empty() {
                let _ = write!(out, " {}", keys.join(", "));
            }
            if mermaid_name(&column.name) != column.name {
                let _ = write!(out, " \"{}\"", column.name.replace('"', "'"));
            }
            out.push('\n');
        }
        out.push_str("    }\n");
    }
    for relation in &diagram.relations {
        let many = if relation.unique { "|o" } else { "}o" };
        let one = if relation.optional { "o|" } else { "||" };
        let _ = writeln!(
            out,
            "    {} {many}--{one} {} : \"{}\"",
            names[relation.from],
            names[relation.to],
            relation.name.replace('"', "'")
        );
    }
    out
}

#[cfg(test)]
mod tests;
