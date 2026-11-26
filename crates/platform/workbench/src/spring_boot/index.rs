use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use zen_core::Session;

use super::source::{self, Facts};

const RESCAN: Duration = Duration::from_secs(3);

type Stamp = (Option<SystemTime>, u64);

#[derive(Default)]
pub(crate) struct Shared {
    root: Option<PathBuf>,
    files: BTreeMap<PathBuf, (Stamp, Arc<Facts>)>,
    scanning: bool,
    scanned: Option<Instant>,
    generation: u64,
}

pub(crate) type Cell = Arc<(Mutex<Shared>, Condvar)>;

#[derive(Default)]
pub(crate) struct Index {
    cell: Cell,
    live: HashMap<PathBuf, (u64, Arc<Facts>)>,
}

fn lock(cell: &Cell) -> std::sync::MutexGuard<'_, Shared> {
    cell.0.lock().unwrap_or_else(PoisonError::into_inner)
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

fn scan(cell: &Cell, root: &Path) {
    let mut found = Vec::new();
    super::sources(root, &mut found, 0);
    let known: BTreeMap<PathBuf, (Stamp, Arc<Facts>)> = {
        let shared = lock(cell);
        if shared.root.as_deref() == Some(root) { shared.files.clone() } else { BTreeMap::new() }
    };
    let mut files = BTreeMap::new();
    let mut changed = known.len() != found.len();
    for path in found {
        let Some(stamp) = stamp(&path) else { continue };
        match known.get(&path) {
            Some((seen, facts)) if *seen == stamp => {
                files.insert(path, (stamp, Arc::clone(facts)));
            }
            _ => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let facts = Arc::new(source::read(&path, &text));
                changed |= known.get(&path).is_none_or(|(_, old)| **old != *facts);
                files.insert(path, (stamp, facts));
            }
        }
    }
    let mut shared = lock(cell);
    if shared.root.as_deref() == Some(root) {
        if changed || shared.files.len() != files.len() {
            shared.generation += 1;
        }
        shared.files = files;
        shared.scanning = false;
        shared.scanned = Some(Instant::now());
    }
    drop(shared);
    cell.1.notify_all();
}

fn cell_of(session: &mut Session) -> Option<(Cell, PathBuf)> {
    if !super::is_on(session) {
        return None;
    }
    let root = crate::run::project_root(session).ok()?;
    let cell = Arc::clone(&session.state_mut::<super::SpringBoot>().index.cell);
    let start = {
        let mut shared = lock(&cell);
        if shared.root.as_deref() != Some(root.as_path()) {
            *shared = Shared { root: Some(root.clone()), generation: shared.generation + 1, ..Shared::default() };
        }
        let due = shared.scanned.is_none_or(|at| at.elapsed() >= RESCAN);
        let start = due && !shared.scanning;
        if start {
            shared.scanning = true;
        }
        start
    };
    if start {
        let (work_cell, work_root) = (Arc::clone(&cell), root.clone());
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || scan(&work_cell, &work_root));
        #[cfg(target_arch = "wasm32")]
        scan(&work_cell, &work_root);
    }
    Some((cell, root))
}

pub(crate) struct Snapshot {
    pub files: Vec<(PathBuf, Arc<Facts>)>,
    pub generation: u64,
    pub scanned: bool,
}

pub(crate) fn snapshot(session: &mut Session, wait: Duration) -> Option<Snapshot> {
    let (cell, _) = cell_of(session)?;
    let deadline = Instant::now() + wait;
    let (mut files, generation, scanned) = {
        let mut shared = lock(&cell);
        while shared.scanning || shared.scanned.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            shared = cell.1.wait_timeout(shared, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        let files: BTreeMap<PathBuf, Arc<Facts>> =
            shared.files.iter().map(|(path, (_, facts))| (path.clone(), Arc::clone(facts))).collect();
        (files, shared.generation, shared.scanned.is_some())
    };
    let open: Vec<(PathBuf, u64, String)> = session
        .state::<crate::Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| source::is_source(document.path()) && document.image().is_none())
                .map(|document| (document.path().to_owned(), document.version(), document.text().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let index = &mut session.state_mut::<super::SpringBoot>().index;
    index.live.retain(|path, _| open.iter().any(|(open, _, _)| open == path));
    let mut live_generation = 0u64;
    for (path, version, text) in open {
        let facts = match index.live.get(&path) {
            Some((seen, facts)) if *seen == version => Arc::clone(facts),
            _ => {
                let facts = Arc::new(source::read(&path, &text));
                index.live.insert(path.clone(), (version, Arc::clone(&facts)));
                facts
            }
        };
        live_generation = live_generation.wrapping_mul(31).wrapping_add(version);
        files.insert(path, facts);
    }
    Some(Snapshot {
        files: files.into_iter().collect(),
        generation: generation.wrapping_mul(1_000_003).wrapping_add(live_generation),
        scanned,
    })
}

pub(crate) fn force_rescan(session: &mut Session) {
    let cell = Arc::clone(&session.state_mut::<super::SpringBoot>().index.cell);
    let mut shared = lock(&cell);
    if !shared.scanning {
        shared.scanned = None;
    }
}
