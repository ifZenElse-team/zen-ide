use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// How bad a finding is: a weak warning is drawn quieter than a warning and counted apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FindingSeverity {
    /// An error.
    Error,
    /// A warning, e.g. a dependency with known vulnerabilities.
    Warning,
    /// A weak warning, e.g. a dependency with a newer version.
    WeakWarning,
    /// A hint, quieter still, e.g. a newer major version of a dependency (`/v2`), which needs
    /// changes in the code to take.
    Information,
}

/// What a fix does when it is picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FixKind {
    /// It is applied by its inspector ([`Inspector::apply`]).
    #[default]
    Apply,
    /// It opens a dialog listing every fix of the file with the same [`InspectionFix::batch`] key,
    /// to apply several at once, e.g. "Update dependencies…".
    Batch,
    /// It shows the finding's [`Finding::details`] in a dialog, e.g. "Show … deprecation notice".
    Details,
    /// It runs a window action: `args` is `{ "action": "view.settings.open", "params": { ... } }`,
    /// e.g. "Configure Go SDK…". Agents run the action themselves (MCP `run_action`).
    Action,
    /// A greyed caption of the fixes after it, e.g. "Fix typo"; it is never picked.
    Heading,
}

/// A quick-fix of a finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionFix {
    /// Its id, unique in the file and stable while the file does not change, e.g.
    /// `go.update:golang.org/x/crypto@v0.31.0`.
    pub id: String,
    /// What the menu says, e.g. "Update to unaffected version v0.31.0".
    pub title: String,
    /// What it does when picked.
    #[serde(default)]
    pub kind: FixKind,
    /// The key of the fixes that can be applied together from one dialog; for a [`FixKind::Batch`]
    /// fix, the fixes its dialog lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<String>,
    /// For a fix in a batch: the label of its row in the batch dialog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether the hover popup offers it on the spot, next to the problem's main fix, e.g. both
    /// "Update go directive to 1.27.3" and "Update to 1.26.10"; the main fix (the first) always is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inline: bool,
    /// The inspector's own arguments, read back by [`Inspector::apply`].
    #[serde(default)]
    pub args: Value,
}

/// A row of a finding's popup, e.g. one vulnerability: its id (a link), its score and its title.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindingRow {
    /// The first column, e.g. `CVE-2024-45337`.
    pub label: String,
    /// Where the label links to, if anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A short value after the label, e.g. the score `9.1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// The rest of the row, e.g. the title.
    pub text: String,
}

/// Something an inspector found in a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// The first byte it covers in the text.
    pub start: usize,
    /// The byte after the last it covers.
    pub end: usize,
    /// How bad it is.
    pub severity: FindingSeverity,
    /// What is wrong, in one line: the popup's heading, e.g. "Newer version is available: v1.2.6".
    pub message: String,
    /// A paragraph under the heading, if any, e.g. "Deprecation notice: …".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The rows under the heading, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<FindingRow>,
    /// More about it in Markdown, shown by "Show details…", if there is more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Its fixes, the main one first (⌥⇧↵).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<InspectionFix>,
    /// Who found it, e.g. `go.mod`.
    pub source: String,
    /// What kind of finding it is, e.g. `newerVersion` or `vulnerability`.
    pub code: String,
    /// Whether what it covers is deprecated: the editor strikes it out instead of underlining
    /// it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    /// Whether what it covers is unused, e.g. a requirement `go mod tidy` removes: the editor draws
    /// its text in the theme's unused colour (`editor.unused`) instead of underlining it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unused: bool,
}

pub struct InspectContext<'a> {
    pub root: &'a Path,
    pub path: &'a Path,
    pub text: &'a str,
    pub tool: &'a Path,
    pub env: &'a [(String, String)],
    pub setting: &'a dyn Fn(&str) -> Option<Value>,
}

/// How a fix that runs in the background is doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum FixState {
    /// Still running; the last line its program printed, if any.
    Running {
        /// The last line printed.
        line: Option<String>,
    },
    /// Done; the files it changed.
    Done {
        /// The files it changed, to read again.
        changed: Vec<PathBuf>,
    },
    /// It failed: what its program printed.
    Failed { output: String },
    /// It was cancelled: its programs were stopped.
    Cancelled,
}

/// An on/off setting an inspector declares (ADR-0059). The IDE shows it on the settings page it
/// names, keeps it with its other settings and lets agents change it (`set_settings`'s
/// `declaredSettings`); the inspector reads it from [`InspectContext::setting`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredSetting {
    /// Its key, e.g. `go.dependencies.checkUpdates`.
    pub key: String,
    /// The settings page it is shown on, e.g. `go` (Languages & Frameworks → Go).
    pub page: String,
    /// The title of its group on the page, e.g. "Dependencies".
    pub group: String,
    /// The checkbox's label.
    pub title: String,
    /// One sentence under it: what it does.
    pub description: String,
    /// Its value when the user has not set it.
    pub default: bool,
    /// The servers it lets the IDE contact, e.g. `proxy.golang.org`; empty when it uses no network.
    /// The page says so next to it, and turning it off stops that access (AGENTS.md rule 10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network: Vec<String>,
}

/// An inspection people can pick by name (Run Inspection by Name) and configure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredInspection {
    /// Its id: the `code` or `source` of the findings it gives, e.g. `newerVersion`.
    pub id: String,
    /// The name shown.
    pub title: String,
    /// The group it is listed under, e.g. `gopls` or `go.mod`.
    pub group: String,
    /// One sentence: what it reports.
    pub description: String,
    /// Whether Code Cleanup applies its fixes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cleanup: bool,
    /// Whether it is off unless a profile turns it on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

pub trait Programs: Send + Sync {
    fn interrupt(&self);

    fn kill(&self);

    fn running(&self) -> bool;
}

pub trait FixJob: Send {
    fn poll(&mut self) -> FixState;

    fn title(&self) -> Option<String> {
        None
    }

    fn drain_output(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn programs(&self) -> Option<Arc<dyn Programs>> {
        None
    }

    fn notice(&mut self) -> Option<FixNotice> {
        None
    }

    fn writes(&self) -> Vec<FixWrite> {
        Vec::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixWrite {
    pub path: PathBuf,
    pub text: String,
    pub read: Option<String>,
}

/// What a finished fix tells the user: the IDE shows it as a notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FixNotice {
    /// The notification's title, e.g. "go directive raised to 1.26.0".
    pub title: String,
    /// What happened and why, e.g. "The go directive was raised from 1.26 to 1.26.0:
    /// github.com/gofiber/fiber/v3 v3.6.1 and 6 more require go 1.26.0".
    pub text: String,
}

pub trait Inspector: Send + Sync {
    fn id(&self) -> &str;

    fn handles(&self, file_name: &str) -> bool;

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding>;

    fn generation(&self) -> u64 {
        0
    }

    fn inputs(&self, _path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    fn set_wake(&self, _wake: Wake) {}

    fn set_cache_dir(&self, _dir: PathBuf) {}

    fn settings(&self) -> Vec<DeclaredSetting> {
        Vec::new()
    }

    fn inspections(&self) -> Vec<DeclaredInspection> {
        Vec::new()
    }

    fn refresh(&self) {}

    fn plugin(&self) -> Option<&str> {
        None
    }

    fn runs_programs(&self) -> bool {
        false
    }

    fn unused_diagnostic(&self, _source: Option<&str>, _message: &str) -> bool {
        false
    }

    fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_round_trip_in_camel_case_without_empty_parts() {
        let finding = Finding {
            start: 3,
            end: 9,
            severity: FindingSeverity::WeakWarning,
            message: "Newer version is available: v1.2.6".to_owned(),
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: vec![InspectionFix {
                id: "go.update:a@v1.2.6".to_owned(),
                title: "Update".to_owned(),
                kind: FixKind::Apply,
                batch: Some("go.update".to_owned()),
                label: None,
                inline: false,
                args: serde_json::json!({ "module": "a" }),
            }],
            source: "go.mod".to_owned(),
            code: "newerVersion".to_owned(),
            deprecated: false,
            unused: false,
        };
        let json = serde_json::to_value(&finding).unwrap();
        assert_eq!(json["severity"], "weakWarning");
        assert_eq!(json["fixes"][0]["kind"], "apply");
        assert!(json.get("rows").is_none() && json.get("details").is_none());
        assert!(json.get("note").is_none() && json.get("deprecated").is_none() && json.get("unused").is_none());
        assert_eq!(serde_json::from_value::<Finding>(json).unwrap(), finding);
    }
}
