use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Color32, Key, Stroke, Ui, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_update::scripts::PathStatus;
use zen_workbench::launcher::LauncherReport;
use zen_workbench::notifications::{Group, Kind, NewNotification};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug)]
pub(super) struct LauncherDialog {
    name: String,
    folder: String,
    add_to_path: bool,
    checked: Option<PathBuf>,
    status: Arc<Mutex<Option<PathStatus>>>,
    error: Option<String>,
}

pub(super) fn tilde(path: &Path) -> String {
    match zen_update::scripts::home().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn expand(folder: &str) -> PathBuf {
    zen_update::scripts::expand_home(Path::new(folder.trim()))
}

const LAUNCHER_CHECK_FOR: std::time::Duration = std::time::Duration::from_secs(30);

type LauncherCheck = (std::time::Instant, Option<Option<PathBuf>>);

static LAUNCHER_CHECKS: Mutex<Vec<(String, LauncherCheck)>> = Mutex::new(Vec::new());

pub(super) fn launcher_on_path(name: &str) -> Option<PathBuf> {
    let mut checks = LAUNCHER_CHECKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = match checks.iter().position(|(known, _)| known == name) {
        Some(index) => index,
        None => {
            checks.push((name.to_owned(), (std::time::Instant::now() - LAUNCHER_CHECK_FOR * 2, None)));
            checks.len() - 1
        }
    };
    let (started, answer) = &mut checks[index].1;
    if started.elapsed() > LAUNCHER_CHECK_FOR {
        *started = std::time::Instant::now();
        let name = name.to_owned();
        let _ = std::thread::Builder::new().name("zen-launcher-check".to_owned()).spawn(move || {
            let found = zen_update::scripts::launcher_on_path(&name, zen_update::scripts::home().as_deref());
            let mut checks = LAUNCHER_CHECKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((_, (_, answer))) = checks.iter_mut().find(|(known, _)| *known == name) {
                *answer = Some(found);
            }
        });
    }
    answer.clone().flatten()
}

fn recheck_launcher() {
    let mut checks = LAUNCHER_CHECKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (_, (started, _)) in checks.iter_mut() {
        *started = std::time::Instant::now() - LAUNCHER_CHECK_FOR * 2;
    }
}

pub(super) fn program_for_commands(executable: Option<PathBuf>, name: &str, found: Option<&Path>) -> Option<PathBuf> {
    if found.is_some() { Some(PathBuf::from(name)) } else { executable }
}

pub(super) fn shell_word(program: &Path) -> String {
    let text = program.display().to_string();
    if program.components().count() == 1 && !text.contains([' ', '"', '\'', '$', '`', '\\']) {
        text
    } else {
        zen_update::scripts::posix_quote(&text)
    }
}

pub(super) fn create_params(name: &str, folder: &str, add_to_path: bool) -> serde_json::Value {
    serde_json::json!({ "name": name.trim(), "folder": expand(folder), "addToPath": add_to_path })
}

impl LauncherDialog {
    pub(super) fn new(name: &str) -> Self {
        let folder =
            zen_update::scripts::default_location().map_or_else(|| "~/.local/bin".to_owned(), |path| tilde(&path));
        Self { name: name.to_owned(), folder, add_to_path: false, checked: None, status: Arc::default(), error: None }
    }

    fn check(&mut self, ctx: &egui::Context) {
        let folder = expand(&self.folder);
        if self.checked.as_ref() == Some(&folder) || !folder.is_absolute() {
            return;
        }
        self.checked = Some(folder.clone());
        let status = Arc::new(Mutex::new(None));
        self.status = Arc::clone(&status);
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("zen-launcher-path".to_owned()).spawn(move || {
            let answer = zen_update::scripts::path_status(&folder, zen_update::scripts::home().as_deref());
            *status.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(answer);
            ctx.request_repaint();
        });
    }

    fn status(&self) -> Option<PathStatus> {
        self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

enum Choice {
    Create,
    Cancel,
}

impl IdeView {
    pub(super) fn open_launcher_dialog(&mut self) {
        self.launcher_dialog = Some(LauncherDialog::new(self.product.id));
    }

    pub(super) fn launcher_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.launcher_dialog else { return };
        let p = self.palette;
        dialog.check(ui.ctx());
        let choice = show(ui, &p, dialog);
        match choice {
            Some(Choice::Cancel) => self.launcher_dialog = None,
            Some(Choice::Create) => {
                let params = create_params(&dialog.name, &dialog.folder, dialog.add_to_path);
                match registry.dispatch(session, "tools.commandLineLauncher.create", params) {
                    Ok(value) => {
                        self.launcher_dialog = None;
                        recheck_launcher();
                        if let Ok(report) = serde_json::from_value::<LauncherReport>(value) {
                            self.notify(created(&report));
                        }
                    }
                    Err(error) => {
                        dialog.error = Some(match error {
                            zen_core::DispatchError::Failed { error, .. } => error.message,
                            other => other.to_string(),
                        });
                    }
                }
            }
            None => {}
        }
    }
}

fn created(report: &LauncherReport) -> NewNotification {
    let script = tilde(&report.script);
    let name = report.script.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let body = if report.on_path {
        t!("ide-launcher-created-on-path", script = script.as_str(), name = name.as_str())
    } else if report.in_profile {
        t!("ide-launcher-created-new-window", script = script.as_str(), name = name.as_str())
    } else {
        t!("ide-launcher-created-not-on-path", script = script.as_str())
    };
    NewNotification::message(Group::General, Kind::Info, body).titled_by(zen_i18n::Message::new("ide-launcher-created"))
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut LauncherDialog) -> Option<Choice> {
    let mut choice = None;
    let status = dialog.status();
    let modal =
        egui::Modal::new(ui.scope_id().with("command-line-launcher")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            let text = |text: String, color: Color32| egui::RichText::new(text).font(theme::regular(13.0)).color(color);
            ui.label(egui::RichText::new(t!("ide-launcher-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add(egui::Label::new(text(t!("ide-launcher-intro"), p.text_secondary)).wrap());
            ui.add_space(6.0);
            egui::Grid::new("launcher-fields").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(text(t!("ide-launcher-name"), p.text));
                ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(340.0));
                ui.end_row();
                ui.label(text(t!("ide-launcher-folder"), p.text));
                ui.add(egui::TextEdit::singleline(&mut dialog.folder).desired_width(340.0));
                ui.end_row();
            });
            ui.add_space(4.0);
            match &status {
                None => {
                    ui.label(text(t!("ide-launcher-checking"), p.text_secondary));
                }
                Some(status) if status.on_path => {
                    let example = format!("{} .", dialog.name.trim());
                    ui.add(
                        egui::Label::new(text(t!("ide-launcher-on-path", example = example.as_str()), p.run)).wrap(),
                    );
                }
                Some(status) if status.in_profile => {
                    ui.add(egui::Label::new(text(t!("ide-launcher-in-profile"), p.text)).wrap());
                }
                Some(status) => {
                    ui.add(egui::Label::new(text(t!("ide-launcher-not-on-path"), p.warning_text)).wrap());
                    egui::Frame::new().fill(p.frame).corner_radius(6).inner_margin(8).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(egui::RichText::new(&status.line).monospace().color(p.text))
                                .selectable(true),
                        );
                    });
                    let files: Vec<String> = status.profiles.iter().map(|path| tilde(path)).collect();
                    let label = t!("ide-launcher-add-to-path", files = files.join(", "));
                    ui.checkbox(&mut dialog.add_to_path, text(label, p.text));
                }
            }
            if let Some(error) = &dialog.error {
                ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = egui::Button::new(
                    egui::RichText::new(t!("ide-launcher-ok")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(ok).clicked() {
                    choice = Some(Choice::Create);
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Create);
            }
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_commands_use_the_short_name_only_when_a_terminal_finds_the_launcher() {
        let exe = PathBuf::from("/Applications/GoGarden.app/Contents/MacOS/gogarden");
        let found = PathBuf::from("/Users/me/.local/bin/gogarden");
        let short = program_for_commands(Some(exe.clone()), "gogarden", Some(&found)).unwrap();
        assert_eq!(format!("{} mcp --headless .", shell_word(&short)), "gogarden mcp --headless .");
        let full = program_for_commands(Some(exe.clone()), "gogarden", None).unwrap();
        assert_eq!(
            format!("{} mcp --headless .", shell_word(&full)),
            r#""/Applications/GoGarden.app/Contents/MacOS/gogarden" mcp --headless ."#
        );
        assert_eq!(shell_word(Path::new("/Users/a b/gogarden")), r#""/Users/a b/gogarden""#);
    }

    #[test]
    fn ok_adds_the_path_line_only_when_ticked() {
        let params = create_params(" gogarden ", "/opt/zen bin", false);
        assert_eq!(params, serde_json::json!({ "name": "gogarden", "folder": "/opt/zen bin", "addToPath": false }));
        assert_eq!(create_params("gogarden", "/opt/bin", true)["addToPath"], true);
        let dialog = LauncherDialog::new("gogarden");
        assert!(!dialog.add_to_path, "the box starts unticked: nothing is added without the user's click");
        if let Some(home) = zen_update::scripts::home() {
            assert_eq!(dialog.folder, "~/.local/bin");
            assert_eq!(
                create_params("gogarden", "~/.local/bin", false)["folder"],
                serde_json::json!(home.join(".local/bin"))
            );
        }
    }
}
