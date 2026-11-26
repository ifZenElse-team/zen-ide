use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use crate::Workbench;
use crate::language::Languages;

pub const INTERVAL: Duration = Duration::from_secs(2);

pub const MEMORY_INTERVAL: Duration = Duration::from_secs(3);

const LINGER: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub resident: u64,
    pub cpu: Duration,
    pub name: String,
}

pub fn processes() -> Vec<ProcessInfo> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/bin/ps")
            .args(["-A", "-o", "pid=,ppid=,rss=,time=,comm="])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        output.map(|output| parse_ps(&String::from_utf8_lossy(&output.stdout))).unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    {
        linux::processes()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

pub fn resident_bytes(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse::<u64>().ok().map(|kilobytes| kilobytes * 1024)
    }
    #[cfg(target_os = "linux")]
    {
        linux::resident(pid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

pub fn parse_ps(output: &str) -> Vec<ProcessInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            let resident = fields.next()?.parse::<u64>().ok()? * 1024;
            let cpu = parse_cpu_time(fields.next()?)?;
            let command = fields.collect::<Vec<_>>().join(" ");
            let name = command.rsplit('/').next().unwrap_or_default().to_owned();
            Some(ProcessInfo { pid, parent, resident, cpu, name })
        })
        .collect()
}

pub fn parse_cpu_time(text: &str) -> Option<Duration> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<f64>().ok()?, rest),
        None => (0.0, text),
    };
    let mut seconds = 0.0;
    for part in rest.split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(Duration::from_secs_f64(days * 86_400.0 + seconds))
}

#[cfg(target_os = "linux")]
mod linux {
    use std::time::Duration;

    use super::ProcessInfo;

    const TICKS: f64 = 100.0;
    const PAGE: u64 = 4096;

    pub(super) fn processes() -> Vec<ProcessInfo> {
        let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
        entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(process)
            .collect()
    }

    fn process(pid: u32) -> Option<ProcessInfo> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat[open + 1..close].to_owned();
        let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        let parent = fields.get(1)?.parse().ok()?;
        let ticks = fields.get(11)?.parse::<f64>().ok()? + fields.get(12)?.parse::<f64>().ok()?;
        let pages = fields.get(21)?.parse::<u64>().ok()?;
        Some(ProcessInfo { pid, parent, resident: pages * PAGE, cpu: Duration::from_secs_f64(ticks / TICKS), name })
    }

    pub(super) fn resident(pid: u32) -> Option<u64> {
        let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        statm.split_whitespace().nth(1)?.parse::<u64>().ok().map(|pages| pages * PAGE)
    }
}

/// The kind of part of the IDE a row of the Activity Monitor measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Subsystem {
    /// The IDE's own process, every thread of it.
    Ide,
    /// The file and text index, inside the IDE's process: its worker threads' time and its tables.
    Index,
    /// A language server's process, with the processes it started.
    LanguageServer,
    /// The Git processes the IDE started.
    Vcs,
    /// Other processes the IDE started: runs, terminals, tools; by program.
    Process,
}

/// One row of the Activity Monitor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    /// What it measures.
    pub subsystem: Subsystem,
    /// Its name: the IDE's program, `index`, the server's id, `git`, or the program's name.
    pub name: String,
    /// The processes it sums, the first the main one; empty for the index.
    pub pids: Vec<u32>,
    /// The processor use over the last interval, in percent of one core.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f32>,
    /// The resident memory, in bytes (the index's: an estimate of its tables).
    pub memory_bytes: u64,
}

/// What `diagnostics.activity.get` answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityReport {
    /// Whether processes have been measured yet; the first measurement takes a moment.
    pub measured: bool,
    /// The milliseconds between the last two measurements, which the processor use is over.
    pub interval_ms: u64,
    /// The rows: the IDE, the index, each language server, Git, the other processes.
    pub rows: Vec<ActivityRow>,
}

#[derive(Default)]
struct Shared {
    report: ActivityReport,
    asked: Option<Instant>,
    running: bool,
    servers: Vec<(String, u32)>,
    index: Option<zen_index::FileIndex>,
}

#[derive(Clone, Default)]
pub struct ActivityMonitor {
    shared: Arc<Mutex<Shared>>,
}

impl ActivityMonitor {
    pub fn report(&self, servers: Vec<(String, u32)>, index: Option<zen_index::FileIndex>) -> ActivityReport {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.asked = Some(Instant::now());
        shared.servers = servers;
        shared.index = index;
        if !shared.running {
            shared.running = true;
            let monitor = self.clone();
            let started =
                std::thread::Builder::new().name("zen-activity-monitor".to_owned()).spawn(move || monitor.run());
            if started.is_err() {
                shared.running = false;
            }
        }
        shared.report.clone()
    }

    fn run(&self) {
        let mut previous: Option<(Instant, HashMap<u32, Duration>, Duration)> = None;
        loop {
            let (servers, index) = {
                let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
                if shared.asked.is_none_or(|asked| asked.elapsed() > LINGER) {
                    shared.running = false;
                    return;
                }
                (shared.servers.clone(), shared.index.clone())
            };
            let now = Instant::now();
            let all = processes();
            let index_busy = index.as_ref().map(zen_index::FileIndex::busy_time).unwrap_or_default();
            let index_memory = index.as_ref().map_or(0, zen_index::FileIndex::memory) as u64;
            let times: HashMap<u32, Duration> = all.iter().map(|process| (process.pid, process.cpu)).collect();
            let report = measure(
                &all,
                std::process::id(),
                &servers,
                (index_busy, index_memory),
                previous.as_ref().map(|(at, times, busy)| (now.saturating_duration_since(*at), times, *busy)),
            );
            previous = Some((now, times, index_busy));
            self.shared.lock().unwrap_or_else(PoisonError::into_inner).report = report;
            std::thread::sleep(INTERVAL);
        }
    }
}

pub fn measure(
    all: &[ProcessInfo],
    ide: u32,
    servers: &[(String, u32)],
    index: (Duration, u64),
    before: Option<(Duration, &HashMap<u32, Duration>, Duration)>,
) -> ActivityReport {
    let by_pid: HashMap<u32, &ProcessInfo> = all.iter().map(|process| (process.pid, process)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in all {
        children.entry(process.parent).or_default().push(process.pid);
    }
    let tree = |root: u32| -> Vec<u32> {
        let mut found = vec![root];
        let mut at = 0;
        while at < found.len() {
            if let Some(more) = children.get(&found[at]) {
                found.extend(more.iter().copied().filter(|pid| *pid != root));
            }
            at += 1;
        }
        found
    };
    let percent = |pids: &[u32]| -> Option<f32> {
        let (interval, times, _) = before?;
        if interval.is_zero() {
            return None;
        }
        let used: f64 = pids
            .iter()
            .filter_map(|pid| {
                let now = by_pid.get(pid)?.cpu;
                Some(now.saturating_sub(times.get(pid).copied().unwrap_or_default()).as_secs_f64())
            })
            .sum();
        Some((used / interval.as_secs_f64() * 100.0) as f32)
    };
    let memory =
        |pids: &[u32]| -> u64 { pids.iter().filter_map(|pid| by_pid.get(pid)).map(|process| process.resident).sum() };
    let row = |subsystem, name: String, pids: Vec<u32>| ActivityRow {
        subsystem,
        name,
        cpu_percent: percent(&pids),
        memory_bytes: memory(&pids),
        pids,
    };
    let name = by_pid.get(&ide).map_or_else(String::new, |process| process.name.clone());
    let mut rows = vec![row(Subsystem::Ide, name, vec![ide])];
    let index_percent = before.and_then(|(interval, _, busy)| {
        (!interval.is_zero())
            .then(|| (index.0.saturating_sub(busy).as_secs_f64() / interval.as_secs_f64() * 100.0) as f32)
    });
    rows.push(ActivityRow {
        subsystem: Subsystem::Index,
        name: "index".to_owned(),
        pids: Vec::new(),
        cpu_percent: index_percent,
        memory_bytes: index.1,
    });
    let server_pids: BTreeMap<u32, &str> = servers.iter().map(|(id, pid)| (*pid, id.as_str())).collect();
    for (id, pid) in servers {
        if by_pid.contains_key(pid) {
            rows.push(row(Subsystem::LanguageServer, id.clone(), tree(*pid)));
        }
    }
    let mut vcs = Vec::new();
    let mut others: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for child in children.get(&ide).into_iter().flatten() {
        if server_pids.contains_key(child) {
            continue;
        }
        let Some(process) = by_pid.get(child) else { continue };
        if process.name == "git" {
            vcs.extend(tree(*child));
        } else {
            others.entry(process.name.clone()).or_default().extend(tree(*child));
        }
    }
    rows.push(row(Subsystem::Vcs, "git".to_owned(), vcs));
    for (program, pids) in others {
        rows.push(row(Subsystem::Process, program, pids));
    }
    let interval_ms = before.map_or(0, |(interval, _, _)| u64::try_from(interval.as_millis()).unwrap_or(u64::MAX));
    ActivityReport { measured: true, interval_ms, rows }
}

/// The parameters of `diagnostics.activity.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityParams {
    /// Wait (up to about five seconds) until a measurement with the processor use is in.
    #[serde(default)]
    pub wait: bool,
}

pub struct ActivityGet;

impl Command for ActivityGet {
    type Params = ActivityParams;
    type Output = ActivityReport;
    const ID: &'static str = "diagnostics.activity.get";
    const TITLE: &'static str = "Activity Monitor";
    const DESCRIPTION: &'static str = "The processor and memory use of the IDE by part, as Help → Diagnostic Tools → \
         Activity Monitor… shows it: the IDE's process, the index (its worker threads' time and an estimate of its \
         tables), each language server's process with what it started, the Git processes and the other processes \
         the IDE started, by program. Measured every 2 s while asked; cpuPercent is over the last interval, in \
         percent of one core, and comes from the second measurement on (wait: true waits for it).";

    fn run(&self, session: &mut Session, params: ActivityParams) -> Result<ActivityReport, CommandError> {
        let servers = session.state::<Languages>().map(Languages::server_processes).unwrap_or_default();
        let index = session.state::<Workbench>().and_then(|workbench| workbench.index.clone());
        let monitor = session.state_mut::<ActivityMonitor>().clone();
        let mut report = monitor.report(servers.clone(), index.clone());
        if params.wait {
            let until = Instant::now() + Duration::from_secs(5);
            while report.interval_ms == 0 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
                report = monitor.report(servers.clone(), index.clone());
            }
        }
        Ok(report)
    }
}

/// What `diagnostics.memory.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryReport {
    /// The IDE's process id.
    pub pid: u32,
    /// Its resident memory in bytes, when the system says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_bytes: Option<u64>,
}

static MEMORY: Mutex<(Option<Instant>, Option<u64>)> = Mutex::new((None, None));

pub fn current_memory() -> Option<u64> {
    let mut memory = MEMORY.lock().unwrap_or_else(PoisonError::into_inner);
    if memory.0.is_none_or(|at| at.elapsed() >= MEMORY_INTERVAL) {
        memory.0 = Some(Instant::now());
        let _ = std::thread::Builder::new().name("zen-memory-indicator".to_owned()).spawn(|| {
            let measured = resident_bytes(std::process::id());
            MEMORY.lock().unwrap_or_else(PoisonError::into_inner).1 = measured;
        });
    }
    memory.1
}

pub struct MemoryGet;

impl Command for MemoryGet {
    type Params = NoParams;
    type Output = MemoryReport;
    const ID: &'static str = "diagnostics.memory.get";
    const TITLE: &'static str = "Memory Indicator";
    const DESCRIPTION: &'static str = "The resident memory of the IDE's process, as the status bar's Memory Indicator \
         shows it (measured now).";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<MemoryReport, CommandError> {
        let pid = std::process::id();
        Ok(MemoryReport { pid, resident_bytes: resident_bytes(pid) })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ActivityGet)?;
    registry.register(MemoryGet)?;
    Ok(())
}

#[cfg(test)]
mod tests;
