use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::Value;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::language::structure::{StructureStatus, Symbol};

use super::glyphs::symbol_badge;
use super::recent_files::check_option;
use super::search_everywhere::search_field;
use super::structure_window::{Row, Structure, read, walk};
use super::{IdePalette, dialog_frame, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const PENDING_POLL: f64 = 0.2;

pub(super) struct FileStructure {
    open: bool,
    query: String,
    pub(super) narrow: bool,
    selected: Option<Vec<usize>>,
    path: PathBuf,
    params: Value,
    structure: Option<Structure>,
    read_at: f64,
    focus: bool,
    chosen_for: Option<String>,
    reveal: bool,
}

impl Default for FileStructure {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            narrow: true,
            selected: None,
            path: PathBuf::new(),
            params: Value::Null,
            structure: None,
            read_at: f64::NEG_INFINITY,
            focus: false,
            chosen_for: None,
            reveal: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pick {
    pub path: PathBuf,
    pub offset: usize,
}

impl FileStructure {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.query
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open(&mut self, path: &Path, params: Value) {
        if self.open {
            return;
        }
        *self = Self { open: true, path: path.to_owned(), params, focus: true, narrow: self.narrow, ..Self::default() };
    }

    pub(super) fn close(&mut self) {
        let narrow = self.narrow;
        *self = Self { narrow, ..Self::default() };
    }

    fn symbol(&self, index: &[usize]) -> Option<&Symbol> {
        let (first, rest) = index.split_first()?;
        let mut symbol = self.structure.as_ref()?.symbols.get(*first)?;
        for at in rest {
            symbol = symbol.children.get(*at)?;
        }
        Some(symbol)
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(structure) = &self.structure {
            walk(&structure.symbols, &mut Vec::new(), &mut Vec::new(), None, &mut rows);
        }
        let query = self.query.trim();
        if query.is_empty() || !self.narrow {
            return rows;
        }
        let matched: Vec<Vec<usize>> = rows
            .iter()
            .filter(|row| {
                self.symbol(&row.index).is_some_and(|symbol| zen_index::match_positions(query, &symbol.name).is_some())
            })
            .map(|row| row.index.clone())
            .collect();
        rows.retain(|row| matched.iter().any(|found| found.starts_with(&row.index)));
        rows
    }

    fn best(&self, rows: &[Row]) -> Option<Vec<usize>> {
        let query = self.query.trim();
        rows.iter()
            .filter_map(|row| {
                let symbol = self.symbol(&row.index)?;
                zen_index::match_positions(query, &symbol.name)?;
                Some((zen_index::score(query, &symbol.name, &symbol.name)?, row.index.clone()))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map(|(_, index)| index)
    }

    fn update(&mut self, structure: Structure, now: f64) {
        let first = self.structure.is_none();
        let current = structure.current.clone();
        self.structure = Some(structure);
        self.read_at = now;
        if first && !current.is_empty() {
            self.selected = Some(current);
            self.reveal = true;
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> Option<Pick> {
        if !self.open {
            return None;
        }
        let now = ui.input(|input| input.time);
        let pending = self.structure.as_ref().is_none_or(|structure| structure.status == StructureStatus::Pending);
        if self.structure.is_none() || (pending && now - self.read_at >= PENDING_POLL) {
            let structure = read(registry, session, &self.path, self.params.clone());
            self.update(structure, now);
        }
        if self.structure.as_ref().is_some_and(|structure| structure.status == StructureStatus::Pending) {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
        }

        let mut rows = self.rows();
        if self.chosen_for.as_deref() != Some(self.query.as_str()) {
            if !self.query.trim().is_empty() {
                self.selected = self.best(&rows).or_else(|| rows.first().map(|row| row.index.clone()));
                self.reveal = true;
            }
            self.chosen_for = Some(self.query.clone());
        }
        let mut pick: Option<Vec<usize>> = None;
        let mut close = false;
        let screen = ui.ctx().content_rect();
        let width = (screen.width() * 0.4).clamp(420.0, 600.0);
        let list_height = (screen.height() * 0.55).clamp(200.0, 560.0);
        let area = egui::Area::new(ui.scope_id().with("file-structure"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.14));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                let name = self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let title = ui.painter().text(
                    pos2(bar.left(), bar.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    theme::semibold(13.0),
                    palette.text,
                );
                accessibility::heading(ui, ui.scope_id().with("file-structure-title"), title, || name.clone());
                let narrow =
                    check_option(ui, bar.right_center(), self.narrow, &t!("ide-file-structure-narrow"), "", palette);
                if narrow.clicked() {
                    self.narrow = !self.narrow;
                    self.focus = true;
                    rows = self.rows();
                }
                ui.add_space(8.0);
                let hint = t!("ide-file-structure-hint");
                let field = search_field(ui, &mut self.query, &hint, palette);
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                super::filter_popups::take_typing(ui);
                let (up, down, enter, escape) = ui.input_mut(|input| {
                    (
                        input.consume_key(Modifiers::NONE, Key::ArrowUp),
                        input.consume_key(Modifiers::NONE, Key::ArrowDown),
                        input.consume_key(Modifiers::NONE, Key::Enter),
                        input.consume_key(Modifiers::NONE, Key::Escape),
                    )
                });
                let at = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.index == selected));
                if up || down {
                    let next = match at {
                        None => 0,
                        Some(at) if up => at.saturating_sub(1),
                        Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
                    };
                    self.selected = rows.get(next).map(|row| row.index.clone());
                    self.reveal = true;
                }
                if enter {
                    pick = self.selected.clone().filter(|selected| rows.iter().any(|row| &row.index == selected));
                }
                if escape {
                    close = true;
                }
                if !field.has_focus() && !close && pick.is_none() {
                    field.request_focus();
                }
                ui.add_space(6.0);
                if rows.is_empty() {
                    let message = match &self.structure {
                        Some(structure) if structure.status == StructureStatus::Pending => t!("ide-structure-loading"),
                        Some(structure) if structure.symbols.is_empty() => {
                            structure.message.clone().unwrap_or_else(|| t!("ide-structure-empty"))
                        }
                        _ => t!("ide-goto-file-nothing"),
                    };
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        message,
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    return;
                }
                let query = self.query.trim().to_owned();
                let reveal = std::mem::take(&mut self.reveal);
                super::filter_popups::list(ui, "file-structure-rows", list_height, |ui| {
                    for row in &rows {
                        let Some(symbol) = self.symbol(&row.index) else { continue };
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::CLICK);
                        let selected = self.selected.as_ref() == Some(&row.index);
                        if selected {
                            ui.painter().rect_filled(rect, 5, palette.selection);
                            if reveal {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 5, palette.popup_hover);
                        }
                        accessibility::tree_item(
                            &response,
                            row.depth + 1,
                            row.parent.then_some(true),
                            selected,
                            &symbol.name,
                        );
                        accessibility::describe(&response, symbol.detail.as_deref().unwrap_or_default());
                        let x = rect.left() + 8.0 + row.depth as f32 * metrics::TREE_INDENT;
                        symbol_badge(
                            ui.painter(),
                            Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(14.0)),
                            symbol.kind,
                            palette,
                        );
                        let painter = ui.painter().with_clip_rect(rect);
                        let job = name_job(&symbol.name, &query, palette);
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        let at = pos2(x + 22.0, rect.center().y - galley.size().y / 2.0);
                        let name_right = at.x + galley.size().x;
                        painter.galley(at, galley, palette.text);
                        if let Some(detail) = symbol.detail.as_deref().filter(|detail| !detail.is_empty()) {
                            painter.text(
                                pos2(name_right + 6.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                detail,
                                theme::regular(13.0),
                                palette.text_secondary,
                            );
                        }
                        if response.clicked() {
                            pick = Some(row.index.clone());
                        }
                    }
                });
            });
        });
        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        let picked = pick
            .and_then(|index| self.symbol(&index).map(|symbol| symbol.selection_start))
            .map(|offset| Pick { path: self.path.clone(), offset });
        if close || picked.is_some() {
            self.close();
        }
        picked
    }
}

pub(super) fn name_job(name: &str, query: &str, palette: &IdePalette) -> LayoutJob {
    let plain = TextFormat::simple(theme::regular(13.0), palette.text);
    let marked = TextFormat { background: palette.search_highlight, ..plain.clone() };
    let positions =
        if query.is_empty() { Vec::new() } else { zen_index::match_positions(query, name).unwrap_or_default() };
    let mut job = LayoutJob::default();
    for (index, character) in name.chars().enumerate() {
        let format = if positions.contains(&index) { marked.clone() } else { plain.clone() };
        job.append(character.encode_utf8(&mut [0; 4]), 0.0, format);
    }
    job
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2};
    use serde_json::json;
    use zen_syntax::SymbolKind;

    use super::*;

    fn symbol(name: &str, kind: SymbolKind, selection_start: usize, children: Vec<Symbol>) -> Symbol {
        Symbol {
            name: name.to_owned(),
            detail: None,
            kind,
            start: selection_start,
            end: selection_start + 10,
            selection_start,
            line: 1,
            column: 1,
            children,
        }
    }

    fn structure(path: &Path) -> Structure {
        Structure {
            path: path.to_owned(),
            status: StructureStatus::Ready,
            message: None,
            symbols: vec![
                symbol(
                    "Server",
                    SymbolKind::Struct,
                    20,
                    vec![
                        symbol("ListenAndServe", SymbolKind::Method, 40, Vec::new()),
                        symbol("Close", SymbolKind::Method, 60, Vec::new()),
                    ],
                ),
                symbol("main", SymbolKind::Function, 90, Vec::new()),
            ],
            current: vec![1],
        }
    }

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        popup: FileStructure,
        picked: Vec<Pick>,
        left: Vec<Key>,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let path = PathBuf::from("/p/main.go");
            let mut popup = FileStructure::default();
            popup.open(&path, json!({ "path": path }));
            popup.update(structure(&path), 0.0);
            Self { ctx, registry, session: Session::new(), popup, picked: Vec::new(), left: Vec::new() }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.picked.extend(self.popup.show(ui, &self.registry, &mut self.session, &IdePalette::zen_dark()));
                let keys = [Key::Enter, Key::Escape, Key::ArrowUp, Key::ArrowDown];
                self.left.extend(keys.into_iter().filter(|key| ui.input(|input| input.key_pressed(*key))));
            });
            output.textures_delta.clear();
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn names(&self) -> Vec<String> {
            self.popup.rows().iter().filter_map(|row| self.popup.symbol(&row.index)).map(|s| s.name.clone()).collect()
        }
    }

    #[test]
    fn opens_on_the_symbol_at_the_caret_and_enter_goes_there_and_closes() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["Server", "ListenAndServe", "Close", "main"]);
        assert_eq!(harness.popup.selected.as_deref(), Some(&[1][..]), "the symbol at the caret");
        harness.key(Key::ArrowUp);
        harness.key(Key::Enter);
        assert_eq!(harness.picked, [Pick { path: PathBuf::from("/p/main.go"), offset: 60 }]);
        assert!(!harness.popup.is_open());
    }

    #[test]
    fn the_keys_it_handles_never_reach_the_editor() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("main".to_owned())]);
        harness.key(Key::ArrowUp);
        harness.key(Key::ArrowDown);
        harness.key(Key::Enter);
        assert_eq!(harness.picked[0].offset, 90);
        let path = PathBuf::from("/p/main.go");
        harness.popup.open(&path, json!({ "path": path }));
        harness.popup.update(structure(&path), 0.0);
        harness.key(Key::Escape);
        assert!(!harness.popup.is_open());
        assert_eq!(harness.left, [], "the editor drawn after the popup would break the line at the symbol");
    }

    #[test]
    fn typing_narrows_down_to_the_matches_and_what_holds_them() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("las".to_owned())]);
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["Server", "ListenAndServe"], "camel humps");
        assert_eq!(harness.popup.selected.as_deref(), Some(&[0, 0][..]), "the match is selected, not its holder");
        harness.key(Key::Enter);
        assert_eq!(harness.picked[0].offset, 40);
    }

    #[test]
    fn without_narrowing_every_symbol_stays_and_the_best_match_is_selected() {
        let mut harness = Harness::new();
        harness.popup.narrow = false;
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("clo".to_owned())]);
        harness.frame(Vec::new());
        assert_eq!(harness.names().len(), 4);
        assert_eq!(harness.popup.selected.as_deref(), Some(&[0, 1][..]));
        harness.key(Key::Escape);
        assert!(!harness.popup.is_open() && harness.picked.is_empty());
        let mut again = FileStructure { narrow: false, ..FileStructure::default() };
        again.open(Path::new("/p/a.go"), Value::Null);
        assert!(!again.narrow, "the choice is kept");
    }

    #[test]
    fn the_matched_letters_are_marked() {
        let palette = IdePalette::zen_dark();
        let job = name_job("ListenAndServe", "las", &palette);
        let marked: String = job
            .sections
            .iter()
            .filter(|section| section.format.background == palette.search_highlight)
            .map(|section| &job.text[section.byte_range.start.0..section.byte_range.end.0])
            .collect();
        assert_eq!(marked, "LAS", "the letters at the word starts");
    }

    #[test]
    fn the_structure_comes_from_the_command_and_says_why_when_there_is_none() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let path = PathBuf::from("/p/none.go");
        let mut popup = FileStructure::default();
        popup.open(&path, json!({ "path": path }));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(popup.show(ui, &registry, &mut session, &IdePalette::zen_dark()).is_none());
        });
        output.textures_delta.clear();
        let structure = popup.structure.as_ref().unwrap();
        assert_eq!(structure.status, StructureStatus::Unavailable);
        assert!(popup.rows().is_empty() && popup.is_open());
    }
}
