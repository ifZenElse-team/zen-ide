#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(clippy::print_stderr, reason = "start-up errors are reported on the terminal")]

use std::sync::Arc;

use zen_core::Registry;
use zen_modules::BuiltinModule;
use zen_ui::ide::{IdeApp, IdeProduct, Program, Start};
use zen_ui::plugins::PluginConfig;

const APP_ID: &str = "space.ifzenelse.webwave";

const VERSION: &str = match option_env!("ZEN_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

struct WebWave(IdeApp);

impl eframe::App for WebWave {
    fn logic(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn main() -> eframe::Result {
    let program = Program { name: "WebWave", id: "webwave", app_id: APP_ID, version: VERSION };
    let start = match zen_ui::ide::read_command_line(&program, std::env::args_os().skip(1)) {
        Ok(start) => start,
        Err(code) => std::process::exit(code),
    };
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);

    let mut registry = Registry::new();
    if let Err(error) = zen_workbench::register(&mut registry) {
        eprintln!("webwave: cannot register commands: {error}");
        std::process::exit(1);
    }
    let recent = zen_workbench::recent::default_file(APP_ID);
    let plugins = zen_workbench::recent::default_file(APP_ID)
        .and_then(|file| Some(file.parent()?.join("plugins")))
        .map(|folder| {
            let catalog =
                std::env::var("ZEN_PLUGINS_CATALOG").unwrap_or_else(|_| zen_update::OFFICIAL_PLUGINS_URL.to_owned());
            PluginConfig {
                folder,
                catalog_url: (catalog != "none").then_some(catalog),
                bundled: zen_ui::plugins::bundled_folder(),
                optional: Vec::new(),
                public_key: std::env::var("ZEN_TRUST_KEY")
                    .unwrap_or_else(|_| zen_update::OFFICIAL_PUBLIC_KEY.to_owned()),
            }
        });

    let product = IdeProduct {
        name: "WebWave",
        id: "webwave",
        version: VERSION,
        modules: vec![
            BuiltinModule::off(&zen_lang_go::MODULE),
            BuiltinModule::off(&zen_lang_rust::MODULE),
            BuiltinModule::on(&zen_workbench::database::MODULE),
        ],
        plugins,
        mcp_port: 63532,
        ..IdeProduct::default()
    };
    let open = match start {
        Start::Headless(options) => std::process::exit(zen_ui::ide::run_headless(product, registry, recent, options)),
        Start::Window(open) => open,
    };
    let mut app = IdeApp::new(product, registry, recent, open);
    if let Some(port) =
        std::env::var("ZEN_AUTOMATION").ok().and_then(|value| zen_ui::ide::automation_port(&value, 63532))
    {
        app.enable_automation(port);
    }
    let mut viewport = eframe::egui::ViewportBuilder::default();
    if let Some(icon) = zen_ui::icons::app_icon("webwave") {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_title("WebWave")
            .with_app_id(APP_ID)
            .with_min_inner_size([640.0, 480.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    let options = eframe::NativeOptions { viewport: app.initial_viewport(options.viewport), ..options };
    zen_ui::keypad::run_native(
        APP_ID,
        options,
        Box::new(move |creation| {
            zen_ui::theme::install(&creation.egui_ctx);
            app.palette().apply(&creation.egui_ctx);
            Ok(Box::new(WebWave(app)))
        }),
    )
}
