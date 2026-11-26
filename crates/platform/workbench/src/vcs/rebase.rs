use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::history::{self, Done, Operation};
use super::workspace::GitWorkspace;
use super::{CONFLICTS, changed, command_error, message, repository};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(PlanRebase)?;
    registry.register(EditRebase)?;
    registry.register(ResetRebase)?;
    registry.register(CancelRebase)?;
    registry.register(StartRebase)?;
    registry.register(ContinueRebase)?;
    registry.register(AbortRebase)?;
    registry.register(RebaseStateCommand)?;
    Ok(())
}

pub const MAX_STEPS: usize = 500;

/// What a step does with its commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RebaseAction {
    /// Keep the commit (key P).
    #[default]
    Pick,
    /// Keep its changes with a new message (key R).
    Reword,
    /// Meld it into the commit before it, joining their messages (key S).
    Squash,
    /// Meld it into the commit before it, keeping only that commit's message (key F).
    Fixup,
    /// Leave it out (key D).
    Drop,
}

impl From<RebaseAction> for zen_vcs::RebaseAction {
    fn from(action: RebaseAction) -> Self {
        match action {
            RebaseAction::Pick => zen_vcs::RebaseAction::Pick,
            RebaseAction::Reword => zen_vcs::RebaseAction::Reword,
            RebaseAction::Squash => zen_vcs::RebaseAction::Squash,
            RebaseAction::Fixup => zen_vcs::RebaseAction::Fixup,
            RebaseAction::Drop => zen_vcs::RebaseAction::Drop,
        }
    }
}

impl RebaseAction {
    pub fn from_key(key: char) -> Option<Self> {
        match key.to_ascii_lowercase() {
            'p' => Some(RebaseAction::Pick),
            'r' => Some(RebaseAction::Reword),
            's' => Some(RebaseAction::Squash),
            'f' => Some(RebaseAction::Fixup),
            'd' => Some(RebaseAction::Drop),
            _ => None,
        }
    }

    pub fn melds(self) -> bool {
        matches!(self, RebaseAction::Squash | RebaseAction::Fixup)
    }
}

/// What the editor does: rewrite a branch, or cherry-pick commits onto `HEAD`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RebaseKind {
    /// An interactive rebase of a branch onto another, or of the current branch from a commit.
    #[default]
    Rebase,
    /// Several commits cherry-picked onto `HEAD`.
    CherryPick,
}

/// One commit of a plan and what is to be done with it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RebaseStepInfo {
    /// The commit's full id.
    pub id: String,
    /// Its first seven characters.
    pub short_id: String,
    /// The first line of its message.
    pub subject: String,
    /// The rest of its message.
    pub body: String,
    /// Its author.
    pub author: String,
    /// When it was authored, in seconds since 1970 (UTC).
    pub time: i64,
    /// The author's time zone, in seconds east of UTC.
    pub offset: i32,
    /// What is done with it.
    pub action: RebaseAction,
    /// For `reword`, the new message (subject, a blank line, the body); its own until changed.
    pub message: Option<String>,
}

impl RebaseStepInfo {
    fn new(commit: zen_vcs::Commit) -> Self {
        RebaseStepInfo {
            short_id: commit.id.chars().take(7).collect(),
            subject: commit.subject.clone(),
            body: commit.body.clone(),
            author: commit.author.clone(),
            time: commit.time,
            offset: commit.offset,
            action: RebaseAction::Pick,
            message: None,
            id: commit.id,
        }
    }

    pub fn new_message(&self) -> String {
        match (&self.action, &self.message) {
            (RebaseAction::Reword, Some(message)) => message.clone(),
            _ => own_message(&self.subject, &self.body),
        }
    }
}

fn own_message(subject: &str, body: &str) -> String {
    if body.is_empty() { subject.to_owned() } else { format!("{subject}\n\n{body}") }
}

/// An interactive rebase (or a cherry-pick of several commits) as the editor shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RebasePlan {
    /// A rebase or a cherry-pick.
    pub kind: RebaseKind,
    /// The branch whose commits are rewritten (or that the commits are picked onto); `None` for a
    /// detached HEAD.
    pub branch: Option<String>,
    /// Whether `branch` is not the current branch and is checked out first.
    pub checkout: bool,
    /// Where the commits go: a full id.
    pub onto: String,
    /// What `onto` is called: the branch or tag it was given as, or the commit's short id.
    pub onto_label: String,
    /// The commit whose descendants are rewritten (`onto` too, except for a branch rebased onto
    /// another, where it is the same).
    pub upstream: String,
    /// The commits, in the order git applies them: the oldest first. The editor shows them the
    /// other way round, the newest on top, as the graph does; a squash or a fixup melds a commit
    /// into the one before it in this order (the one under it in the editor).
    pub steps: Vec<RebaseStepInfo>,
}

impl RebasePlan {
    pub fn problem(&self) -> Option<String> {
        let mut kept = false;
        for step in &self.steps {
            if step.action.melds() && !kept {
                return Some(t!("vcs-rebase-problem-meld", commit = step.short_id.as_str()));
            }
            if step.action == RebaseAction::Reword && step.message.as_deref().is_some_and(|m| m.trim().is_empty()) {
                return Some(t!("vcs-rebase-problem-message", commit = step.short_id.as_str()));
            }
            kept |= step.action != RebaseAction::Drop;
        }
        if self.kind == RebaseKind::CherryPick && !kept {
            return Some(t!("vcs-rebase-problem-nothing"));
        }
        None
    }

    fn script(&self) -> zen_vcs::RebaseScript {
        zen_vcs::RebaseScript {
            upstream: self.upstream.clone(),
            onto: self.onto.clone(),
            branch: self.branch.clone().filter(|_| self.checkout),
            steps: self
                .steps
                .iter()
                .map(|step| zen_vcs::RebaseStep {
                    action: step.action.into(),
                    commit: step.id.clone(),
                    message: (step.action == RebaseAction::Reword).then(|| step.new_message()),
                })
                .collect(),
        }
    }

    fn find(&self, index: Option<usize>, id: Option<&str>) -> Result<usize, CommandError> {
        let found = match (index, id.map(str::trim).filter(|id| !id.is_empty())) {
            (Some(index), _) => (index < self.steps.len()).then_some(index),
            (None, Some(id)) => self.steps.iter().position(|step| step.id.starts_with(id)),
            (None, None) => None,
        };
        found.ok_or_else(|| CommandError::new(t!("vcs-rebase-error-no-step")))
    }
}

#[derive(Default)]
pub struct RebaseEditor {
    plan: Option<RebasePlan>,
    original: Option<RebasePlan>,
    raised: u64,
    revision: u64,
}

impl RebaseEditor {
    pub fn plan(&self) -> Option<&RebasePlan> {
        self.plan.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.plan.is_some()
    }

    pub fn raised(&self) -> u64 {
        self.raised
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn changed(&self) -> bool {
        self.plan != self.original
    }

    fn open(&mut self, plan: RebasePlan) {
        self.original = Some(plan.clone());
        self.plan = Some(plan);
        self.raised += 1;
        self.revision += 1;
    }

    pub(crate) fn close(&mut self) {
        self.plan = None;
        self.original = None;
        self.revision += 1;
    }
}

/// What to rebase, or what to cherry-pick: one of `onto` (with `branch`), `from` or `cherryPick`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanParams {
    /// Rebase onto this branch, tag or commit: the commits of `branch` it does not have.
    pub onto: Option<String>,
    /// The local branch to rebase onto `onto`; the current branch when absent (checked out first
    /// when it is another one).
    pub branch: Option<String>,
    /// Or a commit of the current branch: the commits after it are rewritten, on top of it.
    pub from: Option<String>,
    /// Or commits to cherry-pick onto HEAD (ids, branches, tags), applied the oldest first.
    pub cherry_pick: Vec<String>,
}

pub struct PlanRebase;

impl Command for PlanRebase {
    type Params = PlanParams;
    type Output = RebasePlan;
    const ID: &'static str = "vcs.rebase.plan";
    const TITLE: &'static str = "Interactive Rebase";
    const DESCRIPTION: &'static str = "Opens the interactive rebase editor tab with a plan and answers with it: with `onto`, the \
         commits of `branch` (the current branch when absent) that `onto` does not have; with `from`, \
         the commits of the current branch after that commit; with `cherryPick`, those commits, to be \
         picked onto HEAD. Every step starts as pick; `steps` are in the order git applies them, the \
         oldest first (the editor shows the newest on top). Nothing changes until vcs.rebase.start; \
         vcs.rebase.edit changes a step, vcs.rebase.cancel closes the editor. Refused while an \
         operation is in progress or when there is nothing to rebase.";

    fn run(&self, session: &mut Session, params: PlanParams) -> Result<RebasePlan, CommandError> {
        let repository = repository(session)?;
        if let Some(operation) = repository.in_progress() {
            return Err(CommandError::new(t!("vcs-rebase-error-in-progress", operation = operation.as_str())));
        }
        let failed = |error: zen_vcs::VcsError| CommandError::new(message(&error));
        let head = repository.head().map_err(failed)?;
        let head_id = head.commit.clone().ok_or_else(|| CommandError::new(t!("vcs-rebase-error-nothing")))?;
        let short = |id: &str| id.chars().take(7).collect::<String>();
        let trimmed =
            |text: &Option<String>| text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned);
        let picks: Vec<String> =
            params.cherry_pick.iter().map(|id| id.trim().to_owned()).filter(|id| !id.is_empty()).collect();
        let plan = if !picks.is_empty() {
            let mut commits: Vec<zen_vcs::Commit> = Vec::new();
            for revision in &picks {
                let id = repository.resolve(revision).map_err(failed)?;
                if commits.iter().any(|commit| commit.id == id) {
                    continue;
                }
                if repository.is_ancestor(&id, "HEAD").unwrap_or(false) {
                    return Err(CommandError::new(t!("vcs-rebase-error-on-branch", commit = short(&id))));
                }
                let (commit, _) = repository.commit_details(&id).map_err(failed)?;
                if commit.parents.len() > 1 {
                    return Err(CommandError::new(t!("vcs-rebase-error-merge", commit = short(&id))));
                }
                commits.push(commit);
            }
            let ids: Vec<String> = commits.iter().map(|commit| commit.id.clone()).collect();
            let order = repository.oldest_first(&ids).map_err(failed)?;
            commits.sort_by_key(|commit| order.iter().position(|id| *id == commit.id));
            RebasePlan {
                kind: RebaseKind::CherryPick,
                branch: head.branch.clone(),
                checkout: false,
                onto: head_id.clone(),
                onto_label: head.branch.clone().unwrap_or_else(|| short(&head_id)),
                upstream: head_id,
                steps: commits.into_iter().map(RebaseStepInfo::new).collect(),
            }
        } else if let Some(from) = trimmed(&params.from) {
            let id = repository.resolve(&from).map_err(failed)?;
            if id == head_id || !repository.is_ancestor(&id, "HEAD").unwrap_or(false) {
                return Err(CommandError::new(t!("vcs-rebase-error-from", commit = short(&id))));
            }
            let commits = repository.rebase_commits(&id, "HEAD").map_err(failed)?;
            RebasePlan {
                kind: RebaseKind::Rebase,
                branch: head.branch.clone(),
                checkout: false,
                onto: id.clone(),
                onto_label: short(&id),
                upstream: id,
                steps: commits.into_iter().map(RebaseStepInfo::new).collect(),
            }
        } else if let Some(onto) = trimmed(&params.onto) {
            let onto_id = repository.resolve(&onto).map_err(failed)?;
            let branch = trimmed(&params.branch);
            let checkout = branch.as_ref().is_some_and(|branch| head.branch.as_ref() != Some(branch));
            let branch = branch.or_else(|| head.branch.clone());
            if checkout {
                let name = branch.clone().unwrap_or_default();
                if !repository.branches().map_err(failed)?.iter().any(|b| !b.remote && b.name == name) {
                    return Err(CommandError::new(t!("vcs-error-no-branch", branch = name.as_str())));
                }
            }
            let tip = if checkout { branch.clone().unwrap_or_default() } else { "HEAD".to_owned() };
            let commits = repository.rebase_commits(&onto_id, &tip).map_err(failed)?;
            RebasePlan {
                kind: RebaseKind::Rebase,
                branch,
                checkout,
                onto: onto_id.clone(),
                onto_label: if onto_id.starts_with(&onto) { short(&onto_id) } else { onto },
                upstream: onto_id,
                steps: commits.into_iter().map(RebaseStepInfo::new).collect(),
            }
        } else {
            return Err(CommandError::new(t!("vcs-rebase-error-what")));
        };
        if plan.steps.is_empty() {
            let branch = plan.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
            return Err(CommandError::new(t!(
                "vcs-rebase-error-no-commits",
                branch = branch.as_str(),
                onto = plan.onto_label.as_str()
            )));
        }
        if plan.steps.len() > MAX_STEPS {
            return Err(CommandError::new(t!(
                "vcs-rebase-error-too-many",
                count = plan.steps.len(),
                limit = MAX_STEPS
            )));
        }
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.clear_picked();
        session.state_mut::<RebaseEditor>().open(plan.clone());
        Ok(plan)
    }
}

/// A change of one step of the plan.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EditParams {
    /// The step: its place in `steps` (from 0, the oldest first)...
    pub index: Option<usize>,
    /// ...or its commit (the full id or its start).
    pub id: Option<String>,
    /// What to do with it.
    pub action: Option<RebaseAction>,
    /// The new message of a reworded commit (the step becomes `reword`).
    pub message: Option<String>,
    /// Where to move it in `steps` (from 0, the oldest first): ⌥↑ in the editor is one later, ⌥↓
    /// one earlier.
    pub move_to: Option<usize>,
}

pub struct EditRebase;

impl Command for EditRebase {
    type Params = EditParams;
    type Output = RebasePlan;
    const ID: &'static str = "vcs.rebase.edit";
    const TITLE: &'static str = "Edit Interactive Rebase Step";
    const DESCRIPTION: &'static str = "Changes one step of the open interactive rebase plan, named by `index` (in `steps`, the \
         oldest first) or `id`: `action` (pick, reword, squash, fixup, drop: the keys P, R, S, F, D in \
         the editor), `message` (the new message of a reworded commit; the step becomes reword) and \
         `moveTo` (its new place in `steps`, as dragging it or ⌥↑/⌥↓ does). Answers with the plan; \
         nothing runs until vcs.rebase.start.";

    fn run(&self, session: &mut Session, params: EditParams) -> Result<RebasePlan, CommandError> {
        let editor = session.state_mut::<RebaseEditor>();
        let Some(plan) = editor.plan.as_mut() else { return Err(CommandError::new(t!("vcs-rebase-error-closed"))) };
        let index = plan.find(params.index, params.id.as_deref())?;
        if let Some(action) = params.action {
            let step = &mut plan.steps[index];
            step.action = action;
            if action == RebaseAction::Reword && step.message.is_none() {
                step.message = Some(own_message(&step.subject, &step.body));
            }
        }
        if let Some(message) = params.message {
            let step = &mut plan.steps[index];
            step.action = RebaseAction::Reword;
            step.message = Some(message);
        }
        if let Some(to) = params.move_to {
            let to = to.min(plan.steps.len() - 1);
            let step = plan.steps.remove(index);
            plan.steps.insert(to, step);
        }
        let plan = plan.clone();
        editor.revision += 1;
        Ok(plan)
    }
}

pub struct ResetRebase;

impl Command for ResetRebase {
    type Params = NoParams;
    type Output = RebasePlan;
    const ID: &'static str = "vcs.rebase.reset";
    const TITLE: &'static str = "Reset Interactive Rebase";
    const DESCRIPTION: &'static str = "Starts the open interactive rebase plan over: every commit picked, in its own order, with \
         its own message. Answers with the plan.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RebasePlan, CommandError> {
        let editor = session.state_mut::<RebaseEditor>();
        let Some(original) = editor.original.clone() else {
            return Err(CommandError::new(t!("vcs-rebase-error-closed")));
        };
        editor.plan = Some(original.clone());
        editor.revision += 1;
        Ok(original)
    }
}

pub struct CancelRebase;

impl Command for CancelRebase {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "vcs.rebase.cancel";
    const TITLE: &'static str = "Cancel Interactive Rebase";
    const DESCRIPTION: &'static str = "Closes the interactive rebase editor without running it: the repository stays exactly as \
         it was. (A rebase already stopped on conflicts is aborted with vcs.rebase.abort.)";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        session.state_mut::<RebaseEditor>().close();
        Ok(())
    }
}

/// A step given to `vcs.rebase.start`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartStep {
    /// The commit: its full id or its start; it must be one of the plan's.
    pub id: String,
    /// What to do with it.
    pub action: RebaseAction,
    /// The new message, for `reword`.
    pub message: Option<String>,
}

/// What to run.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartParams {
    /// The whole todo list at once, the oldest first, instead of the plan's steps as they are: the
    /// plan's commits left out are dropped.
    pub steps: Option<Vec<StartStep>>,
}

/// What a rebase did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RebaseOutcome {
    /// The commit HEAD is at now.
    pub head: String,
    /// The branch HEAD is on; `None` when detached.
    pub branch: Option<String>,
}

pub struct StartRebase;

impl Command for StartRebase {
    type Params = StartParams;
    type Output = RebaseOutcome;
    const ID: &'static str = "vcs.rebase.start";
    const TITLE: &'static str = "Start Interactive Rebase";
    const DESCRIPTION: &'static str = "Runs the open interactive rebase plan (or `steps`, given at once) with git, without any \
         editor: picks, rewords, squashes, fixups and drops, in the plan's order; a cherry-pick plan \
         picks its commits onto HEAD. Refused while tracked files have uncommitted changes or the plan \
         has a problem (a squash with no commit before it). The editor closes and the new HEAD is \
         selected in the Git workspace; Undo takes it back. On conflicts the rebase stops as git \
         leaves it and the command fails with the code `conflicts`: the work in progress panel lists \
         the conflicted files, and vcs.rebase.continue (once they are resolved and staged) or \
         vcs.rebase.abort go on.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<RebaseOutcome, CommandError> {
        let Some(mut plan) = session.state::<RebaseEditor>().and_then(RebaseEditor::plan).cloned() else {
            return Err(CommandError::new(t!("vcs-rebase-error-closed")));
        };
        if let Some(steps) = params.steps {
            let mut ordered = Vec::new();
            for given in &steps {
                let index = plan.find(None, Some(given.id.as_str()))?;
                let mut step = plan.steps[index].clone();
                if ordered.iter().any(|known: &RebaseStepInfo| known.id == step.id) {
                    return Err(CommandError::new(t!("vcs-rebase-error-twice", commit = step.short_id.as_str())));
                }
                step.action = given.action;
                step.message = match given.action {
                    RebaseAction::Reword => {
                        Some(given.message.clone().unwrap_or_else(|| own_message(&step.subject, &step.body)))
                    }
                    _ => None,
                };
                ordered.push(step);
            }
            for step in &plan.steps {
                if !ordered.iter().any(|known| known.id == step.id) {
                    ordered.push(RebaseStepInfo { action: RebaseAction::Drop, ..step.clone() });
                }
            }
            plan.steps = ordered;
            let editor = session.state_mut::<RebaseEditor>();
            editor.plan = Some(plan.clone());
            editor.revision += 1;
        }
        if let Some(problem) = plan.problem() {
            return Err(CommandError::new(problem));
        }
        let repository = repository(session)?;
        if repository.has_tracked_changes().map_err(|error| command_error(&error))? {
            return Err(CommandError::new(t!("vcs-rebase-error-changes")));
        }
        let done = match plan.kind {
            RebaseKind::Rebase => {
                let branch = plan.branch.clone().unwrap_or_else(|| plan.onto_label.clone());
                Done::new(Operation::InteractiveRebase, plan.onto_label.clone()).target(branch)
            }
            RebaseKind::CherryPick => {
                let picked = plan.steps.iter().filter(|step| step.action != RebaseAction::Drop).count();
                let first = plan.steps.iter().find(|step| step.action != RebaseAction::Drop);
                let subject = first.map(|step| step.subject.clone()).unwrap_or_default();
                Done::new(Operation::CherryPickMany, subject).count(picked)
            }
        };
        for step in plan.steps.iter().filter(|step| step.action != RebaseAction::Drop) {
            crate::hidden_files::guard_commit(session, &repository, &step.id)?;
        }
        if plan.kind == RebaseKind::Rebase {
            crate::hidden_files::guard_revision(session, &repository, &plan.onto)?;
        }
        let recording = history::begin(session, &repository);
        let result = repository.interactive_rebase(&plan.script());
        session.state_mut::<RebaseEditor>().close();
        match result {
            Ok(_) => history::finish(session, &repository, recording, done),
            Err(zen_vcs::VcsError::Conflicts { .. }) => history::keep_pending(session, &repository, recording, done),
            Err(_) => {}
        }
        outcome(session, &repository, result)
    }
}

pub struct ContinueRebase;

impl Command for ContinueRebase {
    type Params = NoParams;
    type Output = RebaseOutcome;
    const ID: &'static str = "vcs.rebase.continue";
    const TITLE: &'static str = "Continue Rebase";
    const DESCRIPTION: &'static str = "Goes on with the rebase that stopped on conflicts (an interactive one, a cherry-pick of \
         several commits or a plain rebase) once the conflicted files are resolved and staged: git \
         commits the step with its message (a reworded one keeps its new message) and goes on. Fails \
         with the code `conflicts` while files are still conflicted or when the next step conflicts. \
         Once it is over, the new HEAD is selected and Undo takes the whole rebase back.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RebaseOutcome, CommandError> {
        let repository = repository(session)?;
        let result = repository.rebase_continue();
        if result.is_ok() {
            history::finish_pending(session, &repository, false);
        }
        outcome(session, &repository, result)
    }
}

pub struct AbortRebase;

impl Command for AbortRebase {
    type Params = NoParams;
    type Output = RebaseOutcome;
    const ID: &'static str = "vcs.rebase.abort";
    const TITLE: &'static str = "Abort Rebase";
    const DESCRIPTION: &'static str = "Aborts the rebase in progress (git rebase --abort): the branch, the index and the files go \
         back to where they were before it started. Answers with HEAD.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RebaseOutcome, CommandError> {
        let repository = repository(session)?;
        let result = repository.rebase_abort().map(|()| String::new());
        if result.is_ok() {
            history::finish_pending(session, &repository, true);
        }
        outcome(session, &repository, result)
    }
}

fn outcome(
    session: &mut Session,
    repository: &zen_vcs::Repository,
    result: Result<String, zen_vcs::VcsError>,
) -> Result<RebaseOutcome, CommandError> {
    changed(session);
    match result {
        Ok(_) => {
            let head = repository.head().map_err(|error| command_error(&error))?;
            let id = head.commit.clone().unwrap_or_default();
            session.state_mut::<GitWorkspace>().select_commit(id.clone());
            Ok(RebaseOutcome { head: id, branch: head.branch })
        }
        Err(error) => {
            let error = command_error(&error);
            if error.code.as_deref() == Some(CONFLICTS) {
                session.state_mut::<GitWorkspace>().select_wip();
            }
            Err(error)
        }
    }
}

/// A rebase git is in the middle of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RebaseProgressInfo {
    /// The branch being rebased; `None` when it started on a detached HEAD.
    pub branch: Option<String>,
    /// Where its commits go.
    pub onto: Option<String>,
    /// The step it stopped at, from 1.
    pub step: usize,
    /// How many steps there are.
    pub total: usize,
    /// The commit whose step it stopped at.
    pub stopped_at: Option<String>,
    /// That commit's subject.
    pub stopped_subject: Option<String>,
    /// Whether it is an interactive rebase (or a cherry-pick of several commits).
    pub interactive: bool,
    /// The files with unresolved conflicts, relative, with `/`: Continue waits until there are none.
    pub conflicts: Vec<String>,
}

/// The interactive rebase editor and the rebase in progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RebaseState {
    /// Whether the editor tab is open.
    pub open: bool,
    /// Its plan.
    pub plan: Option<RebasePlan>,
    /// Whether the plan differs from the one made (Reset starts over).
    pub changed: bool,
    /// Why Start cannot run the plan as it is.
    pub problem: Option<String>,
    /// A rebase stopped on conflicts (or otherwise unfinished), as the work in progress panel shows it.
    pub in_progress: Option<RebaseProgressInfo>,
}

pub struct RebaseStateCommand;

impl Command for RebaseStateCommand {
    type Params = NoParams;
    type Output = RebaseState;
    const ID: &'static str = "vcs.rebase.state";
    const TITLE: &'static str = "Interactive Rebase State";
    const DESCRIPTION: &'static str = "Whether the interactive rebase editor is open and its plan (with what keeps Start from \
         running it), and the rebase git is in the middle of, if any: the branch, the step it stopped \
         at and the conflicted files, as the work in progress panel shows them with Continue and Abort.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RebaseState, CommandError> {
        let in_progress = match repository(session) {
            Ok(repository) => repository.rebase_progress().map(|progress| {
                let stopped_subject = progress
                    .stopped_at
                    .as_deref()
                    .and_then(|id| repository.commit_details(id).ok())
                    .map(|(commit, _)| commit.subject);
                RebaseProgressInfo {
                    branch: progress.branch,
                    onto: progress.onto,
                    step: progress.step,
                    total: progress.total,
                    stopped_at: progress.stopped_at,
                    stopped_subject,
                    interactive: progress.interactive,
                    conflicts: repository.conflicted_files(),
                }
            }),
            Err(_) => None,
        };
        let editor = session.state_mut::<RebaseEditor>();
        Ok(RebaseState {
            open: editor.is_open(),
            plan: editor.plan.clone(),
            changed: editor.changed(),
            problem: editor.plan.as_ref().and_then(RebasePlan::problem),
            in_progress,
        })
    }
}

#[cfg(test)]
mod tests;
