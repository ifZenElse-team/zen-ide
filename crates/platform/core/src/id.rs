use std::fmt;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CommandId(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidCommandId {
    pub id: String,
    pub reason: &'static str,
}

impl fmt::Display for InvalidCommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid command id `{}`: {}; expected `<area>.<group>.<action>` in lowerCamelCase",
            self.id, self.reason
        )
    }
}

impl std::error::Error for InvalidCommandId {}

impl CommandId {
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidCommandId> {
        let id = id.into();
        let reject = |reason| Err(InvalidCommandId { id: id.clone(), reason });
        let segments: Vec<&str> = id.split('.').collect();
        if segments.len() != 3 {
            return reject("it must have exactly three segments separated by dots");
        }
        for segment in segments {
            let mut chars = segment.chars();
            match chars.next() {
                None => return reject("a segment is empty"),
                Some(first) if !first.is_ascii_lowercase() => {
                    return reject("each segment must start with a lowercase ASCII letter");
                }
                Some(_) => {}
            }
            if !chars.all(|c| c.is_ascii_alphanumeric()) {
                return reject("segments may contain only ASCII letters and digits");
            }
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ids() {
        for id in ["editor.caret.moveToLineEnd", "toolbox.tool.install", "a.b.c", "file.recent.open2"] {
            assert!(CommandId::new(id).is_ok(), "{id}");
        }
    }

    #[test]
    fn invalid_ids() {
        for id in [
            "",
            "editor.caret",
            "a.b.c.d",
            "Editor.caret.move",
            "editor..move",
            "editor.caret.move-to",
            "editor.caret.2move",
            "editor.caret.mové",
        ] {
            assert!(CommandId::new(id).is_err(), "{id}");
        }
    }
}
