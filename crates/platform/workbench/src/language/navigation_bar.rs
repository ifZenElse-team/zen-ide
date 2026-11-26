use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_syntax::SymbolKind;

use super::breadcrumbs::crumbs;
use super::structure::{Resolved, StructureStatus, Symbol, resolve};
use crate::Workbench;

pub const SHOW_MEMBERS: &str = "navigate.navigationBar.members";

/// What a segment of the navigation bar or an entry of its popup is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    /// The project's folder.
    Project,
    /// A folder.
    Folder,
    /// A file.
    File,
    /// A declaration of a file.
    Symbol,
}

/// A segment of the navigation bar, or an entry of a segment's popup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationItem {
    /// The name shown.
    pub name: String,
    /// What it is.
    pub kind: ItemKind,
    /// The folder or file; for a declaration, its file.
    pub path: PathBuf,
    /// What the declaration is; only for a declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<SymbolKind>,
    /// The character offset of the declaration's name, where going to it puts the caret; only for a declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    /// The line of the declaration's name, one-based; only for a declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Whether it has children to list: a folder always, a file when its declarations are shown, a declaration with declarations inside.
    pub has_children: bool,
}

/// The parameters of `navigate.navigationBar.get`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationBarParams {
    /// The file or folder the bar leads to, absolute or relative to the project; the active file when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Whether the declarations at the caret follow the file (Show Members); the setting when absent.
    #[serde(default)]
    pub members: Option<bool>,
    /// How long to wait for the language server, in milliseconds; not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `navigate.navigationBar.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationBarState {
    /// The file or folder the bar leads to; absent when no file is open and none was given.
    pub path: Option<PathBuf>,
    /// Whether the declarations at the caret are shown.
    pub members: bool,
    /// Whether the declarations are ready; absent for a folder or when they are not shown.
    pub status: Option<StructureStatus>,
    /// The segments, the project first: its folders, the file, then the declarations at the caret.
    pub segments: Vec<NavigationItem>,
}

/// The parameters of `navigate.navigationBar.children`.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenParams {
    /// The folder or file whose children are listed, absolute or relative to the project.
    pub path: PathBuf,
    /// The offset of a declaration's name in the file, to list the declarations inside it; the file's own declarations when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// How long to wait for the language server, in milliseconds; not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `navigate.navigationBar.children` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationChildren {
    /// Whether the children are complete: a folder's always are; a file's declarations may still be coming.
    pub status: StructureStatus,
    /// Why there are no declarations, when there are none to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The children: a folder's folders first, then its files; a file's or a declaration's declarations in the order of the text.
    pub items: Vec<NavigationItem>,
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn folder_item(path: &Path, kind: ItemKind) -> NavigationItem {
    NavigationItem {
        name: name_of(path),
        kind,
        path: path.to_owned(),
        symbol_kind: None,
        offset: None,
        line: None,
        has_children: true,
    }
}

fn symbol_item(path: &Path, symbol: &Symbol) -> NavigationItem {
    NavigationItem {
        name: symbol.name.clone(),
        kind: ItemKind::Symbol,
        path: path.to_owned(),
        symbol_kind: Some(symbol.kind),
        offset: Some(symbol.selection_start),
        line: Some(symbol.line),
        has_children: !symbol.children.is_empty(),
    }
}

pub fn path_segments(project: Option<&Path>, path: &Path, is_dir: bool) -> Vec<NavigationItem> {
    let mut segments = Vec::new();
    let (mut at, relative) =
        match project.and_then(|project| path.strip_prefix(project).ok().map(|rest| (project, rest))) {
            Some((project, rest)) => {
                segments.push(folder_item(project, ItemKind::Project));
                (project.to_owned(), rest.to_owned())
            }
            None => (PathBuf::new(), path.to_owned()),
        };
    let parts: Vec<_> = relative.components().filter(|part| matches!(part, std::path::Component::Normal(_))).collect();
    if at.as_os_str().is_empty()
        && let Some(root) = path.ancestors().last()
    {
        at = root.to_owned();
    }
    for (index, part) in parts.iter().enumerate() {
        at.push(part);
        let last = index + 1 == parts.len();
        let kind = if last && !is_dir { ItemKind::File } else { ItemKind::Folder };
        let mut item = folder_item(&at, kind);
        if kind == ItemKind::File {
            item.has_children = false;
        }
        segments.push(item);
    }
    segments
}

fn find(symbols: &[Symbol], offset: usize) -> Option<&Symbol> {
    symbols
        .iter()
        .find_map(|symbol| if symbol.selection_start == offset { Some(symbol) } else { find(&symbol.children, offset) })
}

pub struct GetNavigationBar;

impl Command for GetNavigationBar {
    type Params = NavigationBarParams;
    type Output = NavigationBarState;
    const ID: &'static str = "navigate.navigationBar.get";
    const TITLE: &'static str = "Navigation Bar";
    const DESCRIPTION: &'static str = "The segments of the navigation bar (View → Appearance → Navigation Bar): the project, \
         the folders and the file, then, with Show Members, the declarations around the caret, outermost first; the active \
         file by default, or the file or folder given. navigate.navigationBar.children lists a segment's children.";

    fn run(&self, session: &mut Session, params: NavigationBarParams) -> Result<NavigationBarState, CommandError> {
        let members = params.members.unwrap_or_else(|| session.settings().get::<bool>(SHOW_MEMBERS).unwrap_or(true));
        let workbench = session.state::<Workbench>();
        let project = workbench.and_then(Workbench::project).map(Path::to_owned);
        let given =
            params.path.map(|path| workbench.map_or_else(|| path.clone(), |workbench| workbench.resolve(&path)));
        let path =
            given.or_else(|| workbench.and_then(Workbench::active_document).map(|document| document.path().to_owned()));
        let Some(path) = path else {
            let segments = project.iter().map(|project| folder_item(project, ItemKind::Project)).collect();
            return Ok(NavigationBarState { path: None, members, status: None, segments });
        };
        let is_dir = path.is_dir();
        let mut segments = path_segments(project.as_deref(), &path, is_dir);
        if is_dir || !members {
            return Ok(NavigationBarState { path: Some(path), members, status: None, segments });
        }
        let Resolved { status, symbols, caret, .. } = resolve(session, &path, super::wait(params.wait_ms));
        if let Some(file) = segments.last_mut() {
            file.has_children = !symbols.is_empty();
        }
        if let Some(caret) = caret {
            for crumb in crumbs(&symbols, caret) {
                let has_children =
                    find(&symbols, crumb.selection_start).is_some_and(|found| !found.children.is_empty());
                segments.push(NavigationItem {
                    name: crumb.name,
                    kind: ItemKind::Symbol,
                    path: path.clone(),
                    symbol_kind: Some(crumb.kind),
                    offset: Some(crumb.selection_start),
                    line: Some(crumb.line),
                    has_children,
                });
            }
        }
        Ok(NavigationBarState { path: Some(path), members, status: Some(status), segments })
    }
}

pub struct NavigationBarChildren;

impl Command for NavigationBarChildren {
    type Params = ChildrenParams;
    type Output = NavigationChildren;
    const ID: &'static str = "navigate.navigationBar.children";
    const TITLE: &'static str = "Navigation Bar Children";
    const DESCRIPTION: &'static str = "What the popup of a navigation bar segment lists: a folder's folders and files \
         (version control folders left out), a file's declarations, or with `offset` the declarations inside the \
         declaration whose name starts there.";

    fn run(&self, session: &mut Session, mut params: ChildrenParams) -> Result<NavigationChildren, CommandError> {
        if let Some(workbench) = session.state::<Workbench>() {
            params.path = workbench.resolve(&params.path);
        }
        if params.path.is_dir() {
            let entries = zen_vfs::list(&params.path).map_err(|error| CommandError::new(error.to_string()))?;
            let items = entries
                .into_iter()
                .map(|entry| {
                    let kind = if entry.is_dir { ItemKind::Folder } else { ItemKind::File };
                    let mut item = folder_item(&entry.path, kind);
                    item.name = entry.name;
                    item.has_children = entry.is_dir;
                    item
                })
                .collect();
            return Ok(NavigationChildren { status: StructureStatus::Ready, reason: None, items });
        }
        let Resolved { status, symbols, reason, .. } = resolve(session, &params.path, super::wait(params.wait_ms));
        let level = match params.offset {
            Some(offset) => find(&symbols, offset).map(|symbol| symbol.children.as_slice()).unwrap_or_default(),
            None => symbols.as_slice(),
        };
        let items = level.iter().map(|symbol| symbol_item(&params.path, symbol)).collect();
        Ok(NavigationChildren { status, reason, items })
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetNavigationBar)?;
    registry.register(NavigationBarChildren)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use zen_syntax::{OutlineItem, Outliner};

    use super::*;
    use crate::language::structure::Outliners;

    const SOURCE: &str = "package shop\n\ntype Cart struct {\n\titems []string\n}\n\nfunc (c *Cart) Add(item string) {\n\tc.items = append(c.items, item)\n}\n";

    fn at(needle: &str) -> usize {
        SOURCE.find(needle).unwrap()
    }

    struct CartOutliner;

    impl Outliner for CartOutliner {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, _: &str) -> Vec<OutlineItem> {
            let method = OutlineItem {
                name: "Add".to_owned(),
                detail: None,
                kind: SymbolKind::Method,
                start: at("func (c"),
                end: SOURCE.len() - 1,
                selection_start: at("Add"),
                children: Vec::new(),
            };
            vec![OutlineItem {
                name: "Cart".to_owned(),
                detail: None,
                kind: SymbolKind::Struct,
                start: at("type Cart"),
                end: SOURCE.len() - 1,
                selection_start: at("Cart"),
                children: vec![method],
            }]
        }
    }

    fn setup() -> (crate::language::testing::Fixture, PathBuf) {
        let mut fixture = crate::language::testing::fixture(&[
            ("go.mod", "module shop\n"),
            ("internal/cart/cart.go", SOURCE),
            (".DS_Store", ""),
        ]);
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(CartOutliner)]);
        fixture.open("internal/cart/cart.go");
        let file = fixture.path("internal/cart/cart.go");
        (fixture, file)
    }

    fn names(value: &Value, key: &str) -> Vec<String> {
        value[key].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn the_bar_leads_from_the_project_to_the_declaration_at_the_caret() {
        let (mut fixture, file) = setup();
        fixture.with(|_, workbench| {
            let document = workbench.active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at("c.items ="))));
        });
        let bar = fixture.dispatch("navigate.navigationBar.get", json!({})).unwrap();
        let project = fixture.path("").file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(names(&bar, "segments"), [project.as_str(), "internal", "cart", "cart.go", "Cart", "Add"], "{bar}");
        let kinds: Vec<&str> =
            bar["segments"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["project", "folder", "folder", "file", "symbol", "symbol"]);
        assert_eq!(bar["segments"][5]["offset"], at("Add"));
        assert_eq!(bar["segments"][5]["symbolKind"], "method");
        assert_eq!(bar["path"], json!(file));

        let plain = fixture.dispatch("navigate.navigationBar.get", json!({ "members": false })).unwrap();
        assert_eq!(
            names(&plain, "segments"),
            [project.as_str(), "internal", "cart", "cart.go"],
            "without Show Members"
        );
        fixture.session.settings_mut().set(SHOW_MEMBERS, false).unwrap();
        let off = fixture.dispatch("navigate.navigationBar.get", json!({})).unwrap();
        assert_eq!(off["members"], false);
        assert_eq!(names(&off, "segments").len(), 4, "the setting turns them off");

        let folder = file.parent().unwrap().parent().unwrap();
        let drilled = fixture.dispatch("navigate.navigationBar.get", json!({ "path": folder })).unwrap();
        assert_eq!(names(&drilled, "segments"), [project.as_str(), "internal"]);
        assert_eq!(drilled["segments"][1]["kind"], "folder");
    }

    #[test]
    fn a_segment_lists_its_folders_files_and_declarations() {
        let (mut fixture, file) = setup();
        let project = fixture.path("");
        let root = fixture.dispatch("navigate.navigationBar.children", json!({ "path": project })).unwrap();
        let relative = fixture.dispatch("navigate.navigationBar.children", json!({ "path": "internal" })).unwrap();
        assert_eq!(names(&relative, "items"), ["cart"], "a path relative to the project");
        assert_eq!(names(&root, "items"), ["internal", "go.mod"], "folders first, .DS_Store left out: {root}");
        assert_eq!(root["items"][0]["hasChildren"], true);
        assert_eq!(root["items"][1]["kind"], "file");
        let members = fixture.dispatch("navigate.navigationBar.children", json!({ "path": file })).unwrap();
        assert_eq!(names(&members, "items"), ["Cart"], "{members}");
        assert_eq!(members["items"][0]["hasChildren"], true);
        let inside =
            fixture.dispatch("navigate.navigationBar.children", json!({ "path": file, "offset": at("Cart") })).unwrap();
        assert_eq!(names(&inside, "items"), ["Add"]);
        assert_eq!(inside["items"][0]["offset"], at("Add"));
    }

    #[test]
    fn a_file_outside_the_project_starts_at_the_root() {
        let segments = path_segments(Some(Path::new("/work/shop")), Path::new("/usr/lib/go/fmt/print.go"), false);
        let names: Vec<&str> = segments.iter().map(|segment| segment.name.as_str()).collect();
        assert_eq!(names, ["usr", "lib", "go", "fmt", "print.go"]);
        assert_eq!(segments[4].path, Path::new("/usr/lib/go/fmt/print.go"));
        assert_eq!(segments[4].kind, ItemKind::File);
    }
}
