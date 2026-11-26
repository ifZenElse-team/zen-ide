use std::time::{Duration, SystemTime};

use super::*;

const WAIT: Duration = Duration::from_secs(10);

fn ran(connection: &str, text: &str, source: Source) -> Ran {
    Ran {
        connection: connection.to_owned(),
        context: ContextKind::Console("console.sql".to_owned()),
        schema: Some("public".to_owned()),
        text: text.to_owned(),
        started: SystemTime::now(),
        duration: Duration::from_millis(12),
        rows: Some(3),
        affected: None,
        error: None,
        provenance: Provenance { source, agent: false, place: Some("console.sql".to_owned()), confirmed: false },
        params: Vec::new(),
    }
}

fn all(journal: &Journal) -> Found {
    journal.search(&Query { limit: 1000, ..Query::default() })
}

fn texts(found: &Found) -> Vec<&str> {
    found.entries.iter().map(|entry| entry.text.as_str()).collect()
}

#[test]
fn statements_are_appended_as_json_lines_and_read_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    journal.record(ran("pg", "SELECT 1", Source::Statement));
    let mut failed = ran("pg", "SELEC 2", Source::Script);
    failed.error = Some(DbError::new(ErrorKind::Server, "syntax error at or near \"SELEC\"").with_code("42601"));
    failed.rows = None;
    journal.record(failed);
    let mut cancelled = ran("kv", "GET a", Source::Query);
    cancelled.error = Some(DbError::new(ErrorKind::Cancelled, ""));
    cancelled.provenance.agent = true;
    journal.record(cancelled);
    assert!(journal.flush(WAIT));
    let text = std::fs::read_to_string(dir.path().join(JOURNAL)).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.ends_with('\n'));
    let found = all(&journal);
    assert_eq!(texts(&found), ["GET a", "SELEC 2", "SELECT 1"], "newest first");
    assert_eq!(found.entries[0].outcome, Outcome::Cancelled);
    assert!(found.entries[0].agent);
    assert_eq!(found.entries[1].outcome, Outcome::Failed);
    assert_eq!(found.entries[1].error.as_ref().unwrap().code.as_deref(), Some("42601"));
    assert_eq!(found.entries[2].outcome, Outcome::Done);
    assert_eq!((found.entries[2].rows, found.entries[2].duration_ms), (Some(3), 12));
    assert!(found.entries[0].id > found.entries[1].id && found.entries[1].id > found.entries[2].id);
    drop(journal);

    let reopened = Journal::open(dir.path());
    assert!(reopened.flush(WAIT));
    let again = all(&reopened);
    assert!(again.loaded);
    assert_eq!(again.entries, found.entries);
    reopened.record(ran("pg", "SELECT 4", Source::Statement));
    assert!(reopened.flush(WAIT));
    let newest = all(&reopened);
    assert_eq!(newest.entries[0].text, "SELECT 4");
    assert!(newest.entries[0].id > found.entries[0].id);
}

#[test]
fn a_statement_is_found_as_soon_as_it_is_recorded_even_before_the_journal_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    journal.record(ran("pg", "SELECT 1", Source::Statement));
    assert_eq!(texts(&all(&journal)), ["SELECT 1"], "found without waiting for the writer");
    assert!(journal.flush(WAIT));
    drop(journal);

    let reopened = Journal::open(dir.path());
    reopened.record(ran("pg", "SELECT 2", Source::Statement));
    assert!(texts(&all(&reopened)).contains(&"SELECT 2"), "found while the journal is still being read");
    assert!(reopened.flush(WAIT));
    assert_eq!(texts(&all(&reopened)), ["SELECT 2", "SELECT 1"], "and kept with what was read");
    drop(reopened);
    let again = Journal::open(dir.path());
    assert!(again.flush(WAIT));
    assert_eq!(texts(&all(&again)), ["SELECT 2", "SELECT 1"], "once each after a restart");
}

#[test]
fn a_torn_last_line_after_a_crash_is_dropped_and_the_rest_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    journal.record(ran("pg", "SELECT 1", Source::Statement));
    journal.record(ran("pg", "SELECT 2", Source::Statement));
    assert!(journal.flush(WAIT));
    drop(journal);
    let file = dir.path().join(JOURNAL);
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{\"id\":99,\"at\":1,\"conn");
    std::fs::write(&file, &text).unwrap();
    let reopened = Journal::open(dir.path());
    assert!(reopened.flush(WAIT));
    assert_eq!(texts(&all(&reopened)), ["SELECT 2", "SELECT 1"]);
    let rewritten = std::fs::read_to_string(&file).unwrap();
    assert_eq!(rewritten.lines().count(), 2, "the damaged line is dropped from the file: {rewritten}");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [JOURNAL], "no temporary file is left");
}

#[test]
fn every_line_has_a_crc_and_a_line_changed_on_disk_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    for text in ["SELECT 1", "SELECT 2", "SELECT 3"] {
        journal.record(ran("pg", text, Source::Statement));
    }
    assert!(journal.flush(WAIT));
    drop(journal);
    let file = dir.path().join(JOURNAL);
    let text = std::fs::read_to_string(&file).unwrap();
    for line in text.lines() {
        let (crc, json) = line.split_at(8);
        assert_eq!(u32::from_str_radix(crc, 16).unwrap(), crc32fast::hash(&json.as_bytes()[1..]), "{line}");
    }
    std::fs::write(&file, text.replacen("SELECT 2", "SELECT 9", 1)).unwrap();
    let reopened = Journal::open(dir.path());
    assert!(reopened.flush(WAIT));
    assert_eq!(texts(&all(&reopened)), ["SELECT 3", "SELECT 1"], "a record whose check fails is never read");
    reopened.clear(None, None);
    assert!(reopened.flush(WAIT));
    assert!(!file.exists(), "a history cleared of everything leaves no file");
}

#[test]
fn old_statements_and_those_past_the_limit_are_pruned() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    assert!(journal.flush(WAIT));
    journal.set_settings(Settings { enabled: true, retention_days: 7, max_entries: 100, parameter_values: false });
    let mut old = ran("pg", "SELECT 'old'", Source::Statement);
    old.started = SystemTime::now() - Duration::from_secs(8 * 86_400);
    journal.record(old);
    for index in 0..150 {
        journal.record(ran("pg", &format!("SELECT {index}"), Source::Statement));
    }
    assert!(journal.flush(WAIT));
    let found = all(&journal);
    assert_eq!(found.kept, 100);
    assert_eq!(found.entries.first().unwrap().text, "SELECT 149");
    assert_eq!(found.entries.last().unwrap().text, "SELECT 50");
    let lines = std::fs::read_to_string(dir.path().join(JOURNAL)).unwrap().lines().count();
    assert!(lines <= 100 + 100, "dead lines are compacted away: {lines}");
    drop(journal);
    let settings: Settings = serde_json::from_slice(&std::fs::read(dir.path().join(SETTINGS)).unwrap()).unwrap();
    assert_eq!(settings.max_entries, 100);
    let reopened = Journal::open(dir.path());
    assert!(reopened.flush(WAIT));
    assert_eq!(reopened.settings().retention_days, 7);
    assert_eq!(all(&reopened).kept, 100);
    let bounded = Settings { enabled: true, retention_days: 0, max_entries: 1, parameter_values: false }.bounded();
    assert_eq!((bounded.retention_days, bounded.max_entries), (1, 100));
}

#[test]
fn turned_off_it_records_nothing_and_clearing_removes_what_was_asked() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    journal.record(ran("pg", "SELECT 1", Source::Statement));
    journal.record(ran("kv", "GET a", Source::Statement));
    assert!(journal.flush(WAIT));
    journal.set_settings(Settings { enabled: false, ..Settings::default() });
    journal.record(ran("pg", "SELECT 2", Source::Statement));
    assert!(journal.flush(WAIT));
    assert_eq!(texts(&all(&journal)), ["GET a", "SELECT 1"]);
    journal.set_settings(Settings::default());
    journal.clear(Some("pg".to_owned()), None);
    assert!(journal.flush(WAIT));
    assert_eq!(texts(&all(&journal)), ["GET a"]);
    let future = now() + 1000;
    journal.clear(None, Some(future));
    assert!(journal.flush(WAIT));
    assert_eq!(all(&journal).kept, 0);
    assert!(!dir.path().join(JOURNAL).exists(), "a history with nothing left leaves no journal");
}

#[test]
fn search_matches_every_word_in_any_case_and_filters_by_connection_source_outcome_and_time() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    let mut early = ran("pg", "SELECT * FROM orders WHERE total > 10", Source::Statement);
    early.started = SystemTime::now() - Duration::from_secs(2 * 86_400);
    let boundary = now() - DAY_MS;
    journal.record(early);
    journal.record(ran("pg", "select id from Customers", Source::Script));
    let mut failed = ran("my", "DELETE FROM orders", Source::Save);
    failed.error = Some(DbError::new(ErrorKind::Server, "permission denied for table orders"));
    journal.record(failed);
    let mut agent = ran("pg", "SELECT count(*) FROM orders", Source::Query);
    agent.provenance.agent = true;
    journal.record(agent);
    journal.record(ran("pg", "COMMIT", Source::Transaction));
    assert!(journal.flush(WAIT));
    let search = |query: Query| texts(&journal.search(&Query { limit: 10, ..query })).join(" | ");
    assert_eq!(
        search(Query { text: Some("ORDERS from".to_owned()), ..Query::default() }),
        "SELECT count(*) FROM orders | DELETE FROM orders | SELECT * FROM orders WHERE total > 10"
    );
    assert_eq!(search(Query { text: Some("permission".to_owned()), ..Query::default() }), "DELETE FROM orders");
    assert_eq!(search(Query { connections: vec!["my".to_owned()], ..Query::default() }), "DELETE FROM orders");
    assert_eq!(
        search(Query { sources: vec![Source::Script, Source::Transaction], ..Query::default() }),
        "COMMIT | select id from Customers"
    );
    assert_eq!(search(Query { agent: Some(true), ..Query::default() }), "SELECT count(*) FROM orders");
    assert_eq!(search(Query { outcome: Some(Outcome::Failed), ..Query::default() }), "DELETE FROM orders");
    assert_eq!(search(Query { until: Some(boundary), ..Query::default() }), "SELECT * FROM orders WHERE total > 10");
    assert_eq!(
        search(Query { since: Some(boundary), text: Some("customers".to_owned()), ..Query::default() }),
        "select id from Customers"
    );
    let page = journal.search(&Query { offset: 1, limit: 2, ..Query::default() });
    assert_eq!(texts(&page), ["SELECT count(*) FROM orders", "DELETE FROM orders"]);
    assert_eq!((page.total, page.truncated), (5, true));
    let id = page.entries[0].id;
    assert_eq!(journal.get(id).unwrap().text, "SELECT count(*) FROM orders");
}

#[test]
fn searching_ten_thousand_statements_is_quick() {
    if std::env::var_os("ZEN_TIMING_CHECKS").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    journal.set_settings(Settings { max_entries: 10_000, ..Settings::default() });
    for index in 0..10_000 {
        journal.record(ran(
            "pg",
            &format!("SELECT {index}, name, email FROM customers WHERE id = {index}"),
            Source::Statement,
        ));
    }
    assert!(journal.flush(Duration::from_secs(60)));
    let started = Instant::now();
    let found = journal.search(&Query { text: Some("email 9999".to_owned()), limit: 50, ..Query::default() });
    assert_eq!(found.total, 1);
    assert!(started.elapsed() < Duration::from_millis(30), "{:?}", started.elapsed());
}

#[test]
fn long_statements_are_cut_and_marked() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    let long = format!("SELECT '{}'", "é".repeat(MOST_TEXT));
    journal.record(ran("pg", &long, Source::Statement));
    assert!(journal.flush(WAIT));
    let found = all(&journal);
    assert!(found.entries[0].cut);
    assert_eq!(found.entries[0].text.chars().count(), MOST_TEXT);
}

#[test]
fn passwords_written_in_statements_are_masked() {
    let cases = [
        ("CREATE ROLE ann LOGIN PASSWORD 's3cret'", "CREATE ROLE ann LOGIN PASSWORD '***'"),
        ("ALTER USER ann WITH ENCRYPTED PASSWORD 'it''s'", "ALTER USER ann WITH ENCRYPTED PASSWORD '***'"),
        ("ALTER ROLE ann PASSWORD $$x$$", "ALTER ROLE ann PASSWORD $$***$$"),
        ("ALTER ROLE ann PASSWORD NULL", "ALTER ROLE ann PASSWORD NULL"),
        ("CREATE USER 'u'@'%' IDENTIFIED BY 'pw1'", "CREATE USER 'u'@'%' IDENTIFIED BY '***'"),
        (
            "ALTER USER 'u'@'%' IDENTIFIED WITH caching_sha2_password BY 'pw2'",
            "ALTER USER 'u'@'%' IDENTIFIED WITH caching_sha2_password BY '***'",
        ),
        ("CREATE USER scott IDENTIFIED BY tiger", "CREATE USER scott IDENTIFIED BY ***"),
        ("SET PASSWORD FOR 'u'@'h' = 'pw3'", "SET PASSWORD FOR 'u'@'h' = '***'"),
        ("SET PASSWORD = PASSWORD('pw4')", "SET PASSWORD = PASSWORD('***')"),
        (
            "SELECT * FROM dblink('host=db user=ann password=pw5 dbname=x', 'SELECT 1')",
            "SELECT * FROM dblink('host=db user=ann password=*** dbname=x', 'SELECT 1')",
        ),
        ("SELECT dblink_connect('postgresql://ann:pw6@db/x')", "SELECT dblink_connect('postgresql://ann:***@db/x')"),
        ("UPDATE users SET password = 'pw7' WHERE id = 1", "UPDATE users SET password = '***' WHERE id = 1"),
        ("AUTH default pw8", "AUTH *** ***"),
        ("HELLO 3 AUTH ann pw9 SETNAME zen", "HELLO 3 AUTH *** *** SETNAME zen"),
        ("CONFIG SET requirepass pw10", "CONFIG SET requirepass ***"),
        ("ACL SETUSER ann on >pw11 ~* +@all", "ACL SETUSER ann on >*** ~* +@all"),
    ];
    for (text, masked) in cases {
        let (shown, redacted) = redact(text);
        assert_eq!(shown, masked, "{text}");
        assert_eq!(redacted, text != masked, "{text}");
    }
    for plain in [
        "SELECT password, name FROM users",
        "SELECT password_hash FROM users WHERE id = $1",
        "CREATE TABLE t (password text NOT NULL)",
        "GET auth",
    ] {
        assert_eq!(redact(plain), (plain.to_owned(), false), "{plain}");
    }
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    let mut failing = ran("pg", "CREATE ROLE x PASSWORD 'hunter2'", Source::Statement);
    failing.error = Some(DbError::new(ErrorKind::Server, "role \"x\" PASSWORD 'hunter2' exists"));
    journal.record(failing);
    assert!(journal.flush(WAIT));
    let text = std::fs::read_to_string(dir.path().join(JOURNAL)).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(all(&journal).entries[0].redacted);
}

#[test]
fn times_are_shown_in_the_local_zone_and_days_start_at_local_midnight() {
    let at = now();
    let midnight = local_midnight(at);
    assert!(midnight <= at && at - midnight < DAY_MS + 3_600_000, "{midnight} {at}");
    assert_eq!(local_time(midnight).get(11..), Some("00:00:00"));
    assert_eq!(local_time(at).len(), "2026-10-27 14:05:09".len());
}

#[test]
fn refused_and_confirmed_statements_are_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    let mut refused = ran("pg", "DELETE FROM orders", Source::Query);
    refused.error = Some(DbError::new(ErrorKind::Refused, "DELETE").with_code("agentReadOnly"));
    refused.provenance.agent = true;
    journal.record(refused);
    let mut confirmed = ran("pg", "DELETE FROM orders WHERE id = 1", Source::Statement);
    confirmed.provenance.confirmed = true;
    journal.record(confirmed);
    assert!(journal.flush(WAIT));
    let found = all(&journal);
    assert_eq!(found.entries[0].outcome, Outcome::Done);
    assert!(found.entries[0].confirmed && !found.entries[0].agent);
    assert_eq!(found.entries[1].outcome, Outcome::Refused);
    assert!(found.entries[1].agent && !found.entries[1].confirmed);
    let only_refused = journal.search(&Query { outcome: Some(Outcome::Refused), limit: 10, ..Query::default() });
    assert_eq!(texts(&only_refused), ["DELETE FROM orders"]);
}

#[test]
fn bind_parameters_are_kept_by_name_and_their_values_only_when_asked_with_secrets_masked() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path());
    let params = vec![
        crate::params::NamedValue { name: "id".to_owned(), value: crate::value::Value::Integer(7) },
        crate::params::NamedValue {
            name: "api_token".to_owned(),
            value: crate::value::Value::Text("abc123".to_owned()),
        },
    ];
    let mut bound = ran("pg", "SELECT * FROM t WHERE id = $1 AND token = $2", Source::Statement);
    bound.params.clone_from(&params);
    journal.record(bound.clone());
    journal.set_settings(Settings { parameter_values: true, ..Settings::default() });
    journal.record(bound);
    assert!(journal.flush(WAIT));
    let found = all(&journal);
    assert_eq!(found.entries[1].parameters, [":id", ":api_token"]);
    assert_eq!(found.entries[0].parameters, [":id = 7", ":api_token = ***"]);
    let file = std::fs::read_to_string(dir.path().join(JOURNAL)).unwrap();
    assert!(!file.contains("abc123"), "{file}");
}

#[test]
fn settings_chosen_before_the_journal_is_read_outlive_the_settings_in_its_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(SETTINGS), serde_json::to_vec(&Settings::default()).unwrap()).unwrap();
    let (journal, mut writer, _jobs) = Journal::unstarted(dir.path());
    let chosen = Settings { enabled: false, parameter_values: true, ..Settings::default() };
    journal.set_settings(chosen.clone());
    writer.load();
    assert_eq!(journal.settings(), chosen);
}

#[test]
fn the_settings_in_the_file_apply_when_none_were_chosen_before_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let saved = Settings { parameter_values: true, ..Settings::default() };
    std::fs::write(dir.path().join(SETTINGS), serde_json::to_vec(&saved).unwrap()).unwrap();
    let (journal, mut writer, _jobs) = Journal::unstarted(dir.path());
    writer.load();
    assert_eq!(journal.settings(), saved);
}
