use egui::text::CCursorRange;
use egui::text_edit::TextEditState;
use egui::{Event, Key, Rect, Response, Ui, UiBuilder, vec2};

pub(super) fn take_typing(ui: &Ui) {
    ui.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                Event::Text(_)
                    | Event::Paste(_)
                    | Event::Ime(_)
                    | Event::Cut
                    | Event::Key { key: Key::Backspace | Key::Delete, .. }
            )
        })
    });
}

pub(super) fn hold_typing(ui: &Ui) -> Vec<Event> {
    ui.input_mut(|input| {
        let (typing, rest): (Vec<Event>, Vec<Event>) =
            std::mem::take(&mut input.events).into_iter().partition(|event| {
                matches!(
                    event,
                    Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Ime(_) | Event::Copy | Event::Cut
                )
            });
        input.events = rest;
        typing
    })
}

pub(super) fn release_typing(ui: &Ui, held: Vec<Event>) {
    if !held.is_empty() {
        ui.input_mut(|input| {
            input.events.splice(0..0, held);
        });
    }
}

pub(super) fn focus_selected(ui: &Ui, field: &Response, text: &str) {
    field.request_focus();
    let mut state = TextEditState::load(ui.ctx(), field.id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::select_all_str(text)));
    state.store(ui.ctx(), field.id);
}

pub(super) fn list<R>(ui: &mut Ui, id_salt: &str, max_height: f32, rows: impl FnOnce(&mut Ui) -> R) -> R {
    let room = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), max_height));
    ui.scope_builder(UiBuilder::new().max_rect(room), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .max_height(max_height)
            .auto_shrink([false, true])
            .show(ui, rows)
            .inner
    })
    .inner
}
