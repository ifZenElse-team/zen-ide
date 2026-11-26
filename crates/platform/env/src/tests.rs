use super::*;

fn fake_shell(dir: &Path, body: &str) -> PathBuf {
    let shell = dir.join("shell");
    std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    shell
}

const LOGIN: &str = concat!(
    "echo 'a prompt theme says hello'\n",
    "export FROM_LOGIN=yes KUBECONFIG=/k/a.yaml:/k/b.yaml PATH=/opt/tools/bin:/usr/bin:/bin\n",
    "export ZEN_SECRET=no DYLD_INSERT_LIBRARIES=/evil.dylib LD_PRELOAD=/evil.so SHLVL=3\n",
    "export SPACED='two words' NEWLINE='first\nsecond' EQUALS='a=b'\n",
    "[ \"$1 $2 $3\" = '-l -i -c' ] || exit 9\n",
    "export SCRIPT=\"$4\" MARKED='x</zen-environment>y<zen-environment>z'\n",
    "eval \"$4\"\n",
    "echo 'goodbye from the shell'\n",
);

#[test]
fn the_environment_is_what_the_login_shell_prints_between_the_markers() {
    let nonce = "0123456789abcdef0123456789abcdef";
    let output = b"junk <zen-environment-0123456789abcdef0123456789abcdef>first\0PATH=/x:/y\0A=1\0B=one=two\0SHLVL=2\0ZEN_X=1\0HOME=/elsewhere\0</zen-environment-0123456789abcdef0123456789abcdef>\njunk";
    let variables = parse(output, nonce).unwrap();
    assert_eq!(variables.get("PATH").map(String::as_str), Some("/x:/y"));
    assert_eq!(variables.get("B").map(String::as_str), Some("one=two"));
    for dropped in ["SHLVL", "ZEN_X", "HOME", "first\n"] {
        assert!(!variables.contains_key(dropped), "{dropped}");
    }
    let marked = parse(
        b"<zen-environment>A=0\0</zen-environment>\n<zen-environment-0123456789abcdef0123456789abcdef>A=<zen-environment>1</zen-environment>2\0</zen-environment-0123456789abcdef0123456789abcdef>",
        nonce,
    )
    .unwrap();
    assert_eq!(marked.get("A").map(String::as_str), Some("<zen-environment>1</zen-environment>2"));
    assert!(parse(output, "fedcba9876543210fedcba9876543210").is_err(), "another read's markers");
    assert!(parse(b"no markers at all", nonce).is_err());
    let cut = b"<zen-environment-0123456789abcdef0123456789abcdef>PATH=/x";
    assert!(parse(cut, nonce).is_err(), "cut short");
    let empty = b"<zen-environment-0123456789abcdef0123456789abcdef>SHLVL=1\0</zen-environment-0123456789abcdef0123456789abcdef>";
    assert!(parse(empty, nonce).is_err(), "nothing taken");
}

#[test]
fn every_read_has_its_own_random_markers_that_its_script_never_spells_out() {
    let first = nonce().unwrap();
    let second = nonce().unwrap();
    assert_eq!(first.len(), 32);
    assert!(first.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
    assert_ne!(first, second);
    let script = &command_args(&first)[3];
    let (open, close) = markers(&first);
    assert!(script.contains(&first));
    assert!(!script.contains(&open) && !script.contains(&close), "{script}");
}

#[cfg(unix)]
#[test]
fn a_profile_that_prints_markers_and_junk_around_the_environment_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let guessed = "<zen-environment-00000000000000000000000000000000>";
    let profile = format!(
        "printf '<zen-environment>GUESSED=1\\000</zen-environment>\\n{guessed}GUESSED=2\\000\\n'\n{LOGIN}\nprintf '</zen-environment>AFTER=1\\000</zen-environment>junk\\n'"
    );
    let shell = fake_shell(dir.path(), &profile);
    let environment = read(&shell, dir.path(), Duration::from_secs(10)).unwrap();
    assert_eq!(environment.get("FROM_LOGIN"), Some("yes"));
    assert_eq!(environment.get("MARKED"), Some("x</zen-environment>y<zen-environment>z"));
    assert_eq!(environment.get("GUESSED"), None);
    assert_eq!(environment.get("AFTER"), None);
}

#[cfg(unix)]
#[test]
fn a_login_shell_is_read_in_the_home_folder_without_what_the_ide_never_takes() {
    let dir = tempfile::tempdir().unwrap();
    let shell = fake_shell(dir.path(), &format!("{LOGIN}\npwd > \"$HOME/where\""));
    let environment = read(&shell, dir.path(), Duration::from_secs(10)).unwrap();
    assert_eq!(environment.get("FROM_LOGIN"), Some("yes"));
    assert_eq!(environment.get("KUBECONFIG"), Some("/k/a.yaml:/k/b.yaml"));
    assert_eq!(environment.get("PATH"), Some("/opt/tools/bin:/usr/bin:/bin"));
    assert_eq!(environment.get("SPACED"), Some("two words"));
    assert_eq!(environment.get("NEWLINE"), Some("first\nsecond"));
    assert_eq!(environment.get("EQUALS"), Some("a=b"));
    assert!(
        environment.get("SCRIPT").is_some_and(|script| script.contains("/usr/bin/env -0")),
        "a value may hold the script"
    );
    assert_eq!(environment.get("MARKED"), Some("x</zen-environment>y<zen-environment>z"));
    for never in ["ZEN_SECRET", "DYLD_INSERT_LIBRARIES", "LD_PRELOAD", "SHLVL", "HOME", "PWD"] {
        assert_eq!(environment.get(never), None, "{never}");
    }
    let wd = std::fs::read_to_string(dir.path().join("where")).unwrap();
    assert_eq!(Path::new(wd.trim()).canonicalize().unwrap(), dir.path().canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn a_shell_that_fails_hangs_or_prints_junk_gives_no_environment() {
    let dir = tempfile::tempdir().unwrap();
    let failing = fake_shell(dir.path(), "echo oops; exit 3");
    assert!(read(&failing, dir.path(), Duration::from_secs(10)).unwrap_err().contains("no environment"));
    let started = Instant::now();
    let hanging = fake_shell(dir.path(), "exec sleep 30");
    let error = read(&hanging, dir.path(), Duration::from_millis(500)).unwrap_err();
    assert!(error.contains("did not answer"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10), "the reader gives up at its timeout");
    assert!(read(&dir.path().join("no-such-shell"), dir.path(), Duration::from_secs(1)).is_err());
}

#[cfg(unix)]
#[test]
fn the_environment_is_read_once_in_the_background_and_used_only_while_on() {
    let dir = tempfile::tempdir().unwrap();
    let shell = fake_shell(dir.path(), &format!("sleep 1\n{LOGIN}"));
    assert_eq!(current(), None, "nothing before it is read");
    let before = Instant::now();
    assert!(start(shell.clone(), dir.path().to_owned(), Duration::from_secs(10)));
    assert!(before.elapsed() < Duration::from_millis(500), "starting never waits for the shell");
    assert_eq!(state(), State::Reading);
    assert!(!start(shell, dir.path().to_owned(), Duration::from_secs(10)), "once per start");
    assert!(matches!(wait(Duration::from_secs(20)), State::Ready(_)));
    assert_eq!(var("FROM_LOGIN"), Some(OsString::from("yes")));
    assert_eq!(path_folders().first(), Some(&PathBuf::from("/opt/tools/bin")));
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "printf '%s|%s' \"$FROM_LOGIN\" \"$OWN\""]).env("OWN", "kept");
    apply(&mut command);
    command.env("FROM_LOGIN", "overridden");
    let output = command.output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "overridden|kept", "what a caller sets after apply wins");
    set_enabled(false);
    assert_eq!(current(), None);
    assert!(variables().is_empty());
    assert_eq!(var("FROM_LOGIN"), std::env::var_os("FROM_LOGIN"), "off: the IDE's own environment only");
    set_enabled(true);
    assert!(current().is_some());
}
