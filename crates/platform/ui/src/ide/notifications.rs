use std::collections::BTreeSet;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::notifications::{Group, Kind, NewNotification, Notification, Notifications, RUN_ACTION};

use super::{IdePalette, IdeView, Side, glyphs::*, hint, hover, metrics};
use crate::theme;

pub(super) const STAY: f64 = 10.0;
pub(super) const FADE: f64 = 0.6;
pub(super) const MAX_BALLOONS: usize = 3;
const BALLOON_WIDTH: f32 = 380.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Balloon {
    pub(super) id: u64,
    shown: f64,
    sticky: bool,
}

impl Balloon {
    pub(super) fn opacity(&self, now: f64) -> Option<f32> {
        let since = now - self.shown;
        if self.sticky || since <= STAY {
            return Some(1.0);
        }
        let left = 1.0 - (since - STAY) / FADE;
        (left > 0.0).then_some(left as f32)
    }
}

#[derive(Debug, Default)]
pub(super) struct Balloons {
    shown: Vec<Balloon>,
    seen: u64,
}

impl Balloons {
    pub(super) fn follow(&mut self, store: &Notifications, now: f64, wanted: impl Fn(&Notification) -> bool) {
        if store.last_id() == self.seen {
            return;
        }
        let mut new: Vec<Balloon> = store
            .newest_first()
            .take_while(|item| item.id > self.seen)
            .filter(|item| item.balloon && wanted(item))
            .map(|item| Balloon { id: item.id, shown: now, sticky: item.sticky || item.kind == Kind::Error })
            .collect();
        new.reverse();
        self.shown.extend(new);
        self.seen = store.last_id();
    }

    pub(super) fn visible(&mut self, store: &Notifications, now: f64) -> Vec<(u64, f32)> {
        self.shown
            .retain(|balloon| balloon.opacity(now).is_some() && store.get(balloon.id).is_some_and(|item| !item.read));
        let skip = self.shown.len().saturating_sub(MAX_BALLOONS);
        self.shown[skip..].iter().filter_map(|balloon| Some((balloon.id, balloon.opacity(now)?))).collect()
    }

    pub(super) fn next_change(&self, now: f64) -> Option<f64> {
        self.shown
            .iter()
            .filter(|balloon| !balloon.sticky)
            .map(|balloon| (STAY - (now - balloon.shown)).max(0.0))
            .min_by(f64::total_cmp)
    }

    pub(super) fn hold(&mut self, id: u64, now: f64) {
        if let Some(balloon) = self.shown.iter_mut().find(|balloon| balloon.id == id) {
            balloon.shown = now;
        }
    }

    pub(super) fn close(&mut self, id: u64) {
        self.shown.retain(|balloon| balloon.id != id);
    }

    pub(super) fn close_all(&mut self) {
        self.shown.clear();
    }

    #[cfg(test)]
    pub(super) fn any(&self) -> bool {
        !self.shown.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Click {
    Action(u64, String),
    Remove(u64),
    ClearAll,
    Dismiss(u64),
    Hide,
}

#[derive(Default)]
pub(super) struct NotificationsUi {
    pub(super) badge: Option<bool>,
    balloons: Balloons,
    pending: Vec<(NewNotification, u64)>,
    posting: Option<Group>,
    logged: BTreeSet<String>,
    fresh: BTreeSet<u64>,
    hits: Vec<(u64, String, Rect)>,
    balloon_hits: Vec<(u64, String, Rect)>,
}

impl NotificationsUi {
    pub(super) fn queue(&mut self, new: NewNotification) {
        self.pending.push((new, zen_workbench::notifications::now_ms()));
    }

    pub(super) fn queue_once(&mut self, key: String, new: NewNotification) {
        if self.logged.insert(key) {
            self.queue(new);
        }
    }

    pub(super) fn group(&self) -> Group {
        self.posting.unwrap_or_default()
    }

    pub(super) fn set_posting(&mut self, group: Option<Group>) -> Option<Group> {
        std::mem::replace(&mut self.posting, group)
    }

    pub(super) fn flush(&mut self, session: &mut Session) {
        if self.pending.is_empty() {
            return;
        }
        let store = session.state_mut::<Notifications>();
        for (new, time) in self.pending.drain(..) {
            store.add(new, time);
        }
    }

    #[cfg(test)]
    pub(super) fn hit(&self, id: u64, name: &str) -> Option<Rect> {
        self.hits
            .iter()
            .chain(&self.balloon_hits)
            .find(|(shown, hit, _)| *shown == id && hit == name)
            .map(|(_, _, rect)| *rect)
    }

    #[cfg(test)]
    pub(super) fn balloons(&mut self) -> &mut Balloons {
        &mut self.balloons
    }

    pub(super) fn tool_window(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        store: &Notifications,
        now: u64,
        offset: i32,
    ) -> Option<Click> {
        self.hits.clear();
        for item in store.newest_first().filter(|item| !item.read) {
            self.fresh.insert(item.id);
        }
        let mut click = None;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-notifications"),
            theme::semibold(13.0),
            p.text,
        );
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("notifications-hide"), Sense::click());
        hover(ui, &response, p);
        hint(ui, &response, &t!("ide-notifications-hide"), None, Side::Left);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        if response.clicked() {
            click = Some(Click::Hide);
        }
        if !store.is_empty() {
            let label = t!("ide-notifications-clear-all");
            let galley = ui.painter().layout_no_wrap(label, theme::regular(13.0), p.link);
            let rect = Rect::from_min_size(
                pos2(hide.left() - 12.0 - galley.size().x, header.center().y - galley.size().y / 2.0),
                galley.size(),
            );
            if link(ui, p, rect, galley, "notifications-clear-all") {
                click = Some(Click::ClearAll);
            }
            self.hits.push((0, "clearAll".to_owned(), rect));
        }
        if store.is_empty() {
            let rest = ui.available_rect_before_wrap();
            ui.painter().text(
                rest.center_top() + vec2(0.0, 40.0),
                Align2::CENTER_CENTER,
                t!("ide-notifications-empty"),
                theme::regular(13.0),
                p.text_secondary,
            );
            return click;
        }
        let mut hits = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for item in store.newest_first() {
                let fresh = self.fresh.contains(&item.id);
                if let Some(chosen) = card(ui, p, item, fresh, &time_label(item.time, now, offset), &mut hits) {
                    click = Some(chosen);
                }
            }
        });
        self.hits.extend(hits);
        click
    }

    pub(super) fn closed(&mut self) {
        self.fresh.clear();
    }

    pub(super) fn draw_balloons(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        store: &Notifications,
        now: f64,
    ) -> Option<Click> {
        self.balloon_hits.clear();
        let shown = self.balloons.visible(store, now);
        if shown.is_empty() {
            return None;
        }
        match self.balloons.next_change(now) {
            Some(wait) if wait > 0.0 => ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait)),
            Some(_) => ui.ctx().request_repaint(),
            None => {}
        }
        let screen = ui.ctx().content_rect();
        let mut bottom = screen.bottom() - metrics::STATUS_BAR - 12.0;
        let mut click = None;
        let mut hovered = Vec::new();
        let mut hits = Vec::new();
        for (id, opacity) in shown.into_iter().rev() {
            let Some(item) = store.get(id) else { continue };
            let area = egui::Area::new(ui.scope_id().with(("notification-balloon", id)))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(screen.right() - BALLOON_WIDTH - 56.0, bottom))
                .pivot(Align2::LEFT_BOTTOM)
                .show(ui.ctx(), |ui| {
                    ui.set_opacity(opacity);
                    p.popup_frame().corner_radius(10).inner_margin(14).show(ui, |ui| {
                        ui.set_width(BALLOON_WIDTH - 28.0);
                        let chosen = body(ui, p, item, true, &mut hits);
                        if chosen.is_some() {
                            click = chosen;
                        }
                        let read = ui.min_rect();
                        crate::accessibility::announce(ui, ui.scope_id().with("notification-read"), read, || {
                            [item.title.as_str(), item.body.as_str()]
                                .into_iter()
                                .filter(|text| !text.is_empty())
                                .collect::<Vec<_>>()
                                .join(". ")
                        });
                    });
                });
            if area.response.contains_pointer() {
                hovered.push(id);
            }
            bottom = area.response.rect.top() - 8.0;
        }
        for id in hovered {
            self.balloons.hold(id, now);
        }
        self.balloon_hits = hits;
        click
    }
}

fn link(ui: &Ui, p: &IdePalette, rect: Rect, galley: std::sync::Arc<egui::Galley>, salt: &str) -> bool {
    let response =
        ui.interact(rect, ui.scope_id().with(salt), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, p.link));
    }
    ui.painter().galley(rect.min, galley, p.link);
    response.clicked()
}

fn card(
    ui: &mut Ui,
    p: &IdePalette,
    item: &Notification,
    fresh: bool,
    time: &str,
    hits: &mut Vec<(u64, String, Rect)>,
) -> Option<Click> {
    let mut click = None;
    let frame = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 10 });
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let top = ui.cursor().min;
        let right = ui.max_rect().right();
        let stamp = ui.painter().text(
            pos2(right - 26.0, top.y + 8.0),
            Align2::RIGHT_CENTER,
            time,
            theme::regular(12.0),
            p.text_secondary,
        );
        if fresh {
            ui.painter().circle_filled(pos2(stamp.left() - 8.0, stamp.center().y), 3.0, p.accent);
        }
        click = body(ui, p, item, false, hits);
    });
    let rect = shown.response.rect;
    let close = Rect::from_center_size(pos2(rect.right() - 16.0, rect.top() + 18.0), Vec2::splat(18.0));
    let response = ui.interact(close, ui.scope_id().with(("notification-close", item.id)), Sense::click());
    if ui.rect_contains_pointer(rect) {
        hover(ui, &response, p);
        glyph_close(ui.painter(), close.shrink(5.0), p.text_secondary);
    }
    hint(ui, &response, &t!("ide-notifications-remove"), None, Side::Left);
    hits.push((item.id, "close".to_owned(), close));
    if response.clicked() {
        click = Some(Click::Remove(item.id));
    }
    ui.painter().hline(rect.x_range().shrink(8.0), rect.bottom(), Stroke::new(1.0, p.tab_border));
    click
}

fn body(
    ui: &mut Ui,
    p: &IdePalette,
    item: &Notification,
    balloon: bool,
    hits: &mut Vec<(u64, String, Rect)>,
) -> Option<Click> {
    let mut click = None;
    ui.horizontal_top(|ui| {
        let (mark, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        kind_mark(ui, p, mark, item.kind);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - if balloon { 22.0 } else { 70.0 });
            ui.spacing_mut().item_spacing.y = 4.0;
            let group = group_name(item.group);
            ui.label(egui::RichText::new(group).font(theme::regular(11.5)).color(p.text_secondary));
            if !item.title.is_empty() {
                ui.label(egui::RichText::new(&item.title).font(theme::semibold(13.0)).color(p.text));
            }
            if !item.body.is_empty() {
                let color = if item.title.is_empty() { p.text } else { p.text_secondary };
                ui.add(
                    egui::Label::new(egui::RichText::new(&item.body).font(theme::regular(13.0)).color(color)).wrap(),
                );
            }
            if !item.actions.is_empty() {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for action in &item.actions {
                        let response = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(&action.label).font(theme::regular(13.0)).color(p.link),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.hovered() {
                            ui.painter().line_segment(
                                [response.rect.left_bottom(), response.rect.right_bottom()],
                                Stroke::new(1.0, p.link),
                            );
                        }
                        hits.push((item.id, action.id.clone(), response.rect));
                        if response.clicked() {
                            click = Some(Click::Action(item.id, action.id.clone()));
                        }
                    }
                });
            }
        });
    });
    if balloon {
        let rect = ui.min_rect();
        let close = Rect::from_center_size(pos2(rect.right() - 6.0, rect.top() + 6.0), Vec2::splat(18.0));
        let response = ui.interact(close, ui.scope_id().with(("balloon-close", item.id)), Sense::click());
        hover(ui, &response, p);
        glyph_close(ui.painter(), close.shrink(5.0), p.text_secondary);
        hits.push((item.id, "dismiss".to_owned(), close));
        if response.clicked() {
            click = Some(Click::Dismiss(item.id));
        }
    }
    click
}

fn kind_mark(ui: &Ui, p: &IdePalette, rect: Rect, kind: Kind) {
    let (color, sign) = kind_style(p, kind);
    ui.painter().circle_filled(rect.center(), rect.width() * 0.42, color);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, sign, theme::semibold(11.0), p.accent_text);
}

pub(super) fn kind_style(p: &IdePalette, kind: Kind) -> (Color32, &'static str) {
    match kind {
        Kind::Info => (p.diagnostic_info, "i"),
        Kind::Warning => (p.diagnostic_warning, "!"),
        Kind::Error => (p.diagnostic_error, "×"),
    }
}

fn glyph_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
}

pub(super) fn group_name(group: Group) -> String {
    t!(&format!("ide-notifications-group-{}", group_key(group)))
}

fn group_key(group: Group) -> &'static str {
    match group {
        Group::General => "general",
        Group::Updates => "updates",
        Group::Plugins => "plugins",
        Group::LanguageServers => "language-servers",
        Group::Debugger => "debugger",
        Group::Git => "git",
        Group::Errors => "errors",
        Group::PowerSave => "power-save",
    }
}

pub(super) fn time_label(time: u64, now: u64, offset: i32) -> String {
    let local = |ms: u64| i64::try_from(ms / 1000).unwrap_or(i64::MAX) + i64::from(offset);
    let (when, today) = (local(time), local(now));
    let (day, current) = (when.div_euclid(86_400), today.div_euclid(86_400));
    let minutes = when.rem_euclid(86_400) / 60;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    if day == current {
        return clock;
    }
    if day == current - 1 {
        return t!("ide-notifications-yesterday", time = clock.as_str());
    }
    let (year, month, date) = civil(day);
    format!("{date:02}.{month:02}.{year:04} {clock}")
}

fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

impl IdeView {
    pub(super) fn notify(&mut self, new: NewNotification) {
        self.notifications.queue(new);
    }

    pub(super) fn warn(&mut self, message: String) {
        self.notify(NewNotification::message(Group::General, Kind::Warning, message));
    }

    pub(super) fn as_git<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.with_group(Some(Group::Git), f)
    }

    pub(super) fn with_group<R>(&mut self, group: Option<Group>, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.notifications.set_posting(group);
        let result = f(self);
        self.notifications.set_posting(before);
        result
    }

    pub(super) fn follow_notifications(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        self.notifications.flush(session);
        let queued = session.state_mut::<Notifications>().take_queued();
        for (command, params) in queued {
            self.run_action(ui, registry, session, &command, params);
        }
        self.notifications.flush(session);
        let now = ui.input(|input| input.time);
        let settings = &self.overview.settings;
        let open = self.tool_visible("notifications");
        let store = session.state_mut::<Notifications>();
        self.notifications.badge = (store.unread() > 0).then(|| store.unread_error());
        self.notifications.balloons.follow(store, now, |item| {
            settings.notification_balloons && !open && !settings.balloon_groups_off.contains(item.group.id())
        });
    }

    pub(super) fn notification_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        params: serde_json::Value,
        agent: bool,
    ) -> Result<serde_json::Value, super::agent::ActionError> {
        let mut answer = registry.dispatch(session, RUN_ACTION, params).map_err(super::agent::ActionError::Dispatch)?;
        let mut result = serde_json::Value::Null;
        for (command, params) in session.state_mut::<Notifications>().take_queued() {
            let group = Group::of_command(&command);
            result =
                self.with_group(group, |view| view.perform_action(ctx, registry, session, &command, params, agent))?;
        }
        answer["result"] = result;
        Ok(answer)
    }

    pub(super) fn toggle_notifications(&mut self) {
        self.toggle_tool("notifications");
        if self.tool_visible("notifications") {
            self.notifications.balloons.close_all();
        } else {
            self.notifications.closed();
        }
    }

    pub(super) fn notifications_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = zen_workbench::notifications::now_ms();
        let offset = zen_vcs::local_offset();
        let store = session.state_mut::<Notifications>();
        let click = self.notifications.tool_window(ui, &p, store, now, offset);
        if store.unread() > 0 {
            self.dispatch(ui, registry, session, "ui.notifications.markRead", json!({}));
        }
        self.notification_click(ui, registry, session, click);
    }

    pub(super) fn notification_balloons(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        let store = session.state_mut::<Notifications>();
        let click = self.notifications.draw_balloons(ui, &p, store, now);
        self.notification_click(ui, registry, session, click);
    }

    fn notification_click(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, click: Option<Click>) {
        match click {
            Some(Click::Action(id, action)) => {
                let params = json!({ "id": id, "action": action });
                match self.notification_action(ui.ctx(), registry, session, params, false) {
                    Ok(_) => {}
                    Err(super::agent::ActionError::Dispatch(error)) => self.report(ui, error),
                    Err(super::agent::ActionError::Refused(reason)) => self.post(ui, reason),
                }
            }
            Some(Click::Remove(id)) => {
                self.dispatch(ui, registry, session, "ui.notifications.clear", json!({ "ids": [id] }));
            }
            Some(Click::ClearAll) => self.dispatch(ui, registry, session, "ui.notifications.clear", json!({})),
            Some(Click::Dismiss(id)) => self.notifications.balloons.close(id),
            Some(Click::Hide) => self.toggle_notifications(),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
