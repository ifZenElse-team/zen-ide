#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub up: Vec<Edge>,
    pub down: Vec<Edge>,
    pub width: usize,
}

#[derive(Clone, Copy)]
struct Lane<K> {
    waits_for: K,
    color: usize,
}

pub struct Layout<K> {
    lanes: Vec<Option<Lane<K>>>,
    next_color: usize,
}

impl<K> Default for Layout<K> {
    fn default() -> Self {
        Layout { lanes: Vec::new(), next_color: 0 }
    }
}

pub fn layout(commits: &[(String, Vec<String>)]) -> Vec<GraphRow> {
    let mut layout = Layout::default();
    commits
        .iter()
        .map(|(id, parents)| {
            let parents: Vec<&str> = parents.iter().map(String::as_str).collect();
            layout.push(id.as_str(), &parents)
        })
        .collect()
}

impl<K: Copy + PartialEq> Layout<K> {
    pub fn push(&mut self, id: K, parents: &[K]) -> GraphRow {
        let Layout { lanes, next_color } = self;
        let mut row = GraphRow::default();
        let waited = lanes.iter().position(|lane| lane.is_some_and(|lane| lane.waits_for == id));
        let lane = waited.unwrap_or_else(|| {
            let free = lanes.iter().position(Option::is_none).unwrap_or(lanes.len());
            if free == lanes.len() {
                lanes.push(None);
            }
            lanes[free] = Some(Lane { waits_for: id, color: *next_color });
            *next_color += 1;
            free
        });
        let color = lanes[lane].map_or(0, |lane| lane.color);
        row.lane = lane;
        row.color = color;
        for (index, waiting) in lanes.iter().enumerate() {
            let Some(waiting) = waiting else { continue };
            if waited.is_none() && index == lane {
                continue;
            }
            let to = if waiting.waits_for == id { lane } else { index };
            row.up.push(Edge { from: index, to, color: waiting.color });
        }
        for (index, waiting) in lanes.iter_mut().enumerate() {
            if index != lane && waiting.is_some_and(|waiting| waiting.waits_for == id) {
                *waiting = None;
            }
        }
        lanes[lane] = parents.first().map(|parent| Lane { waits_for: *parent, color });
        let mut parent_lanes = Vec::new();
        if lanes[lane].is_some() {
            parent_lanes.push((lane, color));
        }
        for parent in parents.iter().skip(1) {
            if let Some(index) = lanes.iter().position(|lane| lane.is_some_and(|lane| lane.waits_for == *parent)) {
                parent_lanes.push((index, lanes[index].map_or(0, |lane| lane.color)));
                continue;
            }
            let free = lanes.iter().position(Option::is_none).unwrap_or(lanes.len());
            if free == lanes.len() {
                lanes.push(None);
            }
            lanes[free] = Some(Lane { waits_for: *parent, color: *next_color });
            parent_lanes.push((free, *next_color));
            *next_color += 1;
        }
        for (index, waiting) in lanes.iter().enumerate() {
            let Some(waiting) = waiting else { continue };
            if !parent_lanes.iter().any(|(lane, _)| *lane == index) {
                row.down.push(Edge { from: index, to: index, color: waiting.color });
            }
        }
        for (to, color) in parent_lanes {
            row.down.push(Edge { from: lane, to, color });
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        row.width = row
            .up
            .iter()
            .chain(&row.down)
            .flat_map(|edge| [edge.from, edge.to])
            .chain([lane])
            .max()
            .map_or(1, |widest| widest + 1);
        row
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    Commit(usize),
    Side(usize),
}

pub fn attach(rows: Vec<GraphRow>, sides: &[usize]) -> Vec<(Placed, GraphRow)> {
    let mut by_base: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (side, &base) in sides.iter().enumerate() {
        if base < rows.len() {
            by_base.entry(base).or_default().push(side);
        }
    }
    let mut placed = Vec::with_capacity(rows.len() + sides.len());
    for (index, mut row) in rows.into_iter().enumerate() {
        if let Some(group) = by_base.get(&index) {
            let sides = attach_sides(&mut row, group.len());
            placed.extend(group.iter().zip(sides).map(|(&side, side_row)| (Placed::Side(side), side_row)));
        }
        placed.push((Placed::Commit(index), row));
    }
    placed
}

pub fn attach_sides(row: &mut GraphRow, count: usize) -> Vec<GraphRow> {
    if count == 0 {
        return Vec::new();
    }
    let through: Vec<Edge> = row.up.iter().map(|edge| Edge { to: edge.from, ..*edge }).collect();
    let first = through.iter().map(|edge| edge.from).chain([row.lane]).max().map_or(0, |widest| widest + 1);
    let color = row.color;
    let straight = |lane: usize| Edge { from: lane, to: lane, color };
    let sides = (0..count)
        .map(|above| {
            let lane = first + above;
            let mut up = through.clone();
            up.extend((first..lane).map(straight));
            let mut down = through.clone();
            down.extend((first..=lane).map(straight));
            let width = through.iter().map(|edge| edge.from + 1).chain([lane + 1]).max().unwrap_or(1);
            GraphRow { lane, color, up, down, width }
        })
        .collect();
    row.up.extend((first..first + count).map(|lane| Edge { from: lane, to: row.lane, color }));
    row.width = row.width.max(first + count);
    sides
}

#[derive(Default)]
pub struct PackedRows {
    rows: Vec<PackedRow>,
    edges: Vec<PackedEdge>,
}

#[derive(Clone, Copy)]
struct PackedRow {
    edges: u32,
    up: u16,
    lane: u16,
    color: u32,
}

#[derive(Clone, Copy)]
struct PackedEdge {
    from: u16,
    to: u16,
    color: u32,
}

fn lane16(lane: usize) -> u16 {
    lane.min(usize::from(u16::MAX)) as u16
}

impl PackedRows {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.edges.clear();
    }

    pub fn push(&mut self, row: &GraphRow) {
        let pack = |edge: &Edge| PackedEdge { from: lane16(edge.from), to: lane16(edge.to), color: edge.color as u32 };
        self.rows.push(PackedRow {
            edges: self.edges.len() as u32,
            up: row.up.len().min(usize::from(u16::MAX)) as u16,
            lane: lane16(row.lane),
            color: row.color as u32,
        });
        self.edges.extend(row.up.iter().take(usize::from(u16::MAX)).map(pack));
        self.edges.extend(row.down.iter().map(pack));
    }

    pub fn get(&self, at: usize) -> GraphRow {
        let row = self.rows[at];
        let start = row.edges as usize;
        let end = self.rows.get(at + 1).map_or(self.edges.len(), |next| next.edges as usize);
        let edge = |edge: &PackedEdge| Edge {
            from: usize::from(edge.from),
            to: usize::from(edge.to),
            color: edge.color as usize,
        };
        let (up, down) = self.edges[start..end].split_at(usize::from(row.up));
        let up: Vec<Edge> = up.iter().map(edge).collect();
        let down: Vec<Edge> = down.iter().map(edge).collect();
        let lane = usize::from(row.lane);
        let width =
            up.iter().chain(&down).flat_map(|edge| [edge.from, edge.to]).chain([lane]).max().map_or(1, |w| w + 1);
        GraphRow { lane, color: row.color as usize, up, down, width }
    }

    pub fn heap_bytes(&self) -> usize {
        self.rows.capacity() * size_of::<PackedRow>() + self.edges.capacity() * size_of::<PackedEdge>()
    }
}

pub fn topological(commits: &[(String, Vec<String>)]) -> Vec<usize> {
    use std::collections::HashMap;
    let index: HashMap<&str, usize> = commits.iter().enumerate().map(|(at, (id, _))| (id.as_str(), at)).collect();
    let mut waiting = vec![0usize; commits.len()];
    for (_, parents) in commits {
        for parent in parents {
            if let Some(&at) = index.get(parent.as_str()) {
                waiting[at] += 1;
            }
        }
    }
    let mut order = Vec::with_capacity(commits.len());
    let mut done = vec![false; commits.len()];
    let mut stack: Vec<usize> = (0..commits.len()).rev().filter(|&at| waiting[at] == 0).collect();
    while order.len() < commits.len() {
        let Some(at) = stack.pop() else {
            order.extend((0..commits.len()).filter(|&at| !done[at]));
            break;
        };
        if std::mem::replace(&mut done[at], true) {
            continue;
        }
        order.push(at);
        for parent in commits[at].1.iter().rev() {
            if let Some(&parent) = index.get(parent.as_str()) {
                waiting[parent] -= 1;
                if waiting[parent] == 0 {
                    stack.push(parent);
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_order_keeps_a_branch_together() {
        let list = commits(&[
            ("m", &["b2", "f2"]),
            ("f2", &["f1"]),
            ("b2", &["b1"]),
            ("f1", &["a"]),
            ("b1", &["a"]),
            ("a", &[]),
        ]);
        let order: Vec<&str> = topological(&list).into_iter().map(|at| list[at].0.as_str()).collect();
        assert_eq!(order, ["m", "b2", "b1", "f2", "f1", "a"]);
    }

    fn commits(list: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        list.iter()
            .map(|(id, parents)| (id.to_string(), parents.iter().map(|parent| parent.to_string()).collect()))
            .collect()
    }

    #[test]
    fn a_straight_history_stays_in_one_lane() {
        let rows = layout(&commits(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]));
        assert!(rows.iter().all(|row| row.lane == 0 && row.width == 1));
        assert!(rows[0].up.is_empty(), "nothing above the newest commit");
        assert_eq!(rows[0].down, [Edge { from: 0, to: 0, color: 0 }]);
        assert!(rows[2].down.is_empty(), "nothing below the root");
    }

    #[test]
    fn a_merge_opens_a_lane_that_joins_again_at_the_fork() {
        let rows = layout(&commits(&[("m", &["b", "f"]), ("f", &["a"]), ("b", &["a"]), ("a", &[])]));
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].down, [Edge { from: 0, to: 0, color: 0 }, Edge { from: 0, to: 1, color: 1 }]);
        assert_eq!(rows[1].lane, 1, "the merged branch's commit in the second lane");
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].up, [Edge { from: 0, to: 0, color: 0 }, Edge { from: 1, to: 1, color: 1 }]);
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[3].up, [Edge { from: 0, to: 0, color: 0 }, Edge { from: 1, to: 0, color: 1 }]);
        assert_eq!(rows[3].width, 2);
    }

    #[test]
    fn unrelated_tips_take_their_own_lanes() {
        let rows = layout(&commits(&[("x", &["a"]), ("y", &["a"]), ("a", &[])]));
        assert_eq!((rows[0].lane, rows[1].lane, rows[2].lane), (0, 1, 0));
        assert_ne!(rows[0].color, rows[1].color);
    }

    #[test]
    fn an_octopus_merge_opens_a_lane_for_each_parent() {
        let rows =
            layout(&commits(&[("m", &["a", "b", "c"]), ("c", &["r"]), ("b", &["r"]), ("a", &["r"]), ("r", &[])]));
        let targets: Vec<usize> = rows[0].down.iter().map(|edge| edge.to).collect();
        assert_eq!(targets, [0, 1, 2], "the first parent goes straight on, the others to new lanes");
        assert_eq!((rows[1].lane, rows[2].lane, rows[3].lane), (2, 1, 0));
        assert_eq!(rows[4].lane, 0);
        let joins: Vec<(usize, usize)> = rows[4].up.iter().map(|edge| (edge.from, edge.to)).collect();
        assert_eq!(joins, [(0, 0), (1, 0), (2, 0)], "every lane joins the root");
        assert_eq!(rows[0].width, 3);
    }

    #[test]
    fn a_finished_lane_is_taken_again_by_the_next_branch() {
        let rows = layout(&commits(&[("m", &["a", "f"]), ("f", &["a"]), ("y", &["a"]), ("a", &[])]));
        assert_eq!(rows[1].lane, 1, "the merged branch in the second lane");
        assert_eq!(rows[2].lane, 2, "the second lane still waits for a: a new tip takes the third");
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[3].width, 3);
        let rows = layout(&commits(&[("m", &["b", "f"]), ("f", &["b"]), ("b", &["a"]), ("y", &["a"]), ("a", &[])]));
        assert_eq!(rows[2].lane, 0, "f's lane ends at b");
        assert_eq!(rows[3].lane, 1, "y takes the freed lane");
        assert!(rows[3].up.iter().all(|edge| edge.from != 1), "nothing comes into the freed lane from above");
    }

    #[test]
    fn colours_follow_the_lane_through_its_rows() {
        let rows = layout(&commits(&[("x2", &["x1"]), ("y1", &["a"]), ("x1", &["a"]), ("a", &[])]));
        assert_eq!(rows[0].color, rows[2].color, "a branch keeps its colour");
        assert_ne!(rows[0].color, rows[1].color, "another branch has another");
        assert!(rows[1].up.iter().any(|edge| edge.from == 0 && edge.to == 0 && edge.color == rows[0].color));
    }

    #[test]
    fn stashes_hang_above_their_commit_in_lanes_of_their_own() {
        let rows = layout(&commits(&[("m", &["b", "f"]), ("f", &["a"]), ("b", &["a"]), ("a", &[])]));
        let plain = rows.clone();
        let placed = attach(rows, &[2, 0, 2, 9]);
        let order: Vec<Placed> = placed.iter().map(|(placed, _)| *placed).collect();
        use Placed::{Commit, Side};
        assert_eq!(
            order,
            [Side(1), Commit(0), Commit(1), Side(0), Side(2), Commit(2), Commit(3)],
            "one too far is left out"
        );
        let row = |at: usize| &placed[at].1;
        let edge = |from, to, color| Edge { from, to, color };
        assert_eq!((row(0).lane, row(0).color, row(0).width), (1, plain[0].color, 2));
        assert!(row(0).up.is_empty());
        assert_eq!(row(0).down, [edge(1, 1, plain[0].color)]);
        assert_eq!(row(1).up, [edge(1, 0, plain[0].color)]);
        let through = [edge(0, 0, 0), edge(1, 1, 1)];
        assert_eq!((row(3).lane, row(4).lane), (2, 3));
        assert_eq!(row(3).up, through);
        assert_eq!(row(4).up, [through[0], through[1], edge(2, 2, plain[2].color)], "the upper stash passes by");
        assert_eq!(row(4).down.len(), 4);
        assert_eq!(row(5).up[2..], [edge(2, 0, plain[2].color), edge(3, 0, plain[2].color)], "both join b");
        assert_eq!(row(5).width, 4);
        assert_eq!(row(6), &plain[3], "the rest is as it was");
        assert_eq!(attach(plain.clone(), &[]).into_iter().map(|(_, row)| row).collect::<Vec<_>>(), plain);
    }

    #[test]
    fn packed_rows_give_back_the_rows_and_sides_attach_one_commit_at_a_time() {
        let list = commits(&[("m", &["b", "f"]), ("f", &["a"]), ("b", &["a"]), ("y", &["a"]), ("a", &[])]);
        let rows = layout(&list);
        let mut streamed = Layout::default();
        let ids: Vec<(usize, Vec<usize>)> = list
            .iter()
            .map(|(_, parents)| {
                let at = |id: &str| list.iter().position(|(other, _)| other == id).unwrap();
                (0, parents.iter().map(|parent| at(parent)).collect())
            })
            .collect();
        for (at, (_, parents)) in ids.iter().enumerate() {
            assert_eq!(streamed.push(at, parents), rows[at]);
        }
        let placed = attach(rows.clone(), &[2, 2]);
        let mut packed = PackedRows::default();
        for (at, row) in rows.iter().enumerate() {
            let mut row = row.clone();
            let sides = attach_sides(&mut row, if at == 2 { 2 } else { 0 });
            for side in sides.iter().chain([&row]) {
                packed.push(side);
            }
        }
        assert_eq!(packed.len(), placed.len());
        for (at, (_, row)) in placed.iter().enumerate() {
            assert_eq!(&packed.get(at), row, "row {at}");
        }
        assert!(packed.heap_bytes() > 0 && !packed.is_empty());
        packed.clear();
        assert!(packed.is_empty());
    }

    fn long_history() -> Vec<(String, Vec<String>)> {
        let mut list: Vec<(String, Vec<String>)> = Vec::new();
        for index in (1..20_000).rev() {
            let mut parents = vec![format!("c{}", index - 1)];
            if index % 100 == 0 {
                parents.push(format!("side{index}"));
                list.push((format!("c{index}"), parents));
                list.push((format!("side{index}"), vec![format!("c{}", index - 1)]));
            } else {
                list.push((format!("c{index}"), parents));
            }
        }
        list.push(("c0".to_owned(), Vec::new()));
        list
    }

    #[test]
    fn a_long_history_lays_out() {
        let list = long_history();
        let rows = layout(&list);
        assert_eq!(rows.len(), list.len());
        assert!(rows.iter().all(|row| row.width <= 2), "a side commit never needs a third lane");
    }

    #[test]
    #[ignore = "a timing check; run by hand"]
    fn a_long_history_lays_out_within_two_seconds() {
        let list = long_history();
        let started = std::time::Instant::now();
        assert_eq!(layout(&list).len(), list.len());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "took {:?}", started.elapsed());
    }

    #[test]
    fn a_lane_past_the_last_packed_one_is_drawn_in_the_last_one() {
        let far = usize::from(u16::MAX) + 5;
        let row = GraphRow { lane: far, color: 1, up: Vec::new(), down: Vec::new(), width: 1 };
        let mut packed = PackedRows::default();
        packed.push(&row);
        assert_eq!(packed.get(0).lane, usize::from(u16::MAX));
    }
}
