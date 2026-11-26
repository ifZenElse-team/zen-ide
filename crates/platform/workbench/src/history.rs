use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{Document, Workbench};

pub const LIMIT: usize = 100;

pub const PLACES_LIMIT: usize = 25;

pub const NEAR_LINES: u32 = 10;

/// A place in a file that navigation left or reached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    /// The file.
    pub path: PathBuf,
    /// The caret, a character offset in the file's text.
    pub offset: usize,
    /// The caret's line, one-based.
    pub line: u32,
}

impl Spot {
    pub fn at(document: &Document, offset: usize) -> Self {
        let text = document.text();
        let byte = crate::language::byte_offset(text, offset);
        Self {
            path: document.path().to_owned(),
            offset: crate::language::char_offset(text, byte),
            line: text[..byte].matches('\n').count() as u32 + 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub spot: Spot,
    pub time: u64,
    pub edited: bool,
}

#[derive(Debug, Default)]
pub struct History {
    back: Vec<Spot>,
    forward: Vec<Spot>,
    places: Vec<Place>,
    edited: Vec<PathBuf>,
    edits: Vec<Spot>,
    edit_at: Option<usize>,
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl History {
    pub fn record(&mut self, spot: Spot) {
        self.forward.clear();
        if self.back.last().is_some_and(|last| same_line(last, &spot)) {
            return;
        }
        self.back.push(spot);
        keep_limit(&mut self.back);
    }

    pub fn back(&self) -> &[Spot] {
        &self.back
    }

    pub fn forward(&self) -> &[Spot] {
        &self.forward
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    pub fn edited_files(&self) -> &[PathBuf] {
        &self.edited
    }

    pub fn edits(&self) -> &[Spot] {
        &self.edits
    }

    pub fn edit_at(&self) -> Option<usize> {
        self.edit_at
    }

    pub fn record_edit(&mut self, spot: Spot) {
        self.edit_at = None;
        if self.edits.last().is_some_and(|last| same_line(last, &spot)) {
            self.edits.pop();
        }
        self.edits.push(spot);
        keep_limit(&mut self.edits);
    }

    fn edit_target(&self, last: bool, current: Option<&Spot>) -> Option<usize> {
        if !last {
            return self.edit_at.map(|at| at + 1).filter(|next| *next < self.edits.len());
        }
        match self.edit_at {
            Some(at) => at.checked_sub(1),
            None => {
                let latest = self.edits.len().checked_sub(1)?;
                let here = current.is_some_and(|current| same_line(current, &self.edits[latest]));
                Some(if here && latest > 0 { latest - 1 } else { latest })
            }
        }
    }

    fn can_step_edit(&self, last: bool, reachable: impl Fn(&Spot) -> bool) -> bool {
        let indices = match (last, self.edit_at) {
            (true, None) => 0..self.edits.len(),
            (true, Some(at)) => 0..at,
            (false, Some(at)) => at + 1..self.edits.len(),
            (false, None) => 0..0,
        };
        self.edits[indices].iter().any(reachable)
    }

    fn forget_edit(&mut self, index: usize) {
        self.edits.remove(index);
        if let Some(at) = self.edit_at.filter(|at| *at > index) {
            self.edit_at = Some(at - 1);
        }
    }

    pub fn visit(&mut self, spot: Spot, time: u64, edited: bool) {
        let mut edited = edited;
        self.places.retain(|place| {
            let same = same_line(&place.spot, &spot);
            edited |= same && place.edited;
            !same
        });
        self.places.push(Place { spot, time, edited });
        for kind in [true, false] {
            let count = self.places.iter().filter(|place| place.edited == kind).count();
            if count > PLACES_LIMIT
                && let Some(oldest) = self.places.iter().position(|place| place.edited == kind)
            {
                self.places.remove(oldest);
            }
        }
    }

    pub fn remove(&mut self, path: &std::path::Path, line: u32) -> bool {
        let before = self.places.len() + self.back.len() + self.forward.len() + self.edits.len();
        let other = |spot: &Spot| spot.path != path || spot.line != line;
        self.places.retain(|place| other(&place.spot));
        self.back.retain(other);
        self.forward.retain(other);
        while let Some(index) = self.edits.iter().position(|spot| !other(spot)) {
            self.forget_edit(index);
        }
        before != self.places.len() + self.back.len() + self.forward.len() + self.edits.len()
    }

    pub fn relocate(&mut self, from: &std::path::Path, to: &std::path::Path) {
        let follow = |path: &mut PathBuf| {
            if let Ok(rest) = path.strip_prefix(from) {
                *path = if rest.as_os_str().is_empty() { to.to_owned() } else { to.join(rest) };
            }
        };
        for spot in self.spots_mut() {
            follow(&mut spot.path);
        }
        self.edited.iter_mut().for_each(follow);
    }

    fn take_edits(&mut self, workbench: &mut Workbench) {
        let mut taken: Vec<(u64, Spot, u64)> = Vec::new();
        for document in &mut workbench.documents {
            for (offset, time, order) in document.take_edit_places() {
                taken.push((order, Spot::at(document, offset), time));
            }
        }
        taken.sort_by_key(|(order, _, _)| *order);
        for (_, spot, time) in taken {
            self.visit(spot.clone(), time, true);
            self.edited.retain(|edited| *edited != spot.path);
            self.edited.insert(0, spot.path.clone());
            self.record_edit(spot);
        }
    }

    fn take(&mut self, back: bool, current: Option<&Spot>) -> Option<Spot> {
        let stack = if back { &mut self.back } else { &mut self.forward };
        while let Some(spot) = stack.pop() {
            if current.is_none_or(|current| !same_line(current, &spot)) {
                return Some(spot);
            }
        }
        None
    }

    fn leave(&mut self, back: bool, spot: Spot) {
        let stack = if back { &mut self.forward } else { &mut self.back };
        if stack.last().is_none_or(|last| !same_line(last, &spot)) {
            stack.push(spot);
            keep_limit(stack);
        }
    }

    fn spots_mut(&mut self) -> impl Iterator<Item = &mut Spot> {
        self.back
            .iter_mut()
            .chain(self.forward.iter_mut())
            .chain(self.places.iter_mut().map(|place| &mut place.spot))
            .chain(self.edits.iter_mut())
    }

    fn pull(&mut self, workbench: &Workbench) {
        for document in workbench.documents().iter().filter(|document| document.image().is_none()) {
            let count = self.spots_mut().filter(|spot| spot.path == document.path()).count();
            if count == 0 || document.marks.len() != count {
                continue;
            }
            let mut marks = document.marks.iter();
            for spot in self.spots_mut().filter(|spot| spot.path == document.path()) {
                if let Some(offset) = marks.next() {
                    *spot = Spot::at(document, *offset);
                }
            }
        }
    }

    fn push(&self, workbench: &mut Workbench) {
        for document in &mut workbench.documents {
            document.marks = self
                .back
                .iter()
                .chain(&self.forward)
                .chain(self.places.iter().map(|place| &place.spot))
                .chain(&self.edits)
                .filter(|spot| spot.path == document.path)
                .map(|spot| spot.offset)
                .collect();
        }
    }
}

fn same_line(a: &Spot, b: &Spot) -> bool {
    a.path == b.path && a.line == b.line
}

fn keep_limit(stack: &mut Vec<Spot>) {
    if stack.len() > LIMIT {
        let excess = stack.len() - LIMIT;
        stack.drain(..excess);
    }
}

pub(crate) fn with_history<R>(session: &mut Session, f: impl FnOnce(&mut History, &mut Session) -> R) -> R {
    let mut history = std::mem::take(session.state_mut::<History>());
    history.pull(session.state_mut::<Workbench>());
    history.take_edits(session.state_mut::<Workbench>());
    let result = f(&mut history, session);
    history.push(session.state_mut::<Workbench>());
    session.set_state(history);
    result
}

pub(crate) fn caret(session: &Session) -> Option<Spot> {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| document.image().is_none())
        .map(|document| Spot::at(document, document.selections().primary().head))
}

fn visited(history: &mut History, origin: Option<Spot>, target: Option<Spot>) {
    let time = now();
    for spot in origin.into_iter().chain(target) {
        history.visit(spot, time, false);
    }
}

pub(crate) fn left_file(session: &mut Session, origin: Option<Spot>) {
    let active =
        session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned());
    let target = caret(session);
    if origin.as_ref().is_some_and(|origin| active.as_deref() == Some(origin.path.as_path())) {
        with_history(session, |_, _| ());
        return;
    }
    with_history(session, |history, _| {
        if let Some(origin) = &origin {
            history.record(origin.clone());
        }
        visited(history, origin, target);
    });
}

pub(crate) fn jumped(session: &mut Session, origin: Option<Spot>) {
    let Some(origin) = origin else { return };
    let Some(target) = caret(session) else { return };
    if target.path == origin.path && target.line.abs_diff(origin.line) > NEAR_LINES {
        with_history(session, |history, _| {
            history.record(origin.clone());
            visited(history, Some(origin), Some(target));
        });
    }
}

pub(crate) fn closing(session: &mut Session) {
    with_history(session, |_, _| ());
}

/// The parameters of `navigate.place.goTo`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigateToParams {
    /// The file, an absolute path.
    pub path: PathBuf,
    /// Where the caret goes, a character offset in the file's text.
    pub offset: usize,
    /// The end of a range to select from `offset`, a character offset; none by default.
    #[serde(default)]
    pub end: Option<usize>,
}

pub struct NavigateTo;

impl Command for NavigateTo {
    type Params = NavigateToParams;
    type Output = Spot;
    const ID: &'static str = "navigate.place.goTo";
    const TITLE: &'static str = "Navigate To";
    const DESCRIPTION: &'static str = "Opens a file (or switches to its tab), puts the caret at a character offset, optionally \
         selecting up to `end`, scrolls it into view and records the place left for Back. Returns where it went.";

    fn run(&self, session: &mut Session, params: NavigateToParams) -> Result<Spot, CommandError> {
        let origin = caret(session);
        crate::open_document(session, &params.path)?;
        let target = place_caret(session, params.offset, params.end)?;
        reached(session, origin, &target);
        Ok(target)
    }
}

fn reached(session: &mut Session, origin: Option<Spot>, target: &Spot) {
    with_history(session, |history, _| {
        let origin = origin.filter(|origin| !same_line(origin, target));
        if let Some(origin) = &origin {
            history.record(origin.clone());
        }
        visited(history, origin, Some(target.clone()));
    });
}

pub(crate) fn go_to_line(session: &mut Session, path: &std::path::Path, line: u32) -> Result<Spot, CommandError> {
    let origin = caret(session);
    crate::open_document(session, path)?;
    let offset = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .and_then(|document| document.snapshot().point_to_offset(zen_text::Point::new(line as usize, 0)).ok())
        .unwrap_or_default();
    let target = place_caret(session, offset, None)?;
    reached(session, origin, &target);
    Ok(target)
}

pub(crate) fn go_to_point(
    session: &mut Session,
    path: &std::path::Path,
    line: usize,
    column: usize,
) -> Result<Spot, CommandError> {
    let origin = caret(session);
    crate::open_document(session, path)?;
    let offset = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .map(|document| {
            let text = document.snapshot();
            let line = line.min(text.len_lines().saturating_sub(1));
            text.point_to_offset(zen_text::Point::new(line, column))
                .or_else(|_| text.point_to_offset(zen_text::Point::new(line + 1, 0)).map(|next| next.saturating_sub(1)))
                .unwrap_or_else(|_| text.len_chars())
        })
        .unwrap_or_default();
    let target = place_caret(session, offset, None)?;
    reached(session, origin, &target);
    Ok(target)
}

pub(crate) fn go_to_file(session: &mut Session, path: &std::path::Path) -> Result<Spot, CommandError> {
    let origin = caret(session);
    crate::open_document(session, path)?;
    let target = caret(session).unwrap_or_else(|| Spot { path: path.to_owned(), offset: 0, line: 1 });
    reached(session, origin, &target);
    Ok(target)
}

pub(crate) fn place_caret(session: &mut Session, offset: usize, end: Option<usize>) -> Result<Spot, CommandError> {
    let document = session
        .state_mut::<Workbench>()
        .active_document_mut()
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let length = document.snapshot().len_chars();
    let offset = offset.min(length);
    let selection = match end.map(|end| end.min(length)) {
        Some(end) if end != offset => zen_editor::Selection::new(offset, end),
        _ => zen_editor::Selection::caret(offset),
    };
    document.set_selections(zen_editor::Selections::single(selection));
    let target = Spot::at(document, offset);
    if document.image().is_none() {
        crate::folding::RevealCarets.run(session, NoParams {})?;
    }
    Ok(target)
}

/// What `navigate.history.list` returns.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryList {
    /// The places Back goes to, the next one last.
    pub back: Vec<Spot>,
    /// The places Forward goes to, the next one last.
    pub forward: Vec<Spot>,
}

pub struct Step<const BACK: bool>;

impl<const BACK: bool> Command for Step<BACK> {
    type Params = NoParams;
    type Output = Option<Spot>;
    const ID: &'static str = if BACK { "navigate.history.back" } else { "navigate.history.forward" };
    const TITLE: &'static str = if BACK { "Back" } else { "Forward" };
    const DESCRIPTION: &'static str = if BACK {
        "Goes back to the place navigation last jumped from, across files; returns it."
    } else {
        "Goes forward to the place Back last left; returns it."
    };

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<History>()
            .is_some_and(|history| if BACK { !history.back.is_empty() } else { !history.forward.is_empty() })
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<Spot>, CommandError> {
        let current = caret(session);
        with_history(session, |history, session| {
            while let Some(spot) = history.take(BACK, current.as_ref()) {
                if crate::open_document(session, &spot.path).is_err() {
                    continue;
                }
                let target = place_caret(session, spot.offset, None)?;
                if let Some(current) = current.clone() {
                    history.leave(BACK, current);
                }
                visited(history, current, Some(target.clone()));
                return Ok(Some(target));
            }
            Ok(None)
        })
    }
}

pub struct EditStep<const LAST: bool>;

impl<const LAST: bool> Command for EditStep<LAST> {
    type Params = NoParams;
    type Output = Option<Spot>;
    const ID: &'static str = if LAST { "navigate.edit.last" } else { "navigate.edit.next" };
    const TITLE: &'static str = if LAST { "Last Edit Location" } else { "Next Edit Location" };
    const DESCRIPTION: &'static str = if LAST {
        "Goes to the place the text was last edited, across files; run again, to the edit place before it. Records \
         the place left for Back; returns where it went."
    } else {
        "Goes to the edit place after the one Last Edit Location went to, back toward the latest edit; returns it."
    };

    fn is_available(&self, session: &Session) -> bool {
        let Some(workbench) = session.state::<Workbench>() else { return false };
        if workbench.documents.iter().any(|document| !document.edit_places.is_empty()) {
            return LAST;
        }
        let reachable =
            |spot: &Spot| spot.path.exists() || workbench.documents.iter().any(|document| document.path == spot.path);
        session.state::<History>().is_some_and(|history| history.can_step_edit(LAST, reachable))
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<Spot>, CommandError> {
        let current = caret(session);
        with_history(session, |history, session| {
            while let Some(index) = history.edit_target(LAST, current.as_ref()) {
                let spot = history.edits[index].clone();
                if crate::open_document(session, &spot.path).is_err() {
                    history.forget_edit(index);
                    continue;
                }
                let target = place_caret(session, spot.offset, None)?;
                history.edit_at = Some(index);
                let origin = current.clone().filter(|origin| !same_line(origin, &target));
                if let Some(origin) = &origin {
                    history.record(origin.clone());
                }
                visited(history, origin, Some(target.clone()));
                return Ok(Some(target));
            }
            Ok(None)
        })
    }
}

/// What `navigate.edit.list` returns.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditList {
    /// The edit places, the latest last; one per line for a run of edits.
    pub places: Vec<Spot>,
    /// The index in `places` of the place Last or Next Edit Location went to last; none since the last edit.
    pub current: Option<usize>,
}

pub struct ListEdits;

impl Command for ListEdits {
    type Params = NoParams;
    type Output = EditList;
    const ID: &'static str = "navigate.edit.list";
    const TITLE: &'static str = "Edit Locations";
    const DESCRIPTION: &'static str = "The places Last Edit Location and Next Edit Location go through, the latest edit last, \
         with the index of the one gone to last.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditList, CommandError> {
        Ok(with_history(session, |history, _| EditList { places: history.edits.clone(), current: history.edit_at }))
    }
}

pub struct ListHistory;

impl Command for ListHistory {
    type Params = NoParams;
    type Output = HistoryList;
    const ID: &'static str = "navigate.history.list";
    const TITLE: &'static str = "Navigation History";
    const DESCRIPTION: &'static str = "The places Back and Forward go to, the next one last in each list.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryList, CommandError> {
        Ok(with_history(session, |history, _| HistoryList {
            back: history.back.clone(),
            forward: history.forward.clone(),
        }))
    }
}

pub struct ClearHistory;

impl Command for ClearHistory {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "navigate.history.clear";
    const TITLE: &'static str = "Clear Navigation History";
    const DESCRIPTION: &'static str =
        "Forgets the places Back and Forward go to, Recent Locations and the edit places, as when a project opens.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        with_history(session, |history, _| *history = History::default());
        Ok(())
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(NavigateTo)?;
    registry.register(Step::<true>)?;
    registry.register(Step::<false>)?;
    registry.register(EditStep::<true>)?;
    registry.register(EditStep::<false>)?;
    registry.register(ListEdits)?;
    registry.register(ListHistory)?;
    registry.register(ClearHistory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;

    fn spot(path: &str, offset: usize, line: u32) -> Spot {
        Spot { path: PathBuf::from(path), offset, line }
    }

    #[test]
    fn record_skips_the_same_line_clears_forward_and_keeps_the_limit() {
        let mut history = History::default();
        history.record(spot("/p/a.go", 3, 1));
        history.record(spot("/p/a.go", 9, 1));
        assert_eq!(history.back(), &[spot("/p/a.go", 3, 1)]);
        history.record(spot("/p/b.go", 3, 1));
        assert_eq!(history.back().len(), 2, "the same line of another file is another place");
        history.forward.push(spot("/p/c.go", 0, 1));
        history.record(spot("/p/a.go", 40, 5));
        assert!(history.forward().is_empty());
        for line in 0..150 {
            history.record(spot("/p/d.go", line as usize * 10, line + 10));
        }
        assert_eq!(history.back().len(), LIMIT);
        assert_eq!(history.back().last(), Some(&spot("/p/d.go", 1490, 159)));
        assert_eq!(history.back()[0], spot("/p/d.go", 500, 60));
    }

    #[test]
    fn recent_locations_keep_one_place_per_line_and_a_limit_of_each_kind() {
        let mut history = History::default();
        history.visit(spot("/p/a.go", 3, 1), 10, true);
        history.visit(spot("/p/a.go", 9, 1), 20, false);
        assert_eq!(history.places(), &[Place { spot: spot("/p/a.go", 9, 1), time: 20, edited: true }], "still edited");
        for line in 0..40 {
            history.visit(spot("/p/b.go", line as usize * 10, line + 10), 30, false);
        }
        assert_eq!(history.places().len(), PLACES_LIMIT + 1, "the edited place stays");
        assert!(history.places()[0].edited);
        assert_eq!(history.places().last().unwrap().spot.line, 49);
        history.record(spot("/p/b.go", 0, 49));
        assert!(history.remove(Path::new("/p/b.go"), 49));
        assert!(history.back().is_empty() && history.places().iter().all(|place| place.spot.line != 49));
    }

    #[test]
    fn edits_are_places_that_move_with_the_text() {
        let mut f = fixture();
        f.open_at(&f.a.clone(), 0);
        let in_a = A.find("return").unwrap();
        f.run("editor.selection.set", json!({ "selections": [{ "head": in_a }] }));
        f.run("editor.edit.insert", json!({ "text": "x" }));
        f.run("editor.edit.insert", json!({ "text": "y" }));
        f.open_at(&f.b.clone(), 0);
        let workbench = f.session.state_mut::<Workbench>();
        let a = workbench.documents.iter_mut().find(|document| document.path() == f.a).unwrap();
        assert!(a.replace_ranges(vec![(0..0, "// новое\n".to_owned())]));
        let places = with_history(&mut f.session, |history, _| history.places().to_vec());
        let edited: Vec<&Place> = places.iter().filter(|place| place.edited).collect();
        assert_eq!(edited.len(), 2, "the typing on one line is one place, the new line another: {places:?}");
        let typed = edited.iter().find(|place| place.spot.line == 5).expect("moved down a line");
        assert_eq!(typed.spot.offset, in_a + 2 + 9);
        assert_eq!(with_history(&mut f.session, |history, _| history.edited_files().to_vec()), [f.a.clone()]);
    }

    #[test]
    fn take_skips_the_current_line_and_leave_skips_a_repeat() {
        let mut history = History::default();
        history.record(spot("/p/a.go", 3, 1));
        history.record(spot("/p/b.go", 7, 2));
        assert_eq!(history.take(true, Some(&spot("/p/b.go", 9, 2))), Some(spot("/p/a.go", 3, 1)));
        assert!(history.back().is_empty());
        history.leave(true, spot("/p/b.go", 9, 2));
        history.leave(true, spot("/p/b.go", 11, 2));
        assert_eq!(history.forward(), &[spot("/p/b.go", 9, 2)]);
        assert_eq!(history.take(false, None), Some(spot("/p/b.go", 9, 2)));
        assert_eq!(history.take(false, None), None);
    }

    struct Fixture {
        registry: Registry,
        session: Session,
        a: PathBuf,
        b: PathBuf,
        _dir: tempfile::TempDir,
    }

    const A: &str = "package a\n\nfunc A() {\n\treturn\n}\n";
    const B: &str = "package b\n\n// привет 😀\nfunc B() {}\n";

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.go"), dir.path().join("b.go"));
        std::fs::write(&a, A).unwrap();
        std::fs::write(&b, B).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        Fixture { registry, session, a, b, _dir: dir }
    }

    impl Fixture {
        fn run(&mut self, command: &str, params: serde_json::Value) -> serde_json::Value {
            self.registry.dispatch(&mut self.session, command, params).unwrap()
        }

        fn open_at(&mut self, path: &Path, offset: usize) {
            self.run("file.document.open", json!({ "path": path }));
            let document = self.session.state_mut::<Workbench>().active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(offset)));
        }

        fn caret(&self) -> Spot {
            caret(&self.session).unwrap()
        }

        fn available(&self, command: &str) -> bool {
            self.registry.is_available(&self.session, command)
        }
    }

    #[test]
    fn back_and_forward_go_across_files() {
        let mut f = fixture();
        assert!(!f.available("navigate.history.back") && !f.available("navigate.history.forward"));
        assert!(f.registry.dispatch(&mut f.session, "navigate.history.back", json!(null)).is_err());
        let in_a = A.find("return").unwrap();
        f.open_at(&f.a.clone(), in_a);
        let in_b = B.chars().count() - 3;
        f.open_at(&f.b.clone(), in_b);
        assert!(f.available("navigate.history.back") && !f.available("navigate.history.forward"));

        let went = f.run("navigate.history.back", json!(null));
        assert_eq!(went, json!({ "path": f.a, "offset": in_a, "line": 4 }));
        assert_eq!(f.caret(), spot(f.a.to_str().unwrap(), in_a, 4));
        assert!(!f.available("navigate.history.back") && f.available("navigate.history.forward"));

        let went = f.run("navigate.history.forward", json!(null));
        assert_eq!(went, json!({ "path": f.b, "offset": in_b, "line": 4 }));
        assert_eq!(f.caret().path, f.b);
        let list = f.run("navigate.history.list", json!(null));
        assert_eq!(list, json!({ "back": [{ "path": f.a, "offset": in_a, "line": 4 }], "forward": [] }));
        f.run("navigate.history.clear", json!(null));
        assert!(!f.available("navigate.history.back"));
    }

    #[test]
    fn a_new_jump_clears_forward() {
        let mut f = fixture();
        f.open_at(&f.a.clone(), 0);
        f.open_at(&f.b.clone(), 0);
        f.run("navigate.history.back", json!(null));
        assert!(f.available("navigate.history.forward"));
        f.run("navigate.place.goTo", json!({ "path": f.a, "offset": A.len() - 2 }));
        assert!(!f.available("navigate.history.forward"));
        assert_eq!(session_history(&f).back(), &[spot(f.a.to_str().unwrap(), 0, 1)]);
    }

    fn session_history(f: &Fixture) -> &History {
        f.session.state::<History>().unwrap()
    }

    #[test]
    fn back_reopens_a_closed_file_and_skips_a_deleted_one() {
        let mut f = fixture();
        let c = f.a.with_file_name("c.go");
        std::fs::write(&c, "package c\n").unwrap();
        f.open_at(&c, 4);
        f.open_at(&f.a.clone(), 5);
        f.open_at(&f.b.clone(), 0);
        f.run("file.document.close", json!({ "index": 1 }));
        f.run("file.document.close", json!({ "index": 0 }));
        std::fs::remove_file(&c).unwrap();
        assert_eq!(session_history(&f).back().len(), 2, "closing another tab records nothing");

        let went = f.run("navigate.history.back", json!(null));
        assert_eq!(went, json!({ "path": f.a, "offset": 5, "line": 1 }));
        assert_eq!(session_history(&f).back().len(), 1);
        assert_eq!(f.run("navigate.history.back", json!(null)), json!(null), "a deleted file is dropped");
        assert!(!f.available("navigate.history.back"));
    }

    #[test]
    fn closing_the_active_tab_records_its_caret() {
        let mut f = fixture();
        f.open_at(&f.a.clone(), 0);
        f.open_at(&f.b.clone(), 12);
        f.run("file.document.close", json!({}));
        assert_eq!(f.caret().path, f.a);
        let went = f.run("navigate.history.back", json!(null));
        assert_eq!(went, json!({ "path": f.b, "offset": 12, "line": 3 }));
        assert_eq!(session_history(&f).forward(), &[spot(f.a.to_str().unwrap(), 0, 1)]);
    }

    #[test]
    fn places_move_with_edits_and_stay_within_the_text() {
        let mut f = fixture();
        let in_a = A.find("return").unwrap();
        f.open_at(&f.a.clone(), in_a);
        f.open_at(&f.b.clone(), 0);
        let workbench = f.session.state_mut::<Workbench>();
        let a = workbench.documents.iter_mut().find(|document| document.path() == f.a).unwrap();
        assert!(a.replace_ranges(vec![(0..0, "// новое\n".to_owned())]));
        let list = f.run("navigate.history.list", json!(null));
        assert_eq!(list["back"][0], json!({ "path": f.a, "offset": in_a + 9, "line": 5 }));

        let workbench = f.session.state_mut::<Workbench>();
        let a = workbench.documents.iter_mut().find(|document| document.path() == f.a).unwrap();
        let length = a.snapshot().len_chars();
        assert!(a.replace_ranges(vec![(3..length, "x".to_owned())]));
        let went = f.run("navigate.history.back", json!(null));
        assert_eq!(went, json!({ "path": f.a, "offset": 4, "line": 1 }));
    }

    #[test]
    fn far_caret_jumps_in_a_file_are_recorded_and_near_ones_are_not() {
        let mut f = fixture();
        let long = f.a.with_file_name("long.go");
        std::fs::write(&long, "x\n".repeat(60)).unwrap();
        f.open_at(&long, 0);
        f.run("editor.caret.goTo", json!({ "line": 1 + NEAR_LINES }));
        assert!(session_history(&f).back().is_empty());
        f.run("editor.caret.goTo", json!({ "line": 40 }));
        assert_eq!(session_history(&f).back(), &[spot(long.to_str().unwrap(), 2 * NEAR_LINES as usize, 11)]);
        let went = f.run("navigate.history.back", json!(null));
        assert_eq!(went["line"], json!(11));
        assert_eq!(session_history(&f).forward(), &[spot(long.to_str().unwrap(), 78, 40)]);
    }

    #[test]
    fn switching_tabs_records_the_place_left() {
        let mut f = fixture();
        f.open_at(&f.a.clone(), 3);
        f.open_at(&f.b.clone(), 0);
        f.run("editor.tab.select", json!({ "index": 1 }));
        assert_eq!(session_history(&f).back().len(), 1, "the active tab again is no change");
        f.run("editor.tab.select", json!({ "index": 0 }));
        assert_eq!(session_history(&f).back().last(), Some(&spot(f.b.to_str().unwrap(), 0, 1)));
    }

    #[test]
    fn last_and_next_edit_location_walk_the_edit_places() {
        let mut f = fixture();
        assert!(!f.available("navigate.edit.last") && !f.available("navigate.edit.next"));
        let in_a = A.find("return").unwrap();
        f.open_at(&f.a.clone(), in_a);
        f.run("editor.edit.insert", json!({ "text": "x" }));
        f.run("editor.edit.insert", json!({ "text": "y" }));
        assert!(f.available("navigate.edit.last") && !f.available("navigate.edit.next"));
        f.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
        f.run("editor.edit.insert", json!({ "text": "z" }));
        f.open_at(&f.b.clone(), 0);
        f.run("editor.edit.insert", json!({ "text": "w" }));
        let (path_a, path_b) = (f.a.clone(), f.b.clone());
        let a = |offset: usize, line: u32| json!({ "path": path_a, "offset": offset, "line": line });
        let b = json!({ "path": path_b, "offset": 1, "line": 1 });
        let list = f.run("navigate.edit.list", json!(null));
        assert_eq!(list, json!({ "places": [a(in_a + 3, 4), a(1, 1), b], "current": null }), "moved by the later edit");

        assert_eq!(f.run("navigate.edit.last", json!(null)), a(1, 1), "the caret is on the latest place");
        assert_eq!(session_history(&f).back().last(), Some(&spot(f.b.to_str().unwrap(), 1, 1)), "recorded for Back");
        assert!(f.available("navigate.edit.next"));
        assert_eq!(f.run("navigate.edit.next", json!(null)), b);
        assert!(!f.available("navigate.edit.next"));
        f.run("navigate.edit.last", json!(null));
        assert_eq!(f.run("navigate.edit.last", json!(null)), a(in_a + 3, 4));
        assert_eq!(f.caret(), spot(f.a.to_str().unwrap(), in_a + 3, 4));
        assert!(!f.available("navigate.edit.last") && f.available("navigate.edit.next"));
        assert_eq!(f.run("navigate.edit.list", json!(null))["current"], json!(0));

        f.run("editor.edit.insert", json!({ "text": "v" }));
        assert!(f.available("navigate.edit.last") && !f.available("navigate.edit.next"), "an edit starts again");
        let list = f.run("navigate.edit.list", json!(null));
        assert_eq!(list["places"].as_array().unwrap().len(), 4, "the first place's line again, in a new run");
        assert_eq!(list["places"][3], a(in_a + 4, 4));
        f.run("navigate.history.clear", json!(null));
        assert!(!f.available("navigate.edit.last"));
    }

    #[test]
    fn last_edit_location_reopens_a_closed_file_and_skips_a_deleted_one() {
        let mut f = fixture();
        let c = f.a.with_file_name("c.go");
        std::fs::write(&c, "package c\n").unwrap();
        f.open_at(&f.a.clone(), 0);
        f.run("editor.edit.insert", json!({ "text": "2" }));
        f.run("file.document.save", json!({}));
        f.open_at(&c, 0);
        f.run("editor.edit.insert", json!({ "text": "1" }));
        f.run("file.document.save", json!({}));
        f.open_at(&f.b.clone(), 0);
        f.run("file.document.close", json!({ "index": 1 }));
        f.run("file.document.close", json!({ "index": 0 }));
        std::fs::remove_file(&c).unwrap();
        assert_eq!(session_history(&f).edits().len(), 2);
        assert!(f.available("navigate.edit.last"));

        let went = f.run("navigate.edit.last", json!(null));
        assert_eq!(went, json!({ "path": f.a, "offset": 1, "line": 1 }), "the deleted file is skipped");
        assert_eq!(f.caret().path, f.a, "the closed file opens again");
        assert_eq!(session_history(&f).edits(), &[spot(f.a.to_str().unwrap(), 1, 1)]);
        assert!(!f.available("navigate.edit.last") && !f.available("navigate.edit.next"));
    }

    #[test]
    fn edit_places_keep_the_order_of_the_edits_across_files() {
        let mut f = fixture();
        f.open_at(&f.b.clone(), 0);
        f.open_at(&f.a.clone(), 0);
        f.run("editor.edit.insert", json!({ "text": "1" }));
        f.run("editor.text.replace", json!({ "path": f.b, "oldText": "func B", "newText": "func BB" }));
        let in_a = A.find("return").unwrap() + 1;
        f.run("editor.selection.set", json!({ "selections": [{ "head": in_a }] }));
        f.run("editor.edit.insert", json!({ "text": "2" }));
        let list = f.run("navigate.edit.list", json!(null));
        let lines: Vec<(String, u64)> = list["places"]
            .as_array()
            .unwrap()
            .iter()
            .map(|place| (place["path"].as_str().unwrap().to_owned(), place["line"].as_u64().unwrap()))
            .collect();
        let (a, b) = (f.a.to_str().unwrap().to_owned(), f.b.to_str().unwrap().to_owned());
        assert_eq!(lines, [(a.clone(), 1), (b, 4), (a, 4)]);
    }

    #[test]
    fn edit_places_follow_a_renamed_file_and_leave_a_removed_line() {
        let mut f = fixture();
        f.open_at(&f.a.clone(), 0);
        f.run("editor.edit.insert", json!({ "text": "1" }));
        f.open_at(&f.b.clone(), 0);
        f.run("editor.edit.insert", json!({ "text": "2" }));
        let renamed = f.run("file.fs.rename", json!({ "path": f.a, "name": "c.go" }));
        let renamed = PathBuf::from(renamed.as_str().unwrap());
        let list = f.run("navigate.edit.list", json!(null));
        assert_eq!(list["places"][0], json!({ "path": renamed, "offset": 1, "line": 1 }));
        assert_eq!(f.run("navigate.edit.last", json!(null)), json!({ "path": renamed, "offset": 1, "line": 1 }));
        assert_eq!(f.run("navigate.history.back", json!(null))["path"], json!(f.b), "Back follows it too");

        f.run("navigate.recent.removeLocation", json!({ "path": renamed, "line": 1 }));
        assert_eq!(session_history(&f).edits(), &[spot(f.b.to_str().unwrap(), 1, 1)]);
    }

    #[test]
    fn edit_places_keep_the_last_hundred() {
        let mut history = History::default();
        for line in 1..=LIMIT as u32 + 5 {
            history.record_edit(spot("/a.go", line as usize * 10, line));
        }
        assert_eq!(history.edits().len(), LIMIT);
        assert_eq!(history.edits()[0].line, 6);
        assert_eq!(history.edits().last().unwrap().line, LIMIT as u32 + 5);

        let mut f = fixture();
        let many = f.a.with_file_name("many.go");
        std::fs::write(&many, "//\n".repeat(LIMIT + 10)).unwrap();
        f.open_at(&many, 0);
        for line in 0..LIMIT + 5 {
            f.run("editor.selection.set", json!({ "selections": [{ "head": { "line": line, "column": 0 } }] }));
            f.run("editor.edit.insert", json!({ "text": "x" }));
        }
        let list = f.run("navigate.edit.list", json!(null));
        let places = list["places"].as_array().unwrap();
        assert_eq!(places.len(), LIMIT, "edits in one file without a jump between them");
        assert_eq!(places[0]["line"], json!(6));
    }

    #[test]
    fn navigate_to_opens_the_file_moves_the_caret_and_records_the_origin() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.go"), dir.path().join("b.go"));
        std::fs::write(&a, "package a\n\nfunc A() {}\n").unwrap();
        std::fs::write(&b, "package b\n\n// привет 😀\nfunc Бета() {}\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": a })).unwrap();
        let name = "package b\n\n// привет 😀\nfunc ".chars().count();
        let went = registry
            .dispatch(&mut session, "navigate.place.goTo", json!({ "path": b, "offset": name, "end": name + 4 }))
            .unwrap();
        assert_eq!(went, json!({ "path": b, "offset": name, "line": 4 }));
        let workbench = session.state::<Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        assert_eq!(document.path(), b);
        let selection = document.selections().primary();
        let chars: Vec<char> = document.text().chars().collect();
        assert_eq!(chars[selection.range()].iter().collect::<String>(), "Бета");
        assert_eq!(session.state::<History>().unwrap().back(), &[spot(a.to_str().unwrap(), 0, 1)]);

        registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": b, "offset": name + 2 })).unwrap();
        assert_eq!(session.state::<History>().unwrap().back().len(), 1, "a jump within the line is not recorded");
        assert_eq!(session.state::<Workbench>().unwrap().documents().len(), 2);
        assert!(
            registry
                .dispatch(
                    &mut session,
                    "navigate.place.goTo",
                    json!({ "path": dir.path().join("none.go"), "offset": 0 })
                )
                .is_err()
        );
    }
}
