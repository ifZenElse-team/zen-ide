use std::cell::Cell;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Shape};

use super::*;
use crate::theme;

struct Synthetic {
    columns: Vec<GridColumn>,
    rows: u64,
    reads: Cell<usize>,
}

impl Synthetic {
    fn new(rows: u64, columns: usize) -> Self {
        let columns = (0..columns)
            .map(|index| GridColumn {
                sort: None,
                editable: false,
                filtered: false,
                not_null: false,
                primary: false,
                name: format!("column_{index}"),
                type_name: match index % 4 {
                    0 => "int8",
                    1 => "text",
                    2 => "bool",
                    _ => "timestamptz",
                }
                .to_owned(),
            })
            .collect();
        Self { columns, rows, reads: Cell::new(0) }
    }
}

impl GridSource for Synthetic {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn row_count(&self) -> u64 {
        self.rows
    }

    fn revision(&self) -> u64 {
        self.rows
    }

    fn cells(&self, rows: Range<u64>, columns: Range<usize>) -> Vec<Vec<GridCell>> {
        self.reads.set(self.reads.get() + 1);
        rows.map(|row| {
            columns
                .clone()
                .map(|column| match column % 4 {
                    0 => GridCell { text: row.to_string(), number: true, ..GridCell::default() },
                    1 if row % 7 == 0 => GridCell { null: true, ..GridCell::default() },
                    1 => GridCell { text: format!("r{row}c{column}"), ..GridCell::default() },
                    2 => GridCell {
                        text: (row % 2 == 0).to_string(),
                        boolean: Some(row % 2 == 0),
                        ..GridCell::default()
                    },
                    _ => GridCell { text: "2026-10-26 10:00:00+08".to_owned(), ..GridCell::default() },
                })
                .collect()
        })
        .collect()
    }
}

struct Harness {
    ctx: egui::Context,
    palette: IdePalette,
    state: GridState,
    time: f64,
    features: GridFeatures,
}

impl Harness {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        Self { ctx, palette, state: GridState::default(), time: 0.0, features: GridFeatures::default() }
    }

    fn frame(&mut self, source: &dyn GridSource, events: Vec<Event>) -> (GridOutcome, Vec<(Pos2, String)>) {
        self.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let (palette, state, features) = (&self.palette, &mut self.state, self.features);
        let mut outcome = GridOutcome::default();
        let mut output = self
            .ctx
            .run_ui(input, |ui| outcome = show_with(ui, palette, source, state, Id::NULL.with("grid"), features));
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        (outcome, texts)
    }
}

fn collect(shape: &Shape, out: &mut Vec<(Pos2, String)>) {
    match shape {
        Shape::Text(text) => out.push((text.pos, text.galley.text().to_owned())),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn shows(texts: &[(Pos2, String)], wanted: &str) -> bool {
    texts.iter().any(|(_, text)| text == wanted)
}

fn press(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }
}

#[test]
fn a_million_rows_scroll_to_the_end_without_losing_one() {
    let mut harness = Harness::new();
    let source = Synthetic::new(1_000_000, 300);
    harness.frame(&source, Vec::new());
    harness.state.scroll_to(999_990);
    let (outcome, texts) = harness.frame(&source, Vec::new());
    assert!(shows(&texts, "1000000"), "the last row's number shows: {:?}", &texts[..20.min(texts.len())]);
    assert!(shows(&texts, "999999"), "the last row's value shows");
    assert!(outcome.near_end);
    harness.state.scroll_to(400_123);
    let (outcome, texts) = harness.frame(&source, Vec::new());
    assert!(shows(&texts, "400124"), "past egui's f32 limit rows stay exact");
    assert!(shows(&texts, "r400124c1"));
    assert!(!outcome.near_end);
    assert_eq!(harness.state.top(), 400_123);
}

#[test]
fn null_is_marked_and_booleans_are_check_boxes() {
    let mut harness = Harness::new();
    let source = Synthetic::new(30, 4);
    let (_, texts) = harness.frame(&source, Vec::new());
    assert!(shows(&texts, "[NULL]"), "row 0 of column 1 is NULL");
    assert!(shows(&texts, "true") && shows(&texts, "column_2"));
    assert!(shows(&texts, "123") && shows(&texts, "abc"), "the header marks the columns' kinds");
}

#[test]
fn visible_cells_are_formatted_once_and_kept() {
    let mut harness = Harness::new();
    let source = Synthetic::new(1_000_000, 300);
    for _ in 0..3 {
        harness.frame(&source, Vec::new());
    }
    let first = harness.state.fetched();
    assert_eq!(first, 1, "the first frame reads once, later frames reuse it");
    harness.state.scroll_to(5);
    harness.frame(&source, Vec::new());
    assert_eq!(harness.state.fetched(), first, "a short scroll stays inside the rows read around the view");
    harness.state.scroll_to(500_000);
    harness.frame(&source, Vec::new());
    assert_eq!(harness.state.fetched(), first + 1);
    assert!(harness.state.widths().iter().all(|width| *width >= MIN_WIDTH));
}

#[test]
fn clicks_select_cells_rows_and_columns_and_copy_asks_for_them() {
    let mut harness = Harness::new();
    let source = Synthetic::new(100, 6);
    let (_, texts) = harness.frame(&source, Vec::new());
    let cell = texts.iter().find(|(_, text)| text == "r1c1").map(|(at, _)| *at).expect("r1c1 is shown");
    let other = texts.iter().find(|(_, text)| text == "r3c5").map(|(at, _)| *at).expect("r3c5 is shown");
    let at = cell + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(at), press(at, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(at, false, Modifiers::NONE)]);
    assert_eq!(harness.state.selection().map(|selection| selection.head), Some((1, 1)));
    let to = other + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(to), press(to, true, Modifiers::SHIFT)]);
    harness.frame(&source, vec![press(to, false, Modifiers::SHIFT)]);
    let selection = harness.state.selection().unwrap();
    assert_eq!(selection.bounds(100, 6), (1..4, 1..6));
    let copy =
        Event::Key { key: Key::C, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND };
    let (outcome, _) = harness.frame(&source, vec![copy]);
    assert_eq!(outcome.copy, Some((1..4, 1..6, false)));
    let down = Event::Key {
        key: Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    harness.frame(&source, vec![down]);
    assert_eq!(harness.state.selection().unwrap().head, (4, 5), "arrows move the selection");
    let (_, texts) = harness.frame(&source, Vec::new());
    let number = texts.iter().find(|(_, text)| text == "1").map(|(at, _)| *at).expect("row number 1");
    let column = texts.iter().find(|(_, text)| text == "column_0").map(|(at, _)| *at).expect("header");
    let header = column + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(header), press(header, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(header, false, Modifiers::NONE)]);
    assert_eq!(harness.state.selection().unwrap().kind, SelectionKind::Columns);
    assert_eq!(harness.state.selection().unwrap().bounds(100, 6), (0..100, 0..1));
    let row = number + vec2(2.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(row), press(row, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(row, false, Modifiers::NONE)]);
    assert_eq!(harness.state.selection().unwrap().bounds(100, 6), (0..1, 0..6));
}

#[test]
fn clipboard_events_copy_paste_and_open_the_advanced_dialogs() {
    let mut harness = Harness::new();
    harness.features = GridFeatures { editable: true, clipboard: true, ..GridFeatures::default() };
    let source = Synthetic::new(20, 6);
    let (_, texts) = harness.frame(&source, Vec::new());
    let cell =
        texts.iter().find(|(_, text)| text == "r2c1").map(|(at, _)| *at).expect("r2c1 is shown") + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(cell), press(cell, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(cell, false, Modifiers::NONE)]);
    let (outcome, _) = harness.frame(&source, vec![Event::Copy]);
    assert_eq!(outcome.copy, Some((2..3, 1..2, false)), "⌘C arrives as a copy event");
    let (outcome, _) = harness.frame(&source, vec![Event::Paste("a\tb\nc\td".to_owned())]);
    assert_eq!(
        outcome.actions,
        [GridAction::Paste { row: 2, column: 1, text: Some("a\tb\nc\td".to_owned()), insert: false }],
        "⌘V pastes from the selected cell"
    );
    let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let (outcome, _) = harness.frame(&source, vec![key(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)]);
    assert_eq!(outcome.actions, [GridAction::CopyAdvanced { rows: 2..3, columns: 1..2 }]);
    assert_eq!(outcome.copy, None);
    let (outcome, _) =
        harness.frame(&source, vec![key(Key::V, Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT)]);
    assert_eq!(outcome.actions, [GridAction::PasteAdvanced { row: 2, column: 1 }]);

    harness.features = GridFeatures::default();
    let (outcome, _) = harness.frame(&source, vec![Event::Paste("x".to_owned())]);
    assert!(outcome.actions.is_empty(), "a read-only grid takes no paste");
    let (outcome, _) = harness.frame(&source, vec![key(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)]);
    assert_eq!(outcome.copy, Some((2..3, 1..2, true)), "without Advanced Copy ⇧⌘C copies with the header");
}

#[test]
fn f2_enter_and_typing_open_the_editor_and_a_refused_edit_says_why() {
    let mut harness = Harness::new();
    harness.features = GridFeatures { editable: true, ..GridFeatures::default() };
    let mut source = Synthetic::new(20, 4);
    source.columns[1].editable = true;
    let (_, texts) = harness.frame(&source, Vec::new());
    let cell =
        texts.iter().find(|(_, text)| text == "r2c1").map(|(at, _)| *at).expect("r2c1 is shown") + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(cell), press(cell, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(cell, false, Modifiers::NONE)]);
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    let editing =
        |state: &GridState| state.editor.as_ref().map(|editor| (editor.row, editor.column, editor.text.clone()));

    harness.frame(&source, vec![Event::Text("x".to_owned())]);
    assert_eq!(editing(&harness.state), Some((2, 1, "x".to_owned())), "a typed character replaces the value");
    harness.frame(&source, Vec::new());
    harness.frame(&source, vec![key(Key::Escape)]);
    assert!(harness.state.editor.is_none());

    harness.frame(&source, vec![key(Key::F2)]);
    assert_eq!(editing(&harness.state), Some((2, 1, String::new())), "F2 edits the value");
    harness.frame(&source, vec![key(Key::Escape)]);
    harness.frame(&source, vec![key(Key::Enter)]);
    assert!(harness.state.editor.is_some(), "Enter edits the value");
    harness.frame(&source, vec![key(Key::Escape)]);

    harness.frame(&source, vec![key(Key::ArrowRight)]);
    let (outcome, _) = harness.frame(&source, vec![key(Key::F2)]);
    assert!(harness.state.editor.is_none());
    assert_eq!(outcome.refused.as_deref(), Some("This column cannot be edited: it is generated or computed"));

    harness.features = GridFeatures::default();
    harness.frame(&source, vec![key(Key::ArrowLeft)]);
    let (outcome, _) = harness.frame(&source, vec![Event::Text("y".to_owned())]);
    assert!(harness.state.editor.is_none());
    assert_eq!(outcome.refused.as_deref(), Some("These rows cannot be edited"), "the source's reason when it has none");
}

#[test]
fn keys_scroll_the_selected_cell_into_view_and_a_scrolled_header_keeps_its_name() {
    let mut harness = Harness::new();
    let source = Synthetic::new(20, 30);
    let (_, texts) = harness.frame(&source, Vec::new());
    let cell =
        texts.iter().find(|(_, text)| text == "r2c1").map(|(at, _)| *at).expect("r2c1 is shown") + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(cell), press(cell, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(cell, false, Modifiers::NONE)]);
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    harness.frame(&source, vec![key(Key::End)]);
    let (_, texts) = harness.frame(&source, Vec::new());
    assert!(shows(&texts, "r2c29"), "the last column is in view: {texts:?}");
    let start: f32 = harness.state.widths[..29].iter().sum();
    assert!(start >= harness.state.left, "not scrolled past the cell's start");
    let right = start + harness.state.widths[29] - harness.state.left;
    assert!(
        right <= 1000.0 - 10.0 - 30.0,
        "the whole cell is left of the scroll bar and past the row numbers: {right}"
    );
    harness.frame(&source, vec![key(Key::Home)]);
    assert_eq!(harness.state.left, 0.0);

    harness.state.left = harness.state.widths[0] * 0.6;
    let (_, texts) = harness.frame(&source, Vec::new());
    let numbers = texts.iter().find(|(_, text)| text == "1").map(|(at, _)| at.x).expect("row number 1");
    assert!(
        texts.iter().any(|(at, text)| text.starts_with("col") && at.x > numbers),
        "the partly scrolled first column keeps its title: {texts:?}"
    );
}

#[test]
fn the_column_menu_sorts_and_keys_open_go_to_row_and_the_record() {
    let mut harness = Harness::new();
    harness.features = GridFeatures { sortable: true, result: true, ..GridFeatures::default() };
    let mut source = Synthetic::new(30, 4);
    source.columns[0].primary = true;
    let (_, texts) = harness.frame(&source, Vec::new());
    let badge = texts.iter().find(|(_, text)| text == "123").map(|(at, _)| *at).expect("the first badge");
    let left = badge.x - PADDING;
    let zone = pos2(left + harness.state.widths[0] - SORT_ZONE - MENU_ZONE / 2.0, badge.y + 4.0);
    harness.frame(&source, vec![Event::PointerMoved(zone), press(zone, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(zone, false, Modifiers::NONE)]);
    let (_, texts) = harness.frame(&source, Vec::new());
    assert!(shows(&texts, "Sort Descending") && shows(&texts, "Select Column"), "the column menu: {texts:?}");
    assert!(harness.state.selection().is_none(), "the ▾ does not select the column");
    let descending =
        texts.iter().find(|(_, text)| text == "Sort Descending").map(|(at, _)| *at).unwrap() + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(descending), press(descending, true, Modifiers::NONE)]);
    let (outcome, _) = harness.frame(&source, vec![press(descending, false, Modifiers::NONE)]);
    assert_eq!(outcome.actions, [GridAction::SortBy { column: 0, direction: "desc" }]);
    let (_, after) = harness.frame(&source, Vec::new());
    assert!(!shows(&after, "Sort Descending"), "the menu closes");

    let cell = texts.iter().find(|(_, text)| text == "r2c1").map(|(at, _)| *at).expect("r2c1") + vec2(4.0, 4.0);
    harness.frame(&source, vec![Event::PointerMoved(cell), press(cell, true, Modifiers::NONE)]);
    harness.frame(&source, vec![press(cell, false, Modifiers::NONE)]);
    let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    assert_eq!(harness.state.selection().map(|selection| selection.head), Some((2, 1)), "selected");
    let (outcome, _) = harness.frame(&source, vec![key(Key::G, Modifiers::CTRL)]);
    assert_eq!(outcome.actions, [GridAction::GoToRow]);
    let (outcome, _) = harness.frame(&source, vec![key(Key::Tab, Modifiers::NONE)]);
    assert_eq!(outcome.actions, [GridAction::View { mode: "record" }]);
    harness.frame(&source, vec![key(Key::ArrowDown, Modifiers::COMMAND)]);
    assert_eq!(harness.state.selection().map(|selection| selection.head.0), Some(29), "⌘↓ goes to the last row");
    harness.state.go_to(3, 30);
    assert_eq!(harness.state.current_row(), Some(3));
}

fn percentile(sorted: &[Duration], share: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * share).round() as usize]
}

#[test]
#[allow(clippy::print_stderr, reason = "a benchmark, read from the terminal for docs/performance.md")]
#[ignore = "a benchmark: cargo test -p zen-ui --release grid_scroll_benchmark -- --ignored --nocapture"]
fn grid_scroll_benchmark() {
    let mut harness = Harness::new();
    let source = Synthetic::new(1_000_000, 300);
    harness.frame(&source, Vec::new());
    let mut times = Vec::new();
    let pointer = Pos2::new(500.0, 300.0);
    for frame in 0..600 {
        let delta = if frame % 200 < 100 { -120.0 } else { 90.0 };
        let sideways = if frame % 50 < 25 { -40.0 } else { 40.0 };
        let events = vec![
            Event::PointerMoved(pointer),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(sideways, delta),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            },
        ];
        if frame % 100 == 99 {
            harness.state.scroll_to((frame as u64 * 1663) % 999_000);
        }
        let started = Instant::now();
        harness.frame(&source, events);
        times.push(started.elapsed());
    }
    times.sort();
    let (p50, p95, max) = (percentile(&times, 0.5), percentile(&times, 0.95), *times.last().unwrap());
    eprintln!("grid 1 000 000 × 300 scroll: p50 {p50:?} / p95 {p95:?} / max {max:?}");
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(p95 < Duration::from_millis(16), "{p95:?}");
    }
}
