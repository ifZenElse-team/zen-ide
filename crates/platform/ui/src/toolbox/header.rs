use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, Galley, Rect, Ui, Vec2, pos2};

use crate::theme;

const GAP: f32 = 6.0;
const MIN_SELECTOR: f32 = 98.0;
const NAME_X: f32 = 31.0;
const CHEVRON_X: f32 = 14.0;
const CHEVRON: f32 = 13.0;
const NAME_GAP: f32 = 6.0;

pub(super) struct HeaderText<'a> {
    pub family: &'a str,
    pub product: &'a str,
    pub wordmark_color: (Color32, Color32),
    pub environment: &'a str,
    pub environment_color: Color32,
}

pub(super) struct HeaderLayout {
    pub mark: Rect,
    pub family: Rect,
    pub product: Rect,
    pub family_galley: Arc<Galley>,
    pub product_galley: Arc<Galley>,
    pub selector: Rect,
    pub laptop: Rect,
    pub name: Rect,
    pub name_galley: Arc<Galley>,
    pub chevron: Rect,
    pub gear: Rect,
    pub elided: bool,
}

impl HeaderLayout {
    pub(super) fn new(ui: &Ui, rect: Rect, text: &HeaderText) -> Self {
        let painter = ui.painter();
        let y = rect.center().y;

        let mark = Rect::from_center_size(pos2(rect.left() + 21.0, y), Vec2::splat(34.0));
        let wordmark_x = mark.right() + 11.0;
        let family_galley =
            painter.layout_no_wrap(text.family.to_owned(), theme::semibold(11.5), text.wordmark_color.0);
        let product_galley = painter.layout_no_wrap(text.product.to_owned(), theme::bold(19.0), text.wordmark_color.1);
        let family =
            Rect::from_min_size(pos2(wordmark_x, y - 9.0 - family_galley.size().y / 2.0), family_galley.size());
        let product =
            Rect::from_min_size(pos2(wordmark_x - 1.0, y + 7.0 - product_galley.size().y / 2.0), product_galley.size());

        let gear = Rect::from_center_size(pos2(rect.right() - 16.0, y), Vec2::splat(30.0));

        let right = gear.left() - GAP;
        let leftmost = (family.union(product).right() + GAP).min(right);
        let reserved = NAME_X + NAME_GAP + CHEVRON_X + CHEVRON / 2.0;
        let natural =
            painter.layout_no_wrap(text.environment.to_owned(), theme::semibold(13.5), text.environment_color).size().x;
        let width = (reserved + natural).max(MIN_SELECTOR).min(right - leftmost);
        let selector = Rect::from_min_max(pos2(right - width, y - 15.0), pos2(right, y + 15.0));

        let mut job =
            LayoutJob::simple_singleline(text.environment.to_owned(), theme::semibold(13.5), text.environment_color);
        job.wrap = TextWrapping {
            max_width: (width - reserved).max(1.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let name_galley = painter.layout_job(job);
        let name =
            Rect::from_min_size(pos2(selector.left() + NAME_X, y - name_galley.size().y / 2.0), name_galley.size());

        Self {
            mark,
            family,
            product,
            family_galley,
            product_galley,
            laptop: Rect::from_center_size(pos2(selector.left() + 17.0, y), Vec2::splat(15.0)),
            elided: name_galley.elided,
            name,
            name_galley,
            chevron: Rect::from_center_size(pos2(selector.right() - CHEVRON_X, y), Vec2::splat(CHEVRON)),
            selector,
            gear,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn with_ui(check: impl FnOnce(&Ui)) {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut check = Some(check);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(check) = check.take() {
                check(ui);
            }
        })
        .textures_delta
        .clear();
    }

    fn layout(ui: &Ui, width: f32, environment: &str) -> (Rect, HeaderLayout) {
        let rect = Rect::from_min_size(pos2(12.0, 0.0), vec2(width, 66.0));
        let text = HeaderText {
            family: "Zen",
            product: "Toolbox",
            wordmark_color: (Color32::GRAY, Color32::WHITE),
            environment,
            environment_color: Color32::WHITE,
        };
        (rect, HeaderLayout::new(ui, rect, &text))
    }

    #[test]
    fn the_environment_selector_never_runs_into_the_gear() {
        let names = ["Local", "Локальные", "Удалённые окружения разработчика"];
        with_ui(|ui| {
            for name in names {
                for width in [408.0, 360.0, 320.0, 280.0, 240.0] {
                    let (rect, header) = layout(ui, width, name);
                    let at = format!("`{name}` at {width}");
                    assert!(rect.contains_rect(header.gear), "{at}: the gear is outside the header");
                    assert!(header.selector.right() <= header.gear.left(), "{at}: the selector reaches the gear");
                    assert!(
                        header.selector.left() >= header.family.union(header.product).right(),
                        "{at}: the selector covers the wordmark"
                    );
                    assert!(header.selector.contains_rect(header.name), "{at}: the name leaves the selector");
                    assert!(header.selector.contains_rect(header.chevron), "{at}: the chevron leaves the selector");
                    assert!(
                        header.name.right() + NAME_GAP <= header.chevron.left() + 0.01,
                        "{at}: the name {:?} runs into the chevron {:?}",
                        header.name,
                        header.chevron
                    );
                    assert!(!header.chevron.intersects(header.gear), "{at}: the chevron is under the gear");
                    assert!(!header.laptop.intersects(header.name), "{at}: the name covers the laptop");
                }
                let (_, header) = layout(ui, 408.0, name);
                if name.chars().count() < 12 {
                    assert!(!header.elided, "`{name}` fits in the window in full");
                }
            }
        });
    }

    #[test]
    fn a_name_that_does_not_fit_ends_in_an_ellipsis() {
        with_ui(|ui| {
            let (_, header) = layout(ui, 408.0, "Удалённые окружения разработчика");
            assert!(header.elided);
            let (_, short) = layout(ui, 408.0, "Local");
            assert_eq!(short.selector.width(), MIN_SELECTOR, "Local keeps the narrowest width");
        });
    }
}
