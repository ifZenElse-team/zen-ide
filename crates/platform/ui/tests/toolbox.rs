#![cfg(not(target_arch = "wasm32"))]

use egui::{Color32, ColorImage, vec2};
use zen_core::{Registry, Session};
use zen_toolbox::Tools;
use zen_ui::offscreen::Offscreen;
use zen_ui::theme::Palette;
use zen_ui::toolbox::ToolboxView;

fn render_toolbox(scale: f32) -> ColorImage {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    zen_toolbox::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Tools::demo());
    let mut view = ToolboxView::new(Palette::DARK);
    let mut offscreen = Offscreen::new(vec2(432.0, 720.0), scale);
    Palette::DARK.apply(offscreen.context());
    offscreen.render(|ui| view.show(ui, &registry, &mut session)).into_image()
}

#[test]
fn toolbox_window_renders_offscreen_the_same_every_time() {
    let first = render_toolbox(1.0);
    assert_eq!(first.size, [432, 720]);
    let opaque = first.pixels.iter().filter(|pixel| pixel.a() == 255).count();
    assert!(opaque > first.pixels.len() / 2, "the window background is painted");
    let distinct: std::collections::HashSet<Color32> = first.pixels.iter().copied().collect();
    assert!(distinct.len() > 200, "rows, text and buttons are drawn");
    assert_eq!(first, render_toolbox(1.0));
}

#[test]
fn toolbox_window_renders_at_twice_the_scale() {
    assert_eq!(render_toolbox(2.0).size, [864, 1440]);
}

fn click(at: egui::Pos2) -> Vec<egui::Event> {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(at), button(true), button(false)]
}

fn centre_of(tree: &egui::accesskit::TreeUpdate, role: egui::accesskit::Role) -> Option<egui::Pos2> {
    let (_, node) = tree.nodes.iter().find(|(_, node)| node.role() == role)?;
    let bounds = node.bounds()?;
    Some(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
}

#[test]
fn the_settings_menu_switches_launch_at_login() {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let item = zen_toolbox::login::LoginItem {
        dir: dir.path().join("LaunchAgents"),
        executable: dir.path().join("zen-toolbox"),
        format: zen_toolbox::login::Format::LaunchAgent,
    };
    let mut registry = Registry::new();
    zen_toolbox::register(&mut registry).unwrap();
    let mut session = Session::new();
    let config =
        zen_toolbox::Config { login_item: Some(item.clone()), launch_at_login: true, ..zen_toolbox::Config::default() };
    session.set_state(Tools::new(config));
    assert!(item.is_registered());

    let mut view = ToolboxView::new(Palette::DARK);
    let mut offscreen = Offscreen::new(vec2(432.0, 720.0), 2.0);
    Palette::DARK.apply(offscreen.context());
    let _ = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let gear = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Settings…"))
        .and_then(|(_, node)| node.bounds())
        .map(|bounds| egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
        .expect("the settings button");
    offscreen.step(click(gear), |ui| view.show(ui, &registry, &mut session));
    let menu = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    if let Some(path) = std::env::var_os("ZEN_TOOLBOX_MENU_PNG") {
        zen_ui::offscreen::write_png(std::path::Path::new(&path), &menu).unwrap();
    }
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let switch = centre_of(&tree, egui::accesskit::Role::Switch).expect("the Launch at login switch");
    offscreen.step(click(switch), |ui| view.show(ui, &registry, &mut session));
    let _ = offscreen.render(|ui| view.show(ui, &registry, &mut session));

    assert_eq!(session.settings().get::<bool>("toolbox.launchAtLogin"), Some(false));
    assert!(!item.file().exists(), "the LaunchAgent is removed");
}

#[test]
fn every_control_of_the_toolbox_has_a_role_and_a_name() {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    zen_toolbox::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Tools::demo());
    let mut view = ToolboxView::new(Palette::DARK);
    let mut offscreen = Offscreen::new(vec2(432.0, 720.0), 1.0);
    Palette::DARK.apply(offscreen.context());
    let _ = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    assert_eq!(zen_ui::accessibility::unnamed(&tree), Vec::<String>::new());
    assert!(
        tree.nodes.iter().any(|(_, node)| node.role() == egui::accesskit::Role::Tab && node.label() == Some("Tools"))
    );
}

#[test]
fn the_projects_tab_shows_the_toolboxs_kozo() {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    zen_toolbox::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Tools::demo());
    let mut view = ToolboxView::new(Palette::DARK);
    let mut offscreen = Offscreen::new(vec2(432.0, 720.0), 2.0);
    Palette::DARK.apply(offscreen.context());
    let _ = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let projects = tree
        .nodes
        .iter()
        .find(|(_, node)| node.role() == egui::accesskit::Role::Tab && node.label() == Some("Projects"))
        .and_then(|(_, node)| node.bounds())
        .map(|bounds| egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
        .expect("the Projects tab");
    offscreen.step(click(projects), |ui| view.show(ui, &registry, &mut session));
    let image = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
        zen_ui::offscreen::write_png(&std::path::Path::new(&dir).join("toolbox-projects-kozo.png"), &image).unwrap();
    }
    let robe = |ink: zen_mascot::Ink| {
        let [r, g, b] = ink.rgb(zen_mascot::Variant::Toolbox).unwrap();
        image.pixels.iter().filter(|pixel| **pixel == Color32::from_rgb(r, g, b)).count()
    };
    assert!(robe(zen_mascot::Ink::Robe) > 16 * 40, "the indigo robe");
    assert!(robe(zen_mascot::Ink::Indigo) > 16 * 30, "the toolbox");
}

#[test]
fn settings_show_the_open_source_licences() {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    zen_toolbox::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(Tools::demo());
    let mut view = ToolboxView::new(Palette::DARK);
    view.open_settings(&session);
    let mut offscreen = Offscreen::new(vec2(432.0, 1200.0), 1.0);
    Palette::DARK.apply(offscreen.context());
    let _ = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let button = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Show Licences"))
        .and_then(|(_, node)| node.bounds())
        .map(|bounds| egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
        .expect("the Show Licences button");
    assert!(!view.licences_open());
    offscreen.step(click(button), |ui| view.show(ui, &registry, &mut session));
    let shown = offscreen.render(|ui| view.show(ui, &registry, &mut session));
    if let Some(path) = std::env::var_os("ZEN_TOOLBOX_LICENCES_PNG") {
        zen_ui::offscreen::write_png(std::path::Path::new(&path), &shown).unwrap();
    }
    assert!(view.licences_open() && view.has_dialog());
    let tree = offscreen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    assert!(tree.nodes.iter().any(|(_, node)| node.label() == Some("Close")), "the licences dialog is shown");
}
