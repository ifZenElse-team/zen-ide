pub mod actions_on_save;
pub mod activity_monitor;
pub mod ant;
pub mod artifacts;
pub mod autosave;
pub mod before_launch;
pub mod bookmarks;
pub mod brackets;
pub mod build_check;
pub mod build_sync;
pub mod build_tools;
pub mod build_triggers;
pub mod caffeine;
pub mod calculator;
pub mod charsets;
pub mod clipboard_history;
pub mod closing;
pub mod code_quality;
pub mod code_style;
pub mod code_style_options;
pub mod code_style_scheme;
pub mod compare;
pub mod compare_folders;
pub mod compound;
pub mod coverage;
pub mod database;
pub mod debug;
pub mod dependencies;
pub mod dependency_analysis;
pub mod diagnostics;
pub mod disk;
pub mod editing;
pub mod editor_config;
pub mod exit;
pub mod expand_word;
pub mod extend_selection;
pub mod external_tools;
pub mod file_actions;
pub mod file_clipboard;
pub mod file_encodings;
pub mod file_indent;
pub mod file_types;
pub mod file_watchers;
pub mod find;
pub mod find_history;
pub mod find_results;
pub mod folding;
pub mod font_size;
pub mod formatter_markers;
pub mod gradle;
pub mod groups;
pub mod guard;
pub mod handoff;
pub mod hidden_files;
pub mod history;
pub mod http;
pub mod ide_log;
pub mod ide_settings;
pub mod indent_detection;
pub mod inspect_code;
pub mod inspection_profiles;
pub mod inspections;
pub mod internal_errors;
pub mod java;
pub mod java_run;
pub mod jdk;
pub mod jvm_libraries;
pub mod jvm_modules;
pub mod keymaps;
pub mod language;
pub mod launcher;
pub mod line_numbers;
pub mod line_separators;
pub mod lines;
pub mod local_history;
pub mod login_env;
pub mod macros;
pub mod maintenance;
pub mod markdown;
pub mod maven;
pub mod mcp_stats;
pub mod navigate;
#[cfg(not(target_arch = "wasm32"))]
pub mod network;
pub mod new_project;
pub mod nodejs;
pub mod notifications;
pub mod npm;
pub mod occurrences;
pub mod on_save;
pub mod plugin_diff;
pub mod plugin_document;
pub mod plugin_manager;
pub mod plugin_projects;
pub mod plugin_runs;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugin_tools;
pub mod plugins;
pub mod power_save;
pub mod preview;
pub mod profile;
pub mod project_index;
pub mod project_model;
pub mod project_settings;
pub mod project_structure;
pub mod read_only;
pub mod resource_bundles;
pub mod run;
pub mod run_anything;
pub mod run_services;
pub mod run_templates;
pub mod schemas;
pub mod scratch;
pub mod suppression;
pub mod trust;
pub use zen_project::model;
pub use zen_project::recent;
pub mod file_templates;
pub mod search;
pub mod search_everywhere;
pub mod search_scopes;
pub mod soft_wrap;
pub mod spelling;
pub mod spring_boot;
pub mod sql_format;
pub mod sticky;
pub mod tabs;
pub mod tasks;
pub mod templates;
pub mod terminal;
pub mod test_history;
pub mod text_replace;
pub mod themes;
pub mod todo;
pub mod todo_patterns;
pub mod unwrap;
pub mod vcs;
pub mod virtual_documents;
pub mod visual_aids;
pub mod workspaces;
pub mod wrap_on_typing;

use std::sync::Arc;

use std::path::{Path, PathBuf};

use language::file_operations::Touched as FileTouch;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::FileOperation;
use zen_text::{Buffer, Edit, EditKind, Text};

pub const LOCALES: &[(&str, &str)] = &[
    zen_run::LOCALES[0],
    zen_run::LOCALES[1],
    zen_dap::LOCALES[0],
    zen_dap::LOCALES[1],
    ("en", include_str!("../locales/en/workbench.ftl")),
    ("ru", include_str!("../locales/ru/workbench.ftl")),
    ("en", include_str!("../locales/en/new-project.ftl")),
    ("ru", include_str!("../locales/ru/new-project.ftl")),
    ("en", include_str!("../locales/en/code-style-scheme.ftl")),
    ("ru", include_str!("../locales/ru/code-style-scheme.ftl")),
    ("en", include_str!("../locales/en/diagnostics.ftl")),
    ("ru", include_str!("../locales/ru/diagnostics.ftl")),
    ("en", include_str!("../locales/en/maintenance.ftl")),
    ("ru", include_str!("../locales/ru/maintenance.ftl")),
    ("en", include_str!("../locales/en/actions-on-save.ftl")),
    ("ru", include_str!("../locales/ru/actions-on-save.ftl")),
    ("en", include_str!("../locales/en/build-tools.ftl")),
    ("ru", include_str!("../locales/ru/build-tools.ftl")),
    ("en", include_str!("../locales/en/ant.ftl")),
    ("ru", include_str!("../locales/ru/ant.ftl")),
    ("en", include_str!("../locales/en/resource-bundles.ftl")),
    ("ru", include_str!("../locales/ru/resource-bundles.ftl")),
    ("en", include_str!("../locales/en/spring-boot.ftl")),
    ("ru", include_str!("../locales/ru/spring-boot.ftl")),
    ("en", include_str!("../locales/en/dependencies.ftl")),
    ("ru", include_str!("../locales/ru/dependencies.ftl")),
    ("en", include_str!("../locales/en/maven-settings.ftl")),
    ("ru", include_str!("../locales/ru/maven-settings.ftl")),
    ("en", include_str!("../locales/en/gradle-settings.ftl")),
    ("ru", include_str!("../locales/ru/gradle-settings.ftl")),
    ("en", include_str!("../locales/en/annotation-processing.ftl")),
    ("ru", include_str!("../locales/ru/annotation-processing.ftl")),
    ("en", include_str!("../locales/en/vcs.ftl")),
    ("ru", include_str!("../locales/ru/vcs.ftl")),
    ("en", include_str!("../locales/en/java.ftl")),
    ("ru", include_str!("../locales/ru/java.ftl")),
    ("en", include_str!("../locales/en/debug.ftl")),
    ("ru", include_str!("../locales/ru/debug.ftl")),
    ("en", include_str!("../locales/en/project-settings.ftl")),
    ("ru", include_str!("../locales/ru/project-settings.ftl")),
    ("en", include_str!("../locales/en/search-scopes.ftl")),
    ("ru", include_str!("../locales/ru/search-scopes.ftl")),
    ("en", include_str!("../locales/en/keymaps.ftl")),
    ("ru", include_str!("../locales/ru/keymaps.ftl")),
    ("en", include_str!("../locales/en/terminal.ftl")),
    ("ru", include_str!("../locales/ru/terminal.ftl")),
    ("en", include_str!("../locales/en/http.ftl")),
    ("ru", include_str!("../locales/ru/http.ftl")),
    ("en", include_str!("../locales/en/network.ftl")),
    ("ru", include_str!("../locales/ru/network.ftl")),
    ("en", include_str!("../locales/en/local-history.ftl")),
    ("ru", include_str!("../locales/ru/local-history.ftl")),
    ("en", include_str!("../locales/en/caffeine.ftl")),
    ("ru", include_str!("../locales/ru/caffeine.ftl")),
    ("en", include_str!("../locales/en/search-everywhere.ftl")),
    ("ru", include_str!("../locales/ru/search-everywhere.ftl")),
    ("en", include_str!("../locales/en/bookmarks.ftl")),
    ("en", include_str!("../locales/en/jdk.ftl")),
    ("ru", include_str!("../locales/ru/jdk.ftl")),
    ("en", include_str!("../locales/en/libraries.ftl")),
    ("ru", include_str!("../locales/ru/libraries.ftl")),
    ("en", include_str!("../locales/en/structure.ftl")),
    ("ru", include_str!("../locales/ru/structure.ftl")),
    ("en", include_str!("../locales/en/artifacts.ftl")),
    ("ru", include_str!("../locales/ru/artifacts.ftl")),
    ("en", include_str!("../locales/en/code-quality.ftl")),
    ("ru", include_str!("../locales/ru/code-quality.ftl")),
    ("ru", include_str!("../locales/ru/bookmarks.ftl")),
    ("en", include_str!("../locales/en/macros.ftl")),
    ("ru", include_str!("../locales/ru/macros.ftl")),
    ("en", include_str!("../locales/en/external-tools.ftl")),
    ("ru", include_str!("../locales/ru/external-tools.ftl")),
    ("en", include_str!("../locales/en/before-launch.ftl")),
    ("ru", include_str!("../locales/ru/before-launch.ftl")),
    ("en", include_str!("../locales/en/file-watchers.ftl")),
    ("ru", include_str!("../locales/ru/file-watchers.ftl")),
    ("en", include_str!("../locales/en/database.ftl")),
    ("ru", include_str!("../locales/ru/database.ftl")),
    ("en", include_str!("../locales/en/database-history.ftl")),
    ("ru", include_str!("../locales/ru/database-history.ftl")),
    ("en", include_str!("../locales/en/database-plan.ftl")),
    ("ru", include_str!("../locales/ru/database-plan.ftl")),
    ("en", include_str!("../locales/en/database-diagram.ftl")),
    ("ru", include_str!("../locales/ru/database-diagram.ftl")),
    ("en", include_str!("../locales/en/database-search.ftl")),
    ("ru", include_str!("../locales/ru/database-search.ftl")),
    ("en", include_str!("../locales/en/database-recovery.ftl")),
    ("ru", include_str!("../locales/ru/database-recovery.ftl")),
    ("en", include_str!("../locales/en/database-find.ftl")),
    ("ru", include_str!("../locales/ru/database-find.ftl")),
    ("en", include_str!("../locales/en/database-clipboard.ftl")),
    ("ru", include_str!("../locales/ru/database-clipboard.ftl")),
    ("en", include_str!("../locales/en/database-relations.ftl")),
    ("ru", include_str!("../locales/ru/database-relations.ftl")),
    ("en", include_str!("../locales/en/database-csv-import.ftl")),
    ("ru", include_str!("../locales/ru/database-csv-import.ftl")),
    ("en", include_str!("../locales/en/database-transfer.ftl")),
    ("ru", include_str!("../locales/ru/database-transfer.ftl")),
    ("en", include_str!("../locales/en/database-sessions.ftl")),
    ("ru", include_str!("../locales/ru/database-sessions.ftl")),
    ("en", include_str!("../locales/en/database-clients.ftl")),
    ("ru", include_str!("../locales/ru/database-clients.ftl")),
    ("en", include_str!("../locales/en/database-migrations.ftl")),
    ("ru", include_str!("../locales/ru/database-migrations.ftl")),
    ("en", include_str!("../locales/en/database-structure.ftl")),
    ("ru", include_str!("../locales/ru/database-structure.ftl")),
    ("en", include_str!("../locales/en/database-compare.ftl")),
    ("ru", include_str!("../locales/ru/database-compare.ftl")),
    ("en", include_str!("../locales/en/database-backup.ftl")),
    ("ru", include_str!("../locales/ru/database-backup.ftl")),
];

#[derive(Clone, Debug)]
pub struct Document {
    path: PathBuf,
    buffer: Buffer,
    mirror: String,
    saved: Text,
    dirty: bool,
    save_count: u64,
    version: u64,
    changes: std::collections::VecDeque<VersionChanges>,
    pending: Vec<zen_syntax::TextChange>,
    pending_chars: Vec<CharEdit>,
    image: Option<ImageView>,
    folds: Vec<std::ops::Range<usize>>,
    find_excluded: Vec<std::ops::Range<usize>>,
    find_scope: Option<std::ops::Range<usize>>,
    occurrences_added: Vec<std::ops::Range<usize>>,
    foldable: Option<(u64, Arc<Vec<zen_editor::Fold>>)>,
    manual: Vec<std::ops::Range<usize>>,
    column_mode: bool,
    soft_wrap: Option<bool>,
    font_size: Option<f32>,
    whitespaces: Option<bool>,
    indent_guides: Option<bool>,
    line_numbers: Option<bool>,
    detected_indent: Option<indent_detection::DetectedIndent>,
    configured_indent: bool,
    column: Option<(zen_text::Point, zen_text::Point, zen_editor::Selections)>,
    marks: Vec<usize>,
    edit_places: Vec<(usize, u64, u64)>,
    breakpoint_marks: Vec<Option<std::ops::Range<usize>>>,
    bookmark_marks: Vec<Option<std::ops::Range<usize>>>,
    selections: zen_editor::Selections,
    selection_version: u64,
    folds_version: u64,
    conflict: bool,
    kept_disk: Option<Vec<u8>>,
    read_only: Option<String>,
    locked: Option<String>,
    refused: bool,
    charset: Option<String>,
    encoding: Option<(charsets::Charset, bool)>,
    line_separator: zen_text::LineSeparator,
    separator_found: bool,
    saves: Vec<local_history::SavedText>,
    template: Option<templates::TemplateSession>,
    holder: Option<groups::GroupId>,
    views: Vec<(groups::GroupId, ViewState)>,
}

#[derive(Clone, Debug)]
struct VersionChanges {
    from: u64,
    to: u64,
    edits: Vec<zen_syntax::TextChange>,
    chars: Vec<CharEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharEdit {
    pub range: std::ops::Range<usize>,
    pub inserted: usize,
}

impl CharEdit {
    pub fn map(&self, offset: usize) -> usize {
        if offset >= self.range.end {
            offset - self.range.len() + self.inserted
        } else if offset > self.range.start {
            self.range.start + self.inserted
        } else {
            offset
        }
    }

    pub fn map_region(&self, range: &std::ops::Range<usize>) -> Option<std::ops::Range<usize>> {
        let (from, to) = (self.range.start, self.range.end);
        let shift = |offset: usize| offset - (to - from) + self.inserted;
        let start = if from > range.start {
            range.start
        } else if to > range.start {
            from + self.inserted
        } else {
            shift(range.start)
        };
        let end = if to < range.end {
            shift(range.end)
        } else if from < range.end {
            from
        } else {
            range.end
        };
        (start < end).then_some(start..end)
    }
}

const CHANGES_KEPT: usize = 64;

#[derive(Clone, Debug)]
struct ViewState {
    selections: zen_editor::Selections,
    folds: Vec<std::ops::Range<usize>>,
    selection_version: u64,
}

/// What comparing a document with its file on disk found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiskSync {
    /// The file holds what the document last saved or loaded.
    Unchanged,
    /// The document had no unsaved changes and now shows the file's new text.
    Reloaded,
    /// The file changed while the document has unsaved changes: the user decides.
    Conflict,
    /// The file is gone; the document stays open.
    Removed,
}

#[derive(Clone, Debug)]
pub struct ImageView {
    decoded: Arc<std::sync::OnceLock<Result<Arc<zen_image::Picture>, String>>>,
    file_size: u64,
    zoom: ImageZoom,
    fit_scale: f32,
}

/// How an image is zoomed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ImageZoom {
    /// The image's own size, or smaller to fit the window (how images open).
    Auto,
    /// Fitted to the window, larger or smaller.
    Fit,
    /// A fixed scale, 1.0 being the image's own size.
    Scale(f32),
}

pub(crate) const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.02..=32.0;
pub(crate) const ZOOM_STEP: f32 = 1.25;

impl ImageView {
    pub fn picture(&self) -> Option<&Arc<zen_image::Picture>> {
        self.decoded.get().and_then(|decoded| decoded.as_ref().ok())
    }

    pub fn is_loading(&self) -> bool {
        self.decoded.get().is_none()
    }

    pub fn error(&self) -> Option<&str> {
        self.decoded.get().and_then(|decoded| decoded.as_ref().err()).map(String::as_str)
    }

    fn decode(path: &Path, bytes: Vec<u8>) -> Arc<std::sync::OnceLock<Result<Arc<zen_image::Picture>, String>>> {
        let slot: Arc<std::sync::OnceLock<Result<Arc<zen_image::Picture>, String>>> = Arc::default();
        let decode = {
            let (slot, path) = (Arc::clone(&slot), path.to_owned());
            move || {
                let result = zen_image::decode(&path, &bytes).map(Arc::new).map_err(|error| error.to_string());
                let _ = slot.set(result);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(decode);
        #[cfg(target_arch = "wasm32")]
        decode();
        slot
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn zoom(&self) -> ImageZoom {
        self.zoom
    }

    pub fn scale(&self) -> f32 {
        match self.zoom {
            ImageZoom::Auto => self.fit_scale.min(1.0),
            ImageZoom::Fit => self.fit_scale,
            ImageZoom::Scale(scale) => scale,
        }
    }

    pub fn set_fit_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.fit_scale = scale;
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale.is_finite() {
            self.zoom = ImageZoom::Scale(scale.clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end()));
        }
    }
}

impl Document {
    fn open(path: &Path) -> Result<Self, String> {
        if zen_image::opens_as_picture(path) {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            let file_size = bytes.len() as u64;
            let image =
                ImageView { decoded: ImageView::decode(path, bytes), file_size, zoom: ImageZoom::Auto, fit_scale: 1.0 };
            let buffer = Buffer::new("");
            return Ok(Self {
                path: path.to_owned(),
                saved: buffer.text().clone(),
                save_count: 0,
                buffer,
                mirror: String::new(),
                folds: Vec::new(),
                find_excluded: Vec::new(),
                find_scope: None,
                occurrences_added: Vec::new(),
                foldable: None,
                manual: Vec::new(),
                column_mode: false,
                soft_wrap: None,
                font_size: None,
                whitespaces: None,
                indent_guides: None,
                line_numbers: None,
                detected_indent: None,
                configured_indent: false,
                column: None,
                marks: Vec::new(),
                edit_places: Vec::new(),
                breakpoint_marks: Vec::new(),
                bookmark_marks: Vec::new(),
                dirty: false,
                version: next_version(),
                changes: std::collections::VecDeque::new(),
                pending: Vec::new(),
                pending_chars: Vec::new(),
                image: Some(image),
                selections: zen_editor::Selections::default(),
                selection_version: 0,
                folds_version: 0,
                conflict: false,
                kept_disk: None,
                read_only: virtual_documents::reason(path),
                locked: read_only::lock_reason(path),
                refused: false,
                charset: None,
                encoding: None,
                line_separator: zen_text::LineSeparator::system(),
                separator_found: false,
                saves: Vec::new(),
                template: None,
                holder: None,
                views: Vec::new(),
            });
        }
        let contents = zen_vfs::read_text(path).map_err(|error| error.to_string())?;
        Ok(Self::with_text(path, contents))
    }

    fn read_only_text(path: &Path, bytes: &[u8], reason: String) -> Self {
        let mut document = Self::with_text(path, String::from_utf8_lossy(bytes).into_owned());
        document.read_only = Some(reason);
        document
    }

    fn with_text(path: &Path, contents: String) -> Self {
        let found = zen_text::line_separator::detect(&contents);
        let buffer = Buffer::new(contents.as_str());
        let detected_indent = indent_detection::detect(&contents, &code_style::language_of(path));
        Self {
            path: path.to_owned(),
            saved: buffer.text().clone(),
            save_count: 0,
            buffer,
            mirror: contents,
            folds: Vec::new(),
            find_excluded: Vec::new(),
            find_scope: None,
            occurrences_added: Vec::new(),
            foldable: None,
            manual: Vec::new(),
            column_mode: false,
            soft_wrap: None,
            font_size: None,
            whitespaces: None,
            indent_guides: None,
            line_numbers: None,
            detected_indent,
            configured_indent: false,
            column: None,
            marks: Vec::new(),
            edit_places: Vec::new(),
            breakpoint_marks: Vec::new(),
            bookmark_marks: Vec::new(),
            dirty: false,
            version: next_version(),
            changes: std::collections::VecDeque::new(),
            pending: Vec::new(),
            pending_chars: Vec::new(),
            image: None,
            selections: zen_editor::Selections::default(),
            selection_version: 0,
            folds_version: 0,
            conflict: false,
            kept_disk: None,
            read_only: virtual_documents::reason(path),
            locked: read_only::lock_reason(path),
            refused: false,
            charset: None,
            encoding: None,
            line_separator: found.unwrap_or_else(zen_text::LineSeparator::system),
            separator_found: found.is_some(),
            saves: Vec::new(),
            template: None,
            holder: None,
            views: Vec::new(),
        }
    }

    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref().or(self.locked.as_deref())
    }

    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    pub fn charset(&self) -> &str {
        self.charset.as_deref().unwrap_or("UTF-8")
    }

    pub fn encoding(&self) -> (charsets::Charset, bool) {
        self.encoding.unwrap_or((charsets::Charset::Utf8, false))
    }

    pub(crate) fn set_encoding(&mut self, charset: charsets::Charset, bom: bool) {
        self.encoding = Some((charset, bom));
        self.charset = Some(charset.label().to_owned());
    }

    pub(crate) fn reinterpret(&mut self, text: String) {
        self.read_only = virtual_documents::reason(&self.path);
        self.load(text);
    }

    pub(crate) fn convert(&mut self, charset: charsets::Charset, bom: bool) -> Result<(), zen_vfs::VfsError> {
        self.refuse_read_only()?;
        let stamp = self.check_disk()?;
        let earlier = (self.encoding, self.charset.clone());
        self.set_encoding(charset, bom);
        let written = self.encoded(&self.mirror).and_then(|bytes| self.write(&bytes, stamp));
        if let Err(error) = written {
            (self.encoding, self.charset) = earlier;
            return Err(error);
        }
        self.saved_now();
        Ok(())
    }

    pub fn line_separator(&self) -> zen_text::LineSeparator {
        self.line_separator
    }

    pub fn line_separator_found(&self) -> bool {
        self.separator_found
    }

    pub(crate) fn default_line_separator(&mut self, separator: zen_text::LineSeparator) {
        if !self.separator_found {
            self.line_separator = separator;
        }
    }

    fn detect_line_separator(&mut self) {
        if let Some(found) = zen_text::line_separator::detect(&self.mirror) {
            self.line_separator = found;
            self.separator_found = true;
        }
    }

    pub fn with_line_separator(&self, text: String) -> String {
        if !text.contains(['\n', '\r']) {
            return text;
        }
        match zen_text::line_separator::convert(&text, self.line_separator) {
            std::borrow::Cow::Borrowed(_) => text,
            std::borrow::Cow::Owned(converted) => converted,
        }
    }

    fn separator_repairs(&self, edits: &[Edit]) -> Vec<Edit> {
        zen_text::line_separator::repairs(self.buffer.text(), edits, self.line_separator)
    }

    pub fn convert_line_separators(&mut self, separator: zen_text::LineSeparator) -> bool {
        if self.image.is_some() || self.read_only.is_some() {
            return false;
        }
        self.line_separator = separator;
        let std::borrow::Cow::Owned(converted) = zen_text::line_separator::convert(&self.mirror, separator) else {
            return false;
        };
        let selections: Vec<zen_editor::Selection> = self.selections.iter().copied().collect();
        let offsets: Vec<usize> = selections
            .iter()
            .flat_map(|selection| [selection.anchor, selection.head])
            .chain(self.folds.iter().flat_map(|fold| [fold.start, fold.end]))
            .collect();
        let mapped = zen_text::line_separator::convert_offsets(&self.mirror, separator, &offsets);
        let edit = Edit::replace(0..self.buffer.text().len_chars(), converted);
        if self.buffer.edit(vec![edit.clone()], EditKind::Other).is_err() {
            return false;
        }
        self.applied(&edit);
        let (carets, folds) = mapped.split_at(selections.len() * 2);
        let carets = carets.chunks(2).map(|pair| zen_editor::Selection::new(pair[0], pair[1])).collect();
        self.selections = zen_editor::Selections::from_vec(carets, self.selections.primary_index());
        self.folds = folds.chunks(2).map(|pair| pair[0]..pair[1]).filter(|fold| fold.start < fold.end).collect();
        self.selection_version += 1;
        self.folds_version += 1;
        self.separator_found = true;
        self.raise_version();
        self.dirty = *self.buffer.text() != self.saved;
        true
    }

    pub(crate) fn convert_and_write(
        &mut self,
        separator: zen_text::LineSeparator,
    ) -> Result<Option<line_separators::Rewritten>, zen_vfs::VfsError> {
        self.refuse_read_only()?;
        let std::borrow::Cow::Owned(converted) = zen_text::line_separator::convert(&self.mirror, separator) else {
            self.line_separator = separator;
            return Ok(None);
        };
        let stamp = self.check_disk()?;
        let before = std::fs::read(&self.path).ok();
        let after = self.encoded(&converted)?;
        self.write(&after, stamp)?;
        self.convert_line_separators(separator);
        self.saved = self.buffer.text().clone();
        self.save_count += 1;
        self.dirty = false;
        self.conflict = false;
        self.kept_disk = None;
        Ok(Some((before, after)))
    }

    pub fn image(&self) -> Option<&ImageView> {
        self.image.as_ref()
    }

    pub fn image_mut(&mut self) -> Option<&mut ImageView> {
        self.image.as_mut()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.path.display().to_string(), |name| name.to_string_lossy().into_owned())
    }

    pub fn text(&self) -> &str {
        &self.mirror
    }

    pub fn snapshot(&self) -> &Text {
        self.buffer.text()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn save_count(&self) -> u64 {
        self.save_count
    }

    pub(crate) fn was_edited(&self) -> bool {
        self.dirty || self.save_count > 0
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn selections(&self) -> &zen_editor::Selections {
        &self.selections
    }

    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    pub fn set_selections(&mut self, selections: zen_editor::Selections) {
        self.selections = selections.clamped(self.buffer.text().len_chars());
        self.selection_version += 1;
        self.buffer.close_group();
    }

    pub fn show_in(&mut self, group: groups::GroupId) {
        if self.holder == Some(group) {
            return;
        }
        let parked = self.views.iter().position(|(id, _)| *id == group).map(|index| self.views.remove(index).1);
        if let Some(holder) = self.holder {
            let own = ViewState {
                selections: self.selections.clone(),
                folds: self.folds.clone(),
                selection_version: self.selection_version,
            };
            self.views.push((holder, own));
        }
        if let Some(state) = parked {
            self.selections = state.selections.clamped(self.buffer.text().len_chars());
            self.folds = state.folds;
            self.folds_version += 1;
            self.selection_version = state.selection_version;
        }
        self.holder = Some(group);
    }

    pub(crate) fn retain_views(&mut self, shows: impl Fn(groups::GroupId) -> bool) {
        self.views.retain(|(id, _)| shows(*id));
        if self.holder.is_some_and(|holder| !shows(holder)) {
            self.holder = None;
        }
    }

    pub fn apply_op(&mut self, op: &zen_editor::EditOp) -> bool {
        if self.refuses_edits() {
            return false;
        }
        let op = match op {
            zen_editor::EditOp::Insert { text } => {
                zen_editor::EditOp::Insert { text: self.with_line_separator(text.clone()) }
            }
            zen_editor::EditOp::Paste { text } => {
                zen_editor::EditOp::Paste { text: self.with_line_separator(text.clone()) }
            }
            other => other.clone(),
        };
        let Some((edits, selections)) = zen_editor::edit(self.buffer.text(), &self.selections, &op) else {
            return false;
        };
        let repairs = self.separator_repairs(&edits);
        let typing = repairs.is_empty()
            && matches!(&op, zen_editor::EditOp::Insert { text } if text.chars().count() == 1 && !text.contains(['\n', '\r']));
        let selections = if repairs.is_empty() { selections } else { selections.mapped(&repairs) };
        let edits: Vec<Edit> = edits.into_iter().chain(repairs).collect();
        if self.buffer.edit(edits.clone(), if typing { EditKind::Typing } else { EditKind::Other }).is_err() {
            return false;
        }
        for edit in &edits {
            self.applied(edit);
        }
        self.raise_version();
        self.dirty = *self.buffer.text() != self.saved;
        self.selections = selections.clamped(self.buffer.text().len_chars());
        self.selection_version += 1;
        self.edited_at(self.selections.primary().head);
        true
    }

    fn refuses_edits(&mut self) -> bool {
        self.refused |= self.locked.is_some() && self.read_only.is_none() && self.image.is_none();
        self.image.is_some() || self.read_only().is_some()
    }

    pub fn has_conflict(&self) -> bool {
        self.conflict
    }

    pub fn sync_with_disk(&mut self) -> DiskSync {
        self.locked = read_only::lock_reason(&self.path);
        if self.image.is_some() {
            return DiskSync::Unchanged;
        }
        let Ok(bytes) = std::fs::read(&self.path) else {
            return if self.path.exists() { DiskSync::Unchanged } else { DiskSync::Removed };
        };
        let Some(contents) = self.decoded(&bytes) else { return DiskSync::Unchanged };
        let saved = self.saved.to_string();
        if contents == saved || contents == self.mirror {
            if contents == self.mirror {
                self.saved = self.buffer.text().clone();
                self.dirty = false;
                self.conflict = false;
            }
            return DiskSync::Unchanged;
        }
        if self.dirty && self.kept_disk.as_deref() == Some(bytes.as_slice()) {
            return DiskSync::Unchanged;
        }
        if self.dirty {
            self.conflict = true;
            return DiskSync::Conflict;
        }
        self.load(contents);
        DiskSync::Reloaded
    }

    fn load(&mut self, contents: String) {
        let edit = Edit::replace(0..self.buffer.text().len_chars(), contents);
        if self.buffer.edit(vec![edit.clone()], EditKind::Other).is_ok() {
            self.applied(&edit);
            self.selections = self.selections.clamped(self.buffer.text().len_chars());
            self.selection_version += 1;
            self.raise_version();
        }
        self.saved = self.buffer.text().clone();
        self.dirty = false;
        self.conflict = false;
        self.kept_disk = None;
        self.detect_indent();
        self.detect_line_separator();
    }

    fn detect_indent(&mut self) {
        self.detected_indent = indent_detection::detect(&self.mirror, &code_style::language_of(&self.path));
    }

    pub fn reload_from_disk(&mut self) -> bool {
        match self.read_disk() {
            Ok(contents) => {
                self.load(contents);
                true
            }
            Err(_) => false,
        }
    }

    pub fn keep_mine(&mut self) {
        self.conflict = false;
        self.kept_disk = std::fs::read(&self.path).ok();
    }

    pub(crate) fn changed_on_disk(&self) -> std::io::Result<bool> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(!self.knows(&bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn knows(&self, bytes: &[u8]) -> bool {
        self.kept_disk.as_deref() == Some(bytes)
            || self.decoded(bytes).is_some_and(|disk| disk == self.mirror || self.saved == disk.as_str())
    }

    pub(crate) fn check_disk(&mut self) -> Result<Option<zen_vfs::Stamp>, zen_vfs::VfsError> {
        let stamp = zen_vfs::stamp(&self.path);
        let changed = self.changed_on_disk().map_err(|source| zen_vfs::VfsError {
            action: "read",
            path: self.path.clone(),
            source,
        })?;
        if self.conflict || changed {
            return Err(self.refuse_changed());
        }
        Ok(stamp)
    }

    fn refuse_changed(&mut self) -> zen_vfs::VfsError {
        self.conflict = true;
        let source = std::io::Error::other(t!("workbench-error-changed-on-disk"));
        zen_vfs::VfsError { action: "save", path: self.path.clone(), source }
    }

    pub(crate) fn refuse_read_only(&self) -> Result<(), zen_vfs::VfsError> {
        let Some(reason) = self.read_only() else { return Ok(()) };
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason.to_owned());
        Err(zen_vfs::VfsError { action: "save", path: self.path.clone(), source })
    }

    fn write(&mut self, bytes: &[u8], stamp: Option<zen_vfs::Stamp>) -> Result<(), zen_vfs::VfsError> {
        zen_vfs::write_unless_changed(&self.path, bytes, stamp)
            .map_err(|error| if error.is_changed_on_disk() { self.refuse_changed() } else { error })
    }

    pub fn apply_action(&mut self, edits: Vec<Edit>, selections: zen_editor::Selections) -> bool {
        if self.refuses_edits() {
            return false;
        }
        if edits.iter().all(Edit::is_noop) {
            self.set_selections(selections);
            return false;
        }
        let repairs = self.separator_repairs(&edits);
        let selections = if repairs.is_empty() { selections } else { selections.mapped(&repairs) };
        let edits: Vec<Edit> = edits.into_iter().chain(repairs).collect();
        if self.buffer.edit(edits.clone(), EditKind::Other).is_err() {
            return false;
        }
        for edit in &edits {
            self.applied(edit);
        }
        self.selections = selections.clamped(self.buffer.text().len_chars());
        self.selection_version += 1;
        self.raise_version();
        self.dirty = *self.buffer.text() != self.saved;
        self.edited_at(self.selections.primary().head);
        true
    }

    pub fn code_style(&self, settings: &zen_core::Settings) -> code_style::CodeStyle {
        code_style::for_text(settings, &self.path, &self.mirror)
    }

    pub fn detected_indent(&self) -> Option<indent_detection::DetectedIndent> {
        self.detected_indent
    }

    pub fn uses_configured_indent(&self) -> bool {
        self.configured_indent
    }

    pub fn set_uses_configured_indent(&mut self, configured: bool) {
        self.configured_indent = configured;
    }

    pub fn replace_ranges(&mut self, replacements: Vec<(std::ops::Range<usize>, String)>) -> bool {
        if replacements.is_empty() || self.refuses_edits() {
            return false;
        }
        let edits: Vec<Edit> = replacements
            .into_iter()
            .rev()
            .map(|(range, text)| Edit::replace(range, self.with_line_separator(text)))
            .collect();
        let first = edits.last().map(|first| first.inserted_range().start);
        let repairs = self.separator_repairs(&edits);
        let edits: Vec<Edit> = edits.into_iter().chain(repairs).collect();
        if self.buffer.edit(edits.clone(), EditKind::Other).is_err() {
            return false;
        }
        self.applied_all(&edits);
        self.selections = self.selections.mapped(&edits);
        self.selection_version += 1;
        self.raise_version();
        self.dirty = *self.buffer.text() != self.saved;
        if let Some(first) = first {
            self.edited_at(first);
        }
        true
    }

    fn edited_at(&mut self, offset: usize) {
        let text = self.buffer.text();
        let line = |offset: usize| text.offset_to_point(offset.min(text.len_chars())).map(|point| point.line).ok();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let same = self.edit_places.last().is_some_and(|(last, _, _)| line(*last) == line(offset));
        if same {
            self.edit_places.pop();
        }
        self.edit_places.push((offset, now, next_edit_order()));
        if self.edit_places.len() > EDIT_PLACES_KEPT {
            self.edit_places.remove(0);
        }
    }

    pub(crate) fn take_edit_places(&mut self) -> Vec<(usize, u64, u64)> {
        std::mem::take(&mut self.edit_places)
    }

    fn raise_version(&mut self) {
        let from = self.version;
        self.version = next_version();
        let edits = std::mem::take(&mut self.pending);
        let chars = std::mem::take(&mut self.pending_chars);
        self.changes.push_back(VersionChanges { from, to: self.version, edits, chars });
        if self.changes.len() > CHANGES_KEPT {
            self.changes.pop_front();
        }
    }

    pub fn changes_since(&self, version: u64) -> Option<Vec<zen_syntax::TextChange>> {
        self.versions_since(version, |changes| &changes.edits)
    }

    pub fn char_edits_since(&self, version: u64) -> Option<Vec<CharEdit>> {
        self.versions_since(version, |changes| &changes.chars)
    }

    fn versions_since<T: Clone>(&self, version: u64, edits_of: impl Fn(&VersionChanges) -> &[T]) -> Option<Vec<T>> {
        if version == self.version {
            return Some(Vec::new());
        }
        let at = self.changes.iter().position(|changes| changes.from == version)?;
        let mut edits = Vec::new();
        let mut reached = version;
        for changes in self.changes.range(at..) {
            if changes.from != reached {
                return None;
            }
            edits.extend_from_slice(edits_of(changes));
            reached = changes.to;
        }
        (reached == self.version).then_some(edits)
    }

    fn applied(&mut self, edit: &Edit) {
        let whole = (!self.breakpoint_marks.is_empty() || !self.bookmark_marks.is_empty())
            && edit.range.start == 0
            && edit.range.end > 0
            && edit.range.end == self.mirror.chars().count();
        let lines =
            if whole { debug::breakpoints::mark_lines(&self.mirror, &self.breakpoint_marks) } else { Vec::new() };
        let inserted = edit.text.chars().count();
        let region_edit = if !self.manual.is_empty() && !edit.range.is_empty() && inserted > 0 {
            narrowed(&self.mirror, edit)
        } else {
            CharEdit { range: edit.range.clone(), inserted }
        };
        let bookmark_lines =
            if whole { debug::breakpoints::mark_lines(&self.mirror, &self.bookmark_marks) } else { Vec::new() };
        let before = (whole && !self.bookmark_marks.is_empty()).then(|| self.mirror.clone());
        let (start, end) = apply_to_string(&mut self.mirror, edit);
        self.pending.push(zen_syntax::TextChange { start, old_end: end, new_end: start + edit.text.len() });
        self.pending_chars.push(CharEdit { range: edit.range.clone(), inserted: edit.text.chars().count() });
        if self.template.as_mut().is_some_and(|session| !session.map(edit)) {
            self.template = None;
        }
        let edits = std::slice::from_ref(edit);
        for mark in self.marks.iter_mut().chain(self.edit_places.iter_mut().map(|(offset, _, _)| offset)) {
            *mark = zen_editor::Selection::caret(*mark).mapped(edits).head;
        }
        if whole {
            self.breakpoint_marks = debug::breakpoints::line_marks(&self.mirror, &lines);
            let bookmark_lines = match &before {
                Some(before) => bookmarks::follow_lines(before, &self.mirror, &bookmark_lines),
                None => bookmark_lines,
            };
            self.bookmark_marks = debug::breakpoints::line_marks(&self.mirror, &bookmark_lines);
        } else {
            debug::breakpoints::map_marks(&mut self.breakpoint_marks, edits);
            debug::breakpoints::map_marks(&mut self.bookmark_marks, edits);
        }
        let moved = |range: &std::ops::Range<usize>| {
            let moved = zen_editor::Selection::new(range.start, range.end).mapped(edits);
            (moved.anchor < moved.head).then_some(moved.anchor..moved.head)
        };
        let manual = std::mem::take(&mut self.manual);
        let fold_moved = |range: &std::ops::Range<usize>| {
            if manual.contains(range) { region_edit.map_region(range) } else { moved(range) }
        };
        let folded = self.folds.len() + manual.len();
        self.folds = self.folds.iter().filter_map(fold_moved).collect();
        self.manual = manual.iter().filter_map(|range| region_edit.map_region(range)).collect();
        self.manual.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        self.manual.dedup();
        if self.folds.len() + self.manual.len() != folded {
            self.folds_version += 1;
        }
        self.find_excluded = self.find_excluded.iter().filter_map(moved).collect();
        self.find_scope = self.find_scope.as_ref().and_then(moved);
        self.occurrences_added.clear();
        for (_, view) in &mut self.views {
            view.selections = view.selections.mapped(edits);
            view.folds = view.folds.iter().filter_map(fold_moved).collect();
        }
        if let Some((_, foldable)) = &mut self.foldable {
            let folds = Arc::make_mut(foldable);
            folds.retain_mut(|fold| match fold_moved(&fold.range) {
                Some(range) => {
                    fold.range = range;
                    true
                }
                None => false,
            });
        }
    }

    fn applied_all(&mut self, edits: &[Edit]) {
        let descending = edits.windows(2).all(|pair| pair[1].range.end <= pair[0].range.start);
        let whole = edits.iter().any(|edit| edit.range.start == 0 && edit.range.end > 0);
        let marked = !self.breakpoint_marks.is_empty() || !self.bookmark_marks.is_empty();
        if edits.len() < 2 || !descending || (whole && marked) || !self.manual.is_empty() {
            for edit in edits {
                self.applied(edit);
            }
            return;
        }
        let mut chars = 0;
        let mut bytes = self.mirror.char_indices().map(|(byte, _)| byte).chain(std::iter::once(self.mirror.len()));
        let mut byte = bytes.next().unwrap_or(0);
        let mut byte_of = |char: usize| {
            while chars < char {
                byte = bytes.next().unwrap_or(byte);
                chars += 1;
            }
            byte
        };
        let mut mirror =
            String::with_capacity(self.mirror.len() + edits.iter().map(|edit| edit.text.len()).sum::<usize>());
        let mut ranges = Vec::with_capacity(edits.len());
        let mut copied = 0;
        for edit in edits.iter().rev() {
            let (start, end) = (byte_of(edit.range.start), byte_of(edit.range.end));
            mirror.push_str(&self.mirror[copied..start]);
            mirror.push_str(&edit.text);
            copied = end;
            ranges.push((start, end));
        }
        mirror.push_str(&self.mirror[copied..]);
        self.mirror = mirror;
        for (edit, &(start, end)) in edits.iter().zip(ranges.iter().rev()) {
            self.pending.push(zen_syntax::TextChange { start, old_end: end, new_end: start + edit.text.len() });
            self.pending_chars.push(CharEdit { range: edit.range.clone(), inserted: edit.text.chars().count() });
            if self.template.as_mut().is_some_and(|session| !session.map(edit)) {
                self.template = None;
            }
        }
        let ascending: Vec<(usize, usize, usize)> =
            edits.iter().rev().map(|edit| (edit.range.start, edit.range.end, edit.text.chars().count())).collect();
        let mut shifts = vec![0isize];
        for &(start, end, inserted) in &ascending {
            shifts.push(shifts[shifts.len() - 1] + inserted as isize - (end - start) as isize);
        }
        let map = |offset: usize| {
            let below = ascending.partition_point(|&(_, end, _)| end <= offset);
            let base = match ascending.get(below) {
                Some(&(start, _, inserted)) if offset > start => start + inserted,
                _ => offset,
            };
            base.saturating_add_signed(shifts[below])
        };
        for mark in self.marks.iter_mut().chain(self.edit_places.iter_mut().map(|(offset, _, _)| offset)) {
            *mark = map(*mark);
        }
        debug::breakpoints::map_marks(&mut self.breakpoint_marks, edits);
        debug::breakpoints::map_marks(&mut self.bookmark_marks, edits);
        let moved = |range: &std::ops::Range<usize>| {
            let (start, end) = (map(range.start), map(range.end));
            (start < end).then_some(start..end)
        };
        let folded = self.folds.len();
        self.folds = self.folds.iter().filter_map(moved).collect();
        if self.folds.len() != folded {
            self.folds_version += 1;
        }
        self.find_excluded = self.find_excluded.iter().filter_map(moved).collect();
        self.find_scope = self.find_scope.as_ref().and_then(moved);
        self.occurrences_added.clear();
        for (_, view) in &mut self.views {
            view.selections = view.selections.mapped(edits);
            view.folds = view.folds.iter().filter_map(moved).collect();
        }
        if let Some((_, foldable)) = &mut self.foldable {
            let folds = Arc::make_mut(foldable);
            folds.retain_mut(|fold| match moved(&fold.range) {
                Some(range) => {
                    fold.range = range;
                    true
                }
                None => false,
            });
        }
    }

    pub fn template_session(&self) -> Option<&templates::TemplateSession> {
        self.template.as_ref()
    }

    pub fn soft_wrap(&self) -> Option<bool> {
        self.soft_wrap
    }

    pub fn set_soft_wrap(&mut self, on: Option<bool>) {
        self.soft_wrap = on;
    }

    pub fn font_size(&self) -> Option<f32> {
        self.font_size
    }

    pub fn set_font_size(&mut self, size: Option<f32>) {
        self.font_size = size;
    }

    pub fn whitespaces(&self) -> Option<bool> {
        self.whitespaces
    }

    pub fn set_whitespaces(&mut self, on: Option<bool>) {
        self.whitespaces = on;
    }

    pub fn indent_guides(&self) -> Option<bool> {
        self.indent_guides
    }

    pub fn set_indent_guides(&mut self, on: Option<bool>) {
        self.indent_guides = on;
    }

    pub fn line_numbers(&self) -> Option<bool> {
        self.line_numbers
    }

    pub fn set_line_numbers(&mut self, on: Option<bool>) {
        self.line_numbers = on;
    }

    pub fn column_mode(&self) -> bool {
        self.column_mode
    }

    pub fn set_column_mode(&mut self, on: bool) {
        self.column_mode = on;
        self.column = None;
    }

    pub fn extend_column(&mut self, movement: zen_editor::Movement) {
        let text = self.buffer.text();
        let (anchor, head) = match &self.column {
            Some((anchor, head, made)) if *made == self.selections => (*anchor, *head),
            _ => {
                let primary = self.selections.primary();
                let point = |offset| text.offset_to_point(offset).unwrap_or_default();
                (point(primary.anchor), point(primary.head))
            }
        };
        let head = zen_editor::column_head(text, head, movement);
        let made = zen_editor::column_selection(text, anchor, head);
        self.set_selections(made.clone());
        self.column = Some((anchor, head, self.selections.clone()));
    }

    pub fn folds(&self) -> &[std::ops::Range<usize>] {
        &self.folds
    }

    pub fn find_excluded(&self) -> &[std::ops::Range<usize>] {
        &self.find_excluded
    }

    pub(crate) fn set_find_excluded(&mut self, mut excluded: Vec<std::ops::Range<usize>>) {
        excluded.sort_by_key(|range| range.start);
        self.find_excluded = excluded;
    }

    pub fn fold_all(&mut self, ranges: impl IntoIterator<Item = std::ops::Range<usize>>) {
        self.folds.extend(ranges);
        self.folds.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        self.folds.dedup();
        self.selection_version += 1;
        self.folds_version += 1;
    }

    pub fn find_scope(&self) -> Option<std::ops::Range<usize>> {
        self.find_scope.clone()
    }

    pub(crate) fn set_find_scope(&mut self, scope: Option<std::ops::Range<usize>>) {
        self.find_scope = scope.filter(|scope| !scope.is_empty());
    }

    pub(crate) fn occurrences_added(&mut self) -> &mut Vec<std::ops::Range<usize>> {
        &mut self.occurrences_added
    }

    pub fn set_folded(&mut self, range: std::ops::Range<usize>, folded: bool) {
        self.folds.retain(|other| *other != range);
        if folded {
            self.folds.push(range);
            self.folds.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        }
        self.selection_version += 1;
        self.folds_version += 1;
    }

    pub fn folds_version(&self) -> u64 {
        self.folds_version
    }

    pub fn folded_lines(&self) -> Vec<folding::FoldLines> {
        let text = self.buffer.text();
        let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
        self.folds
            .iter()
            .filter(|range| !self.manual.contains(range))
            .map(|range| folding::FoldLines { start_line: line(range.start), end_line: line(range.end) })
            .collect()
    }

    pub fn manual_lines(&self) -> Vec<folding::ManualRegion> {
        let text = self.buffer.text();
        let point = |offset: usize| text.offset_to_point(offset).unwrap_or_default();
        self.manual
            .iter()
            .map(|range| {
                let (start, end) = (point(range.start), point(range.end));
                folding::ManualRegion {
                    start_line: start.line,
                    start_column: start.column,
                    end_line: end.line,
                    end_column: end.column,
                    folded: self.folds.contains(range),
                    text_hash: text.slice(range.clone()).ok().map(|held| folding::ManualRegion::hash(&held)),
                }
            })
            .collect()
    }

    pub fn fold_state(&self) -> folding::FileFolds {
        folding::FileFolds { path: self.path.clone(), folds: self.folded_lines(), manual: self.manual_lines() }
    }

    pub fn manual_regions(&self) -> &[std::ops::Range<usize>] {
        &self.manual
    }

    pub fn add_manual_region(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() || range.end > self.buffer.text().len_chars() || self.manual.contains(&range) {
            return;
        }
        self.manual.push(range.clone());
        self.manual.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        if let Some((_, foldable)) = &mut self.foldable {
            let folds = Arc::make_mut(foldable);
            *folds = with_manual(std::mem::take(folds), std::slice::from_ref(&range));
        }
        self.folds_version += 1;
    }

    pub fn remove_manual_region(&mut self, range: &std::ops::Range<usize>) {
        self.manual.retain(|other| other != range);
        if let Some((_, foldable)) = &mut self.foldable {
            Arc::make_mut(foldable).retain(|fold| !(fold.kind == zen_editor::FoldKind::Manual && fold.range == *range));
        }
        self.set_folded(range.clone(), false);
    }

    pub fn foldable_wanted(&self, current: bool) -> bool {
        self.image.is_none() && !matches!(&self.foldable, Some((version, _)) if *version == self.version || !current)
    }

    pub fn foldable(&self) -> Vec<zen_editor::Fold> {
        match &self.foldable {
            _ if self.image.is_some() => Vec::new(),
            Some((version, folds)) if *version == self.version => folds.as_ref().clone(),
            _ => with_manual(zen_editor::folds(&self.mirror), &self.manual),
        }
    }

    pub fn foldable_listed(&mut self, current: bool) -> (Arc<Vec<zen_editor::Fold>>, bool) {
        self.foldable_listed_by(current, |document| zen_editor::folds(&document.mirror))
    }

    pub fn foldable_listed_by(
        &mut self,
        current: bool,
        read: impl FnOnce(&Self) -> Vec<zen_editor::Fold>,
    ) -> (Arc<Vec<zen_editor::Fold>>, bool) {
        if self.image.is_some() {
            return (Arc::default(), true);
        }
        match &self.foldable {
            Some((version, folds)) if *version == self.version || !current => {
                (Arc::clone(folds), *version == self.version)
            }
            _ => {
                let folds = Arc::new(with_manual(read(self), &self.manual));
                self.foldable = Some((self.version, Arc::clone(&folds)));
                (folds, true)
            }
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.buffer.undo_depth()
    }

    pub fn close_undo_group(&mut self) {
        self.buffer.close_group();
    }

    pub fn merge_undo_steps(&mut self, count: usize) -> bool {
        self.buffer.merge_last(count)
    }

    pub fn undo(&mut self) -> bool {
        self.replay(Buffer::undo)
    }

    pub fn redo(&mut self) -> bool {
        self.replay(Buffer::redo)
    }

    fn replay(&mut self, step: fn(&mut Buffer) -> Option<Vec<Edit>>) -> bool {
        if self.refuses_edits() {
            return false;
        }
        let Some(edits) = step(&mut self.buffer) else { return false };
        self.template = None;
        for edit in &edits {
            self.applied(edit);
        }
        if let Some(last) = edits.last() {
            let end = last.inserted_range().end;
            self.selections = zen_editor::Selections::single(zen_editor::Selection::caret(end));
            self.selection_version += 1;
            self.edited_at(end);
        }
        self.raise_version();
        self.dirty = *self.buffer.text() != self.saved;
        self.detect_line_separator();
        true
    }

    fn encoded(&self, text: &str) -> Result<Vec<u8>, zen_vfs::VfsError> {
        let Some((charset, bom)) = self.encoding else { return Ok(text.as_bytes().to_vec()) };
        charset.encode(text, bom).map_err(|unmappable| {
            let message = t!(
                "workbench-error-unmappable",
                charset = charset.label(),
                character = unmappable.character.to_string(),
                code = format!("U+{:04X}", u32::from(unmappable.character)),
                line = unmappable.line,
                column = unmappable.column
            );
            let source = std::io::Error::new(std::io::ErrorKind::InvalidData, message);
            zen_vfs::VfsError { action: "save", path: self.path.clone(), source }
        })
    }

    pub(crate) fn decoded(&self, bytes: &[u8]) -> Option<String> {
        match self.encoding {
            Some((charset, _)) => charset.decode(bytes).map(|(text, _)| text),
            None => std::str::from_utf8(bytes).ok().map(str::to_owned),
        }
    }

    fn read_disk(&self) -> Result<String, zen_vfs::VfsError> {
        let Some((charset, _)) = self.encoding else { return zen_vfs::read_text(&self.path) };
        let bytes = std::fs::read(&self.path).map_err(|source| zen_vfs::VfsError {
            action: "read",
            path: self.path.clone(),
            source,
        })?;
        charset.decode(&bytes).map(|(text, _)| text).ok_or_else(|| {
            let source = std::io::Error::new(std::io::ErrorKind::InvalidData, charset.label());
            zen_vfs::VfsError { action: "read", path: self.path.clone(), source }
        })
    }

    fn save(&mut self) -> Result<(), zen_vfs::VfsError> {
        if self.image.is_some() {
            return Ok(());
        }
        self.refuse_read_only()?;
        if !self.dirty && self.changed_on_disk().unwrap_or(false) {
            self.sync_with_disk();
            return Ok(());
        }
        let stamp = self.check_disk()?;
        let bytes = self.encoded(&self.mirror)?;
        self.write(&bytes, stamp)?;
        self.saved_now();
        Ok(())
    }

    fn saved_now(&mut self) {
        let before = std::mem::replace(&mut self.saved, self.buffer.text().clone());
        self.save_count += 1;
        self.saves.push(local_history::SavedText {
            time: local_history::now(),
            before,
            after: self.saved.clone(),
            encoding: self.encoding,
        });
        self.dirty = false;
        self.conflict = false;
        self.kept_disk = None;
    }
}

fn next_version() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn with_manual(mut folds: Vec<zen_editor::Fold>, manual: &[std::ops::Range<usize>]) -> Vec<zen_editor::Fold> {
    let added =
        manual.iter().filter(|range| !folds.iter().any(|fold| fold.range == **range)).cloned().collect::<Vec<_>>();
    if added.is_empty() {
        return folds;
    }
    folds.extend(added.into_iter().map(|range| zen_editor::Fold::new(range, zen_editor::FoldKind::Manual)));
    folds.sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
    folds
}

fn narrowed(text: &str, edit: &Edit) -> CharEdit {
    let byte = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(index, _)| index);
    let old = &text[byte(edit.range.start)..byte(edit.range.end)];
    let new = edit.text.as_str();
    let inserted = new.chars().count();
    let prefix = old.chars().zip(new.chars()).take_while(|(old, new)| old == new).count();
    let suffix = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take(edit.range.len().min(inserted) - prefix)
        .take_while(|(old, new)| old == new)
        .count();
    CharEdit { range: edit.range.start + prefix..edit.range.end - suffix, inserted: inserted - prefix - suffix }
}

fn next_edit_order() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn apply_to_string(text: &mut String, edit: &Edit) -> (usize, usize) {
    let byte = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(index, _)| index);
    let (start, end) = (byte(edit.range.start), byte(edit.range.end));
    text.replace_range(start..end, &edit.text);
    (start, end)
}

#[derive(Debug, Default)]
pub struct Workbench {
    project: Option<PathBuf>,
    documents: Vec<Document>,
    active: Option<usize>,
    messages: Vec<String>,
    revision: u64,
    shown: u64,
    find: find::FindState,
    index: Option<zen_index::FileIndex>,
    recent: Vec<PathBuf>,
    text_search: search::TextSearch,
    replace_search: Option<search::ReplaceSearch>,
    groups: groups::EditorGroups,
    remembered_folds: Vec<folding::FileFolds>,
    closed_tabs: Vec<tabs::ClosedTab>,
    pub(crate) agent_hidden: Option<Vec<String>>,
}

const EDIT_PLACES_KEPT: usize = history::LIMIT;

const FOLDS_REMEMBERED: usize = 200;

impl Workbench {
    pub fn remembered_folds(&self, path: &Path) -> Option<&folding::FileFolds> {
        self.remembered_folds.iter().rev().find(|file| file.path == path)
    }

    pub fn remember_folds(&mut self, folds: folding::FileFolds) {
        self.remembered_folds.retain(|file| file.path != folds.path);
        self.remembered_folds.push(folds);
        if self.remembered_folds.len() > FOLDS_REMEMBERED {
            self.remembered_folds.remove(0);
        }
    }

    pub fn fold_states(&self) -> Vec<folding::FileFolds> {
        let open = self.documents.iter().filter(|document| document.image().is_none());
        let mut states: Vec<folding::FileFolds> = self
            .remembered_folds
            .iter()
            .filter(|file| !self.documents.iter().any(|document| document.path() == file.path))
            .cloned()
            .collect();
        states.extend(open.map(Document::fold_state));
        states
    }

    fn forget_document(&mut self, path: &Path) {
        if let Some(document) =
            self.documents.iter().find(|document| document.path() == path && document.image().is_none())
        {
            let folds = document.fold_state();
            self.remember_folds(folds);
        }
        self.documents.retain(|document| document.path() != path);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn shown(&self) -> u64 {
        self.shown
    }

    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    pub fn file_index(&self) -> Option<&zen_index::FileIndex> {
        self.index.as_ref()
    }

    pub fn spelled(&self, path: &Path) -> PathBuf {
        match &self.project {
            Some(root) => zen_vfs::spelled_from(root, path),
            None => path.to_owned(),
        }
    }

    pub fn resolve(&self, path: &Path) -> PathBuf {
        match &self.project {
            Some(root) if path.is_relative() && !path.as_os_str().is_empty() => root.join(path),
            _ => self.spelled(path),
        }
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn active_document(&self) -> Option<&Document> {
        self.active.and_then(|index| self.documents.get(index))
    }

    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.active.and_then(|index| self.documents.get_mut(index))
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent
    }

    fn touch_recent(&mut self) {
        let Some(path) = self.active_document().map(|document| document.path().to_owned()) else { return };
        self.recent.retain(|recent| *recent != path);
        self.recent.insert(0, path);
        self.recent.truncate(50);
    }

    pub fn find(&self) -> &find::FindState {
        &self.find
    }

    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    pub(crate) fn relocated(&mut self, from: &Path, to: &Path) {
        let follow = |path: &mut PathBuf| {
            if let Ok(rest) = path.strip_prefix(from) {
                *path = if rest.as_os_str().is_empty() { to.to_owned() } else { to.join(rest) };
            }
        };
        for document in &mut self.documents {
            follow(&mut document.path);
        }
        self.recent.iter_mut().for_each(follow);
        self.groups.relocate(from, to);
        self.sync_groups();
        self.revision += 1;
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(OpenProject)?;
    registry.register(OpenDocument)?;
    registry.register(SaveDocument)?;
    registry.register(SaveAll)?;
    registry.register(CloseDocument)?;
    registry.register(SelectTab)?;
    registry.register(Undo)?;
    registry.register(Redo)?;
    registry.register(NewFile)?;
    registry.register(NewDirectory)?;
    registry.register(CopyInto)?;
    registry.register(RenameEntry)?;
    registry.register(DeleteEntries)?;
    registry.register(ZoomImage)?;
    registry.register(ShowImageInfo)?;
    editing::register(registry)?;
    closing::register(registry)?;
    tabs::register(registry)?;
    autosave::register(registry)?;
    expand_word::register(registry)?;
    clipboard_history::register(registry)?;
    file_clipboard::register(registry)?;
    file_actions::register(registry)?;
    file_encodings::register(registry)?;
    compare::register(registry)?;
    file_types::register(registry)?;
    compare_folders::register(registry)?;
    find::register(registry)?;
    find_history::register(registry)?;
    lines::register(registry)?;
    on_save::register(registry)?;
    line_separators::register(registry)?;
    editor_config::register(registry)?;
    code_style_options::register(registry)?;
    disk::register(registry)?;
    navigate::register(registry)?;
    search::register(registry)?;
    find_results::register(registry)?;
    search_everywhere::register(registry)?;
    run_anything::register(registry)?;
    search_scopes::register(registry)?;
    inspect_code::register(registry)?;
    inspection_profiles::register(registry)?;
    suppression::register(registry)?;
    templates::register(registry)?;
    file_templates::register(registry)?;
    text_replace::register(registry)?;
    language::register(registry)?;
    jdk::register(registry)?;
    jvm_libraries::register(registry)?;
    java::bytecode::register(registry)?;
    java::console::register(registry)?;
    folding::register(registry)?;
    font_size::register(registry)?;
    read_only::register(registry)?;
    soft_wrap::register(registry)?;
    spelling::register(registry)?;
    visual_aids::register(registry)?;
    line_numbers::register(registry)?;
    file_indent::register(registry)?;
    sticky::register(registry)?;
    groups::register(registry)?;
    plugins::register(registry)?;
    plugin_projects::register(registry)?;
    schemas::register(registry)?;
    plugin_runs::register(registry)?;
    project_index::register(registry)?;
    #[cfg(not(target_arch = "wasm32"))]
    plugin_tools::register(registry)?;
    new_project::register(registry)?;
    build_sync::register(registry)?;
    trust::register(registry)?;
    maven::register(registry)?;
    java::jacoco::register(registry)?;
    gradle::register(registry)?;
    dependency_analysis::register(registry)?;
    ant::register(registry)?;
    npm::register(registry)?;
    resource_bundles::register(registry)?;
    spring_boot::register(registry)?;
    dependencies::register(registry)?;
    java::processing::register(registry)?;
    preview::register(registry)?;
    markdown::register(registry)?;
    code_quality::register(registry)?;
    occurrences::register(registry)?;
    brackets::register(registry)?;
    extend_selection::register(registry)?;
    history::register(registry)?;
    bookmarks::register(registry)?;
    http::register(registry)?;
    #[cfg(not(target_arch = "wasm32"))]
    network::register(registry)?;
    local_history::register(registry)?;
    login_env::register(registry)?;
    internal_errors::register(registry)?;
    macros::register(registry)?;
    project_model::register(registry)?;
    project_settings::register(registry)?;
    code_style_scheme::register(registry)?;
    actions_on_save::register(registry)?;
    project_structure::register(registry)?;
    artifacts::register(registry)?;
    build_triggers::register(registry)?;
    run::register(registry)?;
    test_history::register(registry)?;
    coverage::register(registry)?;
    profile::register(registry)?;
    debug::register(registry)?;
    vcs::register(registry)?;
    terminal::register(registry)?;
    notifications::register(registry)?;
    tasks::register(registry)?;
    todo::register(registry)?;
    caffeine::register(registry)?;
    scratch::register(registry)?;
    unwrap::register(registry)?;
    todo_patterns::register(registry)?;
    workspaces::register(registry)?;
    external_tools::register(registry)?;
    file_watchers::register(registry)?;
    power_save::register(registry)?;
    activity_monitor::register(registry)?;
    diagnostics::register(registry)?;
    maintenance::register(registry)?;
    Ok(())
}

/// Files or folders to delete.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct DeleteParams {
    /// Files and folders inside the project (not the project folder itself): absolute, or relative
    /// to the project; scratch files and buffers too, absolute.
    pub paths: Vec<PathBuf>,
}

fn project_path(workbench: &Workbench, path: &Path) -> PathBuf {
    match &workbench.project {
        Some(root) if path.as_os_str().is_empty() || path == Path::new(".") => root.clone(),
        _ => workbench.resolve(path),
    }
}

pub struct DeleteEntries;

impl Command for DeleteEntries {
    type Params = DeleteParams;
    type Output = Vec<PathBuf>;
    const ID: &'static str = "file.fs.delete";
    const TITLE: &'static str = "Delete";
    const DESCRIPTION: &'static str = "Moves files and folders of the project (or scratch files) to the system Trash, from where \
         they can be restored, and closes their tabs (unsaved changes in them are lost). Language servers that ask \
         to hear of deletes are told, and the changes they make first are applied (one Undo reverts them). Returns \
         the paths moved.";

    fn run(&self, session: &mut Session, params: DeleteParams) -> Result<Vec<PathBuf>, CommandError> {
        let deleted = delete_entries(session, &params.paths)?;
        Ok(deleted.iter().map(|path| told_path(session, path)).collect())
    }
}

pub(crate) fn delete_entries(session: &mut Session, paths: &[PathBuf]) -> Result<Vec<PathBuf>, CommandError> {
    let scratches = session.state::<scratch::Scratches>().cloned().unwrap_or_default();
    let workbench = session.state_mut::<Workbench>();
    let project = workbench.project.clone().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let paths: Vec<PathBuf> = paths.iter().map(|path| project_path(workbench, path)).collect();
    hidden_files::guard_change(session, &project, &paths)?;
    for path in &paths {
        if !scratches.contains(path) && (path == &project || !inside_project(&project, path)) {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = display(path))));
        }
    }
    let touched: Vec<FileTouch> = paths.iter().map(|path| FileTouch::existing(path, None)).collect();
    let asked = language::file_operations::will(session, FileOperation::Delete, &touched);
    let workbench = session.state_mut::<Workbench>();
    let mut deleted = Vec::new();
    let mut failure = None;
    for path in &paths {
        #[cfg(not(target_arch = "wasm32"))]
        let result = zen_vfs::move_to_trash(path).map_err(|error| error.source.to_string());
        #[cfg(target_arch = "wasm32")]
        let result: Result<(), String> = Err("not available here".to_owned());
        match result {
            Ok(()) => deleted.push(path.clone()),
            Err(error) => failure = Some(t!("workbench-error-delete", path = display(path), error = error)),
        }
    }
    workbench.documents.retain(|document| !deleted.iter().any(|gone| document.path.starts_with(gone)));
    workbench.groups.retain(|path| !deleted.iter().any(|gone| path.starts_with(gone)));
    workbench.sync_groups();
    if !deleted.is_empty() {
        workbench.revision += 1;
        workbench.messages.push(t!("workbench-deleted", count = deleted.len()));
    }
    for path in &deleted {
        file_types::relocate_overrides(session, path, None);
    }
    bookmarks::forget(session, &deleted);
    let gone: Vec<FileTouch> = touched.into_iter().filter(|file| deleted.contains(&file.path)).collect();
    language::file_operations::did(session, FileOperation::Delete, &gone);
    language::file_operations::remember(session, asked, &[]);
    match failure {
        Some(error) => Err(CommandError::new(error)),
        None => Ok(deleted),
    }
}

/// A change of an image's zoom: one of `action` or `scale`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ZoomParams {
    /// `in` and `out` step the zoom, `actual` shows the image at its own size, `fit` fits it to the
    /// window.
    pub action: Option<ZoomAction>,
    /// A fixed zoom instead, 1.0 being the image's own size (from 0.02 to 32).
    pub scale: Option<f32>,
}

/// A zoom step of the image viewer's toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ZoomAction {
    /// Zoom In.
    In,
    /// Zoom Out.
    Out,
    /// Actual Size (100 %).
    Actual,
    /// Fit Zoom to Window.
    Fit,
}

fn active_image(session: &mut Session) -> Result<&mut ImageView, CommandError> {
    session
        .state_mut::<Workbench>()
        .active_document_mut()
        .and_then(Document::image_mut)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-image")))
}

fn has_active_image(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image.is_some())
}

pub struct ZoomImage;

impl Command for ZoomImage {
    type Params = ZoomParams;
    type Output = f32;
    const ID: &'static str = "editor.image.zoom";
    const TITLE: &'static str = "Zoom";
    const DESCRIPTION: &'static str = "Zooms the image in the active tab, or the preview of the active file (an SVG) while it is shown: in, out, to its actual size, to fit the window, or to a given scale; returns the scale.";

    fn is_available(&self, session: &Session) -> bool {
        has_active_image(session) || preview::active(session).is_some_and(|(_, kind)| kind == preview::PreviewKind::Svg)
    }

    fn run(&self, session: &mut Session, params: ZoomParams) -> Result<f32, CommandError> {
        if !has_active_image(session)
            && let Some((path, _)) = preview::active(session).filter(|(_, kind)| *kind == preview::PreviewKind::Svg)
        {
            let previews = session.state_mut::<preview::Previews>();
            let scale = previews.scale(&path);
            let zoom = match (params.action, params.scale) {
                (_, Some(scale)) => ImageZoom::Scale(scale),
                (Some(ZoomAction::In), None) => ImageZoom::Scale(scale * ZOOM_STEP),
                (Some(ZoomAction::Out), None) => ImageZoom::Scale(scale / ZOOM_STEP),
                (Some(ZoomAction::Actual), None) => ImageZoom::Scale(1.0),
                (Some(ZoomAction::Fit), None) => ImageZoom::Fit,
                (None, None) => return Err(CommandError::new(t!("workbench-error-zoom"))),
            };
            previews.set_zoom(&path, zoom);
            return Ok(previews.scale(&path));
        }
        let image = active_image(session)?;
        match (params.action, params.scale) {
            (_, Some(scale)) => image.set_scale(scale),
            (Some(ZoomAction::In), None) => image.set_scale(image.scale() * ZOOM_STEP),
            (Some(ZoomAction::Out), None) => image.set_scale(image.scale() / ZOOM_STEP),
            (Some(ZoomAction::Actual), None) => image.set_scale(1.0),
            (Some(ZoomAction::Fit), None) => image.zoom = ImageZoom::Fit,
            (None, None) => return Err(CommandError::new(t!("workbench-error-zoom"))),
        }
        Ok(image.scale())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub path: PathBuf,
    pub format: String,
    pub width: f32,
    pub height: f32,
    pub bits_per_pixel: Option<u16>,
    pub file_size: u64,
    pub scale: f32,
    pub loading: bool,
    pub error: Option<String>,
}

pub struct ShowImageInfo;

impl Command for ShowImageInfo {
    type Params = NoParams;
    type Output = ImageInfo;
    const ID: &'static str = "editor.image.info";
    const TITLE: &'static str = "Image Info";
    const DESCRIPTION: &'static str =
        "Returns the size, format, colour depth, file size and zoom of the image in the active tab.";

    fn is_available(&self, session: &Session) -> bool {
        has_active_image(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ImageInfo, CommandError> {
        let workbench = session.state::<Workbench>();
        let no_image = || CommandError::new(t!("workbench-error-no-image"));
        let document = workbench.and_then(Workbench::active_document).ok_or_else(no_image)?;
        let image = document.image().ok_or_else(no_image)?;
        let picture = image.picture();
        let (width, height) = picture.map_or((0.0, 0.0), |picture| picture.natural_size());
        Ok(ImageInfo {
            path: document.path.clone(),
            format: picture.map_or_else(String::new, |picture| picture.format.to_owned()),
            width,
            height,
            bits_per_pixel: picture.and_then(|picture| picture.bits_per_pixel),
            file_size: image.file_size,
            scale: image.scale(),
            loading: image.is_loading(),
            error: image.error().map(str::to_owned),
        })
    }
}

/// A name to create in a folder.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct NewEntryParams {
    /// The folder, inside the project: absolute, or relative to the project (`.` is the project folder).
    pub directory: PathBuf,
    /// The new file's or folder's name; a relative path such as `pkg/util.go` creates the folders
    /// on the way.
    pub name: String,
}

/// Files or folders to copy into a folder of the project.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CopyIntoParams {
    /// Absolute paths of the files and folders to copy, from anywhere.
    pub sources: Vec<PathBuf>,
    /// The folder to copy them into, inside the project: absolute, or relative to the project.
    pub directory: PathBuf,
}

pub(crate) fn told_path(session: &Session, path: &Path) -> PathBuf {
    if !zen_core::macros::by_agent(session) {
        return path.to_owned();
    }
    let root = session.state::<Workbench>().and_then(Workbench::project);
    root.and_then(|root| path.strip_prefix(root).ok())
        .filter(|rest| !rest.as_os_str().is_empty())
        .map_or_else(|| path.to_owned(), Path::to_owned)
}

pub fn inside_project(project: &Path, path: &Path) -> bool {
    path.strip_prefix(project)
        .is_ok_and(|rest| rest.components().all(|part| matches!(part, std::path::Component::Normal(_))))
}

pub(crate) fn project_folder(workbench: &Workbench, directory: &Path) -> Result<PathBuf, CommandError> {
    let project = workbench.project.as_deref().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let directory = project_path(workbench, directory);
    if !inside_project(project, &directory) || !directory.is_dir() {
        return Err(CommandError::new(t!("workbench-error-outside-project", path = display(&directory))));
    }
    Ok(directory)
}

fn new_entry_path(directory: &Path, name: &str) -> Result<PathBuf, CommandError> {
    let relative = Path::new(name.trim());
    let valid =
        !name.trim().is_empty() && relative.components().all(|part| matches!(part, std::path::Component::Normal(_)));
    if !valid {
        return Err(CommandError::new(t!("workbench-error-bad-name", name = name)));
    }
    Ok(directory.join(relative))
}

pub struct NewFile;

impl Command for NewFile {
    type Params = NewEntryParams;
    type Output = PathBuf;
    const ID: &'static str = "file.fs.newFile";
    const TITLE: &'static str = "File";
    const DESCRIPTION: &'static str = "Creates an empty file in a project folder and opens it in the editor. Language servers \
         that ask to hear of new files are told, and the changes they make first are applied.";

    fn run(&self, session: &mut Session, params: NewEntryParams) -> Result<PathBuf, CommandError> {
        let origin = history::caret(session);
        let workbench = session.state_mut::<Workbench>();
        let directory = project_folder(workbench, &params.directory)?;
        let path = new_entry_path(&directory, &params.name)?;
        if let Some(root) = workbench.project().map(Path::to_owned) {
            hidden_files::guard_change(session, &root, std::slice::from_ref(&path))?;
        }
        let touched = [FileTouch::new_entry(&path, false)];
        let asked = language::file_operations::will(session, FileOperation::Create, &touched);
        let created = path
            .parent()
            .map_or(Ok(()), |parent| {
                std::fs::create_dir_all(parent).map_err(|error| {
                    CommandError::new(t!("workbench-error-create", path = display(parent), error = error.to_string()))
                })
            })
            .and_then(|()| {
                zen_vfs::create_file(&path).map_err(|error| {
                    CommandError::new(t!(
                        "workbench-error-create",
                        path = display(&path),
                        error = error.source.to_string()
                    ))
                })
            });
        if let Err(error) = created {
            language::file_operations::forget(session, &asked);
            return Err(error);
        }
        language::file_operations::did(session, FileOperation::Create, &touched);
        language::file_operations::remember(session, asked, &[]);
        open_created(session, &path, origin)?;
        Ok(path)
    }
}

pub(crate) fn open_created(
    session: &mut Session,
    path: &Path,
    origin: Option<history::Spot>,
) -> Result<(), CommandError> {
    session.state_mut::<Workbench>().revision += 1;
    let editor_charset = editor_config::charset(session, path);
    let typed = matches!(file_types::opening(path), Ok(file_types::Opening::Text { typed: true }));
    let mut document = open_text(path, None, editor_charset, typed).map_err(|error| {
        CommandError::new(t!("workbench-error-open", path = display(path), error = error.to_string()))
    })?;
    line_separators::settle(session, &mut document);
    let workbench = session.state_mut::<Workbench>();
    workbench.documents.push(document);
    workbench.groups.open(path);
    workbench.sync_groups();
    workbench.shown += 1;
    workbench.touch_recent();
    history::left_file(session, origin);
    Ok(())
}

pub struct NewDirectory;

impl Command for NewDirectory {
    type Params = NewEntryParams;
    type Output = PathBuf;
    const ID: &'static str = "file.fs.newDirectory";
    const TITLE: &'static str = "Directory";
    const DESCRIPTION: &'static str = "Creates a folder (and the folders on the way) in a project folder; language servers \
         that ask to hear of new folders are told.";

    fn run(&self, session: &mut Session, params: NewEntryParams) -> Result<PathBuf, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let directory = project_folder(workbench, &params.directory)?;
        let path = new_entry_path(&directory, &params.name)?;
        let touched = [FileTouch::new_entry(&path, true)];
        let asked = language::file_operations::will(session, FileOperation::Create, &touched);
        if let Err(error) = zen_vfs::create_dir(&path) {
            language::file_operations::forget(session, &asked);
            return Err(CommandError::new(t!(
                "workbench-error-create",
                path = display(&path),
                error = error.source.to_string()
            )));
        }
        session.state_mut::<Workbench>().revision += 1;
        language::file_operations::did(session, FileOperation::Create, &touched);
        language::file_operations::remember(session, asked, &[]);
        Ok(path)
    }
}

pub struct CopyInto;

impl Command for CopyInto {
    type Params = CopyIntoParams;
    type Output = Vec<PathBuf>;
    const ID: &'static str = "file.fs.copyInto";
    const TITLE: &'static str = "Paste";
    const DESCRIPTION: &'static str =
        "Copies files and folders into a project folder; existing files are kept and copies get a free name.";

    fn run(&self, session: &mut Session, params: CopyIntoParams) -> Result<Vec<PathBuf>, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let directory = project_folder(workbench, &params.directory)?;
        let sources: Vec<PathBuf> = params.sources.iter().map(|source| workbench.resolve(source)).collect();
        if let Some(root) = workbench.project().map(Path::to_owned) {
            hidden_files::guard_copy(session, &root, &sources, &directory)?;
        }
        let workbench = session.state_mut::<Workbench>();
        let mut copied = Vec::new();
        let mut failure = None;
        for source in &sources {
            match zen_vfs::copy_into(source, &directory) {
                Ok(path) => copied.push(path),
                Err(error) => failure = Some(error),
            }
        }
        if !copied.is_empty() {
            workbench.revision += 1;
            workbench.messages.push(t!("workbench-copied", count = copied.len()));
        }
        match failure {
            Some(error) if copied.is_empty() => Err(CommandError::new(t!(
                "workbench-error-copy",
                path = display(&error.path),
                error = error.source.to_string()
            ))),
            Some(error) => {
                workbench.messages.push(t!(
                    "workbench-error-copy",
                    path = display(&error.path),
                    error = error.source.to_string()
                ));
                Ok(copied)
            }
            None => Ok(copied),
        }
    }
}

/// A file or folder and its new name.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// A file or folder inside the project (not the project folder itself): absolute, or relative
    /// to the project; a scratch file or buffer too, absolute.
    pub path: PathBuf,
    /// The new name: a plain name, without folders.
    pub name: String,
    /// Let the language servers update what refers to the file (imports, package and class names);
    /// true by default.
    #[serde(default = "updates_references")]
    pub update_references: bool,
}

fn updates_references() -> bool {
    true
}

pub struct RenameEntry;

impl Command for RenameEntry {
    type Params = RenameParams;
    type Output = PathBuf;
    const ID: &'static str = "file.fs.rename";
    const TITLE: &'static str = "Rename File";
    const DESCRIPTION: &'static str = "Renames a file or folder of the project (or a scratch file) in place; refuses a name that is taken. Open \
         tabs of the file (or of the folder's files) follow it. Language servers that ask to hear of renames update \
         what refers to it first (updateReferences: false skips that); one Undo in a changed file reverts their \
         changes and the rename. Returns the new path.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<PathBuf, CommandError> {
        let scratches = session.state::<scratch::Scratches>().cloned().unwrap_or_default();
        let workbench = session.state_mut::<Workbench>();
        let project = workbench.project.clone().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let path = project_path(workbench, &params.path);
        if !scratches.contains(&path) && (path == project || !inside_project(&project, &path)) {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = display(&path))));
        }
        hidden_files::guard_change(session, &project, &[path.clone(), path.with_file_name(params.name.trim())])?;
        let name = params.name.trim();
        let plain =
            matches!(Path::new(name).components().collect::<Vec<_>>().as_slice(), [std::path::Component::Normal(_)]);
        let touched = if plain && params.update_references && std::fs::symlink_metadata(&path).is_ok() {
            vec![FileTouch::existing(&path, Some(path.with_file_name(name)))]
        } else {
            Vec::new()
        };
        let asked = language::file_operations::will(session, FileOperation::Rename, &touched);
        let renamed = match zen_vfs::rename(&path, name) {
            Ok(renamed) => renamed,
            Err(error) => {
                language::file_operations::forget(session, &asked);
                return Err(if error.source.kind() == std::io::ErrorKind::InvalidInput {
                    CommandError::new(t!("workbench-error-bad-name", name = name))
                } else {
                    CommandError::new(t!(
                        "workbench-error-rename",
                        path = display(&error.path),
                        error = error.source.to_string()
                    ))
                });
            }
        };
        session.state_mut::<Workbench>().relocated(&path, &renamed);
        file_types::relocate_overrides(session, &path, Some(&renamed));
        session.state_mut::<history::History>().relocate(&path, &renamed);
        bookmarks::relocate(session, &path, &renamed);
        let told: Vec<FileTouch> =
            touched.into_iter().map(|file| FileTouch { new_path: Some(renamed.clone()), ..file }).collect();
        language::file_operations::did(session, FileOperation::Rename, &told);
        language::file_operations::remember(session, asked, &[(path, renamed.clone())]);
        Ok(renamed)
    }
}

/// A path parameter.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct PathParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
}

pub(crate) fn display(path: &Path) -> String {
    path.display().to_string()
}

pub struct OpenProject;

impl Command for OpenProject {
    type Params = PathParams;
    type Output = ();
    const ID: &'static str = "file.project.open";
    const TITLE: &'static str = "Open…";
    const DESCRIPTION: &'static str = "Opens a folder as the project. Switching from another project saves its consoles, keeps its workspaces' tabs for its workspace.json and drops them, then gives back, collapsed and without connecting, the workspace tabs this window remembers for the new project.";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<(), CommandError> {
        if !params.path.is_dir() {
            return Err(CommandError::new(t!("workbench-error-not-a-folder", path = display(&params.path))));
        }
        let leaving = session.state::<Workbench>().and_then(|workbench| workbench.project.clone());
        if let Some(leaving) = &leaving {
            workspaces::leave(session, leaving);
        }
        let workbench = session.state_mut::<Workbench>();
        *workbench = Workbench { project: Some(params.path.clone()), ..Workbench::default() };
        session.set_state(zen_project::trust::OpenRoot(params.path.clone()));
        session.set_state(history::History::default());
        session.set_state(bookmarks::Bookmarks::default());
        project_settings::project_opened(session, &params.path);
        trust::project_opened(session, &params.path);
        jdk::commands::offer_jdk(session, &params.path);
        index_project(session);
        if leaving.is_some() {
            workspaces::arrive(session, &params.path);
        }
        Ok(())
    }
}

pub(crate) fn index_project(session: &mut Session) {
    let settings = session.state::<project_settings::ProjectSettingsState>().map(|state| state.settings());
    let exclusions = settings.map_or_else(zen_index::Exclusions::default, |settings| {
        zen_index::Exclusions::new(&settings.excluded, &settings.exclude_patterns)
    });
    let workbench = session.state_mut::<Workbench>();
    let Some(root) = workbench.project.clone() else { return };
    let current = workbench.index.as_ref().filter(|index| index.root() == root && *index.exclusions() == exclusions);
    if current.is_some() {
        return;
    }
    workbench.index = None;
    let task = session.state_mut::<tasks::Tasks>().start(&t!("workbench-index-title"), exit::INDEX, false);
    let reporter = task.clone();
    let progress: zen_index::Progress = Arc::new(move |step: zen_index::IndexProgress| {
        if step.done {
            reporter.finish(Ok(t!("workbench-index-done", files = step.indexed)));
        } else {
            let fraction = step.indexed as f32 / step.total.max(1) as f32;
            reporter.progress(Some(fraction), &t!("workbench-index-progress", done = step.indexed, total = step.total));
        }
    });
    let index = zen_index::FileIndex::with_progress(&root, &exclusions.folders, &exclusions.patterns, Some(progress));
    let pause = index.build_pause();
    task.on_pause(move |paused| pause.set(paused));
    session.state_mut::<Workbench>().index = Some(index);
}

/// The parameters of `file.document.open`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
    /// Only into the editor: a file that opens in its associated application is refused instead of
    /// starting that application (reopening the tabs of a project does so).
    #[serde(default)]
    pub editor_only: bool,
    /// The editor group to open it in (`editor.split.list`), which takes the focus; a workspace's group (under `workspaces`) is used only when named here. When absent, a tab of the file already shown somewhere is brought to front there, else a new tab goes to the main group that last had the keyboard.
    #[serde(default)]
    pub group: Option<groups::GroupId>,
    /// Open it in the group's preview tab, reusing that tab, as a single click in the Project tool window does; only while Enable preview tab is on, otherwise an ordinary tab.
    #[serde(default)]
    pub preview: bool,
}

pub struct OpenDocument;

impl Command for OpenDocument {
    type Params = OpenParams;
    type Output = ();
    const ID: &'static str = "file.document.open";
    const TITLE: &'static str = "Open File";
    const DESCRIPTION: &'static str = "Opens a file in the editor, or switches to its tab; a file whose type is \
         \"Files opened in associated applications\" opens in the system's application for it unless `editorOnly`. \
         With `group`, it opens in that editor group, which may be a workspace's. With `preview`, it shows in the \
         group's reused preview tab while Enable preview tab is on; without it, a file in a preview tab is kept open.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<(), CommandError> {
        let wanted = session
            .state::<Workbench>()
            .map_or_else(|| params.path.clone(), |workbench| workbench.resolve(&params.path));
        trust::guard_agent_data_folder(session, &[wanted])?;
        let origin = history::caret(session);
        if params.preview {
            tabs::open_preview(session, &params.path, params.group)?;
        } else if params.editor_only {
            open_bringing(session, &params.path, false, params.group)?;
        } else {
            open_file_in(session, &params.path, true, params.group)?;
            let workbench = session.state_mut::<Workbench>();
            let spelled = workbench.resolve(&params.path);
            workbench.groups.keep_everywhere(&spelled);
        }
        history::left_file(session, origin);
        Ok(())
    }
}

pub(crate) fn open_document(session: &mut Session, path: &Path) -> Result<(), CommandError> {
    open_file(session, path, false)
}

pub(crate) fn open_file(session: &mut Session, path: &Path, launch: bool) -> Result<(), CommandError> {
    open_file_in(session, path, launch, None)
}

pub(crate) fn open_file_in(
    session: &mut Session,
    path: &Path,
    launch: bool,
    group: Option<groups::GroupId>,
) -> Result<(), CommandError> {
    open_bringing(session, path, launch, group)?;
    tabs::keep_to_the_limit(session);
    Ok(())
}

pub(crate) fn open_bringing(
    session: &mut Session,
    path: &Path,
    launch: bool,
    group: Option<groups::GroupId>,
) -> Result<(), CommandError> {
    tabs::follow_policy(session);
    let in_main = session.settings().get_or(groups::OPEN_IN_MAIN_WINDOW, true);
    let workbench = session.state_mut::<Workbench>();
    if let Some(group) = group.filter(|group| workbench.groups.group(*group).is_none()) {
        return Err(CommandError::new(t!("workbench-error-no-group", group = group)));
    }
    let focused = workbench.groups.focused();
    let group = group.or((!in_main && workbench.groups.window(focused).is_some()).then_some(focused));
    let bring = |groups: &mut groups::EditorGroups, path: &Path| match group {
        Some(group) => {
            groups.open_in(path, group);
        }
        None => groups.open(path),
    };
    let spelled = workbench.resolve(path);
    let spelled = jvm_libraries::opened_as(&spelled, &virtual_documents::folder()).unwrap_or(spelled);
    let path = spelled.as_path();
    if workbench.documents.iter().any(|document| document.path == path) {
        bring(&mut workbench.groups, path);
        workbench.sync_groups();
        workbench.shown += 1;

        workbench.touch_recent();
        return Ok(());
    }
    let charset = project_settings::charset_for(session, path);
    let editor_charset = editor_config::charset(session, path);
    let opening = match file_types::opening(path) {
        Err(error) if editor_charset.is_some() && error.code.as_deref() == Some(file_types::UNKNOWN_FILE_TYPE) => {
            file_types::Opening::Text { typed: false }
        }
        opening => opening?,
    };
    let opened = match opening {
        file_types::Opening::Application if launch => return file_types::open_in_application(session, path),
        file_types::Opening::Application => {
            let file = path.file_name().map_or_else(|| display(path), |name| name.to_string_lossy().into_owned());
            return Err(CommandError::new(t!("workbench-error-opens-in-application", file = file.as_str())));
        }
        file_types::Opening::Image => Document::open(path),
        file_types::Opening::Text { typed } => open_text(path, charset.as_deref(), editor_charset, typed),
    };
    let mut document = opened.map_err(|error| {
        CommandError::new(t!("workbench-error-open", path = display(path), error = error.to_string()))
    })?;
    folding::fold_on_open(session, &mut document);
    line_separators::settle(session, &mut document);
    let workbench = session.state_mut::<Workbench>();
    workbench.messages.extend(document.read_only().map(str::to_owned));
    workbench.documents.push(document);
    bring(&mut workbench.groups, path);
    workbench.sync_groups();
    workbench.shown += 1;

    workbench.touch_recent();
    Ok(())
}

fn open_text(
    path: &Path,
    charset: Option<&str>,
    editor_charset: Option<charsets::Charset>,
    typed: bool,
) -> Result<Document, String> {
    let name = path.file_name().map_or_else(|| display(path), |name| name.to_string_lossy().into_owned());
    let reason = |kind: &str, charset: &str| {
        t!("workbench-error-encoding", file = name.as_str(), charset = charset, kind = kind)
    };
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let decoded = |charset: charsets::Charset| {
        let (mut document, bom) = match charset.decode(&bytes) {
            Some((text, bom)) => (Document::with_text(path, text), bom),
            None => (Document::read_only_text(path, &bytes, reason("invalid", charset.label())), false),
        };
        document.set_encoding(charset, bom);
        document
    };
    if let Some(found) = charsets::Charset::of_bom(&bytes) {
        return Ok(decoded(found));
    }
    if let Some(editor_charset) = editor_charset.filter(|charset| *charset != charsets::Charset::Utf8) {
        return Ok(decoded(editor_charset));
    }
    let charset = if editor_charset.is_some() { None } else { charset };
    let foreign = charset.filter(|charset| charsets::Charset::named(charset) != Some(charsets::Charset::Utf8));
    let head = &bytes[..bytes.len().min(file_types::HEAD)];
    if let Some(foreign) = foreign {
        return Ok(match charsets::Charset::named(foreign) {
            Some(_) if typed && file_types::is_binary(head) => {
                Document::read_only_text(path, &bytes, t!("workbench-binary-read-only", file = name.as_str()))
            }
            Some(named) => decoded(named),
            None => {
                let mut document = Document::read_only_text(path, &bytes, reason("charset", foreign));
                document.charset = Some(foreign.to_owned());
                document
            }
        });
    }
    let mut document = if let Some(order) = file_types::utf16_of(head) {
        decoded(match order {
            file_types::Utf16::Little => charsets::Charset::Utf16Le,
            file_types::Utf16::Big => charsets::Charset::Utf16Be,
        })
    } else {
        match String::from_utf8(bytes) {
            Ok(contents) => Document::with_text(path, contents),
            Err(error)
                if typed
                    && file_types::is_binary(&error.as_bytes()[..error.as_bytes().len().min(file_types::HEAD)]) =>
            {
                Document::read_only_text(path, error.as_bytes(), t!("workbench-binary-read-only", file = name.as_str()))
            }
            Err(error) if error.as_bytes().contains(&0) => {
                return zen_vfs::read_text(path).map(|text| Document::with_text(path, text)).map_err(|e| e.to_string());
            }
            Err(error) => {
                Document::read_only_text(path, error.as_bytes(), reason("invalid", charset.unwrap_or("UTF-8")))
            }
        }
    };
    if document.encoding.is_none() {
        document.charset = charset.map(str::to_owned);
    }
    Ok(document)
}

pub struct SaveDocument;

impl Command for SaveDocument {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "file.document.save";
    const TITLE: &'static str = "Save";
    const DESCRIPTION: &'static str = "Saves the active file, first optimizing its imports and reformatting it when editor.onSave.* say so, then \
         removing trailing spaces and blank lines and adding a final line break as the On Save options say \
         (editor.onSave.options), as one undo step. A file changed on disk since the IDE read it is never written \
         over: the save fails, the changes stay unsaved and the file is a conflict, settled with \
         file.document.reloadFromDisk or file.document.keepMine. An unmodified file that changed is read again.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| workbench.active_document().is_some())
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        let active = session.state::<Workbench>().and_then(Workbench::active_document);
        if let Some(path) = active.map(|document| document.path().to_owned()) {
            trust::guard_agent_data_folder(session, &[path])?;
        }
        let workbench = session.state_mut::<Workbench>();
        if let Some(document) = workbench.active_document_mut().filter(|document| document.dirty)
            && let Err(error) = document.check_disk()
        {
            let refused = closing::save_failed(&document.name(), &error);
            workbench.revision += 1;
            return Err(CommandError::new(refused));
        }
        let active =
            session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path.clone());
        if let Some(path) = active {
            let _ = language::format::before_save(session, &path);
            editor_config::before_write(session, &path);
        }
        let workbench = session.state_mut::<Workbench>();
        let document =
            workbench.active_document_mut().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let name = document.name();
        let result = document.save().map_err(|error| CommandError::new(closing::save_failed(&name, &error)));
        local_history::record_saves(session);
        result
    }
}

pub struct SaveAll;

impl Command for SaveAll {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "file.document.saveAll";
    const TITLE: &'static str = "Save All";
    const DESCRIPTION: &'static str = "Saves every modified file, first optimizing imports and reformatting when editor.onSave.* say so, then \
             applying the On Save options (editor.onSave.options) to each, as one undo step per file. A file that cannot be saved, \
             or that changed on disk since the IDE read it (a conflict, settled with file.document.reloadFromDisk or \
             file.document.keepMine), keeps its changes and the error names it; the others are saved.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        let dirty: Vec<PathBuf> = session
            .state::<Workbench>()
            .map(|workbench| {
                workbench
                    .documents
                    .iter()
                    .filter(|document| document.dirty)
                    .map(|document| document.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        trust::guard_agent_data_folder(session, &dirty)?;
        let (_, failed) = closing::save_documents(session, &dirty);
        closing::failure(failed).map_or(Ok(()), Err)
    }
}

pub struct CloseDocument;

/// Which tab to close.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct CloseParams {
    /// The tab index in its group; the group's active tab when absent.
    pub index: Option<usize>,
    /// The editor group (`editor.split.list`); the focused one when absent.
    pub group: Option<groups::GroupId>,
}

impl Command for CloseDocument {
    type Params = CloseParams;
    type Output = closing::ClosedTabs;
    const ID: &'static str = "file.document.close";
    const TITLE: &'static str = "Close Tab";
    const DESCRIPTION: &'static str = "Closes an editor tab. Unsaved changes are saved first, as Save All saves them; a \
         file that cannot be saved keeps its tab and its changes, and the error says why. Closing an empty scratch file deletes it. \
         Without `index` and `group` while a workspace is expanded, it closes the workspace's active inner tab (⌘W), never a \
         hidden code tab. Returns the file closed and whether it was saved, or the inner tab closed.";

    fn run(&self, session: &mut Session, params: CloseParams) -> Result<closing::ClosedTabs, CommandError> {
        if params.index.is_none() && params.group.is_none() {
            workspaces::sync(session);
            if let Some((info, tab)) = workspaces::tab_in_front(session) {
                workspaces::close_inner(session, &info.id, &tab.id)?;
                return Ok(closing::ClosedTabs { tab: Some(tab.id), ..closing::ClosedTabs::default() });
            }
        }
        closing::close_tab(session, params.group, params.index)
    }
}

pub struct SelectTab;

/// Which tab to make active.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct TabParams {
    /// The tab index in its group, from 0.
    pub index: usize,
    /// The editor group (`editor.split.list`), which takes the focus; the focused one when absent.
    pub group: Option<groups::GroupId>,
}

impl Command for SelectTab {
    type Params = TabParams;
    type Output = ();
    const ID: &'static str = "editor.tab.select";
    const TITLE: &'static str = "Select Tab";
    const DESCRIPTION: &'static str = "Makes an editor tab active.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<(), CommandError> {
        let origin = history::caret(session);
        let workbench = session.state_mut::<Workbench>();
        let group = params.group.unwrap_or(workbench.groups.focused());
        if workbench.groups.select(group, params.index) {
            workbench.sync_groups();
            workbench.shown += 1;

            workbench.touch_recent();
        }
        history::left_file(session, origin);
        Ok(())
    }
}

pub struct Undo;

impl Command for Undo {
    type Params = NoParams;
    type Output = bool;
    const ID: &'static str = "editor.edit.undo";
    const TITLE: &'static str = "Undo";
    const DESCRIPTION: &'static str = "Undoes the last change in the active file; when that change was a refactoring, undoes it in every file it changed; when a word was saved to a dictionary from the file since its last change, takes the word out again.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<bool, CommandError> {
        if let Some(undone) = spelling::undo(session) {
            return Ok(undone);
        }
        if let Some(undone) = language::refactor::undo(session) {
            return Ok(undone);
        }
        Ok(session.state_mut::<Workbench>().active_document_mut().is_some_and(Document::undo))
    }
}

pub struct Redo;

impl Command for Redo {
    type Params = NoParams;
    type Output = bool;
    const ID: &'static str = "editor.edit.redo";
    const TITLE: &'static str = "Redo";
    const DESCRIPTION: &'static str = "Redoes the last undone change in the active file.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<bool, CommandError> {
        Ok(session.state_mut::<Workbench>().active_document_mut().is_some_and(Document::redo))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_document_tells_the_byte_edits_since_a_version() {
        let mut document = Document::with_text(Path::new("/a.go"), "ab\nё\n".to_owned());
        let first = document.version();
        assert_eq!(document.changes_since(first), Some(Vec::new()));
        document.replace_ranges(vec![(3..4, "xy".to_owned())]);
        let second = document.version();
        document.replace_ranges(vec![(0..0, "/".to_owned()), (5..5, "z".to_owned())]);
        assert_eq!(document.text(), "/ab\nxyz\n");
        let changes = document.changes_since(first).unwrap();
        assert_eq!(changes[0], zen_syntax::TextChange { start: 3, old_end: 5, new_end: 5 }, "ё is two bytes");
        assert_eq!(changes.len(), 3);
        assert_eq!(document.changes_since(second).unwrap().len(), 2);
        assert_eq!(document.changes_since(12_345_678), None, "not a version of this document");
        for _ in 0..CHANGES_KEPT {
            document.replace_ranges(vec![(0..0, "-".to_owned())]);
        }
        assert_eq!(document.changes_since(first), None, "too old");
    }

    #[test]
    fn a_document_tells_the_character_edits_since_a_version_and_when_its_folds_change() {
        let mut document = Document::with_text(Path::new("/a.go"), "ab\nё {\n}\n".to_owned());
        let first = document.version();
        let folds = document.folds_version();
        document.replace_ranges(vec![(3..4, "xy".to_owned())]);
        assert_eq!(document.folds_version(), folds, "nothing folded or unfolded");
        let edits = document.char_edits_since(first).unwrap();
        assert_eq!(edits, vec![CharEdit { range: 3..4, inserted: 2 }], "in characters: ё is one");
        assert_eq!((edits[0].map(2), edits[0].map(4), edits[0].map(6)), (2, 5, 7));
        document.set_folded(6..8, true);
        assert_ne!(document.folds_version(), folds, "folding is seen");
        let folded = document.folds_version();
        document.replace_ranges(vec![(0..0, "/".to_owned())]);
        assert_eq!(document.folds(), &[std::ops::Range { start: 7, end: 9 }]);
        assert_eq!(document.folds_version(), folded, "a fold moved by an edit is the same fold");
        document.replace_ranges(vec![(6..10, String::new())]);
        assert!(document.folds().is_empty());
        assert_ne!(document.folds_version(), folded, "an edit that removed the fold unfolded it");
        assert_eq!(document.char_edits_since(first).map(|edits| edits.len()), Some(3));
        assert_eq!(document.char_edits_since(12_345_678), None);
    }

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("lib.rs"), "").unwrap();
        (registry, Session::new(), dir)
    }

    struct LegacyEncodings;

    impl zen_project::settings::SettingsProvider for LegacyEncodings {
        fn id(&self) -> &str {
            "legacy"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(
            &self,
            root: &Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let file = |folder: &str, charset: &str| zen_project::settings::FileEncoding {
                path: root.join(folder),
                charset: charset.to_owned(),
            };
            let encodings = zen_project::settings::Encodings {
                default: Some("UTF-8".to_owned()),
                files: vec![file("legacy", "windows-1251"), file("wide", "UTF-32")],
                bom_for_new_files: None,
            };
            Some(zen_project::settings::ProjectSettings { encodings, ..Default::default() })
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_reached_through_a_link_is_the_project_file() {
        let (registry, mut session, dir) = setup();
        let real = dir.path().canonicalize().unwrap();
        std::fs::write(real.join("main.go"), "package main\n").unwrap();
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("project");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": real })).unwrap();

        registry.dispatch(&mut session, "file.document.open", json!({ "path": link.join("main.go") })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": real.join("main.go") })).unwrap();
        let workbench = session.state::<Workbench>().unwrap();
        let paths: Vec<_> = workbench.documents().iter().map(Document::path).collect();
        assert_eq!(paths, [real.join("main.go").as_path()], "one tab, under the project's spelling");
        assert_eq!(workbench.spelled(&link.join("pkg/new.go")), real.join("pkg/new.go"));
        assert_eq!(workbench.spelled(Path::new("/elsewhere/x.go")), Path::new("/elsewhere/x.go"));
    }

    #[test]
    fn a_legacy_encoding_opens_editable_and_saves_in_it() {
        zen_i18n::register(LOCALES);
        let (registry, mut session, dir) = setup();
        let cp1251: &[u8] = &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2, b'\n'];
        std::fs::create_dir_all(dir.path().join("legacy")).unwrap();
        let path = dir.path().join("legacy/greeting.txt");
        std::fs::write(&path, cp1251).unwrap();
        session.state_mut::<project_settings::ProjectSettingsState>().set_providers(vec![Arc::new(LegacyEncodings)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert_eq!(document.text(), "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}\n");
        assert_eq!(document.charset(), "windows-1251");
        assert_eq!(document.read_only(), None);
        assert!(document.replace_ranges(vec![(0..0, "\u{44f}".to_owned())]));
        registry.dispatch(&mut session, "file.document.save", json!(null)).unwrap();
        let mut written = vec![0xff];
        written.extend_from_slice(cp1251);
        assert_eq!(std::fs::read(&path).unwrap(), written);
    }

    #[test]
    fn a_file_in_an_encoding_the_ide_cannot_write_opens_read_only_and_is_never_saved() {
        zen_i18n::register(LOCALES);
        let (registry, mut session, dir) = setup();
        let cp1251: &[u8] = &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2, b'\n'];
        std::fs::create_dir_all(dir.path().join("wide")).unwrap();
        std::fs::write(dir.path().join("wide/greeting.txt"), cp1251).unwrap();
        std::fs::write(dir.path().join("wide/plain.txt"), "ascii\n").unwrap();
        std::fs::write(dir.path().join("broken.txt"), [b'a', 0xff, b'\n']).unwrap();
        session.state_mut::<project_settings::ProjectSettingsState>().set_providers(vec![Arc::new(LegacyEncodings)]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();

        for (name, charset) in [("wide/greeting.txt", "UTF-32"), ("wide/plain.txt", "UTF-32"), ("broken.txt", "UTF-8")]
        {
            let path = dir.path().join(name);
            let before = std::fs::read(&path).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            let workbench = session.state_mut::<Workbench>();
            let messages = workbench.take_messages();
            let document = workbench.active_document_mut().unwrap();
            let reason = document.read_only().unwrap_or_else(|| panic!("{name} is read-only")).to_owned();
            let file = Path::new(name).file_name().unwrap().to_string_lossy().into_owned();
            assert!(reason.contains(charset) && reason.contains(&file), "{reason}");
            assert_eq!(document.charset(), charset);
            assert_eq!(messages, vec![reason.clone()]);
            assert!(!document.apply_op(&zen_editor::EditOp::Insert { text: "x".to_owned() }));
            assert!(!document.replace_ranges(vec![(0..0, "x".to_owned())]));
            assert!(!document.apply_action(vec![Edit::replace(0..0, "x")], zen_editor::Selections::default()));
            assert!(!document.is_dirty());
            let refused = registry.dispatch(&mut session, "file.document.save", json!(null)).unwrap_err().to_string();
            assert!(refused.contains(&reason), "{refused}");
            registry.dispatch(&mut session, "file.document.saveAll", json!(null)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before, "{name} keeps its bytes");
        }

        let index = session.state::<Workbench>().unwrap().index.clone().unwrap();
        while !index.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let replace = json!({ "query": { "text": "ascii", "matchCase": true }, "replacement": "x" });
        let refused = registry.dispatch(&mut session, "search.text.replaceAll", replace).unwrap_err().to_string();
        assert!(refused.contains("plain.txt"), "{refused}");
        assert_eq!(std::fs::read_to_string(dir.path().join("wide/plain.txt")).unwrap(), "ascii\n");

        let binary = dir.path().join("logo.bin");
        std::fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 0xff, 0]).unwrap();
        let refused = registry.dispatch(&mut session, "file.document.open", json!({ "path": binary })).unwrap_err();
        assert!(refused.to_string().contains("logo.bin"), "a binary file is not opened as text: {refused}");
        let workbench = session.state_mut::<Workbench>();
        assert!(workbench.documents().iter().all(|document| document.path() != binary));
        assert!(workbench.take_messages().is_empty());
        std::fs::write(dir.path().join("wide/wide.txt"), [b'a', 0, 0xff, 0]).unwrap();
        let wide = json!({ "path": dir.path().join("wide/wide.txt") });
        registry.dispatch(&mut session, "file.document.open", wide).unwrap();
        let workbench = session.state_mut::<Workbench>();
        assert!(
            workbench.active_document().unwrap().read_only().is_some(),
            "a file configured as text opens read-only"
        );
        workbench.take_messages();

        let main = dir.path().join("main.rs");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        let workbench = session.state_mut::<Workbench>();
        assert!(workbench.take_messages().is_empty());
        let document = workbench.active_document_mut().unwrap();
        assert_eq!(document.read_only(), None, "UTF-8 files behave as before");
        assert_eq!(document.charset(), "UTF-8");
        assert!(document.replace_ranges(vec![(0..0, "// a\n".to_owned())]));
        registry.dispatch(&mut session, "file.document.save", json!(null)).unwrap();
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "// a\nfn main() {}\n");
    }

    #[test]
    fn open_edit_undo_save_close() {
        let (registry, mut session, dir) = setup();
        let main = dir.path().join("main.rs");
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();

        let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
        for c in "// hi\n".chars() {
            document.apply_op(&zen_editor::EditOp::Insert { text: c.to_string() });
        }
        assert!(document.is_dirty());
        assert_eq!(document.text(), "// hi\nfn main() {}\n");
        registry.dispatch(&mut session, "editor.edit.undo", json!(null)).unwrap();
        assert_eq!(session.state::<Workbench>().unwrap().active_document().unwrap().text(), "// hifn main() {}\n");
        registry.dispatch(&mut session, "editor.edit.undo", json!(null)).unwrap();
        assert_eq!(
            session.state::<Workbench>().unwrap().active_document().unwrap().text(),
            "fn main() {}\n",
            "typing is one step"
        );
        assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty(), "back at the saved text");
        registry.dispatch(&mut session, "editor.edit.redo", json!(null)).unwrap();
        registry.dispatch(&mut session, "editor.edit.redo", json!(null)).unwrap();

        registry.dispatch(&mut session, "file.document.save", json!(null)).unwrap();
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "// hi\nfn main() {}\n");
        assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());

        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        assert!(session.state::<Workbench>().unwrap().active_document().is_none());
    }

    #[test]
    fn creates_and_pastes_files_inside_the_project_only() {
        let (registry, mut session, dir) = setup();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        let revision = session.state::<Workbench>().unwrap().revision();

        let created = registry
            .dispatch(&mut session, "file.fs.newFile", json!({ "directory": project, "name": "pkg/util.go" }))
            .unwrap();
        assert_eq!(created, json!(project.join("pkg/util.go")));
        assert_eq!(session.state::<Workbench>().unwrap().active_document().unwrap().name(), "util.go");
        assert!(
            registry
                .dispatch(&mut session, "file.fs.newFile", json!({ "directory": project, "name": "pkg/util.go" }))
                .is_err()
        );
        for bad in ["", "../escape.go", "/etc/x"] {
            assert!(
                registry
                    .dispatch(&mut session, "file.fs.newFile", json!({ "directory": project, "name": bad }))
                    .is_err(),
                "{bad}"
            );
        }
        registry
            .dispatch(&mut session, "file.fs.newDirectory", json!({ "directory": project, "name": "docs" }))
            .unwrap();
        assert!(project.join("docs").is_dir());

        let pasted = registry
            .dispatch(&mut session, "file.fs.copyInto", json!({ "sources": [dir.path().join("main.rs"), dir.path().join("main.rs")], "directory": project.join("docs") }))
            .unwrap();
        assert_eq!(pasted, json!([project.join("docs/main.rs"), project.join("docs/main copy.rs")]));
        assert!(
            registry
                .dispatch(
                    &mut session,
                    "file.fs.copyInto",
                    json!({ "sources": [dir.path().join("lib.rs")], "directory": dir.path() })
                )
                .is_err(),
            "outside the project"
        );
        assert!(session.state::<Workbench>().unwrap().revision() > revision);
    }

    #[test]
    fn a_file_closed_and_opened_again_never_repeats_a_version() {
        let (registry, mut session, dir) = setup();
        let main = dir.path().join("main.rs");
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let version = |session: &Session| session.state::<Workbench>().unwrap().active_document().unwrap().version();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        let first = version(&session);
        registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        assert_ne!(version(&session), first, "a view's cache of the old document would be taken as current");
    }

    #[test]
    fn opening_an_open_file_switches_to_its_tab() {
        let (registry, mut session, dir) = setup();
        for name in ["main.rs", "lib.rs", "main.rs"] {
            registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(name) })).unwrap();
        }
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(workbench.documents().len(), 2);
        assert_eq!(workbench.active(), Some(0));
    }

    #[test]
    fn closing_tabs_keeps_a_sensible_active_tab() {
        let (registry, mut session, dir) = setup();
        for name in ["main.rs", "lib.rs"] {
            registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(name) })).unwrap();
        }
        registry.dispatch(&mut session, "file.document.close", json!({ "index": 0 })).unwrap();
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(workbench.active_document().unwrap().name(), "lib.rs");
    }

    #[test]
    fn unicode_edits_keep_the_mirror_in_step() {
        let mut text = String::from("привет");
        apply_to_string(&mut text, &Edit::replace(1..3, "ЁЁ"));
        assert_eq!(text, "пЁЁвет");
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    const RED_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x28, 0x00, 0x00, 0x00, 0x14, 0x08, 0x02, 0x00, 0x00, 0x00, 0x70, 0x24, 0xe8, 0xec, 0x00, 0x00, 0x00,
        0x23, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xcd, 0xb1, 0x0d, 0x00, 0x00, 0x08, 0xc0, 0xa0, 0xfe, 0xff,
        0xb4, 0x9e, 0xe1, 0x20, 0x09, 0x3b, 0x4d, 0x9d, 0x48, 0x2c, 0x16, 0x8b, 0xc5, 0x62, 0xb1, 0xf8, 0x4f, 0xbc,
        0x7d, 0xfa, 0x1d, 0x0e, 0xa2, 0x1e, 0x87, 0x3d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
        0x60, 0x82,
    ];

    fn session_with_image() -> (tempfile::TempDir, Registry, Session) {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("logo.png");
        std::fs::write(&png, RED_PNG).unwrap();
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": png })).unwrap();
        for _ in 0..200 {
            let image = session.state::<Workbench>().unwrap().active_document().unwrap().image().unwrap().clone();
            if !image.is_loading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        (dir, registry, session)
    }

    #[test]
    fn opens_an_image_and_zooms_it() {
        let (_dir, registry, mut session) = session_with_image();
        let info = registry.dispatch(&mut session, "editor.image.info", serde_json::json!({})).unwrap();
        assert_eq!(info["format"], "PNG", "{info}");
        assert_eq!((info["width"].as_f64(), info["height"].as_f64()), (Some(40.0), Some(20.0)));
        assert_eq!(info["scale"].as_f64(), Some(1.0));
        let first = session.state::<Workbench>().unwrap().active_document().unwrap().version();
        registry.dispatch(&mut session, "file.document.close", serde_json::json!({})).unwrap();
        let path = info["path"].as_str().unwrap().to_owned();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": path })).unwrap();
        assert_ne!(session.state::<Workbench>().unwrap().active_document().unwrap().version(), first);

        let zoom = |session: &mut Session, params| registry.dispatch(session, "editor.image.zoom", params).unwrap();
        assert_eq!(zoom(&mut session, serde_json::json!({ "action": "in" })).as_f64(), Some(1.25));
        assert_eq!(zoom(&mut session, serde_json::json!({ "action": "actual" })).as_f64(), Some(1.0));
        assert_eq!(zoom(&mut session, serde_json::json!({ "scale": 1000.0 })).as_f64(), Some(32.0));

        let workbench = session.state_mut::<Workbench>();
        workbench.active_document_mut().unwrap().image_mut().unwrap().set_fit_scale(3.0);
        assert_eq!(zoom(&mut session, serde_json::json!({ "action": "fit" })).as_f64(), Some(3.0));
        assert!(registry.dispatch(&mut session, "editor.image.zoom", serde_json::json!({})).is_err());

        let document = session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert!(!document.apply_op(&zen_editor::EditOp::Insert { text: "x".to_owned() }));
        assert_eq!(document.text(), "");
        assert!(!document.is_dirty());
        registry.dispatch(&mut session, "file.document.save", serde_json::json!({})).unwrap();
    }

    #[test]
    fn opens_at_its_own_size_or_smaller() {
        let (_dir, _registry, mut session) = session_with_image();
        let image = session.state_mut::<Workbench>().active_document_mut().unwrap().image_mut().unwrap();
        image.set_fit_scale(0.5);
        assert_eq!((image.zoom(), image.scale()), (ImageZoom::Auto, 0.5));
        image.set_fit_scale(4.0);
        assert_eq!(image.scale(), 1.0);
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    #[test]
    fn refuses_paths_outside_the_project_and_the_project_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        let delete = |session: &mut Session, path: &Path| {
            registry.dispatch(session, "file.fs.delete", serde_json::json!({ "paths": [path] }))
        };
        assert!(delete(&mut session, dir.path()).is_err());
        assert!(delete(&mut session, Path::new("/tmp/elsewhere")).is_err());
        let escape = dir.path().join("..").join("elsewhere");
        assert!(delete(&mut session, &escape).is_err());
        let create = registry.dispatch(
            &mut session,
            "file.fs.newFile",
            serde_json::json!({ "directory": dir.path().join(".."), "name": "x.txt" }),
        );
        assert!(create.is_err());
        assert!(inside_project(dir.path(), &dir.path().join("a/b")));
        assert!(!inside_project(dir.path(), &dir.path().join("a/../../b")));
    }

    #[test]
    fn every_keymap_command_is_registered() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        database::register(&mut registry).unwrap();
        for binding in zen_core::keymap::Keymap::default_macos().bindings() {
            if binding.command.starts_with("view.") {
                continue;
            }
            assert!(
                registry.commands().any(|info| info.id.as_str() == binding.command),
                "{} runs {}, which is not registered",
                binding.action,
                binding.command
            );
        }
    }
}
