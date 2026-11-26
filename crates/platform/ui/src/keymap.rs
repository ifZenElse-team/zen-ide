use std::collections::BTreeMap;
use std::sync::RwLock;

use zen_core::keymap::{Keymap, Keystroke};
use zen_workbench::keymaps::{self, SharedKeymaps};

pub const DISABLE_DOUBLE_MODIFIERS: &str = "ui.doubleModifierShortcuts.disabled";

static STORE: RwLock<Option<SharedKeymaps>> = RwLock::new(None);

pub fn load(store: SharedKeymaps) {
    if let Ok(mut state) = STORE.write() {
        *state = Some(store);
    }
}

pub fn store() -> Option<SharedKeymaps> {
    STORE.read().ok().and_then(|state| state.clone())
}

pub fn current() -> Keymap {
    store().map_or_else(Keymap::default_macos, |store| keymaps::lock(&store).active().clone())
}

pub fn current_id() -> String {
    store()
        .map_or_else(|| zen_core::keymap::MACOS_KEYMAP.to_owned(), |store| keymaps::lock(&store).current().to_owned())
}

pub fn keymap(id: &str) -> Option<Keymap> {
    match store() {
        Some(store) => keymaps::lock(&store).keymap(id),
        None => (id == zen_core::keymap::MACOS_KEYMAP).then(Keymap::default_macos),
    }
}

pub fn set(action: &str, keystrokes: Option<Vec<Keystroke>>) -> Result<(), String> {
    let mut user = current().user_changes().clone();
    match keystrokes {
        Some(keys) => {
            user.insert(action.to_owned(), keys);
        }
        None => {
            user.remove(action);
        }
    }
    replace(&current_id(), user)
}

pub fn replace(id: &str, user: BTreeMap<String, Vec<Keystroke>>) -> Result<(), String> {
    let Some(store) = store() else { return Ok(()) };
    keymaps::lock(&store).set_changes_of(id, user).map_err(|error| keymaps::message(&error).message)
}

pub fn recordable(key: egui::Key, physical_key: Option<egui::Key>, modifiers: egui::Modifiers) -> Option<Keystroke> {
    let stroke = presses(key, physical_key, modifiers).swap_remove(0);
    (Keystroke::parse(&stroke.to_string()).as_ref() == Some(&stroke)).then_some(stroke)
}

pub fn keystroke(key: egui::Key, modifiers: egui::Modifiers) -> Keystroke {
    let (control, command) = if cfg!(target_os = "macos") {
        (modifiers.ctrl, modifiers.mac_cmd || modifiers.command)
    } else {
        (false, modifiers.ctrl)
    };
    Keystroke { control, alt: modifiers.alt, shift: modifiers.shift, command, key: format!("{key:?}"), then: None }
}

pub fn presses(key: egui::Key, physical_key: Option<egui::Key>, modifiers: egui::Modifiers) -> Vec<Keystroke> {
    use egui::Key::{Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9};
    let typed = keystroke(key, modifiers);
    match physical_key {
        Some(digit @ (Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9))
            if modifiers.shift && digit != key =>
        {
            vec![keystroke(digit, modifiers), typed]
        }
        _ => vec![typed],
    }
}

pub fn lookup(
    keymap: &Keymap,
    key: egui::Key,
    physical_key: Option<egui::Key>,
    modifiers: egui::Modifiers,
    context: zen_core::keymap::Context,
) -> Option<zen_core::keymap::Binding> {
    presses(key, physical_key, modifiers).iter().find_map(|stroke| keymap.lookup(stroke, context))
}

pub fn shortcut(action: &str) -> Option<String> {
    current().shortcut(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn a_digit_with_shift_stays_a_digit_and_other_shifted_keys_keep_their_meaning() {
        use egui::Key;
        let keymap = Keymap::default_macos();
        let mods = |ctrl, shift, command| egui::Modifiers { ctrl, shift, command, mac_cmd: command, alt: false };
        let action = |key, physical, modifiers| {
            lookup(&keymap, key, Some(physical), modifiers, zen_core::keymap::Context::Editor).map(|found| found.action)
        };
        let ctrl_shift = mods(true, true, false);
        assert_eq!(
            action(Key::Exclamationmark, Key::Num1, ctrl_shift).as_deref(),
            Some("navigate.bookmark.toggleMnemonic/1"),
            "US ⌃⇧1 types !"
        );
        assert_eq!(
            action(Key::Semicolon, Key::Num4, ctrl_shift).as_deref(),
            Some("navigate.bookmark.toggleMnemonic/4"),
            "Russian ⌃⇧4 types ;"
        );
        assert_eq!(
            action(Key::Questionmark, Key::Num7, ctrl_shift).as_deref(),
            Some("navigate.bookmark.toggleMnemonic/7"),
            "Russian ⌃⇧7 types ?"
        );
        assert_eq!(
            action(Key::Num2, Key::Num2, ctrl_shift).as_deref(),
            Some("navigate.bookmark.toggleMnemonic/2"),
            "a symbol egui has no key for"
        );
        assert_eq!(action(Key::Num3, Key::Num3, mods(true, false, false)).as_deref(), Some("navigate.bookmark.goTo/3"));
        let shift_command = mods(false, true, true);
        assert_eq!(action(Key::Num8, Key::Num8, shift_command).as_deref(), Some("editor.selection.columnMode"), "⇧⌘8");
        assert_eq!(
            action(Key::Plus, Key::Equals, shift_command).as_deref(),
            Some("editor.fold.expandAll"),
            "US ⇧⌘= types +"
        );
        assert_eq!(
            action(Key::Plus, Key::Num1, shift_command).as_deref(),
            Some("editor.fold.expandAll"),
            "Swiss ⇧1 types +"
        );
        assert_eq!(
            action(Key::Minus, Key::Minus, mods(false, false, true)).as_deref(),
            Some("editor.fold.collapse"),
            "⌘−"
        );
        assert_eq!(
            action(Key::Plus, Key::Equals, mods(false, false, true)).as_deref(),
            Some("editor.fold.expand"),
            "⌘+"
        );
        assert_eq!(
            action(Key::OpenBracket, Key::OpenBracket, mods(false, false, true)).as_deref(),
            Some("navigate.history.back"),
            "⌘[, also on the Russian layout"
        );
        assert_eq!(
            action(Key::CloseBracket, Key::CloseBracket, mods(false, false, true)).as_deref(),
            Some("navigate.history.forward")
        );
        assert_eq!(action(Key::OpenCurlyBracket, Key::OpenBracket, shift_command), None, "⇧⌘[ is not ⌘[");
    }

    #[test]
    fn a_recorded_shortcut_keeps_the_digit_of_a_shifted_digit_key() {
        let shift = egui::Modifiers { ctrl: true, shift: true, ..egui::Modifiers::NONE };
        let recorded = recordable(egui::Key::Exclamationmark, Some(egui::Key::Num1), shift).unwrap();
        assert_eq!(recorded, Keystroke::parse(if cfg!(target_os = "macos") { "⌃⇧1" } else { "⇧⌘1" }).unwrap());
        let plus = recordable(egui::Key::Plus, Some(egui::Key::Equals), shift).unwrap();
        assert_eq!(plus.key, "Plus", "only digits");
        assert_eq!(recordable(egui::Key::Exclamationmark, None, shift), None, "no key the keymap names");
    }

    #[test]
    fn key_presses_match_the_keymap() {
        let keymap = Keymap::default_macos();
        let press = |key, modifiers| {
            keymap.lookup(&keystroke(key, modifiers), zen_core::keymap::Context::Editor).map(|b| b.action)
        };
        assert_eq!(press(egui::Key::D, egui::Modifiers::COMMAND).as_deref(), Some("editor.line.duplicate"));
        assert_eq!(
            press(egui::Key::Slash, egui::Modifiers::COMMAND | egui::Modifiers::ALT).as_deref(),
            Some("editor.comment.block")
        );
        assert_eq!(press(egui::Key::Num1, egui::Modifiers::COMMAND).as_deref(), Some("view.toolWindow.project"));
        assert_eq!(
            press(egui::Key::ArrowLeft, egui::Modifiers::ALT | egui::Modifiers::SHIFT).as_deref(),
            Some("editor.caret.move/wordLeft+extend")
        );
        assert_eq!(press(egui::Key::G, egui::Modifiers::CTRL).as_deref(), Some("editor.selection.addNextOccurrence"));
    }
}
