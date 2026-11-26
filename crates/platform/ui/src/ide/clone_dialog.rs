use std::path::{Path, PathBuf};

use egui::{Align, Color32, Layout, RichText, Role, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::new_project_dialog::{Outcome, pick_folder};
use super::settings::Dispatch;
use super::{IdePalette, dialog_frame, form};
use crate::{accessibility, theme};

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 500.0;
const LIST: f32 = 200.0;
const LABEL: f32 = 104.0;
const FIELD: f32 = 400.0;
const ROW: f32 = 32.0;
const DEFAULT_DEPTH: &str = "1";
const POLL: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Debug)]
pub(super) struct CloneDialog {
    urls: Vec<String>,
    url: String,
    directory: String,
    directory_edited: bool,
    shallow: bool,
    depth: String,
    problem: Option<String>,
    job: Option<u64>,
    phase: String,
    fraction: Option<f32>,
    error: Option<String>,
    cloned: Option<PathBuf>,
    pub(super) by_agent: bool,
}

impl CloneDialog {
    pub(super) fn open(dispatch: &mut Dispatch<'_>) -> Result<Self, String> {
        let defaults: Value = dispatch("project.fromVcs.defaults", json!({}))?;
        let urls = serde_json::from_value(defaults["urls"].clone()).map_err(|error| error.to_string())?;
        let mut dialog = Self {
            urls,
            url: String::new(),
            directory: defaults["directory"].as_str().unwrap_or_default().to_owned(),
            directory_edited: false,
            shallow: false,
            depth: DEFAULT_DEPTH.to_owned(),
            problem: None,
            job: None,
            phase: String::new(),
            fraction: None,
            error: None,
            cloned: None,
            by_agent: false,
        };
        dialog.check(dispatch);
        Ok(dialog)
    }

    fn check(&mut self, dispatch: &mut Dispatch<'_>) {
        let directory = self.directory_edited.then(|| self.directory.clone());
        match dispatch("project.fromVcs.check", json!({ "url": self.url, "directory": directory })) {
            Ok(answer) => {
                if !self.directory_edited {
                    self.directory = answer["suggested"].as_str().unwrap_or_default().to_owned();
                }
                self.problem = answer["problem"].as_str().map(str::to_owned);
            }
            Err(error) => self.problem = Some(error),
        }
    }

    pub(super) fn set_url(&mut self, dispatch: &mut Dispatch<'_>, url: String) {
        self.url = url;
        self.check(dispatch);
    }

    pub(super) fn set_directory(&mut self, dispatch: &mut Dispatch<'_>, directory: String) {
        self.directory = directory;
        self.directory_edited = true;
        self.check(dispatch);
    }

    #[cfg(test)]
    pub(super) fn set_shallow(&mut self, depth: Option<&str>) {
        self.shallow = depth.is_some();
        if let Some(depth) = depth {
            depth.clone_into(&mut self.depth);
        }
    }

    fn depth(&self) -> Option<u32> {
        self.shallow.then(|| self.depth.trim().parse().unwrap_or(0))
    }

    fn reason(&self) -> Option<String> {
        if self.depth() == Some(0) {
            return Some(t!("ide-clone-bad-depth"));
        }
        self.problem.clone()
    }

    pub(super) fn start(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.reason().is_some() || self.job.is_some() {
            return;
        }
        self.error = None;
        let params = json!({ "url": self.url, "directory": self.directory, "depth": self.depth() });
        match dispatch("project.fromVcs.clone", params) {
            Ok(answer) => self.job = answer["job"].as_u64(),
            Err(error) => self.error = Some(error),
        }
    }

    fn follow(&mut self, dispatch: &mut Dispatch<'_>) {
        let Some(job) = self.job else { return };
        let answer = match dispatch("project.new.status", json!({ "job": job })) {
            Ok(answer) => answer,
            Err(error) => {
                self.job = None;
                self.error = Some(error);
                return;
            }
        };
        self.phase = answer["progress"]["phase"].as_str().unwrap_or_default().to_owned();
        self.fraction = answer["progress"]["fraction"].as_f64().map(|fraction| fraction as f32);
        match answer["state"].as_str() {
            Some("running") => {}
            Some("ready") => {
                self.job = None;
                self.cloned = answer["path"].as_str().map(PathBuf::from);
            }
            _ => {
                self.job = None;
                self.error = Some(answer["error"].as_str().unwrap_or_default().to_owned());
            }
        }
    }

    fn cancel(&mut self, dispatch: &mut Dispatch<'_>) -> bool {
        match self.job.take() {
            Some(job) => {
                let _ = dispatch("project.new.cancel", json!({ "job": job }));
                false
            }
            None => true,
        }
    }

    pub(super) fn state(&self) -> Value {
        json!({
            "url": self.url,
            "directory": self.directory,
            "shallow": self.shallow,
            "depth": self.depth,
            "problem": self.reason(),
            "cloning": self.job.is_some(),
            "phase": self.phase,
            "error": self.error,
            "cloned": self.cloned,
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) -> Option<Outcome> {
        self.follow(dispatch);
        if self.job.is_some() {
            ui.ctx().request_repaint_after(POLL);
        }
        if let Some(path) = self.cloned.clone() {
            return ask_to_open(ui, p, &path);
        }
        let mut outcome = None;
        let size = ui.ctx().content_rect().size();
        let (width, height) = ((size.x - 80.0).clamp(560.0, WIDTH), (size.y - 80.0).clamp(360.0, HEIGHT));
        let modal = egui::Modal::new(ui.scope_id().with("clone")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(t!("ide-clone-title")).font(theme::semibold(15.0)).color(p.text));
            let footer = 66.0;
            let body = vec2(width, height - footer);
            ui.allocate_ui_with_layout(body, Layout::left_to_right(Align::Min), |ui| {
                ui.set_height(body.y - 28.0);
                self.sources(ui, p);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.set_width(width - LIST - 14.0);
                    self.fields(ui, p, dispatch);
                });
            });
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                self.footer(ui, p, dispatch, &mut outcome);
            });
        });
        if modal.should_close() && self.job.is_none() {
            outcome = Some(Outcome::Cancelled);
        }
        outcome
    }

    fn sources(&self, ui: &mut Ui, p: &IdePalette) {
        ui.vertical(|ui| {
            ui.set_width(LIST);
            let name = t!("ide-clone-repository-url");
            let (row, response) = ui.allocate_exact_size(vec2(LIST, ROW), Sense::hover());
            accessibility::item(&response, Role::ListBoxOption, true, &name);
            ui.painter().rect_filled(row, 6, p.selection);
            ui.painter().text(
                row.left_center() + vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                name,
                theme::regular(13.0),
                p.text,
            );
        });
    }

    fn fields(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let idle = self.job.is_none();
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-clone-version-control"), LABEL);
                form::combo(ui, p, "clone-vcs", &t!("ide-clone-version-control"), "Git", 120.0, |ui| {
                    let _ = ui.add(egui::Button::selectable(true, "Git"));
                });
            });
            let mut url = self.url.clone();
            let mut chosen = None;
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-clone-url"), LABEL);
                let field = form::framed(ui, p, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut url).desired_width(FIELD).id_salt("clone-url"))
                });
                accessibility::edit(&field, |node| node.set_label(t!("ide-clone-url")));
                if !self.urls.is_empty() {
                    form::combo(ui, p, "clone-urls", &t!("ide-clone-history"), "", 12.0, |ui| {
                        ui.set_min_width(FIELD);
                        for known in &self.urls {
                            if ui.add(egui::Button::selectable(false, known.as_str())).clicked() {
                                chosen = Some(known.clone());
                            }
                        }
                    });
                }
            });
            if chosen.is_some() || url != self.url {
                self.set_url(dispatch, chosen.unwrap_or(url));
            }
            let mut directory = self.directory.clone();
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-clone-directory"), LABEL);
                let field = form::framed(ui, p, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut directory).desired_width(FIELD).id_salt("clone-directory"))
                });
                accessibility::edit(&field, |node| node.set_label(t!("ide-clone-directory")));
                if field.changed() {
                    self.set_directory(dispatch, directory.clone());
                }
                if ui.button(t!("ide-new-project-browse")).clicked()
                    && let Some(folder) = pick_folder(Path::new(&self.directory))
                {
                    self.set_directory(dispatch, folder.to_string_lossy().into_owned());
                }
            });
            ui.horizontal(|ui| {
                form::check(ui, p, &mut self.shallow, &t!("ide-clone-shallow"));
                let field = form::framed(ui, p, |ui| {
                    let edit = egui::TextEdit::singleline(&mut self.depth).desired_width(48.0).id_salt("clone-depth");
                    ui.add_enabled(self.shallow, edit)
                });
                accessibility::edit(&field, |node| node.set_label(t!("ide-clone-shallow")));
                ui.label(RichText::new(t!("ide-clone-commits")).font(theme::regular(13.0)).color(p.text));
            });
        });
        let note = RichText::new(t!("ide-clone-network")).font(theme::regular(12.0));
        ui.label(note.color(p.text_secondary));
        if !idle {
            ui.add_space(6.0);
            let bar = egui::ProgressBar::new(self.fraction.unwrap_or(0.0)).desired_width(FIELD + LABEL);
            ui.add(if self.fraction.is_some() { bar } else { bar.animate(true) });
            ui.add(
                egui::Label::new(RichText::new(&self.phase).font(theme::regular(12.0)).color(p.text_secondary))
                    .truncate(),
            );
        }
    }

    fn footer(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>, outcome: &mut Option<Outcome>) {
        let reason = self.reason();
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ready = reason.is_none() && self.job.is_none();
                let clone = RichText::new(t!("ide-clone-clone")).font(theme::regular(13.0)).color(p.accent_text);
                let clone = egui::Button::new(clone).min_size(vec2(84.0, 30.0)).corner_radius(6).fill(if ready {
                    p.accent
                } else {
                    p.accent.gamma_multiply(0.4)
                });
                if ui.add_enabled(ready, clone).clicked() {
                    self.start(dispatch);
                }
                let cancel =
                    egui::Button::new(RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text))
                        .min_size(vec2(84.0, 30.0))
                        .corner_radius(6)
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() && self.cancel(dispatch) {
                    *outcome = Some(Outcome::Cancelled);
                }
            });
        });
        let line = self.error.clone().or_else(|| reason.filter(|_| !self.url.trim().is_empty()));
        if let Some(line) = line {
            ui.add(egui::Label::new(RichText::new(line).font(theme::regular(13.0)).color(p.error_text)).truncate());
        }
    }
}

fn ask_to_open(ui: &mut Ui, p: &IdePalette, path: &Path) -> Option<Outcome> {
    let mut outcome = None;
    let id = ui.scope_id().with("clone-open");
    let modal = egui::Modal::new(id).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        let question = t!("ide-clone-open", path = path.display().to_string());
        ui.add(egui::Label::new(RichText::new(question).font(theme::semibold(14.0)).color(p.text)).wrap());
        let note = RichText::new(t!("ide-clone-open-note")).font(theme::regular(13.0));
        ui.label(note.color(p.text_secondary));
        ui.add_space(8.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let label = RichText::new(t!("ide-clone-open-yes")).font(theme::regular(13.0));
            let open = egui::Button::new(label.color(p.accent_text)).corner_radius(6).fill(p.accent);
            if ui.add(open.min_size(vec2(84.0, 30.0))).clicked() {
                outcome = Some(Outcome::Created(path.to_owned()));
            }
            if ui.button(t!("ide-clone-open-no")).clicked() {
                outcome = Some(Outcome::Cancelled);
            }
        });
    });
    outcome.or_else(|| modal.should_close().then_some(Outcome::Cancelled))
}
