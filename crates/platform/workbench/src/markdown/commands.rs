use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{Heading, Rendered, render, to_html, to_text, toggled_task};
use crate::Workbench;

pub const IMAGES_FILE: &str = "markdown-images.json";

#[derive(Default)]
pub struct RemoteImages {
    allowed: BTreeMap<String, BTreeSet<String>>,
    data: Option<PathBuf>,
}

impl RemoteImages {
    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.allowed = std::fs::read(data.join(IMAGES_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        self.data = Some(data);
    }

    pub fn allows(&self, root: &Path, host: &str) -> bool {
        self.allowed.get(&root.display().to_string()).is_some_and(|hosts| hosts.contains(&host.to_ascii_lowercase()))
    }

    pub fn hosts(&self, root: &Path) -> Vec<String> {
        self.allowed.get(&root.display().to_string()).map(|hosts| hosts.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn set(&mut self, root: &Path, host: &str, allowed: bool) {
        let hosts = self.allowed.entry(root.display().to_string()).or_default();
        if allowed {
            hosts.insert(host.to_ascii_lowercase());
        } else {
            hosts.remove(&host.to_ascii_lowercase());
        }
        if let Some(data) = &self.data {
            let _ = std::fs::create_dir_all(data);
            let _ = zen_vfs::write_atomic(
                &data.join(IMAGES_FILE),
                &serde_json::to_vec_pretty(&self.allowed).unwrap_or_default(),
            );
        }
    }
}

pub fn host_of(address: &str) -> Option<String> {
    let rest = address.strip_prefix("https://").or_else(|| address.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

pub(super) fn document_text(session: &Session, path: Option<&Path>) -> Result<(PathBuf, String), CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let document = match path {
        Some(path) => {
            let path = workbench.resolve(path);
            workbench.documents().iter().find(|document| document.path() == path)
        }
        None => workbench.active_document(),
    };
    match document {
        Some(document) => Ok((document.path().to_owned(), document.text().to_owned())),
        None => {
            let path = path
                .map(|path| workbench.resolve(path))
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
            let text = std::fs::read_to_string(&path).map_err(|error| {
                CommandError::new(t!(
                    "workbench-markdown-read",
                    path = crate::display(&path),
                    error = error.to_string()
                ))
            })?;
            Ok((path, text))
        }
    }
}

/// The parameters of `markdown.preview.render`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderParams {
    /// A Markdown file (absolute, or relative to the project); the active one when absent. An open
    /// file is read as the editor has it, unsaved edits included.
    pub path: Option<PathBuf>,
    /// `text` (the default: plain text with list markers, `[x]` boxes and `>` quotes), `html`
    /// (safe HTML, `data-line` on every block) or `blocks` (the preview's blocks as JSON, with their
    /// source lines).
    pub format: Option<String>,
}

/// What `markdown.preview.render` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderAnswer {
    /// The file.
    pub path: PathBuf,
    /// The headings, for the outline.
    pub headings: Vec<Heading>,
    /// The text, with `format` `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The HTML, with `format` `html`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// The blocks, with `format` `blocks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<Rendered>,
}

pub struct RenderPreview;

impl Command for RenderPreview {
    type Params = RenderParams;
    type Output = RenderAnswer;
    const ID: &'static str = "markdown.preview.render";
    const TITLE: &'static str = "Render Markdown Preview";
    const DESCRIPTION: &'static str = "What the Markdown preview shows for a file (the active one by default; an open \
         file as the editor has it): GitHub Flavored Markdown as plain text (format text), safe HTML with data-line \
         on every block (html) or the blocks as JSON with their source lines (blocks), and its headings with their \
         anchors and lines.";

    fn run(&self, session: &mut Session, params: RenderParams) -> Result<RenderAnswer, CommandError> {
        let (path, text) = document_text(session, params.path.as_deref())?;
        let rendered = render(&text);
        let headings = rendered.headings.clone();
        let mut answer = RenderAnswer { path, headings, text: None, html: None, rendered: None };
        match params.format.as_deref().unwrap_or("text") {
            "text" => answer.text = Some(to_text(&rendered)),
            "html" => answer.html = Some(to_html(&rendered)),
            "blocks" => answer.rendered = Some(rendered),
            other => return Err(CommandError::new(t!("workbench-markdown-format", format = other))),
        }
        Ok(answer)
    }
}

/// The parameters of `markdown.task.toggle`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleParams {
    /// The line of the task's `[ ]`, from 1, as the preview's blocks give it.
    pub line: u32,
    /// An open Markdown file (absolute, or relative to the project); the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// What `markdown.task.toggle` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Toggled {
    /// Whether the task is ticked now.
    pub checked: bool,
    /// The line as it is now.
    pub text: String,
}

pub struct ToggleTask;

impl Command for ToggleTask {
    type Params = ToggleParams;
    type Output = Toggled;
    const ID: &'static str = "markdown.task.toggle";
    const TITLE: &'static str = "Toggle Markdown Task";
    const DESCRIPTION: &'static str = "Ticks or unticks the task list item on a line of an open Markdown file \
         (`- [ ]` ↔ `- [x]`), as a click on its checkbox in the preview does: one undo step, saved as edits are.";

    fn run(&self, session: &mut Session, params: ToggleParams) -> Result<Toggled, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let path = match &params.path {
            Some(path) => workbench.resolve(path),
            None => workbench
                .active_document()
                .map(|document| document.path().to_owned())
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        let no_task = || CommandError::new(t!("workbench-markdown-no-task", line = params.line));
        let document = workbench
            .documents
            .iter_mut()
            .find(|document| document.path() == path)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let text = document.text().to_owned();
        let index = params.line.checked_sub(1).ok_or_else(no_task)? as usize;
        let start = text.split_inclusive('\n').take(index).map(str::len).sum::<usize>();
        let line = text[start..].split('\n').next().ok_or_else(no_task)?;
        let line = line.strip_suffix('\r').unwrap_or(line);
        let toggled = toggled_task(line).ok_or_else(no_task)?;
        let (from, to) = (text[..start].chars().count(), text[..start + line.len()].chars().count());
        document.replace_ranges(vec![(from..to, toggled.clone())]);
        let checked = render(&toggled).blocks.iter().any(|block| {
            block
                .nesting
                .iter()
                .any(|container| matches!(container, super::Container::Item { task: Some(task), .. } if task.checked))
        });
        Ok(Toggled { checked, text: toggled })
    }
}

/// The parameters of `markdown.images.allow`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowParams {
    /// The host, e.g. `img.shields.io`, or an image's address.
    pub host: String,
    /// `false` to no longer allow it; `true` by default.
    #[serde(default)]
    pub allowed: Option<bool>,
}

/// What the `markdown.images.*` commands answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowedHosts {
    /// The hosts the open project allows remote images from.
    pub hosts: Vec<String>,
}

fn project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

pub struct AllowImages;

impl Command for AllowImages {
    type Params = AllowParams;
    type Output = AllowedHosts;
    const ID: &'static str = "markdown.images.allow";
    const TITLE: &'static str = "Load Images from Host";
    const DESCRIPTION: &'static str = "Lets the Markdown preview of the open project load remote images from a host \
         (or, with allowed false, no longer); the preview never fetches one before. Kept per project.";

    fn run(&self, session: &mut Session, params: AllowParams) -> Result<AllowedHosts, CommandError> {
        let root = project(session)?;
        let host = host_of(&params.host).unwrap_or_else(|| params.host.trim().to_ascii_lowercase());
        if host.is_empty() || host.contains(['/', ' ']) {
            return Err(CommandError::new(t!("workbench-markdown-host", host = params.host.as_str())));
        }
        let images = session.state_mut::<RemoteImages>();
        images.set(&root, &host, params.allowed.unwrap_or(true));
        Ok(AllowedHosts { hosts: images.hosts(&root) })
    }
}

pub struct ListImageHosts;

impl Command for ListImageHosts {
    type Params = zen_core::NoParams;
    type Output = AllowedHosts;
    const ID: &'static str = "markdown.images.list";
    const TITLE: &'static str = "Image Hosts of the Project";
    const DESCRIPTION: &'static str = "The hosts the Markdown preview of the open project may load remote images from.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<AllowedHosts, CommandError> {
        let root = project(session)?;
        Ok(AllowedHosts {
            hosts: session.state::<RemoteImages>().map(|images| images.hosts(&root)).unwrap_or_default(),
        })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RenderPreview)?;
    registry.register(ToggleTask)?;
    registry.register(AllowImages)?;
    registry.register(ListImageHosts)?;
    registry.register(super::selection::GetSelection)?;
    registry.register(super::mermaid::ListDiagrams)?;
    super::format::register(registry)?;
    Ok(())
}
