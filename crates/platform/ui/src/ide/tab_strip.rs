use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, LayerId, Order, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use zen_i18n::t;
use zen_workbench::tabs::{CloseButton, EditorTabSettings, TabPlacement, TabRows};

use crate::accessibility;

use super::editor_groups::TabRow;
use super::{IdePalette, Side, hint, tab_drag, theme};

pub(super) type TabIcon<'a> = Box<dyn Fn(&Painter, Rect) + 'a>;

pub(super) struct Tab<'a> {
    pub(super) title: String,
    pub(super) color: Color32,
    pub(super) icon: TabIcon<'a>,
    pub(super) dirty: bool,
    pub(super) closable: bool,
    pub(super) draggable: bool,
    pub(super) strip: Option<Color32>,
    pub(super) mark: Option<(Color32, String)>,
    pub(super) pinned: bool,
    pub(super) preview: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Options {
    pub(super) height: f32,
    pub(super) focused: bool,
    pub(super) reorder: bool,
    pub(super) look: Look,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Look {
    pub(super) placement: TabPlacement,
    pub(super) rows: TabRows,
    pub(super) pinned_row: bool,
    pub(super) icons: bool,
    pub(super) close: CloseButton,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            placement: TabPlacement::Top,
            rows: TabRows::Scroll,
            pinned_row: false,
            icons: true,
            close: CloseButton::Right,
        }
    }
}

impl Look {
    pub(super) fn of(settings: &EditorTabSettings) -> Self {
        Self {
            placement: settings.placement,
            rows: settings.rows,
            pinned_row: settings.pinned_row,
            icons: settings.show_icon,
            close: settings.close_button,
        }
    }

    pub(super) fn vertical(self) -> bool {
        matches!(self.placement, TabPlacement::Left | TabPlacement::Right)
    }

    fn lead(self) -> f32 {
        if self.close == CloseButton::Left { CLOSE_SIZE } else { 0.0 }
    }

    fn title_at(self) -> f32 {
        self.lead() + if self.icons { 28.0 } else { 8.0 }
    }

    fn room(self) -> f32 {
        self.title_at() + if self.close == CloseButton::Right { 30.0 } else { 12.0 }
    }
}

#[derive(Default)]
pub(super) struct Outcome {
    pub(super) select: Option<usize>,
    pub(super) close: Option<usize>,
    pub(super) close_others: Option<usize>,
    pub(super) unpin: Option<usize>,
    pub(super) keep_open: Option<usize>,
    pub(super) drag_started: Option<usize>,
    pub(super) reorder: Option<(usize, usize)>,
    pub(super) dragging: Option<(usize, Pos2)>,
    pub(super) dropped: Option<(usize, Pos2)>,
    pub(super) responses: Vec<Option<Response>>,
    pub(super) places: Vec<Option<Rect>>,
}

const GAP: f32 = 4.0;

const TAB_HEIGHT: f32 = 28.0;

const CLOSE_SIZE: f32 = 18.0;

const LEAST_TAB: f32 = 72.0;

const COLUMN_WIDTH: f32 = 200.0;

fn tab_widths(ui: &Ui, p: &IdePalette, tabs: &[&Tab<'_>], look: Look) -> Vec<f32> {
    tabs.iter()
        .map(|tab| ui.painter().layout_no_wrap(tab.title.clone(), theme::regular(13.0), p.text).size().x + look.room())
        .collect()
}

fn squeezed(widths: &[f32], room: f32) -> Vec<f32> {
    let total = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32 + 16.0;
    if total <= room {
        return widths.to_vec();
    }
    let free = (room - GAP * widths.len().saturating_sub(1) as f32 - 16.0).max(0.0);
    let scale = free / widths.iter().sum::<f32>().max(1.0);
    widths.iter().map(|width| (width * scale).max(LEAST_TAB)).collect()
}

fn wrapped(widths: &[f32], room: f32) -> Vec<usize> {
    let mut rows = Vec::with_capacity(widths.len());
    let (mut x, mut row) = (8.0, 0);
    for width in widths {
        if x > 8.0 && x + width + 8.0 > room {
            row += 1;
            x = 8.0;
        }
        rows.push(row);
        x += width + GAP;
    }
    rows
}

fn splits(tabs: &[Tab<'_>], look: Look) -> bool {
    look.pinned_row && !look.vertical() && tabs.iter().any(|tab| tab.pinned) && tabs.iter().any(|tab| !tab.pinned)
}

fn part_height(ui: &Ui, p: &IdePalette, tabs: &[&Tab<'_>], width: f32, options: Options) -> f32 {
    if options.look.rows != TabRows::Multiple || options.look.vertical() {
        return options.height;
    }
    let rows = wrapped(&tab_widths(ui, p, tabs, options.look), width).last().map_or(1, |row| row + 1);
    options.height * rows as f32
}

pub(super) fn areas(whole: Rect, placement: TabPlacement, height: f32) -> (Rect, Rect) {
    let column = COLUMN_WIDTH.min(whole.width() / 2.0);
    let (strip, body) = match placement {
        TabPlacement::Left => (whole.with_max_x(whole.left() + column), whole.with_min_x(whole.left() + column)),
        TabPlacement::Right => (whole.with_min_x(whole.right() - column), whole.with_max_x(whole.right() - column)),
        TabPlacement::Bottom => (whole.with_min_y(whole.bottom() - height), whole.with_max_y(whole.bottom() - height)),
        TabPlacement::Top | TabPlacement::None => {
            (whole.with_max_y(whole.top() + height), whole.with_min_y(whole.top() + height))
        }
    };
    (strip, body)
}

pub(super) fn height(ui: &Ui, p: &IdePalette, tabs: &[Tab<'_>], width: f32, options: Options) -> f32 {
    if options.height <= 0.0 {
        return 0.0;
    }
    let all: Vec<&Tab<'_>> = tabs.iter().collect();
    if !splits(tabs, options.look) {
        return part_height(ui, p, &all, width, options);
    }
    let (pinned, rest): (Vec<&Tab<'_>>, Vec<&Tab<'_>>) = all.into_iter().partition(|tab| tab.pinned);
    part_height(ui, p, &pinned, width, options) + part_height(ui, p, &rest, width, options)
}

pub(super) fn show(
    ui: &mut Ui,
    p: &IdePalette,
    row: &mut TabRow,
    tabs: &[Tab<'_>],
    active: Option<usize>,
    options: Options,
) -> Outcome {
    let mut outcome = Outcome {
        responses: (0..tabs.len()).map(|_| None).collect(),
        places: vec![None; tabs.len()],
        ..Outcome::default()
    };
    let all: Vec<(usize, &Tab<'_>)> = tabs.iter().enumerate().collect();
    if splits(tabs, options.look) {
        let (pinned, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|(_, tab)| tab.pinned);
        let mut still = TabRow::default();
        show_part(ui, p, &mut still, &pinned, active, options, &mut outcome);
        show_part(ui, p, row, &rest, active, options, &mut outcome);
    } else {
        show_part(ui, p, row, &all, active, options, &mut outcome);
    }
    outcome
}

fn show_part(
    ui: &mut Ui,
    p: &IdePalette,
    row: &mut TabRow,
    entries: &[(usize, &Tab<'_>)],
    active: Option<usize>,
    options: Options,
    outcome: &mut Outcome,
) {
    let look = options.look;
    let vertical = look.vertical();
    let tabs: Vec<&Tab<'_>> = entries.iter().map(|(_, tab)| *tab).collect();
    let active = active.and_then(|active| entries.iter().position(|(index, _)| *index == active));
    let original = |at: usize| entries[at].0;
    let full = if vertical { ui.available_height() } else { part_height(ui, p, &tabs, ui.available_width(), options) };
    let (tabs_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), full), Sense::hover());
    if options.height <= 0.0 {
        return;
    }
    let measured = tab_widths(ui, p, &tabs, look);
    let flowing = look.rows == TabRows::Multiple && !vertical;
    let rows = if flowing { wrapped(&measured, tabs_rect.width()) } else { vec![0; measured.len()] };
    let spans: Vec<f32> = if vertical {
        vec![TAB_HEIGHT; measured.len()]
    } else if look.rows == TabRows::Squeeze {
        squeezed(&measured, tabs_rect.width())
    } else {
        measured
    };
    let extent = if vertical { tabs_rect.height() } else { tabs_rect.width() };
    let total = spans.iter().sum::<f32>() + GAP * spans.len().saturating_sub(1) as f32 + 16.0;
    let overflow = total > extent && !flowing;
    let list_button = if vertical {
        Rect::from_center_size(pos2(tabs_rect.center().x, tabs_rect.bottom() - 18.0), Vec2::splat(24.0))
    } else {
        Rect::from_center_size(pos2(tabs_rect.right() - 18.0, tabs_rect.center().y), Vec2::splat(24.0))
    };
    let strip = match (overflow, vertical) {
        (false, _) => tabs_rect,
        (true, false) => Rect::from_min_max(tabs_rect.min, pos2(list_button.left() - GAP, tabs_rect.bottom())),
        (true, true) => Rect::from_min_max(tabs_rect.min, pos2(tabs_rect.right(), list_button.top() - GAP)),
    };
    let room = if vertical { strip.height() } else { strip.width() };
    let max_scroll = if flowing { 0.0 } else { (total - room).max(0.0) };
    if ui.rect_contains_pointer(strip) && max_scroll > 0.0 {
        let delta = ui.input(|input| input.smooth_scroll_delta);
        let step = if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y };
        if step != 0.0 {
            row.scroll -= step;
            row.revealed_in.2 = 0.0;
            row.last_scroll = ui.input(|input| input.time);
            ui.ctx().input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    let shape = (room, tabs.len());
    if active != row.revealed || (shape.0 - row.revealed_in.0).abs() > 0.5 || shape.1 != row.revealed_in.1 {
        row.revealed = active;
        row.revealed_in = (shape.0, shape.1, 0.0);
        if let Some(index) = active.filter(|index| *index < spans.len()) {
            row.scroll = revealing(&spans, index, row.scroll, room);
            row.revealed_in.2 = row.scroll;
        }
    }
    row.scroll = row.scroll.clamp(0.0, max_scroll.max(row.revealed_in.2));
    let painter = ui.painter().with_clip_rect(strip);
    if flowing {
        row.scroll = 0.0;
    }
    let mut along = 8.0 - row.scroll;
    let mut current_row = 0;
    let mut placed = Vec::with_capacity(tabs.len());
    let mut dragged = None;
    let mut dropped = None;
    for (index, span) in spans.iter().enumerate() {
        if rows[index] != current_row {
            current_row = rows[index];
            along = 8.0;
        }
        let rect = if vertical {
            Rect::from_min_size(
                pos2(strip.left() + 4.0, strip.top() + along),
                vec2((strip.width() - 8.0).max(0.0), TAB_HEIGHT),
            )
        } else {
            let middle = tabs_rect.top() + options.height * (current_row as f32 + 0.5);
            Rect::from_min_size(pos2(strip.left() + along, middle - TAB_HEIGHT / 2.0), vec2(*span, TAB_HEIGHT))
        };
        along += span + GAP;
        placed.push(rect);
        outcome.places[original(index)] = Some(rect);
    }
    let gliding = !vertical && !flowing;
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let in_row = pointer.filter(|at| tabs_rect.expand2(vec2(0.0, 8.0)).contains(*at));
    let held = row.drag.and_then(|(id, grab)| entries.iter().position(|(index, _)| *index == id).map(|at| (at, grab)));
    let starts: Vec<f32> = placed.iter().map(|rect| rect.left() - strip.left() + row.scroll).collect();
    let followed = held.filter(|_| gliding).zip(in_row);
    let mut seen: Vec<&str> = Vec::with_capacity(tabs.len());
    let keys: Vec<u64> = tabs
        .iter()
        .map(|tab| {
            let twin = seen.iter().filter(|title| **title == tab.title.as_str()).count();
            seen.push(tab.title.as_str());
            tab_drag::key((tab.title.as_str(), twin))
        })
        .collect();
    let scope = ui.scope_id().with("tab-glide");
    let targets = match followed {
        Some(((at, grab), pointer)) => {
            let pinned: Vec<bool> = tabs.iter().map(|tab| tab.pinned).collect();
            let x = pointer.x - strip.left() + row.scroll;
            let place = tab_drag::drop_place(&starts, &spans, at, x, &pinned, row.place);
            row.place = Some(place);
            if ui.input(|input| input.pointer.primary_released()) {
                tab_drag::begin_settle(
                    ui,
                    scope,
                    &keys,
                    &tab_drag::drag_targets(&starts, &spans, GAP, at, place, None),
                );
            }
            tab_drag::drag_targets(&starts, &spans, GAP, at, place, Some(x - grab))
        }
        None => tab_drag::settled(ui, scope, &keys).unwrap_or_else(|| starts.clone()),
    };
    let follows = followed.map(|((at, _), _)| at);
    let mut shifts = vec![0.0; tabs.len()];
    let mut lifts = vec![0.0; tabs.len()];
    if gliding {
        for index in 0..tabs.len() {
            let id = scope.with(keys[index]);
            shifts[index] = tab_drag::glide(ui, id, targets[index], follows == Some(index)) - starts[index];
            lifts[index] = tab_drag::lift(ui, id.with("lift"), follows == Some(index));
        }
    }
    let order: Vec<usize> = (0..tabs.len()).filter(|index| Some(*index) != follows).chain(follows).collect();
    for index in order {
        let tab = tabs[index];
        let rect = placed[index].translate(vec2(shifts[index], 0.0));
        tab_drag::paint_lift(&painter, rect, p.popup_shadow, lifts[index]);
        let shown = rect.intersect(strip);
        if !shown.is_positive() {
            continue;
        }
        let is_active = Some(index) == active;
        let sense = if tab.draggable { Sense::click_and_drag() } else { Sense::click() };
        let id = original(index);
        let response = ui.interact(shown, ui.scope_id().with(("tab", id)), sense);
        accessibility::item(&response, egui::Role::Tab, is_active, &tab.title);
        if tab.dirty {
            accessibility::edit(&response, |node| node.set_description(t!("a11y-modified")));
        }
        if tab.draggable && response.drag_started() {
            let grab = response.interact_pointer_pos().map_or(0.0, |at| at.x - rect.left());
            row.drag = Some((id, grab));
            row.place = None;
        }
        if tab.draggable {
            if options.reorder {
                if response.dragged() {
                    dragged = Some(index);
                }
                if response.drag_stopped() {
                    dropped = Some((index, response.interact_pointer_pos()));
                }
            } else if response.drag_started() {
                outcome.drag_started = Some(id);
            }
        }
        let pointer_in = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| shown.contains(pointer));
        if is_active {
            painter.rect_filled(rect, 7, p.active_tab);
            if options.focused {
                painter.rect_stroke(rect, 7, Stroke::new(1.0, p.active_tab_border), StrokeKind::Inside);
            }
        } else if lifts[index] > 0.0 {
            painter.rect_filled(rect, 7, p.active_tab.gamma_multiply(lifts[index]));
        } else if pointer_in {
            painter.rect_filled(rect, 7, p.hover);
        }
        if let Some(color) = tab.strip {
            let bar = Rect::from_min_max(
                pos2(rect.left() + 6.0, rect.bottom() - 3.0),
                pos2(rect.right() - 6.0, rect.bottom() - 1.0),
            );
            painter.rect_filled(bar, 1, color);
        }
        if look.icons {
            let center = pos2(rect.left() + look.lead() + 16.0, rect.center().y);
            (tab.icon)(&painter, Rect::from_center_size(center, Vec2::splat(15.0)));
        }
        if let Some((color, meaning)) = &tab.mark {
            let dot = pos2(rect.left() + look.lead() + 22.0, rect.top() + 8.0);
            painter.circle_filled(dot, 3.5, p.active_tab);
            painter.circle_filled(dot, 2.5, *color);
            accessibility::edit(&response, |node| node.set_description(meaning.clone()));
        }
        let format = TextFormat {
            font_id: theme::regular(13.0),
            color: tab.color,
            italics: tab.preview,
            ..TextFormat::default()
        };
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(LayoutJob::simple_format(tab.title.clone(), format)));
        let title_at = pos2(rect.left() + look.title_at(), rect.center().y - galley.size().y / 2.0);
        let room_right = rect.right() - if look.close == CloseButton::Right { 24.0 } else { 6.0 };
        let title_clip = rect.with_min_x(title_at.x).with_max_x(room_right.max(title_at.x)).intersect(strip);
        painter.with_clip_rect(title_clip).galley(title_at, galley, tab.color);
        let button_at = match look.close {
            CloseButton::Left => pos2(rect.left() + CLOSE_SIZE / 2.0 + 4.0, rect.center().y),
            CloseButton::Right | CloseButton::None => pos2(rect.right() - 14.0, rect.center().y),
        };
        if tab.pinned {
            accessibility::edit(&response, |node| node.set_description(t!("a11y-pinned")));
            let pin_rect = Rect::from_center_size(button_at, Vec2::splat(CLOSE_SIZE));
            if strip.contains_rect(pin_rect) {
                let pin_response = ui.interact(pin_rect, ui.scope_id().with(("unpin", id)), Sense::click());
                accessibility::button_with(&pin_response, || t!("a11y-unpin-tab", name = tab.title.as_str()));
                let color = if pin_response.hovered() { p.text } else { p.text_secondary };
                crate::icons::pin(&painter, pin_rect.shrink(3.0), color);
                if pin_response.clicked() {
                    outcome.unpin = Some(id);
                }
            }
        } else if tab.closable && look.close != CloseButton::None && (is_active || pointer_in) {
            let close_rect = Rect::from_center_size(button_at, Vec2::splat(CLOSE_SIZE));
            if strip.contains_rect(close_rect) {
                let close_response = ui.interact(close_rect, ui.scope_id().with(("close", id)), Sense::click());
                accessibility::button_with(&close_response, || t!("a11y-close-tab", name = tab.title.as_str()));
                let c = close_rect.shrink(4.5);
                let color = if close_response.hovered() { p.text } else { p.text_secondary };
                painter.line_segment([c.left_top(), c.right_bottom()], Stroke::new(1.2, color));
                painter.line_segment([c.right_top(), c.left_bottom()], Stroke::new(1.2, color));
                if close_response.clicked() {
                    let alt = ui.input(|input| {
                        input.modifiers.alt
                            || input.events.iter().any(
                                |event| matches!(event, egui::Event::PointerButton { modifiers, .. } if modifiers.alt),
                            )
                    });
                    if alt {
                        outcome.close_others = Some(id);
                    } else {
                        outcome.close = Some(id);
                    }
                }
            }
        }
        if tab.closable && response.middle_clicked() {
            outcome.close = Some(id);
        }
        if response.clicked() && outcome.close.is_none() && outcome.close_others.is_none() && outcome.unpin.is_none() {
            outcome.select = Some(id);
        }
        if tab.preview && response.double_clicked() {
            outcome.keep_open = Some(id);
        }
        outcome.responses[id] = Some(response);
    }
    let off_row = |pointer: Option<Pos2>| pointer.filter(|at| !tabs_rect.expand2(vec2(0.0, 8.0)).contains(*at));
    if let Some((index, pointer)) = dropped {
        match off_row(pointer) {
            Some(at) => outcome.dropped = Some((original(index), at)),
            None => {
                let pinned: Vec<bool> = tabs.iter().map(|tab| tab.pinned).collect();
                let along = |at: Pos2| if vertical { at.y - strip.top() } else { at.x - strip.left() + row.scroll };
                let lead: Vec<f32> = placed
                    .iter()
                    .map(
                        |rect| {
                            if vertical { rect.top() - strip.top() } else { rect.left() - strip.left() + row.scroll }
                        },
                    )
                    .collect();
                let sizes: Vec<f32> =
                    placed.iter().map(|rect| if vertical { rect.height() } else { rect.width() }).collect();
                outcome.reorder = pointer
                    .map(|at| {
                        row.place
                            .unwrap_or_else(|| tab_drag::drop_place(&lead, &sizes, index, along(at), &pinned, None))
                    })
                    .filter(|to| *to != index)
                    .map(|to| (original(index), original(to)));
            }
        }
    }
    if !ui.input(|input| input.pointer.primary_down()) {
        row.drag = None;
    }
    if let Some(index) = dragged {
        if let Some(at) = off_row(pointer) {
            outcome.dragging = Some((original(index), at));
            drag_label(ui, p, at, &tabs[index].title);
        } else if !gliding && let Some(at) = pointer {
            drag_label(ui, p, at, &tabs[index].title);
        }
    }
    let edge = Stroke::new(1.0, p.frame);
    match look.placement {
        TabPlacement::Left => {
            ui.painter().vline(tabs_rect.right() - 0.5, tabs_rect.y_range(), edge);
        }
        TabPlacement::Right => {
            ui.painter().vline(tabs_rect.left() + 0.5, tabs_rect.y_range(), edge);
        }
        TabPlacement::Bottom => {
            ui.painter().hline(tabs_rect.x_range(), tabs_rect.top() + 0.5, edge);
        }
        TabPlacement::Top | TabPlacement::None => {
            ui.painter().hline(tabs_rect.x_range(), tabs_rect.bottom() - 0.5, edge);
        }
    }
    if overflow {
        let now = ui.input(|input| input.time);
        let hovered = ui.rect_contains_pointer(tabs_rect);
        let alpha = if hovered || now - row.last_scroll < 0.8 {
            1.0
        } else {
            (1.0 - (now - row.last_scroll - 0.8) / 0.3).clamp(0.0, 1.0) as f32
        };
        if alpha > 0.0 {
            let length = room * room / total;
            let start = (room - length) * (row.scroll / max_scroll.max(1.0)).min(1.0);
            let bar = if vertical {
                Rect::from_min_size(pos2(tabs_rect.right() - 4.0, strip.top() + start), vec2(3.0, length))
            } else {
                Rect::from_min_size(pos2(strip.left() + start, tabs_rect.bottom() - 4.0), vec2(length, 3.0))
            };
            painter.rect_filled(bar, 2, p.text_secondary.gamma_multiply(0.45 * alpha));
            if alpha < 1.0 {
                ui.ctx().request_repaint();
            }
        }
        let response = ui.interact(list_button, ui.scope_id().with("tab-list"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(list_button, 5, p.hover);
        }
        crate::icons::chevron_down(ui.painter(), list_button.shrink(7.0), p.text_secondary);
        hint(ui, &response, &t!("ide-tabs-all"), None, Side::Below);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(220.0);
            for (index, tab) in tabs.iter().enumerate() {
                let text = if tab.dirty { format!("{} •", tab.title) } else { tab.title.clone() };
                if ui.selectable_label(Some(index) == active, text).clicked() {
                    outcome.select = Some(original(index));
                    ui.close();
                }
            }
        });
    }
}

pub(super) fn revealing(widths: &[f32], index: usize, scroll: f32, width: f32) -> f32 {
    let starts: Vec<f32> = widths
        .iter()
        .scan(0.0, |at, tab| {
            let start = *at;
            *at += tab + GAP;
            Some(start)
        })
        .collect();
    let start = starts[index];
    let end = start + widths[index] + 16.0;
    let wanted = if start < scroll {
        start
    } else if end > scroll + width {
        end - width
    } else {
        scroll
    };
    if wanted <= 0.0 {
        return 0.0;
    }
    starts.iter().copied().find(|tab| *tab >= wanted - 0.5).filter(|tab| *tab <= start).unwrap_or(start)
}

fn drag_label(ui: &Ui, p: &IdePalette, pointer: Pos2, title: &str) {
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, ui.scope_id().with("dragged-inner-tab")));
    let galley = painter.layout_no_wrap(title.to_owned(), theme::regular(13.0), p.text);
    let label = Rect::from_min_size(pointer + vec2(12.0, 8.0), galley.size() + vec2(20.0, 10.0));
    painter.rect_filled(label, 6, p.active_tab);
    painter.rect_stroke(label, 6, Stroke::new(1.0, p.active_tab_border), StrokeKind::Inside);
    painter.galley(label.min + vec2(10.0, 5.0), galley, p.text);
}

#[cfg(test)]
mod tests {
    use super::{LEAST_TAB, revealing, squeezed, wrapped};

    #[test]
    fn tabs_wrap_to_the_next_row_when_the_row_is_full() {
        assert_eq!(wrapped(&[100.0, 100.0, 100.0], 240.0), [0, 0, 1]);
        assert_eq!(wrapped(&[300.0, 100.0], 240.0), [0, 1], "a tab wider than the row still gets a row");
        assert_eq!(wrapped(&[], 240.0), [0usize; 0]);
    }

    #[test]
    fn squeezed_tabs_fit_the_row_down_to_the_narrowest_tab() {
        assert_eq!(squeezed(&[100.0, 100.0], 400.0), [100.0, 100.0], "tabs that fit stay as they are");
        let narrow = squeezed(&[200.0, 200.0, 200.0], 400.0);
        let total = narrow.iter().sum::<f32>() + 8.0 + 16.0;
        assert!((total - 400.0).abs() < 0.5, "{total}");
        assert_eq!(squeezed(&[200.0; 3], 100.0), [LEAST_TAB; 3]);
    }

    #[test]
    fn the_active_tab_comes_into_view_and_no_tab_is_cut_at_the_left() {
        let widths = [100.0, 100.0, 100.0, 100.0, 100.0];
        assert_eq!(revealing(&widths, 0, 250.0, 300.0), 0.0, "the first tab is shown whole");
        assert_eq!(revealing(&widths, 1, 0.0, 300.0), 0.0, "a tab in view keeps the scroll");
        let scroll = revealing(&widths, 4, 0.0, 300.0);
        assert_eq!(scroll, 312.0, "the row starts at a tab, not in the middle of one");
        assert!(scroll + 300.0 >= 416.0 + 100.0, "the active tab ends inside the row");
        assert_eq!(revealing(&widths, 2, 400.0, 300.0), 208.0, "a tab left of the view scrolls back to its start");
    }
}
