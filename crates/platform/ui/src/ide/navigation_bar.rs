use std::path::PathBuf;
use std::time::Duration;

use egui::{Align2, Event, Id, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::language::navigation_bar::{ItemKind, NavigationBarState, NavigationChildren, NavigationItem};
use zen_workbench::language::structure::StructureStatus;

use super::agent::ActionError;
use super::app::Request;
use super::file_structure::name_job;
use super::glyphs::{glyph_folder, glyph_search, glyph_square, symbol_badge};
use super::settings::NavigationBarPlacement;
use super::{IdePalette, IdeView, metrics};
use crate::{accessibility, theme};

const SETTLE: f64 = 0.3;
const PENDING_POLL: f64 = 0.2;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 5.0;
const INNER: f32 = 4.0;
const CHEVRON: f32 = 14.0;
const FONT: f32 = 12.5;
const ROW: f32 = 24.0;
const POPUP_WIDTH: f32 = 260.0;
const POPUP_HEIGHT: f32 = 420.0;
const FLOATING_LEFT: f32 = 60.0;
const FLOATING_TOP: f32 = 6.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadKey {
    path: Option<PathBuf>,
    version: u64,
    caret: usize,
    members: bool,
}

struct Popup {
    segment: usize,
    items: Vec<NavigationItem>,
    status: StructureStatus,
    reason: Option<String>,
    query: String,
    matching: Vec<usize>,
    selected: usize,
    reveal: bool,
    scroll: f32,
    read_at: f64,
    params: Value,
}

impl Popup {
    fn new(segment: usize, read: NavigationChildren, selected: usize, now: f64, params: Value) -> Self {
        let matching = (0..read.items.len()).collect();
        Self {
            segment,
            items: read.items,
            status: read.status,
            reason: read.reason,
            query: String::new(),
            matching,
            selected,
            reveal: true,
            scroll: 0.0,
            read_at: now,
            params,
        }
    }

    fn shown(&self) -> Vec<&NavigationItem> {
        self.matching.iter().filter_map(|&index| self.items.get(index)).collect()
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.reveal = true;
    }

    fn filter(&mut self, query: &str) {
        self.query = query.to_owned();
        self.refilter();
        let best = self.best();
        self.select(best);
    }

    fn refilter(&mut self) {
        let query = self.query.trim();
        self.matching = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| query.is_empty() || zen_index::match_positions(query, &item.name).is_some())
            .map(|(index, _)| index)
            .collect();
        self.selected = self.selected.min(self.matching.len().saturating_sub(1));
    }

    fn best(&self) -> usize {
        let query = self.query.trim();
        self.shown()
            .iter()
            .enumerate()
            .filter_map(|(index, item)| Some((zen_index::score(query, &item.name, &item.name)?, index)))
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map_or(0, |(_, index)| index)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Open(PathBuf),
    GoTo(PathBuf, usize),
    Close,
}

#[derive(Default)]
pub(super) struct NavigationBar {
    state: Option<NavigationBarState>,
    seen: Option<ReadKey>,
    edit: Option<(u64, f64)>,
    read_at: f64,
    browsed: Option<Vec<NavigationItem>>,
    active: bool,
    floating: bool,
    selected: usize,
    popup: Option<Popup>,
    drawn: Vec<Rect>,
    bar_rect: Option<Rect>,
    popup_rect: Option<Rect>,
    above: bool,
}

fn focus_id() -> Id {
    Id::NULL.with("navigation-bar-keyboard")
}

impl NavigationBar {
    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    pub(super) fn popup_open(&self) -> bool {
        self.popup.is_some()
    }

    pub(super) fn is_floating(&self) -> bool {
        self.active && self.floating
    }

    pub(super) fn segments(&self) -> Vec<NavigationItem> {
        self.browsed
            .clone()
            .unwrap_or_else(|| self.state.as_ref().map(|state| state.segments.clone()).unwrap_or_default())
    }

    #[cfg(test)]
    pub(super) fn drawn_rects(&self) -> Vec<Rect> {
        self.drawn.clone()
    }

    pub(super) fn invalidate(&mut self) {
        self.seen = None;
    }

    fn read_key(session: &Session, members: bool) -> ReadKey {
        let document = session.state::<Workbench>().and_then(Workbench::active_document);
        ReadKey {
            path: document.map(|document| document.path().to_owned()),
            version: document.map_or(0, |document| document.version()),
            caret: document.map_or(0, |document| document.selections().primary().head),
            members,
        }
    }

    fn read(&mut self, registry: &Registry, session: &mut Session, key: ReadKey, now: f64) {
        let params = json!({ "members": key.members });
        self.state = registry
            .dispatch(session, "navigate.navigationBar.get", params)
            .ok()
            .and_then(|value| serde_json::from_value(value).ok());
        if self.seen.as_ref().is_some_and(|seen| seen.path != key.path) {
            self.browsed = None;
        }
        self.seen = Some(key);
        self.edit = None;
        self.read_at = now;
        let last = self.segments().len().saturating_sub(1);
        self.selected = self.selected.min(last);
    }

    pub(super) fn refresh(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, members: bool) {
        let now = ctx.input(|input| input.time);
        let key = Self::read_key(session, members);
        let due = match &self.seen {
            None => Some(0.0),
            Some(seen) if seen.path != key.path || seen.members != key.members => Some(0.0),
            Some(seen) if seen.version != key.version => {
                let edited_at = match self.edit {
                    Some((version, at)) if version == key.version => at,
                    _ => {
                        self.edit = Some((key.version, now));
                        now
                    }
                };
                Some(SETTLE - (now - edited_at))
            }
            Some(seen) if seen.caret != key.caret => Some(0.0),
            _ if self.state.as_ref().and_then(|state| state.status) == Some(StructureStatus::Pending) => {
                Some(PENDING_POLL - (now - self.read_at))
            }
            _ => None,
        };
        match due {
            Some(left) if left <= 0.0 => {
                self.read(registry, session, key, now);
                if self.state.as_ref().and_then(|state| state.status) == Some(StructureStatus::Pending) {
                    ctx.request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
                }
            }
            Some(left) => ctx.request_repaint_after(Duration::from_secs_f64(left)),
            None => {}
        }
        if let Some(popup) = &mut self.popup
            && popup.status == StructureStatus::Pending
            && now - popup.read_at >= PENDING_POLL
        {
            let params = popup.params.clone();
            if let Some(children) = children(registry, session, params) {
                popup.items = children.items;
                popup.status = children.status;
                popup.reason = children.reason;
                popup.refilter();
            }
            popup.read_at = now;
            ctx.request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
        }
    }

    pub(super) fn activate(&mut self, floating: bool) {
        self.active = true;
        self.floating = floating;
        self.browsed = None;
        self.popup = None;
        self.selected = self.segments().len().saturating_sub(1);
    }

    pub(super) fn deactivate(&mut self) {
        self.active = false;
        self.floating = false;
        self.popup = None;
        self.browsed = None;
    }

    fn open_segment(&mut self, registry: &Registry, session: &mut Session, index: usize, now: f64) -> Option<Outcome> {
        let segments = self.segments();
        let segment = segments.get(index)?;
        self.selected = index;
        self.active = true;
        let params = match segment.kind {
            ItemKind::Project | ItemKind::Folder => json!({ "path": segment.path }),
            ItemKind::File if segment.has_children => json!({ "path": segment.path }),
            ItemKind::Symbol if segment.has_children => json!({ "path": segment.path, "offset": segment.offset }),
            ItemKind::File => {
                self.popup = None;
                return Some(Outcome::Open(segment.path.clone()));
            }
            ItemKind::Symbol => {
                self.popup = None;
                return Some(Outcome::GoTo(segment.path.clone(), segment.offset.unwrap_or(0)));
            }
        };
        let read = children(registry, session, params.clone()).unwrap_or(NavigationChildren {
            status: StructureStatus::Unavailable,
            reason: None,
            items: Vec::new(),
        });
        let next = segments.get(index + 1).map(|next| (next.path.clone(), next.offset));
        let selected = read
            .items
            .iter()
            .position(|item| next.as_ref().is_some_and(|(path, offset)| &item.path == path && &item.offset == offset))
            .unwrap_or(0);
        self.popup = Some(Popup::new(index, read, selected, now, params));
        None
    }

    fn pick(&mut self, registry: &Registry, session: &mut Session, now: f64) -> Option<Outcome> {
        let popup = self.popup.as_ref()?;
        let item = (*popup.shown().get(popup.selected)?).clone();
        let segment = popup.segment;
        match item.kind {
            ItemKind::Folder | ItemKind::Project => {
                let mut segments = self.segments();
                segments.truncate(segment + 1);
                segments.push(item);
                let index = segments.len() - 1;
                self.browsed = Some(segments);
                self.open_segment(registry, session, index, now)
            }
            ItemKind::File => Some(Outcome::Open(item.path)),
            ItemKind::Symbol => Some(Outcome::GoTo(item.path, item.offset.unwrap_or(0))),
        }
    }

    pub(super) fn keys(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) -> Option<Outcome> {
        if !self.active {
            return None;
        }
        let now = ctx.input(|input| input.time);
        let mut taken: Vec<Event> = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let ours = match event {
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        (modifiers.is_none() || *modifiers == Modifiers::SHIFT)
                            && matches!(
                                key,
                                Key::ArrowLeft
                                    | Key::ArrowRight
                                    | Key::ArrowUp
                                    | Key::ArrowDown
                                    | Key::Enter
                                    | Key::Escape
                                    | Key::Backspace
                                    | Key::Tab
                                    | Key::Home
                                    | Key::End
                            )
                    }
                    Event::Text(_) => true,
                    _ => false,
                };
                if ours {
                    taken.push(event.clone());
                }
                !ours
            });
        });
        let mut outcome = None;
        let mut events = taken.into_iter();
        while outcome.is_none() {
            let Some(event) = events.next() else { break };
            match event {
                Event::Key { key: Key::Escape, .. } => outcome = Some(Outcome::Close),
                Event::Text(text) => {
                    if let Some(popup) = &mut self.popup {
                        let query = format!("{}{text}", popup.query);
                        popup.filter(&query);
                    }
                }
                Event::Key { key: Key::Backspace, .. } => {
                    if let Some(popup) = &mut self.popup {
                        let mut query = popup.query.clone();
                        query.pop();
                        popup.filter(&query);
                    }
                }
                Event::Key { key, .. } => outcome = self.key(registry, session, key, now),
                _ => {}
            }
        }
        let left: Vec<Event> = events.collect();
        if !left.is_empty() {
            ctx.input_mut(|input| {
                input.events.splice(0..0, left);
            });
        }
        if outcome.is_some() {
            self.deactivate();
        }
        ctx.request_repaint();
        outcome
    }

    fn key(&mut self, registry: &Registry, session: &mut Session, key: Key, now: f64) -> Option<Outcome> {
        let count = self.segments().len();
        if let Some(popup) = &mut self.popup {
            let shown = popup.shown().len();
            match key {
                Key::ArrowUp => popup.select(popup.selected.saturating_sub(1)),
                Key::ArrowDown => popup.select((popup.selected + 1).min(shown.saturating_sub(1))),
                Key::Home => popup.select(0),
                Key::End => popup.select(shown.saturating_sub(1)),
                Key::Enter => return self.pick(registry, session, now),
                Key::ArrowLeft if popup.segment > 0 => {
                    let index = popup.segment - 1;
                    return self.open_segment(registry, session, index, now).and(None);
                }
                Key::ArrowRight if popup.segment + 1 < count => {
                    let index = popup.segment + 1;
                    return self.open_segment(registry, session, index, now).and(None);
                }
                Key::ArrowRight => {
                    let folder = popup.shown().get(popup.selected).is_some_and(|item| item.kind == ItemKind::Folder);
                    if folder {
                        return self.pick(registry, session, now);
                    }
                }
                _ => {}
            }
            return None;
        }
        match key {
            Key::ArrowLeft => self.selected = self.selected.saturating_sub(1),
            Key::ArrowRight => self.selected = (self.selected + 1).min(count.saturating_sub(1)),
            Key::Home => self.selected = 0,
            Key::End => self.selected = count.saturating_sub(1),
            Key::ArrowDown | Key::Enter => return self.open_segment(registry, session, self.selected, now),
            _ => {}
        }
        None
    }

    pub(super) fn draw(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        above: bool,
    ) -> Option<Outcome> {
        self.drawn.clear();
        self.above = above;
        self.bar_rect = Some(rect);
        let segments = self.segments();
        let painter = ui.painter_at(rect);
        if self.active {
            let id = focus_id().with(ui.layer_id());
            let keyboard = ui.interact(rect, id, Sense::click());
            accessibility::name_with(&keyboard, egui::Role::Toolbar, || t!("a11y-navigation-bar"));
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let font = theme::regular(FONT);
        let y = rect.center().y;
        let colour = if self.active { p.text } else { p.text_secondary };
        let galleys: Vec<_> =
            segments.iter().map(|segment| painter.layout_no_wrap(segment.name.clone(), font.clone(), colour)).collect();
        let widths: Vec<f32> = galleys.iter().map(|galley| INNER + ICON + ICON_GAP + galley.size().x + INNER).collect();
        let ellipsis = painter.layout_no_wrap("…".to_owned(), font.clone(), p.text_secondary);
        let focus = self.popup.as_ref().map(|popup| popup.segment).or(self.active.then_some(self.selected));
        let first = first_shown(&widths, rect.width(), INNER + ellipsis.size().x + CHEVRON, focus);
        let mut x = rect.left();
        if first > 0 {
            painter.galley(pos2(x + INNER, y - ellipsis.size().y / 2.0), ellipsis.clone(), p.text_secondary);
            x += INNER + ellipsis.size().x;
        }
        let mut clicked = None;
        for (index, (segment, galley)) in segments.iter().zip(galleys).enumerate() {
            if index < first || x >= rect.right() {
                self.drawn.push(Rect::NOTHING);
                continue;
            }
            if index > 0 {
                let chevron = Rect::from_center_size(pos2(x + CHEVRON / 2.0, y), Vec2::splat(10.0));
                crate::icons::chevron_right(&painter, chevron, p.text_secondary);
                x += CHEVRON;
            }
            let item = Rect::from_min_size(pos2(x, y - 10.0), vec2(widths[index], 20.0));
            let response =
                ui.interact(item.intersect(rect), ui.scope_id().with(("navigation-segment", index)), Sense::click());
            accessibility::button(&response, &segment.name);
            let selected = self.active && self.selected == index;
            if selected {
                painter.rect_filled(item, 4, p.selection);
            } else if response.hovered() {
                painter.rect_filled(item, 4, p.hover);
            }
            let icon = Rect::from_center_size(pos2(item.left() + INNER + ICON / 2.0, y), Vec2::splat(ICON));
            item_icon(&painter, icon, segment, p);
            painter.galley(pos2(icon.right() + ICON_GAP, y - galley.size().y / 2.0), galley, colour);
            if response.clicked() {
                clicked = Some(index);
            }
            self.drawn.push(item);
            x = item.right();
        }
        let now = ui.input(|input| input.time);
        let index = clicked?;
        if self.popup.as_ref().is_some_and(|popup| popup.segment == index) {
            self.popup = None;
            return None;
        }
        let outcome = self.open_segment(registry, session, index, now);
        if outcome.is_some() {
            self.deactivate();
        }
        outcome
    }

    pub(super) fn show_popup(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
    ) -> Option<Outcome> {
        let pressed_outside = ui.input(|input| input.pointer.any_pressed())
            && ui.input(|input| input.pointer.interact_pos()).is_some_and(|pointer| {
                !self.bar_rect.is_some_and(|rect| rect.contains(pointer))
                    && !self.popup_rect.is_some_and(|rect| rect.contains(pointer))
            });
        let Some(popup) = &self.popup else {
            self.popup_rect = None;
            if self.active && pressed_outside {
                self.deactivate();
            }
            return None;
        };
        let anchor = self.drawn.get(popup.segment).copied().unwrap_or(Rect::NOTHING);
        let (position, pivot) = if self.above {
            (anchor.left_top() - vec2(0.0, 4.0), Align2::LEFT_BOTTOM)
        } else {
            (anchor.left_bottom() + vec2(0.0, 4.0), Align2::LEFT_TOP)
        };
        let shown = popup.shown();
        let (query, selected, status) = (popup.query.trim(), popup.selected, popup.status);
        let reveal = popup.reveal.then(|| {
            let top = selected as f32 * ROW;
            let view = POPUP_HEIGHT;
            if top < popup.scroll {
                top
            } else if top + ROW > popup.scroll + view {
                top + ROW - view
            } else {
                popup.scroll
            }
        });
        let mut picked = None;
        let mut scroll = None;
        let area = egui::Area::new(ui.scope_id().with("navigation-bar-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .pivot(pivot)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(5)).show(ui, |ui| {
                    ui.set_width(POPUP_WIDTH);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if !popup.query.is_empty() {
                        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                        let glass = Rect::from_center_size(pos2(row.left() + 10.0, row.center().y), Vec2::splat(12.0));
                        glyph_search(ui.painter(), glass, p.text_secondary);
                        let colour = if shown.is_empty() { p.excluded } else { p.text };
                        ui.painter().text(
                            pos2(row.left() + 22.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &popup.query,
                            theme::regular(13.0),
                            colour,
                        );
                    }
                    if shown.is_empty() {
                        let message = match (status, &popup.reason) {
                            (StructureStatus::Pending, _) => t!("ide-structure-loading"),
                            (_, Some(reason)) => reason.clone(),
                            _ => t!("ide-navigation-bar-empty"),
                        };
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                        ui.painter().text(
                            pos2(rect.left() + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            message,
                            theme::regular(13.0),
                            p.text_secondary,
                        );
                        return;
                    }
                    let room = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), POPUP_HEIGHT));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                        let mut list = egui::ScrollArea::vertical()
                            .id_salt("navigation-bar-rows")
                            .max_height(POPUP_HEIGHT)
                            .auto_shrink([false, true]);
                        if let Some(offset) = reveal {
                            list = list.vertical_scroll_offset(offset);
                        }
                        let output = list.show_rows(ui, ROW, shown.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for index in range {
                                let item = shown[index];
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                if index == selected {
                                    ui.painter().rect_filled(rect, 5, p.selection);
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 5, p.popup_hover);
                                }
                                accessibility::name(&response, egui::Role::ListItem, &item.name);
                                let icon = Rect::from_center_size(
                                    pos2(rect.left() + 8.0 + ICON / 2.0, rect.center().y),
                                    Vec2::splat(ICON),
                                );
                                item_icon(ui.painter(), icon, item, p);
                                let job = name_job(&item.name, query, p);
                                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                                let at = pos2(icon.right() + ICON_GAP + 2.0, rect.center().y - galley.size().y / 2.0);
                                ui.painter().with_clip_rect(rect).galley(at, galley, p.text);
                                if response.clicked() {
                                    picked = Some(index);
                                }
                            }
                        });
                        scroll = Some(output.state.clamped_offset().y);
                    });
                });
            });
        if let Some(popup) = &mut self.popup {
            popup.reveal = false;
            if let Some(scroll) = scroll {
                popup.scroll = scroll;
            }
        }
        self.popup_rect = Some(area.response.rect);
        if let Some(index) = picked {
            if let Some(popup) = &mut self.popup {
                popup.selected = index;
            }
            let now = ui.input(|input| input.time);
            let outcome = self.pick(registry, session, now);
            if outcome.is_some() {
                self.deactivate();
            }
            return outcome;
        }
        if pressed_outside {
            self.deactivate();
        }
        None
    }

    pub(super) fn describe(&self, placement: NavigationBarPlacement, members: bool) -> Value {
        let popup = self.popup.as_ref().map(|popup| {
            json!({
                "segment": popup.segment,
                "filter": popup.query,
                "status": popup.status,
                "items": popup.shown(),
                "selected": popup.selected,
            })
        });
        json!({
            "placement": placement.name(),
            "showMembers": members,
            "active": self.active,
            "floating": self.is_floating(),
            "segments": self.segments(),
            "selected": self.active.then_some(self.selected),
            "popup": popup,
        })
    }
}

fn first_shown(widths: &[f32], room: f32, ellipsis: f32, focus: Option<usize>) -> usize {
    let total: f32 = widths.iter().sum::<f32>() + CHEVRON * widths.len().saturating_sub(1) as f32;
    if total <= room {
        return 0;
    }
    let mut first = widths.len().saturating_sub(1);
    let mut used = widths.last().copied().unwrap_or_default() + ellipsis;
    while first > 0 && used + widths[first - 1] + CHEVRON <= room {
        first -= 1;
        used += widths[first] + CHEVRON;
    }
    focus.map_or(first, |focus| first.min(focus))
}

fn children(registry: &Registry, session: &mut Session, params: Value) -> Option<NavigationChildren> {
    registry
        .dispatch(session, "navigate.navigationBar.children", params)
        .ok()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn item_icon(painter: &egui::Painter, rect: Rect, item: &NavigationItem, p: &IdePalette) {
    match item.kind {
        ItemKind::Project => glyph_square(painter, rect.shrink(1.5), p.text_secondary),
        ItemKind::Folder => glyph_folder(painter, rect, p.text_secondary),
        ItemKind::File => crate::file_icons::file_icon(painter, rect, &item.name),
        ItemKind::Symbol => symbol_badge(painter, rect, item.symbol_kind.unwrap_or(zen_syntax::SymbolKind::Other), p),
    }
}

impl IdeView {
    fn navigation_bar_floats(&self) -> bool {
        self.appearance.hides_chrome() || self.overview.settings.navigation_bar == NavigationBarPlacement::Hidden
    }

    pub(super) fn navigation_bar_input(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.navigation_bar.drawn.clear();
        self.navigation_bar.bar_rect = None;
        let members = self.overview.settings.navigation_bar_members;
        let shown = !self.navigation_bar_floats() || self.navigation_bar.is_active();
        if shown {
            self.navigation_bar.refresh(ui.ctx(), registry, session, members);
        }
        let outcome = self.navigation_bar.keys(ui.ctx(), registry, session);
        self.follow_navigation_bar(ui, registry, session, outcome);
    }

    fn follow_navigation_bar(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        outcome: Option<Outcome>,
    ) {
        match outcome {
            Some(Outcome::Open(path)) => {
                self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                self.editor_view.focus();
            }
            Some(Outcome::GoTo(path, offset)) => {
                self.dispatch(ui, registry, session, "navigate.place.goTo", json!({ "path": path, "offset": offset }));
                self.editor_view.focus();
            }
            Some(Outcome::Close) => self.editor_view.focus(),
            None => {}
        }
    }

    pub(super) fn navigation_bar_in(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        registry: &Registry,
        session: &mut Session,
        above: bool,
    ) {
        let palette = self.palette;
        let outcome = self.navigation_bar.draw(ui, rect, registry, session, &palette, above);
        self.follow_navigation_bar(ui, registry, session, outcome);
    }

    pub(super) fn navigation_bar_overlay(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        if self.navigation_bar.is_active() && !self.navigation_bar.floating && self.navigation_bar_floats() {
            self.navigation_bar.floating = true;
        }
        if self.navigation_bar.is_floating() {
            let screen = ui.ctx().content_rect();
            let top = screen.top() + if self.appearance.hides_chrome() { FLOATING_TOP } else { metrics::TOOLBAR };
            let width = (screen.width() - 2.0 * FLOATING_LEFT).max(200.0);
            let mut outcome = None;
            egui::Area::new(ui.scope_id().with("navigation-bar-floating"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(screen.left() + FLOATING_LEFT, top))
                .show(ui.ctx(), |ui| {
                    palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(width, metrics::STATUS_BAR - 8.0), Sense::hover());
                        outcome = self.navigation_bar.draw(ui, rect, registry, session, &palette, false);
                    });
                });
            self.follow_navigation_bar(ui, registry, session, outcome);
        }
        let outcome = self.navigation_bar.show_popup(ui, registry, session, &palette);
        self.follow_navigation_bar(ui, registry, session, outcome);
    }

    pub(super) fn navigation_bar_top(&self) -> bool {
        self.overview.settings.navigation_bar == NavigationBarPlacement::Top
    }

    pub(super) fn navigation_bar_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<Value, ActionError> {
        let members = self.overview.settings.navigation_bar_members;
        let placement = self.overview.settings.navigation_bar;
        if params.get("close").and_then(Value::as_bool) == Some(true) {
            self.navigation_bar.deactivate();
            self.editor_view.focus();
            return Ok(self.navigation_bar.describe(placement, members));
        }
        let now = ctx.input(|input| input.time);
        if !self.navigation_bar.is_active() {
            let key = NavigationBar::read_key(session, members);
            self.navigation_bar.read(registry, session, key, now);
            let floating = self.navigation_bar_floats();
            self.navigation_bar.activate(floating);
        }
        if let Some(segment) = params.get("segment").and_then(Value::as_u64) {
            let count = self.navigation_bar.segments().len();
            let segment = segment as usize;
            if segment >= count {
                return Err(ActionError::Refused(format!(
                    "the navigation bar has {count} segments; `segment` counts from 0"
                )));
            }
            self.navigation_bar.selected = segment;
            if self.navigation_bar.popup.as_ref().is_some_and(|popup| popup.segment != segment) {
                self.navigation_bar.popup = None;
            }
        }
        match params.get("open").and_then(Value::as_bool) {
            Some(true) => {
                let index = self.navigation_bar.selected;
                if self.navigation_bar.open_segment(registry, session, index, now).is_some() {
                    let segments = self.navigation_bar.segments();
                    let name = segments.get(index).map(|segment| segment.name.clone()).unwrap_or_default();
                    return Err(ActionError::Refused(format!(
                        "the segment `{name}` has no children to list; Enter on it opens its file or declaration"
                    )));
                }
            }
            Some(false) => self.navigation_bar.popup = None,
            None => {}
        }
        if let Some(filter) = params.get("filter").and_then(Value::as_str) {
            let Some(popup) = &mut self.navigation_bar.popup else {
                return Err(ActionError::Refused(
                    "`filter` types into a segment's popup: open one with `open: true`".to_owned(),
                ));
            };
            popup.filter(filter);
        }
        ctx.request_repaint();
        Ok(self.navigation_bar.describe(placement, members))
    }

    pub(super) fn navigation_bar_setting(&mut self, action: &str) -> Value {
        let mut settings = self.overview.settings.clone();
        match action {
            "view.navigationBar.top" => settings.navigation_bar = NavigationBarPlacement::Top,
            "view.navigationBar.statusBar" => settings.navigation_bar = NavigationBarPlacement::StatusBar,
            "view.navigationBar.hide" => settings.navigation_bar = NavigationBarPlacement::Hidden,
            "view.navigationBar.members.toggle" => {
                settings.navigation_bar_members = !settings.navigation_bar_members;
                self.navigation_bar.invalidate();
            }
            _ => {}
        }
        if self.navigation_bar.is_active() {
            self.navigation_bar.floating =
                settings.navigation_bar == NavigationBarPlacement::Hidden || self.appearance.hides_chrome();
        }
        let answer = json!({ "navigationBar": { "placement": settings.navigation_bar.name(), "showMembers": settings.navigation_bar_members } });
        self.overview.settings = settings.clone();
        self.requests.push(Request::Settings(Box::new(settings)));
        answer
    }

    pub(super) fn navigation_bar_checked(&self, action: &str) -> bool {
        let settings = &self.overview.settings;
        match action {
            "view.navigationBar.top" => settings.navigation_bar == NavigationBarPlacement::Top,
            "view.navigationBar.statusBar" => settings.navigation_bar == NavigationBarPlacement::StatusBar,
            "view.navigationBar.hide" => settings.navigation_bar == NavigationBarPlacement::Hidden,
            "view.navigationBar.members.toggle" => settings.navigation_bar_members,
            _ => false,
        }
    }
}

pub(super) fn spoken_path(segments: &[NavigationItem]) -> String {
    segments.iter().map(|segment| segment.name.as_str()).collect::<Vec<_>>().join(std::path::MAIN_SEPARATOR_STR)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{PointerButton, Pos2};
    use zen_syntax::{OutlineItem, Outliner, SymbolKind};
    use zen_workbench::language::structure::Outliners;

    use super::*;

    const SOURCE: &str = "package shop\n\ntype Cart struct {\n\titems []string\n}\n\nfunc (c *Cart) Add(item string) {\n\tc.items = append(c.items, item)\n}\n";

    fn at(needle: &str) -> usize {
        SOURCE.find(needle).unwrap()
    }

    struct CartOutliner;

    impl Outliner for CartOutliner {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, _: &str) -> Vec<OutlineItem> {
            let item = |name: &str, kind, start: &str, children| OutlineItem {
                name: name.to_owned(),
                detail: None,
                kind,
                start: at(start),
                end: SOURCE.len() - 1,
                selection_start: at(name),
                children,
            };
            vec![item(
                "Cart",
                SymbolKind::Struct,
                "type Cart",
                vec![item("Add", SymbolKind::Method, "func (c", Vec::new())],
            )]
        }
    }

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        bar: NavigationBar,
        time: f64,
        outcomes: Vec<Outcome>,
        left: Vec<Event>,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let project = dir.path().join("shop");
            std::fs::create_dir(&project).unwrap();
            let path = project.join("cart.go");
            std::fs::write(&path, SOURCE).unwrap();
            let mut session = Session::new();
            session.state_mut::<Outliners>().set(vec![Arc::new(CartOutliner)]);
            registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            let caret = at("append");
            registry
                .dispatch(
                    &mut session,
                    "editor.selection.set",
                    json!({ "selections": [{ "anchor": caret, "head": caret }] }),
                )
                .unwrap();
            Self {
                ctx,
                registry,
                session,
                bar: NavigationBar::default(),
                time: 0.0,
                outcomes: Vec::new(),
                left: Vec::new(),
                _dir: dir,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let mut output = self.ctx.run_ui(input, |ui| {
                self.bar.drawn.clear();
                self.bar.refresh(ui.ctx(), &self.registry, &mut self.session, true);
                self.outcomes.extend(self.bar.keys(ui.ctx(), &self.registry, &mut self.session));
                self.left = ui.input(|input| input.events.clone());
                let rect = Rect::from_min_max(pos2(8.0, 376.0), pos2(792.0, 400.0));
                self.outcomes.extend(self.bar.draw(ui, rect, &self.registry, &mut self.session, &palette, true));
                self.outcomes.extend(self.bar.show_popup(ui, &self.registry, &mut self.session, &palette));
            });
            output.textures_delta.clear();
        }

        fn click(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn names(&self) -> Vec<String> {
            self.bar.segments().into_iter().map(|segment| segment.name).collect()
        }
    }

    #[test]
    fn show_members_adds_the_declarations_at_the_caret_and_a_click_lists_a_segments_children() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["shop", "cart.go", "Cart", "Add"]);
        assert_eq!(harness.bar.drawn.len(), 4);
        assert!(!harness.bar.is_active(), "drawn without the keyboard");

        let cart = harness.bar.drawn[2].center();
        harness.click(cart);
        assert!(harness.bar.is_active() && harness.bar.popup_open(), "a click opens the segment's popup");
        let popup = harness.bar.popup.as_ref().unwrap();
        assert_eq!(popup.items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["Add"]);
        let row = harness.bar.popup_rect.unwrap();
        assert!(row.bottom() <= 376.0, "from the status bar the popup opens upwards: {row:?}");

        harness.click(pos2(row.left() + 40.0, row.bottom() - 10.0));
        let path = harness.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        assert_eq!(harness.outcomes, [Outcome::GoTo(path, at("Add"))], "a declaration is gone to");
        assert!(!harness.bar.is_active());
    }

    #[test]
    fn the_keyboard_moves_between_segments_and_escape_leaves() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        harness.bar.activate(false);
        assert_eq!(harness.bar.selected, 3, "the last segment");
        let key =
            |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        harness.frame(vec![key(Key::ArrowLeft), key(Key::ArrowLeft)]);
        assert_eq!(harness.bar.selected, 1);
        harness.frame(vec![key(Key::ArrowDown)]);
        let items: Vec<String> =
            harness.bar.popup.as_ref().unwrap().items.iter().map(|item| item.name.clone()).collect();
        assert_eq!(items, ["Cart"], "the file's declarations");
        harness.frame(vec![key(Key::ArrowLeft)]);
        assert_eq!(harness.bar.popup.as_ref().unwrap().segment, 0, "← in a popup goes to the previous segment's");
        harness.frame(vec![key(Key::Escape)]);
        assert_eq!(harness.outcomes, [Outcome::Close]);
        assert!(!harness.bar.is_active() && !harness.bar.popup_open());
    }

    #[test]
    fn keys_after_the_bar_closes_are_left_to_the_editor() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        harness.bar.activate(false);
        let escape = Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        harness.frame(vec![escape, Event::Text("x".to_owned())]);
        assert_eq!(harness.outcomes, [Outcome::Close]);
        assert_eq!(harness.left, [Event::Text("x".to_owned())], "the letter typed after Escape is not lost");
    }

    #[test]
    fn a_popup_of_thousands_of_entries_filters_once_per_keystroke_and_keeps_the_selection_in_view() {
        let mut harness = Harness::new();
        let folder = harness._dir.path().join("shop").join("big");
        std::fs::create_dir(&folder).unwrap();
        for index in 0..3000 {
            std::fs::write(folder.join(format!("file_{index:04}.go")), "package big\n").unwrap();
        }
        harness.frame(Vec::new());
        harness.bar.activate(false);
        harness.bar.selected = 0;
        let key =
            |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        harness.frame(vec![key(Key::ArrowDown)]);
        harness.frame(vec![Event::Text("big".to_owned())]);
        harness.frame(vec![key(Key::Enter)]);
        assert_eq!(harness.names(), ["shop", "big"], "browsed into the folder");
        assert_eq!(harness.bar.popup.as_ref().unwrap().shown().len(), 3000);
        harness.frame(vec![key(Key::End)]);
        harness.frame(Vec::new());
        let popup = harness.bar.popup.as_ref().unwrap();
        assert_eq!(popup.selected, 2999);
        let top = popup.selected as f32 * ROW;
        assert!(
            popup.scroll <= top && top + ROW <= popup.scroll + POPUP_HEIGHT + 0.5,
            "the last row is scrolled into view: {} for {top}",
            popup.scroll
        );
        harness.frame(vec![Event::Text("2998".to_owned())]);
        let popup = harness.bar.popup.as_ref().unwrap();
        let shown: Vec<&str> = popup.shown().iter().map(|item| item.name.as_str()).collect();
        assert_eq!(shown, ["file_2998.go"]);
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn a_path_wider_than_the_bar_keeps_its_end_in_view() {
        assert_eq!(first_shown(&[50.0, 50.0, 50.0], 200.0, 20.0, None), 0, "it fits");
        assert_eq!(first_shown(&[50.0, 50.0, 50.0, 50.0], 150.0, 20.0, None), 2, "the leading segments give way");
        assert_eq!(first_shown(&[50.0, 50.0, 50.0, 50.0], 150.0, 20.0, Some(0)), 0, "unless one has the keyboard");
        assert_eq!(first_shown(&[300.0], 100.0, 20.0, None), 0, "the last segment is always drawn");
    }
}
