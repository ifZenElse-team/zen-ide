#![cfg(not(target_arch = "wasm32"))]

use std::cell::Cell;

use egui::accesskit::Role;
use egui::{Color32, ColorImage, Event, Modifiers, PointerButton, Pos2, Rect, Sense, Ui, pos2, vec2};
use zen_ui::components::{self, IconButton, Tab};
use zen_ui::icons;
use zen_ui::offscreen::{self, Offscreen};
use zen_ui::theme::Palette;

const P: Palette = Palette::DARK;
const BACKGROUND: Color32 = Color32::from_rgb(0x12, 0x13, 0x1B);

fn renderer() -> Offscreen {
    let mut offscreen = Offscreen::new(vec2(240.0, 120.0), 1.0);
    offscreen.set_background(BACKGROUND);
    P.apply(offscreen.context());
    offscreen
}

fn changed_pixels(image: &ColorImage) -> usize {
    image.pixels.iter().filter(|pixel| **pixel != BACKGROUND).count()
}

fn pixels_in(image: &ColorImage, rect: Rect) -> Vec<Color32> {
    let [width, _] = image.size;
    let mut pixels = Vec::new();
    for y in rect.top() as usize..rect.bottom() as usize {
        for x in rect.left() as usize..rect.right() as usize {
            pixels.push(image.pixels[y * width + x]);
        }
    }
    pixels
}

fn click(at: Pos2) -> Vec<Event> {
    let button =
        |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    vec![Event::PointerMoved(at), button(true), button(false)]
}

#[test]
fn renders_a_filled_rect_with_its_exact_colour_at_any_scale() {
    for scale in [1.0, 2.0] {
        let mut offscreen = Offscreen::new(vec2(40.0, 30.0), scale);
        let image = offscreen.render(|ui| {
            ui.painter().rect_filled(Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 10.0)), 0, Color32::RED);
        });
        assert_eq!(image.size, [(40.0 * scale) as usize, (30.0 * scale) as usize]);
        let [width, _] = image.size;
        let center = (15.0 * scale) as usize * width + (20.0 * scale) as usize;
        assert_eq!(image.pixels[center], Color32::RED);
        assert_eq!(image.pixels[0], Color32::TRANSPARENT);
    }
}

#[test]
fn the_first_frame_is_laid_out_at_full_size_at_any_scale() {
    for scale in [1.0, 2.0] {
        let mut offscreen = Offscreen::new(vec2(40.0, 30.0), scale);
        let first = Cell::new(Rect::NOTHING);
        let _ = offscreen.frame(Vec::new(), |ui| first.set(ui.ctx().content_rect()));
        assert_eq!(first.get(), Rect::from_min_size(Pos2::ZERO, vec2(40.0, 30.0)));
    }
}

#[test]
fn renders_text_with_the_font_atlas() {
    let mut offscreen = renderer();
    let image = offscreen.render(|ui| {
        ui.painter().text(pos2(10.0, 10.0), egui::Align2::LEFT_TOP, "Zen", zen_ui::theme::regular(20.0), P.text);
    });
    assert!(changed_pixels(&image) > 50, "the glyphs are drawn");
}

#[test]
fn rendering_is_deterministic() {
    let draw = |ui: &mut Ui| {
        let _ = components::primary_button(ui, &P, pos2(200.0, 40.0), "Install", ui.scope_id().with("install"));
    };
    let first = renderer().render(draw);
    let second = renderer().render(draw);
    assert_eq!(first, second);
}

#[test]
fn primary_button_draws_the_accent_and_reacts_to_hover_and_click() {
    let mut offscreen = renderer();
    let mut clicked = false;
    let mut draw = |ui: &mut Ui| {
        let id = ui.scope_id().with("update");
        clicked |= components::primary_button(ui, &P, pos2(200.0, 40.0), "Update", id).clicked();
    };
    let idle = offscreen.render(&mut draw);
    assert!(changed_pixels(&idle) > 500, "the pill is filled");
    let hovered = offscreen.frame(vec![Event::PointerMoved(pos2(190.0, 40.0))], &mut draw);
    assert_ne!(idle, hovered, "hover changes the gradient");
    let _ = offscreen.frame(click(pos2(190.0, 40.0)), &mut draw);
    let _ = offscreen.frame(Vec::new(), &mut draw);
    assert!(clicked);
}

#[test]
fn icon_button_shows_its_hover_background() {
    let mut offscreen = renderer();
    let button = Rect::from_min_size(pos2(20.0, 20.0), vec2(30.0, 30.0));
    let mut draw = |ui: &mut Ui| {
        let style = IconButton { icon: icons::gear, icon_size: 16.0, color: P.text_secondary, radius: 8 };
        let _ = components::icon_button(ui, &P, button, ui.scope_id().with("gear"), style);
    };
    let idle = offscreen.render(&mut draw);
    assert!(changed_pixels(&idle) > 20, "the icon is drawn");
    let hovered = offscreen.frame(vec![Event::PointerMoved(button.center())], &mut draw);
    let corner = Rect::from_min_size(button.min + vec2(4.0, 4.0), vec2(3.0, 3.0));
    assert!(pixels_in(&idle, corner).iter().all(|pixel| *pixel == BACKGROUND));
    assert!(pixels_in(&hovered, corner).iter().all(|pixel| *pixel != BACKGROUND));
}

#[test]
fn list_row_is_highlighted_only_while_hovered() {
    let mut offscreen = renderer();
    let mut draw = |ui: &mut Ui| {
        let _ = components::list_row(ui, &P, 60.0, Sense::click());
    };
    let idle = offscreen.render(&mut draw);
    assert_eq!(changed_pixels(&idle), 0);
    let hovered = offscreen.frame(vec![Event::PointerMoved(pos2(100.0, 30.0))], &mut draw);
    assert!(changed_pixels(&hovered) > 5000);
}

#[test]
fn tabs_underline_the_active_tab_and_report_clicks() {
    let mut offscreen = renderer();
    let items = [Tab { label: "Tools".to_owned(), badge: Some(3) }, Tab { label: "Projects".to_owned(), badge: None }];
    let mut active = 0;
    let row = Rect::from_min_size(Pos2::ZERO, vec2(240.0, 42.0));
    let mut draw = |ui: &mut Ui| {
        if let Some(index) = components::tabs(ui, &P, row, &items, active) {
            active = index;
        }
    };
    let image = offscreen.render(&mut draw);
    let underline = pixels_in(&image, Rect::from_min_max(pos2(10.0, 38.0), pos2(30.0, 40.0)));
    assert!(underline.iter().all(|pixel| *pixel == P.link), "the active tab is underlined");
    let _ = offscreen.frame(click(pos2(150.0, 21.0)), &mut draw);
    let _ = offscreen.frame(Vec::new(), &mut draw);
    assert_eq!(active, 1);
}

#[test]
fn popup_menu_opens_on_click_and_returns_the_chosen_item() {
    let mut offscreen = Offscreen::new(vec2(240.0, 200.0), 1.0);
    offscreen.set_background(BACKGROUND);
    P.apply(offscreen.context());
    let anchor = Rect::from_min_size(pos2(10.0, 10.0), vec2(30.0, 30.0));
    let chosen = Cell::new(false);
    let first_item = Cell::new(None);
    let mut draw = |ui: &mut Ui| {
        let response = ui.interact(anchor, ui.scope_id().with("menu"), Sense::click());
        let _ = components::popup_menu(&response, |ui| {
            if components::menu_item(ui, "What's new") {
                chosen.set(true);
            }
            first_item.set(Some(ui.min_rect()));
            components::menu_item_disabled(ui, "Settings", Some("Not implemented yet"));
        });
    };
    let closed = offscreen.render(&mut draw);
    assert_eq!(changed_pixels(&closed), 0);
    let _ = offscreen.frame(click(anchor.center()), &mut draw);
    let open = offscreen.render(&mut draw);
    let below = Rect::from_min_max(pos2(10.0, 45.0), pos2(120.0, 90.0));
    assert!(pixels_in(&open, below).iter().any(|pixel| *pixel != BACKGROUND), "the menu is drawn below");
    let item = first_item.get().expect("the menu shows its items").center();
    let _ = offscreen.frame(click(item), &mut draw);
    let _ = offscreen.frame(Vec::new(), &mut draw);
    assert!(chosen.get());
}

#[test]
fn toggle_flips_on_click_and_draws_both_states() {
    let mut offscreen = renderer();
    let mut on = false;
    let mut draw = |ui: &mut Ui| {
        let _ = components::toggle(ui, &P, &mut on, "Enabled");
    };
    let off_image = offscreen.render(&mut draw);
    assert!(changed_pixels(&off_image) > 100);
    let toggle = Rect::from_min_size(Pos2::ZERO, vec2(30.0, 18.0)).translate(vec2(8.0, 8.0));
    let _ = offscreen.frame(click(toggle.center()), &mut draw);
    for _ in 0..30 {
        let _ = offscreen.frame(Vec::new(), &mut draw);
    }
    let on_image = offscreen.render(&mut draw);
    assert!(on);
    assert_ne!(off_image, on_image);
}

#[test]
fn text_field_shows_the_hint_and_takes_typed_text() {
    let mut offscreen = renderer();
    let mut text = String::new();
    let field = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, components::metrics::TEXT_FIELD));
    let mut draw = |ui: &mut Ui| {
        let response = components::text_field(ui, &P, field, &mut text, "Search", Some(icons::search));
        if !response.has_focus() {
            response.request_focus();
        }
    };
    let hint = offscreen.render(&mut draw);
    assert!(changed_pixels(&hint) > 500, "the field, its icon and the hint are drawn");
    let _ = offscreen.frame(vec![Event::Text("go".to_owned())], &mut draw);
    let _ = offscreen.frame(Vec::new(), &mut draw);
    assert_eq!(text, "go");
}

#[test]
fn writes_a_png() {
    let mut offscreen = renderer();
    let image = offscreen.render(|ui| {
        let _ = components::primary_button(ui, &P, pos2(200.0, 40.0), "Install", ui.scope_id().with("install"));
    });
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("button.png");
    offscreen::write_png(&path, &image).unwrap();
    let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    assert_eq!([info.width as usize, info.height as usize], image.size);
    let expected: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
    assert_eq!(&bytes[..info.buffer_size()], expected.as_slice());
}

#[test]
fn every_component_a_user_reaches_has_a_role_and_a_name() {
    let mut offscreen = Offscreen::new(vec2(420.0, 320.0), 1.0);
    P.apply(offscreen.context());
    let mut on = true;
    let mut text = String::new();
    let items = [Tab { label: "Tools".to_owned(), badge: Some(2) }, Tab { label: "Projects".to_owned(), badge: None }];
    let mut draw = |ui: &mut Ui| {
        let _ = components::primary_button(ui, &P, pos2(400.0, 20.0), "Install", ui.scope_id().with("install"));
        let style = IconButton { icon: icons::gear, icon_size: 16.0, color: P.text_secondary, radius: 8 };
        let gear = Rect::from_min_size(pos2(10.0, 40.0), vec2(30.0, 30.0));
        let _ =
            components::icon_button_named(ui, &P, gear, ui.scope_id().with("gear"), style, || "Settings".to_owned());
        let _ = components::tabs(ui, &P, Rect::from_min_size(pos2(10.0, 80.0), vec2(300.0, 40.0)), &items, 1);
        let field = Rect::from_min_size(pos2(10.0, 130.0), vec2(200.0, components::metrics::TEXT_FIELD));
        let _ = components::text_field(ui, &P, field, &mut text, "Search tools", Some(icons::search));
        ui.add_space(180.0);
        let _ = components::toggle(ui, &P, &mut on, "Enabled");
        let _ = components::list_row_named(ui, &P, 40.0, Sense::click(), || "GoGarden".to_owned());
    };
    let tree = offscreen.accessibility_tree(&mut draw);
    assert_eq!(zen_ui::accessibility::unnamed(&tree), Vec::<String>::new());
    let named = |role: Role, name: &str| {
        tree.nodes
            .iter()
            .find(|(_, node)| node.role() == role && node.label() == Some(name))
            .map(|(_, node)| node.clone())
    };
    assert!(named(Role::Button, "Install").is_some());
    assert!(named(Role::Button, "Settings").is_some());
    assert!(named(Role::TextInput, "Search tools").is_some(), "the hint names the field");
    assert!(named(Role::Switch, "Enabled").is_some());
    assert!(named(Role::ListItem, "GoGarden").is_some());
    let tools = named(Role::Tab, "Tools").unwrap();
    assert_eq!((tools.is_selected(), tools.description()), (Some(false), Some("2")));
    assert_eq!(named(Role::Tab, "Projects").unwrap().is_selected(), Some(true));
    assert!(tree.nodes.iter().any(|(_, node)| node.role() == Role::TabList));
}
