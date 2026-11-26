use std::collections::{BTreeSet, HashMap};

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Pos2, Rect, Stroke, pos2, vec2};
use zen_workbench::vcs::workspace::{GraphRef, RefKind};

use super::super::IdePalette;
use crate::theme;

pub(super) const LINE: f32 = 2.0;
pub(super) const ELBOW: f32 = 8.0;

pub(super) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0)).to_opaque()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tint {
    Row,
    Hovered,
    Selected,
}

pub(super) fn row_tint(p: &IdePalette, lane: Color32, tint: Tint) -> Color32 {
    let t = match tint {
        Tint::Row => 0.12,
        Tint::Hovered => 0.2,
        Tint::Selected => 0.42,
    };
    mix(p.editor_background, lane, t)
}

pub(super) fn pill_fill(p: &IdePalette, lane: Color32, current: bool) -> Color32 {
    mix(p.editor_background, lane, if current { 0.78 } else { 0.5 })
}

pub(super) fn pill_text(p: &IdePalette, current: bool) -> Color32 {
    if current { p.accent_text } else { p.text }
}

pub(super) fn small_caps(text: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat { font_id: theme::semibold(size), color, extra_letter_spacing: size * 0.06, ..Default::default() },
    );
    job.wrap.max_rows = 1;
    job
}

pub(super) fn small_caps_fitted(
    painter: &egui::Painter,
    text: &str,
    size: f32,
    color: Color32,
    room: f32,
) -> (std::sync::Arc<egui::Galley>, bool) {
    let whole = painter.layout_job(small_caps(text, size, color));
    if whole.size().x <= room {
        return (whole, false);
    }
    let text = text.trim_end();
    for keep in cut_points(text).into_iter().rev() {
        let galley = painter.layout_job(small_caps(&format!("{}…", text[..keep].trim_end()), size, color));
        if galley.size().x <= room {
            return (galley, true);
        }
    }
    (painter.layout_job(small_caps("…", size, color)), true)
}

pub(super) fn cut_points(text: &str) -> Vec<usize> {
    let joins = |c: char| {
        matches!(c as u32,
            0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
            | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF | 0x1F3FB..=0x1F3FF | 0x200C..=0x200D)
    };
    let mut points = Vec::new();
    let mut previous = None;
    for (at, c) in text.char_indices() {
        if at > 0 && !joins(c) && previous != Some('\u{200D}') {
            points.push(at);
        }
        previous = Some(c);
    }
    points
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pill {
    pub name: String,
    pub kind: RefKind,
    pub local: bool,
    pub remote: bool,
    pub current: bool,
}

pub(super) fn pills(refs: &[GraphRef]) -> Vec<Pill> {
    let mut used = vec![false; refs.len()];
    let mut pills = Vec::new();
    for (index, label) in refs.iter().enumerate() {
        if used[index] {
            continue;
        }
        used[index] = true;
        let mut pill = Pill {
            name: label.name.clone(),
            kind: label.kind,
            local: label.kind == RefKind::Local,
            remote: label.kind == RefKind::Remote,
            current: label.current,
        };
        if label.kind == RefKind::Local {
            for (other, remote) in refs.iter().enumerate() {
                let same = remote.name.split_once('/').is_some_and(|(_, name)| name == label.name);
                if !used[other] && remote.kind == RefKind::Remote && same {
                    used[other] = true;
                    pill.remote = true;
                }
            }
        }
        pills.push(pill);
    }
    pills
}

pub(super) fn elbow(from: Pos2, to: Pos2, down: bool) -> Vec<Pos2> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    if dx.abs() < 0.5 || dy.abs() < 0.5 {
        return vec![from, to];
    }
    let (sx, sy) = (dx.signum(), dy.signum());
    let r = ELBOW.min(dx.abs()).min(dy.abs());
    const STEPS: usize = 6;
    let quarter = |step: usize| step as f32 / STEPS as f32 * std::f32::consts::FRAC_PI_2;
    let mut points = vec![from];
    if down {
        let center = pos2(to.x - sx * r, from.y + sy * r);
        points.extend((0..=STEPS).map(|step| {
            let a = quarter(step);
            center + vec2(sx * r * a.sin(), -sy * r * a.cos())
        }));
    } else {
        let center = pos2(from.x + sx * r, to.y - sy * r);
        points.extend((0..=STEPS).map(|step| {
            let a = quarter(step);
            center + vec2(-sx * r * a.cos(), sy * r * a.sin())
        }));
    }
    points.push(to);
    points
}

pub(super) fn draw_elbow(painter: &egui::Painter, from: Pos2, to: Pos2, down: bool, color: Color32) {
    painter.add(egui::Shape::line(elbow(from, to, down), Stroke::new(LINE, color)));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TreeRow {
    Folder { key: String, name: String, depth: usize, open: bool, count: usize },
    Leaf { index: usize, depth: usize, name: String },
}

fn tree_order(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('/').collect(), b.split('/').collect());
    for (index, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            let (x_folder, y_folder) = (index + 1 < a.len(), index + 1 < b.len());
            return y_folder.cmp(&x_folder).then_with(|| x.to_lowercase().cmp(&y.to_lowercase())).then(x.cmp(y));
        }
    }
    a.len().cmp(&b.len())
}

pub(super) fn tree_rows(paths: &[&str], scope: &str, toggled: &BTreeSet<String>, open_all: bool) -> Vec<TreeRow> {
    let mut order: Vec<usize> = (0..paths.len()).collect();
    order.sort_by(|a, b| tree_order(paths[*a], paths[*b]));
    let mut counts: HashMap<String, usize> = HashMap::new();
    for path in paths {
        let mut end = 0;
        while let Some(slash) = path[end..].find('/') {
            end += slash;
            *counts.entry(path[..end].to_owned()).or_default() += 1;
            end += 1;
        }
    }
    let key = |path: &str| format!("{scope}:{path}");
    let folded_by_default = scope != "files";
    let is_open = |path: &str| open_all || (toggled.contains(&key(path)) == folded_by_default);
    let mut rows = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    for index in order {
        let parts: Vec<&str> = paths[index].split('/').collect();
        let (folders, name) = parts.split_at(parts.len() - 1);
        let common = stack.iter().zip(folders).take_while(|(a, b)| a == b).count();
        stack.truncate(common);
        for (depth, folder) in folders.iter().enumerate().skip(common) {
            stack.push(folder);
            let path = stack.join("/");
            if (1..=depth).all(|above| is_open(&stack[..above].join("/"))) {
                rows.push(TreeRow::Folder {
                    key: key(&path),
                    name: (*folder).to_owned(),
                    depth,
                    open: is_open(&path),
                    count: counts.get(&path).copied().unwrap_or_default(),
                });
            }
        }
        if (1..=folders.len()).all(|depth| is_open(&folders[..depth].join("/"))) {
            rows.push(TreeRow::Leaf { index, depth: folders.len(), name: name[0].to_owned() });
        }
    }
    rows
}

pub(super) fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(slash) => path.split_at(slash + 1),
        None => ("", path),
    }
}

pub(super) fn segment(ui: &egui::Ui, p: &IdePalette, rect: Rect, first: bool, chosen: bool, hovered: bool) {
    let radius = egui::CornerRadius {
        nw: if first { 5 } else { 0 },
        sw: if first { 5 } else { 0 },
        ne: if first { 0 } else { 5 },
        se: if first { 0 } else { 5 },
    };
    if chosen {
        ui.painter().rect_filled(rect, radius, p.selection);
    } else if hovered {
        ui.painter().rect_filled(rect, radius, p.hover);
    }
    ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_cut_only_between_whole_letters() {
        assert_eq!(cut_points("Ab"), [1]);
        assert_eq!(cut_points("Пи\u{306}ло"), [2, 6, 8]);
        assert_eq!(cut_points("a\u{1F468}\u{200D}\u{1F469}b"), [1, 12]);
        assert_eq!(cut_points("x\u{2764}\u{FE0F}y"), [1, 7]);
        assert!(cut_points("").is_empty());
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let title = "Сообщение коммита";
            let room = ui.painter().layout_job(small_caps("Сообщение к", 10.5, Color32::WHITE)).size().x;
            let (galley, shortened) = small_caps_fitted(ui.painter(), "Пи\u{306}\u{306}ло", 10.5, Color32::WHITE, 18.0);
            let text = galley.job.text.clone();
            assert!(shortened);
            assert!(text == "…" || !text.trim_end_matches('…').ends_with('И'), "{text:?}");
            let (galley, shortened) = small_caps_fitted(ui.painter(), title, 10.5, Color32::WHITE, room);
            assert!(shortened);
            assert!(galley.size().x <= room);
            assert!(galley.job.text.ends_with('…') && galley.job.text.starts_with("СООБЩЕНИЕ"), "{}", galley.job.text);
        });
        output.textures_delta.clear();
    }

    fn label(name: &str, kind: RefKind, current: bool) -> GraphRef {
        GraphRef { name: name.to_owned(), kind, current }
    }

    #[test]
    fn a_local_branch_takes_in_the_remote_branches_of_its_name() {
        let refs = [
            label("main", RefKind::Local, true),
            label("feature", RefKind::Local, false),
            label("origin/main", RefKind::Remote, false),
            label("upstream/main", RefKind::Remote, false),
            label("origin/other", RefKind::Remote, false),
            label("v1.0", RefKind::Tag, false),
        ];
        let pills = pills(&refs);
        let shown: Vec<(&str, bool, bool, bool)> =
            pills.iter().map(|pill| (pill.name.as_str(), pill.local, pill.remote, pill.current)).collect();
        assert_eq!(
            shown,
            [
                ("main", true, true, true),
                ("feature", true, false, false),
                ("origin/other", false, true, false),
                ("v1.0", false, false, false)
            ]
        );
        assert_eq!(pills[3].kind, RefKind::Tag);
    }

    #[test]
    fn a_line_that_changes_lanes_turns_in_a_rounded_elbow() {
        let points = elbow(pos2(0.0, 14.0), pos2(40.0, 28.0), true);
        assert_eq!(points.first(), Some(&pos2(0.0, 14.0)));
        assert_eq!(points.last(), Some(&pos2(40.0, 28.0)));
        assert!((points[1].y - 14.0).abs() < 1e-3 && (points[1].x - (40.0 - ELBOW)).abs() < 1e-3);
        let turned = points[points.len() - 2];
        assert!((turned.x - 40.0).abs() < 1e-3 && (turned.y - (14.0 + ELBOW)).abs() < 1e-3);
        let points = elbow(pos2(40.0, 0.0), pos2(0.0, 14.0), false);
        assert!((points[1].x - 40.0).abs() < 1e-3 && (points[1].y - (14.0 - ELBOW)).abs() < 1e-3);
        let turned = points[points.len() - 2];
        assert!((turned.y - 14.0).abs() < 1e-3 && (turned.x - (40.0 - ELBOW)).abs() < 1e-3);
        assert_eq!(elbow(pos2(5.0, 0.0), pos2(5.0, 14.0), false).len(), 2);
        let tight = elbow(pos2(0.0, 14.0), pos2(4.0, 28.0), true);
        assert!(tight.iter().all(|point| point.x >= -1e-3 && point.x <= 4.0 + 1e-3));
    }

    #[test]
    fn names_fold_into_folders_listed_before_names() {
        let names = ["main", "feat/b", "fix/x", "feat/a", "feat/deep/c", "feat-x"];
        let rows = tree_rows(&names, "files", &BTreeSet::new(), false);
        let shown: Vec<String> = rows
            .iter()
            .map(|row| match row {
                TreeRow::Folder { name, depth, count, .. } => format!("{}{name}/ ({count})", "  ".repeat(*depth)),
                TreeRow::Leaf { index, depth, name } => format!("{}{name} #{index}", "  ".repeat(*depth)),
            })
            .collect();
        assert_eq!(
            shown,
            ["feat/ (3)", "  deep/ (1)", "    c #4", "  a #3", "  b #1", "fix/ (1)", "  x #2", "feat-x #5", "main #0",]
        );
        let folded = BTreeSet::from(["files:feat".to_owned()]);
        let rows = tree_rows(&names, "files", &folded, false);
        assert_eq!(rows.len(), 5);
        assert!(matches!(&rows[0], TreeRow::Folder { open: false, count: 3, .. }));
        assert_eq!(tree_rows(&names, "files", &folded, true).len(), 9);
        let folded = BTreeSet::from(["files:feat/deep".to_owned()]);
        let rows = tree_rows(&names, "files", &folded, false);
        assert!(rows.contains(&TreeRow::Folder {
            key: "files:feat/deep".to_owned(),
            name: "deep".to_owned(),
            depth: 1,
            open: false,
            count: 1
        }));
        assert_eq!(rows.len(), 8);
    }

    #[test]
    fn branch_folders_start_folded_and_open_when_toggled() {
        let names = ["main", "feat/a", "feat/b"];
        let rows = tree_rows(&names, "local", &BTreeSet::new(), false);
        assert_eq!(rows.len(), 2, "feat/ folded, main");
        assert!(matches!(&rows[0], TreeRow::Folder { open: false, count: 2, .. }));
        let opened = BTreeSet::from(["local:feat".to_owned()]);
        assert_eq!(tree_rows(&names, "local", &opened, false).len(), 4);
        assert_eq!(tree_rows(&names, "local", &BTreeSet::new(), true).len(), 4, "a filter opens them");
    }

    #[test]
    fn a_path_splits_into_its_folder_and_its_name() {
        assert_eq!(split_path("crates/vcs/src/lib.rs"), ("crates/vcs/src/", "lib.rs"));
        assert_eq!(split_path("Cargo.toml"), ("", "Cargo.toml"));
    }

    #[test]
    fn tints_mix_the_lane_into_the_background() {
        let p = IdePalette::zen_dark();
        let lane = p.graph[0];
        assert_eq!(mix(p.editor_background, lane, 0.0), p.editor_background);
        assert_eq!(mix(p.editor_background, lane, 1.0), lane);
        let distance = |color: Color32| (i32::from(color.b()) - i32::from(p.editor_background.b())).abs();
        let (row, hovered, selected) =
            (row_tint(&p, lane, Tint::Row), row_tint(&p, lane, Tint::Hovered), row_tint(&p, lane, Tint::Selected));
        assert!(distance(row) < distance(hovered) && distance(hovered) < distance(selected));
        assert_ne!(pill_fill(&p, lane, true), pill_fill(&p, lane, false));
    }
}
