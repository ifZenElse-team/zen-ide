use egui::Event;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Idle {
    Due,
    In(f64),
    Waiting,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AutosaveClock {
    last_input: f64,
    inputs: u64,
    last_change: f64,
    modified: Option<u64>,
    attempted: Option<(u64, u64)>,
}

impl AutosaveClock {
    pub(super) fn input(&mut self, now: f64) {
        self.last_input = now;
        self.inputs += 1;
    }

    pub(super) fn idle(&mut self, now: f64, seconds: f64, modified: Option<u64>) -> Idle {
        if modified != self.modified {
            self.modified = modified;
            self.last_change = now;
        }
        let Some(fingerprint) = modified else { return Idle::Waiting };
        if self.attempted.is_some_and(|(tried, input)| tried == fingerprint && input == self.inputs) {
            return Idle::Waiting;
        }
        let left = self.last_input.max(self.last_change) + seconds - now;
        if left > 0.0 {
            return Idle::In(left);
        }
        self.attempted = Some((fingerprint, self.inputs));
        Idle::Due
    }

    pub(super) fn saved(&mut self, modified: Option<u64>) {
        self.attempted = modified.map(|fingerprint| (fingerprint, self.inputs));
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Leaving {
    asked: bool,
    since: Option<f64>,
}

impl Leaving {
    const SETTLE: f64 = 0.2;

    pub(super) fn left(&mut self) {
        self.asked = true;
    }

    pub(super) fn poll(&mut self, now: f64, focused: bool) -> Idle {
        if !self.asked {
            return Idle::Waiting;
        }
        if focused {
            *self = Self::default();
            return Idle::Waiting;
        }
        let left = *self.since.get_or_insert(now) + Self::SETTLE - now;
        if left > 0.0 {
            return Idle::In(left);
        }
        *self = Self::default();
        Idle::Due
    }
}

pub(super) fn is_input(event: &Event) -> bool {
    matches!(
        event,
        Event::Copy
            | Event::Cut
            | Event::Paste(_)
            | Event::PasteImage(_)
            | Event::MiddleClickPaste { .. }
            | Event::Text(_)
            | Event::Key { .. }
            | Event::PointerMoved(_)
            | Event::MouseMoved(_)
            | Event::PointerButton { .. }
            | Event::Zoom(_)
            | Event::Rotate(_)
            | Event::Ime(_)
            | Event::Touch { .. }
            | Event::MouseWheel { .. }
            | Event::AccessKitActionRequest(_)
    )
}

pub(super) fn window_left(events: &[Event]) -> bool {
    events.iter().any(|event| matches!(event, Event::WindowFocused(false)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_counts_once_the_focus_stayed_away_from_every_window() {
        let mut leaving = Leaving::default();
        assert_eq!(leaving.poll(0.0, false), Idle::Waiting, "nothing was left");
        leaving.left();
        assert_eq!(leaving.poll(0.0, false), Idle::In(Leaving::SETTLE));
        assert_eq!(leaving.poll(Leaving::SETTLE, false), Idle::Due);
        assert_eq!(leaving.poll(2.0, false), Idle::Waiting, "asked once");
        leaving.left();
        assert_eq!(leaving.poll(0.0, false), Idle::In(Leaving::SETTLE));
        assert_eq!(leaving.poll(0.1, true), Idle::Waiting, "another window of the IDE took the focus");
        assert_eq!(leaving.poll(5.0, false), Idle::Waiting);
    }

    #[test]
    fn the_idle_autosave_waits_for_quiet_after_the_last_input_and_change() {
        let mut clock = AutosaveClock::default();
        assert_eq!(clock.idle(0.0, 15.0, None), Idle::Waiting, "nothing modified");
        assert_eq!(clock.idle(1.0, 15.0, Some(7)), Idle::In(15.0), "the change starts the time");
        clock.input(10.0);
        assert_eq!(clock.idle(12.0, 15.0, Some(7)), Idle::In(13.0), "input starts it again");
        assert_eq!(clock.idle(25.0, 15.0, Some(7)), Idle::Due);
        assert_eq!(clock.idle(26.0, 15.0, Some(7)), Idle::Waiting, "tried once");
        assert_eq!(clock.idle(60.0, 15.0, Some(7)), Idle::Waiting, "no new input, no second try");
        clock.input(70.0);
        assert_eq!(clock.idle(71.0, 15.0, Some(7)), Idle::In(14.0), "new input starts another idle time");
        assert_eq!(clock.idle(85.0, 15.0, Some(7)), Idle::Due, "files left unsaved are tried again after it");
        assert_eq!(clock.idle(86.0, 15.0, Some(7)), Idle::Waiting);
        assert_eq!(clock.idle(91.0, 15.0, Some(8)), Idle::In(15.0), "a change starts it too");
        assert_eq!(clock.idle(106.0, 15.0, Some(8)), Idle::Due);
        assert_eq!(clock.idle(107.0, 15.0, None), Idle::Waiting);
    }

    #[test]
    fn an_autosave_for_another_reason_counts_as_tried() {
        let mut clock = AutosaveClock::default();
        assert_eq!(clock.idle(0.0, 15.0, Some(3)), Idle::In(15.0));
        clock.saved(Some(3));
        assert_eq!(clock.idle(20.0, 15.0, Some(3)), Idle::Waiting);
    }

    #[test]
    fn input_is_what_the_person_does_and_leaving_is_losing_the_keyboard() {
        assert!(is_input(&Event::Text("a".to_owned())));
        assert!(is_input(&Event::PointerMoved(egui::pos2(1.0, 2.0))));
        assert!(!is_input(&Event::WindowFocused(false)));
        assert!(!is_input(&Event::PointerGone));
        assert!(window_left(&[Event::PointerGone, Event::WindowFocused(false)]));
        assert!(!window_left(&[Event::WindowFocused(true)]));
    }
}
