use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{ImageZoom, Workbench};

pub const FILE: &str = "previews.json";

pub const MAX_FILES: usize = 500;

pub const MAX_TEXT: usize = 8 * 1024 * 1024;

/// What a preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    /// An SVG drawn as a picture, with the image viewer's renderer.
    Svg,
    /// Markdown, read as GitHub Flavored Markdown and drawn natively (ADR-0138).
    Markdown,
}

impl PreviewKind {
    pub fn id(self) -> &'static str {
        match self {
            PreviewKind::Svg => "svg",
            PreviewKind::Markdown => "markdown",
        }
    }

    pub fn default_mode(self) -> PreviewMode {
        match self {
            PreviewKind::Svg | PreviewKind::Markdown => PreviewMode::EditorAndPreview,
        }
    }

    pub fn of(path: &Path) -> Option<PreviewKind> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "svg" => Some(PreviewKind::Svg),
            "md" | "markdown" => Some(PreviewKind::Markdown),
            _ => None,
        }
    }

    fn render(self, text: &str) -> Result<Content, String> {
        if text.len() > MAX_TEXT {
            return Err(t!("workbench-preview-too-large", size = MAX_TEXT / (1024 * 1024)));
        }
        match self {
            PreviewKind::Svg => zen_image::decode_svg(text.as_bytes())
                .map(|picture| Content::Picture(Arc::new(picture)))
                .map_err(|error| error.to_string()),
            PreviewKind::Markdown => Ok(Content::Markdown(Arc::new(crate::markdown::render(text)))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Picture(Arc<zen_image::Picture>),
    Markdown(Arc<crate::markdown::Rendered>),
}

impl Content {
    pub fn picture(&self) -> Option<&Arc<zen_image::Picture>> {
        match self {
            Content::Picture(picture) => Some(picture),
            Content::Markdown(_) => None,
        }
    }

    pub fn markdown(&self) -> Option<&Arc<crate::markdown::Rendered>> {
        match self {
            Content::Markdown(rendered) => Some(rendered),
            Content::Picture(_) => None,
        }
    }
}

/// How a file with a preview is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PreviewMode {
    /// Only the text editor.
    EditorOnly,
    /// The text editor and the preview side by side.
    EditorAndPreview,
    /// Only the preview.
    PreviewOnly,
}

impl PreviewMode {
    pub fn shows_preview(self) -> bool {
        self != PreviewMode::EditorOnly
    }
}

/// How the text editor and the preview share a file's editor while both are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SplitOrientation {
    /// Side by side, with a vertical divider between them.
    #[default]
    Vertical,
    /// One above the other, with a horizontal divider between them.
    Horizontal,
}

impl SplitOrientation {
    pub fn flipped(self) -> Self {
        match self {
            SplitOrientation::Vertical => SplitOrientation::Horizontal,
            SplitOrientation::Horizontal => SplitOrientation::Vertical,
        }
    }
}

pub const DEFAULT_MARKDOWN_FONT_SIZE: f32 = 14.5;

pub const MARKDOWN_FONT_SIZES: std::ops::RangeInclusive<f32> = 8.0..=32.0;

/// Settings → Languages & Frameworks → Markdown.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MarkdownSettings {
    /// Preview layout: the mode of Markdown files that were never switched (editorAndPreview by default).
    pub preview_layout: PreviewMode,
    /// Sync scroll: the editor and the preview keep the same source line in view (on by default).
    pub sync_scroll: bool,
    /// Preview font size in points: of the body text, the rest of the preview in proportion (14.5 by default).
    pub preview_font_size: f32,
}

impl Default for MarkdownSettings {
    fn default() -> Self {
        Self {
            preview_layout: PreviewKind::Markdown.default_mode(),
            sync_scroll: true,
            preview_font_size: DEFAULT_MARKDOWN_FONT_SIZE,
        }
    }
}

/// Where the mode a file is shown in comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ModeScope {
    /// The file's own mode, set by the switch in its editor.
    File,
    /// The mode set for every file of its kind (`editor.preview.setMode` with `scope: type`).
    Type,
    /// The kind's own default: the file was never switched and its kind has no mode set.
    Default,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    #[serde(default)]
    kinds: BTreeMap<String, PreviewMode>,
    #[serde(default)]
    files: Vec<(PathBuf, PreviewMode)>,
    #[serde(default)]
    orientations: Vec<(PathBuf, SplitOrientation)>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OnDisk {
    Current(Stored),
    Kinds(BTreeMap<String, PreviewMode>),
}

type Slot = Arc<OnceLock<Result<Content, String>>>;

#[derive(Default)]
struct DocumentPreview {
    shown: Option<(u64, Result<Content, String>)>,
    running: Option<(u64, Slot)>,
    zoom: Option<ImageZoom>,
    fit_scale: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreviewState {
    Rendering,
    Ready { content: Content, version: u64, stale: bool },
    Invalid { error: String, version: u64 },
}

pub struct Previews {
    stored: Stored,
    data: Option<PathBuf>,
    split: f32,
    documents: HashMap<PathBuf, DocumentPreview>,
    markdown_layout_set: Option<PreviewMode>,
}

impl Default for Previews {
    fn default() -> Self {
        Self { stored: Stored::default(), data: None, split: 0.5, documents: HashMap::new(), markdown_layout_set: None }
    }
}

pub const SPLIT_RANGE: std::ops::RangeInclusive<f32> = 0.15..=0.85;

impl Previews {
    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.stored = match std::fs::read(data.join(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()) {
            Some(OnDisk::Current(stored)) => stored,
            Some(OnDisk::Kinds(kinds)) => Stored { kinds, ..Stored::default() },
            None => Stored::default(),
        };
        self.data = Some(data);
    }

    pub fn mode(&self, kind: PreviewKind) -> PreviewMode {
        self.stored.kinds.get(kind.id()).copied().unwrap_or_else(|| kind.default_mode())
    }

    pub fn mode_of(&self, path: &Path, kind: PreviewKind) -> PreviewMode {
        self.file_mode(path).unwrap_or_else(|| self.mode(kind))
    }

    pub fn scope_of(&self, path: &Path, kind: PreviewKind) -> ModeScope {
        if self.file_mode(path).is_some() {
            ModeScope::File
        } else if self.stored.kinds.contains_key(kind.id()) {
            ModeScope::Type
        } else {
            ModeScope::Default
        }
    }

    fn file_mode(&self, path: &Path) -> Option<PreviewMode> {
        self.stored.files.iter().rev().find(|(file, _)| file == path).map(|(_, mode)| *mode)
    }

    pub fn set_mode(&mut self, kind: PreviewKind, mode: PreviewMode) {
        self.stored.kinds.insert(kind.id().to_owned(), mode);
        self.save();
    }

    pub fn set_mode_by_command(&mut self, kind: PreviewKind, mode: PreviewMode) {
        self.set_mode(kind, mode);
        if kind == PreviewKind::Markdown {
            self.markdown_layout_set = Some(mode);
        }
    }

    pub fn take_markdown_layout_set(&mut self) -> Option<PreviewMode> {
        self.markdown_layout_set.take()
    }

    pub fn orientation_of(&self, path: &Path) -> SplitOrientation {
        self.stored
            .orientations
            .iter()
            .rev()
            .find(|(file, _)| file == path)
            .map(|(_, orientation)| *orientation)
            .unwrap_or_default()
    }

    pub fn set_orientation(&mut self, path: &Path, orientation: SplitOrientation) {
        self.stored.orientations.retain(|(file, _)| file != path);
        self.stored.orientations.push((path.to_owned(), orientation));
        let over = self.stored.orientations.len().saturating_sub(MAX_FILES);
        self.stored.orientations.drain(..over);
        self.save();
    }

    pub fn set_file_mode(&mut self, path: &Path, mode: PreviewMode) {
        self.stored.files.retain(|(file, _)| file != path);
        self.stored.files.push((path.to_owned(), mode));
        let over = self.stored.files.len().saturating_sub(MAX_FILES);
        self.stored.files.drain(..over);
        self.save();
    }

    pub fn forget_file_mode(&mut self, path: &Path) {
        let before = self.stored.files.len();
        self.stored.files.retain(|(file, _)| file != path);
        if self.stored.files.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        if let Some(data) = &self.data {
            let _ = std::fs::create_dir_all(data);
            let _ =
                zen_vfs::write_atomic(&data.join(FILE), &serde_json::to_vec_pretty(&self.stored).unwrap_or_default());
        }
    }

    pub fn split(&self) -> f32 {
        self.split
    }

    pub fn set_split(&mut self, split: f32) {
        if split.is_finite() {
            self.split = split.clamp(*SPLIT_RANGE.start(), *SPLIT_RANGE.end());
        }
    }

    pub fn poll(
        &mut self,
        kind: PreviewKind,
        path: &Path,
        version: u64,
        text: impl FnOnce() -> String,
    ) -> PreviewState {
        let preview = self.documents.entry(path.to_owned()).or_default();
        if let Some((running, slot)) = &preview.running
            && let Some(result) = slot.get()
        {
            preview.shown = Some((*running, result.clone()));
            preview.running = None;
        }
        let shown_version = preview.shown.as_ref().map(|(shown, _)| *shown);
        // WHY: one rendering at a time, so the newest text renders next and typing never waits for it.
        if preview.running.is_none() && shown_version != Some(version) {
            let slot: Slot = Arc::default();
            let job = {
                let (slot, text) = (Arc::clone(&slot), text());
                move || {
                    let _ = slot.set(kind.render(&text));
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(job);
            #[cfg(target_arch = "wasm32")]
            job();
            preview.running = Some((version, slot));
        }
        match &preview.shown {
            None => PreviewState::Rendering,
            Some((shown, Ok(content))) => {
                PreviewState::Ready { content: content.clone(), version: *shown, stale: *shown != version }
            }
            Some((shown, Err(error))) => PreviewState::Invalid { error: error.clone(), version: *shown },
        }
    }

    pub fn wants(&self, path: &Path, version: u64) -> bool {
        let Some(preview) = self.documents.get(path) else { return true };
        match &preview.running {
            Some((running, slot)) => slot.get().is_some() && *running != version,
            None => preview.shown.as_ref().is_none_or(|(shown, _)| *shown != version),
        }
    }

    pub fn rendering(&self, path: &Path) -> bool {
        self.documents.get(path).is_some_and(|preview| preview.running.is_some())
    }

    pub fn retain(&mut self, open: &[PathBuf]) {
        self.documents.retain(|path, _| open.contains(path));
    }

    pub fn zoom(&self, path: &Path) -> ImageZoom {
        self.documents.get(path).and_then(|preview| preview.zoom).unwrap_or(ImageZoom::Auto)
    }

    pub fn scale(&self, path: &Path) -> f32 {
        let fit = self
            .documents
            .get(path)
            .map_or(1.0, |preview| if preview.fit_scale > 0.0 { preview.fit_scale } else { 1.0 });
        match self.zoom(path) {
            ImageZoom::Auto => fit.min(1.0),
            ImageZoom::Fit => fit,
            ImageZoom::Scale(scale) => scale,
        }
    }

    pub fn set_fit_scale(&mut self, path: &Path, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.documents.entry(path.to_owned()).or_default().fit_scale = scale;
        }
    }

    pub fn set_zoom(&mut self, path: &Path, zoom: ImageZoom) {
        let zoom = match zoom {
            ImageZoom::Scale(scale) if scale.is_finite() => {
                ImageZoom::Scale(scale.clamp(*crate::ZOOM_RANGE.start(), *crate::ZOOM_RANGE.end()))
            }
            ImageZoom::Scale(_) => return,
            other => other,
        };
        self.documents.entry(path.to_owned()).or_default().zoom = Some(zoom);
    }
}

fn target(session: &Session, path: Option<&Path>) -> Result<(PathBuf, PreviewKind), CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let path = match path {
        Some(path) => workbench.resolve(path),
        None => workbench
            .active_document()
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    let kind = PreviewKind::of(&path)
        .ok_or_else(|| CommandError::new(t!("workbench-preview-none", file = crate::display(&path))))?;
    Ok((path, kind))
}

pub fn active(session: &Session) -> Option<(PathBuf, PreviewKind)> {
    let document = session.state::<Workbench>()?.active_document()?;
    let kind = PreviewKind::of(document.path())?;
    let mode =
        session.state::<Previews>().map_or(kind.default_mode(), |previews| previews.mode_of(document.path(), kind));
    mode.shows_preview().then(|| (document.path().to_owned(), kind))
}

/// What the preview commands answer.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInfo {
    /// The file.
    pub path: PathBuf,
    /// What its preview shows, e.g. `svg`.
    pub kind: PreviewKind,
    /// How the file is shown.
    pub mode: PreviewMode,
    /// How the editor and the preview share the file's editor in editorAndPreview: `vertical` (side
    /// by side) or `horizontal` (one above the other).
    pub orientation: SplitOrientation,
    /// Where that mode comes from: `file` (its own, set by its switch), `type` (set for its kind) or
    /// `default` (its kind's default).
    pub scope: ModeScope,
    /// `ready`, `rendering` (nothing drawn yet, or a newer text being drawn), `invalid` (the text
    /// does not render) or `notOpen`.
    pub state: String,
    /// Why the text does not render, when it does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The picture's own size, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<(f32, f32)>,
    /// For Markdown: how many blocks the preview has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<usize>,
}

fn info(session: &mut Session, path: &Path, kind: PreviewKind) -> PreviewInfo {
    let (mode, scope) = session.state::<Previews>().map_or((kind.default_mode(), ModeScope::Default), |previews| {
        (previews.mode_of(path, kind), previews.scope_of(path, kind))
    });
    let orientation =
        session.state::<Previews>().map_or_else(SplitOrientation::default, |previews| previews.orientation_of(path));
    let document = session.state::<Workbench>().and_then(|workbench| {
        let document = workbench.documents().iter().find(|document| document.path() == path)?;
        Some((document.version(), document.text().to_owned()))
    });
    let Some((version, text)) = document else {
        return PreviewInfo {
            path: path.to_owned(),
            kind,
            mode,
            orientation,
            scope,
            state: "notOpen".to_owned(),
            error: None,
            size: None,
            blocks: None,
        };
    };
    let state = session.state_mut::<Previews>().poll(kind, path, version, || text);
    let (state, error, size, blocks) = match state {
        PreviewState::Rendering => ("rendering", None, None, None),
        PreviewState::Ready { content, stale, .. } => (
            if stale { "rendering" } else { "ready" },
            None,
            content.picture().map(|picture| picture.natural_size()),
            content.markdown().map(|rendered| rendered.blocks.len()),
        ),
        PreviewState::Invalid { error, version: shown } => {
            (if shown == version { "invalid" } else { "rendering" }, Some(error), None, None)
        }
    };
    PreviewInfo { path: path.to_owned(), kind, mode, orientation, scope, state: state.to_owned(), error, size, blocks }
}

/// The parameters of `editor.preview.getMode`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GetModeParams {
    /// An open file (absolute, or relative to the project); the active one when absent.
    pub path: Option<PathBuf>,
    /// Waits up to this long (at most 10 s) for the preview of the current text to be drawn.
    pub wait_ms: Option<u64>,
}

pub struct GetPreviewMode;

impl Command for GetPreviewMode {
    type Params = GetModeParams;
    type Output = PreviewInfo;
    const ID: &'static str = "editor.preview.getMode";
    const TITLE: &'static str = "Preview Mode";
    const DESCRIPTION: &'static str = "The preview of a file that has one (an SVG, Markdown): its kind, the mode it is \
         shown in (editorOnly, editorAndPreview, previewOnly) with scope, where that mode comes from (file: its own; \
         type: set for its kind; default: the kind's default), its split orientation (vertical: side by side; \
         horizontal: one above the other), and the preview's state: ready, rendering, invalid (with \
         error: why the text does not render) or notOpen, and the picture's size. With waitMs it waits for the current \
         text to be drawn.";

    fn run(&self, session: &mut Session, params: GetModeParams) -> Result<PreviewInfo, CommandError> {
        let (path, kind) = target(session, params.path.as_deref())?;
        let deadline = params.wait_ms.map(|wait| std::time::Duration::from_millis(wait.min(10_000)));
        let started = std::time::Instant::now();
        loop {
            let answer = info(session, &path, kind);
            let waiting = answer.state == "rendering" && deadline.is_some_and(|deadline| started.elapsed() < deadline);
            if !waiting {
                return Ok(answer);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// What `editor.preview.setMode` sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SetScope {
    /// The file's own mode, as its editor's switch sets it.
    #[default]
    File,
    /// The mode of every file of its kind that has none of its own; the file's own is dropped.
    Type,
}

/// The parameters of `editor.preview.setMode`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetModeParams {
    /// The mode.
    pub mode: PreviewMode,
    /// An open file of the kind (absolute, or relative to the project); the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// `file` (the default, as the switch): this file's mode; `type`: the mode of the files of its
    /// kind that were never switched, this one included.
    #[serde(default)]
    pub scope: SetScope,
}

pub struct SetPreviewMode;

impl Command for SetPreviewMode {
    type Params = SetModeParams;
    type Output = PreviewInfo;
    const ID: &'static str = "editor.preview.setMode";
    const TITLE: &'static str = "Set Preview Mode";
    const DESCRIPTION: &'static str = "Shows a file with a preview (an SVG, Markdown) in a mode: editorOnly, \
         editorAndPreview or previewOnly. With scope file (the default) it is the file's own mode, kept for the file, \
         as the switch in the editor's top-right corner sets it; with scope type it is the mode of every file of its \
         kind that was never switched, this one included.";

    fn run(&self, session: &mut Session, params: SetModeParams) -> Result<PreviewInfo, CommandError> {
        let (path, kind) = target(session, params.path.as_deref())?;
        let previews = session.state_mut::<Previews>();
        match params.scope {
            SetScope::File => previews.set_file_mode(&path, params.mode),
            SetScope::Type => {
                previews.set_mode_by_command(kind, params.mode);
                previews.forget_file_mode(&path);
            }
        }
        Ok(info(session, &path, kind))
    }
}

/// The parameters of `editor.preview.setOrientation`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetOrientationParams {
    /// `vertical` (side by side) or `horizontal` (one above the other).
    pub orientation: SplitOrientation,
    /// An open file with a preview (absolute, or relative to the project); the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct SetPreviewOrientation;

impl Command for SetPreviewOrientation {
    type Params = SetOrientationParams;
    type Output = PreviewInfo;
    const ID: &'static str = "editor.preview.setOrientation";
    const TITLE: &'static str = "Set Split Orientation";
    const DESCRIPTION: &'static str = "Sets how the editor and the preview of a file (an SVG, Markdown) share its editor \
         in editorAndPreview: vertical (side by side) or horizontal (one above the other), kept for the file.";

    fn run(&self, session: &mut Session, params: SetOrientationParams) -> Result<PreviewInfo, CommandError> {
        let (path, kind) = target(session, params.path.as_deref())?;
        session.state_mut::<Previews>().set_orientation(&path, params.orientation);
        Ok(info(session, &path, kind))
    }
}

pub struct ToggleOrientation;

impl Command for ToggleOrientation {
    type Params = NoParams;
    type Output = PreviewInfo;
    const ID: &'static str = "editor.preview.toggleOrientation";
    const TITLE: &'static str = "Change Split Orientation";
    const DESCRIPTION: &'static str = "Switches the editor and the preview of the active file (an SVG, Markdown) between \
         side by side and one above the other, kept for the file.";

    fn is_available(&self, session: &Session) -> bool {
        has_preview(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<PreviewInfo, CommandError> {
        let (path, kind) = target(session, None)?;
        let previews = session.state_mut::<Previews>();
        let flipped = previews.orientation_of(&path).flipped();
        previews.set_orientation(&path, flipped);
        Ok(info(session, &path, kind))
    }
}

fn has_preview(session: &Session) -> bool {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .is_some_and(|document| PreviewKind::of(document.path()).is_some())
}

macro_rules! mode_action {
    ($name:ident, $id:literal, $title:literal, $mode:expr, $description:literal) => {
        #[doc = concat!("`", $id, "`")]
        pub struct $name;

        impl Command for $name {
            type Params = NoParams;
            type Output = PreviewInfo;
            const ID: &'static str = $id;
            const TITLE: &'static str = $title;
            const DESCRIPTION: &'static str = $description;

            fn is_available(&self, session: &Session) -> bool {
                has_preview(session)
            }

            fn run(&self, session: &mut Session, _: NoParams) -> Result<PreviewInfo, CommandError> {
                let (path, kind) = target(session, None)?;
                session.state_mut::<Previews>().set_file_mode(&path, $mode);
                Ok(info(session, &path, kind))
            }
        }
    };
}

mode_action!(
    EditorOnly,
    "editor.preview.editorOnly",
    "Editor Only",
    PreviewMode::EditorOnly,
    "Shows only the text editor for the active file (an SVG, Markdown), kept for the file."
);
mode_action!(
    EditorAndPreview,
    "editor.preview.editorAndPreview",
    "Editor and Preview",
    PreviewMode::EditorAndPreview,
    "Shows the text editor and the preview side by side for the active file (an SVG, Markdown), kept for the file."
);
mode_action!(
    PreviewOnly,
    "editor.preview.previewOnly",
    "Preview Only",
    PreviewMode::PreviewOnly,
    "Shows only the preview for the active file (an SVG, Markdown), kept for the file."
);

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetPreviewMode)?;
    registry.register(SetPreviewMode)?;
    registry.register(EditorOnly)?;
    registry.register(EditorAndPreview)?;
    registry.register(PreviewOnly)?;
    registry.register(SetPreviewOrientation)?;
    registry.register(ToggleOrientation)?;
    Ok(())
}

#[cfg(test)]
mod tests;
