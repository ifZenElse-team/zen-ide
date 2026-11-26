use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TreeSelection {
    items: Vec<PathBuf>,
    anchor: Option<PathBuf>,
    lead: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Extend {
    pub(super) toggle: bool,
    pub(super) range: bool,
}

impl Extend {
    pub(super) fn from(modifiers: egui::Modifiers) -> Self {
        Self { toggle: modifiers.command, range: modifiers.shift }
    }
}

impl TreeSelection {
    pub(super) fn contains(&self, path: &Path) -> bool {
        self.items.iter().any(|item| item == path)
    }

    pub(super) fn items(&self, visible: &[PathBuf]) -> Vec<PathBuf> {
        let mut items = self.items.clone();
        items.sort_by_key(|item| visible.iter().position(|row| row == item).unwrap_or(usize::MAX));
        items
    }

    pub(super) fn lead(&self) -> Option<&PathBuf> {
        self.lead.as_ref()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(super) fn select(&mut self, path: PathBuf) {
        self.items = vec![path.clone()];
        self.anchor = Some(path.clone());
        self.lead = Some(path);
    }

    pub(super) fn select_all_of(&mut self, paths: Vec<PathBuf>) {
        self.anchor = paths.first().cloned();
        self.lead = paths.last().cloned();
        self.items = paths;
    }

    pub(super) fn click(&mut self, path: &Path, extend: Extend, visible: &[PathBuf]) {
        match (extend.range, extend.toggle) {
            (false, false) => self.select(path.to_owned()),
            (false, true) => {
                if self.contains(path) {
                    self.items.retain(|item| item != path);
                } else {
                    self.items.push(path.to_owned());
                }
                self.anchor = Some(path.to_owned());
                self.lead = Some(path.to_owned());
            }
            (true, add) => {
                let anchor = self.anchor.clone().unwrap_or_else(|| path.to_owned());
                let range = range(visible, &anchor, path);
                if add {
                    for row in range {
                        if !self.contains(&row) {
                            self.items.push(row);
                        }
                    }
                } else {
                    self.items = range;
                }
                self.anchor = Some(anchor);
                self.lead = Some(path.to_owned());
            }
        }
    }

    pub(super) fn step(&mut self, step: isize, extend: bool, visible: &[PathBuf]) {
        if visible.is_empty() {
            return;
        }
        let current = self.lead.as_ref().and_then(|lead| visible.iter().position(|row| row == lead));
        let next = match current {
            Some(index) => index.saturating_add_signed(step).min(visible.len() - 1),
            None if step < 0 => visible.len() - 1,
            None => 0,
        };
        let path = visible[next].clone();
        if extend {
            let anchor = self.anchor.clone().unwrap_or_else(|| path.clone());
            self.items = range(visible, &anchor, &path);
            self.anchor = Some(anchor);
            self.lead = Some(path);
        } else {
            self.select(path);
        }
    }

    pub(super) fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.items.retain(|item| keep(item));
        if self.anchor.as_deref().is_some_and(|anchor| !keep(anchor)) {
            self.anchor = self.items.first().cloned();
        }
        if self.lead.as_deref().is_some_and(|lead| !keep(lead)) {
            self.lead = self.items.last().cloned();
        }
    }
}

fn range(visible: &[PathBuf], from: &Path, to: &Path) -> Vec<PathBuf> {
    let position = |path: &Path| visible.iter().position(|row| row == path);
    match (position(from), position(to)) {
        (Some(a), Some(b)) => visible[a.min(b)..=a.max(b)].to_vec(),
        _ => vec![to.to_owned()],
    }
}

pub(super) fn outermost(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|path| !paths.iter().any(|other| other != *path && path.starts_with(other))).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<PathBuf> {
        ["/p", "/p/a", "/p/b", "/p/c", "/p/d", "/p/e"].iter().map(PathBuf::from).collect()
    }

    fn names(selection: &TreeSelection) -> Vec<String> {
        selection.items(&rows()).iter().map(|path| path.display().to_string()).collect()
    }

    const PLAIN: Extend = Extend { toggle: false, range: false };
    const COMMAND: Extend = Extend { toggle: true, range: false };
    const SHIFT: Extend = Extend { toggle: false, range: true };
    const COMMAND_SHIFT: Extend = Extend { toggle: true, range: true };

    #[test]
    fn click_selects_one() {
        let mut selection = TreeSelection::default();
        selection.click(Path::new("/p/b"), PLAIN, &rows());
        selection.click(Path::new("/p/d"), PLAIN, &rows());
        assert_eq!(names(&selection), ["/p/d"]);
        assert_eq!(selection.lead(), Some(&PathBuf::from("/p/d")));
    }

    #[test]
    fn command_click_toggles() {
        let mut selection = TreeSelection::default();
        selection.click(Path::new("/p/b"), PLAIN, &rows());
        selection.click(Path::new("/p/d"), COMMAND, &rows());
        assert_eq!(names(&selection), ["/p/b", "/p/d"]);
        selection.click(Path::new("/p/b"), COMMAND, &rows());
        assert_eq!(names(&selection), ["/p/d"]);
    }

    #[test]
    fn shift_click_selects_a_range_from_the_anchor_in_both_directions() {
        let mut selection = TreeSelection::default();
        selection.click(Path::new("/p/c"), PLAIN, &rows());
        selection.click(Path::new("/p/e"), SHIFT, &rows());
        assert_eq!(names(&selection), ["/p/c", "/p/d", "/p/e"]);
        selection.click(Path::new("/p/a"), SHIFT, &rows());
        assert_eq!(names(&selection), ["/p/a", "/p/b", "/p/c"]);
    }

    #[test]
    fn command_shift_click_adds_a_range() {
        let mut selection = TreeSelection::default();
        selection.click(Path::new("/p/a"), PLAIN, &rows());
        selection.click(Path::new("/p/d"), COMMAND, &rows());
        selection.click(Path::new("/p/e"), COMMAND_SHIFT, &rows());
        assert_eq!(names(&selection), ["/p/a", "/p/d", "/p/e"]);
    }

    #[test]
    fn arrows_move_and_shift_arrows_extend_and_shrink() {
        let mut selection = TreeSelection::default();
        selection.click(Path::new("/p/b"), PLAIN, &rows());
        selection.step(1, false, &rows());
        assert_eq!(names(&selection), ["/p/c"]);
        selection.step(1, true, &rows());
        selection.step(1, true, &rows());
        assert_eq!(names(&selection), ["/p/c", "/p/d", "/p/e"]);
        selection.step(-1, true, &rows());
        assert_eq!(names(&selection), ["/p/c", "/p/d"]);
        selection.step(-1, true, &rows());
        selection.step(-1, true, &rows());
        assert_eq!(names(&selection), ["/p/b", "/p/c"]);
        selection.step(-1, false, &rows());
        selection.step(-1, false, &rows());
        selection.step(-1, false, &rows());
        assert_eq!(names(&selection), ["/p"]);
    }

    #[test]
    fn rows_that_disappear_leave_the_selection() {
        let mut selection = TreeSelection::default();
        selection.select_all_of(rows()[1..4].to_vec());
        selection.retain(|path| path != Path::new("/p/c"));
        assert_eq!(names(&selection), ["/p/a", "/p/b"]);
        assert_eq!(selection.lead(), Some(&PathBuf::from("/p/b")));
    }

    #[test]
    fn nested_paths_are_left_to_their_folder() {
        let paths: Vec<PathBuf> = ["/p/a", "/p/a/x", "/p/b"].iter().map(PathBuf::from).collect();
        assert_eq!(outermost(&paths), [PathBuf::from("/p/a"), PathBuf::from("/p/b")]);
    }
}
