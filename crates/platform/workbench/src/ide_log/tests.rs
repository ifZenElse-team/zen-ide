use super::*;

fn read(folder: &Path, name: &str) -> String {
    std::fs::read_to_string(folder.join(name)).unwrap()
}

#[test]
fn the_log_rotates_into_numbered_files_and_keeps_only_the_last_ones() {
    let dir = tempfile::tempdir().unwrap();
    let log = FileLog::open(dir.path().to_owned(), &[]);
    let chunk = "x".repeat(LIMIT as usize - 1) + "\n";

    for round in 0..ROTATED + 3 {
        log.write_line(&format!("{round}{}", &chunk[1..]));
    }

    assert!(dir.path().join(FILE_NAME).is_file());
    assert!(read(dir.path(), "zen.1.log").starts_with(&(ROTATED + 1).to_string()));
    assert!(dir.path().join(rotated_name(ROTATED)).is_file());
    assert!(!dir.path().join(rotated_name(ROTATED + 1)).exists());
}

#[test]
fn a_log_goes_on_in_the_file_of_the_last_run() {
    let dir = tempfile::tempdir().unwrap();
    FileLog::open(dir.path().to_owned(), &[]).write_line("first\n");
    FileLog::open(dir.path().to_owned(), &[]).write_line("second\n");
    assert_eq!(read(dir.path(), FILE_NAME), "first\nsecond\n");
}

#[test]
fn a_category_turns_on_debug_and_trace_for_its_modules_alone() {
    let lines = ["zen_lsp".to_owned(), "zen_lsp::client:trace".to_owned(), String::new(), "wgpu_core".to_owned()];
    let categories = parse_categories(&lines);

    assert_eq!(level_of(&categories, "zen_lsp"), LevelFilter::Debug);
    assert_eq!(level_of(&categories, "zen_lsp::server"), LevelFilter::Debug);
    assert_eq!(level_of(&categories, "zen_lsp::client"), LevelFilter::Trace);
    assert_eq!(level_of(&categories, "zen_lspx"), LevelFilter::Info, "a whole module name, not a prefix");
    assert_eq!(level_of(&categories, "wgpu_core"), LevelFilter::Debug);
    assert_eq!(level_of(&categories, "winit"), LevelFilter::Warn, "what the IDE did not write stays quiet");
    assert_eq!(max_level(&categories), LevelFilter::Trace);
    assert_eq!(max_level(&[]), LevelFilter::Info);
}

#[test]
fn records_below_the_level_of_their_category_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let log = FileLog::open(dir.path().to_owned(), &["zen_vcs".to_owned()]);
    let record = |level, target, text: &'static str| {
        log.log(&Record::builder().level(level).target(target).args(format_args!("{text}")).build());
    };

    record(Level::Debug, "zen_vcs", "wanted");
    record(Level::Debug, "zen_lsp", "too detailed");
    record(Level::Info, "egui", "not ours");
    record(Level::Warn, "egui", "a warning");

    let text = read(dir.path(), FILE_NAME);
    assert!(text.contains("zen_vcs wanted") && text.contains("egui a warning"), "{text}");
    assert!(!text.contains("too detailed") && !text.contains("not ours"), "{text}");
}

const PANIC_CHILD: &str = "ZEN_WORKBENCH_TEST_PANIC_LOG";

#[test]
fn a_panic_is_written_with_its_message_place_and_backtrace() {
    if let Some(folder) = std::env::var_os(PANIC_CHILD) {
        let log = FileLog::open(folder.into(), &[]);
        std::panic::set_hook(Box::new(move |info| log.write_panic(info)));
        let doomed = std::thread::Builder::new().name("doomed".to_owned());
        let _ = doomed.spawn(|| panic!("the wheels came off")).unwrap().join();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ide_log::tests::a_panic_is_written_with_its_message_place_and_backtrace"])
        .env(PANIC_CHILD, dir.path())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() && said.contains("1 passed"), "{said}{}", String::from_utf8_lossy(&output.stderr));
    let text = read(dir.path(), FILE_NAME);
    assert!(text.contains("thread 'doomed'") && text.contains("the wheels came off"), "{text}");
    assert!(text.contains("tests.rs"), "where it happened: {text}");
    assert!(text.lines().count() > 1, "the backtrace follows: {text}");
}

#[test]
fn categories_are_kept_in_the_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_categories(dir.path()).is_empty());
    write_categories(dir.path(), &["zen_lsp:trace".to_owned()]).unwrap();
    assert_eq!(read_categories(dir.path()), ["zen_lsp:trace"]);
}

#[test]
fn a_url_keeps_its_host_and_loses_its_credentials() {
    for (text, kept) in [
        ("push to https://user:ghp_abc@github.com/a/b.git failed", "push to https://***@github.com/a/b.git failed"),
        ("https://ghp_abc@github.com/a", "https://***@github.com/a"),
        ("'postgres://me:p@ss@db:5432/x'", "'postgres://***@db:5432/x'"),
        ("https://example.com/a@b no secret", "https://example.com/a@b no secret"),
        ("plain text, a@b.c", "plain text, a@b.c"),
    ] {
        assert_eq!(without_credentials(text), kept);
    }
    assert!(!line(Level::Warn, "zen", "https://u:secret@h/").contains("secret"));
}

#[cfg(unix)]
#[test]
fn the_log_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let log = FileLog::open(dir.path().to_owned(), &[]);
    log.write_line("line\n");
    let mode = std::fs::metadata(dir.path().join(FILE_NAME)).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
