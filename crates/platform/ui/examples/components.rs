use std::path::PathBuf;

use egui::{Rect, Sense, Ui, Vec2, pos2, vec2};
use zen_i18n::t;
use zen_ui::components::{self, IconButton, Tab};
use zen_ui::icons;
use zen_ui::offscreen::{self, Offscreen};
use zen_ui::theme::Palette;

const SIZE: Vec2 = vec2(380.0, 300.0);

fn gallery(ui: &mut Ui, p: &Palette, query: &mut String, on: &mut bool) {
    let window = ui.max_rect();
    let [top_left, top_right, bottom_left, bottom_right] = p.window;
    icons::fill_rounded(ui.painter(), window, 0.0, |at| {
        components::lerp(
            components::lerp(top_left, top_right, at.x),
            components::lerp(bottom_left, bottom_right, at.x),
            at.y,
        )
    });
    let tabs = [
        Tab { label: t!("toolbox-tab-tools"), badge: Some(2) },
        Tab { label: t!("toolbox-tab-projects"), badge: None },
    ];
    let _ = components::tabs(ui, p, Rect::from_min_size(pos2(12.0, 0.0), vec2(356.0, 42.0)), &tabs, 0);
    let search = Rect::from_center_size(pos2(352.0, 21.0), Vec2::splat(30.0));
    let style = IconButton { icon: icons::search, icon_size: 14.0, color: p.text_secondary, radius: 8 };
    let _ =
        components::icon_button_named(ui, p, search, ui.scope_id().with("search"), style, || t!("toolbox-search-hint"));
    let field = Rect::from_min_size(pos2(12.0, 52.0), vec2(356.0, components::metrics::TEXT_FIELD));
    let _ = components::text_field(ui, p, field, query, &t!("toolbox-search-hint"), Some(icons::search));
    let mut rows =
        ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(12.0, 96.0), vec2(356.0, 60.0))));
    let row = components::list_row_named(&mut rows, p, 60.0, Sense::hover(), || "GoGarden".to_owned());
    let anchor = pos2(row.rect.right() - 10.0, row.rect.center().y);
    let id = rows.scope_id().with("install");
    let _ = components::primary_button(&mut rows, p, anchor, &t!("toolbox-action-install"), id);
    let mut toggles =
        ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(24.0, 176.0), vec2(120.0, 20.0))));
    let _ = components::toggle(&mut toggles, p, on, &t!("toolbox-menu-settings"));
    let menu = Rect::from_min_size(pos2(180.0, 170.0), Vec2::splat(30.0));
    let response = ui.interact(menu, ui.scope_id().with("menu"), Sense::click());
    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&response));
    let style = IconButton { icon: icons::more_vertical, icon_size: 15.0, color: p.text_secondary, radius: 6 };
    let _ = components::icon_button_named(ui, p, menu, ui.scope_id().with("menu-icon"), style, || {
        t!("toolbox-menu-settings")
    });
    let _ = components::popup_menu(&response, |ui| {
        let _ = components::menu_item(ui, t!("toolbox-whats-new"));
        components::menu_item_disabled(ui, t!("toolbox-tool-settings"), None);
        ui.separator();
        let _ = components::menu_item(ui, t!("toolbox-uninstall-menu"));
    });
}

fn main() -> Result<(), String> {
    let folder = std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: components <folder>")?;
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    let p = Palette::DARK;
    for scale in [1, 2] {
        let mut offscreen = Offscreen::new(SIZE, scale as f32);
        p.apply(offscreen.context());
        let mut query = String::new();
        let mut on = true;
        let image = offscreen.render(|ui| gallery(ui, &p, &mut query, &mut on));
        offscreen::write_png(&folder.join(format!("components@{scale}x.png")), &image)?;
    }
    Ok(())
}
