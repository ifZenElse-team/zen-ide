use std::collections::{BTreeMap, HashSet};

use zen_i18n::t;
use zen_workbench::vcs::log::RefLabelInfo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum When {
    Moments,
    Minutes(i64),
    Today(String),
    Yesterday(String),
    Date { day: i64, month: i64, year: i64, time: String },
}

pub(super) fn when(time: i64, now: i64, offset: i32) -> When {
    let ago = now - time;
    if (0..60).contains(&ago) {
        return When::Moments;
    }
    if (60..3600).contains(&ago) {
        return When::Minutes(ago / 60);
    }
    let local = time + i64::from(offset);
    let (day, today) = (local.div_euclid(86_400), (now + i64::from(offset)).div_euclid(86_400));
    let minutes = local.rem_euclid(86_400) / 60;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    if day == today {
        return When::Today(clock);
    }
    if day == today - 1 {
        return When::Yesterday(clock);
    }
    let (year, month, day) = civil(day);
    When::Date { day, month, year, time: clock }
}

pub(super) fn relative_date(time: i64, now: i64, offset: i32) -> String {
    match when(time, now, offset) {
        When::Moments => t!("vcs-log-moments"),
        When::Minutes(count) => t!("vcs-log-minutes", count = count),
        When::Today(time) => t!("vcs-log-today", time = time.as_str()),
        When::Yesterday(time) => t!("vcs-log-yesterday", time = time.as_str()),
        When::Date { day, month, year, time } => full_date(day, month, year, &time),
    }
}

pub(super) fn date_and_time(local: i64) -> (String, String) {
    let (year, month, day) = civil(local.div_euclid(86_400));
    let minutes = local.rem_euclid(86_400) / 60;
    (format!("{day:02}.{month:02}.{year:04}"), format!("{:02}:{:02}", minutes / 60, minutes % 60))
}

fn full_date(day: i64, month: i64, year: i64, time: &str) -> String {
    t!(
        "vcs-log-full-date",
        day = format!("{day:02}").as_str(),
        month = format!("{month:02}").as_str(),
        year = format!("{year:04}").as_str(),
        time = time
    )
}

pub(super) fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub(super) fn parse_day(text: &str) -> Option<i64> {
    let mut parts = text.trim().splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (civil(days).2 == day).then_some(days)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LabelKind {
    Current,
    Local,
    Remote,
    Tag,
    Head,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RowLabel {
    pub(super) text: String,
    pub(super) kind: LabelKind,
}

pub(super) fn row_labels(
    labels: &[RefLabelInfo],
    current: Option<&str>,
    upstream: &dyn Fn(&str) -> Option<String>,
) -> Vec<RowLabel> {
    let names =
        |kind: &'static str| labels.iter().filter(move |label| label.kind == kind).map(|label| label.name.as_str());
    let mut remotes: Vec<&str> = names("remote").collect();
    let mut rows = Vec::new();
    for local in names("local") {
        let kind = if Some(local) == current { LabelKind::Current } else { LabelKind::Local };
        let text = match upstream(local).and_then(|up| remotes.iter().position(|remote| *remote == up)) {
            Some(at) => {
                let remote = remotes.remove(at);
                let name = remote.split_once('/').map_or(remote, |(name, _)| name);
                t!("vcs-log-label-both", remote = name, local = short_ref(local).as_str())
            }
            None => short_ref(local),
        };
        rows.push(RowLabel { text, kind });
    }
    rows.extend(remotes.into_iter().map(|remote| RowLabel { text: short_ref(remote), kind: LabelKind::Remote }));
    rows.extend(names("tag").map(|tag| RowLabel { text: tag.to_owned(), kind: LabelKind::Tag }));
    if current.is_none() && labels.iter().any(|label| label.kind == "head") {
        rows.push(RowLabel { text: "HEAD".to_owned(), kind: LabelKind::Head });
    }
    rows.sort_by_key(|row| row.kind);
    rows
}

pub(super) fn short_ref(name: &str) -> String {
    match name.rsplit_once('/') {
        Some((_, last)) if name.chars().count() > 24 => format!("../{last}"),
        _ => name.to_owned(),
    }
}

pub(super) const MIN_SUBJECT: f32 = 160.0;
pub(super) const LABEL_GAP: f32 = 8.0;
pub(super) const MIN_LABEL_NAME: f32 = 28.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LabelsLayout {
    pub(super) subject: f32,
    pub(super) shown: Vec<f32>,
    pub(super) hidden: usize,
}

pub(super) fn labels_layout(room: f32, subject: f32, labels: &[f32], more: f32) -> LabelsLayout {
    let row = |count: usize| -> f32 {
        labels[..count].iter().sum::<f32>()
            + LABEL_GAP * count as f32
            + if count < labels.len() { more + LABEL_GAP } else { 0.0 }
    };
    if labels.is_empty() {
        return LabelsLayout { subject: room.max(0.0), shown: Vec::new(), hidden: 0 };
    }
    let all = row(labels.len());
    if room - all >= subject.min(MIN_SUBJECT) {
        return LabelsLayout { subject: (room - all).max(0.0), shown: labels.to_vec(), hidden: 0 };
    }
    let subject = subject.min(MIN_SUBJECT).min(room.max(0.0));
    let free = room - subject;
    if let Some(count) = (1..labels.len()).rev().find(|&count| row(count) <= free) {
        return LabelsLayout { subject, shown: labels[..count].to_vec(), hidden: labels.len() - count };
    }
    let rest = if labels.len() > 1 { more + LABEL_GAP } else { 0.0 };
    let first = free - LABEL_GAP - rest;
    if first >= MIN_LABEL_NAME + 16.0 {
        return LabelsLayout { subject, shown: vec![first.min(labels[0])], hidden: labels.len() - 1 };
    }
    if room >= more + LABEL_GAP {
        let subject = subject.min(room - more - LABEL_GAP);
        return LabelsLayout { subject, shown: Vec::new(), hidden: labels.len() };
    }
    LabelsLayout { subject: room.max(0.0), shown: Vec::new(), hidden: 0 }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ChangeRow {
    Folder { key: String, name: String, depth: usize, files: usize, expanded: bool },
    File { index: usize, name: String, depth: usize },
}

pub(super) fn change_rows(paths: &[&str], grouped: bool, collapsed: &HashSet<String>) -> Vec<ChangeRow> {
    if !grouped {
        return paths
            .iter()
            .enumerate()
            .map(|(index, path)| ChangeRow::File { index, name: leaf(path).to_owned(), depth: 0 })
            .collect();
    }
    #[derive(Default)]
    struct Node {
        folders: BTreeMap<String, Node>,
        files: Vec<(String, usize)>,
    }
    impl Node {
        fn count(&self) -> usize {
            self.files.len() + self.folders.values().map(Node::count).sum::<usize>()
        }
    }
    let mut root = Node::default();
    for (index, path) in paths.iter().enumerate() {
        let mut node = &mut root;
        let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let name = parts.pop().unwrap_or_default();
        for part in parts {
            node = node.folders.entry(part.to_owned()).or_default();
        }
        node.files.push((name.to_owned(), index));
    }
    fn walk(node: &Node, prefix: &str, depth: usize, collapsed: &HashSet<String>, rows: &mut Vec<ChangeRow>) {
        for (name, folder) in &node.folders {
            let (mut shown, mut key, mut inner) = (name.clone(), format!("{prefix}{name}"), folder);
            while inner.files.is_empty() && inner.folders.len() == 1 {
                let (next, deeper) = inner.folders.iter().next().unwrap_or_else(|| unreachable!());
                shown = format!("{shown}/{next}");
                key = format!("{key}/{next}");
                inner = deeper;
            }
            let expanded = !collapsed.contains(&key);
            rows.push(ChangeRow::Folder { key: key.clone(), name: shown, depth, files: inner.count(), expanded });
            if expanded {
                walk(inner, &format!("{key}/"), depth + 1, collapsed, rows);
            }
        }
        let mut files = node.files.clone();
        files.sort();
        rows.extend(files.into_iter().map(|(name, index)| ChangeRow::File { index, name, depth }));
    }
    let mut rows = Vec::new();
    walk(&root, "", 0, collapsed, &mut rows);
    rows
}

pub(super) fn change_folders(paths: &[&str]) -> Vec<String> {
    change_rows(paths, true, &HashSet::new())
        .into_iter()
        .filter_map(|row| match row {
            ChangeRow::Folder { key, .. } => Some(key),
            ChangeRow::File { .. } => None,
        })
        .collect()
}

pub(super) fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(super) fn share(room: f32, minimum: &[f32], natural: &[f32]) -> Vec<f32> {
    let mut widths: Vec<f32> = minimum.iter().zip(natural).map(|(min, natural)| min.min(*natural)).collect();
    let mut extra = room - widths.iter().sum::<f32>();
    loop {
        let wanting: Vec<usize> = (0..widths.len()).filter(|&at| natural[at] - widths[at] > 0.5).collect();
        if wanting.is_empty() || extra <= 0.5 {
            return widths;
        }
        let each = extra / wanting.len() as f32;
        for at in wanting {
            let add = (natural[at] - widths[at]).min(each);
            widths[at] += add;
            extra -= add;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn recent_dates_are_relative() {
        let now = 1_792_411_200;
        assert_eq!(when(now - 30, now, 0), When::Moments);
        assert_eq!(when(now - 5 * 60, now, 0), When::Minutes(5));
        assert_eq!(when(now - 3 * 3600, now, 0), When::Today("09:00".into()));
        assert_eq!(when(now - DAY + 7 * 3600 + 47 * 60, now, 0), When::Yesterday("19:47".into()));
        assert_eq!(when(now - 3 * DAY, now, 0), When::Date { day: 16, month: 10, year: 2026, time: "12:00".into() });
        assert_eq!(when(now - 13 * 3600, now, 7 * 3600), When::Today("06:00".into()));
        assert_eq!(when(now - 13 * 3600, now, 0), When::Yesterday("23:00".into()));
    }

    #[test]
    fn dates_are_read_back() {
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(parse_day("2026-10-16"), Some(1_792_108_800 / DAY));
        assert_eq!(parse_day(" 2000-02-29 "), Some(951_782_400 / DAY));
        assert_eq!(parse_day("2026-02-31"), None);
        assert_eq!(parse_day("16.10.2026"), None);
        assert_eq!(date_and_time(1_792_108_800 + 19 * 3600 + 36 * 60), ("16.10.2026".into(), "19:36".into()));
    }

    fn label(name: &str, kind: &str) -> RefLabelInfo {
        RefLabelInfo { name: name.to_owned(), kind: kind.to_owned() }
    }

    #[test]
    fn every_ref_gets_a_label_the_current_branch_first() {
        let upstream = |name: &str| (name == "main" || name == "dev").then(|| format!("origin/{name}"));
        let labels = [
            label("HEAD", "head"),
            label("v1", "tag"),
            label("feature/x", "local"),
            label("origin/feature/x", "remote"),
            label("main", "local"),
        ];
        let rows = row_labels(&labels, Some("main"), &upstream);
        let shown: Vec<(&str, LabelKind)> = rows.iter().map(|row| (row.text.as_str(), row.kind)).collect();
        assert_eq!(
            shown,
            [
                ("main", LabelKind::Current),
                ("feature/x", LabelKind::Local),
                ("origin/feature/x", LabelKind::Remote),
                ("v1", LabelKind::Tag),
            ],
            "HEAD is the ring, not a label; a remote branch that is not the upstream keeps its own"
        );
        let rows = row_labels(&[label("dev", "local"), label("origin/dev", "remote")], Some("main"), &upstream);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, LabelKind::Local);
        assert!(rows[0].text.contains("origin") && rows[0].text.ends_with("dev"), "{}", rows[0].text);
        assert!(!rows[0].text.contains("origin/dev"));
        assert_eq!(short_ref("origin/main"), "origin/main");
        assert_eq!(short_ref("feature/published-course-short-description"), "../published-course-short-description");
        let rows = row_labels(&[label("HEAD", "head"), label("v2", "tag")], None, &upstream);
        assert_eq!(rows.last().map(|row| row.text.as_str()), Some("HEAD"));
    }

    #[test]
    fn changed_files_form_a_tree_by_directory() {
        let paths = [
            "docs/SCHEMA.md",
            "docs/FRONTEND_API.md",
            "internal/admin/blocked.go",
            "go.mod",
            "docs/brief/corporate.md",
        ];
        let rows = change_rows(&paths, true, &HashSet::new());
        let shown: Vec<String> = rows
            .iter()
            .map(|row| match row {
                ChangeRow::Folder { name, depth, files, .. } => format!("{depth}:{name}/ {files}"),
                ChangeRow::File { name, depth, .. } => format!("{depth}:{name}"),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "0:docs/ 3",
                "1:brief/ 1",
                "2:corporate.md",
                "1:FRONTEND_API.md",
                "1:SCHEMA.md",
                "0:internal/admin/ 1",
                "1:blocked.go",
                "0:go.mod",
            ]
        );
        let collapsed: HashSet<String> = ["docs".to_owned()].into();
        assert_eq!(change_rows(&paths, true, &collapsed).len(), 4, "a collapsed folder hides its files");
        assert_eq!(change_folders(&paths), ["docs", "docs/brief", "internal/admin"]);
        let flat = change_rows(&paths, false, &HashSet::new());
        assert_eq!(flat[0], ChangeRow::File { index: 0, name: "SCHEMA.md".into(), depth: 0 });
    }

    #[test]
    fn labels_are_never_cut_the_subject_gives_way_first() {
        let all = labels_layout(600.0, 200.0, &[90.0, 110.0], 30.0);
        assert_eq!(all, LabelsLayout { subject: 600.0 - 216.0, shown: vec![90.0, 110.0], hidden: 0 });
        let shorter = labels_layout(420.0, 400.0, &[90.0, 110.0], 30.0);
        assert_eq!(shorter, LabelsLayout { subject: 204.0, shown: vec![90.0, 110.0], hidden: 0 });
        let collapsed = labels_layout(320.0, 400.0, &[90.0, 110.0], 30.0);
        assert_eq!(collapsed, LabelsLayout { subject: MIN_SUBJECT, shown: vec![90.0], hidden: 1 });
        let short = labels_layout(296.0, 80.0, &[90.0, 110.0], 30.0);
        assert_eq!(short, LabelsLayout { subject: 80.0, shown: vec![90.0, 110.0], hidden: 0 });
        let short = labels_layout(290.0, 80.0, &[90.0, 110.0], 30.0);
        assert_eq!(short, LabelsLayout { subject: 80.0, shown: vec![90.0], hidden: 1 });
        let shortened = labels_layout(260.0, 400.0, &[150.0, 110.0], 30.0);
        assert_eq!(shortened, LabelsLayout { subject: MIN_SUBJECT, shown: vec![54.0], hidden: 1 });
        let only_more = labels_layout(200.0, 400.0, &[150.0, 110.0], 30.0);
        assert_eq!(only_more, LabelsLayout { subject: MIN_SUBJECT, shown: Vec::new(), hidden: 2 });
        for room in (0..900).step_by(7) {
            let room = room as f32;
            let layout = labels_layout(room, 300.0, &[90.0, 110.0, 70.0], 30.0);
            let more = if layout.hidden > 0 { 30.0 + LABEL_GAP } else { 0.0 };
            let used = layout.subject + layout.shown.iter().map(|w| w + LABEL_GAP).sum::<f32>() + more;
            assert!(used <= room.max(0.0) + 0.01, "{room}: {layout:?}");
            assert_eq!(layout.shown.len() + layout.hidden, if room >= 38.0 { 3 } else { 0 }, "{room}");
        }
    }

    #[test]
    fn the_filters_share_the_bar() {
        assert_eq!(share(500.0, &[40.0, 30.0, 50.0], &[40.0, 30.0, 120.0]), [40.0, 30.0, 120.0]);
        assert_eq!(share(200.0, &[40.0, 50.0, 50.0], &[40.0, 150.0, 90.0]), [40.0, 80.0, 80.0]);
        assert_eq!(share(220.0, &[40.0, 50.0, 50.0], &[40.0, 150.0, 70.0]), [40.0, 110.0, 70.0]);
        assert_eq!(share(60.0, &[40.0, 50.0], &[40.0, 90.0]), [40.0, 50.0]);
    }
}
