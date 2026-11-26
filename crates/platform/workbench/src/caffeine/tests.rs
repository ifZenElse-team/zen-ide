use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use super::keeper::{FakeKeeper, Plan, caffeinate_args, systemd_inhibit_args};
use super::power::{Battery, FakePower, parse_pmset, parse_power_supplies};
use super::*;

const ON_BATTERY: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t82%; discharging; 4:12 remaining present: true\n";
const LOW_BATTERY: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t15%; discharging; 0:40 remaining present: true\n";
const CHARGING: &str =
    "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t64%; charging; 1:02 remaining present: true\n";
const DESKTOP: &str = "Now drawing from 'AC Power'\n";

fn plan(what: &[Assertion], seconds: Option<u64>) -> Plan {
    Plan { what: what.to_vec(), seconds, pid: 4242, who: "GoGarden".to_owned() }
}

fn started(what: &[Assertion], seconds: Option<u64>) -> Plan {
    Plan { pid: std::process::id(), ..plan(what, seconds) }
}

fn fake(pmset: &str) -> (Caffeine, FakeKeeper, Arc<FakePower>) {
    let keeper = FakeKeeper::default();
    let power = Arc::new(FakePower::new(pmset));
    let mut caffeine = Caffeine::with(Box::new(keeper.clone()), Arc::clone(&power) as Arc<dyn Power>);
    caffeine.set_who("GoGarden");
    (caffeine, keeper, power)
}

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

#[test]
fn caffeinate_gets_a_flag_for_each_kind_the_time_and_the_ides_process() {
    assert_eq!(caffeinate_args(&plan(&[Assertion::Idle], None)), ["-i", "-w", "4242"]);
    assert_eq!(
        caffeinate_args(&plan(&[Assertion::User, Assertion::Display, Assertion::Idle], Some(900))),
        ["-i", "-d", "-u", "-t", "900", "-w", "4242"],
        "in a fixed order, with the seconds left"
    );
    let all = caffeinate_args(&plan(&Assertion::ALL, None));
    assert_eq!(all, ["-i", "-d", "-m", "-s", "-u", "-w", "4242"]);
    assert_eq!(caffeinate_args(&plan(&[Assertion::Disk], Some(0)))[1..3], ["-t", "1"], "never -t 0, which is forever");
}

#[test]
fn systemd_inhibit_holds_its_lock_while_the_ide_runs() {
    assert_eq!(
        systemd_inhibit_args(&plan(&[Assertion::Idle, Assertion::Display], Some(60))),
        [
            "--what=sleep:idle",
            "--who=GoGarden",
            "--why=Caffeine",
            "--mode=block",
            "timeout",
            "60",
            "tail",
            "--pid=4242",
            "-f",
            "/dev/null"
        ]
    );
    assert_eq!(systemd_inhibit_args(&plan(&[Assertion::Display], None))[0], "--what=idle");
}

#[test]
fn pmset_tells_the_power_source_and_the_charge() {
    assert_eq!(parse_pmset(ON_BATTERY), Some(Battery { percent: Some(82), on_battery: true, charging: false }));
    assert_eq!(parse_pmset(CHARGING), Some(Battery { percent: Some(64), on_battery: false, charging: true }));
    assert_eq!(parse_pmset(DESKTOP), Some(Battery { percent: None, on_battery: false, charging: false }), "no battery");
    let full = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
    assert_eq!(parse_pmset(full), Some(Battery { percent: Some(100), on_battery: false, charging: false }));
    let finishing = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t97%; finishing charge; present: true\n";
    assert!(parse_pmset(finishing).is_some_and(|battery| battery.charging));
    assert_eq!(parse_pmset(""), None);
    assert_eq!(parse_pmset("pmset: command failed"), None);
}

#[test]
fn linux_power_supplies_tell_the_same() {
    let supply = |kind: &str, status: &str, capacity: &str, online: &str| {
        (kind.to_owned(), status.to_owned(), capacity.to_owned(), online.to_owned())
    };
    let laptop = [supply("Mains\n", "", "", "0\n"), supply("Battery\n", "Discharging\n", "41\n", "")];
    assert_eq!(parse_power_supplies(&laptop), Some(Battery { percent: Some(41), on_battery: true, charging: false }));
    let plugged = [supply("Mains", "", "", "1"), supply("Battery", "Charging", "41", "")];
    assert_eq!(parse_power_supplies(&plugged), Some(Battery { percent: Some(41), on_battery: false, charging: true }));
    assert_eq!(parse_power_supplies(&[]), None);
}

#[test]
fn a_time_limit_ends_caffeine_and_its_program() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    let status = caffeine.start(&StartParams { minutes: Some(15), ..StartParams::default() }, now).unwrap();
    assert!(status.on && status.awake);
    assert_eq!(status.seconds_left, Some(900));
    assert_eq!(status.length, Some(Length::Minutes { minutes: 15 }));
    assert_eq!(
        keeper.plans(),
        [started(&[Assertion::Idle], Some(900))],
        "the settings' default: the system from idle sleep"
    );
    caffeine.tick(now + minutes(10));
    assert_eq!(caffeine.status(now + minutes(10)).seconds_left, Some(300));
    assert_eq!(keeper.plans().len(), 1, "the time going by does not start the program again");
    caffeine.tick(now + minutes(15));
    let status = caffeine.status(now + minutes(15));
    assert!(!status.on && !status.awake);
    assert_eq!(status.ended, Some(Ended::Expired));
    assert_eq!(keeper.live(), 0, "the program ended");
}

#[test]
fn a_timed_start_while_already_on_restarts_the_program_with_the_time() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    caffeine.start(&StartParams::default(), now).unwrap();
    assert_eq!(keeper.plans(), [started(&[Assertion::Idle], None)]);
    let later = now + minutes(5);
    let status = caffeine.start(&StartParams { minutes: Some(15), ..StartParams::default() }, later).unwrap();
    assert!(status.on && status.awake);
    assert_eq!(
        keeper.plans(),
        [started(&[Assertion::Idle], None), started(&[Assertion::Idle], Some(900))],
        "the same what, started again with the time"
    );
    assert_eq!(keeper.live(), 1, "the program without a time ended");
    caffeine.tick(later + minutes(10));
    assert_eq!(keeper.plans().len(), 2, "the same deadline keeps the program");
    let status =
        caffeine.start(&StartParams { minutes: Some(30), ..StartParams::default() }, later + minutes(10)).unwrap();
    assert_eq!(status.seconds_left, Some(1800));
    assert_eq!(keeper.plans().last(), Some(&started(&[Assertion::Idle], Some(1800))), "a new time, a new program");
    caffeine.start(&StartParams { minutes: Some(0), ..StartParams::default() }, later + minutes(11)).unwrap();
    assert_eq!(keeper.plans().last(), Some(&started(&[Assertion::Idle], None)), "until turned off: no time left over");
    assert_eq!((keeper.plans().len(), keeper.live()), (4, 1));
}

#[test]
fn the_ide_is_woken_when_the_time_ends() {
    let (caffeine, _, _) = fake(DESKTOP);
    let now = Instant::now();
    assert_eq!(caffeine.next_check(now), None, "off: nothing to follow");
    caffeine.start(&StartParams::default(), now).unwrap();
    assert_eq!(caffeine.next_check(now), Some(TICK));
    caffeine.start(&StartParams { minutes: Some(1), ..StartParams::default() }, now).unwrap();
    assert_eq!(caffeine.next_check(now), Some(TICK));
    let almost = now + Duration::from_secs(58);
    assert_eq!(caffeine.next_check(almost), Some(Duration::from_secs(2)), "woken at the deadline, not a tick later");
    let status = caffeine.status(now + minutes(1));
    assert_eq!(
        (status.on, status.awake, status.ended),
        (false, false, Some(Ended::Expired)),
        "off even without a tick"
    );
    assert_eq!(caffeine.next_check(now + minutes(1)), None);
}

#[test]
fn the_program_ends_at_the_deadline_without_a_tick() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let real = Instant::now();
    let started_at = real - Duration::from_millis(59_700);
    caffeine.start(&StartParams::default(), started_at).unwrap();
    caffeine.start(&StartParams { minutes: Some(1), ..StartParams::default() }, started_at).unwrap();
    assert_eq!(keeper.live(), 1);
    let deadline = started_at + minutes(1);
    while keeper.live() > 0 && Instant::now() < deadline + Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let ended = Instant::now();
    assert_eq!(keeper.live(), 0, "the timer ended the program");
    assert!(ended >= deadline && ended - deadline < Duration::from_secs(30), "{:?} late", ended - deadline);
    assert!(!caffeine.is_on());
    let soon = Instant::now() - Duration::from_millis(59_700);
    caffeine.start(&StartParams { minutes: Some(1), ..StartParams::default() }, soon).unwrap();
    caffeine.start(&StartParams { minutes: Some(0), ..StartParams::default() }, Instant::now()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(caffeine.is_on() && keeper.live() == 1, "the old time's timer leaves the new run alone");
    caffeine.stop(Instant::now());
}

#[test]
fn every_timer_ends_with_its_run_and_one_that_runs_out_wakes_the_ide() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    caffeine.set_waker(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let mut timers = Vec::new();
    for round in 0..300 {
        let now = Instant::now();
        caffeine.start(&StartParams { minutes: Some(1 + round % 3), ..StartParams::default() }, now).unwrap();
        let inner = caffeine.lock();
        let timer = inner.held.as_ref().and_then(|running| running._timer.as_ref()).expect("a timer per timed run");
        timers.push(Arc::clone(&timer.cancelled));
        drop(inner);
        if round % 2 == 0 {
            caffeine.stop(now);
        }
    }
    caffeine.stop(Instant::now());
    let deadline = Instant::now() + Duration::from_secs(60);
    while timers.iter().any(|timer| Arc::strong_count(timer) > 1) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(timers.iter().all(|timer| Arc::strong_count(timer) == 1), "every timer's thread has ended");
    assert_eq!(
        (keeper.live(), woken.load(std::sync::atomic::Ordering::SeqCst)),
        (0, 0),
        "a cancelled timer wakes nothing"
    );
    let soon = Instant::now() - Duration::from_millis(59_800);
    caffeine.start(&StartParams { minutes: Some(1), ..StartParams::default() }, soon).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while woken.load(std::sync::atomic::Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 1, "the IDE is woken once, at the deadline");
    assert!(!caffeine.is_on() && keeper.live() == 0, "and finds Caffeine off");
}

#[test]
fn until_turned_off_has_no_time_and_stop_ends_the_program() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    let what = vec![Assertion::Display, Assertion::Idle, Assertion::Display];
    let status =
        caffeine.start(&StartParams { what: Some(what), minutes: Some(0), ..StartParams::default() }, now).unwrap();
    assert_eq!((status.length, status.seconds_left), (Some(Length::Indefinitely), None));
    assert_eq!(status.what, [Assertion::Idle, Assertion::Display], "each once, in order");
    assert_eq!(keeper.plans()[0].seconds, None);
    caffeine.tick(now + minutes(60 * 24));
    assert!(caffeine.is_awake());
    let status = caffeine.stop(now + minutes(60 * 24));
    assert!(!status.on && !status.awake && status.ended.is_none());
    assert_eq!(keeper.live(), 0);
}

#[test]
fn the_toggle_starts_as_the_settings_say_and_stops() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    let settings = CaffeineSettings {
        what: vec![Assertion::Disk],
        length: Length::Minutes { minutes: 30 },
        ..CaffeineSettings::default()
    };
    caffeine.configure(settings, now);
    let status = caffeine.toggle(now).unwrap();
    assert_eq!((status.on, status.seconds_left), (true, Some(1800)));
    assert_eq!(keeper.plans(), [started(&[Assertion::Disk], Some(1800))]);
    assert!(!caffeine.toggle(now).unwrap().on);
    assert_eq!(keeper.live(), 0);
}

#[test]
fn maximum_caffeine_keeps_everything_awake_until_turned_off_and_a_second_press_ends_it() {
    let (caffeine, keeper, power) = fake(DESKTOP);
    let now = Instant::now();
    let settings = CaffeineSettings {
        what: vec![Assertion::Disk],
        length: Length::Minutes { minutes: 30 },
        off_on_battery: true,
        ..CaffeineSettings::default()
    };
    caffeine.configure(settings.clone(), now);
    let status = caffeine.maximum(now).unwrap();
    assert!(status.on && status.awake && status.maximum, "{status:?}");
    assert_eq!(
        (status.what, status.length, status.seconds_left),
        (Assertion::ALL.to_vec(), Some(Length::Indefinitely), None)
    );
    assert_eq!(keeper.plans(), [started(&Assertion::ALL, None)]);
    assert_eq!(caffeine.settings(), settings, "the settings are left as they were");
    power.set(ON_BATTERY);
    caffeine.tick(now + minutes(2));
    let status = caffeine.status(now + minutes(2));
    assert_eq!((status.on, status.awake, status.paused, status.maximum), (true, false, Some(Pause::OnBattery), true));
    power.set(DESKTOP);
    caffeine.tick(now + minutes(4));
    assert!(caffeine.is_awake());
    assert!(!caffeine.maximum(now + minutes(4)).unwrap().on);
    assert_eq!(keeper.live(), 0);
    caffeine.maximum(now).unwrap();
    assert!(!caffeine.toggle(now).unwrap().on, "Turn Off ends it too");
    caffeine.start(&StartParams::default(), now).unwrap();
    assert!(!caffeine.status(now).maximum);
    assert!(caffeine.maximum(now).unwrap().maximum);
    caffeine.stop(now);
    assert_eq!(keeper.live(), 0);
}

#[test]
fn while_something_runs_it_holds_only_while_a_watched_activity_runs() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    let params = StartParams { during: Some(vec![Activity::Run, Activity::Agent]), ..StartParams::default() };
    let status = caffeine.start(&params, now).unwrap();
    assert_eq!((status.on, status.awake, status.paused), (true, false, Some(Pause::Waiting)));
    assert_eq!(status.during, [Activity::Run, Activity::Agent]);
    caffeine.observe(&[Activity::Indexing], now);
    assert!(!caffeine.is_awake(), "indexing is not watched");
    caffeine.observe(&[Activity::Run], now);
    assert!(caffeine.is_awake());
    caffeine.observe(&[Activity::Agent, Activity::Indexing], now);
    assert_eq!(keeper.plans().len(), 1, "one program while any watched activity goes on");
    caffeine.observe(&[], now + minutes(1));
    let status = caffeine.status(now + minutes(1));
    assert_eq!((status.on, status.awake, status.paused), (true, false, Some(Pause::Waiting)));
    assert_eq!(keeper.live(), 0);
    caffeine.observe(&[Activity::Run], now + minutes(2));
    assert_eq!(keeper.live(), 1, "the next run keeps the computer awake again");
    caffeine.stop(now + minutes(3));
    assert_eq!(keeper.live(), 0);
}

#[test]
fn an_empty_while_list_takes_the_settings_and_none_at_all_is_an_error() {
    let (caffeine, _, _) = fake(DESKTOP);
    let now = Instant::now();
    let status = caffeine.start(&StartParams { during: Some(Vec::new()), ..StartParams::default() }, now).unwrap();
    assert_eq!(status.during, Activity::ALL);
    caffeine.configure(CaffeineSettings { during: Vec::new(), ..CaffeineSettings::default() }, now);
    caffeine.stop(now);
    assert!(caffeine.start(&StartParams { during: Some(Vec::new()), ..StartParams::default() }, now).is_err());
}

#[test]
fn on_battery_it_pauses_and_goes_on_again_on_mains_power() {
    let (caffeine, keeper, power) = fake(ON_BATTERY);
    let now = Instant::now();
    caffeine.configure(CaffeineSettings { off_on_battery: true, ..CaffeineSettings::default() }, now);
    let status = caffeine.start(&StartParams::default(), now).unwrap();
    assert_eq!((status.on, status.awake, status.paused), (true, false, Some(Pause::OnBattery)));
    assert_eq!(status.battery, Some(Battery { percent: Some(82), on_battery: true, charging: false }));
    assert!(keeper.plans().is_empty(), "nothing started on battery");
    power.set(CHARGING);
    caffeine.tick(now + Duration::from_secs(10));
    assert!(!caffeine.is_awake(), "the battery is read once a minute, not every frame");
    caffeine.tick(now + BATTERY_EVERY);
    let status = caffeine.status(now + BATTERY_EVERY);
    assert_eq!((status.awake, status.paused), (true, None), "back on mains power");
    power.set(ON_BATTERY);
    caffeine.tick(now + BATTERY_EVERY * 2);
    assert_eq!(caffeine.status(now + BATTERY_EVERY * 2).paused, Some(Pause::OnBattery));
    assert_eq!(keeper.live(), 0);
}

#[test]
fn below_the_level_it_pauses_and_without_resuming_it_turns_off() {
    let (caffeine, keeper, power) = fake(LOW_BATTERY);
    let now = Instant::now();
    let settings = CaffeineSettings { low_battery: true, low_battery_level: 20, ..CaffeineSettings::default() };
    caffeine.configure(settings.clone(), now);
    assert_eq!(caffeine.start(&StartParams::default(), now).unwrap().paused, Some(Pause::LowBattery));
    power.set(ON_BATTERY);
    caffeine.tick(now + BATTERY_EVERY);
    assert!(caffeine.is_awake(), "82% is above the level, even on battery");
    caffeine.stop(now + BATTERY_EVERY);
    let (caffeine, keeper2, _) = fake(LOW_BATTERY);
    caffeine.configure(CaffeineSettings { resume_on_charge: false, ..settings }, now);
    let status = caffeine.start(&StartParams::default(), now).unwrap();
    assert_eq!((status.on, status.ended), (false, Some(Ended::Battery)));
    assert_eq!(keeper.live() + keeper2.live(), 0);
}

#[test]
fn the_battery_is_read_only_while_a_rule_needs_it() {
    let (caffeine, _, power) = fake(ON_BATTERY);
    let now = Instant::now();
    caffeine.start(&StartParams::default(), now).unwrap();
    caffeine.tick(now + BATTERY_EVERY * 3);
    assert_eq!(power.reads.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(caffeine.is_awake(), "no rule: on battery too");
    assert_eq!(caffeine.status(now).battery, None);
}

#[test]
fn a_change_of_what_to_keep_awake_applies_to_the_running_program() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    caffeine.start(&StartParams { minutes: Some(60), ..StartParams::default() }, now).unwrap();
    let what = vec![Assertion::Idle, Assertion::Display];
    caffeine.configure(CaffeineSettings { what, ..CaffeineSettings::default() }, now + minutes(20));
    assert_eq!(keeper.plans().last(), Some(&started(&[Assertion::Idle, Assertion::Display], Some(2400))));
    assert_eq!(keeper.live(), 1, "the old program ended");
    caffeine.configure(CaffeineSettings { custom_minutes: 45, ..caffeine.settings() }, now + minutes(21));
    assert_eq!(keeper.plans().len(), 2, "other settings leave the program alone");
}

#[test]
fn a_program_that_ended_by_itself_starts_again_after_a_moment() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let now = Instant::now();
    caffeine.start(&StartParams::default(), now).unwrap();
    keeper.end_last();
    caffeine.tick(now + Duration::from_secs(1));
    assert!(!caffeine.is_awake());
    caffeine.tick(now + Duration::from_secs(2));
    assert_eq!(keeper.plans().len(), 1, "not started again at once");
    caffeine.tick(now + Duration::from_secs(12));
    assert_eq!((keeper.plans().len(), keeper.live()), (2, 1));
}

#[test]
fn without_a_program_caffeine_is_not_available() {
    let keeper = FakeKeeper { missing: true, ..FakeKeeper::default() };
    let caffeine = Caffeine::with(Box::new(keeper), Arc::new(FakePower::new(DESKTOP)));
    assert!(!caffeine.available());
    let error = caffeine.start(&StartParams::default(), Instant::now()).unwrap_err();
    assert_eq!(error.message, zen_i18n::t!("workbench-caffeine-unavailable"));
    assert!(!caffeine.status(Instant::now()).available);
    assert!(
        caffeine
            .start(&StartParams { minutes: Some(MAX_MINUTES + 1), ..StartParams::default() }, Instant::now())
            .is_err()
    );
}

#[test]
fn too_long_a_time_is_refused() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let params = StartParams { minutes: Some(MAX_MINUTES + 1), ..StartParams::default() };
    assert!(caffeine.start(&params, Instant::now()).is_err());
    assert!(keeper.plans().is_empty());
}

#[test]
fn shutdown_ends_the_program_and_clones_share_one_caffeine() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let other = caffeine.clone();
    other.start(&StartParams::default(), Instant::now()).unwrap();
    assert!(caffeine.is_on());
    caffeine.shutdown();
    assert!(!other.is_on());
    assert_eq!(keeper.live(), 0);
    let (caffeine, keeper, _) = fake(DESKTOP);
    caffeine.start(&StartParams::default(), Instant::now()).unwrap();
    drop(caffeine);
    assert_eq!(keeper.live(), 0, "dropped with the IDE, the program ends");
}

#[test]
fn agents_drive_caffeine_through_commands() {
    let (caffeine, keeper, _) = fake(DESKTOP);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    assert!(registry.dispatch(&mut session, "caffeine.status.get", json!({})).is_err(), "no Caffeine in the session");
    session.set_state(caffeine);
    let status = registry
        .dispatch(&mut session, "caffeine.awake.start", json!({ "what": ["display", "idle"], "minutes": 30 }))
        .unwrap();
    assert_eq!(status["on"], true);
    assert_eq!(status["what"], json!(["idle", "display"]));
    assert_eq!(status["length"], json!({ "kind": "minutes", "minutes": 30 }));
    assert!(status["secondsLeft"].as_u64().is_some_and(|left| left > 1790 && left <= 1800));
    assert_eq!(status["backend"], "fake");
    let status = registry.dispatch(&mut session, "caffeine.awake.start", json!({ "while": ["run", "debug"] })).unwrap();
    assert_eq!((status["while"].clone(), status["paused"].clone()), (json!(["run", "debug"]), json!("waiting")));
    assert_eq!(registry.dispatch(&mut session, "caffeine.status.get", json!({})).unwrap()["awake"], false);
    assert!(registry.dispatch(&mut session, "caffeine.awake.start", json!({ "what": ["coffee"] })).is_err());
    let status = registry.dispatch(&mut session, "caffeine.awake.stop", json!({})).unwrap();
    assert_eq!(status["on"], false);
    assert_eq!(registry.dispatch(&mut session, "caffeine.awake.toggle", json!({})).unwrap()["on"], true);
    assert_eq!(registry.dispatch(&mut session, "caffeine.awake.toggle", json!({})).unwrap()["on"], false);
    let status = registry.dispatch(&mut session, "caffeine.awake.maximum", json!({})).unwrap();
    assert_eq!((status["on"].clone(), status["maximum"].clone()), (json!(true), json!(true)));
    assert_eq!(status["what"], json!(["idle", "display", "disk", "system", "user"]));
    assert_eq!(registry.dispatch(&mut session, "caffeine.awake.maximum", json!({})).unwrap()["on"], false);
    let status =
        registry.dispatch(&mut session, "caffeine.awake.start", json!({ "maximum": true, "minutes": 5 })).unwrap();
    assert_eq!((status["maximum"].clone(), status.get("secondsLeft")), (json!(true), None), "maximum wins");
    registry.dispatch(&mut session, "caffeine.awake.stop", json!({})).unwrap();
    assert_eq!(keeper.live(), 0);
}

#[test]
fn a_window_tells_what_runs_in_it() {
    let mut session = Session::new();
    assert!(session_activity(&session).is_empty());
    let tasks = session.state_mut::<crate::tasks::Tasks>();
    let index = tasks.start("Indexing the project", crate::exit::INDEX, false);
    let push = tasks.start("Pushing main", "vcs", false);
    assert_eq!(session_activity(&session), [Activity::Task, Activity::Indexing]);
    index.finish(Ok(String::new()));
    push.finish(Ok(String::new()));
    assert!(session_activity(&session).is_empty());
}

#[test]
fn the_settings_round_trip_with_their_defaults() {
    let settings: CaffeineSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, CaffeineSettings::default());
    assert_eq!(settings.what, [Assertion::Idle]);
    assert!(settings.resume_on_charge && !settings.start_on_launch && !settings.battery_rules());
    let json = serde_json::to_value(CaffeineSettings { length: Length::While, ..settings }).unwrap();
    assert_eq!(json["length"], json!({ "kind": "while" }));
    assert_eq!(json["while"], json!(["run", "debug", "task", "indexing", "agent"]));
    assert_eq!(json["lowBatteryLevel"], 20);
}

#[cfg(target_os = "macos")]
#[test]
fn caffeinate_ends_with_the_process_it_waits_for() {
    use super::keeper::{Keeper, SystemKeeper};
    let path = std::path::PathBuf::from("/usr/bin/caffeinate");
    if !path.is_file() {
        return;
    }
    let mut keeper = SystemKeeper::caffeinate(path);
    let mut ide = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let plan = Plan { what: vec![Assertion::Idle], seconds: Some(20), pid: ide.id(), who: "test".to_owned() };
    let mut held = keeper.start(&plan).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(held.alive(), "caffeinate runs while the IDE does");
    ide.kill().unwrap();
    ide.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while held.alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!held.alive(), "caffeinate ended with the IDE");
    held.stop();
    let mut ide = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let mut held = keeper.start(&Plan { pid: ide.id(), ..plan }).unwrap();
    let pid = held.pid().unwrap();
    held.stop();
    assert!(!held.alive());
    let ps = std::process::Command::new("/bin/ps").args(["-p", &pid.to_string()]).output().unwrap();
    assert!(!String::from_utf8_lossy(&ps.stdout).contains("caffeinate"), "no caffeinate left");
    let _ = ide.kill();
    let _ = ide.wait();
}
