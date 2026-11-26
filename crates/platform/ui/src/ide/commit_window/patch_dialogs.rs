use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::patch::{PatchDiffView, PatchFile, PatchOf, PatchPreview};

use super::dialogs::{DialogFile, FilePicker, Pressed, buttons, error_label, file_picker, preview, title_label};
use super::{CommitTab, checkbox, status_named};
use crate::ide::vcs::status_color;
use crate::ide::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const ENCODINGS: [&str; 7] = ["UTF-8", "windows-1251", "KOI8-R", "ISO-8859-1", "windows-1252", "Shift_JIS", "GBK"];

#[derive(Debug)]
pub(in crate::ide) struct CreateDialog {
    source: PatchOf,
    revision: Option<String>,
    shelf: Option<u64>,
    subject: String,
    picker: Option<FilePicker>,
    to_file: bool,
    file: String,
    base: String,
    reverse: bool,
    encoding: String,
    error: Option<String>,
}

#[derive(Debug)]
pub(in crate::ide) struct ApplyDialog {
    path: String,
    text: Option<String>,
    base_dir: String,
    strip: Option<usize>,
    reverse: bool,
    encoding: String,
    preview: Option<PatchPreview>,
    checked: BTreeSet<String>,
    selected: Option<String>,
    diff: Option<PatchDiffView>,
    stale: bool,
    diff_stale: bool,
    lists: Vec<String>,
    target: Option<String>,
    new_list: String,
    import: bool,
    error: Option<String>,
}

#[derive(Debug)]
pub(in crate::ide) enum PatchDialog {
    Create(Box<CreateDialog>),
    Apply(Box<ApplyDialog>),
}

impl PatchDialog {
    pub(in crate::ide) fn name(&self) -> &'static str {
        match self {
            PatchDialog::Create(_) => "createPatch",
            PatchDialog::Apply(_) => "applyPatch",
        }
    }
}

fn file_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(
            |character| {
                if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') { character } else { '_' }
            },
        )
        .collect();
    let cleaned = cleaned.trim_matches('_');
    let cleaned: String = cleaned.chars().take(60).collect();
    if cleaned.is_empty() { "changes".to_owned() } else { cleaned }
}

impl ApplyDialog {
    fn input(&self) -> Value {
        let mut input = json!({ "reverse": self.reverse, "encoding": self.encoding });
        match &self.text {
            Some(text) => input["text"] = json!(text),
            None => input["path"] = json!(self.path.trim()),
        }
        if !self.base_dir.trim().is_empty() {
            input["baseDir"] = json!(self.base_dir.trim());
        }
        if let Some(strip) = self.strip {
            input["strip"] = json!(strip);
        }
        input
    }

    fn read(&mut self, registry: &Registry, session: &mut Session) {
        self.stale = false;
        self.diff_stale = true;
        match registry.dispatch(session, "vcs.patch.read", self.input()) {
            Ok(value) => {
                let preview: PatchPreview = serde_json::from_value(value).unwrap_or_default();
                let fresh = self.preview.is_none();
                let names: BTreeSet<String> = preview.files.iter().map(|file| file.relative.clone()).collect();
                if fresh {
                    self.checked = preview
                        .files
                        .iter()
                        .filter(|file| !matches!(file.state.as_str(), "missing" | "failed"))
                        .map(|file| file.relative.clone())
                        .collect();
                } else {
                    self.checked.retain(|path| names.contains(path));
                }
                if self.selected.as_ref().is_none_or(|selected| !names.contains(selected)) {
                    self.selected = preview.files.first().map(|file| file.relative.clone());
                }
                if self.strip.is_none() || fresh {
                    self.strip = Some(preview.strip);
                }
                self.preview = Some(preview);
                self.error = None;
            }
            Err(error) => {
                self.preview = None;
                self.diff = None;
                self.error = Some(dispatch_message(error));
            }
        }
    }

    fn read_diff(&mut self, registry: &Registry, session: &mut Session) {
        self.diff_stale = false;
        self.diff = self.selected.as_ref().and_then(|file| {
            let mut params = self.input();
            params["file"] = json!(file);
            serde_json::from_value(registry.dispatch(session, "vcs.patch.diff", params).ok()?).ok()
        });
    }

    fn selected_file(&self) -> Option<&PatchFile> {
        let selected = self.selected.as_ref()?;
        self.preview.as_ref()?.files.iter().find(|file| &file.relative == selected)
    }
}

pub(in crate::ide) fn is_patch_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("patch") || extension.eq_ignore_ascii_case("diff"))
}

fn dispatch_message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

impl IdeView {
    pub(in crate::ide) fn open_create_patch(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        paths: Vec<PathBuf>,
    ) {
        self.refresh_changes(registry, session);
        let window = &self.commit_window;
        let files: Vec<DialogFile> = window.changes.iter().map(DialogFile::from).collect();
        let checked: BTreeSet<PathBuf> =
            if paths.is_empty() { window.checked_paths() } else { paths }.into_iter().collect();
        let subject = checked
            .iter()
            .find_map(|path| window.list_of(path))
            .map_or_else(|| window.active_list(), |list| list.name.clone());
        self.patch_dialog = Some(PatchDialog::Create(Box::new(self.create_dialog(
            PatchOf::Changes,
            subject,
            Some(FilePicker::new(files, checked)),
        ))));
    }

    pub(in crate::ide) fn open_create_commit_patch(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        revision: &str,
    ) {
        let details = registry.dispatch(session, "vcs.commit.details", json!({ "id": revision })).ok();
        let subject = details
            .as_ref()
            .and_then(|details| details["subject"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| revision.chars().take(8).collect());
        let mut dialog = self.create_dialog(PatchOf::Commit, subject, None);
        dialog.revision = Some(revision.to_owned());
        self.patch_dialog = Some(PatchDialog::Create(Box::new(dialog)));
    }

    pub(in crate::ide) fn open_create_shelf_patch(&mut self, registry: &Registry, session: &mut Session, id: u64) {
        self.refresh_shelf(registry, session, true);
        let Some(shelf) = self.commit_window.shelf.shelves.iter().find(|shelf| shelf.id == id) else { return };
        let mut dialog = self.create_dialog(PatchOf::Shelf, shelf.name.clone(), None);
        dialog.shelf = Some(id);
        self.patch_dialog = Some(PatchDialog::Create(Box::new(dialog)));
    }

    fn create_dialog(&self, source: PatchOf, subject: String, picker: Option<FilePicker>) -> CreateDialog {
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let file = root.join(format!("{}.patch", file_name(&subject)));
        CreateDialog {
            source,
            revision: None,
            shelf: None,
            subject,
            picker,
            to_file: true,
            file: file.display().to_string(),
            base: root.display().to_string(),
            reverse: false,
            encoding: "UTF-8".to_owned(),
            error: None,
        }
    }

    pub(in crate::ide) fn open_apply_patch(&mut self, path: Option<PathBuf>, text: Option<String>) {
        let lists = self.commit_window.list_names();
        let target = Some(self.commit_window.active_list());
        self.patch_dialog = Some(PatchDialog::Apply(Box::new(ApplyDialog {
            path: path.map(|path| path.display().to_string()).unwrap_or_default(),
            text,
            base_dir: String::new(),
            strip: None,
            reverse: false,
            encoding: "UTF-8".to_owned(),
            preview: None,
            checked: BTreeSet::new(),
            selected: None,
            diff: None,
            stale: true,
            diff_stale: true,
            lists,
            target,
            new_list: String::new(),
            import: false,
            error: None,
        })));
    }

    pub(in crate::ide) fn copy_patch(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        paths: Vec<PathBuf>,
        shelf: Option<u64>,
    ) -> Result<Value, zen_core::DispatchError> {
        let params = match shelf {
            Some(id) => json!({ "source": "shelf", "shelf": id, "clipboard": true }),
            None => {
                let paths = if paths.is_empty() { self.commit_window.checked_paths() } else { paths };
                json!({ "paths": paths, "clipboard": true })
            }
        };
        let result = registry.dispatch(session, "vcs.patch.create", params)?;
        let count = result["files"].as_array().map_or(0, Vec::len);
        self.post_at(ctx, t!("vcs-patch-copied", count = count));
        Ok(result)
    }

    pub(in crate::ide) fn dropped_patch(&mut self, ui: &Ui) {
        let area = ui.max_rect();
        if ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| !area.contains(pointer)) {
            return;
        }
        let patch = ui.input(|input| {
            input.raw.dropped_files.iter().map(|file| file.path().to_owned()).find(|path| is_patch_file(path))
        });
        let Some(path) = patch else { return };
        if self.vcs.info.root.is_none() {
            return;
        }
        ui.ctx().input_mut(|input| input.raw.dropped_files.retain(|file| !is_patch_file(file.path())));
        self.open_apply_patch(Some(path), None);
    }

    pub(in crate::ide) fn patch_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
        agent: bool,
    ) -> Result<Value, crate::ide::agent::ActionError> {
        use crate::ide::agent::ActionError::{Dispatch, Refused};
        let root = self.vcs.info.root.clone().unwrap_or_default();
        match action {
            "view.vcs.createPatch" => {
                if let Some(revision) = params.get("revision").and_then(Value::as_str) {
                    self.open_create_commit_patch(registry, session, revision);
                } else if let Some(id) = params.get("shelf").and_then(Value::as_u64) {
                    self.open_create_shelf_patch(registry, session, id);
                    if self.patch_dialog.is_none() {
                        return Err(Refused(format!("there are no shelved changes with the id {id}")));
                    }
                } else {
                    let given = self.given_paths(params);
                    if given.is_empty() && self.commit_window.checked_paths().is_empty() {
                        return Err(Refused("there are no changes to put in a patch".to_owned()));
                    }
                    self.open_create_patch(registry, session, given);
                }
                Ok(Value::Null)
            }
            "view.vcs.copyPatch" => {
                let given = self.given_paths(params);
                let shelf = params.get("shelf").and_then(Value::as_u64);
                self.copy_patch(ctx, registry, session, given, shelf).map_err(Dispatch)
            }
            "view.vcs.applyPatch" => {
                let path = match params.get("path").and_then(Value::as_str) {
                    Some(path) => Some(PathBuf::from(path)),
                    None if agent => {
                        return Err(Refused(
                            "Apply Patch… asks for the patch file in a system dialog; give its `path`".to_owned(),
                        ));
                    }
                    None => browse(&t!("vcs-patch-apply-title"), None),
                };
                let Some(path) = path else { return Ok(Value::Null) };
                let path = if path.is_absolute() { path } else { root.join(path) };
                self.open_apply_patch(Some(path), None);
                Ok(Value::Null)
            }
            _ => {
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| zen_workbench::file_clipboard::clipboard_text(session));
                if text.trim().is_empty() {
                    return Err(Refused("the clipboard has no patch".to_owned()));
                }
                self.open_apply_patch(None, Some(text));
                Ok(Value::Null)
            }
        }
    }

    pub(in crate::ide) fn patch_frame(&mut self, registry: &Registry, session: &mut Session) {
        if let Some(revision) = session.state_mut::<zen_workbench::vcs::patch::PatchDialogRequest>().take() {
            self.open_create_commit_patch(registry, session, &revision);
        }
        let draft = session.state::<zen_workbench::vcs::patch::CommitDraft>().cloned().unwrap_or_default();
        if draft.generation != self.commit_window.draft_generation {
            self.commit_window.draft_generation = draft.generation;
            if let Some(message) = draft.message {
                self.commit_window.message = message;
            }
            self.commit_window.author = draft.author;
        }
    }

    pub(in crate::ide) fn patch_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.patch_frame(registry, session);
        let Some(mut dialog) = self.patch_dialog.take() else { return };
        let p = self.palette;
        let root = self.vcs.info.root.clone().unwrap_or_default();
        if let PatchDialog::Apply(apply) = &mut dialog {
            if apply.stale {
                apply.read(registry, session);
            }
            if apply.diff_stale {
                apply.read_diff(registry, session);
            }
        }
        let id = ui.scope_id().with(("vcs-patch-dialog", dialog.name()));
        let mut run: Option<(&'static str, Value)> = None;
        let mut close = false;
        let modal = egui::Modal::new(id).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let pressed = match &mut dialog {
                PatchDialog::Create(create) => create_body(ui, &p, create, &root),
                PatchDialog::Apply(apply) => apply_body(ui, &p, apply, &root),
            };
            match pressed {
                Pressed::Confirm => run = Some(confirm(&dialog)),
                Pressed::Cancel => close = true,
                Pressed::None => {}
            }
        });
        if close || (run.is_none() && modal.should_close()) {
            return;
        }
        if let Some((command, params)) = run {
            match registry.dispatch(session, command, params) {
                Ok(result) => {
                    self.after_patch_dialog(ui, &dialog, &result);
                    return;
                }
                Err(error) => {
                    let message = dispatch_message(error);
                    match &mut dialog {
                        PatchDialog::Create(create) => create.error = Some(message),
                        PatchDialog::Apply(apply) => apply.error = Some(message),
                    }
                }
            }
        }
        self.patch_dialog = Some(dialog);
    }

    fn after_patch_dialog(&mut self, ui: &Ui, dialog: &PatchDialog, result: &Value) {
        match dialog {
            PatchDialog::Create(_) => {
                let count = result["files"].as_array().map_or(0, Vec::len);
                if result["clipboard"].as_bool() == Some(true) {
                    self.post(ui, t!("vcs-patch-copied", count = count));
                } else {
                    let path = result["path"].as_str().unwrap_or_default().to_owned();
                    self.post(ui, t!("vcs-patch-created", count = count, path = path));
                }
            }
            PatchDialog::Apply(apply) if apply.import => {
                self.commit_window.shelf.stale = true;
                self.show_commit_tab(CommitTab::Shelf);
                let name = result["name"].as_str().unwrap_or_default().to_owned();
                self.post(ui, t!("vcs-patch-imported", name = name));
            }
            PatchDialog::Apply(_) => {
                self.commit_window.revision = None;
                self.commit_window.tab = CommitTab::Changes;
                let applied = result["applied"].as_array().map_or(0, Vec::len);
                let conflicts = result["conflicts"].as_array().map_or(0, Vec::len);
                if conflicts == 0 {
                    self.post(ui, t!("vcs-patch-applied", count = applied));
                }
            }
        }
    }
}

fn confirm(dialog: &PatchDialog) -> (&'static str, Value) {
    match dialog {
        PatchDialog::Create(create) => {
            let mut params = json!({
                "source": create.source,
                "reverse": create.reverse,
                "encoding": create.encoding,
                "basePath": create.base.trim(),
                "message": create.subject,
            });
            if create.to_file {
                params["output"] = json!(create.file.trim());
            } else {
                params["clipboard"] = json!(true);
            }
            if let Some(picker) = &create.picker {
                params["paths"] = json!(picker.checked());
            }
            if let Some(revision) = &create.revision {
                params["revision"] = json!(revision);
            }
            if let Some(shelf) = create.shelf {
                params["shelf"] = json!(shelf);
            }
            ("vcs.patch.create", params)
        }
        PatchDialog::Apply(apply) => {
            let mut params = apply.input();
            let files: Vec<&String> = apply
                .preview
                .iter()
                .flat_map(|preview| &preview.files)
                .map(|file| &file.relative)
                .filter(|path| apply.checked.contains(*path))
                .collect();
            params["paths"] = json!(files);
            if apply.import {
                return ("vcs.shelf.import", params);
            }
            params["changelist"] = match &apply.target {
                Some(list) => json!(list),
                None => json!(apply.new_list.trim()),
            };
            ("vcs.patch.apply", params)
        }
    }
}

fn row_label(ui: &mut Ui, p: &IdePalette, text: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, text, theme::regular(13.0), p.text);
}

fn field(ui: &mut Ui, text: &mut String, id: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(text).id_salt(id).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)),
    )
}

fn encoding_combo(ui: &mut Ui, id: &str, encoding: &mut String) -> bool {
    let before = encoding.clone();
    egui::ComboBox::from_id_salt(id).selected_text(encoding.as_str()).width(160.0).show_ui(ui, |ui| {
        for name in ENCODINGS {
            ui.selectable_value(encoding, name.to_owned(), name);
        }
    });
    *encoding != before
}

#[cfg(not(target_arch = "wasm32"))]
fn browse(title: &str, save: Option<&str>) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let dialog = rfd::FileDialog::new().set_title(title).add_filter("Patch", &["patch", "diff"]);
    match save {
        Some(name) => dialog.set_file_name(name).save_file(),
        None => dialog.pick_file(),
    }
}

#[cfg(target_arch = "wasm32")]
fn browse(_title: &str, _save: Option<&str>) -> Option<PathBuf> {
    None
}

fn create_body(ui: &mut Ui, p: &IdePalette, create: &mut CreateDialog, root: &Path) -> Pressed {
    ui.set_width(640.0);
    let title = match create.source {
        PatchOf::Changes => t!("vcs-patch-create-title"),
        PatchOf::Commit | PatchOf::Shelf => t!("vcs-patch-create-title-of", name = create.subject.as_str()),
    };
    title_label(ui, p, &title);
    if let Some(picker) = &mut create.picker {
        file_picker(ui, p, picker, root);
        ui.add_space(12.0);
    }
    let width = 120.0;
    ui.horizontal(|ui| {
        ui.radio_value(&mut create.to_file, true, t!("vcs-patch-save-to-file"));
        ui.add_space(8.0);
        let browse_width = 90.0;
        ui.allocate_ui(vec2(ui.available_width() - browse_width, 28.0), |ui| {
            field(ui, &mut create.file, "vcs-patch-file", create.to_file);
        });
        if ui.add_enabled(create.to_file, egui::Button::new(t!("vcs-patch-browse"))).clicked()
            && let Some(file) =
                browse(&t!("vcs-patch-save-to-file"), Some(&format!("{}.patch", file_name(&create.subject))))
        {
            create.file = file.display().to_string();
        }
    });
    ui.add_space(6.0);
    ui.radio_value(&mut create.to_file, false, t!("vcs-patch-copy-to-clipboard"));
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        row_label(ui, p, &t!("vcs-patch-base-path"), width);
        field(ui, &mut create.base, "vcs-patch-base", true);
    });
    ui.add_space(8.0);
    ui.checkbox(&mut create.reverse, t!("vcs-patch-reverse"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        row_label(ui, p, &t!("vcs-patch-encoding"), width);
        ui.add_enabled_ui(create.to_file, |ui| encoding_combo(ui, "vcs-patch-create-encoding", &mut create.encoding));
    });
    error_label(ui, p, create.error.as_deref());
    let enter = ui.memory(|memory| memory.focused().is_none()) && ui.input(|input| input.key_pressed(Key::Enter));
    let ready = create.picker.as_ref().is_none_or(|picker| !picker.checked().is_empty());
    match buttons(ui, p, &t!("vcs-patch-create-confirm"), enter) {
        Pressed::Confirm if !ready => Pressed::None,
        other => other,
    }
}

fn state_color(p: &IdePalette, state: &str) -> egui::Color32 {
    match state {
        "conflict" | "exists" | "failed" => p.vcs_conflict,
        "missing" => p.text_secondary,
        _ => p.text_secondary,
    }
}

fn apply_files(ui: &mut Ui, p: &IdePalette, apply: &mut ApplyDialog, root: &Path) {
    let files: Vec<PatchFile> = apply.preview.as_ref().map(|preview| preview.files.clone()).unwrap_or_default();
    egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(120.0);
        egui::ScrollArea::vertical().id_salt("vcs-patch-files").max_height(170.0).show(ui, |ui| {
            for file in &files {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                let on = apply.checked.contains(&file.relative);
                let selected = apply.selected.as_ref() == Some(&file.relative);
                if selected {
                    ui.painter().rect_filled(row, 4, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(row, 4, p.hover);
                }
                let check = Rect::from_center_size(pos2(row.left() + 12.0, row.center().y), Vec2::splat(14.0));
                let check_response =
                    ui.interact(check.expand(3.0), ui.scope_id().with(("patch-check", &file.relative)), Sense::click());
                checkbox(ui.painter(), check, Some(on), p);
                check_response
                    .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &file.relative));
                crate::accessibility::item(&response, egui::Role::ListItem, selected, &file.relative);
                let name = file.relative.rsplit('/').next().unwrap_or_default();
                crate::file_icons::file_icon(
                    ui.painter(),
                    Rect::from_center_size(pos2(row.left() + 34.0, row.center().y), Vec2::splat(15.0)),
                    name,
                );
                let painter = ui.painter().with_clip_rect(row);
                let conflicted = matches!(file.state.as_str(), "conflict" | "exists" | "failed" | "missing");
                let color = if conflicted { p.vcs_conflict } else { status_color(p, status_named(&file.status)) };
                let label = painter.text(
                    pos2(row.left() + 48.0, row.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    theme::regular(13.0),
                    color,
                );
                let folder = Path::new(&file.relative).parent().filter(|folder| !folder.as_os_str().is_empty());
                if let Some(folder) = folder {
                    painter.text(
                        pos2(label.right() + 7.0, row.center().y),
                        Align2::LEFT_CENTER,
                        root.join(folder).strip_prefix(root).unwrap_or(folder).display().to_string(),
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                }
                let state = match file.state.as_str() {
                    "clean" | "binary" => None,
                    state => Some(t!(&format!("vcs-patch-state-{state}"))),
                };
                if let Some(state) = state {
                    painter.text(
                        pos2(row.right() - 8.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        state,
                        theme::regular(12.0),
                        state_color(p, &file.state),
                    );
                }
                if check_response.clicked() {
                    if on {
                        apply.checked.remove(&file.relative);
                    } else {
                        apply.checked.insert(file.relative.clone());
                    }
                } else if response.clicked() && !selected {
                    apply.selected = Some(file.relative.clone());
                    apply.diff_stale = true;
                }
            }
        });
    });
}

fn apply_body(ui: &mut Ui, p: &IdePalette, apply: &mut ApplyDialog, root: &Path) -> Pressed {
    ui.set_width(720.0);
    let title = if apply.text.is_some() { t!("vcs-patch-apply-clipboard-title") } else { t!("vcs-patch-apply-title") };
    title_label(ui, p, &title);
    let width = 215.0;
    if apply.text.is_none() {
        ui.horizontal(|ui| {
            row_label(ui, p, &t!("vcs-patch-file"), width);
            ui.allocate_ui(vec2(ui.available_width() - 90.0, 28.0), |ui| {
                if field(ui, &mut apply.path, "vcs-patch-apply-file", true).lost_focus() {
                    apply.preview = None;
                    apply.strip = None;
                    apply.stale = true;
                }
            });
            if ui.button(t!("vcs-patch-browse")).clicked()
                && let Some(file) = browse(&t!("vcs-patch-apply-title"), None)
            {
                apply.path = file.display().to_string();
                apply.preview = None;
                apply.strip = None;
                apply.stale = true;
            }
        });
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        row_label(ui, p, &t!("vcs-patch-base-dir"), width);
        let hint = root.display().to_string();
        let response = ui.add(
            egui::TextEdit::singleline(&mut apply.base_dir)
                .id_salt("vcs-patch-base-dir")
                .hint_text(hint)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 5)),
        );
        if response.lost_focus() {
            apply.stale = true;
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        row_label(ui, p, &t!("vcs-patch-strip"), width);
        let strip = apply.strip.unwrap_or(0);
        if ui.add_enabled(strip > 0, egui::Button::new("−")).on_hover_text(t!("vcs-patch-restore-directory")).clicked()
        {
            apply.strip = Some(strip - 1);
            apply.stale = true;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(strip.to_string()).font(theme::regular(13.0)).color(p.text));
        ui.add_space(6.0);
        if ui.button("+").on_hover_text(t!("vcs-patch-strip-directory")).clicked() {
            apply.strip = Some(strip + 1);
            apply.stale = true;
        }
        ui.add_space(24.0);
        if ui.checkbox(&mut apply.reverse, t!("vcs-patch-reverse")).changed() {
            apply.stale = true;
        }
        ui.add_space(24.0);
        ui.label(egui::RichText::new(t!("vcs-patch-encoding")).font(theme::regular(13.0)).color(p.text));
        if encoding_combo(ui, "vcs-patch-apply-encoding", &mut apply.encoding) {
            apply.stale = true;
        }
    });
    ui.add_space(12.0);
    if let Some(preview) = &apply.preview
        && (preview.author.is_some() || preview.message.is_some())
    {
        let subject = preview.message.as_deref().and_then(|message| message.lines().next()).unwrap_or_default();
        let author = preview.author.as_deref().unwrap_or_default();
        let text = t!("vcs-patch-from-commit", author = author, subject = subject);
        ui.label(egui::RichText::new(text).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        let show = ui.add(egui::Button::new(t!("vcs-patch-show-diff")).shortcut_text("⌘D"));
        if show.clicked() {
            apply.diff_stale = true;
        }
    });
    ui.add_space(6.0);
    apply_files(ui, p, apply, root);
    if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::D)) {
        apply.diff_stale = true;
    }
    if let Some(file) = apply.selected_file() {
        let mut notes: Vec<(String, egui::Color32)> = Vec::new();
        if let Some(reason) = &file.reason {
            notes.push((reason.clone(), p.vcs_conflict));
        }
        if !file.failed.is_empty() {
            let hunks: Vec<String> = file.failed.iter().map(|hunk| hunk.hunk.to_string()).collect();
            let text =
                t!("vcs-patch-failed-hunks", count = file.failed.len(), hunks = hunks.join(", "), total = file.hunks);
            notes.push((text, p.vcs_conflict));
        }
        if !file.approximate.is_empty() {
            let hunks: Vec<String> = file.approximate.iter().map(usize::to_string).collect();
            let text = t!("vcs-patch-approximate-hunks", count = file.approximate.len(), hunks = hunks.join(", "));
            notes.push((text, p.text_secondary));
        }
        match file.executable {
            Some(true) => notes.push((t!("vcs-patch-makes-executable"), p.text_secondary)),
            Some(false) => notes.push((t!("vcs-patch-makes-not-executable"), p.text_secondary)),
            None => {}
        }
        for (text, color) in notes {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).font(theme::regular(12.5)).color(color));
        }
    }
    ui.add_space(8.0);
    let conflicts: Vec<(usize, usize)> = apply
        .diff
        .as_ref()
        .map(|diff| diff.conflicts.iter().map(|range| (range.start, range.end)).collect())
        .unwrap_or_default();
    preview(ui, p, apply.diff.as_ref().map(|diff| &diff.view), &conflicts);
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!apply.import, |ui| {
            ui.label(egui::RichText::new(t!("vcs-unshelve-changelist")).font(theme::regular(13.0)).color(p.text));
            let shown = apply.target.clone().unwrap_or_else(|| t!("vcs-patch-new-changelist"));
            egui::ComboBox::from_id_salt("vcs-patch-changelist").selected_text(shown).show_ui(ui, |ui| {
                for list in &apply.lists {
                    ui.selectable_value(&mut apply.target, Some(list.clone()), list);
                }
                ui.selectable_value(&mut apply.target, None, t!("vcs-patch-new-changelist"));
            });
            if apply.target.is_none() {
                ui.add(
                    egui::TextEdit::singleline(&mut apply.new_list)
                        .id_salt("vcs-patch-new-list")
                        .hint_text(t!("vcs-changelist-name"))
                        .desired_width(200.0)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
            }
        });
        ui.add_space(16.0);
        ui.checkbox(&mut apply.import, t!("vcs-patch-import-to-shelf"));
    });
    error_label(ui, p, apply.error.as_deref());
    let ready = apply.preview.is_some() && !apply.checked.is_empty();
    let enter =
        ready && ui.memory(|memory| memory.focused().is_none()) && ui.input(|input| input.key_pressed(Key::Enter));
    let confirm = if apply.import { t!("vcs-patch-import-confirm") } else { t!("vcs-patch-apply-confirm") };
    let pressed = buttons(ui, p, &confirm, enter);
    match pressed {
        Pressed::Confirm if !ready => Pressed::None,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_names_are_plain() {
        assert_eq!(file_name("Fix the build: now!"), "Fix_the_build__now");
        assert_eq!(file_name("///"), "changes");
        assert_eq!(file_name("Изменения"), "Изменения");
    }

    #[test]
    fn the_dialogs_have_names_for_agents() {
        let create = CreateDialog {
            source: PatchOf::Changes,
            revision: None,
            shelf: None,
            subject: String::new(),
            picker: None,
            to_file: false,
            file: String::new(),
            base: String::new(),
            reverse: false,
            encoding: "UTF-8".to_owned(),
            error: None,
        };
        assert_eq!(PatchDialog::Create(Box::new(create)).name(), "createPatch");
    }
}
