use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::Session;
use zen_text::LineSeparator;

use crate::Workbench;
use crate::charsets::Charset;

/// What the project's `.editorconfig` files say about saving a file; `None` leaves it to the IDE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveProperties {
    /// `trim_trailing_whitespace`: whether a save removes the spaces and tabs at the ends of lines.
    pub trim_trailing_whitespace: Option<bool>,
    /// `insert_final_newline`: whether a save ends the file with a line break.
    pub insert_final_newline: Option<bool>,
    /// `end_of_line`: the line separator new files get and a save writes.
    pub end_of_line: Option<LineSeparator>,
    /// `charset`: the encoding the file is opened and saved in.
    pub charset: Option<Charset>,
}

impl SaveProperties {
    pub fn from_properties(properties: &std::collections::BTreeMap<String, String>) -> SaveProperties {
        let flag = |name: &str| match properties.get(name).map(String::as_str) {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        };
        SaveProperties {
            trim_trailing_whitespace: flag("trim_trailing_whitespace"),
            insert_final_newline: flag("insert_final_newline"),
            end_of_line: properties.get("end_of_line").and_then(|value| LineSeparator::parse(value)),
            charset: properties.get("charset").and_then(|value| Charset::parse(value)),
        }
    }
}

pub fn save_properties(session: &Session, path: &Path) -> SaveProperties {
    super::found_for(session, path).map(|found| SaveProperties::from_properties(&found.properties)).unwrap_or_default()
}

pub fn charset(session: &Session, path: &Path) -> Option<Charset> {
    save_properties(session, path).charset
}

pub fn before_write(session: &mut Session, path: &Path) -> bool {
    let Some(separator) = save_properties(session, path).end_of_line else { return false };
    let read_only_on_disk = std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly());
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) else {
        return false;
    };
    if !document.is_dirty() || document.image().is_some() || document.read_only().is_some() || read_only_on_disk {
        return false;
    }
    document.convert_line_separators(separator)
}
