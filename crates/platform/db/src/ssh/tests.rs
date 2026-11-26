use super::*;

fn settings(host: &str) -> SshSettings {
    SshSettings { host: host.to_owned(), ..SshSettings::default() }
}

fn texts(args: &[OsString]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

#[test]
fn a_tunnel_forwards_a_loopback_port_through_the_alias_with_its_jumps() {
    let ssh = SshSettings {
        host: "bastion".to_owned(),
        user: Some("deploy".to_owned()),
        port: Some(2222),
        jump: Some("jump1,jump2:2200".to_owned()),
    };
    let args = texts(&tunnel_args(&ssh, 40001, ("db.internal", 5432), Duration::from_secs(15)).unwrap());
    let joined = args.join(" ");
    for part in [
        "-v -N -L 127.0.0.1:40001:db.internal:5432",
        "-l deploy",
        "-p 2222",
        "-J jump1,jump2:2200",
        "-o ExitOnForwardFailure=yes",
        "-o ControlMaster=no",
        "-o ControlPath=none",
        "-o ConnectTimeout=15",
        "-- bastion",
    ] {
        assert!(joined.contains(part), "{part} in {joined}");
    }
    assert!(!joined.contains("StrictHostKeyChecking"), "host keys are ssh's own checks");
    assert_eq!(args.last().map(String::as_str), Some("bastion"));
    let v6 = texts(&tunnel_args(&settings("b"), 1, ("::1", 5432), Duration::from_secs(1)).unwrap());
    assert!(v6.contains(&"127.0.0.1:1:[::1]:5432".to_owned()), "{v6:?}");
}

#[test]
fn options_in_disguise_are_refused() {
    let wait = Duration::from_secs(1);
    for host in ["-oProxyCommand=touch /tmp/x", "", "two words", "a'b"] {
        assert!(tunnel_args(&settings(host), 1, ("db", 1), wait).is_err(), "{host:?}");
    }
    assert!(tunnel_args(&settings("b"), 1, ("-db", 1), wait).is_err());
    let user = SshSettings { user: Some("-x".to_owned()), ..settings("b") };
    assert!(tunnel_args(&user, 1, ("db", 1), wait).is_err());
    let jump = SshSettings { jump: Some("ok,-oProxyCommand=x".to_owned()), ..settings("b") };
    assert!(tunnel_args(&jump, 1, ("db", 1), wait).is_err());
}

#[test]
fn the_control_path_fits_the_socket_limit_of_macos() {
    let folder = short_folder();
    let longest = folder.join(format!("{}.{}", "f".repeat(40), "X".repeat(16)));
    assert!(longest.as_os_str().len() < 104, "{}", longest.display());
}

#[test]
fn what_ssh_said_is_cut_to_what_matters() {
    let said = "Warning: Permanently added 'bastion' (ED25519) to the list of known hosts.\n\n\
                deploy@bastion: Permission denied (publickey,password).\n";
    assert_eq!(clean_stderr(said), "deploy@bastion: Permission denied (publickey,password).");
}

#[cfg(unix)]
#[test]
fn the_helper_asks_the_ide_over_its_socket() {
    let folder = tempfile::tempdir().unwrap();
    let socket = folder.path().join("ask.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let request: AskRequest = serde_json::from_str(&line).unwrap();
        let reply = AskReply { answer: Some(format!("answer to {}", request.prompt)) };
        let mut stream = stream;
        writeln!(stream, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
        request.token
    });
    let answer = ask(&socket, &AskRequest { token: "t1".to_owned(), prompt: "pass?".to_owned() });
    assert_eq!(answer.as_deref(), Some("answer to pass?"));
    assert_eq!(server.join().unwrap(), "t1");
    assert_eq!(
        ask(&folder.path().join("none.sock"), &AskRequest { token: String::new(), prompt: String::new() }),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_folder_others_can_enter_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("s");
    private_folder(&folder).unwrap();
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_folder(&folder).is_err());
}

const ROOT_TEMP_CHILD: &str = "ZEN_DB_TEST_PRIVATE_FOLDER";

#[cfg(unix)]
#[test]
fn the_private_folder_belongs_to_the_person_when_the_temp_folder_belongs_to_root() {
    if let Some(folder) = std::env::var_os(ROOT_TEMP_CHILD) {
        private_folder(Path::new(&folder)).unwrap();
        let uid = rustix::process::geteuid().as_raw();
        assert_eq!(short_folder(), PathBuf::from(format!("/tmp/zen-ssh-{uid}")));
        return;
    }
    let parent = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "ssh::tests::the_private_folder_belongs_to_the_person_when_the_temp_folder_belongs_to_root",
            "--test-threads=1",
        ])
        .env("TMPDIR", "/")
        .env(ROOT_TEMP_CHILD, parent.path().join("s"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() && said.contains("1 passed"), "{said}{}", String::from_utf8_lossy(&output.stderr));
}

pub(crate) fn client_hello() -> (SocketAddr, std::thread::JoinHandle<Vec<u8>>) {
    use std::io::Read;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let heard = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut header = [0u8; 5];
        stream.read_exact(&mut header).unwrap();
        let mut hello = vec![0u8; usize::from(u16::from_be_bytes([header[3], header[4]]))];
        stream.read_exact(&mut hello).unwrap();
        hello
    });
    (address, heard)
}

pub(crate) fn names(hello: &[u8], host: &str) -> bool {
    hello.windows(host.len()).any(|window| window == host.as_bytes())
}

#[test]
fn the_askpass_helper_reaches_a_loopback_port_as_on_windows() {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let served = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let request: AskRequest = serde_json::from_str(&line).unwrap();
        let mut stream = stream;
        let answer = if request.token == "t0ken" { Some("pass".to_owned()) } else { None };
        writeln!(stream, "{}", serde_json::to_string(&AskReply { answer }).unwrap()).unwrap();
    });
    let channel = PathBuf::from(format!("{TCP_CHANNEL}{address}"));
    let asked = ask(&channel, &AskRequest { token: "t0ken".to_owned(), prompt: "Password:".to_owned() });
    served.join().unwrap();
    assert_eq!(asked.as_deref(), Some("pass"));
    let elsewhere = PathBuf::from(format!("{TCP_CHANNEL}192.0.2.1:22"));
    assert_eq!(ask(&elsewhere, &AskRequest { token: "t".to_owned(), prompt: "p".to_owned() }), None, "loopback only");
}

#[test]
fn only_ssh_s_own_report_of_the_forward_on_the_chosen_port_counts() {
    assert!(reports_forward("debug1: Local forwarding listening on 127.0.0.1 port 40001.", 40001));
    assert!(reports_forward("debug1: Local forwarding listening on 127.0.0.1 port 40001.\r", 40001));
    assert!(!reports_forward("debug1: Local forwarding listening on 127.0.0.1 port 400010.", 40001));
    assert!(!reports_forward("debug1: Local forwarding listening on 127.0.0.1 port 4000.", 40001));
    assert!(!reports_forward("debug1: Local forwarding listening on 0.0.0.0 port 40001.", 40001));
    assert!(!reports_forward("echo debug1: Local forwarding listening on 127.0.0.1 port 40001.", 40001));
    assert_eq!(
        clean_stderr("debug1: Reading configuration data\nPermission denied (publickey)."),
        "Permission denied (publickey)."
    );
}
