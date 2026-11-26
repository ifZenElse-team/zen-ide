#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(clippy::print_stderr, reason = "start-up errors are reported on the terminal")]

use eframe::egui::{self, Pos2, Vec2, ViewportCommand};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_toolbox::{Channel, CommandLine, Config, Defaults, Source, StartNotes, Tools};
use zen_ui::theme::{self, Palette};
use zen_ui::toolbox::{LANGUAGE_SETTING, ToolboxView};
use zen_ui::tray::{Anchor, Tray, TrayEvent};

const SIZE: Vec2 = Vec2::new(432.0, 720.0);
const GAP: f32 = 6.0;

const RELEASES_URL: &str = "https://ide.ifzenelse.space/releases/manifest.json";
const RELEASES_PUBLIC_KEY: &str = "464c3ceb2e14dfdf571c14dbb4114f21b3ee9014a0cacca158933ed6dab9bf33";
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const CHECK_ON_OPEN_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

const VERSION: &str = match option_env!("ZEN_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

struct ToolboxApp {
    registry: Registry,
    session: Session,
    view: ToolboxView,
    tray: Option<Tray>,
    visible: bool,
    was_focused: bool,
    hidden_by_focus: Option<std::time::Instant>,
    waits_for_click: bool,
    settings_file: Option<std::path::PathBuf>,
    saved_settings: zen_core::Settings,
    first_show: Option<u32>,
    hide_at_start: u32,
}

const HIDE_AT_START_FRAMES: u32 = 3;

const OFF_SCREEN: Pos2 = Pos2::new(-20_000.0, -20_000.0);

const TOGGLE_AFTER_FOCUS_LOSS: std::time::Duration = std::time::Duration::from_millis(500);

const FIRST_SHOW_FRAMES: u32 = 60;

impl ToolboxApp {
    fn check_if_older_than(&mut self, age: std::time::Duration) {
        let due = self.session.state::<Tools>().is_some_and(|tools| tools.check_due(age));
        if due && self.registry.is_available(&self.session, "toolbox.releases.refresh") {
            let _ = self.registry.dispatch(&mut self.session, "toolbox.releases.refresh", serde_json::Value::Null);
        }
    }

    fn show(&mut self, ctx: &egui::Context, anchor: Option<Anchor>) {
        self.check_if_older_than(CHECK_ON_OPEN_AFTER);
        let position = placement(ctx, anchor);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.visible = true;
        self.was_focused = false;
        self.waits_for_click = false;
    }

    fn hide(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.visible = false;
    }
}

fn on_screen(ctx: &egui::Context, anchor: &Anchor) -> bool {
    let (scale, monitor) = ctx.input(|input| {
        let viewport = input.viewport();
        (viewport.native_pixels_per_point.unwrap_or(1.0), viewport.monitor_size)
    });
    let Some(monitor) = monitor else { return false };
    let bottom = (anchor.y + anchor.height) as f32 / scale;
    anchor.y >= 0.0 && bottom <= monitor.y + 1.0
}

fn placement(ctx: &egui::Context, anchor: Option<Anchor>) -> Pos2 {
    let (scale, monitor) = ctx.input(|input| {
        let viewport = input.viewport();
        (viewport.native_pixels_per_point.unwrap_or(1.0), viewport.monitor_size.unwrap_or(Vec2::new(1440.0, 900.0)))
    });
    let Some(anchor) = anchor else {
        return Pos2::new(monitor.x - SIZE.x - GAP * 2.0, 32.0);
    };
    let (x, y, width, height) =
        (anchor.x as f32 / scale, anchor.y as f32 / scale, anchor.width as f32 / scale, anchor.height as f32 / scale);
    let left = (x + width / 2.0 - SIZE.x / 2.0).clamp(GAP, (monitor.x - SIZE.x - GAP).max(GAP));
    let icon_bottom = if cfg!(target_os = "macos") { y } else { y + height };
    let top = if y < monitor.y / 2.0 { icon_bottom + GAP } else { y - SIZE.y - GAP };
    Pos2::new(left, top)
}

impl eframe::App for ToolboxApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_if_older_than(CHECK_EVERY);
        zen_toolbox::auto_update::follow(&mut self.session);
        if !zen_toolbox::notifications::follow(&mut self.session).is_empty() {
            self.save_settings_if_changed();
        }
        ctx.request_repaint_after(CHECK_EVERY);
        if self.hide_at_start > 0 {
            self.hide_at_start -= 1;
            self.hide(ctx);
            ctx.request_repaint();
        }
        if let Some(waited) = self.first_show {
            let anchor = self.tray.as_ref().and_then(Tray::anchor).filter(|anchor| on_screen(ctx, anchor));
            let monitor_known = ctx.input(|input| input.viewport().monitor_size.is_some());
            if (anchor.is_some() || self.tray.is_none()) && monitor_known || waited >= FIRST_SHOW_FRAMES {
                self.first_show = None;
                self.show(ctx, anchor);
            } else {
                self.first_show = Some(waited + 1);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        let events = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for event in events {
            match event {
                TrayEvent::Toggle(anchor) => {
                    let just_hidden =
                        self.hidden_by_focus.take().is_some_and(|at| at.elapsed() < TOGGLE_AFTER_FOCUS_LOSS);
                    if self.visible {
                        self.hide(ctx);
                    } else if !just_hidden {
                        self.show(ctx, anchor);
                    }
                }
                TrayEvent::Open => {
                    let anchor = self.tray.as_ref().and_then(Tray::anchor);
                    self.show(ctx, anchor);
                }
                TrayEvent::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }

        let (focused, escape) =
            ctx.input(|input| (input.viewport().focused.unwrap_or(false), input.key_pressed(egui::Key::Escape)));
        let escape = escape && !self.view.has_dialog();
        let lost = self.was_focused && !focused;
        let clicked_outside = self.waits_for_click
            && !focused
            && outside_input::mouse_down()
            && !ctx.input(|input| input.pointer.any_down());
        if lost && !outside_input::mouse_down() && !outside_input::command_down() {
            self.waits_for_click = true;
        } else if self.visible && self.tray.is_some() && (lost || clicked_outside || escape) {
            self.hide(ctx);
            self.waits_for_click = false;
            if !escape {
                self.hidden_by_focus = Some(std::time::Instant::now());
            }
        }
        if focused {
            self.waits_for_click = false;
        }
        if self.visible && self.waits_for_click {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.was_focused = focused;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.view.show(ui, &self.registry, &mut self.session);
        zen_ui::capture::tick(ui.ctx());
        self.save_settings_if_changed();
        if self.session.state::<Tools>().is_some_and(Tools::take_restart_request) {
            restart(ui.ctx());
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl ToolboxApp {
    fn save_settings_if_changed(&mut self) {
        if *self.session.settings() == self.saved_settings {
            return;
        }
        self.saved_settings = self.session.settings().clone();
        if let Some(path) = &self.settings_file {
            let written =
                serde_json::to_vec_pretty(&self.saved_settings).map_err(std::io::Error::other).and_then(|bytes| {
                    std::fs::create_dir_all(path.parent().unwrap_or(path))?;
                    std::fs::write(path, bytes)
                });
            if let Err(error) = written {
                eprintln!("zen-toolbox: cannot save settings to {}: {error}", path.display());
            }
        }
    }
}

fn load_settings(path: Option<&std::path::Path>) -> zen_core::Settings {
    path.and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn apply_language(settings: &zen_core::Settings) {
    let language: Option<String> = settings.get(LANGUAGE_SETTING);
    zen_i18n::set_language(language.as_deref());
}

fn restart(ctx: &egui::Context) {
    let args = std::env::args().skip(1).filter(|arg| arg != zen_toolbox::login::TRAY_ARGUMENT);
    let started =
        std::env::current_exe().and_then(|executable| std::process::Command::new(executable).args(args).spawn());
    if let Err(error) = started {
        eprintln!("zen-toolbox: cannot restart: {error}");
    }
    ctx.send_viewport_cmd(ViewportCommand::Close);
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|arg| arg == "--demo");
    let in_tray = args.iter().any(|arg| arg == zen_toolbox::login::TRAY_ARGUMENT);
    let palette = if args.iter().any(|arg| arg == "--light") { Palette::LIGHT } else { Palette::DARK };
    let source = match (option(&args, "--releases"), option(&args, "--trust-key")) {
        (Some(manifest_url), Some(public_key)) => Some(Source { manifest_url, public_key, channel: String::new() }),
        (None, None) => Some(Source {
            manifest_url: RELEASES_URL.to_owned(),
            public_key: RELEASES_PUBLIC_KEY.to_owned(),
            channel: String::new(),
        }),
        (Some(none), None) if none == "none" => None,
        _ => {
            eprintln!("zen-toolbox: --releases and --trust-key go together");
            std::process::exit(2);
        }
    };

    if let Ok(executable) = std::env::current_exe() {
        zen_update::install::remove_leftovers(&executable);
    }
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_toolbox::LOCALES);
    let channel = match option(&args, "--channel").map(|channel| Channel::parse(&channel)).transpose() {
        Ok(channel) => channel,
        Err(error) => {
            eprintln!("zen-toolbox: --channel: {error}");
            std::process::exit(2);
        }
    };
    let command_line = CommandLine { channel, tools_dir: option(&args, "--tools-dir").map(std::path::PathBuf::from) };
    let defaults = Defaults::for_this_user();
    let settings_file = zen_update::install::Installation::default_root()
        .and_then(|root| root.parent().map(|dir| dir.join("settings.json")));
    let settings = load_settings(settings_file.as_deref());
    let run = zen_toolbox::settings::for_this_run(&settings, &defaults, &command_line);
    let ran_before = settings_file.as_deref().is_some_and(std::path::Path::exists)
        || run.tools_dir.as_deref().is_some_and(std::path::Path::exists);
    apply_language(&settings);
    let last_shown: Option<String> = settings.get(zen_toolbox::WHATS_NEW_SETTING);
    let start_notes = if demo || option_env!("ZEN_BUILD_VERSION").is_none() {
        StartNotes::Nothing
    } else {
        zen_toolbox::notes_on_start(last_shown.as_deref(), VERSION, ran_before)
    };

    let mut registry = Registry::new();
    if let Err(error) = zen_toolbox::register(&mut registry) {
        eprintln!("zen-toolbox: cannot register commands: {error}");
        std::process::exit(1);
    }
    let mut session = Session::new();
    *session.settings_mut() = settings.clone();
    let mut view = ToolboxView::new(palette);
    match start_notes {
        StartNotes::Nothing => {}
        StartNotes::Remember => {
            let _ = session.settings_mut().set(zen_toolbox::WHATS_NEW_SETTING, VERSION);
        }
        StartNotes::Show { newer_than } => view.show_own_notes(newer_than),
    }
    let open_settings = args.iter().any(|arg| arg == "--settings");
    session.set_state(if demo {
        Tools::demo()
    } else {
        let mut config = Config {
            source,
            applications: zen_update::install::Installation::default_applications(),
            self_version: VERSION.to_owned(),
            self_executable: option_env!("ZEN_BUILD_VERSION").and_then(|_| std::env::current_exe().ok()),
            login_item: (option_env!("ZEN_BUILD_VERSION").is_some()
                || std::env::var_os(zen_toolbox::login::DIR_VARIABLE).is_some_and(|dir| !dir.is_empty()))
            .then(std::env::current_exe)
            .and_then(Result::ok)
            .and_then(zen_toolbox::login::LoginItem::for_this_user),
            command_line,
            defaults,
            ..Config::default()
        };
        config.apply(run);
        Tools::new(config)
            .with_notifier(std::sync::Arc::new(zen_toolbox::notifications::system))
            .with_running_programs(std::sync::Arc::new(zen_update::install::running_programs))
    });
    if open_settings {
        view.open_settings(&session);
    }

    let viewport = egui::ViewportBuilder::default();
    let viewport = if in_tray { viewport.with_position(OFF_SCREEN) } else { viewport };
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_title(if demo { "Zen Toolbox (demo data)" } else { "Zen Toolbox" })
            .with_app_id("space.ifzenelse.toolbox")
            .with_inner_size(SIZE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_visible(false),
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                builder.with_activation_policy(ActivationPolicy::Accessory);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = builder;
        })),
        ..Default::default()
    };
    eframe::run_native(
        "space.ifzenelse.toolbox",
        options,
        Box::new(move |creation| {
            let ctx = &creation.egui_ctx;
            theme::install(ctx);
            palette.apply(ctx);
            let tray = match Tray::new(
                ctx,
                "space.ifzenelse.toolbox.tray",
                &t!("toolbox-tray-tooltip"),
                &t!("toolbox-tray-open"),
                &t!("toolbox-tray-quit"),
            ) {
                Ok(tray) => Some(tray),
                Err(error) => {
                    eprintln!("zen-toolbox: no tray icon ({error}); running as a normal window");
                    None
                }
            };
            let has_tray = tray.is_some();
            let app = ToolboxApp {
                registry,
                session,
                view,
                tray,
                visible: false,
                was_focused: false,
                hidden_by_focus: None,
                waits_for_click: false,
                settings_file,
                saved_settings: settings,
                first_show: (!in_tray || !has_tray).then_some(0),
                hide_at_start: if in_tray && has_tray { HIDE_AT_START_FRAMES } else { 0 },
            };
            ctx.request_repaint();
            Ok(Box::new(app))
        }),
    )
}

mod outside_input {
    #[cfg(target_os = "macos")]
    pub(super) fn mouse_down() -> bool {
        objc2_app_kit::NSEvent::pressedMouseButtons() != 0
    }

    #[cfg(target_os = "macos")]
    pub(super) fn command_down() -> bool {
        objc2_app_kit::NSEvent::modifierFlags_class().contains(objc2_app_kit::NSEventModifierFlags::Command)
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn mouse_down() -> bool {
        true
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn command_down() -> bool {
        true
    }
}
