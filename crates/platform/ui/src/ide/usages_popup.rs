use std::path::{Path, PathBuf};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::language::show_usages::{
    DeclarationKind, ShowUsagesOptions, ShowUsagesState, UsageAccess, UsageRow, UsagesScope,
};

use super::caret_popup::{CaretPlace, PopupAnchor};
use super::glyphs::{glyph_more, glyph_settings};
use super::{IdePalette, Side, hint, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 22.0;
const VISIBLE: usize = 15;
const WIDTH: f32 = 680.0;
const PREVIEW_LINES: usize = 7;
const SCOPES: [UsagesScope; 4] =
    [UsagesScope::CurrentFile, UsagesScope::OpenFiles, UsagesScope::ProjectFiles, UsagesScope::AllPlaces];

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PopupAction {
    Options(Value),
    More,
    Close,
    OpenInFindWindow,
    Settings,
    GoTo { path: PathBuf, offset: usize },
}

impl PopupAction {
    pub(super) fn command(&self) -> (&'static str, Value) {
        match self {
            Self::Options(params) => ("navigate.showUsages.setOptions", params.clone()),
            Self::More => ("navigate.showUsages.more", json!({})),
            Self::Close => ("navigate.showUsages.close", json!({})),
            Self::OpenInFindWindow => ("navigate.showUsages.openInFindWindow", json!({})),
            Self::Settings => ("navigate.usages.settings", json!({})),
            Self::GoTo { path, offset } => ("navigate.place.goTo", json!({ "path": path, "offset": offset })),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toggle {
    Merge,
    Read,
    Write,
    Generated,
    Imports,
    Comments,
    Preview,
}

impl Toggle {
    const TOOLBAR: [Self; 6] = [Self::Merge, Self::Read, Self::Write, Self::Generated, Self::Imports, Self::Comments];

    fn key(self) -> &'static str {
        match self {
            Self::Merge => "mergeSameLine",
            Self::Read => "read",
            Self::Write => "write",
            Self::Generated => "generated",
            Self::Imports => "imports",
            Self::Comments => "comments",
            Self::Preview => "preview",
        }
    }

    fn name(self) -> String {
        match self {
            Self::Merge => t!("ide-show-usages-merge"),
            Self::Read => t!("ide-show-usages-read"),
            Self::Write => t!("ide-show-usages-write"),
            Self::Generated => t!("ide-show-usages-generated"),
            Self::Imports => t!("ide-show-usages-imports"),
            Self::Comments => t!("ide-show-usages-comments"),
            Self::Preview => t!("ide-show-usages-preview"),
        }
    }

    fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Merge => Some("⌃L"),
            Self::Read => Some("⌃R"),
            Self::Write => Some("⌃W"),
            Self::Imports => Some("⌃I"),
            _ => None,
        }
    }

    fn on(self, options: &ShowUsagesOptions) -> bool {
        match self {
            Self::Merge => options.merge_same_line,
            Self::Read => options.read,
            Self::Write => options.write,
            Self::Generated => options.generated,
            Self::Imports => options.imports,
            Self::Comments => options.comments,
            Self::Preview => options.preview,
        }
    }

    fn flipped(self, options: &ShowUsagesOptions) -> PopupAction {
        PopupAction::Options(json!({ self.key(): !self.on(options) }))
    }

    fn glyph(self, painter: &egui::Painter, rect: Rect, color: Color32) {
        match self {
            Self::Merge => glyph_merge(painter, rect, color),
            Self::Read => glyph_read(painter, rect, color),
            Self::Write => glyph_write(painter, rect, color),
            Self::Generated => glyph_generated(painter, rect, color),
            Self::Imports => glyph_import(painter, rect, color),
            Self::Comments => glyph_comment(painter, rect, color),
            Self::Preview => glyph_preview(painter, rect, color),
        }
    }
}

pub(super) fn scope_name(scope: UsagesScope) -> String {
    match scope {
        UsagesScope::CurrentFile => t!("ide-show-usages-scope-current"),
        UsagesScope::OpenFiles => t!("ide-show-usages-scope-open"),
        UsagesScope::ProjectFiles => t!("ide-show-usages-scope-project"),
        UsagesScope::AllPlaces => t!("ide-show-usages-scope-all"),
    }
}

pub(super) fn kind_name(kind: DeclarationKind) -> String {
    match kind {
        DeclarationKind::Variable => t!("ide-usages-kind-variable"),
        DeclarationKind::Parameter => t!("ide-usages-kind-parameter"),
        DeclarationKind::Constant => t!("ide-usages-kind-constant"),
        DeclarationKind::Function => t!("ide-usages-kind-function"),
        DeclarationKind::Method => t!("ide-usages-kind-method"),
        DeclarationKind::Type => t!("ide-usages-kind-type"),
        DeclarationKind::Struct => t!("ide-usages-kind-struct"),
        DeclarationKind::Interface => t!("ide-usages-kind-interface"),
        DeclarationKind::Class => t!("ide-usages-kind-class"),
        DeclarationKind::Field => t!("ide-usages-kind-field"),
        DeclarationKind::Property => t!("ide-usages-kind-property"),
        DeclarationKind::Package => t!("ide-usages-kind-package"),
        DeclarationKind::Module => t!("ide-usages-kind-module"),
        DeclarationKind::Symbol => t!("ide-usages-kind-symbol"),
    }
}

pub(super) fn title(state: &ShowUsagesState) -> String {
    let name = state.symbol.clone().unwrap_or_default();
    let path = state.declaration_relative.clone().unwrap_or_default();
    let head = t!("ide-show-usages-title", kind = kind_name(state.kind), name = name, path = path);
    if state.pending {
        return head;
    }
    format!("{head} · {}", t!("ide-show-usages-count", count = state.total))
}

struct Source {
    path: PathBuf,
    line: u32,
    first: usize,
    lines: Vec<String>,
}

#[derive(Default)]
pub(super) struct UsagesPopup {
    state: Option<ShowUsagesState>,
    generation: Option<u64>,
    anchor: Option<Pos2>,
    at: Option<PopupAnchor>,
    selected: usize,
    reveal: bool,
    source: Option<Source>,
}

impl UsagesPopup {
    pub(super) fn open_at(&mut self, at: Pos2) {
        self.anchor = Some(at);
    }

    pub(super) fn is_open(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.open)
    }

    pub(super) fn generation(&self) -> Option<u64> {
        self.generation
    }

    pub(super) fn hide(&mut self) {
        self.state = None;
        self.generation = None;
        self.at = None;
        self.anchor = None;
        self.source = None;
    }

    pub(super) fn takes_key(&self, key: Key, modifiers: Modifiers) -> bool {
        self.is_open() && modifiers.matches_exact(Modifiers::CTRL) && [Key::L, Key::R, Key::W, Key::I].contains(&key)
    }

    pub(super) fn follow_source(&mut self, session: &zen_core::Session) {
        let wanted = self
            .state
            .as_ref()
            .filter(|state| state.options.preview)
            .and_then(|_| self.selected_row())
            .map(|row| (row.path.clone(), row.line));
        if self.source.as_ref().map(|source| (&source.path, source.line))
            == wanted.as_ref().map(|(path, line)| (path, *line))
        {
            return;
        }
        self.source = wanted.map(|(path, line)| {
            let text = preview_text(session, &path).unwrap_or_default();
            let first = (line.saturating_sub(1) as usize).saturating_sub(PREVIEW_LINES / 2);
            let lines = text.lines().skip(first).take(PREVIEW_LINES).map(|line| line.replace('\t', "    ")).collect();
            Source { path, line, first, lines }
        });
    }

    pub(super) fn update(&mut self, state: ShowUsagesState) {
        let fresh = self.state.as_ref().is_none_or(|shown| shown.search != state.search || !shown.open);
        if fresh {
            self.at = Some(self.anchor.take().map_or(PopupAnchor::UNDER_CARET, PopupAnchor::At));
            self.selected = 0;
            self.reveal = true;
        }
        self.generation = Some(state.generation);
        self.selected = self.selected.min(Self::count(&state).saturating_sub(1));
        self.state = Some(state);
    }

    fn count(state: &ShowUsagesState) -> usize {
        state.rows.len() + usize::from(state.more > 0)
    }

    pub(super) fn selected_row(&self) -> Option<&UsageRow> {
        self.state.as_ref()?.rows.get(self.selected)
    }

    fn activate(&self) -> Vec<PopupAction> {
        let Some(state) = &self.state else { return Vec::new() };
        match state.rows.get(self.selected) {
            Some(row) => vec![PopupAction::GoTo { path: row.path.clone(), offset: row.start }, PopupAction::Close],
            None if state.more > 0 => vec![PopupAction::More],
            None => Vec::new(),
        }
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Vec<PopupAction> {
        let Some(state) = &self.state else { return Vec::new() };
        if !state.open {
            return Vec::new();
        }
        let count = Self::count(state);
        let options = state.options;
        let pressed =
            |ui: &mut Ui, modifiers: Modifiers, key: Key| ui.input_mut(|input| input.consume_key(modifiers, key));
        let up = pressed(ui, Modifiers::NONE, Key::ArrowUp);
        let down = pressed(ui, Modifiers::NONE, Key::ArrowDown);
        let enter = pressed(ui, Modifiers::NONE, Key::Enter);
        let escape = pressed(ui, Modifiers::NONE, Key::Escape);
        let find_window = pressed(ui, Modifiers::ALT, Key::F7);
        let toggles: Vec<Toggle> =
            [(Key::L, Toggle::Merge), (Key::R, Toggle::Read), (Key::W, Toggle::Write), (Key::I, Toggle::Imports)]
                .into_iter()
                .filter(|(key, _)| pressed(ui, Modifiers::CTRL, *key))
                .map(|(_, toggle)| toggle)
                .collect();
        let mut actions: Vec<PopupAction> = toggles.into_iter().map(|toggle| toggle.flipped(&options)).collect();
        if count > 0 {
            if up {
                self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if down {
                self.selected = (self.selected + 1) % count;
            }
        }
        self.reveal |= up || down;
        if enter {
            actions.extend(self.activate());
        }
        if find_window {
            actions.push(PopupAction::OpenInFindWindow);
        } else if escape {
            actions.push(PopupAction::Close);
        }
        super::filter_popups::take_typing(ui);
        actions
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: CaretPlace) -> Vec<PopupAction> {
        let mut actions = Vec::new();
        let Some(state) = self.state.clone() else { return actions };
        if !state.open {
            return actions;
        }
        let screen = ui.ctx().content_rect();
        let width = WIDTH.min(screen.width() - 40.0).max(320.0);
        let anchor = self.at.unwrap_or(PopupAnchor::At(screen.center() - vec2(width / 2.0, 120.0)));
        let area = egui::Area::new(ui.scope_id().with("show-usages-popup")).order(egui::Order::Foreground);
        let Some(area) = anchor.area(ui.ctx(), area, caret) else {
            return actions;
        };
        let mut moved = Vec2::ZERO;
        let mut menus: Vec<Rect> = Vec::new();
        let area = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                moved = self.header(ui, palette, &state, &mut actions);
                self.toolbar(ui, palette, &state, &mut actions, &mut menus);
                if state.settings {
                    settings_panel(ui, palette, &state, &mut actions, &mut menus);
                }
                self.list(ui, palette, &state, &mut actions);
                if state.options.preview {
                    self.preview(ui, palette);
                }
                self.footer(ui, palette);
            });
        });
        accessibility::name_with(&area.response, egui::Role::Dialog, || title(&state));
        if moved != Vec2::ZERO {
            self.at = Some(PopupAnchor::At(area.response.rect.left_top() + moved));
        }
        self.reveal = false;
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| {
                    !area.response.rect.contains(pos) && !menus.iter().any(|menu| menu.contains(pos))
                })
        });
        if pressed_outside && !egui::Popup::is_any_open(ui.ctx()) {
            actions.push(PopupAction::Close);
        }
        actions
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, state: &ShowUsagesState, actions: &mut Vec<PopupAction>) -> Vec2 {
        let (rect, drag) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::drag());
        let painter = ui.painter();
        let link_label = t!("ide-show-usages-open-find");
        let link = painter.layout_no_wrap(link_label.clone(), theme::regular(12.5), p.link);
        let shortcut = painter.layout_no_wrap("⌥F7".to_owned(), theme::regular(12.0), p.text_secondary);
        let shortcut_left = rect.right() - 8.0 - shortcut.size().x;
        let link_rect = Rect::from_min_size(
            pos2(shortcut_left - 6.0 - link.size().x, rect.center().y - link.size().y / 2.0),
            link.size(),
        );
        let mut job = LayoutJob::default();
        let plain = TextFormat::simple(theme::regular(13.0), p.text);
        let strong = TextFormat::simple(theme::semibold(13.0), p.text);
        let grey = TextFormat::simple(theme::regular(13.0), p.text_secondary);
        let kind = kind_name(state.kind);
        let name = state.symbol.clone().unwrap_or_default();
        let path = state.declaration_relative.clone().unwrap_or_default();
        for (piece, format) in header_pieces(&kind, &name, &path) {
            let format = match format {
                Piece::Strong => strong.clone(),
                Piece::Plain => plain.clone(),
            };
            job.append(&piece, 0.0, format);
        }
        let count = if state.pending {
            t!("ide-show-usages-searching")
        } else {
            t!("ide-show-usages-count", count = state.total)
        };
        job.append(&format!(" · {count}"), 0.0, grey);
        let galley = painter.layout_job(job);
        let text_at = pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0);
        let clip = Rect::from_min_max(rect.min, pos2(link_rect.left() - 12.0, rect.max.y));
        painter.with_clip_rect(clip).galley(text_at, galley, p.text);
        let link_response = ui.interact(link_rect.expand(2.0), ui.scope_id().with("show-usages-find"), Sense::click());
        let painter = ui.painter();
        painter.galley(link_rect.min, link, p.link);
        painter.galley(pos2(shortcut_left, rect.center().y - shortcut.size().y / 2.0), shortcut, p.text_secondary);
        if link_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        accessibility::button(&link_response, &link_label);
        if link_response.clicked() {
            actions.push(PopupAction::OpenInFindWindow);
        }
        if drag.dragged() { drag.drag_delta() } else { Vec2::ZERO }
    }

    fn toolbar(
        &self,
        ui: &mut Ui,
        p: &IdePalette,
        state: &ShowUsagesState,
        actions: &mut Vec<PopupAction>,
        menus: &mut Vec<Rect>,
    ) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
        let button = |index: usize| {
            Rect::from_center_size(pos2(rect.left() + 16.0 + 26.0 * index as f32, rect.center().y), Vec2::splat(24.0))
        };
        let toggles = Toggle::TOOLBAR.into_iter().chain(std::iter::once(Toggle::Preview));
        let mut index = 0;
        for toggle in toggles {
            if toggle == Toggle::Preview {
                let options = ui.interact(button(index), ui.scope_id().with("show-usages-options"), Sense::click());
                hover(ui, &options, p);
                glyph_more(ui.painter(), options.rect.shrink(6.0), p.text_secondary);
                hint(ui, &options, &t!("ide-show-usages-options"), None, Side::Below);
                if let Some(menu) = egui::Popup::menu(&options).show(|ui| {
                    ui.set_min_width(260.0);
                    for toggle in Toggle::TOOLBAR.into_iter().chain(std::iter::once(Toggle::Preview)) {
                        let mut on = toggle.on(&state.options);
                        let label = match toggle.shortcut() {
                            Some(shortcut) => format!("{}  {shortcut}", toggle.name()),
                            None => toggle.name(),
                        };
                        if ui.checkbox(&mut on, label).changed() {
                            actions.push(toggle.flipped(&state.options));
                        }
                    }
                }) {
                    menus.push(menu.response.rect);
                }
                index += 1;
            }
            let on = toggle.on(&state.options);
            let response =
                ui.interact(button(index), ui.scope_id().with(("show-usages-toggle", index)), Sense::click());
            if on {
                ui.painter().rect_filled(response.rect, 6, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            toggle.glyph(ui.painter(), response.rect.shrink(5.0), if on { p.text } else { p.text_secondary });
            hint(ui, &response, &toggle.name(), toggle.shortcut(), Side::Below);
            accessibility::pressed(&response, on);
            if response.clicked() {
                actions.push(toggle.flipped(&state.options));
            }
            index += 1;
        }
        let left = button(index).left() + 6.0;
        let scope = scope_combo(ui, p, pos2(left, rect.center().y), state.scope, actions, menus, "show-usages-scope");
        ui.painter().text(
            pos2(scope.right() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            t!("ide-show-usages-usages"),
            theme::regular(12.5),
            p.text_secondary,
        );
        let settings = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(24.0));
        let settings = ui.interact(settings, ui.scope_id().with("show-usages-settings"), Sense::click());
        if state.settings {
            ui.painter().rect_filled(settings.rect, 6, p.popup_hover);
        } else {
            hover(ui, &settings, p);
        }
        glyph_settings(ui.painter(), settings.rect.shrink(5.0), p.text_secondary);
        hint(ui, &settings, &t!("ide-show-usages-settings"), Some("⌥⇧⌘F7"), Side::Below);
        accessibility::pressed(&settings, state.settings);
        if settings.clicked() {
            actions.push(PopupAction::Settings);
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.tab_border));
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette, state: &ShowUsagesState, actions: &mut Vec<PopupAction>) {
        if state.rows.is_empty() {
            let text = match (&state.error, state.pending) {
                (_, true) => t!("ide-show-usages-searching"),
                (Some(error), false) => error.clone(),
                (None, false) => t!("ide-show-usages-none"),
            };
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW * 2.0), Sense::hover());
            ui.painter().text(
                pos2(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                text,
                theme::regular(13.0),
                p.text_secondary,
            );
            if state.more == 0 {
                return;
            }
        }
        let columns = Columns::measure(ui, &state.rows, p.text);
        let count = Self::count(state);
        super::filter_popups::list(ui, "show-usages-rows", ROW * VISIBLE.min(count.max(1)) as f32, |ui| {
            for index in 0..count {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                if index == self.selected {
                    ui.painter().rect_filled(rect, 4, p.selection);
                    if self.reveal {
                        response.scroll_to_me(None);
                    }
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.popup_hover);
                }
                match state.rows.get(index) {
                    Some(row) => {
                        accessibility::name_with(&response, egui::Role::ListBoxOption, || {
                            format!("{} {}: {}", row.relative, row.line, row.preview)
                        });
                        draw_row(ui, rect, row, &columns, p);
                    }
                    None => {
                        let more = t!("ide-show-usages-more", count = state.more);
                        accessibility::name_with(&response, egui::Role::ListBoxOption, || more.clone());
                        ui.painter().text(
                            pos2(rect.left() + 28.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            more,
                            theme::regular(13.0),
                            p.link,
                        );
                    }
                }
                accessibility::edit(&response, |node| node.set_selected(index == self.selected));
                if response.clicked() {
                    self.selected = index;
                    actions.extend(self.activate());
                }
            }
        });
    }

    fn preview(&self, ui: &mut Ui, p: &IdePalette) {
        let height = PREVIEW_LINES as f32 * 18.0 + 10.0;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.tab_border));
        let Some(source) = &self.source else { return };
        let family = metrics::editor_family(ui.ctx());
        let line = source.line.saturating_sub(1) as usize;
        let painter = ui.painter().with_clip_rect(rect.shrink(2.0));
        for (shown, text) in source.lines.iter().enumerate() {
            let index = source.first + shown;
            let y = rect.top() + 5.0 + shown as f32 * 18.0 + 9.0;
            if index == line {
                let band = Rect::from_min_max(pos2(rect.left() + 2.0, y - 9.0), pos2(rect.right() - 2.0, y + 9.0));
                painter.rect_filled(band, 0, p.current_line);
            }
            painter.text(
                pos2(rect.left() + 40.0, y),
                Align2::RIGHT_CENTER,
                (index + 1).to_string(),
                FontId::new(12.0, family.clone()),
                p.gutter_text,
            );
            painter.text(
                pos2(rect.left() + 52.0, y),
                Align2::LEFT_CENTER,
                text,
                FontId::new(12.5, family.clone()),
                p.code,
            );
        }
    }

    fn footer(&self, ui: &mut Ui, p: &IdePalette) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.tab_border));
        let Some(row) = self.selected_row() else { return };
        let painter = ui.painter().with_clip_rect(rect);
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            &row.relative,
            theme::regular(12.0),
            p.text_secondary,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    Plain,
    Strong,
}

fn header_pieces(kind: &str, name: &str, path: &str) -> Vec<(String, Piece)> {
    const MARK: &str = "\u{1}";
    let marked = t!("ide-show-usages-title", kind = kind, name = format!("{MARK}{name}{MARK}"), path = path);
    marked
        .split(MARK)
        .enumerate()
        .filter(|(_, piece)| !piece.is_empty())
        .map(|(index, piece)| (piece.to_owned(), if index % 2 == 1 { Piece::Strong } else { Piece::Plain }))
        .collect()
}

fn hover(ui: &Ui, response: &egui::Response, p: &IdePalette) {
    if response.hovered() {
        ui.painter().rect_filled(response.rect, 6, p.hover);
    }
}

fn scope_combo(
    ui: &mut Ui,
    p: &IdePalette,
    left_center: Pos2,
    scope: UsagesScope,
    actions: &mut Vec<PopupAction>,
    menus: &mut Vec<Rect>,
    salt: &str,
) -> Rect {
    let label = format!("{} ⌄", scope_name(scope));
    let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(12.5), p.text);
    let rect = Rect::from_min_size(pos2(left_center.x, left_center.y - 11.0), vec2(galley.size().x + 12.0, 22.0));
    let response = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
    hover(ui, &response, p);
    ui.painter().galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    accessibility::name_with(&response, egui::Role::ComboBox, || label.clone());
    if let Some(menu) = egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(180.0);
        for option in SCOPES {
            if ui.selectable_label(option == scope, scope_name(option)).clicked() {
                actions.push(PopupAction::Options(json!({ "scope": option })));
            }
        }
    }) {
        menus.push(menu.response.rect);
    }
    rect
}

fn settings_panel(
    ui: &mut Ui,
    p: &IdePalette,
    state: &ShowUsagesState,
    actions: &mut Vec<PopupAction>,
    menus: &mut Vec<Rect>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("ide-show-usages-settings-title")).font(theme::semibold(12.5)).color(p.text));
    });
    ui.add_space(4.0);
    for (toggle, label) in [
        (Toggle::Comments, "ide-show-usages-settings-comments"),
        (Toggle::Imports, "ide-show-usages-settings-imports"),
        (Toggle::Generated, "ide-show-usages-settings-generated"),
        (Toggle::Merge, "ide-show-usages-settings-merge"),
    ] {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let mut on = toggle.on(&state.options);
            if ui.checkbox(&mut on, t!(label)).changed() {
                actions.push(toggle.flipped(&state.options));
            }
        });
    }
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let label = ui.painter().text(
        pos2(row.left() + 12.0, row.center().y),
        Align2::LEFT_CENTER,
        t!("ide-show-usages-settings-scope"),
        theme::regular(12.5),
        p.text,
    );
    scope_combo(
        ui,
        p,
        pos2(label.right() + 8.0, row.center().y),
        state.scope,
        actions,
        menus,
        "show-usages-settings-scope",
    );
    ui.add_space(4.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(line.x_range(), line.top(), Stroke::new(1.0, p.tab_border));
}

struct Columns {
    name: f32,
    module: f32,
    line: f32,
}

impl Columns {
    fn measure(ui: &Ui, rows: &[UsageRow], color: Color32) -> Self {
        let width = |text: &str, font: FontId| ui.painter().layout_no_wrap(text.to_owned(), font, color).size().x;
        let mut columns = Self { name: 0.0, module: 0.0, line: 0.0 };
        for row in rows.iter().take(VISIBLE * 4) {
            columns.name = columns.name.max(width(&row.file_name, theme::regular(13.0)));
            columns.module =
                columns.module.max(row.module.as_deref().map_or(0.0, |module| width(module, theme::regular(12.5))));
            columns.line = columns.line.max(width(&row.line.to_string(), theme::regular(12.5)));
        }
        columns.name = columns.name.min(180.0);
        columns.module = columns.module.min(140.0);
        columns
    }
}

fn draw_row(ui: &Ui, rect: Rect, row: &UsageRow, columns: &Columns, p: &IdePalette) {
    let painter = ui.painter().with_clip_rect(rect);
    let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(16.0));
    crate::file_icons::file_icon_at(&painter, icon, &row.path);
    let mut x = rect.left() + 28.0;
    let y = rect.center().y;
    let name_clip = Rect::from_min_max(pos2(x, rect.top()), pos2(x + columns.name, rect.bottom()));
    painter.with_clip_rect(name_clip).text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        &row.file_name,
        theme::regular(13.0),
        p.text,
    );
    x += columns.name + 8.0;
    if columns.module > 0.0 {
        if let Some(module) = &row.module {
            let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(x + columns.module, rect.bottom()));
            painter.with_clip_rect(clip).text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                module,
                theme::regular(12.5),
                p.text_secondary,
            );
        }
        x += columns.module + 8.0;
    }
    painter.text(
        pos2(x + columns.line, y),
        Align2::RIGHT_CENTER,
        row.line.to_string(),
        theme::regular(12.5),
        p.text_secondary,
    );
    x += columns.line + 8.0;
    let access = Rect::from_center_size(pos2(x + 7.0, y), Vec2::splat(13.0));
    match (row.comment, row.access) {
        (true, _) => glyph_comment(&painter, access, p.text_secondary),
        (false, UsageAccess::Write) => glyph_write(&painter, access, p.text),
        (false, UsageAccess::Read) => glyph_read(&painter, access, p.text_secondary),
    }
    x += 22.0;
    let galley = painter.layout_job(matched(&row.preview, &row.highlights, p));
    painter.galley(pos2(x, y - galley.size().y / 2.0), galley, p.text);
}

fn matched(preview: &str, highlights: &[[usize; 2]], p: &IdePalette) -> LayoutJob {
    let plain = TextFormat::simple(theme::regular(13.0), p.text);
    let strong = TextFormat::simple(theme::bold(13.0), p.text);
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut bold = false;
    for (index, c) in preview.chars().enumerate() {
        let now = highlights.iter().any(|[from, to]| (*from..*to).contains(&index));
        if now != bold && !run.is_empty() {
            job.append(&run, 0.0, if bold { strong.clone() } else { plain.clone() });
            run.clear();
        }
        bold = now;
        run.push(c);
    }
    if !run.is_empty() {
        job.append(&run, 0.0, if bold { strong } else { plain });
    }
    job
}

fn line(painter: &egui::Painter, points: &[Pos2], stroke: Stroke) {
    painter.add(egui::Shape::line(points.to_vec(), stroke));
}

fn stroke(rect: Rect, color: Color32) -> Stroke {
    Stroke::new((rect.width() / 10.0).max(1.2), color)
}

fn glyph_merge(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let middle = rect.center().y;
    line(painter, &[rect.left_top() + vec2(0.0, 2.0), pos2(rect.center().x, middle), rect.right_center()], s);
    line(painter, &[rect.left_bottom() - vec2(0.0, 2.0), pos2(rect.center().x, middle)], s);
}

fn glyph_read(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.width() / 2.0;
    let eye: Vec<Pos2> = (0..=16)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / 16.0;
            rect.center() + vec2(angle.cos() * r, angle.sin() * r * 0.55)
        })
        .collect();
    line(painter, &eye, s);
    painter.circle_filled(rect.center(), r * 0.28, color);
}

fn glyph_write(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let tip = rect.left_bottom() + vec2(1.0, -1.0);
    let end = rect.right_top() + vec2(-1.0, 1.0);
    line(painter, &[tip, end], s);
    line(painter, &[tip + vec2(0.0, -3.0), end + vec2(-3.0, 0.0)], s);
    line(painter, &[tip + vec2(3.0, 0.0), end + vec2(0.0, 3.0)], s);
}

fn glyph_generated(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let h = rect.height();
    let at = |x: f32, y: f32| rect.left_top() + vec2(x * w, y * h);
    line(
        painter,
        &[at(0.6, 0.0), at(0.25, 0.55), at(0.55, 0.55), at(0.4, 1.0), at(0.78, 0.4), at(0.48, 0.4), at(0.6, 0.0)],
        s,
    );
}

fn glyph_import(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let h = rect.height();
    let at = |x: f32, y: f32| rect.left_top() + vec2(x * w, y * h);
    line(painter, &[at(0.45, 0.1), at(0.95, 0.1), at(0.95, 0.9), at(0.45, 0.9)], s);
    line(painter, &[at(0.0, 0.5), at(0.7, 0.5)], s);
    line(painter, &[at(0.45, 0.28), at(0.7, 0.5), at(0.45, 0.72)], s);
}

fn glyph_comment(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let h = rect.height();
    let at = |x: f32, y: f32| rect.left_top() + vec2(x * w, y * h);
    line(
        painter,
        &[
            at(0.05, 0.1),
            at(0.95, 0.1),
            at(0.95, 0.7),
            at(0.45, 0.7),
            at(0.2, 0.95),
            at(0.25, 0.7),
            at(0.05, 0.7),
            at(0.05, 0.1),
        ],
        s,
    );
}

pub(super) fn glyph_preview(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect.shrink(0.5), 2, s, egui::StrokeKind::Inside);
    for row in 1..=3 {
        let y = rect.top() + rect.height() * row as f32 / 4.0;
        painter.hline(rect.left() + 3.0..=rect.right() - 3.0, y, s);
    }
}

const PREVIEW_LIMIT: u64 = 2 << 20;

pub(super) fn preview_text(session: &zen_core::Session, path: &Path) -> Option<String> {
    let open = session
        .state::<zen_workbench::Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned());
    open.or_else(|| {
        let small = std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= PREVIEW_LIMIT);
        small.then(|| std::fs::read_to_string(path).ok()).flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_too_big_to_preview_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let (big, small) = (dir.path().join("big.log"), dir.path().join("small.go"));
        std::fs::File::create(&big).unwrap().set_len(PREVIEW_LIMIT + 1).unwrap();
        std::fs::write(&small, "package main\n").unwrap();
        let session = zen_core::Session::new();
        assert_eq!(preview_text(&session, &big), None);
        assert_eq!(preview_text(&session, &small).as_deref(), Some("package main\n"));
    }

    fn state() -> ShowUsagesState {
        let row = |line: u32, start: usize, access: UsageAccess| UsageRow {
            path: PathBuf::from("/p/main.go"),
            relative: "main.go".to_owned(),
            file_name: "main.go".to_owned(),
            module: Some("app".to_owned()),
            line,
            column: 2,
            start,
            end: start + 7,
            preview: "println(Version)".to_owned(),
            highlights: vec![[8, 15]],
            access,
            comment: false,
            count: 1,
        };
        ShowUsagesState {
            search: 1,
            generation: 3,
            open: true,
            symbol: Some("Version".to_owned()),
            kind: DeclarationKind::Variable,
            declaration: Some(PathBuf::from("/p/cmd/main.go")),
            declaration_relative: Some("cmd/main.go".to_owned()),
            total: 2,
            found: 2,
            rows: vec![row(6, 40, UsageAccess::Write), row(7, 60, UsageAccess::Read)],
            more: 3,
            ..ShowUsagesState::default()
        }
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn frame(ctx: &egui::Context, popup: &mut UsagesPopup, events: Vec<egui::Event>) -> Vec<PopupAction> {
        let mut out = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            out = popup.keys(ui);
            out.extend(popup.show(ui, &IdePalette::zen_dark(), CaretPlace::default()));
        });
        output.textures_delta.clear();
        out
    }

    #[test]
    fn the_header_names_the_kind_the_symbol_the_file_and_the_count() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let state = state();
        assert_eq!(title(&state), "Variable Version in cmd/main.go · 2 usages");
        let pieces = header_pieces("Variable", "Version", "cmd/main.go");
        assert_eq!(pieces[1], ("Version".to_owned(), Piece::Strong));
        assert_eq!(pieces[0].1, Piece::Plain);
        assert_eq!(scope_name(UsagesScope::ProjectFiles), "Project Files");
        assert_eq!(zen_i18n::source_text("ide-show-usages-more", &[("count", 250.into())]), "250 more usages");
    }

    #[test]
    fn arrows_enter_more_toggles_and_escape() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        let mut popup = UsagesPopup::default();
        popup.open_at(pos2(10.0, 20.0));
        popup.update(state());
        assert_eq!(popup.at, Some(PopupAnchor::At(pos2(10.0, 20.0))), "the hint's place wins over the caret");
        assert!(popup.is_open());
        assert!(frame(&ctx, &mut popup, vec![key(Key::ArrowDown, Modifiers::NONE)]).is_empty());
        assert_eq!(popup.selected_row().unwrap().line, 7);
        assert_eq!(
            frame(&ctx, &mut popup, vec![key(Key::Enter, Modifiers::NONE)]),
            [PopupAction::GoTo { path: PathBuf::from("/p/main.go"), offset: 60 }, PopupAction::Close]
        );
        frame(&ctx, &mut popup, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert!(popup.selected_row().is_none(), "the more usages row");
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Enter, Modifiers::NONE)]), [PopupAction::More]);
        frame(&ctx, &mut popup, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert_eq!(popup.selected_row().unwrap().line, 6, "round the end");
        assert_eq!(
            frame(&ctx, &mut popup, vec![key(Key::L, Modifiers::CTRL), key(Key::W, Modifiers::CTRL)]),
            [PopupAction::Options(json!({ "mergeSameLine": true })), PopupAction::Options(json!({ "write": false }))]
        );
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::F7, Modifiers::ALT)]), [PopupAction::OpenInFindWindow]);
        assert!(popup.takes_key(Key::R, Modifiers::CTRL), "⌃R is the popup's, not Run's");
        assert!(!popup.takes_key(Key::R, Modifiers::COMMAND));
        assert_eq!(frame(&ctx, &mut popup, vec![key(Key::Escape, Modifiers::NONE)]), [PopupAction::Close]);
        assert_eq!(PopupAction::More.command().0, "navigate.showUsages.more");
        assert_eq!(PopupAction::Settings.command().0, "navigate.usages.settings");
    }

    #[test]
    fn the_same_search_keeps_its_place_and_a_new_one_opens_at_the_caret() {
        let mut popup = UsagesPopup::default();
        popup.update(state());
        popup.at = Some(PopupAnchor::At(pos2(5.0, 5.0)));
        popup.selected = 1;
        let mut same = state();
        same.generation = 4;
        same.rows.truncate(1);
        same.more = 0;
        popup.update(same);
        assert_eq!((popup.at, popup.selected, popup.generation()), (Some(PopupAnchor::At(pos2(5.0, 5.0))), 0, Some(4)));
        let mut next = state();
        next.search = 2;
        popup.update(next);
        assert_eq!(popup.at, Some(PopupAnchor::UNDER_CARET));
        popup.hide();
        assert!(!popup.is_open());
    }

    #[test]
    fn the_preview_reads_the_lines_around_the_usage_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        let text: String = (1..=20).map(|line| format!("line {line}\n")).collect();
        std::fs::write(&file, text).unwrap();
        let mut state = state();
        state.options.preview = true;
        for row in &mut state.rows {
            row.path = file.clone();
        }
        let mut popup = UsagesPopup::default();
        popup.update(state);
        let session = zen_core::Session::new();
        popup.follow_source(&session);
        let source = popup.source.as_ref().unwrap();
        assert_eq!((source.first, source.lines.first().map(String::as_str)), (2, Some("line 3")));
        assert_eq!(source.lines.len(), PREVIEW_LINES);
        std::fs::write(&file, "changed\n").unwrap();
        popup.follow_source(&session);
        assert_eq!(popup.source.as_ref().unwrap().lines[0], "line 3", "not read again for the same row");
        popup.selected = 1;
        popup.follow_source(&session);
        assert!(popup.source.as_ref().unwrap().lines.is_empty(), "read again for another row");
        popup.hide();
        assert!(popup.source.is_none() && popup.anchor.is_none());
    }

    #[test]
    fn the_matches_are_bold() {
        let job = matched("println(Version, Version)", &[[8, 15], [17, 24]], &IdePalette::zen_dark());
        let texts: Vec<&str> = job
            .sections
            .iter()
            .map(|section| &job.text[section.byte_range.start.0..section.byte_range.end.0])
            .collect();
        assert_eq!(texts, ["println(", "Version", ", ", "Version", ")"]);
    }
}
