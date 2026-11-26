use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::debug::breakpoints::{mark_lines_of, whole_line_marks};
use crate::history::Spot;

pub const MNEMONIC_TAKEN: &str = "mnemonicTaken";

pub const FOLDER_BOOKMARK: &str = "folderBookmark";

pub const POPUP_LINE_ONLY: &str = "bookmarks.popup.lineBookmarksOnly";

pub const MNEMONIC_ROWS: [&str; 4] = ["1234567890", "QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bookmark {
    pub line: Option<u32>,
    pub folder: bool,
    pub mnemonic: Option<char>,
    pub description: String,
    pub list: String,
    pub rank: u64,
}

/// A bookmark with its file, as the commands give it and the workspace file keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkInfo {
    /// The file or folder.
    pub path: PathBuf,
    /// The line (from 1 in what the commands answer, from 0 in the workspace file); absent for the bookmark of a whole file or folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Whether it is the bookmark of a folder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub folder: bool,
    /// The digit or capital letter of a mnemonic bookmark; absent for an anonymous one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<char>,
    /// What it says on hover; absent when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The bookmark list it is in; absent for the default list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
}

impl BookmarkInfo {
    fn new(path: &Path, bookmark: &Bookmark, default: Option<&str>) -> Self {
        Self {
            path: path.to_owned(),
            line: bookmark.line,
            folder: bookmark.folder,
            mnemonic: bookmark.mnemonic,
            description: bookmark.description.clone(),
            list: (Some(bookmark.list.as_str()) != default).then(|| bookmark.list.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListError {
    Empty,
    Taken(String),
    Missing(String),
}

impl From<ListError> for CommandError {
    fn from(error: ListError) -> Self {
        CommandError::new(match error {
            ListError::Empty => t!("bookmark-list-empty-name"),
            ListError::Taken(name) => t!("bookmark-list-taken", name = name),
            ListError::Missing(name) => t!("bookmark-list-missing", name = name),
        })
    }
}

pub fn mnemonic_of(text: &str) -> Option<char> {
    let mut chars = text.trim().chars();
    let c = chars.next()?.to_ascii_uppercase();
    (chars.next().is_none() && (c.is_ascii_digit() || c.is_ascii_uppercase())).then_some(c)
}

#[derive(Clone, Debug, Default)]
pub struct Bookmarks {
    files: BTreeMap<PathBuf, Vec<Bookmark>>,
    lists: Vec<String>,
    project: String,
    next_rank: u64,
    replace_without_asking: bool,
    group_by_file: bool,
    revision: u64,
}

impl Bookmarks {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn for_path(&self, path: &Path) -> &[Bookmark] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn get(&self, path: &Path, line: Option<u32>) -> Option<&Bookmark> {
        self.for_path(path).iter().find(|bookmark| bookmark.line == line)
    }

    pub fn list(&self, path: Option<&Path>) -> Vec<BookmarkInfo> {
        let default = self.lists.first().map(String::as_str);
        self.files
            .iter()
            .filter(|(file, _)| path.is_none_or(|path| path == file.as_path()))
            .flat_map(|(file, bookmarks)| {
                bookmarks.iter().map(move |bookmark| BookmarkInfo::new(file, bookmark, default))
            })
            .collect()
    }

    pub fn lists(&self) -> &[String] {
        &self.lists
    }

    pub fn default_list(&self) -> String {
        match self.lists.first() {
            Some(list) => list.clone(),
            None if self.project.is_empty() => t!("bookmark-list-fallback"),
            None => self.project.clone(),
        }
    }

    pub fn in_list(&self, list: &str) -> Vec<BookmarkInfo> {
        let mut found: Vec<(u64, BookmarkInfo)> = self
            .files
            .iter()
            .flat_map(|(file, bookmarks)| {
                bookmarks
                    .iter()
                    .filter(|bookmark| bookmark.list == list)
                    .map(move |bookmark| (bookmark.rank, BookmarkInfo::new(file, bookmark, Some(list))))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, info)| info).collect()
    }

    pub fn saved(&self) -> Vec<BookmarkInfo> {
        let default = self.lists.first();
        self.lists
            .iter()
            .flat_map(|list| {
                let named = (Some(list) != default).then(|| list.clone());
                self.in_list(list).into_iter().map(move |info| BookmarkInfo { list: named.clone(), ..info })
            })
            .collect()
    }

    pub fn with_mnemonic(&self, mnemonic: char) -> Option<(PathBuf, Option<u32>)> {
        self.files.iter().find_map(|(path, bookmarks)| {
            bookmarks
                .iter()
                .find(|bookmark| bookmark.mnemonic == Some(mnemonic))
                .map(|found| (path.clone(), found.line))
        })
    }

    pub fn replace_without_asking(&self) -> bool {
        self.replace_without_asking
    }

    pub fn set_replace_without_asking(&mut self, without_asking: bool) {
        if self.replace_without_asking != without_asking {
            self.replace_without_asking = without_asking;
            self.revision += 1;
        }
    }

    pub fn group_by_file(&self) -> bool {
        self.group_by_file
    }

    pub fn set_group_by_file(&mut self, group: bool) {
        if self.group_by_file != group {
            self.group_by_file = group;
            self.revision += 1;
        }
    }

    pub fn restore(&mut self, saved: Vec<BookmarkInfo>, lists: Vec<String>) {
        self.files.clear();
        self.lists.clear();
        let lists = if lists.is_empty() {
            let mut named: Vec<String> = saved.iter().filter_map(|info| info.list.clone()).collect();
            named.sort();
            std::iter::once(self.default_list()).chain(named).collect()
        } else {
            lists
        };
        for list in lists {
            let list = list.trim().to_owned();
            if !list.is_empty() && !self.lists.contains(&list) {
                self.lists.push(list);
            }
        }
        for info in saved {
            let mnemonic = info.mnemonic.and_then(|c| mnemonic_of(&c.to_string()));
            let mnemonic = mnemonic.filter(|c| self.with_mnemonic(*c).is_none());
            let list = info.list.map(|list| list.trim().to_owned()).unwrap_or_default();
            let bookmark =
                Bookmark { line: info.line, folder: false, mnemonic, description: info.description, list, rank: 0 };
            self.insert(&info.path, bookmark);
        }
        self.revision += 1;
    }

    fn own_list(&mut self, list: &str) -> String {
        let list = list.trim();
        let list = if list.is_empty() { self.default_list() } else { list.to_owned() };
        if !self.lists.contains(&list) {
            self.lists.push(list.clone());
        }
        list
    }

    fn insert(&mut self, path: &Path, mut bookmark: Bookmark) {
        bookmark.list = self.own_list(&bookmark.list);
        bookmark.folder = bookmark.line.is_none() && is_folder(path);
        bookmark.rank = self.next_rank;
        self.next_rank += 1;
        self.place(path, bookmark);
    }

    fn place(&mut self, path: &Path, bookmark: Bookmark) {
        let bookmarks = self.files.entry(path.to_owned()).or_default();
        match bookmarks.iter_mut().find(|kept| kept.line == bookmark.line) {
            Some(kept) => *kept = bookmark,
            None => {
                bookmarks.push(bookmark);
                bookmarks.sort_by_key(|kept| kept.line);
            }
        }
    }

    fn info(&self, path: &Path, line: Option<u32>) -> Option<BookmarkInfo> {
        self.get(path, line).map(|bookmark| BookmarkInfo::new(path, bookmark, self.lists.first().map(String::as_str)))
    }

    fn change(&mut self, path: &Path, line: Option<u32>, change: impl FnOnce(&mut Bookmark)) -> Option<BookmarkInfo> {
        let bookmark = self.files.get_mut(path)?.iter_mut().find(|bookmark| bookmark.line == line)?;
        change(bookmark);
        self.revision += 1;
        self.info(path, line)
    }

    pub fn remove(&mut self, path: &Path, line: Option<u32>) -> bool {
        let Some(bookmarks) = self.files.get_mut(path) else { return false };
        let before = bookmarks.len();
        bookmarks.retain(|bookmark| bookmark.line != line);
        let removed = bookmarks.len() != before;
        if bookmarks.is_empty() {
            self.files.remove(path);
        }
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub fn toggle(&mut self, path: &Path, line: Option<u32>) -> Option<BookmarkInfo> {
        if self.remove(path, line) {
            return None;
        }
        self.insert(path, Bookmark { line, ..Bookmark::default() });
        self.revision += 1;
        self.info(path, line)
    }

    pub fn toggle_items(&mut self, paths: &[PathBuf]) -> bool {
        if !paths.is_empty() && paths.iter().all(|path| self.get(path, None).is_some()) {
            for path in paths {
                self.remove(path, None);
            }
            return false;
        }
        for path in paths {
            if self.get(path, None).is_none() {
                self.insert(path, Bookmark::default());
            }
        }
        self.revision += 1;
        true
    }

    pub fn remove_items(&mut self, paths: &[PathBuf]) -> usize {
        paths.iter().filter(|path| self.remove(path, None)).count()
    }

    pub fn bookmark_items(&mut self, name: &str, paths: &[PathBuf]) -> Result<String, ListError> {
        let name = self.create_list(name)?;
        let mut order: Vec<(PathBuf, Option<u32>)> = Vec::new();
        for path in paths {
            if order.iter().any(|(kept, _)| kept == path) {
                continue;
            }
            match self.files.get_mut(path).and_then(|bookmarks| bookmarks.iter_mut().find(|kept| kept.line.is_none())) {
                Some(kept) => kept.list.clone_from(&name),
                None => self.insert(path, Bookmark { list: name.clone(), ..Bookmark::default() }),
            }
            order.push((path.clone(), None));
        }
        self.reorder(&order);
        Ok(name)
    }

    pub fn set_mnemonic(
        &mut self,
        path: &Path,
        line: Option<u32>,
        mnemonic: char,
        description: Option<String>,
    ) -> BookmarkInfo {
        if let Some((other, at)) =
            self.with_mnemonic(mnemonic).filter(|(other, at)| (other.as_path(), *at) != (path, line))
        {
            self.remove(&other, at);
        }
        if self.get(path, line).is_none() {
            self.insert(path, Bookmark { line, ..Bookmark::default() });
        }
        let changed = self.change(path, line, |bookmark| {
            bookmark.mnemonic = Some(mnemonic);
            if let Some(description) = description {
                bookmark.description = description.trim().to_owned();
            }
        });
        changed.unwrap_or_else(|| {
            BookmarkInfo::new(path, &Bookmark { line, mnemonic: Some(mnemonic), ..Bookmark::default() }, Some(""))
        })
    }

    pub fn remove_mnemonic(&mut self, path: &Path, line: Option<u32>) -> Option<BookmarkInfo> {
        self.change(path, line, |bookmark| bookmark.mnemonic = None)
    }

    pub fn rename(&mut self, path: &Path, line: Option<u32>, description: &str) -> Option<BookmarkInfo> {
        self.change(path, line, |bookmark| bookmark.description = description.trim().to_owned())
    }

    pub fn move_to_list(
        &mut self,
        path: &Path,
        line: Option<u32>,
        list: Option<String>,
        index: Option<usize>,
    ) -> Option<BookmarkInfo> {
        self.get(path, line)?;
        let list = self.own_list(list.as_deref().unwrap_or_default());
        let mut order: Vec<(PathBuf, Option<u32>)> = self
            .in_list(&list)
            .into_iter()
            .map(|info| (info.path, info.line))
            .filter(|place| (place.0.as_path(), place.1) != (path, line))
            .collect();
        let at = index.unwrap_or(order.len()).min(order.len());
        order.insert(at, (path.to_owned(), line));
        self.change(path, line, |bookmark| bookmark.list = list);
        self.reorder(&order);
        self.info(path, line)
    }

    pub fn move_by_one(&mut self, path: &Path, line: Option<u32>, up: bool) -> Option<BookmarkInfo> {
        let list = self.get(path, line)?.list.clone();
        let mut order: Vec<(PathBuf, Option<u32>)> =
            self.in_list(&list).into_iter().map(|info| (info.path, info.line)).collect();
        let at = order.iter().position(|place| (place.0.as_path(), place.1) == (path, line))?;
        let other = if up { at.checked_sub(1) } else { Some(at + 1).filter(|other| *other < order.len()) };
        if let Some(other) = other {
            order.swap(at, other);
            self.reorder(&order);
        }
        self.info(path, line)
    }

    fn reorder(&mut self, order: &[(PathBuf, Option<u32>)]) {
        for (path, line) in order {
            let rank = self.next_rank;
            self.next_rank += 1;
            if let Some(bookmark) =
                self.files.get_mut(path).and_then(|bookmarks| bookmarks.iter_mut().find(|kept| kept.line == *line))
            {
                bookmark.rank = rank;
            }
        }
        self.revision += 1;
    }

    fn named(name: &str) -> Result<String, ListError> {
        let name = name.trim();
        if name.is_empty() { Err(ListError::Empty) } else { Ok(name.to_owned()) }
    }

    fn existing(&self, name: &str) -> Result<String, ListError> {
        let name = Self::named(name)?;
        if self.lists.contains(&name) { Ok(name) } else { Err(ListError::Missing(name)) }
    }

    pub fn create_list(&mut self, name: &str) -> Result<String, ListError> {
        let name = Self::named(name)?;
        if self.lists.contains(&name) {
            return Err(ListError::Taken(name));
        }
        self.lists.push(name.clone());
        self.revision += 1;
        Ok(name)
    }

    pub fn rename_list(&mut self, name: &str, new_name: &str) -> Result<String, ListError> {
        let name = self.existing(name)?;
        let new_name = Self::named(new_name)?;
        if new_name != name && self.lists.contains(&new_name) {
            return Err(ListError::Taken(new_name));
        }
        for list in self.lists.iter_mut().filter(|list| **list == name) {
            list.clone_from(&new_name);
        }
        for bookmark in self.files.values_mut().flatten().filter(|bookmark| bookmark.list == name) {
            bookmark.list.clone_from(&new_name);
        }
        self.revision += 1;
        Ok(new_name)
    }

    pub fn set_default_list(&mut self, name: &str) -> Result<String, ListError> {
        let name = self.existing(name)?;
        self.lists.retain(|list| *list != name);
        self.lists.insert(0, name.clone());
        self.revision += 1;
        Ok(name)
    }

    pub fn delete_list(&mut self, name: &str) -> Result<usize, ListError> {
        let name = self.existing(name)?;
        self.lists.retain(|list| *list != name);
        let mut removed = 0;
        for bookmarks in self.files.values_mut() {
            let before = bookmarks.len();
            bookmarks.retain(|bookmark| bookmark.list != name);
            removed += before - bookmarks.len();
        }
        self.files.retain(|_, bookmarks| !bookmarks.is_empty());
        self.revision += 1;
        Ok(removed)
    }

    pub fn next_after(&self, path: Option<&Path>, line: u32, forward: bool) -> Option<(PathBuf, u32)> {
        let all: Vec<(&Path, u32)> = self
            .files
            .iter()
            .flat_map(|(file, bookmarks)| {
                bookmarks.iter().filter_map(move |bookmark| Some((file.as_path(), bookmark.line?)))
            })
            .collect();
        let here = path.map(|path| (path, line));
        let found = if forward {
            here.and_then(|here| all.iter().find(|place| **place > here)).or(all.first())
        } else {
            here.and_then(|here| all.iter().rev().find(|place| **place < here)).or(all.last())
        };
        found.map(|(path, line)| (path.to_path_buf(), *line))
    }

    pub fn has_lines(&self) -> bool {
        self.files.values().flatten().any(|bookmark| bookmark.line.is_some())
    }

    pub fn relocate(&mut self, from: &Path, to: &Path) {
        let moved: Vec<PathBuf> = self.files.keys().filter(|path| path.starts_with(from)).cloned().collect();
        for path in moved {
            let Some(bookmarks) = self.files.remove(&path) else { continue };
            let rest = path.strip_prefix(from).unwrap_or(Path::new(""));
            let target = if rest.as_os_str().is_empty() { to.to_owned() } else { to.join(rest) };
            for bookmark in bookmarks {
                self.place(&target, bookmark);
            }
            self.revision += 1;
        }
    }

    pub fn retain_files(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        let before = self.files.len();
        self.files.retain(|path, _| keep(path));
        if self.files.len() != before {
            self.revision += 1;
        }
    }

    fn name_after(&mut self, workbench: &Workbench) {
        let project = workbench
            .project()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.project != project {
            self.project = project;
        }
        if self.lists.is_empty() && !self.project.is_empty() {
            self.lists.push(self.project.clone());
            self.revision += 1;
        }
    }

    fn pull(&mut self, workbench: &Workbench) {
        let mut changed = false;
        for document in workbench.documents().iter().filter(|document| document.image().is_none()) {
            let Some(bookmarks) = self.files.get_mut(document.path()) else { continue };
            let (mut moved, lined): (Vec<Bookmark>, Vec<Bookmark>) =
                bookmarks.iter().cloned().partition(|bookmark| bookmark.line.is_none());
            if document.bookmark_marks.len() != lined.len() {
                continue;
            }
            let lines = mark_lines_of(document, &document.bookmark_marks);
            let mut followed: Vec<Bookmark> = lined
                .into_iter()
                .zip(lines)
                .filter_map(|(bookmark, line)| Some(Bookmark { line: Some(line?), ..bookmark }))
                .collect();
            followed.sort_by_key(|bookmark| bookmark.line);
            followed.dedup_by_key(|bookmark| bookmark.line);
            moved.extend(followed);
            if moved != *bookmarks {
                *bookmarks = moved;
                changed = true;
            }
        }
        self.files.retain(|_, bookmarks| !bookmarks.is_empty());
        if changed {
            self.revision += 1;
        }
    }

    fn push(&mut self, workbench: &mut Workbench) {
        let mut changed = false;
        for document in workbench.documents.iter_mut().filter(|document| document.image.is_none()) {
            let Some(bookmarks) = self.files.get_mut(&document.path) else {
                document.bookmark_marks.clear();
                continue;
            };
            let lines = document.buffer.text().len_lines();
            let before = bookmarks.len();
            bookmarks.retain(|bookmark| bookmark.line.is_none_or(|line| (line as usize) < lines));
            changed |= bookmarks.len() != before;
            let wanted: Vec<u32> = bookmarks.iter().filter_map(|bookmark| bookmark.line).collect();
            document.bookmark_marks = whole_line_marks(document.buffer.text(), &wanted);
        }
        self.files.retain(|_, bookmarks| !bookmarks.is_empty());
        if changed {
            self.revision += 1;
        }
    }
}

fn is_folder(path: &Path) -> bool {
    !cfg!(target_arch = "wasm32") && path.is_dir()
}

pub(crate) fn follow_lines(before: &str, after: &str, lines: &[Option<usize>]) -> Vec<Option<usize>> {
    let hunks = zen_vcs::diff::hunks(before, after);
    lines
        .iter()
        .map(|line| {
            let line = (*line)?;
            let mut shift = 0isize;
            for hunk in hunks.iter().take_while(|hunk| hunk.before.start <= line) {
                if line < hunk.before.end {
                    let last = hunk.after.len().checked_sub(1)?;
                    return Some(hunk.after.start + (line - hunk.before.start).min(last));
                }
                shift = hunk.after.end as isize - hunk.before.end as isize;
            }
            line.checked_add_signed(shift)
        })
        .collect()
}

pub fn sync(session: &mut Session) {
    let mut bookmarks = std::mem::take(session.state_mut::<Bookmarks>());
    bookmarks.name_after(session.state_mut::<Workbench>());
    bookmarks.pull(session.state_mut::<Workbench>());
    bookmarks.push(session.state_mut::<Workbench>());
    session.set_state(bookmarks);
}

pub fn restore(session: &mut Session, saved: Vec<BookmarkInfo>, lists: Vec<String>) {
    let kept = saved
        .into_iter()
        .filter(|bookmark| {
            cfg!(target_arch = "wasm32")
                || if bookmark.line.is_some() { bookmark.path.is_file() } else { bookmark.path.exists() }
        })
        .collect();
    let mut bookmarks = std::mem::take(session.state_mut::<Bookmarks>());
    bookmarks.name_after(session.state_mut::<Workbench>());
    bookmarks.restore(kept, lists);
    session.set_state(bookmarks);
    sync(session);
}

pub fn relocate(session: &mut Session, from: &Path, to: &Path) {
    session.state_mut::<Bookmarks>().relocate(from, to);
}

pub fn forget(session: &mut Session, gone: &[PathBuf]) {
    session.state_mut::<Bookmarks>().retain_files(|path| !gone.iter().any(|gone| path.starts_with(gone)));
}

fn forget_missing(session: &mut Session) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let open: Vec<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    session.state_mut::<Bookmarks>().retain_files(|path| open.iter().any(|open| open == path) || path.exists());
}

fn with_bookmarks<R>(session: &mut Session, change: impl FnOnce(&mut Bookmarks) -> R) -> R {
    sync(session);
    let result = change(session.state_mut::<Bookmarks>());
    sync(session);
    result
}

fn told(info: &BookmarkInfo) -> BookmarkInfo {
    BookmarkInfo { line: info.line.map(|line| line + 1), ..info.clone() }
}

fn one_based<S: serde::Serializer>(info: &Option<BookmarkInfo>, serializer: S) -> Result<S::Ok, S::Error> {
    info.as_ref().map(told).serialize(serializer)
}

fn all_one_based<S: serde::Serializer>(infos: &[BookmarkInfo], serializer: S) -> Result<S::Ok, S::Error> {
    infos.iter().map(told).collect::<Vec<_>>().serialize(serializer)
}

fn from_one(line: Option<u32>) -> Result<Option<u32>, CommandError> {
    match line {
        Some(0) => Err(CommandError::new(t!("bookmark-line-from-one"))),
        other => Ok(other.map(|line| line - 1)),
    }
}

fn caret_place(
    session: &Session,
    path: Option<PathBuf>,
    line: Option<u32>,
    whole: bool,
) -> Result<(PathBuf, Option<u32>), CommandError> {
    let line = from_one(line)?;
    let active = session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .filter(|document| whole || document.image().is_none());
    let path = match path.or_else(|| active.map(|document| document.path().to_owned())) {
        Some(path) => path,
        None => return Err(CommandError::new(t!("workbench-error-no-document"))),
    };
    if whole {
        return Ok((path, None));
    }
    let line = match line {
        Some(line) => line,
        None => active
            .filter(|document| document.path() == path)
            .and_then(|document| document.snapshot().offset_to_point(document.selections().primary().head).ok())
            .and_then(|point| u32::try_from(point.line).ok())
            .ok_or_else(|| CommandError::new(t!("bookmark-no-line")))?,
    };
    Ok((path, Some(line)))
}

fn parse_mnemonic(text: &str) -> Result<char, CommandError> {
    mnemonic_of(text).ok_or_else(|| CommandError::new(t!("bookmark-bad-mnemonic", mnemonic = text)))
}

fn no_bookmark(path: &Path, line: Option<u32>) -> CommandError {
    match line {
        Some(line) => CommandError::new(t!("bookmark-none", path = crate::display(path), line = line + 1)),
        None => CommandError::new(t!("bookmark-none-item", path = crate::display(path))),
    }
}

fn current(session: &Session, path: &Path, line: Option<u32>) -> Option<BookmarkInfo> {
    session.state::<Bookmarks>()?.info(path, line)
}

fn go_to(session: &mut Session, path: &Path, line: Option<u32>) -> Result<Spot, CommandError> {
    match line {
        Some(line) => crate::history::go_to_line(session, path, line),
        None if is_folder(path) => {
            let message = t!("bookmark-folder", path = crate::display(path));
            Err(CommandError::with_code(FOLDER_BOOKMARK, message).with_data(serde_json::json!({ "path": path })))
        }
        None => crate::history::go_to_file(session, path),
    }
}

/// A line of a file, the caret's line of the active editor when absent, or with `whole` a file or folder itself.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaceParams {
    /// The file or folder; the active editor's file when absent.
    pub path: Option<PathBuf>,
    /// The line, from 1; the caret's when absent.
    pub line: Option<u32>,
    /// Whether it is the bookmark of the whole file or folder, not of a line.
    pub whole: bool,
}

/// What the bookmark commands answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkAnswer {
    /// The bookmark, its line from 1; absent when it was removed.
    #[serde(serialize_with = "one_based")]
    pub bookmark: Option<BookmarkInfo>,
}

pub struct ToggleBookmark;

impl Command for ToggleBookmark {
    type Params = PlaceParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.toggle";
    const TITLE: &'static str = "Toggle Bookmark";
    const DESCRIPTION: &'static str = "Sets an anonymous line bookmark or removes the bookmark there (F3), at the \
         caret of the active editor by default; with `whole`, the bookmark of the file or folder itself (an editor \
         tab's Bookmarks › Toggle Bookmark).";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BookmarkAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let set = with_bookmarks(session, |bookmarks| bookmarks.toggle(&path, line));
        Ok(BookmarkAnswer { bookmark: set.and_then(|_| current(session, &path, line)) })
    }
}

/// The parameters of `navigate.bookmark.toggleMnemonic`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleMnemonicParams {
    /// The digit or letter.
    pub mnemonic: String,
    /// The file; the active editor's when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The line, from 1; the caret's when absent.
    #[serde(default)]
    pub line: Option<u32>,
    /// Whether it is the bookmark of the whole file or folder, not of a line.
    #[serde(default)]
    pub whole: bool,
}

pub struct ToggleMnemonic;

impl Command for ToggleMnemonic {
    type Params = ToggleMnemonicParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.toggleMnemonic";
    const TITLE: &'static str = "Toggle Bookmark with Mnemonic";
    const DESCRIPTION: &'static str = "Removes the line's bookmark when it has this mnemonic; otherwise gives the line \
         a bookmark with it, taking it from the line that had it, without asking (⌃⇧ and a digit).";

    fn run(&self, session: &mut Session, params: ToggleMnemonicParams) -> Result<BookmarkAnswer, CommandError> {
        let mnemonic = parse_mnemonic(&params.mnemonic)?;
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let set = with_bookmarks(session, |bookmarks| {
            if bookmarks.get(&path, line).is_some_and(|bookmark| bookmark.mnemonic == Some(mnemonic)) {
                bookmarks.remove(&path, line);
                false
            } else {
                bookmarks.set_mnemonic(&path, line, mnemonic, None);
                true
            }
        });
        Ok(BookmarkAnswer { bookmark: if set { current(session, &path, line) } else { None } })
    }
}

/// The parameters of `navigate.bookmark.setMnemonic`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetMnemonicParams {
    /// The digit or letter.
    pub mnemonic: String,
    /// The description; unchanged when absent.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether to take the mnemonic from another line that has it; refused with `mnemonicTaken` when absent and another line has it.
    #[serde(default)]
    pub replace: bool,
    /// The file; the active editor's when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The line, from 1; the caret's when absent.
    #[serde(default)]
    pub line: Option<u32>,
    /// Whether it is the bookmark of the whole file or folder, not of a line.
    #[serde(default)]
    pub whole: bool,
}

pub struct SetMnemonic;

impl Command for SetMnemonic {
    type Params = SetMnemonicParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.setMnemonic";
    const TITLE: &'static str = "Add Mnemonic Bookmark";
    const DESCRIPTION: &'static str = "Gives the line a bookmark with a mnemonic (0–9, A–Z) and an optional \
         description, or changes the mnemonic of the bookmark there (⌥F3); with `whole`, the file or folder \
         itself. A mnemonic another place has is refused with the code mnemonicTaken and that place, unless \
         `replace` moves it here.";

    fn run(&self, session: &mut Session, params: SetMnemonicParams) -> Result<BookmarkAnswer, CommandError> {
        let mnemonic = parse_mnemonic(&params.mnemonic)?;
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        sync(session);
        let taken = session
            .state::<Bookmarks>()
            .and_then(|bookmarks| bookmarks.with_mnemonic(mnemonic))
            .filter(|(other, at)| (other.as_path(), *at) != (path.as_path(), line));
        if let Some((other, at)) = taken.filter(|_| !params.replace) {
            let message = match at {
                Some(at) => t!(
                    "bookmark-mnemonic-taken",
                    mnemonic = mnemonic.to_string(),
                    path = crate::display(&other),
                    line = at + 1
                ),
                None => {
                    t!("bookmark-mnemonic-taken-item", mnemonic = mnemonic.to_string(), path = crate::display(&other))
                }
            };
            let place = match at {
                Some(at) => serde_json::json!({ "path": other, "line": at + 1 }),
                None => serde_json::json!({ "path": other }),
            };
            return Err(CommandError::with_code(MNEMONIC_TAKEN, message).with_data(place));
        }
        let set =
            with_bookmarks(session, |bookmarks| bookmarks.set_mnemonic(&path, line, mnemonic, params.description));
        Ok(BookmarkAnswer { bookmark: Some(set) })
    }
}

pub struct RemoveMnemonic;

impl Command for RemoveMnemonic {
    type Params = PlaceParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.removeMnemonic";
    const TITLE: &'static str = "Remove Mnemonic";
    const DESCRIPTION: &'static str = "Makes the mnemonic bookmark of a line (or with `whole` of a file or folder) \
         anonymous, keeping its description.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BookmarkAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let changed = with_bookmarks(session, |bookmarks| bookmarks.remove_mnemonic(&path, line));
        changed.map(|bookmark| BookmarkAnswer { bookmark: Some(bookmark) }).ok_or_else(|| no_bookmark(&path, line))
    }
}

/// The parameters of `navigate.bookmark.rename`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The new description; empty clears it.
    pub description: String,
    /// The file; the active editor's when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The line, from 1; the caret's when absent.
    #[serde(default)]
    pub line: Option<u32>,
    /// Whether it is the bookmark of the whole file or folder, not of a line.
    #[serde(default)]
    pub whole: bool,
}

pub struct RenameBookmark;

impl Command for RenameBookmark {
    type Params = RenameParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.rename";
    const TITLE: &'static str = "Rename Bookmark";
    const DESCRIPTION: &'static str = "Changes the description of the bookmark of a line, which its gutter icon \
         shows on hover, or with `whole` of a file or folder.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<BookmarkAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let changed = with_bookmarks(session, |bookmarks| bookmarks.rename(&path, line, &params.description));
        changed.map(|bookmark| BookmarkAnswer { bookmark: Some(bookmark) }).ok_or_else(|| no_bookmark(&path, line))
    }
}

pub struct RemoveBookmark;

impl Command for RemoveBookmark {
    type Params = PlaceParams;
    type Output = ();
    const ID: &'static str = "navigate.bookmark.remove";
    const TITLE: &'static str = "Remove Bookmark";
    const DESCRIPTION: &'static str = "Removes the bookmark of a line, anonymous or mnemonic, or with `whole` the \
         bookmark of a file or folder itself (an editor tab's Bookmarks › Delete).";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<(), CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        if with_bookmarks(session, |bookmarks| bookmarks.remove(&path, line)) {
            Ok(())
        } else {
            Err(no_bookmark(&path, line))
        }
    }
}

/// The parameters of `navigate.bookmark.moveToList`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveToListParams {
    /// The list's name, a new one creating it; absent or empty for the default list.
    #[serde(default)]
    pub list: Option<String>,
    /// Where in the list it goes, from 0; last when absent.
    #[serde(default)]
    pub index: Option<usize>,
    /// The file; the active editor's when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The line, from 1; the caret's when absent.
    #[serde(default)]
    pub line: Option<u32>,
    /// Whether it is the bookmark of the whole file or folder, not of a line.
    #[serde(default)]
    pub whole: bool,
}

pub struct MoveToList;

impl Command for MoveToList {
    type Params = MoveToListParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = "navigate.bookmark.moveToList";
    const TITLE: &'static str = "Add Bookmark to Another List";
    const DESCRIPTION: &'static str = "Puts the bookmark of a line (or with `whole` of a file or folder) in another \
         bookmark list, created by its name, at `index` in it or last (what dragging it there in the Bookmarks tool \
         window does); without a name, in the default list.";

    fn run(&self, session: &mut Session, params: MoveToListParams) -> Result<BookmarkAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let changed =
            with_bookmarks(session, |bookmarks| bookmarks.move_to_list(&path, line, params.list, params.index));
        changed.map(|bookmark| BookmarkAnswer { bookmark: Some(bookmark) }).ok_or_else(|| no_bookmark(&path, line))
    }
}

pub struct MoveBookmark<const UP: bool>;

impl<const UP: bool> Command for MoveBookmark<UP> {
    type Params = PlaceParams;
    type Output = BookmarkAnswer;
    const ID: &'static str = if UP { "navigate.bookmark.moveUp" } else { "navigate.bookmark.moveDown" };
    const TITLE: &'static str = if UP { "Move Up" } else { "Move Down" };
    const DESCRIPTION: &'static str = if UP {
        "Moves the bookmark of a line (or with `whole` of a file or folder) one place up in its bookmark list; the \
         first one stays."
    } else {
        "Moves the bookmark of a line (or with `whole` of a file or folder) one place down in its bookmark list; \
         the last one stays."
    };

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BookmarkAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line, params.whole)?;
        let moved = with_bookmarks(session, |bookmarks| bookmarks.move_by_one(&path, line, UP));
        moved.map(|bookmark| BookmarkAnswer { bookmark: Some(bookmark) }).ok_or_else(|| no_bookmark(&path, line))
    }
}

/// The parameters of `navigate.bookmark.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Only this file's bookmarks.
    pub path: Option<PathBuf>,
}

/// A bookmark list with its bookmarks in their order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkGroup {
    /// The list's name.
    pub name: String,
    /// Whether it is the default list, which new bookmarks go to.
    pub is_default: bool,
    /// Its bookmarks, in their order, their lines from 1.
    #[serde(serialize_with = "all_one_based")]
    pub bookmarks: Vec<BookmarkInfo>,
}

/// What `navigate.bookmark.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkList {
    /// The bookmarks, by file and line (from 1), a file's or folder's own first.
    #[serde(serialize_with = "all_one_based")]
    pub bookmarks: Vec<BookmarkInfo>,
    /// The name of the default list, at first the project folder's.
    pub default_list: String,
    /// The other lists, in their order.
    pub lists: Vec<String>,
    /// Every list in its order, the default one first, with its bookmarks in their order.
    pub groups: Vec<BookmarkGroup>,
    /// Whether choosing a mnemonic another line has moves it without asking.
    pub replace_without_asking: bool,
    /// Whether the Bookmarks tool window groups the line bookmarks of a list by file.
    pub group_by_file: bool,
}

pub struct ListBookmarks;

impl Command for ListBookmarks {
    type Params = ListParams;
    type Output = BookmarkList;
    const ID: &'static str = "navigate.bookmark.list";
    const TITLE: &'static str = "Bookmarks";
    const DESCRIPTION: &'static str = "The bookmarks by file and line from 1, with their mnemonics, descriptions \
         and lists, as their lines are now in the open editors; a bookmark of a whole file or folder has no `line` \
         (and `folder` for a folder). And the bookmark lists in their order, the default one first, with their \
         bookmarks in their order, as the Bookmarks tool window shows them.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<BookmarkList, CommandError> {
        sync(session);
        forget_missing(session);
        let bookmarks = session.state::<Bookmarks>().cloned().unwrap_or_default();
        let default_list = bookmarks.default_list();
        let mut names = bookmarks.lists().to_vec();
        if names.is_empty() {
            names.push(default_list.clone());
        }
        let only = |infos: Vec<BookmarkInfo>| -> Vec<BookmarkInfo> {
            infos.into_iter().filter(|info| params.path.as_deref().is_none_or(|path| info.path == path)).collect()
        };
        let groups = names
            .iter()
            .map(|name| BookmarkGroup {
                name: name.clone(),
                is_default: *name == default_list,
                bookmarks: only(bookmarks.in_list(name)),
            })
            .collect();
        Ok(BookmarkList {
            bookmarks: bookmarks.list(params.path.as_deref()),
            lists: names.iter().filter(|name| **name != default_list).cloned().collect(),
            default_list,
            groups,
            replace_without_asking: bookmarks.replace_without_asking(),
            group_by_file: bookmarks.group_by_file(),
        })
    }
}

/// The parameters of `navigate.bookmark.options`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OptionsParams {
    /// Whether choosing a mnemonic another line has moves it without asking; unchanged when absent.
    pub replace_without_asking: Option<bool>,
    /// Whether the Bookmarks tool window groups the line bookmarks of a list by file; unchanged when absent.
    pub group_by_file: Option<bool>,
}

/// What `navigate.bookmark.options` answers: the options as they are now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkOptionsAnswer {
    /// Whether choosing a mnemonic another line has moves it without asking (Ask Before Rewriting Mnemonic off).
    pub replace_without_asking: bool,
    /// Whether the Bookmarks tool window groups the line bookmarks of a list by file.
    pub group_by_file: bool,
}

pub struct BookmarkOptions;

impl Command for BookmarkOptions {
    type Params = OptionsParams;
    type Output = BookmarkOptionsAnswer;
    const ID: &'static str = "navigate.bookmark.options";
    const TITLE: &'static str = "Bookmark Options";
    const DESCRIPTION: &'static str = "Sets the options of the Bookmarks tool window's ⋮ menu: whether choosing a \
         mnemonic another line has moves it there without asking (Ask Before Rewriting Mnemonic off, the \
         question's Don't ask again) and Group Line Bookmarks by File; returns them.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<BookmarkOptionsAnswer, CommandError> {
        let bookmarks = session.state_mut::<Bookmarks>();
        if let Some(without_asking) = params.replace_without_asking {
            bookmarks.set_replace_without_asking(without_asking);
        }
        if let Some(group) = params.group_by_file {
            bookmarks.set_group_by_file(group);
        }
        Ok(BookmarkOptionsAnswer {
            replace_without_asking: bookmarks.replace_without_asking(),
            group_by_file: bookmarks.group_by_file(),
        })
    }
}

/// The parameters of `navigate.bookmark.goTo`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoToParams {
    /// The digit or letter.
    pub mnemonic: String,
}

pub struct GoToBookmark;

impl Command for GoToBookmark {
    type Params = GoToParams;
    type Output = Spot;
    const ID: &'static str = "navigate.bookmark.goTo";
    const TITLE: &'static str = "Go to Bookmark";
    const DESCRIPTION: &'static str = "Opens the file of the bookmark with a mnemonic at its line, or the file of a \
         file bookmark (⌃ and a digit), and records the place left for Back; returns where it went. A folder's \
         bookmark is refused with the code folderBookmark and its path.";

    fn run(&self, session: &mut Session, params: GoToParams) -> Result<Spot, CommandError> {
        let mnemonic = parse_mnemonic(&params.mnemonic)?;
        sync(session);
        forget_missing(session);
        let found = session.state::<Bookmarks>().and_then(|bookmarks| bookmarks.with_mnemonic(mnemonic));
        let Some((path, line)) = found else {
            return Err(CommandError::new(t!("bookmark-no-mnemonic", mnemonic = mnemonic.to_string())));
        };
        go_to(session, &path, line)
    }
}

/// The parameters of `navigate.bookmark.open`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// The file.
    pub path: PathBuf,
    /// The line, from 1; absent for the bookmark of the whole file, which opens it as it was.
    #[serde(default)]
    pub line: Option<u32>,
}

pub struct OpenBookmark;

impl Command for OpenBookmark {
    type Params = OpenParams;
    type Output = Spot;
    const ID: &'static str = "navigate.bookmark.open";
    const TITLE: &'static str = "Jump to Source";
    const DESCRIPTION: &'static str = "Opens the file of a bookmark or breakpoint at its line, or the file of a \
         file bookmark without `line`, as Enter or a double-click in the Bookmarks tool window and the Show Line \
         Bookmarks popup does, and records the place left for Back; returns where it went. A folder's bookmark is \
         refused with the code folderBookmark: view.project.select shows it.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<Spot, CommandError> {
        sync(session);
        go_to(session, &params.path, from_one(params.line)?)
    }
}

pub struct NextBookmark<const FORWARD: bool>;

impl<const FORWARD: bool> Command for NextBookmark<FORWARD> {
    type Params = NoParams;
    type Output = Spot;
    const ID: &'static str = if FORWARD { "navigate.bookmark.next" } else { "navigate.bookmark.previous" };
    const TITLE: &'static str = if FORWARD { "Next Line Bookmark" } else { "Previous Line Bookmark" };
    const DESCRIPTION: &'static str = if FORWARD {
        "Goes to the line bookmark after the caret, by file and line, round to the first; records the place left \
         for Back."
    } else {
        "Goes to the line bookmark before the caret, by file and line, round to the last; records the place left \
         for Back."
    };

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Bookmarks>().is_some_and(Bookmarks::has_lines)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Spot, CommandError> {
        sync(session);
        forget_missing(session);
        let here = crate::history::caret(session);
        let line = here.as_ref().map_or(0, |spot| spot.line.saturating_sub(1));
        let found = session
            .state::<Bookmarks>()
            .and_then(|bookmarks| bookmarks.next_after(here.as_ref().map(|spot| spot.path.as_path()), line, FORWARD));
        let Some((path, line)) = found else { return Err(CommandError::new(t!("bookmark-none-at-all"))) };
        go_to(session, &path, Some(line))
    }
}

/// The parameters of `navigate.bookmarkList.create`, `navigate.bookmarkList.setDefault` and `navigate.bookmarkList.delete`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListNameParams {
    /// The list's name.
    pub name: String,
}

pub struct CreateList;

impl Command for CreateList {
    type Params = ListNameParams;
    type Output = String;
    const ID: &'static str = "navigate.bookmarkList.create";
    const TITLE: &'static str = "Create Bookmarks List";
    const DESCRIPTION: &'static str = "Creates an empty bookmark list after the others; returns its name. A name \
         another list has is refused.";

    fn run(&self, session: &mut Session, params: ListNameParams) -> Result<String, CommandError> {
        Ok(with_bookmarks(session, |bookmarks| bookmarks.create_list(&params.name))?)
    }
}

/// The parameters of `navigate.bookmarkList.rename`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameListParams {
    /// The list's name.
    pub name: String,
    /// Its new name.
    pub new_name: String,
}

pub struct RenameList;

impl Command for RenameList {
    type Params = RenameListParams;
    type Output = String;
    const ID: &'static str = "navigate.bookmarkList.rename";
    const TITLE: &'static str = "Rename Bookmark List";
    const DESCRIPTION: &'static str = "Gives a bookmark list another name, keeping its bookmarks and its place; \
         returns the new name. A name another list has is refused.";

    fn run(&self, session: &mut Session, params: RenameListParams) -> Result<String, CommandError> {
        Ok(with_bookmarks(session, |bookmarks| bookmarks.rename_list(&params.name, &params.new_name))?)
    }
}

pub struct SetDefaultList;

impl Command for SetDefaultList {
    type Params = ListNameParams;
    type Output = String;
    const ID: &'static str = "navigate.bookmarkList.setDefault";
    const TITLE: &'static str = "Mark List as Default";
    const DESCRIPTION: &'static str = "Makes a bookmark list the default one, which new bookmarks go to, and moves \
         it to the top; returns its name.";

    fn run(&self, session: &mut Session, params: ListNameParams) -> Result<String, CommandError> {
        Ok(with_bookmarks(session, |bookmarks| bookmarks.set_default_list(&params.name))?)
    }
}

pub struct DeleteList;

impl Command for DeleteList {
    type Params = ListNameParams;
    type Output = usize;
    const ID: &'static str = "navigate.bookmarkList.delete";
    const TITLE: &'static str = "Delete Bookmark List";
    const DESCRIPTION: &'static str = "Deletes a bookmark list and removes its bookmarks from the code; returns how \
         many were removed. With no list left, the next bookmark makes one named after the project.";

    fn run(&self, session: &mut Session, params: ListNameParams) -> Result<usize, CommandError> {
        Ok(with_bookmarks(session, |bookmarks| bookmarks.delete_list(&params.name))?)
    }
}

/// The parameters of `navigate.bookmark.toggleFiles` and `navigate.bookmark.removeFiles`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesParams {
    /// The files and folders, as the Project tool window's selection gives them.
    pub paths: Vec<PathBuf>,
}

/// What `navigate.bookmark.toggleFiles` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesAnswer {
    /// The bookmarks of the files and folders now, in their order; empty when they were removed.
    pub bookmarks: Vec<BookmarkInfo>,
}

fn files_of(session: &Session, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, CommandError> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths {
        let path = match session.state::<Workbench>() {
            Some(workbench) => workbench.resolve(&path),
            None => path,
        };
        if !cfg!(target_arch = "wasm32") && !path.exists() {
            return Err(CommandError::new(t!("bookmark-no-item", path = crate::display(&path))));
        }
        if !kept.contains(&path) {
            kept.push(path);
        }
    }
    if kept.is_empty() {
        return Err(CommandError::new(t!("bookmark-no-items")));
    }
    Ok(kept)
}

pub struct ToggleFiles;

impl Command for ToggleFiles {
    type Params = FilesParams;
    type Output = FilesAnswer;
    const ID: &'static str = "navigate.bookmark.toggleFiles";
    const TITLE: &'static str = "Toggle File Bookmarks";
    const DESCRIPTION: &'static str = "Toggle Bookmark (F3) on files and folders selected in the Project tool \
         window: removes their bookmarks when every one has one, otherwise bookmarks each that has none in the \
         default list. Line bookmarks stay. Returns their bookmarks now.";

    fn run(&self, session: &mut Session, params: FilesParams) -> Result<FilesAnswer, CommandError> {
        let paths = files_of(session, params.paths)?;
        let set = with_bookmarks(session, |bookmarks| bookmarks.toggle_items(&paths));
        let bookmarks =
            if set { paths.iter().filter_map(|path| current(session, path, None)).collect() } else { Vec::new() };
        Ok(FilesAnswer { bookmarks })
    }
}

pub struct RemoveFiles;

impl Command for RemoveFiles {
    type Params = FilesParams;
    type Output = usize;
    const ID: &'static str = "navigate.bookmark.removeFiles";
    const TITLE: &'static str = "Delete File Bookmarks";
    const DESCRIPTION: &'static str = "Removes the bookmarks of files and folders themselves (the Project tool \
         window's Bookmarks › Delete), not those of their lines; returns how many were removed.";

    fn run(&self, session: &mut Session, params: FilesParams) -> Result<usize, CommandError> {
        let paths = files_of(session, params.paths)?;
        Ok(with_bookmarks(session, |bookmarks| bookmarks.remove_items(&paths)))
    }
}

pub struct BookmarkOpenTabs;

impl Command for BookmarkOpenTabs {
    type Params = ListNameParams;
    type Output = BookmarkGroup;
    const ID: &'static str = "navigate.bookmarkList.fromOpenTabs";
    const TITLE: &'static str = "Bookmark Open Tabs";
    const DESCRIPTION: &'static str = "Bookmark Open Tabs…: creates a bookmark list with this name after the others \
         holding a bookmark of every open editor tab's file in tab order, a file bookmarked already moving into it. \
         A name another list has is refused. Returns the list.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty())
    }

    fn run(&self, session: &mut Session, params: ListNameParams) -> Result<BookmarkGroup, CommandError> {
        let open: Vec<PathBuf> = session
            .state::<Workbench>()
            .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
            .unwrap_or_default();
        if open.is_empty() {
            return Err(CommandError::new(t!("workbench-error-no-document")));
        }
        let name = with_bookmarks(session, |bookmarks| bookmarks.bookmark_items(&params.name, &open))?;
        let bookmarks = session.state::<Bookmarks>().map(|bookmarks| bookmarks.in_list(&name)).unwrap_or_default();
        let is_default = session.state::<Bookmarks>().is_some_and(|bookmarks| bookmarks.default_list() == name);
        Ok(BookmarkGroup { name, is_default, bookmarks })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ToggleBookmark)?;
    registry.register(ToggleMnemonic)?;
    registry.register(SetMnemonic)?;
    registry.register(RemoveMnemonic)?;
    registry.register(RenameBookmark)?;
    registry.register(RemoveBookmark)?;
    registry.register(MoveToList)?;
    registry.register(MoveBookmark::<true>)?;
    registry.register(MoveBookmark::<false>)?;
    registry.register(ListBookmarks)?;
    registry.register(BookmarkOptions)?;
    registry.register(GoToBookmark)?;
    registry.register(OpenBookmark)?;
    registry.register(NextBookmark::<true>)?;
    registry.register(NextBookmark::<false>)?;
    registry.register(CreateList)?;
    registry.register(RenameList)?;
    registry.register(SetDefaultList)?;
    registry.register(DeleteList)?;
    registry.register(ToggleFiles)?;
    registry.register(RemoveFiles)?;
    registry.register(BookmarkOpenTabs)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    trait Mentions {
        fn mentions(&self, id: &str) -> bool;
    }

    impl Mentions for String {
        fn mentions(&self, id: &str) -> bool {
            let translated = t!(id);
            let head = translated.split(['{', '\u{2068}']).next().unwrap_or_default();
            self.contains(id) || (!head.is_empty() && head != id && self.contains(head))
        }
    }

    const MAIN: &str = "package main\n\nfunc a() {}\n\nfunc b() {}\n\nfunc c() {}\n";

    struct Fixture {
        registry: Registry,
        session: Session,
        main: PathBuf,
        other: PathBuf,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let (main, other) = (dir.path().join("main.go"), dir.path().join("other.go"));
        std::fs::write(&main, MAIN).unwrap();
        std::fs::write(&other, "package main\n\nvar x = 1\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        Fixture { registry, session, main, other, _dir: dir }
    }

    impl Fixture {
        fn run(&mut self, command: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, command, params).unwrap()
        }

        fn fail(&mut self, command: &str, params: Value) -> CommandError {
            match self.registry.dispatch(&mut self.session, command, params) {
                Err(zen_core::DispatchError::Failed { error, .. }) => error,
                other => panic!("{command} did not fail: {other:?}"),
            }
        }

        fn caret_at_line(&mut self, line: usize) {
            let document = self.session.state_mut::<Workbench>().active_document_mut().unwrap();
            let offset = document.snapshot().point_to_offset(zen_text::Point::new(line, 0)).unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(offset)));
        }

        fn listed(&mut self) -> Value {
            self.run("navigate.bookmark.list", json!({}))["bookmarks"].clone()
        }

        fn caret_line(&self) -> u32 {
            crate::history::caret(&self.session).unwrap().line
        }
    }

    #[test]
    fn bookmark_lines_count_from_one_as_every_place_an_agent_sees() {
        let mut f = fixture();
        f.run("file.document.open", json!({ "path": f.main }));
        let found = f.run("search.text.find", json!({ "query": { "text": "func a" } }));
        let line = found["matches"][0]["line"].clone();
        assert_eq!(line, 3);
        let set = f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": line }));
        assert_eq!(set["bookmark"]["line"], 3, "{set}");
        assert_eq!(f.listed()[0]["line"], 3);
        f.run("navigate.bookmark.open", json!({ "path": f.main, "line": 3 }));
        assert_eq!(f.caret_line(), 3, "the bookmark is on func a");
        f.caret_at_line(4);
        assert_eq!(f.run("navigate.bookmark.toggle", json!({}))["bookmark"]["line"], 5, "the caret's line, from 1");
        let refused = f.fail("navigate.bookmark.toggle", json!({ "path": f.main, "line": 0 }));
        assert!(refused.message.mentions("bookmark-line-from-one"), "{}", refused.message);
    }

    #[test]
    fn toggle_sets_an_anonymous_bookmark_at_the_caret_and_removes_it() {
        let mut f = fixture();
        f.run("file.document.open", json!({ "path": f.main }));
        f.caret_at_line(2);
        let set = f.run("navigate.bookmark.toggle", json!({}));
        assert_eq!(set["bookmark"], json!({ "path": f.main, "line": 3 }));
        let listed = f.run("navigate.bookmark.list", json!({}));
        assert_eq!(listed["bookmarks"], json!([{ "path": f.main, "line": 3 }]));
        assert_eq!(listed["defaultList"], json!(f._dir.path().file_name().unwrap().to_string_lossy()));
        assert_eq!(f.run("navigate.bookmark.toggle", json!({}))["bookmark"], Value::Null);
        assert_eq!(f.listed(), json!([]));
    }

    #[test]
    fn a_taken_mnemonic_is_refused_unless_replaced_and_then_moves() {
        let mut f = fixture();
        let set = f.run(
            "navigate.bookmark.setMnemonic",
            json!({ "path": f.main, "line": 3, "mnemonic": "a", "description": " start " }),
        );
        assert_eq!(set["bookmark"], json!({ "path": f.main, "line": 3, "mnemonic": "A", "description": "start" }));
        let taken = f.fail("navigate.bookmark.setMnemonic", json!({ "path": f.other, "line": 3, "mnemonic": "A" }));
        assert_eq!(taken.code.as_deref(), Some(MNEMONIC_TAKEN));
        assert_eq!(taken.data.unwrap()["line"], 3);
        f.run("navigate.bookmark.setMnemonic", json!({ "path": f.other, "line": 3, "mnemonic": "A", "replace": true }));
        assert_eq!(
            f.listed(),
            json!([{ "path": f.other, "line": 3, "mnemonic": "A" }]),
            "the mnemonic moved with its bookmark"
        );
        assert!(
            f.fail("navigate.bookmark.setMnemonic", json!({ "line": 2, "mnemonic": "!", "path": f.main }))
                .message
                .mentions("bookmark-bad-mnemonic")
        );
        f.run("navigate.bookmark.setMnemonic", json!({ "path": f.other, "line": 3, "mnemonic": "B" }));
        assert_eq!(
            f.listed(),
            json!([{ "path": f.other, "line": 3, "mnemonic": "B" }]),
            "the line's own mnemonic changes"
        );
    }

    #[test]
    fn digits_toggle_without_asking_and_jumps_record_back() {
        let mut f = fixture();
        f.run("file.document.open", json!({ "path": f.main }));
        f.caret_at_line(4);
        f.run("navigate.bookmark.toggleMnemonic", json!({ "mnemonic": "1" }));
        f.run("file.document.open", json!({ "path": f.other }));
        f.caret_at_line(2);
        let toggled = f.run("navigate.bookmark.toggleMnemonic", json!({ "mnemonic": "1" }));
        assert_eq!(toggled["bookmark"], json!({ "path": f.other, "line": 3, "mnemonic": "1" }));
        assert_eq!(f.listed().as_array().unwrap().len(), 1, "the digit moved without a question");
        f.run("navigate.bookmark.toggleMnemonic", json!({ "path": f.main, "line": 5, "mnemonic": "2" }));
        f.caret_at_line(0);
        let went = f.run("navigate.bookmark.goTo", json!({ "mnemonic": "2" }));
        assert_eq!(went["path"], json!(f.main));
        assert_eq!(went["line"], 5);
        let back = f.run("navigate.history.list", json!(null))["back"].clone();
        let left = back.as_array().unwrap().last().cloned().unwrap();
        assert_eq!(left, json!({ "path": f.other, "offset": 0, "line": 1 }), "the place left is recorded for Back");
        assert!(f.fail("navigate.bookmark.goTo", json!({ "mnemonic": "9" })).message.mentions("bookmark-no-mnemonic"));
        assert_eq!(f.run("navigate.bookmark.toggleMnemonic", json!({ "mnemonic": "2" }))["bookmark"], Value::Null);
    }

    #[test]
    fn next_and_previous_go_round_every_file() {
        let mut f = fixture();
        assert!(!f.registry.is_available(&f.session, "navigate.bookmark.next"));
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 3 }));
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 7 }));
        f.run("navigate.bookmark.toggle", json!({ "path": f.other, "line": 3 }));
        f.run("file.document.open", json!({ "path": f.main }));
        f.caret_at_line(3);
        assert_eq!(f.run("navigate.bookmark.next", json!(null))["line"], 7);
        let went = f.run("navigate.bookmark.next", json!(null));
        assert_eq!((went["path"].clone(), went["line"].clone()), (json!(f.other), json!(3)));
        assert_eq!(f.run("navigate.bookmark.next", json!(null))["line"], 3, "round to the first");
        assert_eq!(f.caret_line(), 3);
        let went = f.run("navigate.bookmark.previous", json!(null));
        assert_eq!((went["path"].clone(), went["line"].clone()), (json!(f.other), json!(3)), "round to the last");
    }

    #[test]
    fn bookmarks_move_with_edits_and_round_trip() {
        let mut f = fixture();
        f.run("file.document.open", json!({ "path": f.main }));
        f.run("navigate.bookmark.setMnemonic", json!({ "line": 5, "mnemonic": "7", "description": "b" }));
        f.run("navigate.bookmark.toggle", json!({ "line": 7 }));
        f.run("navigate.bookmark.rename", json!({ "line": 7, "description": "c" }));
        f.run("navigate.bookmark.moveToList", json!({ "line": 7, "list": "Later" }));
        let edit = |f: &mut Fixture, range: std::ops::Range<usize>, text: &str| {
            let document = f.session.state_mut::<Workbench>().active_document_mut().unwrap();
            assert!(document.replace_ranges(vec![(range, text.to_owned())]));
        };
        edit(&mut f, 0..0, "// top\n");
        let lines: Vec<u64> = f.listed().as_array().unwrap().iter().map(|b| b["line"].as_u64().unwrap()).collect();
        assert_eq!(lines, [6, 8]);
        let start = MAIN.find("func b").unwrap() + "// top\n".len();
        edit(&mut f, start..start + "func b() {}\n".len(), "");
        assert_eq!(
            f.listed(),
            json!([{ "path": f.main, "line": 7, "description": "c", "list": "Later" }]),
            "a deleted line takes its bookmark"
        );
        assert_eq!(f.run("navigate.bookmark.list", json!({}))["lists"], json!(["Later"]));
        f.run("navigate.bookmark.setMnemonic", json!({ "line": 7, "mnemonic": "Z" }));
        f.run("navigate.bookmark.removeMnemonic", json!({ "line": 7 }));
        let missing = f.fail("navigate.bookmark.rename", json!({ "line": 2, "description": "x" }));
        assert!(
            missing.message.mentions("bookmark-none") || missing.message.contains("main.go:2"),
            "{}",
            missing.message
        );

        let saved = f.session.state::<Bookmarks>().unwrap().list(None);
        let mut other = Bookmarks::default();
        let lists = f.session.state::<Bookmarks>().unwrap().lists().to_vec();
        other.restore(serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap(), lists);
        assert_eq!(other.list(None), saved);
        f.run("navigate.bookmark.remove", json!({ "line": 7 }));
        assert_eq!(f.listed(), json!([]));
        assert_eq!(
            f.run("navigate.bookmark.options", json!({})),
            json!({ "replaceWithoutAsking": false, "groupByFile": false })
        );
        let options = f.run("navigate.bookmark.options", json!({ "replaceWithoutAsking": true, "groupByFile": true }));
        assert_eq!(options, json!({ "replaceWithoutAsking": true, "groupByFile": true }));
    }

    #[test]
    fn bookmarks_follow_moved_files_and_reloads_and_leave_with_deleted_files_and_the_project() {
        let mut f = fixture();
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 5 }));
        f.run("navigate.bookmark.toggleMnemonic", json!({ "path": f.other, "line": 3, "mnemonic": "2" }));
        f.run("file.document.open", json!({ "path": f.main }));
        let app = PathBuf::from(f.run("file.fs.rename", json!({ "path": f.main, "name": "app.go" })).as_str().unwrap());
        assert_eq!(
            f.listed(),
            json!([{ "path": app, "line": 5 }, { "path": f.other, "line": 3, "mnemonic": "2" }]),
            "a renamed file keeps its bookmarks"
        );
        let folder = f._dir.path().join("pkg");
        std::fs::create_dir(&folder).unwrap();
        f.run("project.file.cut", json!({ "paths": [f.other], "clipboard": false }));
        let pasted = f.run("project.file.paste", json!({ "directory": folder, "clipboard": false }));
        let moved = PathBuf::from(pasted["paths"][0].as_str().unwrap());
        assert_eq!(f.listed()[1], json!({ "path": moved, "line": 3, "mnemonic": "2" }), "so does a moved one");
        let renamed =
            PathBuf::from(f.run("file.fs.rename", json!({ "path": folder, "name": "lib" })).as_str().unwrap());
        assert_eq!(f.listed()[1]["path"], json!(renamed.join("other.go")), "and a file of a renamed folder");

        std::fs::write(&app, format!("// one\n// two\n{MAIN}")).unwrap();
        f.session.state_mut::<Workbench>().active_document_mut().unwrap().sync_with_disk();
        assert_eq!(f.listed()[0], json!({ "path": app, "line": 7 }), "a change made elsewhere moves the line");
        std::fs::write(&app, "package main\n").unwrap();
        f.session.state_mut::<Workbench>().active_document_mut().unwrap().sync_with_disk();
        assert_eq!(f.listed().as_array().unwrap().len(), 1, "a line removed elsewhere takes its bookmark");

        std::fs::remove_file(renamed.join("other.go")).unwrap();
        assert_eq!(f.listed(), json!([]), "a file deleted elsewhere loses its bookmarks");
        f.run("navigate.bookmark.toggle", json!({ "path": app, "line": 1 }));
        forget(&mut f.session, &[f._dir.path().to_owned()]);
        assert_eq!(f.listed(), json!([]), "deleted with its folder");
        f.run("navigate.bookmark.toggle", json!({ "path": app, "line": 1 }));
        let project = f._dir.path().to_owned();
        f.run("file.project.open", json!({ "path": project }));
        assert_eq!(f.listed(), json!([]), "another project starts without them");
    }

    #[test]
    fn lists_are_created_renamed_made_default_reordered_and_deleted_with_their_bookmarks() {
        let mut f = fixture();
        let project = f._dir.path().file_name().unwrap().to_string_lossy().into_owned();
        let groups = |f: &mut Fixture| {
            let listed = f.run("navigate.bookmark.list", json!({}));
            listed["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| {
                    let lines: Vec<String> = group["bookmarks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|b| format!("{}:{}", b["path"].as_str().unwrap().rsplit('/').next().unwrap(), b["line"]))
                        .collect();
                    let tag = if group["isDefault"].as_bool().unwrap() { "*" } else { "" };
                    format!("{tag}{} [{}]", group["name"].as_str().unwrap(), lines.join(" "))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(groups(&mut f), [format!("*{project} []")], "the project's list is there from the start");
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 7 }));
        f.run("navigate.bookmark.toggle", json!({ "path": f.other, "line": 1 }));
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 3 }));
        assert_eq!(groups(&mut f), [format!("*{project} [main.go:7 other.go:1 main.go:3]")], "in the order they came");
        assert_eq!(f.run("navigate.bookmarkList.create", json!({ "name": " Later " })), json!("Later"));
        assert!(
            f.fail("navigate.bookmarkList.create", json!({ "name": "Later" })).message.mentions("bookmark-list-taken")
        );
        assert!(
            f.fail("navigate.bookmarkList.create", json!({ "name": " " })).message.mentions("bookmark-list-empty-name")
        );
        f.run("navigate.bookmark.moveToList", json!({ "path": f.other, "line": 1, "list": "Later" }));
        f.run("navigate.bookmark.moveToList", json!({ "path": f.main, "line": 3, "list": "Later", "index": 0 }));
        assert_eq!(
            groups(&mut f),
            [format!("*{project} [main.go:7]"), "Later [main.go:3 other.go:1]".to_owned()],
            "dropped first in the list"
        );
        f.run("navigate.bookmark.moveDown", json!({ "path": f.main, "line": 3 }));
        assert_eq!(groups(&mut f)[1], "Later [other.go:1 main.go:3]");
        f.run("navigate.bookmark.moveDown", json!({ "path": f.main, "line": 3 }));
        assert_eq!(groups(&mut f)[1], "Later [other.go:1 main.go:3]", "the last one stays");
        f.run("navigate.bookmark.moveUp", json!({ "path": f.main, "line": 3 }));
        assert_eq!(groups(&mut f)[1], "Later [main.go:3 other.go:1]");

        assert_eq!(f.run("navigate.bookmarkList.setDefault", json!({ "name": "Later" })), json!("Later"));
        assert_eq!(groups(&mut f), ["*Later [main.go:3 other.go:1]".to_owned(), format!("{project} [main.go:7]")]);
        let listed = f.run("navigate.bookmark.list", json!({}));
        assert_eq!((listed["defaultList"].clone(), listed["lists"].clone()), (json!("Later"), json!([project])));
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 5 }));
        assert_eq!(groups(&mut f)[0], "*Later [main.go:3 other.go:1 main.go:5]", "a new bookmark goes to the default");
        assert_eq!(
            f.run("navigate.bookmark.list", json!({}))["bookmarks"][0],
            json!({ "path": f.main, "line": 3 }),
            "the default list is not named on its bookmarks"
        );

        f.run("navigate.bookmarkList.rename", json!({ "name": project, "newName": "Old" }));
        assert_eq!(groups(&mut f)[1], "Old [main.go:7]");
        assert!(
            f.fail("navigate.bookmarkList.rename", json!({ "name": "Old", "newName": "Later" }))
                .message
                .mentions("bookmark-list-taken")
        );
        assert!(
            f.fail("navigate.bookmarkList.rename", json!({ "name": "Gone", "newName": "X" }))
                .message
                .mentions("bookmark-list-missing")
        );
        let saved = f.session.state::<Bookmarks>().unwrap().saved();
        let lists = f.session.state::<Bookmarks>().unwrap().lists().to_vec();
        let mut restored = Bookmarks::default();
        restored.restore(saved.clone(), lists);
        assert_eq!(restored.saved(), saved, "the lists and their order survive a restart");
        assert_eq!(restored.lists(), ["Later", "Old"]);

        assert_eq!(f.run("navigate.bookmarkList.delete", json!({ "name": "Later" })), json!(3));
        assert_eq!(groups(&mut f), ["*Old [main.go:7]"], "its bookmarks left the code");
        assert_eq!(f.run("navigate.bookmark.list", json!({}))["bookmarks"].as_array().unwrap().len(), 1);
        f.run("navigate.bookmarkList.delete", json!({ "name": "Old" }));
        assert_eq!(groups(&mut f), [format!("*{project} []")], "with none left, the project's list is back");

        f.run("file.document.open", json!({ "path": f.other }));
        let went = f.run("navigate.bookmark.open", json!({ "path": f.main, "line": 5 }));
        assert_eq!((went["path"].clone(), went["line"].clone()), (json!(f.main), json!(5)));
        let back = f.run("navigate.history.list", json!(null))["back"].clone();
        assert_eq!(back.as_array().unwrap().last().unwrap()["path"], json!(f.other), "Back returns");
    }

    #[test]
    fn files_and_folders_are_bookmarked_beside_their_lines_and_open_tabs_make_a_list() {
        let mut f = fixture();
        let folder = f._dir.path().join("pkg");
        std::fs::create_dir(&folder).unwrap();
        f.run("file.document.open", json!({ "path": f.main }));
        f.run("navigate.bookmark.toggle", json!({ "path": f.main, "line": 3 }));
        let set = f.run("navigate.bookmark.toggleFiles", json!({ "paths": [f.main, folder] }));
        assert_eq!(set["bookmarks"], json!([{ "path": f.main }, { "path": folder, "folder": true }]));
        assert_eq!(
            f.listed(),
            json!([{ "path": f.main }, { "path": f.main, "line": 3 }, { "path": folder, "folder": true }]),
            "a file's own bookmark first, then its lines"
        );
        let edit = |f: &mut Fixture| {
            let document = f.session.state_mut::<Workbench>().active_document_mut().unwrap();
            assert!(document.replace_ranges(vec![(0..0, "// top\n".to_owned())]));
        };
        edit(&mut f);
        assert_eq!(f.listed()[1], json!({ "path": f.main, "line": 4 }), "the line moves, the file's stays");
        assert_eq!(f.listed()[0], json!({ "path": f.main }));

        let mixed = f.run("navigate.bookmark.toggleFiles", json!({ "paths": [f.main, f.other] }));
        assert_eq!(mixed["bookmarks"], json!([{ "path": f.main }, { "path": f.other }]), "one without: all set");
        let removed = f.run("navigate.bookmark.toggleFiles", json!({ "paths": [f.main, f.other] }));
        assert_eq!(removed["bookmarks"], json!([]), "all with one: all removed");
        assert_eq!(f.listed().as_array().unwrap().len(), 2, "the line and the folder stay");

        f.run("navigate.bookmark.setMnemonic", json!({ "path": f.other, "whole": true, "mnemonic": "f" }));
        let taken = f.fail("navigate.bookmark.setMnemonic", json!({ "path": f.main, "line": 1, "mnemonic": "F" }));
        assert_eq!(taken.code.as_deref(), Some(MNEMONIC_TAKEN));
        let place = taken.data.unwrap();
        assert_eq!((place.get("path"), place.get("line")), (Some(&json!(f.other)), None), "the file's own");
        f.run("navigate.bookmark.rename", json!({ "path": f.other, "whole": true, "description": "vars" }));
        assert_eq!(
            f.run("navigate.bookmark.list", json!({ "path": f.other }))["bookmarks"],
            json!([{ "path": f.other, "mnemonic": "F", "description": "vars" }])
        );
        let went = f.run("navigate.bookmark.goTo", json!({ "mnemonic": "f" }));
        assert_eq!((went["path"].clone(), went["line"].clone()), (json!(f.other), json!(1)), "⌃F opens the file");
        let back = f.run("navigate.history.list", json!(null))["back"].clone();
        assert_eq!(back.as_array().unwrap().last().unwrap()["path"], json!(f.main), "and records Back");
        f.run("navigate.bookmark.setMnemonic", json!({ "path": folder, "whole": true, "mnemonic": "7" }));
        let refused = f.fail("navigate.bookmark.goTo", json!({ "mnemonic": "7" }));
        assert_eq!(refused.code.as_deref(), Some(FOLDER_BOOKMARK));
        assert_eq!(refused.data.unwrap()["path"], json!(folder));
        let opened = f.run("navigate.bookmark.open", json!({ "path": f.main }));
        assert_eq!(opened["path"], json!(f.main));

        assert_eq!(f.run("navigate.bookmark.removeFiles", json!({ "paths": [folder, f.main] })), json!(1));
        assert!(f.fail("navigate.bookmark.removeFiles", json!({ "paths": [] })).message.mentions("bookmark-no-items"));
        let gone = f._dir.path().join("gone.go");
        let refused = f.fail("navigate.bookmark.toggleFiles", json!({ "paths": [gone] }));
        assert_eq!(refused.message, t!("bookmark-no-item", path = crate::display(&gone)));
        f.run("navigate.bookmark.remove", json!({ "path": f.other, "whole": true }));
        assert_eq!(f.listed(), json!([{ "path": f.main, "line": 4 }]));

        f.run("navigate.bookmark.toggleFiles", json!({ "paths": [f.main] }));
        f.run("file.document.open", json!({ "path": f.other }));
        let tabs = f.run("navigate.bookmarkList.fromOpenTabs", json!({ "name": "Tabs" }));
        assert_eq!(
            tabs,
            json!({ "name": "Tabs", "isDefault": false, "bookmarks": [{ "path": f.main }, { "path": f.other }] }),
            "every open tab, the bookmarked file moved in"
        );
        let project = f._dir.path().file_name().unwrap().to_string_lossy().into_owned();
        let groups = f.run("navigate.bookmark.list", json!({}))["groups"].clone();
        assert_eq!(groups[0]["name"], json!(project));
        assert_eq!(groups[0]["bookmarks"], json!([{ "path": f.main, "line": 4 }]));
        assert!(
            f.fail("navigate.bookmarkList.fromOpenTabs", json!({ "name": "Tabs" }))
                .message
                .mentions("bookmark-list-taken")
        );

        f.run("navigate.bookmark.toggleFiles", json!({ "paths": [folder] }));
        let renamed =
            PathBuf::from(f.run("file.fs.rename", json!({ "path": folder, "name": "lib" })).as_str().unwrap());
        let listed = f.listed();
        assert!(listed.as_array().unwrap().contains(&json!({ "path": renamed, "folder": true })), "{listed}");
        std::fs::remove_dir(&renamed).unwrap();
        let listed = f.listed();
        assert!(!listed.to_string().contains("\"folder\""), "a folder deleted elsewhere loses it: {listed}");
        assert!(f.registry.is_available(&f.session, "navigate.bookmark.next"));
        let saved = f.session.state::<Bookmarks>().unwrap().saved();
        let lists = f.session.state::<Bookmarks>().unwrap().lists().to_vec();
        let mut restored = Bookmarks::default();
        restored.restore(serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap(), lists);
        assert_eq!(restored.saved(), saved, "file bookmarks survive a restart");
    }

    #[test]
    fn a_workspace_file_written_before_lists_keeps_the_project_list_as_the_default() {
        let place = |line: u32, list: Option<&str>| BookmarkInfo {
            path: PathBuf::from("/p/a.go"),
            line: Some(line),
            folder: false,
            mnemonic: None,
            description: String::new(),
            list: list.map(str::to_owned),
        };
        let mut bookmarks = Bookmarks { project: "shop".to_owned(), ..Bookmarks::default() };
        bookmarks.restore(vec![place(1, Some("Zed")), place(2, Some("Later")), place(3, None)], Vec::new());
        assert_eq!(bookmarks.lists(), ["shop", "Later", "Zed"]);
        assert_eq!(bookmarks.in_list("shop").len(), 1);
        assert_eq!(bookmarks.list(None)[0].list.as_deref(), Some("Zed"));
        assert_eq!(bookmarks.list(None)[2].list, None);
        let saved = bookmarks.saved();
        assert_eq!(saved[0].list, None, "an older build reads the default list's bookmarks as its own default ones");
        assert_eq!(saved[1].list.as_deref(), Some("Later"));
    }

    mod properties {
        use std::collections::BTreeMap;

        use proptest::prelude::*;

        use super::*;

        type Place = (PathBuf, Option<u32>);

        #[derive(Clone, Debug)]
        enum Op {
            Toggle(u8, Option<u32>),
            Mnemonic(u8, Option<u32>, char),
            Remove(u8, Option<u32>),
            MoveToList(u8, Option<u32>, Option<usize>, Option<usize>),
            MoveByOne(u8, Option<u32>, bool),
            ToggleItems(Vec<u8>),
            RemoveItems(Vec<u8>),
            BookmarkItems(usize, Vec<u8>),
            Create(usize),
            Rename(usize, usize),
            SetDefault(usize),
            Delete(usize),
            Restart,
        }

        const NAMES: [&str; 5] = ["shop", "A", "B", " A ", ""];

        fn path(file: u8) -> PathBuf {
            PathBuf::from(format!("/p/{file}.go"))
        }

        fn op() -> impl Strategy<Value = Op> {
            let file = 0u8..3;
            let line = prop::option::weighted(0.75, 0u32..4);
            let name = 0usize..NAMES.len();
            let files = proptest::collection::vec(0u8..3, 0..4);
            prop_oneof![
                3 => (file.clone(), line.clone()).prop_map(|(f, l)| Op::Toggle(f, l)),
                2 => (file.clone(), line.clone(), prop::sample::select(vec!['1', '2', 'K']))
                    .prop_map(|(f, l, m)| Op::Mnemonic(f, l, m)),
                1 => (file.clone(), line.clone()).prop_map(|(f, l)| Op::Remove(f, l)),
                3 => (file.clone(), line.clone(), prop::option::of(name.clone()), prop::option::of(0usize..6))
                    .prop_map(|(f, l, n, i)| Op::MoveToList(f, l, n, i)),
                2 => (file, line, any::<bool>()).prop_map(|(f, l, u)| Op::MoveByOne(f, l, u)),
                2 => files.clone().prop_map(Op::ToggleItems),
                1 => files.clone().prop_map(Op::RemoveItems),
                1 => (name.clone(), files).prop_map(|(n, f)| Op::BookmarkItems(n, f)),
                1 => name.clone().prop_map(Op::Create),
                1 => (name.clone(), name.clone()).prop_map(|(a, b)| Op::Rename(a, b)),
                1 => name.clone().prop_map(Op::SetDefault),
                1 => name.prop_map(Op::Delete),
                1 => Just(Op::Restart),
            ]
        }

        #[derive(Default)]
        struct Model {
            lists: Vec<(String, Vec<Place>)>,
            mnemonics: BTreeMap<Place, char>,
        }

        impl Model {
            fn find(&self, place: &Place) -> Option<(usize, usize)> {
                self.lists.iter().enumerate().find_map(|(at, (_, places))| {
                    places.iter().position(|found| found == place).map(|index| (at, index))
                })
            }

            fn take(&mut self, place: &Place) -> bool {
                let Some((list, index)) = self.find(place) else { return false };
                self.lists[list].1.remove(index);
                self.mnemonics.remove(place);
                true
            }

            fn list(&mut self, name: &str) -> usize {
                let name = name.trim();
                let name = if name.is_empty() {
                    self.lists.first().map_or_else(|| "shop".to_owned(), |(name, _)| name.clone())
                } else {
                    name.to_owned()
                };
                match self.lists.iter().position(|(found, _)| *found == name) {
                    Some(at) => at,
                    None => {
                        self.lists.push((name, Vec::new()));
                        self.lists.len() - 1
                    }
                }
            }

            fn add(&mut self, place: Place) {
                let list = self.list("");
                self.lists[list].1.push(place);
            }

            fn named(&self, name: &str) -> Option<usize> {
                self.lists.iter().position(|(found, _)| found == name.trim())
            }
        }

        fn apply(bookmarks: &mut Bookmarks, model: &mut Model, op: Op) {
            match op {
                Op::Toggle(file, line) => {
                    let place = (path(file), line);
                    bookmarks.toggle(&place.0, line);
                    if !model.take(&place) {
                        model.add(place);
                    }
                }
                Op::Mnemonic(file, line, mnemonic) => {
                    let place = (path(file), line);
                    bookmarks.set_mnemonic(&place.0, line, mnemonic, None);
                    let other = model.mnemonics.iter().find(|(_, m)| **m == mnemonic).map(|(p, _)| p.clone());
                    if let Some(other) = other.filter(|other| *other != place) {
                        model.take(&other);
                    }
                    if model.find(&place).is_none() {
                        model.add(place.clone());
                    }
                    model.mnemonics.insert(place, mnemonic);
                }
                Op::Remove(file, line) => {
                    let place = (path(file), line);
                    assert_eq!(bookmarks.remove(&place.0, line), model.take(&place));
                }
                Op::MoveToList(file, line, name, index) => {
                    let place = (path(file), line);
                    let name = name.map(|name| NAMES[name].to_owned());
                    let moved = bookmarks.move_to_list(&place.0, line, name.clone(), index);
                    assert_eq!(moved.is_some(), model.find(&place).is_some());
                    if moved.is_some() {
                        let (list, at) = model.find(&place).unwrap();
                        model.lists[list].1.remove(at);
                        let target = model.list(name.as_deref().unwrap_or_default());
                        let places = &mut model.lists[target].1;
                        places.insert(index.unwrap_or(places.len()).min(places.len()), place);
                    }
                }
                Op::MoveByOne(file, line, up) => {
                    let place = (path(file), line);
                    let moved = bookmarks.move_by_one(&place.0, line, up);
                    assert_eq!(moved.is_some(), model.find(&place).is_some());
                    if let Some((list, at)) = model.find(&place) {
                        let places = &mut model.lists[list].1;
                        let other = if up { at.checked_sub(1) } else { Some(at + 1).filter(|o| *o < places.len()) };
                        if let Some(other) = other {
                            places.swap(at, other);
                        }
                    }
                }
                Op::ToggleItems(files) => {
                    let mut places: Vec<Place> = Vec::new();
                    for file in files {
                        if !places.contains(&(path(file), None)) {
                            places.push((path(file), None));
                        }
                    }
                    let paths: Vec<PathBuf> = places.iter().map(|place| place.0.clone()).collect();
                    let all = !places.is_empty() && places.iter().all(|place| model.find(place).is_some());
                    assert_eq!(bookmarks.toggle_items(&paths), !all);
                    for place in places {
                        if all {
                            model.take(&place);
                        } else if model.find(&place).is_none() {
                            model.add(place);
                        }
                    }
                }
                Op::RemoveItems(files) => {
                    let paths: Vec<PathBuf> = files.into_iter().map(path).collect();
                    let removed = paths.iter().filter(|path| model.take(&((*path).clone(), None))).count();
                    assert_eq!(bookmarks.remove_items(&paths), removed);
                }
                Op::BookmarkItems(name, files) => {
                    let paths: Vec<PathBuf> = files.into_iter().map(path).collect();
                    let created = bookmarks.bookmark_items(NAMES[name], &paths);
                    let trimmed = NAMES[name].trim();
                    let ok = !trimmed.is_empty() && model.named(trimmed).is_none();
                    assert_eq!(created.is_ok(), ok);
                    if ok {
                        let mut places: Vec<Place> = Vec::new();
                        for path in paths {
                            let place = (path, None);
                            if places.contains(&place) {
                                continue;
                            }
                            if let Some((list, at)) = model.find(&place) {
                                model.lists[list].1.remove(at);
                            }
                            places.push(place);
                        }
                        model.lists.push((trimmed.to_owned(), places));
                    }
                }
                Op::Create(name) => {
                    let created = bookmarks.create_list(NAMES[name]);
                    let name = NAMES[name].trim();
                    let ok = !name.is_empty() && model.named(name).is_none();
                    assert_eq!(created.is_ok(), ok);
                    if ok {
                        model.lists.push((name.to_owned(), Vec::new()));
                    }
                }
                Op::Rename(from, to) => {
                    let renamed = bookmarks.rename_list(NAMES[from], NAMES[to]);
                    let (from, to) = (NAMES[from].trim(), NAMES[to].trim());
                    let ok = !from.is_empty()
                        && model.named(from).is_some()
                        && !to.is_empty()
                        && (to == from || model.named(to).is_none());
                    assert_eq!(renamed.is_ok(), ok);
                    if ok {
                        let at = model.named(from).unwrap();
                        model.lists[at].0 = to.to_owned();
                    }
                }
                Op::SetDefault(name) => {
                    let set = bookmarks.set_default_list(NAMES[name]);
                    assert_eq!(
                        set.is_ok(),
                        model.named(NAMES[name]).filter(|_| !NAMES[name].trim().is_empty()).is_some()
                    );
                    if set.is_ok() {
                        let at = model.named(NAMES[name]).unwrap();
                        let list = model.lists.remove(at);
                        model.lists.insert(0, list);
                    }
                }
                Op::Delete(name) => {
                    let deleted = bookmarks.delete_list(NAMES[name]);
                    let at = model.named(NAMES[name]).filter(|_| !NAMES[name].trim().is_empty());
                    assert_eq!(deleted.is_ok(), at.is_some());
                    if let Some(at) = at {
                        let (_, places) = model.lists.remove(at);
                        assert_eq!(deleted.unwrap(), places.len(), "only the list's bookmarks leave the code");
                        for place in places {
                            model.mnemonics.remove(&place);
                        }
                    }
                }
                Op::Restart => {
                    let saved = serde_json::to_string(&bookmarks.saved()).unwrap();
                    let lists = serde_json::to_string(bookmarks.lists()).unwrap();
                    *bookmarks = Bookmarks { project: "shop".to_owned(), ..Bookmarks::default() };
                    bookmarks.restore(serde_json::from_str(&saved).unwrap(), serde_json::from_str(&lists).unwrap());
                    if model.lists.is_empty() {
                        model.lists.push(("shop".to_owned(), Vec::new()));
                    }
                }
            }
        }

        fn check(bookmarks: &Bookmarks, model: &Model) {
            let names: Vec<&str> = model.lists.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(bookmarks.lists(), names.as_slice());
            for (name, places) in &model.lists {
                let found: Vec<Place> =
                    bookmarks.in_list(name).into_iter().map(|info| (info.path, info.line)).collect();
                assert_eq!(&found, places, "the order of {name}");
            }
            let all = bookmarks.list(None);
            assert_eq!(
                all.len(),
                model.lists.iter().map(|(_, places)| places.len()).sum::<usize>(),
                "none lost or doubled"
            );
            for info in &all {
                let place = (info.path.clone(), info.line);
                assert_eq!(info.mnemonic, model.mnemonics.get(&place).copied(), "the mnemonic of {place:?}");
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(512))]

            #[test]
            fn moves_renames_deletes_and_restarts_never_lose_or_double_a_bookmark(
                ops in proptest::collection::vec(op(), 1..60)
            ) {
                let mut bookmarks = Bookmarks { project: "shop".to_owned(), ..Bookmarks::default() };
                bookmarks.lists.push("shop".to_owned());
                let mut model = Model { lists: vec![("shop".to_owned(), Vec::new())], ..Model::default() };
                for op in ops {
                    apply(&mut bookmarks, &mut model, op);
                    check(&bookmarks, &model);
                }
            }
        }
    }

    #[test]
    fn lines_follow_a_whole_text_change_by_its_diff() {
        let before = "a\nb\nc\nd\n";
        let lines = [Some(0), Some(1), Some(2), Some(3), None];
        assert_eq!(follow_lines(before, "x\na\nb\nc\nd\n", &lines), [Some(1), Some(2), Some(3), Some(4), None]);
        assert_eq!(follow_lines(before, "a\nc\nd\n", &lines), [Some(0), None, Some(1), Some(2), None]);
        assert_eq!(follow_lines(before, "a\nB\nc\nd\n", &lines), [Some(0), Some(1), Some(2), Some(3), None]);
    }

    #[test]
    fn mnemonics_are_digits_and_capital_letters() {
        assert_eq!(mnemonic_of("q"), Some('Q'));
        assert_eq!(mnemonic_of("0"), Some('0'));
        assert_eq!(mnemonic_of("ab"), None);
        assert_eq!(mnemonic_of("ж"), None);
        assert_eq!(MNEMONIC_ROWS.concat().len(), 36);
    }
}
