use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Anchor {
    pub fn is_placed(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayEvent {
    Toggle(Option<Anchor>),
    Open,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    events: Receiver<TrayEvent>,
}

impl Tray {
    pub fn new(
        ctx: &egui::Context,
        autosave: &str,
        tooltip: &str,
        open_label: &str,
        quit_label: &str,
    ) -> Result<Self, String> {
        let open = MenuItem::new(open_label, true, None);
        let quit = MenuItem::new(quit_label, true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit]).map_err(|error| error.to_string())?;

        let image = tray_image().map_err(|error| error.to_string())?;
        let builder = TrayIconBuilder::new().with_tooltip(tooltip).with_autosave_name(autosave);
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(image);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(image);
        let icon = builder
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|error| error.to_string())?;

        let (sender, events) = channel();
        forward_tray_events(sender.clone(), ctx.clone());
        forward_menu_events(sender, ctx.clone(), open.id().clone(), quit.id().clone());
        Ok(Self { icon, events })
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.icon.rect().map(anchor_of).filter(Anchor::is_placed)
    }

    pub fn poll(&self) -> Vec<TrayEvent> {
        self.events.try_iter().collect()
    }
}

fn anchor_of(rect: tray_icon::Rect) -> Anchor {
    Anchor { x: rect.position.x, y: rect.position.y, width: rect.size.width as f64, height: rect.size.height as f64 }
}

fn forward_tray_events(sender: Sender<TrayEvent>, ctx: egui::Context) {
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event
        {
            let anchor = Some(anchor_of(rect)).filter(Anchor::is_placed);
            let _ = sender.send(TrayEvent::Toggle(anchor));
            ctx.request_repaint();
        }
    }));
}

fn forward_menu_events(
    sender: Sender<TrayEvent>,
    ctx: egui::Context,
    open: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
) {
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let mapped = if event.id == open {
            TrayEvent::Open
        } else if event.id == quit {
            TrayEvent::Quit
        } else {
            return;
        };
        let _ = sender.send(mapped);
        ctx.request_repaint();
    }));
}

fn tray_image() -> Result<Icon, tray_icon::BadIcon> {
    Icon::from_rgba(tray_rgba(), 36, 36)
}

fn tray_rgba() -> Vec<u8> {
    const SIZE: u32 = 36;
    const SAMPLES: u32 = 4;
    let (left, top, span) = (23.0_f32, 25.0_f32, 82.0_f32);
    let rounded_rect = |x: f32, y: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), radius: f32| {
        let dx = (x0 + radius - x).max(x - (x1 - radius)).max(0.0);
        let dy = (y0 + radius - y).max(y - (y1 - radius)).max(0.0);
        x >= x0 && x <= x1 && y >= y0 && y <= y1 && dx * dx + dy * dy <= radius * radius
    };
    let inside = |x: f32, y: f32| {
        let sag = 3.1 * (1.0 - ((x - 64.0) / 27.4).powi(2)).max(0.0);
        let beam_top = 38.0 + sag;
        let beam = y >= beam_top
            && y <= beam_top + 6.9
            && ((35.9..=92.1).contains(&x)
                || (x - 35.9).powi(2) + (y - beam_top - 3.45).powi(2) <= 3.45 * 3.45
                || (x - 92.1).powi(2) + (y - beam_top - 3.45).powi(2) <= 3.45 * 3.45);
        let crossbar = rounded_rect(x, y, (39.36, 54.85, 88.64, 61.89), 3.52);
        let pillars = rounded_rect(x, y, (46.4, 44.0, 53.44, 94.27), 3.52)
            || rounded_rect(x, y, (74.56, 44.0, 81.6, 94.27), 3.52);
        beam || crossbar || pillars
    };
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let mut covered = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let fx = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / SIZE as f32;
                    let fy = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / SIZE as f32;
                    if inside(left + fx * span, top + fy * span) {
                        covered += 1;
                    }
                }
            }
            let alpha = covered as f32 / (SAMPLES * SAMPLES) as f32;
            rgba.extend_from_slice(&[0, 0, 0, (alpha * 255.0).round() as u8]);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_rectangles_are_not_positions() {
        let placeholder = Anchor { x: 0.0, y: 1964.0, width: 76.0, height: 0.0 };
        assert!(!placeholder.is_placed());
        assert!(Anchor { x: 2400.0, y: 0.0, width: 44.0, height: 48.0 }.is_placed());
    }

    #[test]
    fn the_tray_image_is_valid() {
        assert!(tray_image().is_ok());
    }

    #[test]
    fn the_tray_image_is_a_torii_with_an_open_middle() {
        let alpha = |x: u32, y: u32| -> u8 {
            let rgba = tray_rgba();
            rgba[((y * 36 + x) * 4 + 3) as usize]
        };
        assert_eq!(alpha(0, 35), 0, "a corner is transparent");
        assert_eq!(alpha(18, 24), 0, "the middle between the pillars is open");
        assert!(alpha(12, 24) > 200, "the left pillar is drawn");
        assert!(alpha(18, 14) > 200, "the crossbar is drawn");
    }
}
