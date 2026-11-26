use serde_json::json;
use zen_http::test_server::TestServer;

use super::super::testing::*;
use super::*;

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196,
    137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 248, 207, 80, 15, 0, 3, 134, 1, 128, 90, 52, 125, 107, 0, 0,
    0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn png_reply() -> Vec<u8> {
    let mut reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        PNG.len()
    )
    .into_bytes();
    reply.extend_from_slice(PNG);
    reply
}

fn not_found() -> Vec<u8> {
    TestServer::reply(404, "Not Found", &[], "")
}

struct Fixture {
    repo: tempfile::TempDir,
    data: tempfile::TempDir,
    registry: Registry,
}

impl Fixture {
    fn new() -> Self {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        Fixture { repo: test_repository(), data: tempfile::tempdir().unwrap(), registry }
    }

    fn session(&self, on: bool, server: &str) -> Session {
        let mut session = Session::new();
        self.registry.dispatch(&mut session, "file.project.open", json!({ "path": self.repo.path() })).unwrap();
        session.settings_mut().set(AVATARS, on).unwrap();
        let avatars = session.state_mut::<Avatars>();
        avatars.set_folder(self.data.path().join("avatars"));
        avatars.set_endpoints(Endpoints {
            gravatar: format!("{server}/avatar"),
            github_avatars: server.to_owned(),
            github: server.to_owned(),
            gitlab: Some(server.to_owned()),
        });
        session
    }

    fn get(&self, session: &mut Session, email: &str) -> AvatarInfo {
        let answer =
            self.registry.dispatch(session, "vcs.avatar.get", json!({ "emails": [email], "wait": true })).unwrap();
        let mut list: Vec<AvatarInfo> = serde_json::from_value(answer).unwrap();
        list.remove(0)
    }
}

#[test]
fn nothing_is_fetched_while_the_setting_is_off() {
    let f = Fixture::new();
    let server = TestServer::start(vec![png_reply()]);
    let mut session = f.session(false, &server.url(""));
    let info = f.get(&mut session, "ada@example.com");
    assert_eq!(info.status, AvatarStatus::Off);
    assert_eq!(server.connections(), 0, "no network while it is off");
    const { assert!(AVATARS_BY_DEFAULT) };
    let mut fresh = Session::new();
    let answer = f.registry.dispatch(&mut fresh, "vcs.avatar.get", json!({ "emails": ["a@b.c"] })).unwrap();
    assert_eq!(answer[0]["status"], "none");
}

#[test]
fn a_picture_comes_from_gravatar_by_the_hash_and_is_kept_on_disk() {
    let f = Fixture::new();
    let server = TestServer::start(vec![png_reply()]);
    let mut session = f.session(true, &server.url(""));
    let info = f.get(&mut session, " Ada@Example.com ");
    assert_eq!((info.status, info.source), (AvatarStatus::Found, Some(AvatarSource::Gravatar)));
    let file = info.file.unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
    let hash = email_hash("ada@example.com");
    assert_eq!(hash, hex::encode(Sha256::digest(b"ada@example.com")));
    let asked = server.finish();
    assert_eq!(asked[0].target, format!("/avatar/{hash}?s=80&d=404"), "the address itself is not sent");
    assert_eq!(f.get(&mut session, "ada@example.com").status, AvatarStatus::Found);
    let mut other = f.session(true, &server.url(""));
    let again = f.get(&mut other, "ada@example.com");
    assert_eq!((again.status, again.file), (AvatarStatus::Found, Some(file)));
    assert_eq!(server.connections(), 1);
    other.settings_mut().set(AVATARS, false).unwrap();
    assert_eq!(f.get(&mut other, "ada@example.com").status, AvatarStatus::Off);
}

#[test]
fn a_gitlab_remote_is_asked_first_for_its_public_avatar() {
    let f = Fixture::new();
    git(f.repo.path(), &["remote", "add", "origin", "git@gitlab.com:group/project.git"]);
    let server = TestServer::start(vec![
        TestServer::json(200, r#"{"avatar_url": "/uploads/-/system/user/avatar/7/ada.png"}"#),
        png_reply(),
    ]);
    let mut session = f.session(true, &server.url(""));
    let info = f.get(&mut session, "ada@example.com");
    assert_eq!((info.status, info.source), (AvatarStatus::Found, Some(AvatarSource::Gitlab)));
    let asked: Vec<String> = server.finish().into_iter().map(|request| request.target).collect();
    assert_eq!(asked, ["/api/v4/avatar?email=ada%40example.com&size=80", "/uploads/-/system/user/avatar/7/ada.png"]);
}

#[test]
fn a_github_no_reply_address_shows_its_accounts_picture() {
    let f = Fixture::new();
    git(f.repo.path(), &["remote", "add", "origin", "https://github.com/owner/repo.git"]);
    let server = TestServer::start(vec![png_reply()]);
    let mut session = f.session(true, &server.url(""));
    let info = f.get(&mut session, "123+ada@users.noreply.github.com");
    assert_eq!((info.status, info.source), (AvatarStatus::Found, Some(AvatarSource::Github)));
    assert_eq!(server.finish()[0].target, "/u/123?s=80&v=4");
}

#[test]
fn without_a_picture_the_initials_stay_and_it_is_not_asked_again_for_a_week() {
    let f = Fixture::new();
    git(f.repo.path(), &["remote", "add", "origin", "https://gitlab.example.org/group/project.git"]);
    let server = TestServer::start(vec![TestServer::json(200, r#"{"avatar_url": null}"#), not_found()]);
    let mut session = f.session(true, &server.url(""));
    assert_eq!(f.get(&mut session, "nobody@example.com").status, AvatarStatus::None);
    assert_eq!(server.connections(), 2);
    let mut other = f.session(true, &server.url(""));
    assert_eq!(f.get(&mut other, "nobody@example.com").status, AvatarStatus::None);
    assert_eq!(server.connections(), 2, "kept on disk as having none");
    assert!(f.data.path().join("avatars").join(format!("{}.none", email_hash("nobody@example.com"))).is_file());
}

#[test]
fn offline_the_initials_stay_and_the_address_is_asked_again_later() {
    let f = Fixture::new();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut session = f.session(true, &format!("http://127.0.0.1:{closed}"));
    assert_eq!(f.get(&mut session, "ada@example.com").status, AvatarStatus::None);
    let folder = f.data.path().join("avatars");
    assert!(!folder.join(format!("{}.none", email_hash("ada@example.com"))).exists(), "not marked as having none");
}

#[test]
fn remotes_name_their_hosting() {
    let endpoints = Endpoints::default();
    let urls = |list: &[&str]| list.iter().map(|url| (*url).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        hostings(
            &urls(&[
                "git@gitlab.com:acme/tools/shop.git",
                "https://github.com/owner/repo",
                "ssh://git@gitlab.example.org:2222/group/repo.git",
                "https://gitlab.com/other/repo.git",
                "/srv/git/repo.git",
                "https://example.com/repo.git",
            ]),
            &endpoints
        ),
        [
            Hosting::Gitlab("https://gitlab.com".to_owned()),
            Hosting::Github,
            Hosting::Gitlab("https://gitlab.example.org".to_owned()),
        ]
    );
    assert_eq!(url_host("https://user:secret@GitLab.com:8443/a/b"), Some("gitlab.com".to_owned()));
    assert_eq!(url_host("../relative/repo"), None);
    assert_eq!(github_noreply("123+Ada@users.noreply.github.com"), Some((Some(123), "ada".to_owned())));
    assert_eq!(github_noreply("ada@users.noreply.github.com"), Some((None, "ada".to_owned())));
    assert_eq!(github_noreply("ada@example.com"), None);
    assert_eq!(query_escape("a+b@c.d"), "a%2Bb%40c.d");
}
