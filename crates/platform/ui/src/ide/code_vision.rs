use std::path::{Path, PathBuf};

use egui::{Color32, CursorIcon, FontId, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::language::code_vision::{CodeVisionList, Lens};
use zen_workbench::{Document, Workbench};

use super::IdePalette;
use super::soft_wrap::ShownRows;

const MARGIN: usize = 40;
const POLL: f64 = 0.1;
const REREAD: f64 = 2.0;

#[derive(Clone, Debug, Default)]
pub(super) struct RowLayout {
    line_height: f32,
    marked: Vec<usize>,
}

impl RowLayout {
    pub(super) fn new(line_height: f32, rows: &[usize], lines: &[usize]) -> Self {
        let mut marked: Vec<usize> = lines.iter().filter_map(|line| rows.binary_search(line).ok()).collect();
        marked.sort_unstable();
        marked.dedup();
        Self { line_height, marked }
    }

    fn block_start(&self, rows: &ShownRows, row: usize) -> usize {
        rows.top(row) + self.marked.partition_point(|&marked| marked < row)
    }

    pub(super) fn top(&self, rows: &ShownRows, row: usize) -> f32 {
        (rows.top(row) + self.marked.partition_point(|&marked| marked <= row)) as f32 * self.line_height
    }

    pub(super) fn height(&self, rows: &ShownRows, count: usize) -> f32 {
        self.block_start(rows, count) as f32 * self.line_height
    }

    pub(super) fn row_at(&self, rows: &ShownRows, y: f32) -> usize {
        let slot = (y / self.line_height).floor().max(0.0) as usize;
        let end = self.block_start(rows, rows.len());
        if slot >= end {
            return rows.len() + (slot - end);
        }
        let (mut above, mut below) = (0, rows.len());
        while above < below {
            let middle = (above + below) / 2;
            if self.block_start(rows, middle) <= slot {
                above = middle + 1;
            } else {
                below = middle;
            }
        }
        above.saturating_sub(1)
    }

    pub(super) fn visual_at(&self, rows: &ShownRows, y: f32) -> f32 {
        let slot = (y / self.line_height).max(0.0);
        let row = self.row_at(rows, y);
        if row >= rows.len() {
            return slot - self.marked.len() as f32;
        }
        let text = self.top(rows, row) / self.line_height;
        rows.top(row) as f32 + (slot - text).max(0.0)
    }

    pub(super) fn y_of_visual(&self, rows: &ShownRows, visual: f32) -> f32 {
        let row = rows.row_at(visual.max(0.0) as usize).min(rows.len().saturating_sub(1));
        (visual + self.marked.partition_point(|&marked| marked <= row) as f32) * self.line_height
    }

    pub(super) fn is_marked(&self, row: usize) -> bool {
        self.marked.binary_search(&row).is_ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Click {
    Usages(usize),
    Authors(String),
}

struct Shown {
    path: PathBuf,
    version: u64,
    anchors: Vec<usize>,
    lines: Vec<usize>,
    lenses: Vec<Lens>,
    pending: bool,
}

struct Asked {
    path: PathBuf,
    version: u64,
    window: (usize, usize),
    kinds: (bool, bool),
    at: f64,
}

#[derive(Default)]
pub(super) struct CodeVisionView {
    pub(super) enabled: bool,
    pub(super) kinds: (bool, bool),
    shown: Option<Shown>,
    asked: Option<Asked>,
    caret: Option<(PathBuf, u64, usize, usize)>,
    drawn: Vec<(usize, Rect, Click)>,
}

impl CodeVisionView {
    pub(super) fn follow(&mut self, document: &Document) {
        if !self.enabled {
            self.caret = None;
            return;
        }
        let head = document.selections().primary().head;
        let snapshot = document.snapshot();
        let chars = snapshot.len_chars();
        if let Some(shown) = &mut self.shown
            && shown.path == document.path()
            && shown.version != document.version()
            && let Some((path, version, old_head, old_chars)) = &self.caret
            && *path == shown.path
            && *version == shown.version
        {
            let delta = chars as isize - *old_chars as isize;
            let at = (*old_head).min(head);
            let removed = (-delta).max(0) as usize;
            let moved = |offset: usize| {
                if offset >= at + removed {
                    offset.saturating_add_signed(delta)
                } else if offset > at {
                    at
                } else {
                    offset
                }
            };
            let line = |offset: usize| snapshot.offset_to_point(offset.min(chars)).map_or(0, |point| point.line);
            for anchor in &mut shown.anchors {
                *anchor = moved(*anchor);
            }
            shown.lines = shown.anchors.iter().map(|&anchor| line(anchor)).collect();
            for lens in &mut shown.lenses {
                lens.offset = moved(lens.offset);
                lens.line = line(lens.offset);
            }
            shown.version = document.version();
        }
        match &mut self.caret {
            Some((path, version, old_head, old_chars)) if path == document.path() => {
                (*version, *old_head, *old_chars) = (document.version(), head, chars);
            }
            caret => *caret = Some((document.path().to_owned(), document.version(), head, chars)),
        }
    }

    pub(super) fn refresh(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        settled: bool,
        (first, last): (usize, usize),
    ) {
        self.drawn.clear();
        if !self.enabled {
            self.shown = None;
            self.asked = None;
            return;
        }
        if self.shown.as_ref().is_some_and(|shown| shown.path != path) {
            self.shown = None;
        }
        let Some(version) = session.state::<Workbench>().and_then(Workbench::active_document).map(Document::version)
        else {
            return;
        };
        let window = self
            .asked
            .as_ref()
            .filter(|asked| asked.path == path && asked.window.0 <= first && last <= asked.window.1)
            .map_or((first.saturating_sub(MARGIN), last + MARGIN), |asked| asked.window);
        let now = ui.input(|input| input.time);
        let pending = self.shown.as_ref().is_none_or(|shown| shown.pending);
        let fresh = self.asked.as_ref().is_some_and(|asked| {
            asked.path == path
                && asked.version == version
                && asked.window == window
                && asked.kinds == self.kinds
                && now - asked.at < if pending { POLL } else { REREAD }
        });
        if !settled || fresh {
            if pending && settled {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(POLL));
            }
            return;
        }
        self.asked = Some(Asked { path: path.to_owned(), version, window, kinds: self.kinds, at: now });
        let params = json!({ "path": path, "fromLine": window.0, "toLine": window.1 });
        let Some(list) = registry
            .dispatch(session, "editor.codeVision.list", params)
            .ok()
            .and_then(|value| serde_json::from_value::<CodeVisionList>(value).ok())
        else {
            return;
        };
        if list.pending {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(POLL));
        }
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
        if list.version != document.version() {
            return;
        }
        if list.lines.is_empty()
            && list.pending
            && let Some(shown) = self.shown.as_mut().filter(|shown| shown.path == path)
        {
            shown.pending = true;
            return;
        }
        let snapshot = document.snapshot();
        let anchors = list
            .lines
            .iter()
            .map(|&line| snapshot.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default())
            .collect();
        self.shown = Some(Shown {
            path: list.path,
            version: list.version,
            anchors,
            lines: list.lines,
            lenses: list.lenses,
            pending: list.pending,
        });
    }

    pub(super) fn lines(&self, path: &Path) -> &[usize] {
        match &self.shown {
            Some(shown) if self.enabled && shown.path == path => &shown.lines,
            _ => &[],
        }
    }

    pub(super) fn paint(
        &mut self,
        ui: &Ui,
        id: egui::Id,
        line: usize,
        rect: Rect,
        font: &FontId,
        palette: &IdePalette,
    ) -> Option<Click> {
        let lenses = &self.shown.as_ref()?.lenses;
        let lens = lenses[lenses.binary_search_by_key(&line, |lens| lens.line).ok()?].clone();
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let font = FontId::new((font.size - 1.0).max(8.0), font.family.clone());
        let gap = font.size;
        let mut x = rect.left();
        let mut clicked = None;
        let mut parts: Vec<(String, Click, bool)> = Vec::new();
        if let Some(count) = lens.usages {
            parts.push((t!("code-vision-usages", count = count), Click::Usages(lens.offset), false));
        }
        if let Some(authors) = &lens.authors {
            let label = if authors.others == 0 {
                authors.name.clone()
            } else {
                t!("code-vision-authors", name = authors.name.as_str(), others = authors.others)
            };
            parts.push((label, Click::Authors(authors.commit.clone()), true));
        }
        for (index, (label, click, person)) in parts.into_iter().enumerate() {
            let icon = if person { font.size } else { 0.0 };
            let galley = painter.layout_no_wrap(label, font.clone(), palette.text_secondary);
            let size = galley.size();
            let hit = Rect::from_min_size(pos2(x, rect.center().y - size.y / 2.0), vec2(icon + size.x, size.y));
            let response = ui.interact(hit, id.with(("code-vision", line, index)), Sense::click());
            crate::accessibility::name(&response, egui::Role::Link, galley.text());
            let color = if response.hovered() { palette.text } else { palette.text_secondary };
            if person {
                person_icon(&painter, Rect::from_min_size(hit.min, vec2(icon, size.y)).shrink(1.5), color);
            }
            let text_at = pos2(hit.left() + icon, hit.top());
            painter.galley(text_at, galley, color);
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                painter.hline(text_at.x..=hit.right(), hit.bottom() - 1.0, Stroke::new(1.0, color));
            }
            if response.clicked() {
                clicked = Some(click.clone());
            }
            self.drawn.push((line, hit, click));
            x = hit.right() + gap;
        }
        clicked
    }
}

pub(super) fn person_icon(painter: &Painter, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    let center = rect.center();
    painter.circle_stroke(pos2(center.x, center.y - side * 0.18), side * 0.2, Stroke::new(1.0, color));
    let body = Rect::from_center_size(pos2(center.x, center.y + side * 0.3), vec2(side * 0.75, side * 0.36));
    painter.rect_stroke(
        body,
        egui::CornerRadius { nw: (side * 0.3) as u8, ne: (side * 0.3) as u8, sw: 0, se: 0 },
        Stroke::new(1.0, color),
        egui::StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_below_a_lens_move_down_by_a_row() {
        let shown = ShownRows::new(vec![0, 1, 2, 5, 6]);
        let layout = RowLayout::new(10.0, &shown, &[2, 6, 3]);
        assert_eq!(layout.marked, [2, 4], "a folded-away line gets no row");
        let tops: Vec<f32> = (0..5).map(|row| layout.top(&shown, row)).collect();
        assert_eq!(tops, [0.0, 10.0, 30.0, 40.0, 60.0]);
        assert_eq!(layout.height(&shown, 5), 70.0);
        assert_eq!(layout.height(&shown, 3), 40.0);
        let rows: Vec<usize> =
            [0.0, 15.0, 20.0, 29.0, 30.0, 45.0, 50.0, 65.0, 75.0].map(|y| layout.row_at(&shown, y)).to_vec();
        assert_eq!(rows, [0, 1, 2, 2, 2, 3, 4, 4, 5], "a lens row belongs to the row under it");
        assert!(layout.is_marked(2) && !layout.is_marked(3));
        let plain_rows = ShownRows::new(vec![0, 1, 2]);
        let plain = RowLayout::new(10.0, &plain_rows, &[]);
        assert_eq!(
            (plain.top(&plain_rows, 2), plain.row_at(&plain_rows, 25.0), plain.height(&plain_rows, 3)),
            (20.0, 2, 30.0)
        );
    }

    #[test]
    fn lens_rows_go_above_the_first_visual_row_of_a_wrapped_line() {
        let shown = ShownRows::wrapped(vec![0, 1, 2, 3], vec![0, 1, 4, 5, 7]);
        let layout = RowLayout::new(10.0, &shown, &[1, 3]);
        let tops: Vec<f32> = (0..4).map(|row| layout.top(&shown, row)).collect();
        assert_eq!(tops, [0.0, 20.0, 50.0, 70.0]);
        assert_eq!(layout.height(&shown, 4), 90.0);
        let rows: Vec<usize> = [5.0, 15.0, 45.0, 55.0, 65.0, 85.0, 95.0].map(|y| layout.row_at(&shown, y)).to_vec();
        assert_eq!(rows, [0, 1, 1, 2, 3, 3, 4]);
        assert_eq!(layout.visual_at(&shown, 15.0), 1.0, "a lens row is at the start of the row under it");
        assert_eq!(layout.visual_at(&shown, 45.0), 3.5);
        assert_eq!(layout.y_of_visual(&shown, 3.5), 45.0);
        assert_eq!(layout.y_of_visual(&shown, 5.0), 70.0);
    }

    #[test]
    fn a_row_is_found_by_its_place_in_a_long_file() {
        let rows: Vec<usize> = (0..100_000).collect();
        let declarations: Vec<usize> = (0..100_000).step_by(7).collect();
        let rows = ShownRows::new(rows);
        let layout = RowLayout::new(10.0, &rows, &declarations);
        for row in [0, 1, 6, 7, 8, 50_000, 99_995, 99_999] {
            let top = layout.top(&rows, row);
            assert_eq!(layout.row_at(&rows, top + 5.0), row, "the text of row {row}");
            if layout.is_marked(row) {
                assert_eq!(layout.row_at(&rows, top - 5.0), row, "the lens above row {row}");
            }
        }
    }
}

#[cfg(test)]
mod view_tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use egui::Event;
    use serde_json::{Value, json};
    use zen_lsp::fake::FakeServer;
    use zen_workbench::language::usages::UsagesState;
    use zen_workbench::language::{Languages, ServerSpec};

    use super::*;
    use crate::ide::editor_view::EditorView;

    const MAIN: &str =
        "package main\n\nimport \"fmt\"\n\nfunc Greet() {\n\tfmt.Println(\"hi\")\n}\n\nfunc main() {\n\tGreet()\n}\n";

    struct Window {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        view: EditorView,
        path: PathBuf,
        _server: FakeServer,
        _dir: tempfile::TempDir,
    }

    fn range(line: u64, from: u64, to: u64) -> Value {
        json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
    }

    fn symbol(name: &str, line: u64, last: u64) -> Value {
        json!({
            "name": name,
            "kind": 12,
            "range": { "start": { "line": line, "character": 0 }, "end": { "line": last, "character": 1 } },
            "selectionRange": range(line, 5, 5 + name.len() as u64),
        })
    }

    impl Window {
        fn new(git: bool) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            zen_i18n::register(zen_workbench::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("main.go");
            std::fs::write(&path, MAIN).unwrap();
            if git {
                for args in [&["init", "-q", "-b", "main"][..], &["add", "main.go"], &["commit", "-q", "-m", "First"]] {
                    let status = std::process::Command::new("git")
                        .current_dir(dir.path())
                        .args(args)
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_NOSYSTEM", "1")
                        .env("GIT_AUTHOR_NAME", "Ada Lovelace")
                        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
                        .env("GIT_COMMITTER_NAME", "Ada Lovelace")
                        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
                        .status()
                        .unwrap();
                    assert!(status.success(), "git {args:?}");
                }
            }
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.settings_mut().set(zen_workbench::language::code_vision::AUTHORS, git).unwrap();
            let server = FakeServer::new();
            server.set_capabilities(json!({
                "textDocumentSync": { "openClose": true, "change": 2 },
                "referencesProvider": true,
                "documentSymbolProvider": true,
            }));
            server.on("textDocument/documentSymbol", json!([symbol("Greet", 4, 6), symbol("main", 8, 10)]));
            let uri = zen_lsp::position::file_uri(&path);
            server.on_with("textDocument/references", move |params| match params["position"]["line"].as_u64() {
                Some(4) => json!([{ "uri": uri, "range": range(9, 1, 6) }]),
                _ => json!([]),
            });
            let launched = server.clone();
            session.state_mut::<Languages>().set_specs(vec![ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            }]);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            let mut view = EditorView::default();
            view.code_vision.enabled = true;
            Self { ctx: egui::Context::default(), registry, session, view, path, _server: server, _dir: dir }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let _ = self.registry.dispatch(&mut self.session, "language.servers.sync", json!({}));
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let mut output = self.ctx.run_ui(input, |ui| {
                self.view.show(ui, &self.registry, &mut self.session, None, &palette);
            });
            output.textures_delta.clear();
        }

        fn until_drawn(&mut self, parts: usize) -> Vec<(usize, Rect, Click)> {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                self.frame(Vec::new());
                let drawn = self.view.code_vision.drawn.clone();
                if drawn.len() == parts || Instant::now() > deadline {
                    return drawn;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn click(&mut self, at: egui::Pos2) {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), press(true)]);
            self.frame(vec![press(false)]);
        }

        fn caret(&self) -> usize {
            self.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head
        }
    }

    #[test]
    fn the_usages_are_drawn_above_the_declarations_and_a_click_shows_them() {
        let mut window = Window::new(false);
        let drawn = window.until_drawn(2);
        let lines: Vec<usize> = drawn.iter().map(|(line, ..)| *line).collect();
        assert_eq!(lines, [4, 8]);
        let greet_offset = MAIN.find("Greet").unwrap();
        assert_eq!(drawn[0].2, Click::Usages(greet_offset));
        let shown = window.view.code_vision.shown.as_ref().unwrap();
        let labels: Vec<Option<usize>> = shown.lenses.iter().map(|lens| lens.usages).collect();
        assert_eq!(labels, [Some(1), Some(0)]);
        let height = drawn[0].1.height();
        let line_height = drawn[1].1.center().y - drawn[0].1.center().y;
        assert!((line_height / 5.0 - height).abs() < height, "the second lens is five rows further: {line_height}");
        let row = line_height / 5.0;
        assert!((drawn[0].1.center().y - 4.5 * row).abs() < 1.0, "the lens takes the row of line 4");

        let text_of_line_4 = pos2(drawn[0].1.left() + 2.0, 5.5 * row);
        window.click(text_of_line_4);
        let line_4 = MAIN.lines().take(4).map(|line| line.chars().count() + 1).sum::<usize>();
        assert_eq!(window.caret(), line_4, "a click on the line under a lens lands on that line");

        window.click(drawn[0].1.center());
        let state = window
            .registry
            .dispatch(&mut window.session, "navigate.showUsages.get", json!({ "waitMs": 5000 }))
            .unwrap();
        assert_eq!(
            (state["search"].clone(), state["open"].clone()),
            (json!(1), json!(true)),
            "a click shows the usages"
        );
        assert_eq!(state["symbol"], "Greet");
        assert!(
            window.session.state::<UsagesState>().is_none_or(|usages| usages.search == 0),
            "not in the Find tool window"
        );
        window.frame(Vec::new());
        assert!(window.view.has_popup(), "the Show Usages popup is open");
        let escape = Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        window.frame(vec![escape]);
        let state = window.registry.dispatch(&mut window.session, "navigate.showUsages.get", json!({})).unwrap();
        assert_eq!(state["open"], false, "Escape closes it");
        window.frame(Vec::new());
        assert!(!window.view.has_popup());
    }

    #[test]
    fn switched_off_nothing_is_drawn_and_the_rows_close() {
        let mut window = Window::new(false);
        assert_eq!(window.until_drawn(2).len(), 2);
        window.view.code_vision.enabled = false;
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.view.code_vision.drawn.is_empty());
        assert!(window.view.code_vision.lines(&window.path).is_empty());
    }

    #[test]
    fn the_author_is_drawn_after_the_usages_and_a_click_shows_the_commit() {
        let mut window = Window::new(true);
        let head =
            std::process::Command::new("git").current_dir(window._dir.path()).args(["rev-parse", "HEAD"]).output();
        let commit = String::from_utf8(head.unwrap().stdout).unwrap().trim().to_owned();
        let drawn = window.until_drawn(4);
        let clicks: Vec<(usize, Click)> = drawn.iter().map(|(line, _, click)| (*line, click.clone())).collect();
        let greet = MAIN.find("Greet").unwrap();
        let main = MAIN.find("main()").unwrap();
        assert_eq!(
            clicks,
            [
                (4, Click::Usages(greet)),
                (4, Click::Authors(commit.clone())),
                (8, Click::Usages(main)),
                (8, Click::Authors(commit.clone()))
            ]
        );
        assert!(drawn[1].1.left() > drawn[0].1.right(), "the author follows the usages");
        let shown = window.view.code_vision.shown.as_ref().unwrap();
        assert_eq!(shown.lenses[0].authors.as_ref().map(|authors| authors.name.as_str()), Some("Ada Lovelace"));
        window.click(drawn[1].1.center());
        assert_eq!(window.view.take_blame_request(), Some(commit), "a click on the author shows its commit");
    }

    #[test]
    fn the_counts_read_as_usages_and_authors() {
        let usages = |count: usize| zen_i18n::source_text("code-vision-usages", &[("count", count.into())]);
        assert_eq!([usages(0), usages(1), usages(7)], ["no usages", "1 usage", "7 usages"]);
        let authors = zen_i18n::source_text("code-vision-authors", &[("name", "Ada".into()), ("others", 2.into())]);
        assert_eq!(authors.replace(['\u{2068}', '\u{2069}'], ""), "Ada +2");
    }

    #[test]
    fn while_typing_goes_on_the_lenses_move_with_the_edits() {
        let mut window = Window::new(false);
        assert_eq!(window.until_drawn(2).len(), 2);
        let path = window.path.clone();
        let line_1 = MAIN.find('\n').unwrap() + 1;
        let set = json!({ "selections": [{ "head": line_1 }] });
        window.registry.dispatch(&mut window.session, "editor.selection.set", set).unwrap();
        window.frame(Vec::new());
        window.registry.dispatch(&mut window.session, "editor.edit.insert", json!({ "text": "\n\n" })).unwrap();
        window.frame(Vec::new());
        assert_eq!(window.view.code_vision.lines(&path), [6, 10], "moved down with the new lines");
        let end = MAIN.chars().count() + 2;
        let set = json!({ "selections": [{ "head": end }] });
        window.registry.dispatch(&mut window.session, "editor.selection.set", set).unwrap();
        window.frame(Vec::new());
        window.registry.dispatch(&mut window.session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        window.frame(Vec::new());
        assert_eq!(window.view.code_vision.lines(&path), [6, 10], "an edit below leaves them");
    }
}
