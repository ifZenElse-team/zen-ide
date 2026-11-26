use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Family, Tool};

pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const HOMEBREW: [&str; 3] = ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"];

const KEGS: [&str; 5] = ["libpq", "postgresql", "mysql-client", "mysql", "mariadb"];

/// Where a tool was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// A folder of `PATH` or a usual system folder.
    Path,
    /// Homebrew's folders, keg-only formulae included.
    Homebrew,
    /// Postgres.app.
    App,
    /// Inside the database's Docker container, run with `docker exec`.
    Docker,
}

/// A running Docker container that publishes the connection's port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    /// The container's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its image.
    pub image: String,
    /// The port the database listens on inside it.
    pub port: u16,
}

/// A tool found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// Which tool it is.
    pub tool: Tool,
    /// Where it was found.
    pub origin: Origin,
    /// The program: a path on this computer, or the name inside the container.
    pub program: PathBuf,
    /// The container it runs in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<Container>,
    /// The `docker` program that runs it in the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<PathBuf>,
    /// The first line of its `--version`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Its major version, for the server's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub major: Option<u32>,
}

/// What was learnt of Docker.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum DockerState {
    /// Not looked at: the connection's host is not this computer.
    #[default]
    NotLocal,
    /// The `docker` program was not found.
    NotFound,
    /// `docker ps` failed: the daemon is not running, or no access.
    NotRunning {
        /// What it said.
        detail: String,
    },
    /// No running container publishes the connection's port.
    NoContainer,
    /// The container that publishes it.
    Container {
        /// The container.
        container: Container,
    },
}

/// The tools found for a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    /// Every tool found, on this computer first.
    pub tools: Vec<Found>,
    /// What was learnt of Docker.
    pub docker: DockerState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub folders: Vec<(PathBuf, Origin)>,
    pub docker: Option<PathBuf>,
    pub timeout: Duration,
}

impl Default for Probe {
    fn default() -> Self {
        Probe { folders: Vec::new(), docker: None, timeout: PROBE_TIMEOUT }
    }
}

impl Probe {
    pub fn system() -> Probe {
        Probe::system_outside(None)
    }

    pub fn system_outside(project: Option<&Path>) -> Probe {
        let path: Vec<PathBuf> =
            std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect()).unwrap_or_default();
        Probe::on_path(path, project)
    }

    pub(super) fn on_path(path: Vec<PathBuf>, project: Option<&Path>) -> Probe {
        let path = path.into_iter().filter(|folder| outside(folder, project)).collect();
        let folders = folders(path, &HOMEBREW.map(PathBuf::from), Path::new("/Applications/Postgres.app"));
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from);
        let docker = docker_program(&folders, home.as_deref());
        Probe { folders, docker, timeout: PROBE_TIMEOUT }
    }
}

pub fn outside(folder: &Path, project: Option<&Path>) -> bool {
    if !folder.is_absolute() {
        return false;
    }
    let Some(project) = project else { return true };
    let canonical = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    !canonical(folder).starts_with(canonical(project))
}

fn origin_of(folder: &Path, homebrew: &[PathBuf]) -> Origin {
    if folder.starts_with("/Applications/Postgres.app") {
        return Origin::App;
    }
    let brewed = homebrew.iter().any(|prefix| {
        let local = prefix == Path::new("/usr/local");
        let inside = ["bin", "opt", "Cellar"].iter().any(|part| folder.starts_with(prefix.join(part)));
        inside && !(local && folder == prefix.join("bin"))
    });
    if brewed { Origin::Homebrew } else { Origin::Path }
}

fn version_key(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit()).filter_map(|part| part.parse().ok()).collect()
}

pub fn folders(path: Vec<PathBuf>, homebrew: &[PathBuf], postgres_app: &Path) -> Vec<(PathBuf, Origin)> {
    let mut all: Vec<PathBuf> = path;
    for prefix in homebrew {
        all.push(prefix.join("bin"));
        let mut kegs: Vec<(String, PathBuf)> = std::fs::read_dir(prefix.join("opt"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let family = name.split('@').next().unwrap_or_default();
                KEGS.contains(&family).then(|| (name, entry.path().join("bin")))
            })
            .collect();
        kegs.sort_by(|a, b| version_key(&b.0).cmp(&version_key(&a.0)).then_with(|| a.0.cmp(&b.0)));
        all.extend(kegs.into_iter().map(|(_, bin)| bin));
    }
    let mut versions: Vec<(String, PathBuf)> = std::fs::read_dir(postgres_app.join("Contents/Versions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path().join("bin")))
        .filter(|(name, _)| name != "latest")
        .collect();
    versions.sort_by_key(|b| std::cmp::Reverse(version_key(&b.0)));
    all.extend(versions.into_iter().map(|(_, bin)| bin));
    all.extend(["/usr/bin", "/bin"].map(PathBuf::from));
    let mut unique: Vec<(PathBuf, Origin)> = Vec::new();
    for folder in all {
        if folder.as_os_str().is_empty() || unique.iter().any(|(known, _)| *known == folder) {
            continue;
        }
        let origin = origin_of(&folder, homebrew);
        unique.push((folder, origin));
    }
    unique
}

fn executable(folder: &Path, name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { folder.join(format!("{name}.exe")) } else { folder.join(name) };
    file.is_file().then_some(file)
}

fn docker_program(folders: &[(PathBuf, Origin)], home: Option<&Path>) -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = folders.iter().map(|(folder, _)| folder.clone()).collect();
    places.push(PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"));
    places.push(PathBuf::from("/Applications/OrbStack.app/Contents/MacOS/xbin"));
    if let Some(home) = home {
        places.push(home.join(".orbstack/bin"));
        places.push(home.join(".docker/bin"));
        places.push(home.join(".rd/bin"));
    }
    places.iter().find_map(|folder| executable(folder, "docker"))
}

pub fn capture(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_string(&mut text);
        }
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} did not answer within {} s", program.display(), timeout.as_secs()));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let text = out.join().unwrap_or_default();
    let errors = err.join().unwrap_or_default();
    if status.success() {
        Ok(text)
    } else {
        let said = errors.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_owned();
        Err(if said.is_empty() { format!("{} failed: {status}", program.display()) } else { said })
    }
}

pub fn parse_version(line: &str) -> Option<(String, u32)> {
    let rest = line
        .split_once("Distrib ")
        .map(|(_, rest)| rest)
        .or_else(|| line.split_once(" from ").map(|(_, rest)| rest))
        .unwrap_or(line);
    let word = rest
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()) && word.contains('.'))?;
    let version: String = word.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let version = version.trim_end_matches('.').to_owned();
    let major = version.split('.').next()?.parse().ok()?;
    Some((version, major))
}

fn with_version(mut found: Found, line: Option<String>) -> Found {
    let line = line.map(|line| line.trim().to_owned()).filter(|line| !line.is_empty());
    found.major = line.as_deref().and_then(parse_version).map(|(_, major)| major);
    found.version = line;
    found
}

pub fn local(family: Family, folders: &[(PathBuf, Origin)]) -> Vec<Found> {
    let mut found = Vec::new();
    for &tool in family.tools() {
        for (folder, origin) in folders {
            let Some(program) = tool.programs(family).iter().find_map(|name| executable(folder, name)) else {
                continue;
            };
            let real = std::fs::canonicalize(&program).unwrap_or_else(|_| program.clone());
            let same = |known: &Found| {
                known.tool == tool
                    && std::fs::canonicalize(&known.program).unwrap_or_else(|_| known.program.clone()) == real
            };
            if found.iter().any(same) {
                continue;
            }
            found.push(Found {
                tool,
                origin: *origin,
                program,
                container: None,
                docker: None,
                version: None,
                major: None,
            });
        }
    }
    found
}

pub fn is_local(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "0.0.0.0"
        || host.eq_ignore_ascii_case("host.docker.internal")
}

pub fn published(ports: &str) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    for mapping in ports.split(',').map(str::trim) {
        let Some((host, inside)) = mapping.split_once("->") else { continue };
        let host_ports = host.rsplit(':').next().unwrap_or_default();
        let inside_ports = inside.split('/').next().unwrap_or_default();
        let range = |text: &str| -> Option<(u16, u16)> {
            match text.split_once('-') {
                Some((from, to)) => Some((from.parse().ok()?, to.parse().ok()?)),
                None => text.parse().ok().map(|port| (port, port)),
            }
        };
        let (Some((host_from, host_to)), Some((inside_from, _))) = (range(host_ports), range(inside_ports)) else {
            continue;
        };
        for offset in 0..=host_to.saturating_sub(host_from).min(1024) {
            let pair = (host_from + offset, inside_from.saturating_add(offset));
            if !found.contains(&pair) {
                found.push(pair);
            }
        }
    }
    found
}

pub fn container_for(listing: &str, port: u16) -> Option<Container> {
    listing.lines().find_map(|line| {
        let mut fields = line.split('\t');
        let id = fields.next()?.trim();
        let name = fields.next()?.trim();
        let image = fields.next()?.trim();
        let ports = fields.next().unwrap_or_default();
        let (_, inside) = published(ports).into_iter().find(|(host, _)| *host == port)?;
        (!id.is_empty()).then(|| Container {
            id: id.to_owned(),
            name: name.to_owned(),
            image: image.to_owned(),
            port: inside,
        })
    })
}

fn container_script(family: Family) -> String {
    let mut names: Vec<&str> = Vec::new();
    for tool in family.tools() {
        for name in tool.programs(family) {
            if !names.contains(name) {
                names.push(name);
            }
        }
    }
    names
        .iter()
        .map(|name| {
            format!("if command -v {name} >/dev/null 2>&1; then printf '%s\\t%s\\n' {name} \"$({name} --version 2>&1 | head -n 1)\"; fi")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn in_container(family: Family, printed: &str, container: &Container, docker: &Path) -> Vec<Found> {
    let listed: Vec<(&str, &str)> = printed.lines().filter_map(|line| line.split_once('\t')).collect();
    family
        .tools()
        .iter()
        .filter_map(|&tool| {
            let (name, version) = tool
                .programs(family)
                .iter()
                .find_map(|name| listed.iter().find(|(listed, _)| listed == name).copied())?;
            let found = Found {
                tool,
                origin: Origin::Docker,
                program: PathBuf::from(name),
                container: Some(container.clone()),
                docker: Some(docker.to_owned()),
                version: None,
                major: None,
            };
            Some(with_version(found, Some(version.to_owned())))
        })
        .collect()
}

// WHY: it runs programs, so a call on the window's thread freezes the window.
pub fn detect(probe: &Probe, family: Family, host: &str, port: u16) -> Detection {
    let tools: Vec<Found> = local(family, &probe.folders)
        .into_iter()
        .map(|found| {
            let line = capture(&found.program, &["--version"], probe.timeout)
                .ok()
                .and_then(|text| text.lines().next().map(str::to_owned));
            with_version(found, line)
        })
        .collect();
    let mut detection = Detection { tools, docker: DockerState::NotLocal };
    if !is_local(host) {
        return detection;
    }
    let Some(docker) = &probe.docker else {
        detection.docker = DockerState::NotFound;
        return detection;
    };
    let format = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Ports}}";
    let listing = match capture(docker, &["ps", "--format", format], probe.timeout) {
        Ok(listing) => listing,
        Err(detail) => {
            detection.docker = DockerState::NotRunning { detail };
            return detection;
        }
    };
    let Some(container) = container_for(&listing, port) else {
        detection.docker = DockerState::NoContainer;
        return detection;
    };
    let script = container_script(family);
    if let Ok(printed) = capture(docker, &["exec", &container.id, "sh", "-c", &script], probe.timeout) {
        detection.tools.extend(in_container(family, &printed, &container, docker));
    }
    detection.docker = DockerState::Container { container };
    detection
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionProblem {
    TooNew { tool: u32, server: u32 },
    TooOld { tool: u32, server: u32 },
}

pub fn for_server<'a>(
    candidates: &[&'a Found],
    chosen: Option<&'a Found>,
    server: Option<u32>,
) -> Result<Option<&'a Found>, VersionProblem> {
    let (Some(server), Some(first)) = (server, chosen.or_else(|| candidates.first().copied())) else {
        return Ok(chosen);
    };
    let tool = first.tool;
    if !matches!(tool, Tool::PgDump | Tool::PgRestore | Tool::Psql) {
        return Ok(chosen);
    }
    let known: Vec<(&'a Found, u32)> = candidates.iter().filter_map(|found| Some((*found, found.major?))).collect();
    if known.is_empty() {
        return Ok(chosen);
    }
    if let Some(chosen) = chosen.filter(|chosen| chosen.major == Some(server)) {
        return Ok(Some(chosen));
    }
    if let Some((same, _)) = known.iter().find(|(_, major)| *major == server) {
        return Ok(Some(*same));
    }
    let newest = known.iter().map(|(_, major)| *major).max().unwrap_or(0);
    let picked = if tool == Tool::PgDump {
        known.iter().filter(|(_, major)| *major >= server).min_by_key(|(_, major)| *major)
    } else {
        known.iter().filter(|(_, major)| *major <= server).max_by_key(|(_, major)| *major)
    };
    match picked {
        Some((found, _)) => Ok(Some(*found)),
        None if tool == Tool::PgDump => Err(VersionProblem::TooOld { tool: newest, server }),
        None if tool == Tool::Psql => Ok(chosen),
        None => Err(VersionProblem::TooNew { tool: newest, server }),
    }
}

pub fn choose(detection: &Detection, tool: Tool) -> Option<&Found> {
    let local = detection
        .tools
        .iter()
        .filter(|found| found.tool == tool && found.origin != Origin::Docker)
        .enumerate()
        .max_by(|(a_index, a), (b_index, b)| {
            a.major.unwrap_or(0).cmp(&b.major.unwrap_or(0)).then_with(|| b_index.cmp(a_index))
        })
        .map(|(_, found)| found);
    let docker = detection.tools.iter().find(|found| found.tool == tool && found.origin == Origin::Docker);
    match (local, docker) {
        (Some(local), Some(docker)) if local.major.unwrap_or(0) < docker.major.unwrap_or(0) => Some(docker),
        (Some(local), _) => Some(local),
        (None, docker) => docker,
    }
}
