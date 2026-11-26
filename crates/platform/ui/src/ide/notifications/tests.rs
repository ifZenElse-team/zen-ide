use egui::{Event, PointerButton, Pos2};
use zen_workbench::notifications::Action;

use super::*;

fn store_with(kinds: &[Kind]) -> Notifications {
    let mut store = Notifications::default();
    for (index, kind) in kinds.iter().enumerate() {
        store.add(NewNotification::message(Group::General, *kind, format!("note {index}")), 0);
    }
    store
}

#[test]
fn an_information_balloon_stays_then_fades_and_goes() {
    let store = store_with(&[Kind::Info]);
    let mut balloons = Balloons::default();
    balloons.follow(&store, 100.0, |_| true);
    assert_eq!(balloons.visible(&store, 100.0), [(1, 1.0)]);
    assert_eq!(balloons.visible(&store, 100.0 + STAY), [(1, 1.0)]);
    let fading = balloons.visible(&store, 100.0 + STAY + FADE / 2.0);
    assert!(fading[0].1 > 0.4 && fading[0].1 < 0.6, "{fading:?}");
    assert!(balloons.visible(&store, 100.0 + STAY + FADE + 0.01).is_empty());
    assert!(!balloons.any(), "a gone balloon is forgotten");
}

#[test]
fn an_error_balloon_stays_until_closed() {
    let store = store_with(&[Kind::Error, Kind::Warning]);
    let mut balloons = Balloons::default();
    balloons.follow(&store, 0.0, |_| true);
    let later = balloons.visible(&store, 3600.0);
    assert_eq!(later, [(1, 1.0)], "the warning faded, the error stays");
    balloons.close(1);
    assert!(balloons.visible(&store, 3600.0).is_empty());
}

#[test]
fn balloons_ask_for_a_repaint_only_when_one_starts_or_goes_on_fading() {
    let mut store = store_with(&[Kind::Error]);
    let mut balloons = Balloons::default();
    balloons.follow(&store, 0.0, |_| true);
    assert_eq!(balloons.next_change(5.0), None, "an error's balloon never fades");
    store.add(NewNotification::message(Group::General, Kind::Info, "saved"), 0);
    balloons.follow(&store, 4.0, |_| true);
    assert_eq!(balloons.next_change(6.0), Some(STAY - 2.0), "the next repaint is when it starts to fade");
    assert_eq!(balloons.next_change(4.0 + STAY + FADE / 2.0), Some(0.0), "every frame while it fades");
    balloons.visible(&store, 4.0 + STAY + FADE + 1.0);
    assert_eq!(balloons.next_change(4.0 + STAY + FADE + 1.0), None, "nothing once it went");
    let seen = store.last_id();
    balloons.follow(&store, 100.0, |_| panic!("nothing new is looked at"));
    assert_eq!(store.last_id(), seen);
}

#[test]
fn hovering_keeps_a_balloon_and_reading_closes_it() {
    let mut store = store_with(&[Kind::Info, Kind::Info]);
    let mut balloons = Balloons::default();
    balloons.follow(&store, 0.0, |_| true);
    balloons.hold(1, STAY);
    let shown = balloons.visible(&store, STAY + FADE + 1.0);
    assert_eq!(shown, [(1, 1.0)], "the held one stays, the other went");
    store.mark_read(Some(&[1]));
    assert!(balloons.visible(&store, STAY + FADE + 1.0).is_empty(), "read in the tool window or by an agent");
}

#[test]
fn only_new_wanted_notifications_come_up_and_at_most_three_show() {
    let mut store = store_with(&[Kind::Info]);
    let mut balloons = Balloons::default();
    balloons.follow(&store, 0.0, |_| true);
    balloons.follow(&store, 1.0, |_| true);
    assert_eq!(balloons.visible(&store, 1.0).len(), 1, "a notification comes up once");
    store.add(NewNotification::message(Group::Git, Kind::Info, "pushed"), 0);
    store.add(NewNotification::message(Group::General, Kind::Info, "silent").silent(), 0);
    balloons.follow(&store, 2.0, |item| item.group != Group::Git);
    assert_eq!(balloons.visible(&store, 2.0).len(), 1, "Git's balloons are off, the silent one is only listed");
    for n in 0..4 {
        store.add(NewNotification::message(Group::General, Kind::Info, format!("more {n}")), 0);
    }
    balloons.follow(&store, 3.0, |_| true);
    let shown: Vec<u64> = balloons.visible(&store, 3.0).into_iter().map(|(id, _)| id).collect();
    assert_eq!(shown, [5, 6, 7], "the newest three, the oldest first");
    balloons.close_all();
    assert!(!balloons.any());
}

#[test]
fn times_read_as_today_yesterday_or_a_date() {
    zen_i18n::register(crate::toolbox::LOCALES);
    let day = 86_400_000;
    let now = 20_000 * day + 15 * 3_600_000;
    assert_eq!(time_label(now - 3_600_000, now, 0), "14:00");
    assert_eq!(time_label(now - 3_600_000, now, 3 * 3600), "17:00", "in the local time zone");
    assert_eq!(time_label(now - day, now, 0), t!("ide-notifications-yesterday", time = "15:00"));
    assert_eq!(time_label(0, now, 0), "01.01.1970 00:00");
    assert_eq!(time_label(1_792_368_000_000, now, 0), "19.10.2026 00:00");
}

struct Painted {
    ctx: egui::Context,
    ui: NotificationsUi,
    store: Notifications,
    clicks: Vec<Click>,
}

impl Painted {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut store = Notifications::default();
        store.add(NewNotification::message(Group::General, Kind::Info, "Saved"), 1_000);
        store.add(
            NewNotification::message(Group::Debugger, Kind::Warning, "Go code is debugged with Delve.")
                .titled("Delve is not installed")
                .action(Action::new("install", "Install Delve".to_owned(), "view.debugger.install", json!({}))),
            2_000,
        );
        Self { ctx, ui: NotificationsUi::default(), store, clicks: Vec::new() }
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 600.0))),
            ..Default::default()
        };
        let Self { ctx, ui: notifications, store, clicks } = self;
        let mut output = ctx.run_ui(input, |ui| {
            clicks.extend(notifications.tool_window(ui, &IdePalette::zen_dark(), store, 3_000, 0));
        });
        output.textures_delta.clear();
    }

    fn click(&mut self, at: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
    }
}

#[test]
fn the_tool_window_lists_the_newest_first_with_links_that_run_actions() {
    let mut painted = Painted::new();
    painted.frame(Vec::new());
    let install = painted.ui.hit(2, "install").expect("the action's link");
    let first_close = painted.ui.hit(1, "close").expect("the older one's ×");
    let second_close = painted.ui.hit(2, "close").expect("the newer one's ×");
    assert!(second_close.top() < first_close.top(), "the newest is on top");
    assert!(painted.ui.hit(0, "clearAll").is_some());
    painted.click(install.center());
    assert_eq!(painted.clicks, [Click::Action(2, "install".to_owned())]);
    painted.clicks.clear();
    painted.click(first_close.center());
    assert_eq!(painted.clicks, [Click::Remove(1)]);
    painted.clicks.clear();
    let clear = painted.ui.hit(0, "clearAll").unwrap();
    painted.click(clear.center());
    assert_eq!(painted.clicks, [Click::ClearAll]);
}

#[test]
fn the_tool_window_marks_what_was_new_when_it_opened_and_shows_an_empty_state() {
    let mut painted = Painted::new();
    painted.frame(Vec::new());
    assert_eq!(painted.ui.fresh.len(), 2, "both were unread when shown");
    painted.store.mark_read(None);
    painted.frame(Vec::new());
    assert_eq!(painted.ui.fresh.len(), 2, "they stay marked while the window is open");
    painted.ui.closed();
    assert!(painted.ui.fresh.is_empty());
    painted.store.remove(None);
    painted.frame(Vec::new());
    assert!(painted.ui.hit(0, "clearAll").is_none(), "nothing to clear");
    assert!(painted.ui.hits.is_empty());
}

#[test]
fn the_balloons_draw_with_their_links_and_close_button() {
    let mut painted = Painted::new();
    painted.ui.balloons().follow(&painted.store, 0.0, |_| true);
    let mut click = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
        ..Default::default()
    };
    let Painted { ctx, ui: notifications, store, .. } = &mut painted;
    let mut output = ctx.run_ui(input, |ui| {
        click = notifications.draw_balloons(ui, &IdePalette::zen_dark(), store, 1.0);
    });
    output.textures_delta.clear();
    assert_eq!(click, None);
    let install = painted.ui.hit(2, "install").expect("a balloon's action");
    let dismiss = painted.ui.hit(2, "dismiss").expect("a balloon's ×");
    let older = painted.ui.hit(1, "dismiss").expect("the older balloon");
    assert!(older.top() < dismiss.top(), "the newest balloon is at the bottom");
    assert!(install.right() > 500.0 && install.bottom() < 700.0 - metrics::STATUS_BAR, "bottom right: {install:?}");
}
