pub mod keeper;
pub mod power;

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::tasks::Waker;
use keeper::{Held, Keeper, Plan, SystemKeeper};
use power::{Battery, Power, SystemPower};

pub const PRESETS: &[u32] = &[15, 30, 60, 120, 240];

pub const MAX_MINUTES: u32 = 7 * 24 * 60;

pub const AGENT_IDLE: Duration = Duration::from_secs(120);

pub const BATTERY_EVERY: Duration = Duration::from_secs(60);

const RETRY_AFTER: Duration = Duration::from_secs(10);

pub const TICK: Duration = Duration::from_secs(5);

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Status)?;
    registry.register(Start)?;
    registry.register(Stop)?;
    registry.register(Toggle)?;
    registry.register(Maximum)?;
    Ok(())
}

/// What Caffeine keeps awake; each is a flag of `caffeinate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Assertion {
    /// The system from idle sleep (`-i`).
    Idle,
    /// The display from sleeping (`-d`).
    Display,
    /// The disk from idle sleep (`-m`).
    Disk,
    /// The system from sleeping, on mains power only (`-s`).
    System,
    /// "The user is active": wakes the display and keeps it on (`-u`).
    User,
}

impl Assertion {
    pub const ALL: [Assertion; 5] =
        [Assertion::Idle, Assertion::Display, Assertion::Disk, Assertion::System, Assertion::User];

    pub fn flag(self) -> &'static str {
        match self {
            Assertion::Idle => "-i",
            Assertion::Display => "-d",
            Assertion::Disk => "-m",
            Assertion::System => "-s",
            Assertion::User => "-u",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Assertion::Idle => "idle",
            Assertion::Display => "display",
            Assertion::Disk => "disk",
            Assertion::System => "system",
            Assertion::User => "user",
        }
    }
}

/// What can keep Caffeine on while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Activity {
    /// A run or a test.
    Run,
    /// A debug session.
    Debug,
    /// A background task: go get, cargo, a Git push or pull.
    Task,
    /// Indexing: a language server loading or indexing the project.
    Indexing,
    /// An agent connected over MCP, to a headless or a windowed IDE.
    Agent,
}

impl Activity {
    pub const ALL: [Activity; 5] =
        [Activity::Run, Activity::Debug, Activity::Task, Activity::Indexing, Activity::Agent];

    pub fn id(self) -> &'static str {
        match self {
            Activity::Run => "run",
            Activity::Debug => "debug",
            Activity::Task => "task",
            Activity::Indexing => "indexing",
            Activity::Agent => "agent",
        }
    }
}

/// For how long Caffeine stays on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Length {
    /// Until it is turned off.
    #[default]
    Indefinitely,
    /// For a number of minutes.
    Minutes {
        /// How many.
        minutes: u32,
    },
    /// While something it is told to watch runs.
    While,
}

/// Settings → Tools → Caffeine: what it keeps awake, for how long, and the battery rules; saved
/// with the IDE's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CaffeineSettings {
    /// What to keep awake (the system from idle sleep by default).
    pub what: Vec<Assertion>,
    /// For how long Turn On in the Caffeine tool window, or the start at launch, turns it on.
    pub length: Length,
    /// What keeps it on in the "while something runs" mode.
    #[serde(rename = "while")]
    pub during: Vec<Activity>,
    /// The minutes of the menu's custom duration.
    pub custom_minutes: u32,
    /// Pause on battery power.
    pub off_on_battery: bool,
    /// Pause when the battery is below [`Self::low_battery_level`].
    pub low_battery: bool,
    /// The charge, in percent, below which it pauses.
    pub low_battery_level: u8,
    /// Go on again once the computer is back on mains power; when off, a battery rule turns
    /// Caffeine off instead of pausing it.
    pub resume_on_charge: bool,
    /// Turn Caffeine on when the IDE starts.
    pub start_on_launch: bool,
}

impl Default for CaffeineSettings {
    fn default() -> Self {
        Self {
            what: vec![Assertion::Idle],
            length: Length::Indefinitely,
            during: Activity::ALL.to_vec(),
            custom_minutes: 90,
            off_on_battery: false,
            low_battery: false,
            low_battery_level: 20,
            resume_on_charge: true,
            start_on_launch: false,
        }
    }
}

impl CaffeineSettings {
    pub fn battery_rules(&self) -> bool {
        self.off_on_battery || self.low_battery
    }
}

/// Why Caffeine is on but does not keep the computer awake now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Pause {
    /// The computer runs on its battery and the rule says to pause then.
    OnBattery,
    /// The battery is below the level the rule sets.
    LowBattery,
    /// Nothing it watches runs ("while something runs").
    Waiting,
}

/// Why Caffeine last turned off by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Ended {
    /// Its time was over.
    Expired,
    /// A battery rule turned it off (going on again on mains power is off).
    Battery,
}

/// What Caffeine does, as commands report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaffeineStatus {
    /// Whether this system has a program to keep it awake.
    pub available: bool,
    /// The program: `caffeinate` or `systemd-inhibit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Whether Caffeine is on.
    pub on: bool,
    /// Whether it keeps the computer awake right now (its program runs).
    pub awake: bool,
    /// What it keeps awake.
    pub what: Vec<Assertion>,
    /// For how long it is on, while it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<Length>,
    /// The seconds left, when it is on for a time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_left: Option<u64>,
    /// What keeps it on, while it is on "while something runs".
    #[serde(rename = "while", skip_serializing_if = "Vec::is_empty")]
    pub during: Vec<Activity>,
    /// What runs now, of what it can watch.
    pub busy: Vec<Activity>,
    /// Why it is on but not keeping the computer awake now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<Pause>,
    /// Why it last turned off by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
    /// The battery as last read, while a battery rule needs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<Battery>,
    /// The process id of the program keeping it awake.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Why the program could not start, when it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether it is on at maximum: every "keep awake" option, until turned off.
    pub maximum: bool,
}

/// What to turn Caffeine on with; what is left out comes from the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StartParams {
    /// What to keep awake: `idle` (the system from idle sleep, -i), `display` (-d), `disk` (-m),
    /// `system` (the system, on mains power only, -s), `user` (the user is active, -u).
    pub what: Option<Vec<Assertion>>,
    /// For how many minutes (1 to 10080); 0 is until turned off.
    pub minutes: Option<u32>,
    /// Stay on only while one of these runs: `run` (runs and tests), `debug`, `task` (background
    /// tasks), `indexing`, `agent` (an agent connected over MCP); an empty list takes the settings'.
    #[serde(rename = "while")]
    pub during: Option<Vec<Activity>>,
    /// Maximum caffeine: every "keep awake" option, until turned off; `what`, `minutes` and
    /// `while` are then not used. The battery rules stay as the settings say.
    pub maximum: bool,
}

impl StartParams {
    pub fn maximum() -> Self {
        Self { maximum: true, ..Self::default() }
    }
}

#[derive(Clone, Debug)]
struct Activation {
    what: Vec<Assertion>,
    length: Length,
    until: Option<Instant>,
    during: Vec<Activity>,
}

struct Inner {
    settings: CaffeineSettings,
    keeper: Box<dyn Keeper>,
    power: Arc<dyn Power>,
    power_in_background: bool,
    power_slot: Arc<Mutex<(Option<Option<Battery>>, bool)>>,
    activation: Option<Activation>,
    held: Option<Running>,
    battery: Option<Battery>,
    battery_read: Option<Instant>,
    busy: Vec<Activity>,
    paused: Option<Pause>,
    ended: Option<Ended>,
    failed: Option<(Instant, String)>,
    pid: u32,
    who: String,
    me: Weak<Mutex<Inner>>,
    waker: Option<Waker>,
}

struct Running {
    plan: Plan,
    until: Option<Instant>,
    held: Box<dyn Held>,
    _timer: Option<Timer>,
}

#[derive(Default)]
struct Timer {
    cancelled: Arc<(Mutex<bool>, Condvar)>,
}

impl Timer {
    fn start(inner: Weak<Mutex<Inner>>, until: Instant) -> Option<Self> {
        let timer = Timer::default();
        let cancelled = Arc::clone(&timer.cancelled);
        let spawned = std::thread::Builder::new().name("zen-caffeine-timer".to_owned()).spawn(move || {
            let (lock, signal) = &*cancelled;
            let mut stop = lock.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                let now = Instant::now();
                if *stop || now >= until {
                    break;
                }
                stop = signal.wait_timeout(stop, until - now).unwrap_or_else(PoisonError::into_inner).0;
            }
            if *stop {
                return;
            }
            drop(stop);
            if let Some(inner) = inner.upgrade() {
                let waker = {
                    let mut inner = inner.lock().unwrap_or_else(PoisonError::into_inner);
                    inner.update(Instant::now());
                    inner.waker.clone()
                };
                if let Some(waker) = waker {
                    waker();
                }
            }
        });
        spawned.is_ok().then_some(timer)
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let (lock, signal) = &*self.cancelled;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        signal.notify_all();
    }
}

#[derive(Clone)]
pub struct Caffeine {
    inner: Arc<Mutex<Inner>>,
}

impl Default for Caffeine {
    fn default() -> Self {
        let mut caffeine = Self::with(Box::new(SystemKeeper::detect()), Arc::new(SystemPower));
        caffeine.lock().power_in_background = true;
        caffeine.set_who("Zen IDE");
        caffeine
    }
}

impl std::fmt::Debug for Caffeine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Caffeine").finish_non_exhaustive()
    }
}

fn seconds_until(until: Instant, now: Instant) -> u64 {
    let left = until.saturating_duration_since(now);
    left.as_secs() + u64::from(left.subsec_nanos() > 0)
}

fn normalized(what: &[Assertion]) -> Vec<Assertion> {
    let what: Vec<Assertion> = Assertion::ALL.into_iter().filter(|kind| what.contains(kind)).collect();
    if what.is_empty() { vec![Assertion::Idle] } else { what }
}

fn normalized_during(during: &[Activity]) -> Vec<Activity> {
    Activity::ALL.into_iter().filter(|kind| during.contains(kind)).collect()
}

impl Caffeine {
    pub fn with(keeper: Box<dyn Keeper>, power: Arc<dyn Power>) -> Self {
        Self {
            inner: Arc::new_cyclic(|me| {
                Mutex::new(Inner {
                    settings: CaffeineSettings::default(),
                    keeper,
                    power,
                    power_in_background: false,
                    power_slot: Arc::default(),
                    activation: None,
                    held: None,
                    battery: None,
                    battery_read: None,
                    busy: Vec::new(),
                    paused: None,
                    ended: None,
                    failed: None,
                    pid: std::process::id(),
                    who: String::new(),
                    me: me.clone(),
                    waker: None,
                })
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_who(&mut self, who: &str) {
        who.clone_into(&mut self.lock().who);
    }

    pub fn set_waker(&self, waker: Waker) {
        self.lock().waker = Some(waker);
    }

    pub fn settings(&self) -> CaffeineSettings {
        self.lock().settings.clone()
    }

    pub fn configure(&self, settings: CaffeineSettings, now: Instant) {
        let mut inner = self.lock();
        let old = std::mem::replace(&mut inner.settings, settings);
        let new = inner.settings.clone();
        if let Some(activation) = inner.activation.as_mut() {
            if old.what != new.what {
                activation.what = normalized(&new.what);
            }
            if old.during != new.during && activation.length == Length::While {
                activation.during = normalized_during(&new.during);
            }
        }
        if !new.battery_rules() {
            inner.battery = None;
            inner.battery_read = None;
        }
        inner.update(now);
    }

    pub fn available(&self) -> bool {
        self.lock().keeper.backend().is_some()
    }

    pub fn start(&self, params: &StartParams, now: Instant) -> Result<CaffeineStatus, CommandError> {
        let mut inner = self.lock();
        if inner.keeper.backend().is_none() {
            return Err(CommandError::new(t!("workbench-caffeine-unavailable")));
        }
        if let Some(minutes) = params.minutes
            && minutes > MAX_MINUTES
        {
            return Err(CommandError::new(t!("workbench-caffeine-minutes", max = MAX_MINUTES)));
        }
        let settings = inner.settings.clone();
        if params.maximum {
            inner.activation = Some(Activation {
                what: Assertion::ALL.to_vec(),
                length: Length::Indefinitely,
                until: None,
                during: Vec::new(),
            });
            inner.ended = None;
            inner.failed = None;
            inner.update(now);
            return Ok(inner.status(now));
        }
        let what = normalized(params.what.as_deref().unwrap_or(&settings.what));
        let length = match (params.minutes, &params.during) {
            (Some(0), _) => Length::Indefinitely,
            (Some(minutes), _) => Length::Minutes { minutes },
            (None, Some(_)) => Length::While,
            (None, None) => match settings.length {
                Length::Minutes { minutes: 0 } => Length::Indefinitely,
                length => length,
            },
        };
        let during = match &params.during {
            Some(during) if !during.is_empty() => normalized_during(during),
            _ => normalized_during(&settings.during),
        };
        if length == Length::While && during.is_empty() {
            return Err(CommandError::new(t!("workbench-caffeine-while-empty")));
        }
        let until = match length {
            Length::Minutes { minutes } => Some(now + Duration::from_secs(u64::from(minutes) * 60)),
            _ => None,
        };
        inner.activation = Some(Activation { what, length, until, during });
        inner.ended = None;
        inner.failed = None;
        inner.update(now);
        Ok(inner.status(now))
    }

    pub fn stop(&self, now: Instant) -> CaffeineStatus {
        let mut inner = self.lock();
        inner.activation = None;
        inner.ended = None;
        inner.update(now);
        inner.status(now)
    }

    pub fn toggle(&self, now: Instant) -> Result<CaffeineStatus, CommandError> {
        if self.lock().activation.is_some() { Ok(self.stop(now)) } else { self.start(&StartParams::default(), now) }
    }

    pub fn maximum(&self, now: Instant) -> Result<CaffeineStatus, CommandError> {
        if self.status(now).maximum { Ok(self.stop(now)) } else { self.start(&StartParams::maximum(), now) }
    }

    pub fn observe(&self, busy: &[Activity], now: Instant) {
        let mut inner = self.lock();
        inner.busy = normalized_during(busy);
        inner.update(now);
    }

    pub fn tick(&self, now: Instant) {
        self.lock().update(now);
    }

    pub fn status(&self, now: Instant) -> CaffeineStatus {
        let mut inner = self.lock();
        if inner.expired(now) {
            inner.update(now);
        }
        inner.status(now)
    }

    pub fn next_check(&self, now: Instant) -> Option<Duration> {
        let inner = self.lock();
        let activation = inner.activation.as_ref()?;
        Some(activation.until.map_or(TICK, |until| until.saturating_duration_since(now).min(TICK)))
    }

    pub fn is_on(&self) -> bool {
        self.lock().activation.is_some()
    }

    pub fn is_awake(&self) -> bool {
        self.lock().held.is_some()
    }

    pub fn shutdown(&self) {
        let mut inner = self.lock();
        inner.activation = None;
        if let Some(mut running) = inner.held.take() {
            running.held.stop();
        }
    }
}

impl Inner {
    fn battery_blocks(&self) -> Option<Pause> {
        let battery = self.battery.filter(|_| self.settings.battery_rules())?;
        if !battery.on_battery {
            return None;
        }
        if self.settings.off_on_battery {
            return Some(Pause::OnBattery);
        }
        let low = battery.percent.is_some_and(|percent| percent < self.settings.low_battery_level);
        (self.settings.low_battery && low).then_some(Pause::LowBattery)
    }

    fn read_battery(&mut self, now: Instant) {
        if !self.settings.battery_rules() || self.activation.is_none() {
            return;
        }
        if self.power_in_background {
            let mut slot = self.power_slot.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(reading) = slot.0.take() {
                self.battery = reading;
                self.battery_read = Some(now);
            }
            let due = self.battery_read.is_none_or(|read| now.saturating_duration_since(read) >= BATTERY_EVERY);
            if due && !slot.1 {
                slot.1 = true;
                let power = Arc::clone(&self.power);
                let result = Arc::clone(&self.power_slot);
                let spawned = std::thread::Builder::new().name("zen-caffeine-battery".to_owned()).spawn(move || {
                    let battery = power.read();
                    let mut slot = result.lock().unwrap_or_else(PoisonError::into_inner);
                    *slot = (Some(battery), false);
                });
                if spawned.is_err() {
                    slot.1 = false;
                }
            }
        } else if self.battery_read.is_none_or(|read| now.saturating_duration_since(read) >= BATTERY_EVERY) {
            self.battery = self.power.read();
            self.battery_read = Some(now);
        }
    }

    fn expired(&self, now: Instant) -> bool {
        self.activation.as_ref().and_then(|activation| activation.until).is_some_and(|until| until <= now)
    }

    fn update(&mut self, now: Instant) {
        if self.expired(now) {
            self.activation = None;
            self.ended = Some(Ended::Expired);
        }
        self.read_battery(now);
        let mut pause = None;
        if let Some(activation) = &self.activation {
            pause = self.battery_blocks();
            if pause.is_some() && !self.settings.resume_on_charge {
                self.activation = None;
                self.ended = Some(Ended::Battery);
                pause = None;
            } else if pause.is_none()
                && activation.length == Length::While
                && !activation.during.iter().any(|kind| self.busy.contains(kind))
            {
                pause = Some(Pause::Waiting);
            }
        }
        self.paused = pause;
        let wanted = self.activation.as_ref().filter(|_| pause.is_none()).map(|activation| {
            let plan = Plan {
                what: activation.what.clone(),
                seconds: activation.until.map(|until| seconds_until(until, now)),
                pid: self.pid,
                who: self.who.clone(),
            };
            (plan, activation.until)
        });
        if let Some(running) = self.held.as_mut()
            && !running.held.alive()
        {
            self.held = None;
            self.failed = Some((now, String::new()));
        }
        match (wanted, self.held.as_ref()) {
            (None, Some(_)) => {
                if let Some(mut running) = self.held.take() {
                    running.held.stop();
                }
            }
            (Some((plan, until)), Some(running)) if plan.what != running.plan.what || until != running.until => {
                let old = self.held.take();
                self.spawn(plan, until, now);
                if let Some(mut old) = old {
                    old.held.stop();
                }
            }
            (Some((plan, until)), None) => {
                let waiting =
                    self.failed.as_ref().is_some_and(|(at, _)| now.saturating_duration_since(*at) < RETRY_AFTER);
                if !waiting {
                    self.spawn(plan, until, now);
                }
            }
            _ => {}
        }
    }

    fn spawn(&mut self, plan: Plan, until: Option<Instant>, now: Instant) {
        match self.keeper.start(&plan) {
            Ok(held) => {
                let timer = until.and_then(|until| Timer::start(self.me.clone(), until));
                self.held = Some(Running { plan, until, held, _timer: timer });
                self.failed = None;
            }
            Err(error) => self.failed = Some((now, error.to_string())),
        }
    }

    fn status(&self, now: Instant) -> CaffeineStatus {
        let activation = self.activation.as_ref();
        CaffeineStatus {
            available: self.keeper.backend().is_some(),
            backend: self.keeper.backend().map(str::to_owned),
            on: activation.is_some(),
            awake: self.held.is_some(),
            what: activation.map_or_else(|| normalized(&self.settings.what), |activation| activation.what.clone()),
            length: activation.map(|activation| activation.length),
            seconds_left: activation.and_then(|activation| activation.until).map(|until| seconds_until(until, now)),
            during: activation
                .filter(|activation| activation.length == Length::While)
                .map(|activation| activation.during.clone())
                .unwrap_or_default(),
            busy: self.busy.clone(),
            paused: self.paused,
            ended: self.ended,
            battery: self.battery.filter(|_| self.settings.battery_rules()),
            pid: self.held.as_ref().and_then(|running| running.held.pid()),
            error: self.failed.as_ref().map(|(_, error)| error.clone()).filter(|error| !error.is_empty()),
            maximum: activation.is_some_and(|activation| {
                activation.length == Length::Indefinitely && activation.what == Assertion::ALL
            }),
        }
    }
}

pub fn session_activity(session: &Session) -> Vec<Activity> {
    let mut busy = Vec::new();
    for work in crate::exit::running_work(session) {
        busy.push(match work.kind {
            crate::exit::WorkKind::Run => Activity::Run,
            crate::exit::WorkKind::Debug => Activity::Debug,
            crate::exit::WorkKind::Git | crate::exit::WorkKind::DependencyUpdate | crate::exit::WorkKind::Task => {
                Activity::Task
            }
        });
    }
    let indexing = session.state::<crate::tasks::Tasks>().is_some_and(|tasks| {
        tasks.list().iter().any(|task| {
            task.state == crate::tasks::TaskState::Running && task.source == crate::exit::INDEX && !task.paused
        })
    });
    let languages = session.state::<crate::language::Languages>().is_some_and(|languages| languages.busy().is_some());
    if indexing || languages {
        busy.push(Activity::Indexing);
    }
    normalized_during(&busy)
}

fn caffeine(session: &Session) -> Result<Caffeine, CommandError> {
    session.state::<Caffeine>().cloned().ok_or_else(|| CommandError::new(t!("workbench-caffeine-unavailable")))
}

pub struct Status;

impl Command for Status {
    type Params = NoParams;
    type Output = CaffeineStatus;
    const ID: &'static str = "caffeine.status.get";
    const TITLE: &'static str = "Caffeine Status";
    const DESCRIPTION: &'static str = "Tells whether Caffeine keeps the computer awake: whether it is on and awake now, \
         what it keeps awake (idle, display, disk, system, user), for how long (secondsLeft) or while what runs, \
         what runs now (busy), why it pauses (onBattery, lowBattery, waiting), the battery, and the program's pid.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CaffeineStatus, CommandError> {
        Ok(caffeine(session)?.status(Instant::now()))
    }
}

pub struct Start;

impl Command for Start {
    type Params = StartParams;
    type Output = CaffeineStatus;
    const ID: &'static str = "caffeine.awake.start";
    const TITLE: &'static str = "Start Caffeine";
    const DESCRIPTION: &'static str = "Keeps the computer awake with the system's caffeinate (systemd-inhibit on \
         Linux), tied to the IDE's process: {what?: [idle|display|disk|system|user], minutes?: 0 = until stopped, \
         while?: [run|debug|task|indexing|agent]: only while one of these runs, maximum?: true = every option \
         until stopped}. What is left out comes from Settings → Tools → Caffeine; the battery rules always do. \
         Returns the status.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<CaffeineStatus, CommandError> {
        caffeine(session)?.start(&params, Instant::now())
    }
}

pub struct Stop;

impl Command for Stop {
    type Params = NoParams;
    type Output = CaffeineStatus;
    const ID: &'static str = "caffeine.awake.stop";
    const TITLE: &'static str = "Stop Caffeine";
    const DESCRIPTION: &'static str =
        "Turns Caffeine off: the computer may sleep again as the system settings say. Returns the status.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CaffeineStatus, CommandError> {
        Ok(caffeine(session)?.stop(Instant::now()))
    }
}

pub struct Toggle;

impl Command for Toggle {
    type Params = NoParams;
    type Output = CaffeineStatus;
    const ID: &'static str = "caffeine.awake.toggle";
    const TITLE: &'static str = "Toggle Caffeine";
    const DESCRIPTION: &'static str = "Turns Caffeine off when it is on, else on as Settings → Tools → Caffeine \
         says (what to keep awake and for how long). Returns the status.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CaffeineStatus, CommandError> {
        caffeine(session)?.toggle(Instant::now())
    }
}

pub struct Maximum;

impl Command for Maximum {
    type Params = NoParams;
    type Output = CaffeineStatus;
    const ID: &'static str = "caffeine.awake.maximum";
    const TITLE: &'static str = "Maximum Caffeine";
    const DESCRIPTION: &'static str = "Maximum caffeine, as the button in the Caffeine tool window: turns Caffeine on \
         with every keep-awake option (idle, display, disk, system, user) until turned off, or off when it is on at \
         maximum already (status.maximum). The battery rules stay as the settings say. Returns the status.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CaffeineStatus, CommandError> {
        caffeine(session)?.maximum(Instant::now())
    }
}

#[cfg(test)]
mod tests;
