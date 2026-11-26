use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_terminal::Options;

use crate::Workbench;

pub mod agents;
pub mod links;
pub mod selection;
pub use links::{LinkAction, LinkChoices};

pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub type Launcher = Arc<dyn Fn(&Path) -> Options + Send + Sync>;

pub const MAX_SEND: usize = 64 * 1024;

pub const MAX_READ: usize = 64 * 1024;

pub const MAX_SHELL_TITLE: usize = 24;

pub const RESTORE: &str = "terminal.restore";

pub const RECORD_AGE: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 60 * 60);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn new_terminal_id() -> String {
    use sha2::Digest;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let seed = format!("{nanos}-{}-{}-{:p}", std::process::id(), next_id(), &NEXT_ID);
    sha2::Sha256::digest(seed.as_bytes())[..16].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where a split puts the new pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// To the right of the pane: the panes side by side.
    Right,
    /// Below the pane: the panes one above the other.
    Down,
}

pub struct Pane {
    id: u64,
    session: zen_terminal::Session,
    cwd: PathBuf,
    rerun: Option<String>,
    terminal: String,
    resumed: Option<String>,
}

impl Pane {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn session(&self) -> &zen_terminal::Session {
        &self.session
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn rerun(&self) -> Option<&str> {
        self.rerun.as_deref()
    }

    pub fn terminal(&self) -> &str {
        &self.terminal
    }

    pub fn resumed(&self) -> Option<&str> {
        self.resumed.as_deref()
    }

    pub fn title(&self) -> String {
        if let Some(title) = self.session.title().filter(|title| !title.trim().is_empty()) {
            return title;
        }
        self.cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| t!("terminal-tab-local"))
    }
}

pub struct Split {
    pub id: u64,
    pub direction: Direction,
    pub ratio: f32,
    pub first: Box<Node>,
    pub second: Box<Node>,
}

pub enum Node {
    Pane(Pane),
    Split(Split),
}

impl Node {
    pub fn panes(&self) -> Vec<&Pane> {
        let mut panes = Vec::new();
        self.collect(&mut panes);
        panes
    }

    fn collect<'a>(&'a self, panes: &mut Vec<&'a Pane>) {
        match self {
            Node::Pane(pane) => panes.push(pane),
            Node::Split(split) => {
                split.first.collect(panes);
                split.second.collect(panes);
            }
        }
    }

    fn pane_mut(&mut self, id: u64) -> Option<&mut Pane> {
        match self {
            Node::Pane(pane) => (pane.id == id).then_some(pane),
            Node::Split(split) => split.first.pane_mut(id).or_else(|| split.second.pane_mut(id)),
        }
    }

    fn any(&self, test: &dyn Fn(&Pane) -> bool) -> bool {
        match self {
            Node::Pane(pane) => test(pane),
            Node::Split(split) => split.first.any(test) || split.second.any(test),
        }
    }

    fn retain(self, keep: &dyn Fn(&Pane) -> bool) -> Option<Node> {
        match self {
            Node::Pane(pane) => keep(&pane).then_some(Node::Pane(pane)),
            Node::Split(Split { id, direction, ratio, first, second }) => {
                match (first.retain(keep), second.retain(keep)) {
                    (Some(first), Some(second)) => Some(Node::Split(Split {
                        id,
                        direction,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    })),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    fn split(self, at: u64, direction: Direction, new: &mut Option<Pane>) -> Node {
        match self {
            Node::Pane(pane) if pane.id == at => match new.take() {
                Some(added) => Node::Split(Split {
                    id: next_id(),
                    direction,
                    ratio: 0.5,
                    first: Box::new(Node::Pane(pane)),
                    second: Box::new(Node::Pane(added)),
                }),
                None => Node::Pane(pane),
            },
            Node::Pane(pane) => Node::Pane(pane),
            Node::Split(Split { id, direction: own, ratio, first, second }) => Node::Split(Split {
                id,
                direction: own,
                ratio,
                first: Box::new(first.split(at, direction, new)),
                second: Box::new(second.split(at, direction, new)),
            }),
        }
    }

    fn set_ratio(&mut self, split: u64, ratio: f32) -> bool {
        match self {
            Node::Pane(_) => false,
            Node::Split(node) if node.id == split => {
                node.ratio = ratio.clamp(0.1, 0.9);
                true
            }
            Node::Split(node) => node.first.set_ratio(split, ratio) || node.second.set_ratio(split, ratio),
        }
    }

    fn save(&self) -> SavedLayout {
        match self {
            Node::Pane(pane) => SavedLayout::Pane {
                cwd: pane.cwd.clone(),
                session: pane.session.hosted().map(SavedSession::from),
                terminal: pane.session.hosted().map(|_| pane.terminal.clone()),
            },
            Node::Split(split) => SavedLayout::Split {
                direction: split.direction,
                ratio: split.ratio,
                first: Box::new(split.first.save()),
                second: Box::new(split.second.save()),
            },
        }
    }

    fn info(&self) -> LayoutInfo {
        match self {
            Node::Pane(pane) => LayoutInfo::Session { session: pane.id },
            Node::Split(split) => LayoutInfo::Split {
                direction: split.direction,
                ratio: split.ratio,
                first: Box::new(split.first.info()),
                second: Box::new(split.second.info()),
            },
        }
    }
}

pub struct Tab {
    name: Option<String>,
    root: Node,
    focused: u64,
    number: usize,
    transient: bool,
}

impl Tab {
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn focused(&self) -> u64 {
        self.focused
    }

    pub fn title(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let panes = self.root.panes();
        let focused = panes.iter().find(|pane| pane.id == self.focused).or(panes.first());
        focused
            .and_then(|pane| pane.session.title())
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty() && title.chars().count() <= MAX_SHELL_TITLE)
            .unwrap_or_else(|| self.default_title())
    }

    pub fn default_title(&self) -> String {
        if self.number <= 1 {
            t!("terminal-tab-local")
        } else {
            t!("terminal-tab-local-numbered", number = self.number)
        }
    }

    fn contains(&self, id: u64) -> bool {
        self.root.any(&|pane| pane.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTab {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub layout: SavedLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub id: u64,
    pub protocol: u32,
    pub host: u64,
}

impl From<zen_terminal::HostedSession> for SavedSession {
    fn from(hosted: zen_terminal::HostedSession) -> SavedSession {
        SavedSession { id: hosted.id, protocol: hosted.protocol, host: hosted.host }
    }
}

impl From<SavedSession> for zen_terminal::HostedSession {
    fn from(saved: SavedSession) -> zen_terminal::HostedSession {
        zen_terminal::HostedSession { id: saved.id, protocol: saved.protocol, host: saved.host }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum SavedLayout {
    Pane {
        cwd: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<SavedSession>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
    },
    Split {
        direction: Direction,
        ratio: f32,
        first: Box<SavedLayout>,
        second: Box<SavedLayout>,
    },
}

#[derive(Default)]
pub struct Terminals {
    tabs: Vec<Tab>,
    active: usize,
    waker: Arc<Mutex<Option<Waker>>>,
    launcher: Option<Launcher>,
    host: Option<zen_terminal::Host>,
    no_restore: bool,
    plugin_agents: Option<agents::AgentSource>,
    user_agents: Vec<agents::AgentAdapter>,
    reveal: bool,
    pending: Vec<SavedTab>,
    reveal_in_project: Option<PathBuf>,
}

impl Terminals {
    pub fn set_waker(&mut self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(waker);
    }

    pub fn set_launcher(&mut self, launcher: Launcher) {
        self.launcher = Some(launcher);
    }

    pub fn set_host(&mut self, host: Option<zen_terminal::Host>) {
        self.host = host;
    }

    pub fn host(&self) -> Option<&zen_terminal::Host> {
        self.host.as_ref()
    }

    pub fn restores(&self) -> bool {
        !self.no_restore
    }

    pub fn set_restore(&mut self, on: bool) {
        if on == !self.no_restore {
            return;
        }
        self.no_restore = !on;
        for pane in self.tabs.iter().flat_map(|tab| tab.root.panes()) {
            pane.session.set_keep(on);
        }
        if !on && let Some(host) = &self.host {
            host.forget_all();
        }
    }

    pub fn pane_mut(&mut self, id: u64) -> Option<&mut Pane> {
        self.tabs.iter_mut().find_map(|tab| tab.root.pane_mut(id))
    }

    pub fn shell_program(&self, cwd: &Path) -> PathBuf {
        self.launcher.as_ref().and_then(|launcher| launcher(cwd).program).unwrap_or_else(zen_terminal::default_shell)
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<usize> {
        (!self.tabs.is_empty()).then(|| self.active.min(self.tabs.len() - 1))
    }

    pub fn focused(&self) -> Option<u64> {
        self.active().map(|index| self.tabs[index].focused)
    }

    pub fn pane(&self, id: u64) -> Option<&Pane> {
        self.tabs.iter().flat_map(|tab| tab.root.panes()).find(|pane| pane.id == id)
    }

    pub fn tab_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.contains(id))
    }

    pub fn take_reveal(&mut self) -> bool {
        std::mem::take(&mut self.reveal)
    }

    pub fn take_reveal_in_project(&mut self) -> Option<PathBuf> {
        self.reveal_in_project.take()
    }

    fn spawn(&self, cwd: &Path) -> Result<Pane, CommandError> {
        let mut options = match &self.launcher {
            Some(launcher) => launcher(cwd),
            None => Options::shell(cwd),
        };
        options.cwd = cwd.to_owned();
        self.open(options, new_terminal_id())
    }

    fn identify(&self, options: &mut Options, terminal: &str) {
        let mut set = |key: &str, value: String| {
            options.env.retain(|(known, _)| known != key);
            options.env.push((key.to_owned(), value));
        };
        set(agents::TERMINAL_ID, terminal.to_owned());
        if let Some(folder) = self.host.as_ref().and_then(zen_terminal::Host::agents_folder) {
            set(agents::AGENTS_FOLDER, folder.to_string_lossy().into_owned());
        }
    }

    fn open(&self, mut options: Options, terminal: String) -> Result<Pane, CommandError> {
        options.keep = !self.no_restore;
        self.identify(&mut options, &terminal);
        if let Some(host) = &self.host
            && let Ok(session) = zen_terminal::Session::open(host, options.clone(), self.wake())
        {
            return Ok(Pane { id: next_id(), session, cwd: options.cwd, rerun: None, terminal, resumed: None });
        }
        let mut pane = self.spawn_options(options)?;
        pane.terminal = terminal;
        Ok(pane)
    }

    #[cfg(unix)]
    fn resumable(&self, record: &zen_terminal::host::record::Record, terminal: &str) -> Option<(String, String, bool)> {
        let host = self.host.as_ref()?;
        let running = record.running.as_ref()?;
        self.adapters().into_iter().filter(|(adapter, _)| adapter.recognises(running)).find_map(|(adapter, source)| {
            let session = adapter.session_of(&host.agent_report(terminal, &adapter.id)?)?;
            Some((adapter.name.clone(), adapter.resume_line(&session)?, source.resumes_by_itself()))
        })
    }

    #[cfg(unix)]
    fn revive(
        &self,
        saved: SavedSession,
        cwd: &Path,
        why: Ended,
        fallback: &Path,
        terminal: Option<&str>,
    ) -> Option<Pane> {
        if self.no_restore {
            return None;
        }
        let host = self.host.as_ref()?;
        let hosted = zen_terminal::HostedSession::from(saved);
        let record = host.record(hosted)?;
        let why = match why {
            Ended::Unknown => {
                let rebooted = zen_terminal::host::record::booted_at().is_some_and(|booted| booted > record.saved);
                if rebooted { Ended::Restarted } else { Ended::HostStopped }
            }
            known => known,
        };
        let cwd = [record.cwd.as_path(), cwd, fallback].into_iter().find(|cwd| usable_folder(cwd))?.to_owned();
        let mut options = match &self.launcher {
            Some(launcher) => launcher(&cwd),
            None => Options::shell(&cwd),
        };
        options.cwd = cwd;
        for (key, value) in record.env.iter().filter(|(key, _)| key != "PWD") {
            options.env.retain(|(known, _)| known != key);
            options.env.push((key.clone(), value.clone()));
        }
        let given = record.env.iter().find(|(key, _)| key == agents::TERMINAL_ID).map(|(_, value)| value.as_str());
        let terminal = terminal
            .or(given)
            .filter(|terminal| agents::valid_terminal(terminal))
            .map_or_else(new_terminal_id, str::to_owned);
        let resume = self.resumable(&record, &terminal);
        let mut ended = why.message();
        if let Some((agent, _, true)) = &resume {
            ended = format!("{ended}. {}", t!("terminal-agent-resuming", agent = agent.clone()));
        }
        options.preface = preface(&record.text, &ended);
        let mut pane = self.open(options, terminal).ok()?;
        match resume {
            Some((agent, line, true)) => {
                pane.session.write(format!("{line}\r").into_bytes());
                pane.resumed = Some(agent);
            }
            Some((_, line, false)) => pane.rerun = Some(clean_command(&line)),
            None => pane.rerun = record.command.as_deref().map(clean_command).filter(|command| !command.is_empty()),
        }
        host.forget(hosted);
        Some(pane)
    }

    #[cfg(not(unix))]
    fn revive(
        &self,
        _saved: SavedSession,
        _cwd: &Path,
        _why: Ended,
        _fallback: &Path,
        _terminal: Option<&str>,
    ) -> Option<Pane> {
        None
    }

    fn attach(&self, saved: SavedSession, cwd: &Path, terminal: String) -> Option<Pane> {
        let host = self.host.as_ref()?;
        let (hosted, size) = (saved.into(), zen_terminal::Size::default());
        let session = match zen_terminal::Session::attach(host, hosted, size, self.wake()) {
            Ok(session) => session,
            Err(zen_terminal::Error::Incompatible { protocol }) => {
                let text = t!("terminal-session-elsewhere", protocol = protocol);
                zen_terminal::Session::elsewhere(host, hosted, size, &text, self.wake()).ok()?
            }
            Err(_) => return None,
        };
        Some(Pane { id: next_id(), session, cwd: cwd.to_owned(), rerun: None, terminal, resumed: None })
    }

    fn wake(&self) -> impl Fn() + Send + Sync + 'static {
        let waker = Arc::clone(&self.waker);
        move || {
            let waker = waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            if let Some(waker) = waker {
                waker();
            }
        }
    }

    fn spawn_options(&self, options: Options) -> Result<Pane, CommandError> {
        let cwd = options.cwd.clone();
        let session = zen_terminal::Session::spawn(options, self.wake()).map_err(|error| match error {
            zen_terminal::Error::Spawn(cause) => {
                CommandError::new(t!("terminal-session-start-failed", error = cause.to_string()))
            }
            zen_terminal::Error::Unsupported => CommandError::new(t!("terminal-session-unsupported")),
            zen_terminal::Error::Incompatible { .. } => {
                CommandError::new(t!("terminal-session-start-failed", error = error.to_string()))
            }
        })?;
        Ok(Pane { id: next_id(), session, cwd, rerun: None, terminal: new_terminal_id(), resumed: None })
    }

    fn free_number(&self) -> usize {
        (1..).find(|number| self.tabs.iter().all(|tab| tab.number != *number)).unwrap_or(1)
    }

    pub fn new_tab(&mut self, cwd: &Path) -> Result<u64, CommandError> {
        let pane = self.spawn(cwd)?;
        let id = pane.id;
        let number = self.free_number();
        self.tabs.push(Tab { name: None, root: Node::Pane(pane), focused: id, number, transient: false });
        self.active = self.tabs.len() - 1;
        self.reveal = true;
        Ok(id)
    }

    pub fn run_in_new_tab(&mut self, options: Options, name: String) -> Result<u64, CommandError> {
        let pane = self.spawn_options(options)?;
        let id = pane.id;
        let number = self.free_number();
        self.tabs.push(Tab { name: Some(name), root: Node::Pane(pane), focused: id, number, transient: true });
        self.active = self.tabs.len() - 1;
        self.reveal = true;
        Ok(id)
    }

    pub fn split(&mut self, id: Option<u64>, direction: Direction) -> Result<u64, CommandError> {
        let at = self.session_or_focused(id)?;
        let index = self.tab_of(at).ok_or_else(|| missing_session(at))?;
        let cwd = self.pane(at).map(|pane| pane.cwd.clone()).ok_or_else(|| missing_session(at))?;
        let pane = self.spawn(&cwd)?;
        let new = pane.id;
        let Tab { name, root, number, transient, .. } = self.tabs.remove(index);
        let root = root.split(at, direction, &mut Some(pane));
        self.tabs.insert(index, Tab { name, root, focused: new, number, transient });
        self.active = index;
        self.reveal = true;
        Ok(new)
    }

    pub fn close(&mut self, id: u64) -> bool {
        let Some(index) = self.tab_of(id) else { return false };
        if let Some(pane) = self.pane(id) {
            pane.session.close();
            if let Some(host) = &self.host {
                host.forget_agents(&pane.terminal);
            }
        }
        self.retain_in(index, &|pane| pane.id != id);
        true
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        for pane in self.tabs.remove(index).root.panes() {
            pane.session.close();
            if let Some(host) = &self.host {
                host.forget_agents(&pane.terminal);
            }
        }
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        true
    }

    pub fn prune(&mut self) -> bool {
        let mut changed = self.revive_lost();
        let mut index = 0;
        while index < self.tabs.len() {
            let ended = self.tabs[index].root.any(&|pane| pane.session.has_exited());
            if ended {
                changed = true;
                if !self.retain_in(index, &|pane| !pane.session.has_exited()) {
                    continue;
                }
            }
            index += 1;
        }
        changed
    }

    fn revive_lost(&mut self) -> bool {
        let lost: Vec<(u64, SavedSession, PathBuf, String)> = self
            .tabs
            .iter()
            .flat_map(|tab| tab.root.panes())
            .filter(|pane| pane.session.lost_host())
            .filter_map(|pane| Some((pane.id, pane.session.hosted()?.into(), pane.cwd.clone(), pane.terminal.clone())))
            .collect();
        let mut revived = false;
        for (id, saved, cwd, terminal) in lost {
            if let Some(new) = self.revive(saved, &cwd, Ended::HostStopped, &cwd, Some(&terminal))
                && let Some(pane) = self.pane_mut(id)
            {
                pane.session = new.session;
                pane.cwd = new.cwd;
                pane.rerun = new.rerun;
                pane.resumed = new.resumed;
                revived = true;
            }
        }
        revived
    }

    fn retain_in(&mut self, index: usize, keep: &dyn Fn(&Pane) -> bool) -> bool {
        let Tab { name, root, focused, number, transient } = self.tabs.remove(index);
        let order: Vec<u64> = root.panes().iter().map(|pane| pane.id).collect();
        let position = order.iter().position(|id| *id == focused).unwrap_or(0);
        match root.retain(keep) {
            Some(root) => {
                let left: Vec<u64> = root.panes().iter().map(|pane| pane.id).collect();
                let focused = if left.contains(&focused) { focused } else { left[position.min(left.len() - 1)] };
                self.tabs.insert(index, Tab { name, root, focused, number, transient });
                true
            }
            None => {
                if self.active > index || self.active >= self.tabs.len() {
                    self.active = self.active.saturating_sub(1);
                }
                false
            }
        }
    }

    pub fn focus(&mut self, id: u64) -> bool {
        let Some(index) = self.tab_of(id) else { return false };
        self.tabs[index].focused = id;
        self.active = index;
        true
    }

    pub fn cycle(&mut self, forward: bool) -> Option<u64> {
        let index = self.active()?;
        let tab = &mut self.tabs[index];
        let order: Vec<u64> = tab.root.panes().iter().map(|pane| pane.id).collect();
        let position = order.iter().position(|id| *id == tab.focused).unwrap_or(0);
        let next = if forward { (position + 1) % order.len() } else { (position + order.len() - 1) % order.len() };
        tab.focused = order[next];
        Some(tab.focused)
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        true
    }

    pub fn rename(&mut self, index: usize, name: Option<String>) -> bool {
        let Some(tab) = self.tabs.get_mut(index) else { return false };
        tab.name = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty());
        true
    }

    pub fn set_ratio(&mut self, split: u64, ratio: f32) {
        for tab in &mut self.tabs {
            if tab.root.set_ratio(split, ratio) {
                return;
            }
        }
    }

    pub fn save(&self) -> Vec<SavedTab> {
        if self.tabs.is_empty() {
            return self.pending.clone();
        }
        self.tabs
            .iter()
            .filter(|tab| !tab.transient)
            .map(|tab| SavedTab { name: tab.name.clone(), layout: tab.root.save() })
            .collect()
    }

    pub fn restore(&mut self, tabs: Vec<SavedTab>) {
        self.pending = tabs;
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn restore_pending(&mut self, fallback: &Path) -> Result<(), CommandError> {
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            return Ok(());
        }
        let mut restored = Vec::new();
        let mut result = Ok(());
        for saved in &pending {
            match self.start_layout(&saved.layout, fallback) {
                Ok(root) => {
                    let focused = root.panes().first().map_or(0, |pane| pane.id);
                    restored.push(Tab { name: saved.name.clone(), root, focused, number: 0, transient: false });
                }
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        let count = restored.len();
        let opened = !self.tabs.is_empty();
        restored.append(&mut self.tabs);
        self.tabs = restored;
        for index in 0..count {
            self.tabs[index].number = self.free_number();
        }
        self.active = if opened { self.active + count } else { 0 };
        result
    }

    fn start_layout(&self, layout: &SavedLayout, fallback: &Path) -> Result<Node, CommandError> {
        Ok(match layout {
            SavedLayout::Pane { cwd, session, terminal } => {
                let cwd = if usable_folder(cwd) { cwd.as_path() } else { fallback };
                let terminal = terminal.as_deref().filter(|terminal| agents::valid_terminal(terminal));
                let attached = session
                    .and_then(|saved| self.attach(saved, cwd, terminal.map_or_else(new_terminal_id, str::to_owned)));
                let revived = || session.and_then(|saved| self.revive(saved, cwd, Ended::Unknown, fallback, terminal));
                match attached.or_else(revived) {
                    Some(pane) => Node::Pane(pane),
                    None => Node::Pane(self.spawn(cwd)?),
                }
            }
            SavedLayout::Split { direction, ratio, first, second } => Node::Split(Split {
                id: next_id(),
                direction: *direction,
                ratio: ratio.clamp(0.1, 0.9),
                first: Box::new(self.start_layout(first, fallback)?),
                second: Box::new(self.start_layout(second, fallback)?),
            }),
        })
    }

    pub fn list(&self) -> TerminalList {
        let active = self.active();
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabInfo {
                index,
                title: tab.title(),
                name: tab.name.clone(),
                active: Some(index) == active,
                focused: tab.focused,
                layout: tab.root.info(),
                sessions: tab
                    .root
                    .panes()
                    .into_iter()
                    .map(|pane| SessionInfo {
                        id: pane.id,
                        title: pane.title(),
                        cwd: pane.cwd.clone(),
                        exited: pane.session.has_exited(),
                        elsewhere: pane.session.is_elsewhere(),
                        rerun: pane.rerun.clone(),
                        terminal: pane.terminal.clone(),
                        resumed: pane.resumed.clone(),
                        focused: pane.id == tab.focused,
                    })
                    .collect(),
            })
            .collect();
        TerminalList { tabs, active }
    }

    fn session_or_focused(&self, id: Option<u64>) -> Result<u64, CommandError> {
        match id {
            Some(id) if self.pane(id).is_some() => Ok(id),
            Some(id) => Err(missing_session(id)),
            None => self.focused().ok_or_else(|| CommandError::new(t!("terminal-session-none"))),
        }
    }

    fn tab_or_active(&self, tab: Option<usize>) -> Result<usize, CommandError> {
        match tab {
            Some(index) if index < self.tabs.len() => Ok(index),
            Some(index) => Err(CommandError::new(t!("terminal-tab-missing", tab = index))),
            None => self.active().ok_or_else(|| CommandError::new(t!("terminal-session-none"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
enum Ended {
    Unknown,
    Restarted,
    HostStopped,
}

impl Ended {
    #[cfg_attr(not(unix), allow(dead_code))]
    fn message(self) -> String {
        match self {
            Ended::Restarted => t!("terminal-session-ended-restart"),
            Ended::Unknown | Ended::HostStopped => t!("terminal-session-ended-host"),
        }
    }
}

fn printable(text: &str) -> String {
    text.chars().filter(|ch| *ch == '\n' || *ch == '\t' || !ch.is_control()).collect()
}

#[cfg_attr(not(unix), allow(dead_code))]
fn clean_command(command: &str) -> String {
    printable(command).replace(['\n', '\t'], " ").trim().chars().take(4096).collect()
}

pub fn preface(text: &str, ended: &str) -> String {
    let mut out = String::from("\x1b[0m\x1b[2m");
    let text = printable(text);
    for line in text.trim_end().lines() {
        out.push_str(line.trim_end());
        out.push_str("\r\n");
    }
    out.push_str("\x1b[0m\x1b[7m ");
    out.push_str(&printable(ended).replace('\n', " "));
    out.push_str(" \x1b[0m\r\n");
    out
}

fn usable_folder(path: &Path) -> bool {
    path.is_absolute() && std::fs::read_dir(path).is_ok()
}

fn tail(text: String, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text, false);
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_owned(), true)
}

fn missing_session(id: u64) -> CommandError {
    CommandError::new(t!("terminal-session-missing", id = id))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalList {
    pub tabs: Vec<TabInfo>,
    pub active: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub index: usize,
    pub title: String,
    pub name: Option<String>,
    pub active: bool,
    pub focused: u64,
    pub layout: LayoutInfo,
    pub sessions: Vec<SessionInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum LayoutInfo {
    Session { session: u64 },
    Split { direction: Direction, ratio: f32, first: Box<LayoutInfo>, second: Box<LayoutInfo> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u64,
    pub title: String,
    pub cwd: PathBuf,
    pub exited: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub elsewhere: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<String>,
    pub terminal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed: Option<String>,
    pub focused: bool,
}

/// The parameters of `terminal.session.new`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewParams {
    /// The folder to start in, absolute or relative to the project; the project's folder by default.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub id: u64,
    pub tab: usize,
}

/// The parameters of the commands that act on one session, the focused one by default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionParams {
    /// The session's id from `terminal.session.list`; the focused session of the tab shown by default.
    #[serde(default)]
    pub id: Option<u64>,
}

/// The parameters of `terminal.session.split`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitParams {
    /// The session to split; the focused one by default.
    #[serde(default)]
    pub id: Option<u64>,
    /// Where the new pane goes: `right` or `down`.
    pub direction: Direction,
}

/// The parameters of `terminal.session.send`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendParams {
    /// The session; the focused one by default.
    #[serde(default)]
    pub id: Option<u64>,
    /// The text to type; end it with a line break to press Enter.
    pub text: String,
}

/// The parameters of `terminal.session.focus`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FocusParams {
    /// The session to give the keyboard.
    pub id: u64,
}

/// The parameters of `terminal.tab.select` and `terminal.tab.close`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabParams {
    /// The tab's index; the tab shown by default.
    #[serde(default)]
    pub tab: Option<usize>,
}

/// The parameters of `terminal.tab.rename`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The tab's index; the tab shown by default.
    #[serde(default)]
    pub tab: Option<usize>,
    /// The new name; `null` or an empty name gives the tab back its own title.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Read {
    pub id: u64,
    pub text: String,
    pub truncated: bool,
    pub title: String,
    pub exited: bool,
}

fn has_sessions(session: &Session) -> bool {
    session.state::<Terminals>().is_some_and(|terminals| !terminals.tabs.is_empty() || terminals.has_pending())
}

fn sync_settings(session: &mut Session) {
    let on = session.settings().get_or(RESTORE, true);
    let user: Vec<serde_json::Value> = session.settings().get(agents::AGENTS).unwrap_or_default();
    let terminals = session.state_mut::<Terminals>();
    terminals.set_restore(on);
    terminals.set_user_agents(agents::from_values(&user));
}

fn started(session: &mut Session) -> Result<&mut Terminals, CommandError> {
    restore_pending(session)?;
    Ok(session.state_mut::<Terminals>())
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf)
}

pub fn default_cwd(session: &Session) -> PathBuf {
    project_root(session).or_else(|| std::env::var_os("HOME").map(PathBuf::from)).unwrap_or_else(std::env::temp_dir)
}

pub fn restore_pending(session: &mut Session) -> Result<(), CommandError> {
    sync_settings(session);
    let fallback = default_cwd(session);
    let terminals = session.state_mut::<Terminals>();
    let first = terminals.has_pending();
    let result = terminals.restore_pending(&fallback);
    if first && let Some(host) = terminals.host() {
        host.forget_older_than(RECORD_AGE);
    }
    result
}

pub struct NewSession;

impl Command for NewSession {
    type Params = NewParams;
    type Output = Started;
    const ID: &'static str = "terminal.session.new";
    const TITLE: &'static str = "New Tab";
    const DESCRIPTION: &'static str = "Starts the user's login shell in a new tab of the Terminal tool window and shows it; \
         `cwd` is the folder, absolute or relative to the project (the project's folder by default). Returns the \
         session's `id` and its `tab`.";

    fn run(&self, session: &mut Session, params: NewParams) -> Result<Started, CommandError> {
        let root = default_cwd(session);
        let cwd = match params.cwd {
            Some(cwd) if cwd.is_absolute() => cwd,
            Some(cwd) => root.join(cwd),
            None => root,
        };
        if !cwd.is_dir() {
            return Err(CommandError::new(t!("terminal-session-no-folder", path = cwd.display().to_string())));
        }
        restore_pending(session)?;
        let terminals = session.state_mut::<Terminals>();
        let id = terminals.new_tab(&cwd)?;
        Ok(Started { id, tab: terminals.tabs.len() - 1 })
    }
}

pub struct SplitSession;

impl Command for SplitSession {
    type Params = SplitParams;
    type Output = Started;
    const ID: &'static str = "terminal.session.split";
    const TITLE: &'static str = "Split";
    const DESCRIPTION: &'static str = "Splits a session's pane (the focused one by default) and starts a new shell in \
         the same folder, `right` of it or `down` below it; the new session gets the keyboard. Returns its `id` and \
         `tab`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: SplitParams) -> Result<Started, CommandError> {
        let terminals = started(session)?;
        terminals.prune();
        let id = terminals.split(params.id, params.direction)?;
        Ok(Started { id, tab: terminals.tab_of(id).unwrap_or_default() })
    }
}

pub struct ListSessions;

impl Command for ListSessions {
    type Params = NoParams;
    type Output = TerminalList;
    const ID: &'static str = "terminal.session.list";
    const TITLE: &'static str = "Terminal Sessions";
    const DESCRIPTION: &'static str = "Lists the Terminal's tabs in order: each with `index`, `title`, `name` (given \
         by the user), `active`, the `focused` session, its `layout` of splits and its `sessions` (`id`, `title`, \
         `cwd`, `exited`, `focused`, `elsewhere` when the session runs in an older IDE's terminal host this IDE \
         cannot show: it keeps running and its tab closes when that host stops, and `rerun`, the last command of a \
         terminal that came back after a restart, which only the person can run again, `terminal`, its \
         `ZEN_TERMINAL_ID`, and `resumed`, the agent whose conversation it resumed after a restart); `active` is the \
         index of the tab shown.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TerminalList, CommandError> {
        let terminals = started(session)?;
        terminals.prune();
        Ok(terminals.list())
    }
}

pub struct CloseSession;

impl Command for CloseSession {
    type Params = SessionParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.session.close";
    const TITLE: &'static str = "Close Session";
    const DESCRIPTION: &'static str = "Ends a session (the focused one by default) and removes its pane; a tab left \
         without panes closes.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: SessionParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        terminals.close(id);
        Ok(NoParams {})
    }
}

pub struct SendText;

impl Command for SendText {
    type Params = SendParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.session.send";
    const TITLE: &'static str = "Send Text to Terminal";
    const DESCRIPTION: &'static str = "Types `text` (at most 64 KiB) into a session (the focused one by default), as \
         if on the keyboard; end it with a line break to press Enter.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: SendParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        if params.text.len() > MAX_SEND {
            return Err(CommandError::new(t!("terminal-send-too-long", limit = MAX_SEND)));
        }
        if let Some(pane) = terminals.pane(id) {
            pane.session.write(params.text.replace('\n', "\r").into_bytes());
            pane.session.scroll_to_bottom();
        }
        Ok(NoParams {})
    }
}

pub struct ReadSession;

impl Command for ReadSession {
    type Params = SessionParams;
    type Output = Read;
    const ID: &'static str = "terminal.session.read";
    const TITLE: &'static str = "Read Terminal";
    const DESCRIPTION: &'static str = "Returns the visible `text` of a session (the focused one by default; at most \
         64 KiB from the end of the screen, `truncated` when cut), its `title` and whether its program has `exited`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: SessionParams) -> Result<Read, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane(id).ok_or_else(|| missing_session(id))?;
        let (text, truncated) = tail(pane.session.snapshot().text(), MAX_READ);
        Ok(Read { id, text, truncated, title: pane.title(), exited: pane.session.has_exited() })
    }
}

pub struct FocusSession;

impl Command for FocusSession {
    type Params = FocusParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.session.focus";
    const TITLE: &'static str = "Focus Terminal Session";
    const DESCRIPTION: &'static str = "Gives a session the keyboard and shows its tab.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: FocusParams) -> Result<NoParams, CommandError> {
        if started(session)?.focus(params.id) { Ok(NoParams {}) } else { Err(missing_session(params.id)) }
    }
}

pub struct FocusNext;

impl Command for FocusNext {
    type Params = NoParams;
    type Output = SessionParams;
    const ID: &'static str = "terminal.session.focusNext";
    const TITLE: &'static str = "Next Splitter";
    const DESCRIPTION: &'static str = "Gives the keyboard to the next pane of the tab shown, in reading order; returns \
         the session's `id`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, _: NoParams) -> Result<SessionParams, CommandError> {
        let id = started(session)?.cycle(true);
        id.map(|id| SessionParams { id: Some(id) }).ok_or_else(|| CommandError::new(t!("terminal-session-none")))
    }
}

pub struct FocusPrevious;

impl Command for FocusPrevious {
    type Params = NoParams;
    type Output = SessionParams;
    const ID: &'static str = "terminal.session.focusPrevious";
    const TITLE: &'static str = "Previous Splitter";
    const DESCRIPTION: &'static str = "Gives the keyboard to the previous pane of the tab shown, in reading order; \
         returns the session's `id`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, _: NoParams) -> Result<SessionParams, CommandError> {
        let id = started(session)?.cycle(false);
        id.map(|id| SessionParams { id: Some(id) }).ok_or_else(|| CommandError::new(t!("terminal-session-none")))
    }
}

pub struct SelectTab;

impl Command for SelectTab {
    type Params = TabParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.tab.select";
    const TITLE: &'static str = "Select Terminal Tab";
    const DESCRIPTION: &'static str = "Shows a tab of the Terminal by its index.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: TabParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let index = terminals.tab_or_active(params.tab)?;
        terminals.select(index);
        Ok(NoParams {})
    }
}

pub struct CloseTab;

impl Command for CloseTab {
    type Params = TabParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.tab.close";
    const TITLE: &'static str = "Close Tab";
    const DESCRIPTION: &'static str = "Closes a tab of the Terminal (the one shown by default) and ends every session \
         in it.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: TabParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let index = terminals.tab_or_active(params.tab)?;
        terminals.close_tab(index);
        Ok(NoParams {})
    }
}

pub struct RenameTab;

impl Command for RenameTab {
    type Params = RenameParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.tab.rename";
    const TITLE: &'static str = "Rename Tab";
    const DESCRIPTION: &'static str = "Names a tab of the Terminal (the one shown by default); a `null` or empty \
         `name` gives it back its own title. The name is remembered with the project.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }
    fn run(&self, session: &mut Session, params: RenameParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let index = terminals.tab_or_active(params.tab)?;
        terminals.rename(index, params.name);
        Ok(NoParams {})
    }
}

pub struct RerunCommand;

impl Command for RerunCommand {
    type Params = SessionParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.session.rerun";
    const TITLE: &'static str = "Run Again";
    const DESCRIPTION: &'static str = "Runs again the last command of a terminal that came back after a restart \
         (`rerun` in terminal.session.list), in its new shell, as the tab offers: the person's choice, so it is \
         refused to agents with the code `userOnly`.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<NoParams, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(links::USER_ONLY, t!("terminal-rerun-user-only")));
        }
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        let pane = terminals.pane_mut(id).ok_or_else(|| missing_session(id))?;
        let command = pane.rerun.take().ok_or_else(|| CommandError::new(t!("terminal-rerun-none")))?;
        pane.session.write(format!("{command}\r").into_bytes());
        pane.session.scroll_to_bottom();
        Ok(NoParams {})
    }
}

pub struct DismissRerun;

impl Command for DismissRerun {
    type Params = SessionParams;
    type Output = NoParams;
    const ID: &'static str = "terminal.session.dismissRerun";
    const TITLE: &'static str = "Dismiss";
    const DESCRIPTION: &'static str = "Hides the offer to run the last command again in a terminal that came back \
         after a restart; nothing runs.";

    fn is_available(&self, session: &Session) -> bool {
        has_sessions(session)
    }

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<NoParams, CommandError> {
        let terminals = started(session)?;
        let id = terminals.session_or_focused(params.id)?;
        terminals.pane_mut(id).ok_or_else(|| missing_session(id))?.rerun = None;
        Ok(NoParams {})
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(agents::ListAgents)?;
    registry.register(agents::ReportAgent)?;
    registry.register(RerunCommand)?;
    registry.register(DismissRerun)?;
    registry.register(NewSession)?;
    registry.register(SplitSession)?;
    registry.register(ListSessions)?;
    registry.register(CloseSession)?;
    registry.register(SendText)?;
    registry.register(ReadSession)?;
    registry.register(FocusSession)?;
    registry.register(FocusNext)?;
    registry.register(FocusPrevious)?;
    registry.register(SelectTab)?;
    registry.register(CloseTab)?;
    registry.register(RenameTab)?;
    registry.register(links::ListLinks)?;
    registry.register(selection::GetSelection)?;
    registry.register(selection::CopySelection)?;
    registry.register(selection::SelectAll)?;
    registry.register(selection::CopyLines)?;
    registry.register(selection::PasteClipboard)?;
    registry.register(links::OpenLink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_end_on_a_character_boundary() {
        assert_eq!(tail("short".to_owned(), 10), ("short".to_owned(), false));
        assert_eq!(tail("abcdef".to_owned(), 3), ("def".to_owned(), true));
        assert_eq!(tail("aяб".to_owned(), 3), ("б".to_owned(), true));
    }
}
