use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zen_core::Session;

use super::Scratches;
use crate::Workbench;
use crate::project_model::ProjectModels;

#[derive(Default)]
pub(crate) struct ScratchHomes {
    root: Option<PathBuf>,
    revision: u64,
    placed: BTreeMap<PathBuf, u64>,
}

fn open_scratches(session: &Session) -> Vec<(PathBuf, u64)> {
    let folders = session.state::<Scratches>().map(Scratches::folders).unwrap_or_default();
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    let inside = |path: &Path| folders.iter().any(|folder| path.starts_with(folder));
    workbench
        .documents()
        .iter()
        .filter(|document| document.image().is_none() && inside(document.path()))
        .map(|document| (document.path().to_owned(), document.version()))
        .collect()
}

fn text_of(session: &Session, path: &Path) -> Option<String> {
    let workbench = session.state::<Workbench>()?;
    let document = workbench.documents().iter().find(|document| document.path() == path)?;
    Some(document.text().to_owned())
}

pub(crate) fn place(session: &mut Session) {
    let open = open_scratches(session);
    if open.is_empty() && session.state::<ScratchHomes>().is_none_or(|homes| homes.placed.is_empty()) {
        return;
    }
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let models = session.state_mut::<ProjectModels>();
    models.ensure(root.as_deref());
    let revision = models.revision();
    let homes = session.state_mut::<ScratchHomes>();
    let moved = homes.root != root || homes.revision != revision;
    homes.root.clone_from(&root);
    homes.revision = revision;
    homes.placed.retain(|path, _| {
        let kept = open.iter().any(|(open, _)| open == path);
        if !kept {
            zen_lsp::position::unmap_uri(path);
        }
        kept
    });
    let stale: Vec<(PathBuf, u64)> =
        open.into_iter().filter(|(path, version)| moved || homes.placed.get(path) != Some(version)).collect();
    if stale.is_empty() {
        return;
    }
    let context = root.as_deref().map(|root| crate::run::run_context(session, root));
    for (path, version) in stale {
        let text = text_of(session, &path).unwrap_or_default();
        let home = context.as_ref().and_then(|context| crate::run::scratch_home(session, &path, &text, context));
        match home {
            Some(home) => zen_lsp::position::map_uri(&zen_lsp::position::file_uri(&home), &path),
            None => zen_lsp::position::unmap_uri(&path),
        }
        session.state_mut::<ScratchHomes>().placed.insert(path, version);
    }
}
