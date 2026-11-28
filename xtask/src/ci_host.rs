#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Host {
    None,
    Github,
    Unknown,
}

pub(crate) const RELEASE_BRANCH: &str = "release";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ci {
    pub(crate) host: Host,
    reference: String,
    branch: Option<String>,
    commit: Option<String>,
    run: Option<String>,
    protected: bool,
    pull_request: bool,
}

const MARKERS: [&str; 2] = ["CI", "CONTINUOUS_INTEGRATION"];

impl Ci {
    pub(crate) fn from_environment() -> Self {
        Self::read(|name| std::env::var(name).ok())
    }

    pub(crate) fn read(env: impl Fn(&str) -> Option<String>) -> Self {
        let value = |name: &str| env(name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let on = |name: &str| value(name).as_deref() == Some("true");
        let marked = MARKERS.iter().any(|name| value(name).is_some_and(|value| value != "false" && value != "0"));
        match (on("GITLAB_CI"), on("GITHUB_ACTIONS")) {
            (false, true) => {
                let reference = value("GITHUB_REF").unwrap_or_default();
                let branch = reference.strip_prefix("refs/heads/").map(str::to_owned).filter(|branch| {
                    value("GITHUB_REF_TYPE").as_deref() == Some("branch")
                        && value("GITHUB_REF_NAME").as_deref() == Some(branch.as_str())
                });
                let pull_request = reference.starts_with("refs/pull/")
                    || value("GITHUB_EVENT_NAME").is_some_and(|event| event.starts_with("pull_request"));
                Self {
                    host: Host::Github,
                    reference,
                    branch,
                    commit: value("GITHUB_SHA"),
                    run: value("GITHUB_RUN_ID"),
                    protected: on("GITHUB_REF_PROTECTED") && !pull_request,
                    pull_request,
                }
            }
            (false, false) if !marked => Self::none(),
            _ => Self { host: Host::Unknown, ..Self::none() },
        }
    }

    pub(crate) fn none() -> Self {
        Self {
            host: Host::None,
            reference: String::new(),
            branch: None,
            commit: None,
            run: None,
            protected: false,
            pull_request: false,
        }
    }

    pub(crate) fn run(&self) -> Option<&str> {
        self.run.as_deref()
    }

    pub(crate) fn commit(&self) -> Option<&str> {
        self.commit.as_deref()
    }

    fn on_release_branch(&self) -> bool {
        !self.pull_request && self.branch.as_deref() == Some(RELEASE_BRANCH)
    }

    fn ref_shown(&self) -> &str {
        if self.reference.is_empty() { "unknown" } else { &self.reference }
    }

    pub(crate) fn secrets_problem(&self) -> Option<String> {
        let refused = "nothing was signed or sent";
        match self.host {
            Host::None => None,
            Host::Github if self.pull_request => {
                Some(format!("the release secrets are never used by a pull request's workflow (ADR-0300); {refused}"))
            }
            Host::Github if !self.on_release_branch() => Some(format!(
                "the release secrets are used only by a workflow run of the branch `{RELEASE_BRANCH}` (ADR-0301): \
                 this run's ref is {}; {refused}",
                self.ref_shown()
            )),
            Host::Github => (!self.protected).then(|| {
                format!(
                    "the release secrets are used only by a workflow run of the protected branch `{RELEASE_BRANCH}` \
                     (ADR-0301): this run's ref is not protected (GITHUB_REF_PROTECTED is not true); {refused}"
                )
            }),
            Host::Unknown => Some(format!(
                "the release secrets are used only on a maintainer's machine or in GitHub Actions: this is a CI of \
                 another kind (CI is set, and GITHUB_ACTIONS is not true, or GITLAB_CI is set too); {refused}"
            )),
        }
    }

    pub(crate) fn release_problem(&self, built: Option<&str>) -> Option<String> {
        match self.host {
            Host::None => None,
            Host::Github if !self.on_release_branch() => Some(format!(
                "a release runs only in a workflow run of the branch `{RELEASE_BRANCH}` (ADR-0301): this run's ref \
                 is {}",
                self.ref_shown()
            )),
            Host::Github => match (built, self.commit()) {
                (_, None) => Some("this run does not say its commit (GITHUB_SHA is not set)".to_owned()),
                (Some(built), Some(commit)) if built != commit => Some(format!(
                    "this run releases commit {commit}, the branch `{RELEASE_BRANCH}` it was started for; the build \
                     is of commit {built}"
                )),
                _ => None,
            },
            Host::Unknown => Some(
                "a release runs on a maintainer's machine or in GitHub Actions: this is a CI of another kind"
                    .to_owned(),
            ),
        }
    }

    pub(crate) fn secrets_come_from_variables(&self) -> bool {
        self.host == Host::Github
    }

    pub(crate) fn where_variables_live(&self) -> &'static str {
        "GitHub gives the release secrets only to the jobs of the `release` environment, after its reviewers \
         approve (docs/releasing.md)"
    }
}

pub(crate) fn moved_problem(commit: &str, tip: Option<&str>) -> Option<String> {
    match tip {
        Some(tip) if tip == commit => None,
        Some(tip) => Some(format!(
            "the branch `{RELEASE_BRANCH}` has moved to {tip} since this run started for {commit}: only the run of \
             the commit it points at releases (the newer run does), and nothing was signed, sent or made live"
        )),
        None => Some(format!(
            "cannot read where the branch `{RELEASE_BRANCH}` points (git ls-remote origin); nothing was signed, sent \
             or made live"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci(variables: &[(&str, &str)]) -> Ci {
        Ci::read(|name| variables.iter().find(|(known, _)| *known == name).map(|(_, value)| (*value).to_owned()))
    }

    fn github(reference: &str, kind: &str, name: &str, protected: &str, event: &str) -> Ci {
        ci(&[
            ("CI", "true"),
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REF", reference),
            ("GITHUB_REF_TYPE", kind),
            ("GITHUB_REF_NAME", name),
            ("GITHUB_REF_PROTECTED", protected),
            ("GITHUB_EVENT_NAME", event),
            ("GITHUB_RUN_ID", "9001"),
            ("GITHUB_SHA", "c0ffee"),
        ])
    }

    fn release(protected: &str, event: &str) -> Ci {
        github("refs/heads/release", "branch", "release", protected, event)
    }

    #[test]
    fn a_maintainers_machine_is_no_ci_and_nothing_is_refused_there() {
        for variables in [&[][..], &[("CI", "")], &[("CI", "false")], &[("CI", "0")], &[("GITHUB_ACTIONS", "false")]] {
            let ci = ci(variables);
            assert_eq!(ci, Ci::none(), "{variables:?}");
            assert_eq!(ci.secrets_problem(), None);
            assert_eq!(ci.release_problem(None), None);
            assert_eq!(ci.release_problem(Some("c0ffee")), None);
            assert_eq!((ci.commit(), ci.run()), (None, None));
            assert!(!ci.secrets_come_from_variables());
        }
    }

    #[test]
    fn gitlab_is_a_ci_of_another_kind_now() {
        let tag = ci(&[
            ("CI", "true"),
            ("GITLAB_CI", "true"),
            ("CI_COMMIT_TAG", "v26.10.4"),
            ("CI_COMMIT_REF_PROTECTED", "true"),
            ("CI_PIPELINE_ID", "2142"),
        ]);
        assert_eq!((tag.host, tag.commit(), tag.run()), (Host::Unknown, None, None));
        assert!(tag.secrets_problem().unwrap().contains("another kind"));
        assert!(tag.release_problem(None).is_some());
        assert!(!tag.secrets_come_from_variables());
    }

    #[test]
    fn only_the_protected_branch_release_reads_the_secrets_and_releases() {
        for event in ["push", "workflow_dispatch"] {
            let run = release("true", event);
            assert_eq!((run.host, run.commit(), run.run()), (Host::Github, Some("c0ffee"), Some("9001")));
            assert_eq!(run.secrets_problem(), None, "{event}");
            assert_eq!(run.release_problem(None), None);
            assert_eq!(run.release_problem(Some("c0ffee")), None);
            assert!(run.secrets_come_from_variables());
        }
        let unprotected = release("false", "push");
        assert!(unprotected.secrets_problem().unwrap().contains("GITHUB_REF_PROTECTED"));
        assert_eq!(unprotected.release_problem(None), None, "the builds need no secret; the plan refuses it first");
        let missing = ci(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REF", "refs/heads/release"),
            ("GITHUB_REF_TYPE", "branch"),
            ("GITHUB_REF_NAME", "release"),
        ]);
        assert!(missing.secrets_problem().is_some(), "unknown means not protected");
        assert!(missing.release_problem(None).unwrap().contains("GITHUB_SHA"));
    }

    #[test]
    fn a_run_releases_only_the_commit_it_was_started_for() {
        let run = release("true", "push");
        let problem = run.release_problem(Some("deadbeef")).unwrap();
        assert!(problem.contains("c0ffee") && problem.contains("deadbeef"), "{problem}");
        assert_eq!(moved_problem("c0ffee", Some("c0ffee")), None);
        assert!(moved_problem("c0ffee", Some("f00d")).unwrap().contains("has moved to f00d"));
        assert!(moved_problem("c0ffee", None).unwrap().contains("cannot read"));
    }

    #[test]
    fn main_other_branches_and_tags_never_read_the_secrets_nor_release() {
        for (reference, kind, name) in [
            ("refs/heads/main", "branch", "main"),
            ("refs/heads/feature", "branch", "feature"),
            ("refs/heads/release/notes-26.11.11", "branch", "release/notes-26.11.11"),
            ("refs/tags/v26.11.11", "tag", "v26.11.11"),
            ("refs/tags/release", "tag", "release"),
        ] {
            for protected in ["true", "false"] {
                let run = github(reference, kind, name, protected, "push");
                let refused = run.secrets_problem().unwrap();
                assert!(refused.contains(reference) && refused.contains("nothing was signed"), "{refused}");
                assert!(run.release_problem(None).unwrap().contains(reference), "{reference}");
            }
        }
        let mismatched = github("refs/heads/release", "branch", "main", "true", "push");
        assert!(mismatched.secrets_problem().is_some(), "the three variables must agree");
        let typed_wrong = github("refs/heads/release", "tag", "release", "true", "push");
        assert!(typed_wrong.release_problem(None).is_some());
    }

    #[test]
    fn on_github_a_pull_request_never_reads_the_secrets() {
        for (reference, event) in [
            ("refs/pull/7/merge", "pull_request"),
            ("refs/pull/7/merge", "pull_request_target"),
            ("refs/heads/release", "pull_request_target"),
            ("refs/heads/release", "pull_request"),
        ] {
            for protected in ["true", "false"] {
                let run = github(reference, "branch", reference.trim_start_matches("refs/heads/"), protected, event);
                let refused = run.secrets_problem().unwrap();
                assert!(refused.contains("pull request") && refused.contains("nothing was signed"), "{refused}");
                assert!(run.release_problem(None).is_some());
            }
        }
    }

    #[test]
    fn a_ci_of_another_kind_is_refused() {
        for variables in [
            &[("CI", "true")][..],
            &[("CI", "1")],
            &[("CONTINUOUS_INTEGRATION", "true")],
            &[("CI", "true"), ("GITLAB_CI", "false")],
            &[("GITLAB_CI", "true"), ("GITHUB_ACTIONS", "true"), ("GITHUB_REF", "refs/heads/release")],
        ] {
            let ci = ci(variables);
            assert_eq!(ci.host, Host::Unknown, "{variables:?}");
            assert!(ci.secrets_problem().unwrap().contains("nothing was signed"));
            assert!(ci.release_problem(None).is_some());
            assert!(!ci.secrets_come_from_variables());
        }
    }
}
