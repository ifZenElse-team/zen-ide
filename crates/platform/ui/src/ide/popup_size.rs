#[cfg(test)]
use egui::pos2;
use egui::{Context, CursorIcon, Id, Pos2, Rect, Sense, Ui, Vec2, vec2};

const GRIP: f32 = 6.0;

pub(super) const MARGIN: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct PopupKind {
    pub(super) name: &'static str,
    pub(super) min: Vec2,
}

impl PopupKind {
    fn id(self) -> Id {
        Id::unique(("popup-size", self.name))
    }

    pub(super) fn size(self, ctx: &Context, default: Vec2, max: Vec2) -> Vec2 {
        let remembered = ctx.data(|data| data.get_temp::<Vec2>(self.id()));
        self.clamp(remembered.unwrap_or(default), max)
    }

    #[cfg(test)]
    pub(super) fn forget(self, ctx: &Context) {
        ctx.data_mut(|data| data.remove::<Vec2>(self.id()));
    }

    fn clamp(self, size: Vec2, max: Vec2) -> Vec2 {
        size.clamp(self.min, max.max(self.min))
    }

    pub(super) fn grips(self, ui: &Ui, rect: Rect, size: Vec2, max: Vec2, place: impl Fn(Vec2) -> Rect) {
        let span = |edge: Edge, start: f32, end: f32| match edge {
            Edge::Start => start..=start + GRIP,
            Edge::End => end - GRIP..=end,
            Edge::Middle => start + GRIP..=end - GRIP,
        };
        let grips = [
            (Edge::Start, Edge::Middle, CursorIcon::ResizeHorizontal),
            (Edge::End, Edge::Middle, CursorIcon::ResizeHorizontal),
            (Edge::Middle, Edge::Start, CursorIcon::ResizeVertical),
            (Edge::Middle, Edge::End, CursorIcon::ResizeVertical),
            (Edge::Start, Edge::Start, CursorIcon::ResizeNwSe),
            (Edge::End, Edge::End, CursorIcon::ResizeNwSe),
            (Edge::End, Edge::Start, CursorIcon::ResizeNeSw),
            (Edge::Start, Edge::End, CursorIcon::ResizeNeSw),
        ];
        let mut wanted = None;
        for (x, y, cursor) in grips {
            let grip = Rect::from_x_y_ranges(span(x, rect.left(), rect.right()), span(y, rect.top(), rect.bottom()));
            let response = ui.interact(grip, self.id().with((x, y)), Sense::drag());
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(cursor);
            }
            if let (true, Some(pointer)) = (response.dragged(), response.interact_pointer_pos()) {
                let mut to = size;
                for (axis, edge) in [(0, x), (1, y)] {
                    if let Some(length) = edge.follow(axis, pointer, size, rect, &place) {
                        to[axis] = length;
                    }
                }
                wanted = Some(to);
            }
        }
        if let Some(wanted) = wanted {
            let wanted = self.clamp(wanted, max);
            ui.ctx().data_mut(|data| data.insert_temp(self.id(), wanted));
            if wanted != size {
                ui.ctx().request_repaint();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Edge {
    Middle,
    Start,
    End,
}

impl Edge {
    fn follow(self, axis: usize, pointer: Pos2, size: Vec2, rect: Rect, place: &impl Fn(Vec2) -> Rect) -> Option<f32> {
        let edge = |rect: Rect| match self {
            Edge::Start => Some(rect.min[axis]),
            Edge::End => Some(rect.max[axis]),
            Edge::Middle => None,
        };
        let step = if axis == 0 { vec2(100.0, 0.0) } else { vec2(0.0, 100.0) };
        let now = edge(rect)?;
        let rate = (edge(place(size + step))? - edge(place(size))?) / 100.0;
        (rate.abs() > f32::EPSILON).then(|| size[axis] + (pointer[axis] - now) / rate)
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, Pos2, RawInput};

    use super::*;

    #[test]
    fn each_kind_of_popup_keeps_its_own_size() {
        let ctx = Context::default();
        let first = PopupKind { name: "first", min: vec2(100.0, 100.0) };
        let second = PopupKind { name: "second", min: vec2(100.0, 100.0) };
        let (default, max) = (vec2(300.0, 200.0), vec2(800.0, 600.0));
        let place = |size: Vec2| Rect::from_min_size(Pos2::ZERO, size);
        let edge = pos2(298.0, 100.0);
        for (at, pressed) in
            [(edge, None), (edge, Some(true)), (edge + vec2(50.0, 0.0), None), (edge + vec2(50.0, 0.0), Some(false))]
        {
            let mut events = vec![Event::PointerMoved(at)];
            if let Some(pressed) = pressed {
                events.push(Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
                let size = first.size(ui.ctx(), default, max);
                first.grips(ui, place(size), size, max, place);
            });
            output.textures_delta.clear();
        }
        assert_eq!(first.size(&ctx, default, max), vec2(348.0, 200.0), "the edge follows the pointer");
        assert_eq!(second.size(&ctx, default, max), default);
        assert_eq!(first.size(&ctx, default, vec2(320.0, 150.0)), vec2(320.0, 150.0), "kept within the window");
    }
}
