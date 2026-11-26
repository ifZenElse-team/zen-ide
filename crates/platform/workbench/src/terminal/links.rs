use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_terminal::links::{self, Link, LinkKind};

use super::{SessionParams, Terminals, missing_session, project_root, started};

pub const USER_ONLY: &str = "userOnly";

/// What to do with a link, as the ⌘-click dialog offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkAction {
    /// A URL: open it in the system's browser (a person only).
    OpenInBrowser,
    /// A URL: put it on the clipboard.
    CopyLink,
    /// A file: open it in the editor at the line and column printed after it.
    OpenInEditor,
    /// A file or folder: select it in the Project tool window.
    RevealInProject,
    /// A file or folder: show it in the Finder (the system's file manager elsewhere).
    OpenInFinder,
    /// A file or folder: put its absolute path on the clipboard.
    CopyPath,
}

impl LinkAction {
    pub fn offered(kind: LinkKind) -> &'static [LinkAction] {
        match kind {
            LinkKind::Url => &[LinkAction::OpenInBrowser, LinkAction::CopyLink],
            LinkKind::File => {
                &[LinkAction::OpenInEditor, LinkAction::RevealInProject, LinkAction::OpenInFinder, LinkAction::CopyPath]
            }
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LinkAction::OpenInBrowser => "openInBrowser",
            LinkAction::CopyLink => "copyLink",
            LinkAction::OpenInEditor => "openInEditor",
            LinkAction::RevealInProject => "revealInProject",
            LinkAction::OpenInFinder => "openInFinder",
            LinkAction::CopyPath => "copyPath",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkChoices {
    pub url: Option<LinkAction>,
    pub file: Option<LinkAction>,
}

impl LinkChoices {
    pub fn get(&self, kind: LinkKind) -> Option<LinkAction> {
        let action = match kind {
            LinkKind::Url => self.url,
            LinkKind::File => self.file,
        };
        action.filter(|action| LinkAction::offered(kind).contains(action))
    }

    pub fn set(&mut self, kind: LinkKind, action: Option<LinkAction>) {
        let action = action.filter(|action| LinkAction::offered(kind).contains(action));
        match kind {
            LinkKind::Url => self.url = action,
            LinkKind::File => self.file = action,
        }
    }
}

pub fn kind_id(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Url => "url",
        LinkKind::File => "file",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    pub text: String,
    pub kind: &'static str,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub row: usize,
    pub start_column: usize,
    pub end_row: usize,
    pub end_column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkList {
    pub id: u64,
    pub links: Vec<LinkInfo>,
}

/// The parameters of `terminal.link.open`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// The link as `terminal.link.list` gives its `text`: a URL, or a path absolute or relative to the
    /// session's folder or the project, with an optional `:line[:column]` (`src/main.rs:12:5`).
    pub target: String,
    /// What to do; by default `openInBrowser` for a URL, `openInEditor` for a file and
    /// `revealInProject` for a folder.
    #[serde(default)]
    pub action: Option<LinkAction>,
    /// The session whose folder relative paths start from; the focused one by default.
    #[serde(default)]
    pub id: Option<u64>,
    /// Only answer with the command line of the browser or the file manager, without starting it.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub kind: &'static str,
    pub action: &'static str,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copied: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    pub done: bool,
}

fn bases(session: &Session, id: Option<u64>) -> Vec<PathBuf> {
    let terminals = session.state::<Terminals>();
    let pane = terminals.and_then(|terminals| id.or_else(|| terminals.focused()).and_then(|id| terminals.pane(id)));
    pane.map(|pane| pane.cwd().to_owned()).into_iter().chain(project_root(session)).collect()
}

pub fn screen_links(screen: &zen_terminal::Screen, bases: &[PathBuf]) -> Vec<LinkInfo> {
    screen
        .links()
        .into_iter()
        .filter_map(|found| {
            let target = match found.link.kind {
                LinkKind::Url => found.link.target.clone(),
                LinkKind::File => links::resolve(&found.link.target, bases)?.display().to_string(),
            };
            let (row, first) = found.cells.first()?.clone();
            let (end_row, last) = found.cells.last()?.clone();
            Some(LinkInfo {
                text: found.link.text,
                kind: kind_id(found.link.kind),
                target,
                line: found.link.line,
                column: found.link.column,
                row: row + 1,
                start_column: first.start + 1,
                end_row: end_row + 1,
                end_column: last.end,
            })
        })
        .collect()
}

fn browser_command(url: &str) -> (String, Vec<String>) {
    if cfg!(target_os = "macos") {
        ("open".to_owned(), vec![url.to_owned()])
    } else if cfg!(target_os = "windows") {
        ("rundll32".to_owned(), vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()])
    } else {
        ("xdg-open".to_owned(), vec![url.to_owned()])
    }
}

fn open_in_browser(url: &str, dry_run: bool) -> Result<(String, Vec<String>), CommandError> {
    let (program, args) = browser_command(url);
    if !dry_run {
        #[cfg(not(target_arch = "wasm32"))]
        std::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| CommandError::new(t!("terminal-link-browser-failed", error = error.to_string())))?;
    }
    Ok((program, args))
}

fn not_offered(link: &Link, action: LinkAction) -> CommandError {
    CommandError::new(t!("terminal-link-action-not-offered", action = action.id(), kind = kind_id(link.kind)))
}

pub fn open(session: &mut Session, params: OpenParams) -> Result<Opened, CommandError> {
    let link = links::parse(&params.target).ok_or_else(|| CommandError::new(t!("terminal-link-empty")))?;
    if let Some(id) = params.id
        && session.state::<Terminals>().is_none_or(|terminals| terminals.pane(id).is_none())
    {
        return Err(missing_session(id));
    }
    let kind = kind_id(link.kind);
    match link.kind {
        LinkKind::Url => {
            let action = params.action.unwrap_or(LinkAction::OpenInBrowser);
            let url = link.target.clone();
            match action {
                LinkAction::OpenInBrowser => {
                    if zen_core::macros::by_agent(session) {
                        return Err(CommandError::with_code(USER_ONLY, t!("terminal-link-browser-user-only")));
                    }
                    let (program, args) = open_in_browser(&url, params.dry_run)?;
                    Ok(Opened {
                        kind,
                        action: action.id(),
                        target: url,
                        program: Some(program),
                        args,
                        done: !params.dry_run,
                        ..Opened::default()
                    })
                }
                LinkAction::CopyLink => {
                    let done = crate::file_clipboard::copy_text(session, &url);
                    Ok(Opened {
                        kind,
                        action: action.id(),
                        copied: Some(url.clone()),
                        target: url,
                        done,
                        ..Opened::default()
                    })
                }
                other => Err(not_offered(&link, other)),
            }
        }
        LinkKind::File => {
            let bases = bases(session, params.id);
            let path = links::resolve(&link.target, &bases)
                .ok_or_else(|| CommandError::new(t!("terminal-link-no-file", path = link.target.as_str())))?;
            let folder = path.is_dir();
            let action =
                params.action.unwrap_or(if folder { LinkAction::RevealInProject } else { LinkAction::OpenInEditor });
            let target = path.display().to_string();
            let answer = Opened { kind, action: action.id(), target: target.clone(), ..Opened::default() };
            match action {
                LinkAction::OpenInEditor => {
                    if folder {
                        return Err(CommandError::new(t!("terminal-link-folder-in-editor", path = target)));
                    }
                    open_in_editor(session, &path, link.line, link.column)?;
                    Ok(Opened { line: link.line, column: link.column, done: true, ..answer })
                }
                LinkAction::RevealInProject => {
                    session.state_mut::<Terminals>().reveal_in_project = Some(path);
                    Ok(Opened { done: true, ..answer })
                }
                LinkAction::OpenInFinder => {
                    let revealed = crate::file_actions::reveal(&path, params.dry_run)?;
                    Ok(Opened { program: Some(revealed.program), args: revealed.args, done: revealed.ran, ..answer })
                }
                LinkAction::CopyPath => {
                    let done = crate::file_clipboard::copy_text(session, &target);
                    Ok(Opened { copied: Some(target), done, ..answer })
                }
                other => Err(not_offered(&link, other)),
            }
        }
    }
}

fn open_in_editor(
    session: &mut Session,
    path: &Path,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<(), CommandError> {
    match line {
        Some(line) => {
            let line = (line as usize).saturating_sub(1);
            let column = column.map_or(0, |column| (column as usize).saturating_sub(1));
            crate::history::go_to_point(session, path, line, column).map(|_| ())
        }
        None => crate::history::go_to_file(session, path).map(|_| ()),
    }
}

pub struct ListLinks;

impl Command for ListLinks {
    type Params = SessionParams;
    type Output = LinkList;
    const ID: &'static str = "terminal.link.list";
    const TITLE: &'static str = "Terminal Links";
    const DESCRIPTION: &'static str = "Lists the links on a session's visible screen (the focused session by default), \
         top to bottom, as ⌘-click finds them: URLs, and paths of existing files and folders with the `line` and \
         `column` printed after them (`src/main.rs:12:5`, `app.ts(3,14)`). Each has its `text` as printed (the \
         `target` of terminal.link.open), `kind` (`url` or `file`), `target` (the URL or the absolute path) and \
         where it is: `row`, `startColumn`, `endRow`, `endColumn`, one-based.";

    fn is_available(&self, session: &Session) -> bool {
        super::has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: SessionParams) -> Result<LinkList, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
        let screen = pane.session().snapshot();
        let bases = bases(session, Some(id));
        Ok(LinkList { id, links: screen_links(&screen, &bases) })
    }
}

pub struct OpenLink;

impl Command for OpenLink {
    type Params = OpenParams;
    type Output = Opened;
    const ID: &'static str = "terminal.link.open";
    const TITLE: &'static str = "Open Terminal Link";
    const DESCRIPTION: &'static str = "Does what ⌘-click on a terminal link offers. `target` is the link as printed \
         (terminal.link.list's `text`): a URL, or a path absolute or relative to the session's folder (the focused \
         session's, or `id`'s) or the project, with an optional `:line[:column]`. `action`: for a URL `openInBrowser` \
         (the default; refused to agents with the code `userOnly`, a person opens URLs) or `copyLink`; for a file \
         `openInEditor` (the default, at the line and column), `revealInProject` (the default for a folder), \
         `openInFinder` (the system's file manager) or `copyPath`. `dryRun` only answers with the browser's or the \
         file manager's command line.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<Opened, CommandError> {
        open(session, params)
    }
}
