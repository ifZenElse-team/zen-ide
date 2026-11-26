use egui::Id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    Editor,
    Widget(Id),
}

#[derive(Debug, Default)]
pub(super) struct FocusReturn {
    owner: Option<Owner>,
    previous: Option<Owner>,
    overlay: bool,
    dialog_focus: Option<Id>,
}

impl FocusReturn {
    pub(super) fn frame(&mut self, overlay: bool, editor: bool, focused: Option<Id>) -> Option<Owner> {
        let sample = if editor { Some(Owner::Editor) } else { focused.map(Owner::Widget) };
        let previous = std::mem::replace(&mut self.previous, sample);
        if overlay {
            if !self.overlay {
                self.overlay = true;
                self.dialog_focus = None;
            }
            if let Some(Owner::Widget(id)) = sample {
                self.dialog_focus = Some(id);
            }
            return None;
        }
        if std::mem::take(&mut self.overlay) {
            self.previous = None;
            let dialog = self.dialog_focus.take();
            if focused.is_none() || focused == dialog {
                return self.owner;
            }
            self.owner = sample;
            return None;
        }
        if previous.is_some() {
            self.owner = previous;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_dialog_gives_the_keyboard_back_to_the_editor() {
        let mut focus = FocusReturn::default();
        let (editor, field) = (Id::unique("editor"), Id::unique("dialog-field"));
        focus.frame(false, true, Some(editor));
        assert_eq!(focus.frame(false, false, Some(field)), None);
        assert_eq!(focus.frame(true, false, Some(field)), None);
        assert_eq!(focus.frame(true, false, Some(field)), None);
        assert_eq!(focus.frame(false, false, None), Some(Owner::Editor), "⎋ left nobody with the keyboard");
        assert_eq!(focus.frame(false, false, None), None, "only once");
        focus.frame(false, true, Some(editor));
        focus.frame(true, false, Some(field));
        assert_eq!(focus.frame(false, false, Some(field)), Some(Owner::Editor));
    }

    #[test]
    fn a_widget_that_had_the_keyboard_gets_it_back_and_a_new_owner_keeps_it() {
        let mut focus = FocusReturn::default();
        let (terminal, field, tree) = (Id::unique("terminal"), Id::unique("field"), Id::unique("tree-search"));
        focus.frame(false, false, Some(terminal));
        focus.frame(false, false, None);
        focus.frame(false, false, None);
        focus.frame(true, false, Some(field));
        assert_eq!(focus.frame(false, false, None), Some(Owner::Widget(terminal)), "a click elsewhere kept the owner");

        focus.frame(true, false, Some(field));
        assert_eq!(focus.frame(false, false, Some(tree)), None, "the dialog handed the keyboard on");
        focus.frame(true, false, Some(field));
        assert_eq!(focus.frame(false, false, None), Some(Owner::Widget(tree)));
        focus.frame(true, false, Some(field));
        assert_eq!(focus.frame(false, true, Some(Id::unique("editor"))), None, "the editor took it itself");
    }
}
