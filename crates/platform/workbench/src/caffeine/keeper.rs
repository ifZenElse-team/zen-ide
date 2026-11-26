use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use super::Assertion;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub what: Vec<Assertion>,
    pub seconds: Option<u64>,
    pub pid: u32,
    pub who: String,
}

pub trait Keeper: Send {
    fn backend(&self) -> Option<&'static str>;
    fn start(&mut self, plan: &Plan) -> std::io::Result<Box<dyn Held>>;
}

pub trait Held: Send {
    fn alive(&mut self) -> bool;
    fn pid(&self) -> Option<u32>;
    fn stop(&mut self);
}

pub fn caffeinate_args(plan: &Plan) -> Vec<String> {
    let mut args: Vec<String> = Assertion::ALL
        .iter()
        .filter(|assertion| plan.what.contains(assertion))
        .map(|assertion| assertion.flag().to_owned())
        .collect();
    if let Some(seconds) = plan.seconds {
        args.extend(["-t".to_owned(), seconds.max(1).to_string()]);
    }
    args.extend(["-w".to_owned(), plan.pid.to_string()]);
    args
}

pub fn systemd_what(plan: &Plan) -> String {
    let mut what = Vec::new();
    let has = |assertion| plan.what.contains(&assertion);
    if has(Assertion::Idle) || has(Assertion::System) || has(Assertion::Disk) {
        what.push("sleep");
    }
    if has(Assertion::Display) || has(Assertion::User) || what.is_empty() {
        what.push("idle");
    }
    what.join(":")
}

pub fn systemd_inhibit_args(plan: &Plan) -> Vec<String> {
    let mut args = vec![
        format!("--what={}", systemd_what(plan)),
        format!("--who={}", plan.who),
        "--why=Caffeine".to_owned(),
        "--mode=block".to_owned(),
    ];
    if let Some(seconds) = plan.seconds {
        args.extend(["timeout".to_owned(), seconds.max(1).to_string()]);
    }
    args.extend(["tail".to_owned(), format!("--pid={}", plan.pid), "-f".to_owned(), "/dev/null".to_owned()]);
    args
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Caffeinate,
    SystemdInhibit,
}

pub struct SystemKeeper {
    program: Option<(Kind, PathBuf)>,
}

impl Default for SystemKeeper {
    fn default() -> Self {
        Self::detect()
    }
}

impl SystemKeeper {
    pub fn detect() -> Self {
        let found = |kind: Kind, paths: &[&str]| {
            paths.iter().map(PathBuf::from).find(|path| path.is_file()).map(|path| (kind, path))
        };
        let program = if cfg!(target_os = "macos") {
            found(Kind::Caffeinate, &["/usr/bin/caffeinate"])
        } else if cfg!(target_os = "linux") {
            found(Kind::SystemdInhibit, &["/usr/bin/systemd-inhibit", "/bin/systemd-inhibit"])
        } else {
            None
        };
        Self { program }
    }

    pub fn caffeinate(path: PathBuf) -> Self {
        Self { program: Some((Kind::Caffeinate, path)) }
    }
}

impl Keeper for SystemKeeper {
    fn backend(&self) -> Option<&'static str> {
        self.program.as_ref().map(|(kind, _)| match kind {
            Kind::Caffeinate => "caffeinate",
            Kind::SystemdInhibit => "systemd-inhibit",
        })
    }

    fn start(&mut self, plan: &Plan) -> std::io::Result<Box<dyn Held>> {
        let Some((kind, program)) = &self.program else {
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no keeper on this system"));
        };
        let args = match kind {
            Kind::Caffeinate => caffeinate_args(plan),
            Kind::SystemdInhibit => systemd_inhibit_args(plan),
        };
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Box::new(ChildHeld(Some(child))))
    }
}

struct ChildHeld(Option<Child>);

impl Held for ChildHeld {
    fn alive(&mut self) -> bool {
        self.0.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    fn pid(&self) -> Option<u32> {
        self.0.as_ref().map(Child::id)
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildHeld {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(any(test, feature = "fake"))]
#[derive(Clone, Default)]
pub struct FakeKeeper {
    pub started: std::sync::Arc<std::sync::Mutex<Vec<Plan>>>,
    pub running: std::sync::Arc<std::sync::Mutex<Vec<bool>>>,
    pub missing: bool,
}

#[cfg(any(test, feature = "fake"))]
impl FakeKeeper {
    pub fn plans(&self) -> Vec<Plan> {
        self.started.lock().map(|started| started.clone()).unwrap_or_default()
    }

    pub fn live(&self) -> usize {
        self.running.lock().map(|running| running.iter().filter(|alive| **alive).count()).unwrap_or(0)
    }

    pub fn end_last(&self) {
        if let Ok(mut running) = self.running.lock()
            && let Some(last) = running.last_mut()
        {
            *last = false;
        }
    }
}

#[cfg(any(test, feature = "fake"))]
impl Keeper for FakeKeeper {
    fn backend(&self) -> Option<&'static str> {
        (!self.missing).then_some("fake")
    }

    fn start(&mut self, plan: &Plan) -> std::io::Result<Box<dyn Held>> {
        if self.missing {
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no keeper"));
        }
        let mut started = self.started.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        started.push(plan.clone());
        let mut running = self.running.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        running.push(true);
        Ok(Box::new(FakeHeld { index: running.len() - 1, running: std::sync::Arc::clone(&self.running) }))
    }
}

#[cfg(any(test, feature = "fake"))]
struct FakeHeld {
    index: usize,
    running: std::sync::Arc<std::sync::Mutex<Vec<bool>>>,
}

#[cfg(any(test, feature = "fake"))]
impl Held for FakeHeld {
    fn alive(&mut self) -> bool {
        self.running.lock().is_ok_and(|running| running.get(self.index).copied().unwrap_or(false))
    }

    fn pid(&self) -> Option<u32> {
        u32::try_from(self.index).ok().map(|index| 90_000 + index)
    }

    fn stop(&mut self) {
        if let Ok(mut running) = self.running.lock()
            && let Some(alive) = running.get_mut(self.index)
        {
            *alive = false;
        }
    }
}

#[cfg(any(test, feature = "fake"))]
impl Drop for FakeHeld {
    fn drop(&mut self) {
        self.stop();
    }
}
