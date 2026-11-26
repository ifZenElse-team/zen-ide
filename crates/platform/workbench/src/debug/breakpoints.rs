use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use zen_dap::{
    Capabilities, Configuration, DataBreakpoint, ExceptionBreakpointsFilter, ExceptionFilterOptions,
    FunctionBreakpoint, SetExceptionBreakpointsArguments, SourceBreakpoint,
};
use zen_i18n::t;
use zen_text::{Edit, Point};

use crate::Workbench;

/// What a breakpoint does to the program when it is hit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Suspend {
    /// The program does not stop: the hit is only logged.
    None,
    /// Every thread stops.
    #[default]
    All,
    /// Only the thread that hit it stops; the others run on.
    Thread,
}

/// What a breakpoint that waits for another one does after it stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AfterHit {
    /// It waits for the other breakpoint again.
    #[default]
    DisableAgain,
    /// It stays on.
    LeaveEnabled,
}

/// A line of a file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinePlace {
    pub path: PathBuf,
    /// The line, from 0.
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Breakpoint {
    pub line: u32,
    pub enabled: bool,
    pub condition: Option<String>,
    pub verified: Option<bool>,
    pub message: Option<String>,
    pub id: u64,
    pub suspend: Suspend,
    pub log_hit: bool,
    pub log_stack: bool,
    pub log_expression: Option<String>,
    pub remove_once_hit: bool,
    pub hit_condition: Option<String>,
    pub depends_on: Option<u64>,
    pub after_hit: AfterHit,
    pub group: Option<String>,
    pub description: Option<String>,
}

impl Breakpoint {
    pub fn is_logging(&self) -> bool {
        self.log_hit || self.log_stack || self.log_expression.is_some()
    }
}

/// A breakpoint with its file, as the commands give it and the workspace file keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointInfo {
    /// The file.
    pub path: PathBuf,
    /// The line, from 0.
    pub line: u32,
    /// Whether it stops the program.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Whether the debugger could set it; absent when no session runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// What the debugger says about it; absent when no session runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// What a hit does: `all` stops every thread, `thread` only its own, `none` logs and lets the program run on.
    #[serde(default, skip_serializing_if = "is_all")]
    pub suspend: Suspend,
    /// Whether a hit logs "Breakpoint reached at" its place to the console.
    #[serde(default, skip_serializing_if = "is_false")]
    pub log_hit: bool,
    /// Whether a hit logs the stack to the console.
    #[serde(default, skip_serializing_if = "is_false")]
    pub log_stack: bool,
    /// An expression evaluated at every hit, its value logged to the console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_expression: Option<String>,
    /// Whether it is removed once it stops the program (a temporary breakpoint).
    #[serde(default, skip_serializing_if = "is_false")]
    pub remove_once_hit: bool,
    /// How many hits it takes to stop (the pass count), as the debugger reads it; Delve takes `5`, `> 5` or `% 2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// The breakpoint whose hit turns this one on; it stays off until then in every session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<LinePlace>,
    /// What it does after it stops when it depends on another breakpoint.
    #[serde(default, skip_serializing_if = "is_disable_again")]
    pub after_hit: AfterHit,
    /// The user's group it is shown in by View Breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The user's description of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether it still waits for `dependsOn` to be hit in the running sessions.
    #[serde(default, skip_serializing_if = "is_false")]
    pub waiting: bool,
}

/// A breakpoint on a function, by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpointInfo {
    /// The function's name as the debugger reads it, such as `main.run`.
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits it takes to stop, as the debugger reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Whether the debugger could set it; absent when no session runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// What the debugger says about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// An exception breakpoint: a filter of the debugger turned on or off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpointInfo {
    /// The debugger's id of the filter, such as `uncaught`.
    pub filter: String,
    /// Its name, such as `Uncaught Exceptions`.
    pub label: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// An expression that must be true for it to stop, when the debugger takes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// A data breakpoint (a watchpoint) of the running sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointInfo {
    /// The debugger's id of the watched data.
    pub data_id: String,
    /// What is watched, as the debugger says it.
    pub description: String,
    /// The access it stops on: `read`, `write` or `readWrite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_type: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits it takes to stop, as the debugger reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Whether the debugger could set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}

/// Changes to a line breakpoint; what is absent stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BreakpointChange {
    /// Whether it stops the program; on for a new breakpoint, unchanged for one that exists, when absent.
    pub enabled: Option<bool>,
    /// An expression that must be true for it to stop; cleared when empty.
    pub condition: Option<String>,
    /// What a hit does: `all`, `thread` or `none` (log only).
    pub suspend: Option<Suspend>,
    /// Whether a hit logs "Breakpoint reached at" its place.
    pub log_hit: Option<bool>,
    /// Whether a hit logs the stack.
    pub log_stack: Option<bool>,
    /// An expression evaluated and logged at every hit; cleared when empty.
    pub log_expression: Option<String>,
    /// Whether it is removed once it stops the program.
    pub remove_once_hit: Option<bool>,
    /// How many hits it takes to stop (the pass count); cleared when empty.
    pub hit_condition: Option<String>,
    /// The breakpoint whose hit turns this one on; `null` makes it independent.
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Option<LinePlace>")]
    pub depends_on: Option<Option<LinePlace>>,
    /// What it does after it stops when it depends on another breakpoint.
    pub after_hit: Option<AfterHit>,
    /// The group View Breakpoints shows it in; cleared when empty.
    pub group: Option<String>,
    /// The user's description; cleared when empty.
    pub description: Option<String>,
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

fn enabled() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_all(suspend: &Suspend) -> bool {
    *suspend == Suspend::All
}

fn is_disable_again(after: &AfterHit) -> bool {
    *after == AfterHit::DisableAgain
}

fn text(value: String) -> Option<String> {
    Some(value).filter(|value| !value.trim().is_empty())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) line: u32,
    pub(crate) suspend: Suspend,
    pub(crate) log_hit: bool,
    pub(crate) log_stack: bool,
    pub(crate) evaluate: Option<String>,
}

pub fn hit_message(path: &Path, line: u32) -> String {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    t!("debug-log-hit", place = format!("{name}:{}", line + 1))
}

#[derive(Clone, Debug, Default)]
pub struct Breakpoints {
    files: BTreeMap<PathBuf, Vec<Breakpoint>>,
    revision: u64,
    next_id: u64,
    armed: BTreeSet<u64>,
    muted: bool,
    functions: Vec<FunctionBreakpointInfo>,
    exceptions: Vec<ExceptionBreakpointInfo>,
    known: Vec<ExceptionBreakpointsFilter>,
    data: Vec<DataBreakpointInfo>,
}

impl Breakpoints {
    pub fn exceptions(&self) -> Vec<String> {
        self.exceptions.iter().filter(|row| row.enabled).map(|row| row.filter.clone()).collect()
    }

    pub fn exception_rows(&self) -> &[ExceptionBreakpointInfo] {
        &self.exceptions
    }

    pub fn known_filters(&self) -> &[ExceptionBreakpointsFilter] {
        &self.known
    }

    pub fn set_exceptions(&mut self, filters: Vec<String>) {
        let mut changed = false;
        for row in &mut self.exceptions {
            let on = filters.contains(&row.filter);
            changed |= row.enabled != on;
            row.enabled = on;
        }
        for filter in filters {
            if !self.exceptions.iter().any(|row| row.filter == filter) {
                let label = self.label_of(&filter);
                self.exceptions.push(ExceptionBreakpointInfo { filter, label, enabled: true, condition: None });
                changed = true;
            }
        }
        if changed {
            self.revision += 1;
        }
    }

    fn label_of(&self, filter: &str) -> String {
        self.known
            .iter()
            .find(|known| known.filter == filter)
            .map_or_else(|| filter.to_owned(), |known| known.label.clone())
    }

    pub fn set_exception(
        &mut self,
        filter: &str,
        enabled: Option<bool>,
        condition: Option<String>,
    ) -> ExceptionBreakpointInfo {
        let label = self.label_of(filter);
        let index = match self.exceptions.iter().position(|row| row.filter == filter) {
            Some(index) => index,
            None => {
                self.exceptions.push(ExceptionBreakpointInfo {
                    filter: filter.to_owned(),
                    label,
                    enabled: true,
                    condition: None,
                });
                self.exceptions.len() - 1
            }
        };
        let row = &mut self.exceptions[index];
        if let Some(enabled) = enabled {
            row.enabled = enabled;
        }
        if let Some(condition) = condition {
            row.condition = text(condition);
        }
        self.revision += 1;
        row.clone()
    }

    pub fn remove_exception(&mut self, filter: &str) -> bool {
        let before = self.exceptions.len();
        self.exceptions.retain(|row| row.filter != filter);
        let removed = before != self.exceptions.len();
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub(crate) fn offer(&mut self, filters: &[ExceptionBreakpointsFilter]) {
        for filter in filters {
            match self.known.iter_mut().find(|known| known.filter == filter.filter) {
                Some(known) if known == filter => continue,
                Some(known) => *known = filter.clone(),
                None => self.known.push(filter.clone()),
            }
            for row in self.exceptions.iter_mut().filter(|row| row.filter == filter.filter) {
                row.label = filter.label.clone();
            }
            self.revision += 1;
        }
    }

    pub fn functions(&self) -> &[FunctionBreakpointInfo] {
        &self.functions
    }

    pub fn set_function(
        &mut self,
        name: &str,
        enabled: Option<bool>,
        condition: Option<String>,
        hit_condition: Option<String>,
    ) -> FunctionBreakpointInfo {
        let index = match self.functions.iter().position(|function| function.name == name) {
            Some(index) => index,
            None => {
                self.functions.push(FunctionBreakpointInfo {
                    name: name.to_owned(),
                    enabled: true,
                    condition: None,
                    hit_condition: None,
                    verified: None,
                    message: None,
                });
                self.functions.len() - 1
            }
        };
        let function = &mut self.functions[index];
        if let Some(enabled) = enabled {
            function.enabled = enabled;
        }
        if let Some(condition) = condition {
            function.condition = text(condition);
        }
        if let Some(hit) = hit_condition {
            function.hit_condition = text(hit);
        }
        self.revision += 1;
        function.clone()
    }

    pub fn remove_function(&mut self, name: &str) -> bool {
        let before = self.functions.len();
        self.functions.retain(|function| function.name != name);
        let removed = before != self.functions.len();
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub fn data(&self) -> &[DataBreakpointInfo] {
        &self.data
    }

    pub fn add_data(&mut self, breakpoint: DataBreakpointInfo) {
        self.data.retain(|kept| kept.data_id != breakpoint.data_id);
        self.data.push(breakpoint);
        self.revision += 1;
    }

    pub fn set_data(&mut self, data_id: &str, enabled: Option<bool>) -> Option<DataBreakpointInfo> {
        let breakpoint = self.data.iter_mut().find(|kept| kept.data_id == data_id)?;
        if let Some(enabled) = enabled {
            breakpoint.enabled = enabled;
        }
        let changed = breakpoint.clone();
        self.revision += 1;
        Some(changed)
    }

    pub fn remove_data(&mut self, data_id: &str) -> bool {
        let before = self.data.len();
        self.data.retain(|kept| kept.data_id != data_id);
        let removed = before != self.data.len();
        if removed {
            self.revision += 1;
        }
        removed
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn set_muted(&mut self, muted: bool) {
        if self.muted != muted {
            self.muted = muted;
            self.revision += 1;
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn for_path(&self, path: &Path) -> &[Breakpoint] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn get(&self, path: &Path, line: u32) -> Option<&Breakpoint> {
        self.for_path(path).iter().find(|breakpoint| breakpoint.line == line)
    }

    fn place_of(&self, id: u64) -> Option<LinePlace> {
        self.files.iter().find_map(|(path, breakpoints)| {
            breakpoints
                .iter()
                .find(|breakpoint| breakpoint.id == id)
                .map(|breakpoint| LinePlace { path: path.clone(), line: breakpoint.line })
        })
    }

    pub fn is_waiting(&self, breakpoint: &Breakpoint) -> bool {
        breakpoint.depends_on.is_some() && !self.armed.contains(&breakpoint.id)
    }

    fn info(&self, path: &Path, breakpoint: &Breakpoint) -> BreakpointInfo {
        BreakpointInfo {
            path: path.to_owned(),
            line: breakpoint.line,
            enabled: breakpoint.enabled,
            verified: breakpoint.verified,
            message: breakpoint.message.clone(),
            condition: breakpoint.condition.clone(),
            suspend: breakpoint.suspend,
            log_hit: breakpoint.log_hit,
            log_stack: breakpoint.log_stack,
            log_expression: breakpoint.log_expression.clone(),
            remove_once_hit: breakpoint.remove_once_hit,
            hit_condition: breakpoint.hit_condition.clone(),
            depends_on: breakpoint.depends_on.and_then(|id| self.place_of(id)),
            after_hit: breakpoint.after_hit,
            group: breakpoint.group.clone(),
            description: breakpoint.description.clone(),
            waiting: self.is_waiting(breakpoint),
        }
    }

    pub fn list(&self, path: Option<&Path>) -> Vec<BreakpointInfo> {
        self.files
            .iter()
            .filter(|(file, _)| path.is_none_or(|path| path == file.as_path()))
            .flat_map(|(file, breakpoints)| breakpoints.iter().map(|breakpoint| self.info(file, breakpoint)))
            .collect()
    }

    pub fn all(&self) -> Vec<BreakpointInfo> {
        self.list(None)
            .into_iter()
            .map(|info| BreakpointInfo { verified: None, message: None, waiting: false, ..info })
            .collect()
    }

    pub fn saved_functions(&self) -> Vec<FunctionBreakpointInfo> {
        self.functions
            .iter()
            .map(|function| FunctionBreakpointInfo { verified: None, message: None, ..function.clone() })
            .collect()
    }

    pub fn restore(&mut self, saved: Vec<BreakpointInfo>) {
        self.files.clear();
        self.armed.clear();
        let mut links = Vec::new();
        for info in saved {
            let id = self.fresh_id();
            if let Some(master) = info.depends_on.clone() {
                links.push((id, master));
            }
            self.insert(
                &info.path,
                Breakpoint {
                    line: info.line,
                    enabled: info.enabled,
                    condition: info.condition.and_then(text),
                    id,
                    suspend: info.suspend,
                    log_hit: info.log_hit,
                    log_stack: info.log_stack,
                    log_expression: info.log_expression.and_then(text),
                    remove_once_hit: info.remove_once_hit,
                    hit_condition: info.hit_condition.and_then(text),
                    depends_on: None,
                    after_hit: info.after_hit,
                    group: info.group.and_then(text),
                    description: info.description.and_then(text),
                    ..Breakpoint::default()
                },
            );
        }
        for (id, master) in links {
            let master = self.get(&master.path, master.line).map(|breakpoint| breakpoint.id);
            if let Some(breakpoint) = self.by_id_mut(id) {
                breakpoint.depends_on = master.filter(|master| *master != id);
            }
        }
        self.revision += 1;
    }

    pub fn restore_functions(&mut self, saved: Vec<FunctionBreakpointInfo>) {
        self.functions = saved;
        self.revision += 1;
    }

    pub fn restore_exceptions(&mut self, rows: Vec<ExceptionBreakpointInfo>, known: Vec<ExceptionBreakpointsFilter>) {
        self.exceptions = rows;
        self.known = known;
        self.revision += 1;
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn by_id_mut(&mut self, id: u64) -> Option<&mut Breakpoint> {
        self.files.values_mut().flatten().find(|breakpoint| breakpoint.id == id)
    }

    fn insert(&mut self, path: &Path, breakpoint: Breakpoint) {
        let breakpoints = self.files.entry(path.to_owned()).or_default();
        match breakpoints.iter_mut().find(|kept| kept.line == breakpoint.line) {
            Some(kept) => *kept = breakpoint,
            None => {
                breakpoints.push(breakpoint);
                breakpoints.sort_by_key(|kept| kept.line);
            }
        }
    }

    pub fn set(&mut self, path: &Path, line: u32, change: BreakpointChange) -> BreakpointInfo {
        let mut breakpoint = match self.get(path, line) {
            Some(kept) => kept.clone(),
            None => Breakpoint { line, enabled: true, id: self.fresh_id(), ..Breakpoint::default() },
        };
        let BreakpointChange {
            enabled,
            condition,
            suspend,
            log_hit,
            log_stack,
            log_expression,
            remove_once_hit,
            hit_condition,
            depends_on,
            after_hit,
            group,
            description,
        } = change;
        if let Some(enabled) = enabled {
            breakpoint.enabled = enabled;
        }
        if let Some(condition) = condition {
            breakpoint.condition = text(condition);
        }
        if let Some(suspend) = suspend {
            breakpoint.suspend = suspend;
        }
        if let Some(log_hit) = log_hit {
            breakpoint.log_hit = log_hit;
        }
        if let Some(log_stack) = log_stack {
            breakpoint.log_stack = log_stack;
        }
        if let Some(expression) = log_expression {
            breakpoint.log_expression = text(expression);
        }
        if let Some(remove) = remove_once_hit {
            breakpoint.remove_once_hit = remove;
        }
        if let Some(hit) = hit_condition {
            breakpoint.hit_condition = text(hit);
        }
        if let Some(master) = depends_on {
            let master = master.and_then(|master| self.get(&master.path, master.line)).map(|master| master.id);
            breakpoint.depends_on = master.filter(|master| *master != breakpoint.id);
            self.armed.remove(&breakpoint.id);
        }
        if let Some(after) = after_hit {
            breakpoint.after_hit = after;
        }
        if let Some(group) = group {
            breakpoint.group = text(group);
        }
        if let Some(description) = description {
            breakpoint.description = text(description);
        }
        self.insert(path, breakpoint.clone());
        self.revision += 1;
        self.info(path, &breakpoint)
    }

    pub fn remove(&mut self, path: &Path, line: u32) -> bool {
        let Some(breakpoints) = self.files.get_mut(path) else { return false };
        let gone: Vec<u64> =
            breakpoints.iter().filter(|breakpoint| breakpoint.line == line).map(|breakpoint| breakpoint.id).collect();
        breakpoints.retain(|breakpoint| breakpoint.line != line);
        if breakpoints.is_empty() {
            self.files.remove(path);
        }
        if gone.is_empty() {
            return false;
        }
        for breakpoint in self.files.values_mut().flatten() {
            if breakpoint.depends_on.is_some_and(|master| gone.contains(&master)) {
                breakpoint.depends_on = None;
            }
        }
        self.revision += 1;
        true
    }

    pub fn toggle(&mut self, path: &Path, line: u32) -> Option<BreakpointInfo> {
        if self.remove(path, line) { None } else { Some(self.set(path, line, BreakpointChange::default())) }
    }

    pub fn copy(&mut self, path: &Path, from: u32, to: u32) -> Option<BreakpointInfo> {
        if from == to {
            return None;
        }
        let mut copied = self.get(path, from)?.clone();
        copied.line = to;
        copied.id = self.fresh_id();
        copied.verified = None;
        copied.message = None;
        self.insert(path, copied.clone());
        self.revision += 1;
        Some(self.info(path, &copied))
    }

    pub fn others(&self, path: &Path, line: u32) -> Vec<LinePlace> {
        self.files
            .iter()
            .flat_map(|(file, breakpoints)| {
                breakpoints.iter().map(move |breakpoint| LinePlace { path: file.clone(), line: breakpoint.line })
            })
            .filter(|place| !(place.path == path && place.line == line))
            .collect()
    }

    pub(crate) fn hit(&mut self, path: &Path, line: u32) {
        let Some(breakpoint) = self.get(path, line).cloned() else { return };
        if breakpoint.remove_once_hit {
            self.remove(path, line);
        } else if breakpoint.depends_on.is_some() && breakpoint.after_hit == AfterHit::DisableAgain {
            self.armed.remove(&breakpoint.id);
        }
        let dependents: Vec<u64> = self
            .files
            .values()
            .flatten()
            .filter(|dependent| dependent.depends_on == Some(breakpoint.id))
            .map(|dependent| dependent.id)
            .collect();
        self.armed.extend(dependents);
        self.revision += 1;
    }

    pub(crate) fn end_sessions(&mut self) {
        let mut changed = !self.armed.is_empty() || !self.data.is_empty();
        self.armed.clear();
        self.data.clear();
        for function in &mut self.functions {
            changed |= function.verified.take().is_some() | function.message.take().is_some();
        }
        if changed {
            self.revision += 1;
        }
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = (&Path, &[Breakpoint])> {
        self.files.iter().map(|(path, breakpoints)| (path.as_path(), breakpoints.as_slice()))
    }

    pub(crate) fn breakpoint_mut(&mut self, path: &Path, line: u32) -> Option<&mut Breakpoint> {
        self.files.get_mut(path)?.iter_mut().find(|breakpoint| breakpoint.line == line)
    }

    pub fn move_to(&mut self, path: &Path, from: u32, to: u32) -> bool {
        if from == to || self.get(path, to).is_some() {
            return false;
        }
        let Some(breakpoint) = self.breakpoint_mut(path, from) else { return false };
        breakpoint.line = to;
        if let Some(breakpoints) = self.files.get_mut(path) {
            breakpoints.sort_by_key(|kept| kept.line);
        }
        self.revision += 1;
        true
    }

    pub(crate) fn set_status(&mut self, path: &Path, line: u32, verified: Option<bool>, message: Option<String>) {
        if let Some(breakpoint) = self.breakpoint_mut(path, line)
            && (breakpoint.verified != verified || breakpoint.message != message)
        {
            breakpoint.verified = verified;
            breakpoint.message = message;
            self.revision += 1;
        }
    }

    pub(crate) fn set_function_status(&mut self, statuses: &[(String, bool, Option<String>)]) {
        for (name, verified, message) in statuses {
            if let Some(function) = self.functions.iter_mut().find(|function| function.name == *name)
                && (function.verified != Some(*verified) || function.message != *message)
            {
                function.verified = Some(*verified);
                function.message.clone_from(message);
                self.revision += 1;
            }
        }
    }

    pub(crate) fn set_data_status(&mut self, statuses: &[(String, bool)]) {
        for (id, verified) in statuses {
            if let Some(data) = self.data.iter_mut().find(|data| data.data_id == *id)
                && data.verified != Some(*verified)
            {
                data.verified = Some(*verified);
                self.revision += 1;
            }
        }
    }

    pub(crate) fn clear_status(&mut self) {
        let mut changed = false;
        for breakpoint in self.files.values_mut().flatten() {
            changed |= breakpoint.verified.take().is_some() | breakpoint.message.take().is_some();
        }
        if changed {
            self.revision += 1;
        }
    }

    pub(crate) fn planned(&self, path: &Path, capabilities: &Capabilities) -> Vec<(SourceBreakpoint, Plan)> {
        if self.muted {
            return Vec::new();
        }
        let masters: BTreeSet<u64> =
            self.files.values().flatten().filter_map(|breakpoint| breakpoint.depends_on).collect();
        self.for_path(path)
            .iter()
            .filter(|breakpoint| breakpoint.enabled && !self.is_waiting(breakpoint))
            .map(|breakpoint| {
                let watched = breakpoint.remove_once_hit
                    || masters.contains(&breakpoint.id)
                    || (breakpoint.depends_on.is_some() && breakpoint.after_hit == AfterHit::DisableAgain);
                let by_adapter = capabilities.supports_log_points
                    && breakpoint.suspend == Suspend::None
                    && !breakpoint.log_stack
                    && !watched
                    && breakpoint.is_logging();
                let log_message = by_adapter.then(|| {
                    let mut parts = Vec::new();
                    if breakpoint.log_hit {
                        parts.push(hit_message(path, breakpoint.line));
                    }
                    if let Some(expression) = &breakpoint.log_expression {
                        parts.push(format!("{{{expression}}}"));
                    }
                    parts.join("\n")
                });
                let request = SourceBreakpoint {
                    line: i64::from(breakpoint.line) + 1,
                    condition: breakpoint.condition.clone().filter(|_| capabilities.supports_conditional_breakpoints),
                    hit_condition: breakpoint
                        .hit_condition
                        .clone()
                        .filter(|_| capabilities.supports_hit_conditional_breakpoints),
                    log_message,
                    ..SourceBreakpoint::default()
                };
                let plan = Plan {
                    line: breakpoint.line,
                    suspend: breakpoint.suspend,
                    log_hit: breakpoint.log_hit && !by_adapter,
                    log_stack: breakpoint.log_stack,
                    evaluate: breakpoint.log_expression.clone().filter(|_| !by_adapter),
                };
                (request, plan)
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn source(&self, path: &Path) -> Vec<SourceBreakpoint> {
        let capabilities = Capabilities { supports_conditional_breakpoints: true, ..Capabilities::default() };
        self.planned(path, &capabilities).into_iter().map(|(request, _)| request).collect()
    }

    pub(crate) fn planned_functions(&self, capabilities: &Capabilities) -> Vec<FunctionBreakpoint> {
        if self.muted || !capabilities.supports_function_breakpoints {
            return Vec::new();
        }
        self.functions
            .iter()
            .filter(|function| function.enabled)
            .map(|function| FunctionBreakpoint {
                name: function.name.clone(),
                condition: function.condition.clone().filter(|_| capabilities.supports_conditional_breakpoints),
                hit_condition: function
                    .hit_condition
                    .clone()
                    .filter(|_| capabilities.supports_hit_conditional_breakpoints),
            })
            .collect()
    }

    pub(crate) fn planned_exceptions(&self, capabilities: &Capabilities) -> SetExceptionBreakpointsArguments {
        let mut arguments = SetExceptionBreakpointsArguments::default();
        if self.muted {
            return arguments;
        }
        for row in self.exceptions.iter().filter(|row| row.enabled) {
            let Some(offered) =
                capabilities.exception_breakpoint_filters.iter().find(|offered| offered.filter == row.filter)
            else {
                continue;
            };
            match row
                .condition
                .clone()
                .filter(|_| offered.supports_condition && capabilities.supports_exception_filter_options)
            {
                Some(condition) => arguments
                    .filter_options
                    .get_or_insert_with(Vec::new)
                    .push(ExceptionFilterOptions { filter_id: row.filter.clone(), condition: Some(condition) }),
                None => arguments.filters.push(row.filter.clone()),
            }
        }
        arguments
    }

    pub(crate) fn planned_data(&self, capabilities: &Capabilities) -> Vec<DataBreakpoint> {
        if self.muted || !capabilities.supports_data_breakpoints {
            return Vec::new();
        }
        self.data
            .iter()
            .filter(|data| data.enabled)
            .map(|data| DataBreakpoint {
                data_id: data.data_id.clone(),
                access_type: data.access_type.clone(),
                condition: data.condition.clone().filter(|_| capabilities.supports_conditional_breakpoints),
                hit_condition: data.hit_condition.clone().filter(|_| capabilities.supports_hit_conditional_breakpoints),
            })
            .collect()
    }

    pub(crate) fn configuration(&self, capabilities: &Capabilities) -> (Configuration, Vec<Vec<Plan>>) {
        let mut breakpoints = Vec::new();
        let mut plans = Vec::new();
        for path in self.files.keys() {
            let planned = self.planned(path, capabilities);
            if planned.is_empty() {
                continue;
            }
            let (requests, path_plans): (Vec<_>, Vec<_>) = planned.into_iter().unzip();
            breakpoints.push((path.clone(), requests));
            plans.push(path_plans);
        }
        let configuration = Configuration {
            breakpoints,
            functions: self.planned_functions(capabilities),
            exceptions: self.planned_exceptions(capabilities),
            data: self.planned_data(capabilities),
        };
        (configuration, plans)
    }

    pub(crate) fn pull(&mut self, workbench: &Workbench) {
        let mut changed = false;
        for document in workbench.documents().iter().filter(|document| document.image().is_none()) {
            let Some(breakpoints) = self.files.get_mut(document.path()) else { continue };
            if document.breakpoint_marks.len() != breakpoints.len() {
                continue;
            }
            let lines = mark_lines_of(document, &document.breakpoint_marks);
            let mut moved: Vec<Breakpoint> = breakpoints
                .iter()
                .zip(lines)
                .filter_map(|(breakpoint, line)| Some(Breakpoint { line: line?, ..breakpoint.clone() }))
                .collect();
            moved.sort_by_key(|breakpoint| breakpoint.line);
            moved.dedup_by_key(|breakpoint| breakpoint.line);
            if moved != *breakpoints {
                *breakpoints = moved;
                changed = true;
            }
        }
        self.files.retain(|_, breakpoints| !breakpoints.is_empty());
        if changed {
            self.revision += 1;
        }
    }

    pub(crate) fn push(&mut self, workbench: &mut Workbench) {
        let mut changed = false;
        for document in workbench.documents.iter_mut().filter(|document| document.image.is_none()) {
            let Some(breakpoints) = self.files.get_mut(&document.path) else {
                document.breakpoint_marks.clear();
                continue;
            };
            let lines = document.buffer.text().len_lines();
            let before = breakpoints.len();
            breakpoints.retain(|breakpoint| (breakpoint.line as usize) < lines);
            changed |= breakpoints.len() != before;
            let wanted: Vec<u32> = breakpoints.iter().map(|breakpoint| breakpoint.line).collect();
            document.breakpoint_marks = whole_line_marks(document.buffer.text(), &wanted);
        }
        self.files.retain(|_, breakpoints| !breakpoints.is_empty());
        if changed {
            self.revision += 1;
        }
    }
}

pub(crate) fn mark_lines_of(document: &crate::Document, marks: &[Option<Range<usize>>]) -> Vec<Option<u32>> {
    let text = document.snapshot();
    marks.iter().map(|mark| u32::try_from(text.offset_to_point(mark.as_ref()?.start).ok()?.line).ok()).collect()
}

pub(crate) fn whole_line_marks(text: &zen_text::Text, lines: &[u32]) -> Vec<Option<Range<usize>>> {
    let count = text.len_lines();
    lines
        .iter()
        .map(|line| {
            let line = *line as usize;
            let start = text.point_to_offset(Point::new(line, 0)).ok()?;
            let end =
                if line + 1 < count { text.point_to_offset(Point::new(line + 1, 0)).ok()? } else { text.len_chars() };
            Some(start..end)
        })
        .collect()
}

pub(crate) fn map_marks(marks: &mut [Option<Range<usize>>], edits: &[Edit]) {
    for mark in marks.iter_mut() {
        if let Some(range) = mark.clone() {
            let moved = zen_editor::Selection::new(range.start, range.end).mapped(edits);
            *mark = (moved.anchor < moved.head || range.is_empty()).then_some(moved.anchor..moved.head);
        }
    }
}

pub(crate) fn mark_lines(text: &str, marks: &[Option<Range<usize>>]) -> Vec<Option<usize>> {
    marks
        .iter()
        .map(|mark| {
            let start = mark.as_ref()?.start;
            Some(text.chars().take(start).filter(|c| *c == '\n').count())
        })
        .collect()
}

pub(crate) fn line_marks(text: &str, lines: &[Option<usize>]) -> Vec<Option<Range<usize>>> {
    let mut starts = vec![0];
    let mut length = 0;
    for (index, c) in text.chars().enumerate() {
        length = index + 1;
        if c == '\n' {
            starts.push(index + 1);
        }
    }
    lines
        .iter()
        .map(|line| {
            let line = (*line)?;
            let start = *starts.get(line)?;
            Some(start..starts.get(line + 1).copied().unwrap_or(length))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_follow_inserted_lines_and_drop_with_deleted_ones() {
        let text = "a\nb\nc\n";
        let mut marks = line_marks(text, &[Some(1), Some(2)]);
        assert_eq!(marks, vec![Some(2..4), Some(4..6)]);
        map_marks(&mut marks, &[Edit::replace(2..2, "x\n".to_owned())]);
        assert_eq!(mark_lines("a\nx\nb\nc\n", &marks), vec![Some(2), Some(3)]);
        map_marks(&mut marks, &[Edit::replace(4..6, String::new())]);
        assert_eq!(marks[0], None);
        assert_eq!(mark_lines("a\nx\nc\n", &marks), vec![None, Some(2)]);
    }

    #[test]
    fn breakpoints_are_set_toggled_and_restored() {
        let mut breakpoints = Breakpoints::default();
        let path = Path::new("/p/main.go");
        assert!(breakpoints.toggle(path, 4).is_some());
        let change = BreakpointChange {
            enabled: Some(false),
            condition: Some("x > 1".to_owned()),
            ..BreakpointChange::default()
        };
        assert_eq!(breakpoints.set(path, 2, change).condition.as_deref(), Some("x > 1"));
        assert_eq!(breakpoints.for_path(path).iter().map(|breakpoint| breakpoint.line).collect::<Vec<_>>(), [2, 4]);
        breakpoints.set_status(path, 4, Some(true), None);
        let saved = breakpoints.all();
        assert!(saved.iter().all(|info| info.verified.is_none()));
        assert_eq!(breakpoints.toggle(path, 4), None);
        let mut restored = Breakpoints::default();
        restored.restore(serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap());
        assert_eq!(restored.all(), saved);
        assert_eq!(restored.source(path), [SourceBreakpoint { line: 5, ..SourceBreakpoint::default() }]);
        assert!(!restored.remove(path, 9));
    }

    #[test]
    fn every_property_and_the_master_survive_the_workspace() {
        let mut breakpoints = Breakpoints::default();
        let path = Path::new("/p/main.go");
        breakpoints.toggle(path, 1);
        let change = BreakpointChange {
            suspend: Some(Suspend::None),
            log_hit: Some(true),
            log_stack: Some(true),
            log_expression: Some("x".to_owned()),
            remove_once_hit: Some(true),
            hit_condition: Some("3".to_owned()),
            depends_on: Some(Some(LinePlace { path: path.to_owned(), line: 1 })),
            after_hit: Some(AfterHit::LeaveEnabled),
            group: Some("Mine".to_owned()),
            description: Some("why".to_owned()),
            ..BreakpointChange::default()
        };
        breakpoints.set(path, 5, change);
        breakpoints.set_function("main.run", None, Some("n > 1".to_owned()), None);
        breakpoints.set_exceptions(vec!["uncaught".to_owned()]);
        let saved = serde_json::to_value(breakpoints.all()).unwrap();
        assert_eq!(saved[1]["dependsOn"], serde_json::json!({ "path": "/p/main.go", "line": 1 }));
        assert_eq!(saved[1]["suspend"], "none");
        assert_eq!(saved[0].as_object().unwrap().len(), 3, "defaults are left out: {saved}");
        let mut restored = Breakpoints::default();
        restored.restore(serde_json::from_value(saved.clone()).unwrap());
        restored.restore_functions(breakpoints.saved_functions());
        restored.restore_exceptions(breakpoints.exception_rows().to_vec(), Vec::new());
        assert_eq!(serde_json::to_value(restored.all()).unwrap(), saved);
        assert_eq!(restored.exceptions(), ["uncaught"]);
        assert_eq!(restored.functions()[0].condition.as_deref(), Some("n > 1"));
        assert!(restored.planned(path, &Capabilities::default()).iter().all(|(request, _)| request.line == 2));
        restored.hit(path, 1);
        assert_eq!(restored.planned(path, &Capabilities::default()).len(), 2, "the master's hit arms it");
        restored.remove(path, 1);
        assert_eq!(restored.list(Some(path))[0].depends_on, None, "removing the master frees it");
    }
}
