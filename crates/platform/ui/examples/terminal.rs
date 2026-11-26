use eframe::egui;
use zen_ui::terminal::TerminalView;

fn main() -> eframe::Result {
    zen_i18n::register(zen_ui::toolbox::LOCALES);
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    let mut args = std::env::args().skip(1);
    let options = match args.next() {
        Some(program) => {
            let rest: Vec<String> = args.collect();
            let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
            zen_terminal::Options::command(program, &rest, &cwd)
        }
        None => zen_terminal::Options::shell(&cwd),
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Terminal").with_inner_size([900.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "zen-terminal-example",
        native,
        Box::new(move |creation| {
            zen_ui::theme::install(&creation.egui_ctx);
            let view = TerminalView::with_options(options)
                .map_err(|message| -> Box<dyn std::error::Error + Send + Sync> { message.into() })?;
            Ok(Box::new(Example(view)))
        }),
    )
}

struct Example(TerminalView);

impl eframe::App for Example {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        zen_ui::capture::tick(ui.ctx());
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let _ = self.0.show(ui);
        });
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(self.0.title()));
    }
}
