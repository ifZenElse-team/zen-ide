use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use zen_core::{Command, Session};
use zen_i18n::t;

use crate::build_sync::{Severity, SyncFix, SyncMessage, jdk_fix};
use crate::jdk::commands::{ChooseJdk, DownloadJdk};
use crate::jdk::detect::read;
use crate::jdk::{Jdk, JdkSource, Jdks};
use crate::trust::GrantTrust;

const OFFERED_WITHOUT_A_LEVEL: u32 = 21;

/// Why a project does not load as it should.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    /// No program of the build tool is found.
    ToolNotFound,
    /// The person has not trusted the project (ADR-0205).
    NotTrusted,
    /// No JDK, or not the one the build asks for.
    JdkMismatch,
}

/// The one reason the tool window's banner gives: the first of tool not found, not trusted, JDK mismatch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBlock {
    pub kind: BlockKind,
    pub text: String,
    /// What the banner offers, as commands. `project.trust.grant` is the person's: agents are refused.
    pub fixes: Vec<SyncFix>,
}

impl LoadBlock {
    pub fn message(&self) -> SyncMessage {
        let severity = if self.kind == BlockKind::ToolNotFound { Severity::Error } else { Severity::Warning };
        SyncMessage::new(severity, self.text.clone()).with_fix(self.fixes.first().cloned())
    }
}

/// The JDK a build runs with against the one it asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildJdk {
    /// The feature release of the JDK it runs with; absent when none is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub major: Option<u32>,
    /// The feature release the build asks for, when its files say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wanted: Option<u32>,
    /// The build file that says so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wanted_by: Option<PathBuf>,
    /// Whether a person chose this JDK, for the project or in the tool's settings.
    pub chosen: bool,
    /// Whether it differs from what the build asks for. A newer JDK someone chose differs without a
    /// warning; a Gradle toolchain finds its own JDK and never differs.
    pub differs: bool,
    /// Whether the banner warns about it.
    pub warned: bool,
}

pub struct ToolCheck {
    pub found: bool,
    pub not_found: String,
    pub download: SyncFix,
    pub java_home: Option<PathBuf>,
    pub jdk_set_for_tool: bool,
}

pub struct Checked {
    pub jdk: BuildJdk,
    pub block: Option<LoadBlock>,
}

fn jdk_state(session: &mut Session, root: &Path, tool: &ToolCheck) -> (BuildJdk, Option<Jdk>) {
    let jdks = session.state_mut::<Jdks>().clone();
    let mut store = jdks.store();
    let level = store.build_level(root);
    let chosen = tool.jdk_set_for_tool || store.project(root).is_some_and(|project| project.chosen);
    let major = tool.java_home.as_deref().and_then(|home| read(home, JdkSource::Added)).map(|jdk| jdk.major);
    let wanted = level.as_ref().map(|level| level.level);
    let toolchain = level.as_ref().is_some_and(|level| level.toolchain);
    let (differs, warned) = match (wanted, major) {
        (Some(wanted), Some(major)) if !toolchain && wanted != major => (true, major < wanted || !chosen),
        _ => (false, false),
    };
    let matching = wanted.and_then(|wanted| store.jdk_of(wanted));
    let state = BuildJdk { major, wanted, wanted_by: level.map(|level| level.file), chosen, differs, warned };
    (state, matching)
}

fn jdk_fixes(root: &Path, jdk: &BuildJdk, installed: Option<Jdk>) -> Vec<SyncFix> {
    let wanted = jdk.wanted.unwrap_or(OFFERED_WITHOUT_A_LEVEL);
    let first = match installed {
        Some(found) => SyncFix {
            title: t!("workbench-block-use-jdk", level = wanted),
            command: ChooseJdk::ID.to_owned(),
            params: json!({ "home": found.home, "project": root }),
            reload: true,
        },
        None => SyncFix {
            title: t!("jdk-no-match-download", level = wanted),
            command: DownloadJdk::ID.to_owned(),
            params: json!({ "version": wanted }),
            reload: false,
        },
    };
    vec![first, jdk_fix()]
}

pub fn check(session: &mut Session, root: &Path, tool: ToolCheck) -> Checked {
    let (jdk, installed) = jdk_state(session, root, &tool);
    let block = if !tool.found {
        Some(LoadBlock { kind: BlockKind::ToolNotFound, text: tool.not_found, fixes: vec![tool.download] })
    } else if !crate::trust::trusted(session, root) {
        let fix = SyncFix {
            title: t!("workbench-trust-action"),
            command: GrantTrust::ID.to_owned(),
            params: json!({ "path": root }),
            reload: true,
        };
        Some(LoadBlock { kind: BlockKind::NotTrusted, text: t!("workbench-block-not-trusted"), fixes: vec![fix] })
    } else if tool.java_home.is_none() {
        let fixes = jdk_fixes(root, &jdk, installed);
        Some(LoadBlock { kind: BlockKind::JdkMismatch, text: t!("workbench-block-no-jdk"), fixes })
    } else if let (true, Some(wanted), Some(major)) = (jdk.warned, jdk.wanted, jdk.major) {
        let fixes = jdk_fixes(root, &jdk, installed);
        let text = t!("workbench-block-jdk", wanted = wanted, major = major);
        Some(LoadBlock { kind: BlockKind::JdkMismatch, text, fixes })
    } else {
        None
    };
    Checked { jdk, block }
}

pub fn note_in_sync(
    session: &mut Session,
    root: &Path,
    block: Option<&LoadBlock>,
    sync: &mut crate::build_sync::SyncRun,
) {
    crate::jdk::commands::offer_jdk(session, root);
    if let Some(block) = block.filter(|block| block.kind != BlockKind::ToolNotFound) {
        sync.step(t!("workbench-sync-check"), std::time::Instant::now(), vec![block.message()]);
    }
}

#[cfg(test)]
mod tests;
