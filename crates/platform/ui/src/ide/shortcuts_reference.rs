mod pdf;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zen_core::Session;
use zen_core::keymap::{GROUPS, group_of};
use zen_i18n::t;

use super::main_menu::{Entry, MENUS};
use super::{IdeView, keymap_page};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) title: String,
    pub(super) keys: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Group {
    pub(super) title: String,
    pub(super) rows: Vec<Row>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Pdf,
    Html,
}

impl Format {
    fn of(path: &Path, params: &Value) -> Result<Self, String> {
        let named = params.get("format").and_then(Value::as_str);
        let extension = path.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        match named.or(extension.as_deref()) {
            Some("pdf") => Ok(Self::Pdf),
            Some("html" | "htm") => Ok(Self::Html),
            other => Err(format!("no format `{}`; the formats are pdf and html", other.unwrap_or_default())),
        }
    }
}

fn walk(parent: Option<&str>, entries: &[Entry], rows: &mut Vec<Row>, seen: &mut BTreeSet<String>) {
    for entry in entries {
        match entry {
            Entry::Action(action) => add(action, keymap_page::action_title(action), rows, seen),
            Entry::Named(action, key) => {
                let title = t!(key);
                let title = match parent {
                    Some(parent) if title.chars().count() <= 2 => format!("{} {title}", t!(parent)),
                    _ => title,
                };
                add(action, title, rows, seen);
            }
            Entry::Submenu(key, entries) => walk(Some(key), entries, rows, seen),
            Entry::Separator | Entry::Layouts | Entry::NewItems | Entry::RecentProjects | Entry::PluginToolWindows => {}
        }
    }
}

fn add(action: &str, title: String, rows: &mut Vec<Row>, seen: &mut BTreeSet<String>) {
    if let Some(keys) = crate::keymap::shortcut(action)
        && seen.insert(action.to_owned())
    {
        rows.push(Row { title, keys });
    }
}

pub(super) fn groups() -> Vec<Group> {
    let mut seen = BTreeSet::new();
    let mut groups: Vec<Group> = MENUS
        .iter()
        .map(|(title, entries)| {
            let mut rows = Vec::new();
            walk(None, entries, &mut rows, &mut seen);
            Group { title: t!(title), rows }
        })
        .collect();
    let mut others: Vec<Vec<Row>> = vec![Vec::new(); GROUPS.len()];
    for binding in crate::keymap::current().bindings() {
        let group = group_of(&binding.action).min(GROUPS.len() - 1);
        add(&binding.action, keymap_page::action_title(&binding.action), &mut others[group], &mut seen);
    }
    for (rows, title) in others.into_iter().zip(GROUPS) {
        let mut rows = rows;
        rows.sort_by(|a, b| a.title.cmp(&b.title));
        groups.push(Group { title: t!(title), rows });
    }
    groups.retain(|group| !group.rows.is_empty());
    groups
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub(super) fn html(title: &str, groups: &[Group]) -> String {
    let mut page = format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{0}</title>\n<style>body{{font-family:\
         sans-serif;columns:2;margin:2em}}h1{{column-span:all}}section{{break-inside:avoid-column}}table{{width:100%;\
         border-collapse:collapse}}td:last-child{{text-align:right;white-space:nowrap}}</style>\n</head>\n<body>\n\
         <h1>{0}</h1>\n",
        escape(title)
    );
    for group in groups {
        page.push_str(&format!("<section>\n<h2>{}</h2>\n<table>\n", escape(&group.title)));
        for row in &group.rows {
            page.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>\n", escape(&row.title), escape(&row.keys)));
        }
        page.push_str("</table>\n</section>\n");
    }
    page.push_str("</body>\n</html>\n");
    page
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut url = String::from("file://");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

fn write(path: &Path, format: Format, title: &str) -> Result<(), String> {
    let groups = groups();
    let bytes = match format {
        Format::Pdf => pdf::document(title, &groups)?,
        Format::Html => html(title, &groups).into_bytes(),
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    zen_vfs::write_atomic(path, &bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn agent_path(session: &mut Session, path: &Path) -> Result<PathBuf, String> {
    let workbench = session.state_mut::<zen_workbench::Workbench>();
    let path = workbench.resolve(path);
    let root = workbench.project().map(Path::to_owned).ok_or_else(|| t!("ide-shortcuts-reference-agent-path"))?;
    if !zen_workbench::inside_project(&root, &path) || path.exists() {
        return Err(t!("ide-shortcuts-reference-agent-path"));
    }
    zen_core::macros::as_agent(session, |session| zen_workbench::hidden_files::guard_change(session, &root, &[&path]))
        .map_err(|error| error.message)?;
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_file(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .set_file_name("keyboard-shortcuts.pdf")
        .add_filter("PDF", &["pdf"])
        .add_filter("HTML", &["html"])
        .save_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_file(_: &str) -> Option<PathBuf> {
    None
}

impl IdeView {
    fn reference_title(&self) -> String {
        let keymap = crate::keymap::current_id();
        t!("ide-shortcuts-reference-title", product = self.product.name, keymap = keymap)
    }

    pub(super) fn open_shortcuts_reference(&mut self, ctx: &egui::Context) -> Result<Value, String> {
        let folder = self.help.folder.as_ref().ok_or_else(|| t!("ide-shortcuts-reference-no-folder"))?;
        let path = folder.join("keyboard-shortcuts").join("keyboard-shortcuts.pdf");
        write(&path, Format::Pdf, &self.reference_title())?;
        ctx.open_url(egui::OpenUrl::new_tab(file_url(&path)));
        Ok(json!({ "path": path }))
    }

    pub(super) fn export_shortcuts_reference(
        &mut self,
        session: &mut Session,
        params: &Value,
        agent: bool,
    ) -> Result<Value, String> {
        let path = match params.get("path").and_then(Value::as_str) {
            Some(path) if agent => agent_path(session, Path::new(path))?,
            Some(path) => PathBuf::from(path),
            None if agent => return Err("`path` is required: the save dialog is only for a person".to_owned()),
            None => match pick_file(&t!("keymap-view-keyboardShortcuts-export")) {
                Some(path) => path,
                None => return Ok(Value::Null),
            },
        };
        let format = Format::of(&path, params)?;
        write(&path, format, &self.reference_title())?;
        Ok(json!({ "path": path }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcut_list_is_grouped_as_the_menus_and_follows_the_active_keymap() {
        let groups = groups();
        assert_eq!(groups[0].title, t!("menu-file"), "the menus come first, in their order");
        let help = groups.iter().find(|group| group.title == t!("menu-help")).unwrap();
        let find_action = crate::keymap::shortcut("view.findAction.open").unwrap();
        assert!(help.rows.iter().any(|row| row.keys == find_action), "{help:?}");
        let every: Vec<&Row> = groups.iter().flat_map(|group| &group.rows).collect();
        assert!(every.iter().all(|row| !row.keys.is_empty() && !row.title.is_empty()));
        let shortcuts = keymap_page::action_title("view.keyboardShortcuts.open");
        assert!(!every.iter().any(|row| row.title == shortcuts), "the item has no default shortcut");
    }

    #[test]
    fn the_html_page_escapes_and_names_every_row() {
        let groups = [Group {
            title: "A & B".to_owned(),
            rows: vec![Row { title: "<x>".to_owned(), keys: "⇧⌘A".to_owned() }],
        }];
        let page = html("Keys", &groups);
        assert!(page.contains("<title>Keys</title>"));
        assert!(page.contains("<h2>A &amp; B</h2>"));
        assert!(page.contains("<td>&lt;x&gt;</td><td>⇧⌘A</td>"));
    }

    #[test]
    fn the_format_comes_from_the_parameter_or_the_file_name() {
        let path = Path::new("/tmp/keys.html");
        assert_eq!(Format::of(path, &json!({})).unwrap(), Format::Html);
        assert_eq!(Format::of(path, &json!({ "format": "pdf" })).unwrap(), Format::Pdf);
        assert!(Format::of(Path::new("/tmp/keys.txt"), &json!({})).unwrap_err().contains("pdf and html"));
    }

    #[test]
    fn a_path_with_spaces_is_a_file_url() {
        assert_eq!(file_url(Path::new("/a b/ключи.pdf")), "file:///a%20b/%D0%BA%D0%BB%D1%8E%D1%87%D0%B8.pdf");
    }
}
