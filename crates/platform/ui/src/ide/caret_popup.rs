use egui::{Align2, Area, Context, Pos2, Rect, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CaretPlace {
    rect: Option<Rect>,
    moving: bool,
    text: Option<Rect>,
}

impl CaretPlace {
    pub(super) fn new(rect: Option<Rect>, moving: bool, text: Option<Rect>) -> Self {
        Self { rect, moving, text }
    }

    pub(super) fn rect(self) -> Option<Rect> {
        if self.moving {
            return None;
        }
        let text = self.text?;
        match self.rect {
            Some(rect) if text.intersects(rect) => Some(rect),
            _ => Some(Rect::from_center_size(text.center(), Vec2::ZERO)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PopupAnchor {
    Caret(Vec2),
    At(Pos2),
}

impl PopupAnchor {
    pub(super) const UNDER_CARET: Self = Self::Caret(vec2(-24.0, 2.0));

    pub(super) fn area(self, ctx: &Context, area: Area, caret: CaretPlace) -> Option<Area> {
        let screen = ctx.content_rect();
        let height = ctx.memory(|memory| memory.area_rect(area.layer().id)).map_or(0.0, |rect| rect.height());
        let (at, pivot) = self.place(caret, screen, height)?;
        Some(area.pivot(pivot).fixed_pos(at).constrain_to(screen))
    }

    fn place(self, caret: CaretPlace, screen: Rect, height: f32) -> Option<(Pos2, Align2)> {
        let offset = match self {
            Self::At(at) => return Some((at, Align2::LEFT_TOP)),
            Self::Caret(offset) => offset,
        };
        if caret.moving {
            return None;
        }
        let rect = caret.rect().unwrap_or_else(|| Rect::from_center_size(screen.center(), Vec2::ZERO));
        let below = rect.left_bottom() + offset;
        let room_below = screen.bottom() - below.y;
        let above = rect.top() - offset.y;
        if height > room_below && above - screen.top() > room_below {
            Some((pos2(below.x, above), Align2::LEFT_BOTTOM))
        } else {
            Some((below, Align2::LEFT_TOP))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_popup_at_the_caret_waits_for_the_caret_and_flips_above_it_at_the_bottom() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0));
        let text = Some(Rect::from_min_max(pos2(50.0, 40.0), pos2(1000.0, 800.0)));
        let caret = Rect::from_min_size(pos2(100.0, 200.0), vec2(1.0, 20.0));
        let shown = CaretPlace::new(Some(caret), false, text);
        assert_eq!(PopupAnchor::UNDER_CARET.place(shown, screen, 300.0), Some((pos2(76.0, 222.0), Align2::LEFT_TOP)));
        assert_eq!(shown.rect(), Some(caret));
        let moving = CaretPlace::new(Some(caret), true, text);
        assert_eq!(PopupAnchor::UNDER_CARET.place(moving, screen, 300.0), None);
        assert_eq!(moving.rect(), None);
        let at = PopupAnchor::At(pos2(1.0, 2.0));
        assert_eq!(at.place(moving, screen, 300.0), Some((pos2(1.0, 2.0), Align2::LEFT_TOP)));
        let low = CaretPlace::new(Some(caret.translate(vec2(0.0, 560.0))), false, text);
        assert_eq!(PopupAnchor::UNDER_CARET.place(low, screen, 300.0), Some((pos2(76.0, 758.0), Align2::LEFT_BOTTOM)));
        assert_eq!(PopupAnchor::UNDER_CARET.place(low, screen, 10.0), Some((pos2(76.0, 782.0), Align2::LEFT_TOP)));
    }

    #[test]
    fn a_caret_off_the_screen_puts_the_popup_in_the_middle_of_the_text() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0));
        let text = Rect::from_min_max(pos2(50.0, 40.0), pos2(1050.0, 840.0));
        let away = CaretPlace::new(Some(Rect::from_min_size(pos2(100.0, -400.0), vec2(1.0, 20.0))), false, Some(text));
        assert_eq!(away.rect(), Some(Rect::from_center_size(text.center(), Vec2::ZERO)));
        let undrawn = CaretPlace::new(None, false, Some(text));
        assert_eq!(undrawn.rect(), away.rect());
        assert_eq!(
            PopupAnchor::Caret(vec2(0.0, 2.0)).place(CaretPlace::default(), screen, 0.0),
            Some((pos2(500.0, 402.0), Align2::LEFT_TOP))
        );
    }
}
