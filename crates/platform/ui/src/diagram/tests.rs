use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton};

use super::*;
use crate::offscreen::Offscreen;

fn scene() -> Scene {
    let node = |x: f32, title: &str, rows: Vec<Row>| Node {
        rect: Rect::from_min_size(pos2(x, 0.0), vec2(180.0, 26.0 + rows.len() as f32 * 19.0 + 6.0)),
        title: title.to_owned(),
        rows,
        more: None,
        dashed: false,
        muted: false,
    };
    let row = |text: &str, detail: &str, marker| Row { text: text.to_owned(), detail: detail.to_owned(), marker };
    let customers = node(0.0, "customers", vec![row("id", "integer", Marker::Key), row("name", "text", Marker::None)]);
    let orders = node(
        300.0,
        "orders",
        vec![row("id", "integer", Marker::Key), row("customer_id", "integer", Marker::ForeignKey)],
    );
    let mut view = node(0.0, "big_orders", vec![row("id", "integer", Marker::None)]);
    view.rect = view.rect.translate(vec2(0.0, 120.0));
    view.dashed = true;
    Scene {
        nodes: vec![customers, orders, view],
        edges: vec![Edge {
            points: vec![pos2(300.0, 64.5), pos2(240.0, 64.5), pos2(240.0, 35.5), pos2(180.0, 35.5)],
            from: 1,
            to: 0,
            start: End::Many,
            end: End::ZeroOrOne,
            dashed: false,
        }],
        size: vec2(480.0, 177.0),
        header: 26.0,
        row: 19.0,
        padding: 8.0,
    }
}

fn palettes() -> [(&'static str, IdePalette); 2] {
    [
        ("dark", IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark())),
        ("light", IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light())),
    ]
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::default() }
}

fn pixel(image: &egui::ColorImage, at: Pos2, scale: f32) -> Color32 {
    image[((at.x * scale) as usize, (at.y * scale) as usize)]
}

#[test]
fn the_canvas_draws_boxes_and_lines_in_the_theme_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let scene = scene();
    for (name, palette) in palettes() {
        let mut screen = Offscreen::new(vec2(560.0, 240.0), 2.0);
        palette.apply(screen.context());
        let mut camera = Camera { zoom: 1.0, origin: pos2(-20.0, -20.0) };
        let image = screen.render(|ui| {
            let canvas = ui.max_rect();
            show(ui, &palette, canvas, (&scene, &mut camera, Some(1)), "diagram");
        });
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("diagram-canvas-{name}.png")), image.image()).unwrap();
        }
        assert_eq!(pixel(&image, pos2(5.0, 5.0), 2.0), palette.editor_background, "{name}: the background");
        assert_eq!(pixel(&image, pos2(150.0, 90.0), 2.0), palette.island, "{name}: inside a box");
        assert_eq!(pixel(&image, pos2(100.0, 185.0), 2.0), palette.island, "{name}: inside the view's box");
        assert_eq!(pixel(&image, pos2(260.0, 55.5), 2.0), palette.accent, "{name}: the selected box's line");
    }
}

#[test]
fn clicks_select_and_open_boxes_and_dragging_pans() {
    let scene = scene();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let mut screen = Offscreen::new(vec2(560.0, 240.0), 1.0);
    let mut camera = Camera { zoom: 1.0, origin: pos2(-20.0, -20.0) };
    let frame = |screen: &mut Offscreen, camera: &mut Camera, events: Vec<Event>| {
        let mut seen = Interaction::default();
        screen.step(events, |ui| {
            let canvas = ui.max_rect();
            seen = show(ui, &palette, canvas, (&scene, camera, None), "diagram");
        });
        seen
    };
    let idle = |screen: &mut Offscreen, camera: &mut Camera| {
        for _ in 0..30 {
            frame(screen, camera, Vec::new());
        }
    };
    frame(&mut screen, &mut camera, Vec::new());
    let on_orders = pos2(400.0, 60.0);
    let clicked = frame(
        &mut screen,
        &mut camera,
        vec![Event::PointerMoved(on_orders), press(on_orders, true), press(on_orders, false)],
    );
    assert_eq!((clicked.clicked, clicked.hovered), (Some(1), Some(1)));
    let opened = frame(&mut screen, &mut camera, vec![press(on_orders, true), press(on_orders, false)]);
    assert_eq!(opened.opened, Some(1), "a second click is a double click");
    idle(&mut screen, &mut camera);
    let empty = pos2(400.0, 200.0);
    let cleared =
        frame(&mut screen, &mut camera, vec![Event::PointerMoved(empty), press(empty, true), press(empty, false)]);
    assert!(cleared.cleared && cleared.clicked.is_none());
    idle(&mut screen, &mut camera);

    let start = pos2(450.0, 200.0);
    let end = start + vec2(-100.0, -20.0);
    frame(&mut screen, &mut camera, vec![Event::PointerMoved(start), press(start, true)]);
    frame(&mut screen, &mut camera, vec![Event::PointerMoved(start + vec2(-60.0, -10.0))]);
    frame(&mut screen, &mut camera, vec![Event::PointerMoved(end), press(end, false)]);
    let moved = camera.origin - pos2(-20.0, -20.0);
    assert!(
        moved.x > 40.0 && moved.x <= 100.0 && moved.y > 0.0 && moved.y <= 20.0,
        "the diagram follows the pointer: {moved:?}"
    );

    let canvas = Rect::from_min_size(Pos2::ZERO, vec2(560.0, 240.0));
    let anchor = pos2(200.0, 100.0);
    let fixed = camera.to_diagram(canvas, anchor);
    frame(&mut screen, &mut camera, vec![Event::PointerMoved(anchor), Event::Zoom(2.0)]);
    assert!((camera.zoom - 2.0).abs() < 1e-3, "{camera:?}");
    assert!((camera.to_diagram(canvas, anchor) - fixed).length() < 0.01, "zooming keeps the point under the pointer");
}

#[test]
fn the_camera_converts_both_ways_and_fits_the_scene() {
    let canvas = Rect::from_min_size(pos2(10.0, 20.0), vec2(400.0, 300.0));
    let camera = Camera { zoom: 0.5, origin: pos2(100.0, 50.0) };
    let point = pos2(123.0, 77.0);
    assert!((camera.to_diagram(canvas, camera.to_screen(canvas, point)) - point).length() < 1e-4);
    let fitted = fit(&scene(), canvas);
    let shown = fitted.visible(canvas);
    assert!(shown.contains(pos2(0.0, 0.0)) && shown.contains(scene().size.to_pos2()), "{shown:?}");
    assert!(fitted.zoom <= 1.0);
}

#[test]
fn a_far_zoom_draws_boxes_without_text_and_render_draws_everything() {
    let scene = scene();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    let mut screen = Offscreen::new(vec2(200.0, 100.0), 1.0);
    let camera = Camera { zoom: 0.2, origin: Pos2::ZERO };
    let image = screen.render(|ui| paint(ui.painter(), &palette, ui.max_rect(), &scene, &camera, None));
    assert_eq!(pixel(&image, pos2(20.0, 8.0), 1.0), palette.island);
    let whole = render(&scene, &palette, 2.0);
    assert_eq!(whole.size, [(480.0 + 32.0) as usize * 2, (177.0 + 32.0) as usize * 2]);
    assert_eq!(pixel(&whole, pos2(4.0, 4.0), 2.0), palette.editor_background);
    assert_eq!(pixel(&whole, pos2(16.0 + 150.0, 16.0 + 70.0), 2.0), palette.island);
}
