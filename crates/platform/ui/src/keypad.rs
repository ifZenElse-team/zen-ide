use std::cell::RefCell;
use std::time::{Duration, Instant};

use zen_core::keymap::Keystroke;

pub const MULTIPLY: &str = "NumpadMultiply";

const KEPT: Duration = Duration::from_secs(1);

thread_local! {
    static PRESSED: RefCell<Vec<(Keystroke, Instant)>> = const { RefCell::new(Vec::new()) };
}

pub fn press(control: bool, alt: bool, shift: bool, command: bool) {
    let stroke = Keystroke { control, alt, shift, command, key: MULTIPLY.to_owned(), then: None };
    PRESSED.with(|pressed| pressed.borrow_mut().push((stroke, Instant::now())));
}

pub fn take() -> Vec<Keystroke> {
    PRESSED.with(|pressed| {
        std::mem::take(&mut *pressed.borrow_mut())
            .into_iter()
            .filter(|(_, at)| at.elapsed() < KEPT)
            .map(|(stroke, _)| stroke)
            .collect()
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native(
    app_name: &str,
    mut options: eframe::NativeOptions,
    app_creator: eframe::AppCreator<'_>,
) -> eframe::Result {
    use std::rc::Rc;
    use winit::platform::run_on_demand::EventLoopExtRunOnDemand as _;

    let mut builder = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event();
    if let Some(hook) = options.event_loop_builder.take() {
        hook(&mut builder);
    }
    let mut event_loop = builder.build()?;
    let created: Rc<RefCell<Option<Result<(), String>>>> = Rc::default();
    let outcome = Rc::clone(&created);
    let app_creator: eframe::AppCreator<'_> = Box::new(move |creation| {
        let app = app_creator(creation);
        *outcome.borrow_mut() = Some(app.as_ref().map(|_| ()).map_err(ToString::to_string));
        app
    });
    let windows = eframe::create_native(app_name, options, app_creator, &event_loop);
    let mut app = native::KeypadTap { windows, modifiers: winit::keyboard::ModifiersState::empty() };
    event_loop.run_app_on_demand(&mut app)?;
    drop(app);
    started(created.take())
}

#[cfg(not(target_arch = "wasm32"))]
fn started(created: Option<Result<(), String>>) -> eframe::Result {
    match created {
        Some(Ok(())) => Ok(()),
        Some(Err(error)) => Err(eframe::Error::AppCreation(error.into())),
        None => Err(eframe::Error::AppCreation("the window could not start; the log says why".into())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn tapped(
    key: winit::keyboard::PhysicalKey,
    state: winit::event::ElementState,
    repeat: bool,
    held: winit::keyboard::ModifiersState,
) -> Option<(bool, bool, bool, bool)> {
    let star = winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::NumpadMultiply);
    if key != star || state != winit::event::ElementState::Pressed || repeat {
        return None;
    }
    Some(if cfg!(target_os = "macos") {
        (held.control_key(), held.alt_key(), held.shift_key(), held.super_key())
    } else {
        (false, held.alt_key(), held.shift_key(), held.control_key())
    })
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use winit::application::ApplicationHandler;
    use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
    use winit::event_loop::ActiveEventLoop;
    use winit::keyboard::ModifiersState;
    use winit::window::WindowId;

    pub(super) struct KeypadTap<'a> {
        pub(super) windows: eframe::EframeWinitApplication<'a>,
        pub(super) modifiers: ModifiersState,
    }

    impl ApplicationHandler<eframe::UserEvent> for KeypadTap<'_> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.windows.resumed(event_loop);
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
            match &event {
                WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
                WindowEvent::KeyboardInput { event: key, .. } => {
                    if let Some((control, alt, shift, command)) =
                        super::tapped(key.physical_key, key.state, key.repeat, self.modifiers)
                    {
                        super::press(control, alt, shift, command);
                    }
                }
                _ => {}
            }
            self.windows.window_event(event_loop, window_id, event);
        }

        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            self.windows.new_events(event_loop, cause);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
            self.windows.user_event(event_loop, event);
        }

        fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
            self.windows.device_event(event_loop, device_id, event);
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.windows.about_to_wait(event_loop);
        }

        fn suspended(&mut self, event_loop: &ActiveEventLoop) {
            self.windows.suspended(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.windows.exiting(event_loop);
        }

        fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
            self.windows.memory_warning(event_loop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_are_taken_once_as_keystrokes() {
        press(false, true, false, true);
        let taken = take();
        assert_eq!(taken.iter().map(ToString::to_string).collect::<Vec<_>>(), ["⌥⌘NumPad*"]);
        assert!(take().is_empty(), "taken once");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn only_a_fresh_press_of_the_keypad_star_is_tapped_with_the_modifiers_held() {
        use winit::event::ElementState::{Pressed, Released};
        use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
        let star = PhysicalKey::Code(KeyCode::NumpadMultiply);
        let held = ModifiersState::SUPER | ModifiersState::ALT;
        let expected = if cfg!(target_os = "macos") { (false, true, false, true) } else { (false, true, false, false) };
        assert_eq!(tapped(star, Pressed, false, held), Some(expected));
        let control = if cfg!(target_os = "macos") { (true, false, false, false) } else { (false, false, false, true) };
        assert_eq!(tapped(star, Pressed, false, ModifiersState::CONTROL), Some(control));
        assert_eq!(tapped(star, Released, false, held), None);
        assert_eq!(tapped(star, Pressed, true, held), None, "a held key repeating");
        assert_eq!(tapped(PhysicalKey::Code(KeyCode::Digit8), Pressed, false, ModifiersState::SHIFT), None);
        assert_eq!(tapped(PhysicalKey::Code(KeyCode::NumpadAdd), Pressed, false, held), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_window_that_never_started_is_an_error_as_with_eframe() {
        assert!(started(Some(Ok(()))).is_ok());
        let failed = started(Some(Err("no adapter".to_owned()))).unwrap_err();
        assert!(failed.to_string().contains("no adapter"), "{failed}");
        assert!(started(None).is_err(), "the app was never created");
    }
}
