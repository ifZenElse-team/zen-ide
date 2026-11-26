use std::collections::HashMap;
use std::time::Duration;

use super::*;

fn process(pid: u32, parent: u32, megabytes: u64, seconds: u64, name: &str) -> ProcessInfo {
    ProcessInfo { pid, parent, resident: megabytes << 20, cpu: Duration::from_secs(seconds), name: name.to_owned() }
}

#[test]
fn ps_lines_are_read_with_their_times_and_names() {
    let output = "    1     0  21856  32:25.86 /sbin/launchd\n\
                  67495 76786   3136   0:00.01 /bin/zsh\n\
                  500 1 1024 1:02:03.50 /Applications/Go Garden.app/Contents/MacOS/gogarden\n\
                  501 1 10 2-00:00:01 /usr/bin/thing\n\
                  garbage\n";
    let found = parse_ps(output);
    assert_eq!(found.len(), 4);
    assert_eq!(
        found[0],
        ProcessInfo {
            pid: 1,
            parent: 0,
            resident: 21856 * 1024,
            cpu: Duration::from_secs_f64(32.0 * 60.0 + 25.86),
            name: "launchd".to_owned()
        }
    );
    assert_eq!(found[1].name, "zsh");
    assert_eq!(found[2].name, "gogarden", "a folder with a space");
    assert_eq!(found[2].cpu, Duration::from_secs_f64(3723.5));
    assert_eq!(found[3].cpu, Duration::from_secs(2 * 86_400 + 1));
    assert_eq!(parse_cpu_time("x"), None);
}

#[test]
fn rows_sum_each_part_and_its_children_over_the_interval() {
    let before_times: HashMap<u32, Duration> =
        [(100, 10), (200, 4), (201, 1), (400, 2)].map(|(pid, s)| (pid, Duration::from_secs(s))).into();
    let all = [
        process(100, 1, 500, 11, "gogarden"),
        process(200, 100, 300, 5, "gopls"),
        process(201, 200, 50, 2, "go"),
        process(300, 100, 20, 1, "git"),
        process(400, 100, 10, 2, "zsh"),
        process(999, 1, 10, 50, "other"),
    ];
    let servers = vec![("gopls".to_owned(), 200)];
    let index = (Duration::from_millis(1500), 4 << 20);
    let first = measure(&all, 100, &servers, index, None);
    assert!(first.measured);
    assert!(first.rows.iter().all(|row| row.cpu_percent.is_none()), "no use before a second measurement");
    let report =
        measure(&all, 100, &servers, index, Some((Duration::from_secs(2), &before_times, Duration::from_millis(500))));
    let row = |subsystem: Subsystem| report.rows.iter().find(|row| row.subsystem == subsystem).unwrap();
    assert_eq!(report.interval_ms, 2000);
    assert_eq!(row(Subsystem::Ide).name, "gogarden");
    assert_eq!(row(Subsystem::Ide).cpu_percent, Some(50.0));
    assert_eq!(row(Subsystem::Ide).memory_bytes, 500 << 20);
    assert_eq!(row(Subsystem::Index).cpu_percent, Some(50.0));
    assert_eq!(row(Subsystem::Index).memory_bytes, 4 << 20);
    let server = row(Subsystem::LanguageServer);
    assert_eq!((server.name.as_str(), server.pids.as_slice()), ("gopls", &[200, 201][..]), "with what it started");
    assert_eq!(server.cpu_percent, Some(100.0));
    assert_eq!(server.memory_bytes, 350 << 20);
    assert_eq!(row(Subsystem::Vcs).pids, [300]);
    assert_eq!(row(Subsystem::Vcs).cpu_percent, Some(50.0), "a process started meanwhile counts from its start");
    let others: Vec<&str> =
        report.rows.iter().filter(|row| row.subsystem == Subsystem::Process).map(|row| row.name.as_str()).collect();
    assert_eq!(others, ["zsh"], "only what the IDE started");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn this_process_is_measured() {
    let pid = std::process::id();
    let resident = resident_bytes(pid).expect("the system says");
    assert!(resident > 1 << 20, "{resident}");
    let me = processes().into_iter().find(|process| process.pid == pid).expect("listed");
    assert!(me.resident > 0);
}
