mod actions;
mod keystroke;

use std::collections::BTreeMap;

use roxmltree::Node;
use zen_core::keymap::{ImportWarning, KeymapImport, KeymapImporter, Unmapped, UnmappedReason};

pub use actions::KEYMAP_ACTIONS;

pub const MACOS_PARENT: &str = "Mac OS X 10.5+";

#[derive(Clone, Copy, Debug, Default)]
pub struct JetBrainsKeymapImporter;

impl KeymapImporter for JetBrainsKeymapImporter {
    fn detects(&self, text: &str) -> bool {
        crate::idea::xml::parse(text).is_ok_and(|document| document.root_element().has_tag_name("keymap"))
    }

    fn import(&self, text: &str) -> Result<KeymapImport, String> {
        let document = crate::idea::xml::parse(text)?;
        let root = document.root_element();
        if !root.has_tag_name("keymap") {
            return Err(format!("the root element is <{}>, not <keymap>", root.tag_name().name()));
        }
        Ok(read(root))
    }
}

fn read(root: Node<'_, '_>) -> KeymapImport {
    let parent = root.attribute("parent").map(str::to_owned);
    let mut report = KeymapImport {
        name: root.attribute("name").unwrap_or_default().trim().to_owned(),
        parent: parent.clone(),
        ..KeymapImport::default()
    };
    if parent.as_deref() != Some(MACOS_PARENT) {
        report.warnings.push(ImportWarning::ForeignParent { parent });
    }
    let mut shortcuts: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for action in root.children().filter(|node| node.has_tag_name("action")) {
        let Some(id) = action.attribute("id") else { continue };
        let targets: Vec<&str> = actions::targets(id).collect();
        if targets.is_empty() {
            report.unmapped.push(unmapped(id, UnmappedReason::UnknownAction, None));
            continue;
        }
        report.mapped += 1;
        let mut keys = Vec::new();
        for shortcut in action.children().filter(Node::is_element) {
            if shortcut.has_tag_name("mouse-shortcut") {
                let text = shortcut.attribute("keystroke").map(str::to_owned);
                report.unmapped.push(unmapped(id, UnmappedReason::MouseShortcut, text));
                continue;
            }
            if !shortcut.has_tag_name("keyboard-shortcut") {
                continue;
            }
            let first = shortcut.attribute("first-keystroke").unwrap_or_default();
            let parsed = match shortcut.attribute("second-keystroke").filter(|second| !second.trim().is_empty()) {
                Some(second) => keystroke::parse(first)
                    .zip(keystroke::parse(second))
                    .map(|(first, second)| first.followed_by(second))
                    .ok_or_else(|| format!("{first}, {second}")),
                None => keystroke::parse(first).ok_or_else(|| first.to_owned()),
            };
            match parsed {
                Ok(stroke) => keys.push(stroke),
                Err(text) => report.unmapped.push(unmapped(id, UnmappedReason::UnknownKey, Some(text))),
            }
        }
        for target in targets {
            let merged = shortcuts.entry(target.to_owned()).or_default();
            for stroke in &keys {
                if !merged.contains(stroke) {
                    merged.push(stroke.clone());
                }
            }
        }
    }
    report.shortcuts = shortcuts;
    report
}

fn unmapped(action_id: &str, reason: UnmappedReason, shortcut: Option<String>) -> Unmapped {
    Unmapped { action_id: action_id.to_owned(), reason, shortcut }
}
