use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zen_i18n::t;

use super::{Languages, Problem, Reason};
use crate::Workbench;

const OWNER: &str = "inspectCode";
const AT_ONCE: usize = 16;
const START_LIMIT: Duration = Duration::from_secs(120);
const FILE_LIMIT: Duration = Duration::from_secs(20);
const QUIET_FILE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub(crate) struct Sweep {
    waiting: VecDeque<PathBuf>,
    open: BTreeMap<PathBuf, Instant>,
    found: Vec<(PathBuf, Vec<Problem>)>,
    unchecked: Vec<(PathBuf, String)>,
    start_waited: Option<Instant>,
    quiet: bool,
    total: usize,
}

impl Sweep {
    pub(crate) fn new(paths: Vec<PathBuf>, languages: &mut Languages, workbench: &Workbench) -> Self {
        languages.sync(workbench);
        let waiting: VecDeque<PathBuf> = paths.into_iter().filter(|path| languages.serves(path)).collect();
        Self { total: waiting.len(), waiting, ..Self::default() }
    }

    pub(crate) fn left(&self) -> usize {
        self.waiting.len() + self.open.len()
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }

    pub(crate) fn take_found(&mut self) -> Vec<(PathBuf, Vec<Problem>)> {
        std::mem::take(&mut self.found)
    }

    pub(crate) fn take_unchecked(&mut self) -> Vec<(PathBuf, String)> {
        std::mem::take(&mut self.unchecked)
    }

    pub(crate) fn stop(&mut self, languages: &mut Languages, workbench: &Workbench) {
        self.waiting.clear();
        self.open.clear();
        languages.hold(OWNER, BTreeSet::new());
        languages.sync(workbench);
    }

    fn open_more(&mut self, languages: &mut Languages, workbench: &Workbench) {
        while self.open.len() < AT_ONCE {
            let Some(path) = self.waiting.pop_front() else { return };
            languages.start_for(&path);
            match languages.absence_for(&path) {
                Some(absence) if absence.reason == Reason::Starting => {
                    let since = *self.start_waited.get_or_insert_with(Instant::now);
                    if since.elapsed() < START_LIMIT {
                        self.waiting.push_front(path);
                        return;
                    }
                    self.unchecked.push((path, absence.message));
                }
                Some(absence) => self.unchecked.push((path, absence.message)),
                None => {
                    self.start_waited = None;
                    if !is_open(workbench, &path)
                        && let Err(error) = languages.lend(&path)
                    {
                        self.unchecked.push((path, error));
                        continue;
                    }
                    self.open.insert(path, Instant::now());
                }
            }
        }
    }

    fn collect(&mut self, languages: &mut Languages, workbench: &Workbench) {
        let indexing = languages.work_progress().is_some();
        let limit = if self.quiet { QUIET_FILE_LIMIT } else { FILE_LIMIT };
        let open: Vec<(PathBuf, Instant)> = self.open.iter().map(|(path, at)| (path.clone(), *at)).collect();
        for (path, since) in open {
            let version = document_version(workbench, &path);
            let waits = matches!(languages.analysis(&path, version), Some(Reason::Checking | Reason::NotChecked));
            let settle = languages.settle_ms(&path);
            if !waits && (indexing || since.elapsed() < Duration::from_millis(settle)) {
                if indexing {
                    self.open.insert(path, Instant::now());
                }
            } else if !waits {
                self.found.push((path.clone(), languages.problems(&path).to_vec()));
                self.open.remove(&path);
            } else if indexing {
                self.open.insert(path, Instant::now());
            } else if since.elapsed() > limit {
                self.quiet = true;
                self.unchecked.push((path.clone(), t!("workbench-inspect-no-answer")));
                self.open.remove(&path);
            }
        }
    }

    pub(crate) fn step(&mut self, languages: &mut Languages, workbench: &Workbench) -> bool {
        languages.sync(workbench);
        self.collect(languages, workbench);
        self.open_more(languages, workbench);
        languages.hold(OWNER, self.open.keys().cloned().collect());
        if self.left() == 0 {
            self.stop(languages, workbench);
            return true;
        }
        false
    }
}

fn is_open(workbench: &Workbench, path: &Path) -> bool {
    workbench.documents().iter().any(|document| document.path() == path)
}

fn document_version(workbench: &Workbench, path: &Path) -> Option<u64> {
    workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version)
}
