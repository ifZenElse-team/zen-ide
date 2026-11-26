#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Host {
    None,
    Gitlab,
    Github,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ci {
    pub(crate) host: Host,
    tag: Option<String>,
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
            (true, false) => Self {
                host: Host::Gitlab,
                tag: value("CI_COMMIT_TAG"),
                run: value("CI_PIPELINE_ID"),
                protected: on("CI_COMMIT_REF_PROTECTED"),
                pull_request: false,
            },
            (false, true) => {
                let reference = value("GITHUB_REF").unwrap_or_default();
                let tag = reference.strip_prefix("refs/tags/").map(str::to_owned).filter(|tag| {
                    value("GITHUB_REF_TYPE").as_deref() == Some("tag")
                        && value("GITHUB_REF_NAME").as_deref() == Some(tag.as_str())
                });
                let pull_request = reference.starts_with("refs/pull/")
                    || value("GITHUB_EVENT_NAME").is_some_and(|event| event.starts_with("pull_request"));
                Self {
                    host: Host::Github,
                    tag,
                    run: value("GITHUB_RUN_ID"),
                    protected: on("GITHUB_REF_PROTECTED") && !pull_request,
                    pull_request,
                }
            }
            (false, false) if !marked => Self::none(),
            _ => Self { host: Host::Unknown, tag: None, run: None, protected: false, pull_request: false },
        }
    }

    pub(crate) fn none() -> Self {
        Self { host: Host::None, tag: None, run: None, protected: false, pull_request: false }
    }

    pub(crate) fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub(crate) fn run(&self) -> Option<&str> {
        self.run.as_deref()
    }

    pub(crate) fn tag_variable(&self) -> &'static str {
        match self.host {
            Host::Github => "GITHUB_REF_NAME",
            _ => "CI_COMMIT_TAG",
        }
    }

    pub(crate) fn secrets_problem(&self) -> Option<String> {
        let refused = "nothing was signed or sent";
        match self.host {
            Host::None => None,
            Host::Gitlab => (!self.protected).then(|| {
                format!(
                    "the release secrets are used only by a pipeline of a protected tag (ADR-0171): this job's ref \
                     is not protected (CI_COMMIT_REF_PROTECTED is not true); {refused}"
                )
            }),
            Host::Github if self.pull_request => {
                Some(format!("the release secrets are never used by a pull request's workflow (ADR-0300); {refused}"))
            }
            Host::Github => (!self.protected).then(|| {
                format!(
                    "the release secrets are used only by a workflow run of a protected ref (ADR-0300): this run's \
                     ref is not protected (GITHUB_REF_PROTECTED is not true); {refused}"
                )
            }),
            Host::Unknown => Some(format!(
                "the release secrets are used only on a maintainer's machine, in GitLab CI or in GitHub Actions: \
                 this is a CI of another kind (CI is set, and exactly one of GITLAB_CI and GITHUB_ACTIONS must be \
                 true); {refused}"
            )),
        }
    }

    pub(crate) fn tag_problem(&self, tag: &str) -> Option<String> {
        match self.host {
            Host::None => None,
            Host::Gitlab | Host::Github => (self.tag() != Some(tag)).then(|| {
                format!(
                    "a release runs only in the pipeline of its own tag: this pipeline's tag is {}, not {tag}",
                    self.tag().unwrap_or("none")
                )
            }),
            Host::Unknown => Some(format!(
                "a release runs on a maintainer's machine, in GitLab CI or in GitHub Actions: this is a CI of \
                 another kind, and {tag} is not released from it"
            )),
        }
    }

    pub(crate) fn secrets_come_from_variables(&self) -> bool {
        matches!(self.host, Host::Gitlab | Host::Github)
    }

    pub(crate) fn where_variables_live(&self) -> &'static str {
        match self.host {
            Host::Github => {
                "GitHub gives the release secrets only to the jobs of the `release` environment, after its \
                 reviewers approve (docs/releasing.md)"
            }
            _ => {
                "GitLab gives the release variables only to the jobs of the `release` environment in a pipeline of \
                 a protected tag (docs/releasing.md)"
            }
        }
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
        ])
    }

    #[test]
    fn a_maintainers_machine_is_no_ci_and_nothing_is_refused_there() {
        for variables in [&[][..], &[("CI", "")], &[("CI", "false")], &[("CI", "0")], &[("GITHUB_ACTIONS", "false")]] {
            let ci = ci(variables);
            assert_eq!(ci, Ci::none(), "{variables:?}");
            assert_eq!(ci.secrets_problem(), None);
            assert_eq!(ci.tag_problem("v26.10.4"), None);
            assert_eq!((ci.tag(), ci.run()), (None, None));
            assert!(!ci.secrets_come_from_variables());
        }
    }

    #[test]
    fn gitlab_is_read_as_before() {
        let tag = ci(&[
            ("CI", "true"),
            ("GITLAB_CI", "true"),
            ("CI_COMMIT_TAG", "v26.10.4"),
            ("CI_COMMIT_REF_PROTECTED", "true"),
            ("CI_PIPELINE_ID", "2142"),
        ]);
        assert_eq!((tag.host, tag.tag(), tag.run()), (Host::Gitlab, Some("v26.10.4"), Some("2142")));
        assert_eq!(tag.secrets_problem(), None);
        assert_eq!(tag.tag_problem("v26.10.4"), None);
        assert!(tag.tag_problem("v26.10.3").is_some());
        assert!(tag.secrets_come_from_variables());

        let branch = ci(&[("GITLAB_CI", "true"), ("CI_COMMIT_REF_PROTECTED", "false")]);
        let refused = branch.secrets_problem().unwrap();
        assert!(refused.contains("CI_COMMIT_REF_PROTECTED") && refused.contains("nothing was signed"), "{refused}");
        assert!(branch.tag_problem("v26.10.4").unwrap().contains("none"), "a branch pipeline");
        assert!(ci(&[("GITLAB_CI", "true")]).secrets_problem().is_some(), "unknown means not protected");

        let main = ci(&[("GITLAB_CI", "true"), ("CI_COMMIT_REF_PROTECTED", "true")]);
        assert_eq!(main.secrets_problem(), None, "the credentials check runs on protected main");
        assert!(main.tag_problem("v26.10.4").is_some(), "and releases nothing");
    }

    #[test]
    fn on_github_a_pull_request_never_reads_the_secrets() {
        for (reference, event) in [
            ("refs/pull/7/merge", "pull_request"),
            ("refs/pull/7/merge", "pull_request_target"),
            ("refs/heads/main", "pull_request_target"),
        ] {
            for protected in ["true", "false"] {
                let ci = github(reference, "branch", "7/merge", protected, event);
                let refused = ci.secrets_problem().unwrap();
                assert!(refused.contains("pull request") && refused.contains("nothing was signed"), "{refused}");
                assert!(ci.tag_problem("v26.10.4").is_some());
            }
        }
    }

    #[test]
    fn on_github_a_branch_push_reads_the_secrets_only_when_protected_and_releases_nothing() {
        let branch = github("refs/heads/feature", "branch", "feature", "false", "push");
        assert!(branch.secrets_problem().unwrap().contains("GITHUB_REF_PROTECTED"));
        assert!(branch.tag_problem("v26.10.4").unwrap().contains("none"));

        let main = github("refs/heads/main", "branch", "main", "true", "workflow_dispatch");
        assert_eq!(main.secrets_problem(), None, "the credentials check runs on protected main");
        assert!(main.tag_problem("v26.10.4").is_some(), "and releases nothing");

        let named_like_a_tag = github("refs/heads/v26.10.4", "branch", "v26.10.4", "true", "push");
        assert_eq!(named_like_a_tag.tag(), None);
        assert!(named_like_a_tag.tag_problem("v26.10.4").is_some(), "a branch named as a tag is not the tag");
    }

    #[test]
    fn on_github_only_a_protected_tag_releases_and_only_itself() {
        let unprotected = github("refs/tags/v26.10.4", "tag", "v26.10.4", "false", "push");
        assert_eq!(unprotected.tag(), Some("v26.10.4"));
        assert!(unprotected.secrets_problem().unwrap().contains("not protected"));
        assert_eq!(unprotected.tag_problem("v26.10.4"), None, "the build job of the tag needs no secret");

        let missing = ci(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REF", "refs/tags/v26.10.4"),
            ("GITHUB_REF_TYPE", "tag"),
            ("GITHUB_REF_NAME", "v26.10.4"),
        ]);
        assert!(missing.secrets_problem().is_some(), "unknown means not protected");

        let protected = github("refs/tags/v26.10.4", "tag", "v26.10.4", "true", "push");
        assert_eq!((protected.host, protected.tag(), protected.run()), (Host::Github, Some("v26.10.4"), Some("9001")));
        assert_eq!(protected.secrets_problem(), None);
        assert_eq!(protected.tag_problem("v26.10.4"), None);
        assert!(protected.tag_problem("v26.10.3").unwrap().contains("v26.10.4"));
        assert!(protected.secrets_come_from_variables());

        let mismatched = github("refs/tags/v26.10.4", "tag", "v26.10.3", "true", "push");
        assert_eq!(mismatched.tag(), None, "the three variables must agree");
    }

    #[test]
    fn a_ci_of_another_kind_is_refused() {
        for variables in [
            &[("CI", "true")][..],
            &[("CI", "1")],
            &[("CONTINUOUS_INTEGRATION", "true")],
            &[("CI", "true"), ("GITLAB_CI", "false")],
            &[("GITLAB_CI", "true"), ("GITHUB_ACTIONS", "true"), ("CI_COMMIT_REF_PROTECTED", "true")],
        ] {
            let ci = ci(variables);
            assert_eq!(ci.host, Host::Unknown, "{variables:?}");
            assert!(ci.secrets_problem().unwrap().contains("nothing was signed"));
            assert!(ci.tag_problem("v26.10.4").is_some());
            assert!(!ci.secrets_come_from_variables());
        }
    }
}
