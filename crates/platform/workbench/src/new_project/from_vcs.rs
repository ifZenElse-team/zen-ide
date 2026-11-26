use super::{
    CreateAnswer, Created, INVALID_LOCATION, JobOutput, NewProjects, Staging, answer, check_location,
    default_directory, publish, shared,
};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Session};
use zen_i18n::t;

const MAX_URLS: usize = 20;

fn repository_name(url: &str) -> Option<String> {
    let path = url.trim().trim_end_matches('/');
    let last = path.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_owned())
}

fn is_helper_address(url: &str) -> bool {
    url.split_once("::").is_some_and(|(helper, _)| helper.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn url_problem(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return Some(t!("workbench-clone-no-url"));
    }
    (url.starts_with('-') || url.chars().any(char::is_control) || is_helper_address(url))
        .then(|| t!("workbench-clone-bad-url"))
}

fn without_credentials(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_owned() };
    let (authority, path) = rest.split_once('/').map_or((rest, None), |(authority, path)| (authority, Some(path)));
    let Some((user, host)) = authority.rsplit_once('@') else { return url.to_owned() };
    let web = matches!(scheme, "http" | "https");
    let kept = match user.split_once(':') {
        _ if web => host.to_owned(),
        Some((name, _)) => format!("{name}@{host}"),
        None => authority.to_owned(),
    };
    path.map_or_else(|| format!("{scheme}://{kept}"), |path| format!("{scheme}://{kept}/{path}"))
}

/// What the Clone Repository dialog starts with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloneDefaults {
    /// The addresses cloned before, newest first, without passwords.
    pub urls: Vec<String>,
    /// Where a new clone goes: Default project directory.
    pub directory: PathBuf,
}

pub struct FromVcsDefaults;

impl Command for FromVcsDefaults {
    type Params = NoParams;
    type Output = CloneDefaults;
    const ID: &'static str = "project.fromVcs.defaults";
    const TITLE: &'static str = "Get Clone Defaults";
    const DESCRIPTION: &'static str = "What Clone Repository starts with: the addresses cloned before and the default directory. Works with no project open.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<CloneDefaults, CommandError> {
        let urls = shared(session).lock().kept.clone_urls.clone();
        Ok(CloneDefaults { urls, directory: default_directory(session) })
    }
}

/// A clone to check.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CheckParams {
    /// The repository's address.
    pub url: String,
    /// The folder to clone into; absent for the suggested one.
    pub directory: Option<String>,
}

/// Whether a clone can start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloneCheck {
    /// The folder the address suggests under the default directory.
    pub suggested: PathBuf,
    /// Why Clone is disabled; absent when it can start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

fn directory_problem(directory: &str) -> Option<String> {
    let check = check_location(directory);
    let not_empty = || t!("workbench-new-project-not-empty", path = check.path.display().to_string());
    check.problem.clone().or_else(|| check.not_empty.then(not_empty))
}

pub struct FromVcsCheck;

impl Command for FromVcsCheck {
    type Params = CheckParams;
    type Output = CloneCheck;
    const ID: &'static str = "project.fromVcs.check";
    const TITLE: &'static str = "Check Clone";
    const DESCRIPTION: &'static str = "Whether `url` can be cloned into `directory`: `problem` is why Clone is disabled; `suggested` is the folder the address suggests. Touches no network.";

    fn run(&self, session: &mut Session, params: CheckParams) -> Result<CloneCheck, CommandError> {
        let base = default_directory(session);
        let suggested = repository_name(&params.url).map_or_else(|| base.clone(), |name| base.join(name));
        let directory = params.directory.unwrap_or_else(|| suggested.to_string_lossy().into_owned());
        let problem = url_problem(&params.url).or_else(|| directory_problem(&directory));
        Ok(CloneCheck { suggested, problem })
    }
}

/// What to clone.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CloneParams {
    /// The repository's address.
    pub url: String,
    /// The folder to clone into; it must not exist or be empty.
    pub directory: String,
    /// Keep only this many commits of the history (Shallow clone).
    pub depth: Option<u32>,
    /// How long to wait for it to end, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
}

fn clone_into(
    url: &str,
    target: &Path,
    depth: Option<u32>,
    stop: &AtomicBool,
    output: &JobOutput,
    programs: &zen_run::Programs,
) -> Result<Created, String> {
    let mut staging = Staging::new(target)?;
    let request = zen_vcs::git::CloneRequest { url: url.to_owned(), folder: staging.folder.clone(), depth };
    let running = programs.clone();
    let watcher: zen_vcs::git::Watcher = Box::new(move |spawned| match spawned {
        zen_vcs::git::Spawned::Started(pid) => running.add(pid),
        zen_vcs::git::Spawned::Ended(pid) => running.remove(pid),
    });
    let cloned = zen_vcs::git::watched(watcher, || {
        zen_vcs::git::clone(&request, &mut |progress| {
            let phase = if progress.phase.is_empty() { progress.line.clone() } else { progress.phase.clone() };
            output.progress(&phase, progress.fraction);
        })
    });
    if stop.load(Ordering::Relaxed) {
        return Err(t!("workbench-clone-cancelled"));
    }
    cloned.map_err(|error| error.to_string())?;
    let kept = publish(&mut staging, target)?;
    Ok(Created { path: target.to_owned(), kept, ..Created::default() })
}

impl NewProjects {
    pub(super) fn remember_url(&self, url: &str) {
        let url = without_credentials(url);
        self.change_kept(|kept| {
            kept.clone_urls.retain(|known| *known != url);
            kept.clone_urls.insert(0, url);
            kept.clone_urls.truncate(MAX_URLS);
        });
    }
}

pub struct FromVcs;

impl Command for FromVcs {
    type Params = CloneParams;
    type Output = CreateAnswer;
    const ID: &'static str = "project.fromVcs.clone";
    const TITLE: &'static str = "Clone Repository";
    const DESCRIPTION: &'static str = "Clones `url`, without its submodules, into `directory` in the background, using the network; `depth` truncates the history. A hidden folder beside `directory` takes its place only when the clone worked, so a failure or project.new.cancel leaves nothing. Follow it with project.new.status, or give `waitMs`: `progress` says how far it is. A clone is never trusted (ADR-0205): the person trusts it after opening it. Does not open the project: call `open_project` with `path`.";

    fn run(&self, session: &mut Session, params: CloneParams) -> Result<CreateAnswer, CommandError> {
        let problem = url_problem(&params.url).or_else(|| directory_problem(&params.directory));
        if let Some(problem) = problem {
            return Err(CommandError::with_code(INVALID_LOCATION, problem));
        }
        let projects = shared(session);
        let (working, url, target) =
            (projects.clone(), params.url.trim().to_owned(), check_location(&params.directory).path);
        let depth = params.depth.filter(|depth| *depth > 0);
        let job = projects.start(false, move |stop, output, programs| {
            let created = clone_into(&url, &target, depth, stop, output, programs)?;
            working.remember_url(&url);
            Ok(created)
        });
        answer(session, &projects, job, params.wait_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_follows_the_repository_s_name() {
        for url in [
            "https://gitlab.com/acme/shop.git",
            "https://gitlab.com/acme/shop/",
            "git@gitlab.com:acme/shop.git",
            "git@gitlab.com:shop",
            "/Users/me/shop",
        ] {
            assert_eq!(repository_name(url).as_deref(), Some("shop"), "{url}");
        }
        assert_eq!(repository_name(""), None);
    }

    #[test]
    fn the_history_keeps_no_passwords() {
        assert_eq!(without_credentials("https://me:secret@host/a.git"), "https://host/a.git");
        assert_eq!(without_credentials("https://token@host/a.git"), "https://host/a.git");
        assert_eq!(without_credentials("ssh://git:secret@host/a.git"), "ssh://git@host/a.git");
        assert_eq!(without_credentials("ssh://git@host/a.git"), "ssh://git@host/a.git");
        assert_eq!(without_credentials("git@host:a.git"), "git@host:a.git");
        assert_eq!(without_credentials("https://host/a.git"), "https://host/a.git");
    }

    #[test]
    fn an_address_that_would_run_a_program_or_be_an_option_is_refused() {
        for url in ["ext::sh -c id", "fd::3", "--upload-pack=x", "a\nb"] {
            assert!(url_problem(url).is_some(), "{url}");
        }
        for url in ["https://host/a.git", "git@host:a.git", "file:///a", "/local/a"] {
            assert_eq!(url_problem(url), None, "{url}");
        }
    }
}
