use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};

use super::repository;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetAvatars)?;
    Ok(())
}

pub const AVATARS: &str = "vcs.workspace.avatars";
pub const AVATARS_BY_DEFAULT: bool = true;
pub const AVATAR_HOSTS: &[&str] = &["gravatar.com", "gitlab.com", "github.com", "avatars.githubusercontent.com"];
pub const AVATAR_SIZE: u32 = 80;
const RETRY_NONE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
#[cfg(not(target_arch = "wasm32"))]
const TIMEOUT: Duration = Duration::from_secs(10);
const WORKERS: usize = 4;
const WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Endpoints {
    pub gravatar: String,
    pub github_avatars: String,
    pub github: String,
    pub gitlab: Option<String>,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            gravatar: "https://gravatar.com/avatar".to_owned(),
            github_avatars: "https://avatars.githubusercontent.com".to_owned(),
            github: "https://github.com".to_owned(),
            gitlab: None,
        }
    }
}

/// What is known of an address's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AvatarStatus {
    /// The setting is off: nothing is fetched, the initials show.
    Off,
    /// Being fetched.
    Pending,
    /// Found: `file` holds it.
    Found,
    /// No picture (or no network): the initials show.
    None,
}

/// Where a picture came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AvatarSource {
    /// The GitLab the repository's remote is on.
    Gitlab,
    /// GitHub, by a no-reply address.
    Github,
    /// Gravatar.
    Gravatar,
}

impl AvatarSource {
    fn name(self) -> &'static str {
        match self {
            AvatarSource::Gitlab => "gitlab",
            AvatarSource::Github => "github",
            AvatarSource::Gravatar => "gravatar",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [AvatarSource::Gitlab, AvatarSource::Github, AvatarSource::Gravatar]
            .into_iter()
            .find(|s| s.name() == name.trim())
    }
}

/// An address's picture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AvatarInfo {
    /// The e-mail address, as asked.
    pub email: String,
    /// What is known of its picture.
    pub status: AvatarStatus,
    /// Where it came from, once found.
    pub source: Option<AvatarSource>,
    /// The file it is kept in (PNG, JPEG, GIF or WebP), once found.
    pub file: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Hosting {
    Gitlab(String),
    Github,
}

fn hostings(urls: &[String], endpoints: &Endpoints) -> Vec<Hosting> {
    let mut found = Vec::new();
    for url in urls {
        let Some(host) = url_host(url) else { continue };
        let hosting = if host == "github.com" || host.ends_with(".github.com") {
            Hosting::Github
        } else if host.split(['.', '-']).any(|part| part == "gitlab") {
            Hosting::Gitlab(endpoints.gitlab.clone().unwrap_or_else(|| format!("https://{host}")))
        } else {
            continue;
        };
        if !found.contains(&hosting) {
            found.push(hosting);
        }
    }
    found
}

fn url_host(url: &str) -> Option<String> {
    let authority = match url.split_once("://") {
        Some((_, rest)) => rest.split('/').next()?,
        None => url.split_once(':').map(|(before, _)| before).filter(|before| !before.contains('/'))?,
    };
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?.trim().to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn normalized(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn email_hash(email: &str) -> String {
    hex::encode(Sha256::digest(normalized(email).as_bytes()))
}

fn github_noreply(email: &str) -> Option<(Option<u64>, String)> {
    let local = normalized(email).strip_suffix("@users.noreply.github.com")?.to_owned();
    match local.split_once('+') {
        Some((id, name)) => Some((id.parse().ok(), name.to_owned())),
        None => Some((None, local)),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Known {
    Pending,
    Found(AvatarSource, PathBuf),
    None,
}

#[derive(Default)]
struct Queue {
    known: HashMap<String, Known>,
    jobs: VecDeque<Job>,
    workers: usize,
}

struct Job {
    email: String,
    hostings: Vec<Hosting>,
}

#[derive(Default)]
pub struct Avatars {
    folder: Option<PathBuf>,
    endpoints: Endpoints,
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl Avatars {
    pub fn set_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
    }

    pub fn set_endpoints(&mut self, endpoints: Endpoints) {
        self.endpoints = endpoints;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.shared.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn look_up(&self, email: &str, hostings: &dyn Fn() -> Vec<Hosting>) -> Known {
        let email = normalized(email);
        if let Some(known) = self.lock().known.get(&email) {
            return known.clone();
        }
        let Some(folder) = self.folder.clone() else { return Known::None };
        if let Some(known) = on_disk(&folder, &email) {
            self.lock().known.insert(email, known.clone());
            return known;
        }
        let job = Job { email: email.clone(), hostings: hostings() };
        let mut queue = self.lock();
        queue.known.insert(email, Known::Pending);
        queue.jobs.push_back(job);
        if queue.workers < WORKERS {
            queue.workers += 1;
            let (shared, endpoints) = (Arc::clone(&self.shared), self.endpoints.clone());
            std::thread::spawn(move || work(&shared, &folder, &endpoints));
        }
        Known::Pending
    }

    fn wait(&self, emails: &[String]) {
        let (lock, changed) = &*self.shared;
        let mut queue = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let deadline = std::time::Instant::now() + WAIT;
        while emails.iter().any(|email| queue.known.get(&normalized(email)) == Some(&Known::Pending)) {
            let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) else { return };
            queue = changed.wait_timeout(queue, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }
}

fn on_disk(folder: &Path, email: &str) -> Option<Known> {
    let hash = email_hash(email);
    let picture = folder.join(format!("{hash}.img"));
    if picture.is_file() {
        let source = std::fs::read_to_string(folder.join(format!("{hash}.source")))
            .ok()
            .and_then(|name| AvatarSource::from_name(&name))
            .unwrap_or(AvatarSource::Gravatar);
        return Some(Known::Found(source, picture));
    }
    let none = folder.join(format!("{hash}.none"));
    let asked = std::fs::metadata(&none).and_then(|meta| meta.modified()).ok()?;
    let age = SystemTime::now().duration_since(asked).unwrap_or_default();
    (age < RETRY_NONE).then_some(Known::None)
}

fn work(shared: &(Mutex<Queue>, Condvar), folder: &Path, endpoints: &Endpoints) {
    loop {
        let job = {
            let mut queue = shared.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match queue.jobs.pop_front() {
                Some(job) => job,
                None => {
                    queue.workers -= 1;
                    return;
                }
            }
        };
        let known = fetch_and_keep(folder, endpoints, &job);
        let mut queue = shared.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.known.insert(job.email, known);
        shared.1.notify_all();
    }
}

fn fetch_and_keep(folder: &Path, endpoints: &Endpoints, job: &Job) -> Known {
    let hash = email_hash(&job.email);
    let mut reached = false;
    for (source, url) in candidates(endpoints, job, &hash) {
        let Some(url) = url else { continue };
        match fetch_picture(&url) {
            Fetched::Picture(bytes) => {
                let _ = std::fs::create_dir_all(folder);
                let file = folder.join(format!("{hash}.img"));
                if zen_vfs::write_atomic(&file, &bytes).is_ok() {
                    let _ = std::fs::write(folder.join(format!("{hash}.source")), source.name());
                    let _ = std::fs::remove_file(folder.join(format!("{hash}.none")));
                    return Known::Found(source, file);
                }
                return Known::None;
            }
            Fetched::Nothing => reached = true,
            Fetched::Unreachable => {}
        }
    }
    if reached {
        let _ = std::fs::create_dir_all(folder);
        let _ = std::fs::write(folder.join(format!("{hash}.none")), b"");
    }
    Known::None
}

fn candidates(endpoints: &Endpoints, job: &Job, hash: &str) -> Vec<(AvatarSource, Option<Lookup>)> {
    let mut list = Vec::new();
    for hosting in &job.hostings {
        match hosting {
            Hosting::Gitlab(base) => {
                let url = format!(
                    "{}/api/v4/avatar?email={}&size={AVATAR_SIZE}",
                    base.trim_end_matches('/'),
                    query_escape(&job.email)
                );
                list.push((AvatarSource::Gitlab, Some(Lookup::GitlabApi(url))));
            }
            Hosting::Github => {
                let url = github_noreply(&job.email).map(|(id, name)| match id {
                    Some(id) => {
                        format!("{}/u/{id}?s={AVATAR_SIZE}&v=4", endpoints.github_avatars.trim_end_matches('/'))
                    }
                    None => format!("{}/{name}.png?size={AVATAR_SIZE}", endpoints.github.trim_end_matches('/')),
                });
                list.push((AvatarSource::Github, url.map(Lookup::Picture)));
            }
        }
    }
    let gravatar = format!("{}/{hash}?s={AVATAR_SIZE}&d=404", endpoints.gravatar.trim_end_matches('/'));
    list.push((AvatarSource::Gravatar, Some(Lookup::Picture(gravatar))));
    list
}

enum Lookup {
    Picture(String),
    GitlabApi(String),
}

enum Fetched {
    Picture(Vec<u8>),
    Nothing,
    Unreachable,
}

fn fetch_picture(lookup: &Lookup) -> Fetched {
    match lookup {
        Lookup::Picture(url) => picture(url),
        Lookup::GitlabApi(url) => {
            let response = match get(url) {
                Ok(response) => response,
                Err(()) => return Fetched::Unreachable,
            };
            let address = (response.status == 200)
                .then(|| serde_json::from_slice::<serde_json::Value>(&response.body).ok())
                .flatten()
                .and_then(|json| json["avatar_url"].as_str().map(str::to_owned));
            match address {
                Some(address) if address.starts_with('/') => {
                    let base = url.split("/api/v4/").next().unwrap_or_default();
                    picture(&format!("{base}{address}"))
                }
                Some(address) if address.starts_with("http") => picture(&address),
                _ => Fetched::Nothing,
            }
        }
    }
}

fn picture(url: &str) -> Fetched {
    match get(url) {
        Ok(response) if response.status == 200 => {
            let bytes = response.body;
            if zen_image::decode(Path::new("avatar.img"), &bytes).is_ok() {
                Fetched::Picture(bytes)
            } else {
                Fetched::Nothing
            }
        }
        Ok(_) => Fetched::Nothing,
        Err(()) => Fetched::Unreachable,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn get(url: &str) -> Result<zen_http::Response, ()> {
    let headers = vec![("Accept".to_owned(), "image/*, application/json".to_owned())];
    let prepared = zen_http::Prepared {
        method: "GET".to_owned(),
        url: url.to_owned(),
        headers: headers.clone(),
        body: None,
        follow_redirects: true,
        use_proxy: true,
        shown: zen_http::Shown { url: url.to_owned(), headers, body: None },
        files: Vec::new(),
    };
    zen_http::send(&prepared, TIMEOUT).map_err(drop)
}

#[cfg(target_arch = "wasm32")]
fn get(_url: &str) -> Result<zen_http::Response, ()> {
    Err(())
}

fn query_escape(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Which pictures to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AvatarParams {
    /// The authors' e-mail addresses.
    pub emails: Vec<String>,
    /// Wait until they are fetched (at most 30 seconds) instead of answering at once.
    pub wait: bool,
}

pub struct GetAvatars;

impl Command for GetAvatars {
    type Params = AvatarParams;
    type Output = Vec<AvatarInfo>;
    const ID: &'static str = "vcs.avatar.get";
    const TITLE: &'static str = "Author Pictures";
    const DESCRIPTION: &'static str = "The pictures of authors by e-mail address, which the Git workspace's graph shows on the \
         commits' nodes instead of the initials. Only while the setting vcs.workspace.avatars is on \
         (Settings → Version Control → Git Workspace, `gitAvatars` of set_settings; on by default): \
         each is looked up on the repository's GitLab (its public avatar by e-mail) or GitHub (a \
         no-reply address's account), then on Gravatar (the address's SHA-256 hash), in the background, \
         and kept in the data folder. `status` is `off` while the setting is off, `pending` while \
         fetched (`wait` waits), `found` with the `file` and its `source`, or `none` (the initials \
         show; an address without a picture is asked again after a week).";

    fn run(&self, session: &mut Session, params: AvatarParams) -> Result<Vec<AvatarInfo>, CommandError> {
        let on = session.settings().get::<bool>(AVATARS).unwrap_or(AVATARS_BY_DEFAULT);
        if !on {
            let off = |email: &String| AvatarInfo {
                email: email.clone(),
                status: AvatarStatus::Off,
                source: None,
                file: None,
            };
            return Ok(params.emails.iter().map(off).collect());
        }
        let urls: Vec<String> = repository(session)
            .ok()
            .and_then(|repository| repository.remote_urls().ok())
            .map(|urls| urls.into_iter().map(|(_, url)| url).collect())
            .unwrap_or_default();
        let avatars = session.state_mut::<Avatars>();
        let lookup = || hostings(&urls, &avatars.endpoints);
        for email in &params.emails {
            avatars.look_up(email, &lookup);
        }
        if params.wait {
            avatars.wait(&params.emails);
        }
        let queue = avatars.lock();
        Ok(params
            .emails
            .iter()
            .map(|email| {
                let (status, source, file) = match queue.known.get(&normalized(email)) {
                    Some(Known::Found(source, file)) => (AvatarStatus::Found, Some(*source), Some(file.clone())),
                    Some(Known::Pending) => (AvatarStatus::Pending, None, None),
                    _ => (AvatarStatus::None, None, None),
                };
                AvatarInfo { email: email.clone(), status, source, file }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
