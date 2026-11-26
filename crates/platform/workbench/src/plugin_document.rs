use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use zen_core::{Command, Session};
use zen_i18n::t;
use zen_run::output::SharedOutput;

use crate::virtual_documents::{self, VirtualDocument, VirtualKind};

pub const MAX_DOCUMENT_MIB: usize = 16;

pub const MAX_DOCUMENT: usize = MAX_DOCUMENT_MIB << 20;

pub const FOLDER: &str = "run-output";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub file: String,
    pub command: String,
    pub document_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentResult {
    pub run_id: u64,
    pub asked: Asked,
    pub text: String,
}

#[derive(Clone, Default)]
pub struct PluginDocuments {
    ready: Arc<Mutex<Vec<DocumentResult>>>,
}

fn safe_name(text: &str) -> String {
    let name: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' })
        .take(80)
        .collect();
    let name = name.trim_matches(['.', ' ']).to_owned();
    if name.is_empty() { "output".to_owned() } else { name }
}

pub fn relative_path(asked: &Asked) -> PathBuf {
    let key = virtual_documents::hashed(&format!("{}\n{}", asked.file, asked.command));
    let name = format!(
        "{}.{}",
        safe_name(&t!("workbench-document-title", command = asked.command.as_str())),
        asked.document_type
    );
    Path::new(FOLDER).join(key).join(name)
}

pub(crate) fn watch(session: &mut Session, run_id: u64, output: SharedOutput, asked: Asked) {
    output.update(|output| output.capture_stdout(MAX_DOCUMENT));
    let ready = Arc::clone(&session.state_mut::<PluginDocuments>().ready);
    std::thread::spawn(move || {
        output.wait_end(crate::plugin_diff::WAIT);
        let (exit, captured) = output.read(|output| (output.exit_code(), output.captured().cloned()));
        let Some(captured) = captured.filter(|_| exit == Some(0)) else { return };
        let file =
            relative_path(&asked).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        output.update(|output| {
            if captured.cut {
                output.system(&t!("workbench-document-cut", size = MAX_DOCUMENT_MIB));
            }
            output.system(&t!("workbench-document-opened", file = file.as_str()));
        });
        ready.lock().unwrap_or_else(PoisonError::into_inner).push(DocumentResult {
            run_id,
            asked,
            text: captured.text,
        });
    });
}

pub fn take(session: &mut Session) -> Vec<DocumentResult> {
    let Some(documents) = session.state::<PluginDocuments>() else { return Vec::new() };
    std::mem::take(&mut *documents.ready.lock().unwrap_or_else(PoisonError::into_inner))
}

pub fn open(session: &mut Session, result: &DocumentResult) -> Result<PathBuf, zen_core::CommandError> {
    let folder = virtual_documents::folder();
    let document = VirtualDocument {
        kind: VirtualKind::Output,
        origin: result.asked.command.clone(),
        library: None,
        bytecode: None,
        class_path: None,
    };
    let path = virtual_documents::store(&folder, &relative_path(&result.asked), &result.text, document, None)
        .map_err(|error| zen_core::CommandError::new(error.to_string()))?;
    if let Some(open) =
        session.state_mut::<crate::Workbench>().documents.iter_mut().find(|document| document.path() == path)
        && open.text() != result.text
    {
        open.reload_from_disk();
    }
    crate::OpenDocument
        .run(session, crate::OpenParams { path: path.clone(), editor_only: true, group: None, preview: false })?;
    Ok(path)
}

pub fn open_ready(session: &mut Session) -> Vec<PathBuf> {
    take(session).iter().filter_map(|result| open(session, result).ok()).collect()
}
