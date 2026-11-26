use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::branches::{
    BranchInfo, FastForwardBranch, FastForwardParams, MergeBranch, MergeParams, RebaseBranch, RebaseParams,
};
use super::repository;
use super::workspace::{
    GitWorkspace, Prompt, PromptKind, RefKind, RefTarget, Selection, WorkspaceState, after_conflicts, ref_target, state,
};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(DropMenu)?;
    registry.register(DropActionCommand)?;
    Ok(())
}

/// What a branch dropped on another can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DropAction {
    /// Fast-forward the branch dropped on to the dropped one (vcs.branch.fastForward): offered
    /// only when it is behind with nothing of its own.
    FastForward,
    /// Merge the dropped branch into the one it was dropped on (vcs.branch.merge with `target`).
    Merge,
    /// Rebase the dropped branch onto the one it was dropped on (vcs.branch.rebase with
    /// `branch`).
    Rebase,
    /// Interactive Rebase: the dropped branch's commits onto the one it was dropped on, in the
    /// interactive rebase editor (vcs.rebase.plan with `onto` and `branch`).
    InteractiveRebase,
}

/// An item of the drop menu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropItem {
    /// What it does.
    pub action: DropAction,
    /// Whether it can be chosen: a merge of what the branch already has, or a rebase onto what the
    /// dropped branch is already on top of, would do nothing.
    pub enabled: bool,
    /// The local branch that is checked out first, when it is not the current one: the user is
    /// asked first (for Interactive Rebase, Start in the editor checks it out).
    pub checkout: Option<String>,
}

/// The menu a branch dropped on another opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropMenuInfo {
    /// The branch (or tag) dragged.
    pub source: RefTarget,
    /// The branch it was dropped on.
    pub target: RefTarget,
    /// What it offers, in the menu's order.
    pub items: Vec<DropItem>,
}

pub(super) fn offers(
    session: &mut Session,
    source: &RefTarget,
    target: &RefTarget,
) -> Result<DropMenuInfo, CommandError> {
    if source.kind == RefKind::Head || target.kind == RefKind::Head {
        return Err(CommandError::new(t!("vcs-error-drop-nothing")));
    }
    let (source, from) = ref_target(session, source.name.trim(), source.kind)?;
    let (target, onto) = ref_target(session, target.name.trim(), target.kind)?;
    if source == target {
        return Err(CommandError::new(t!("vcs-error-drop-same")));
    }
    let repository = repository(session)?;
    let current = repository.head().ok().and_then(|head| head.branch);
    let contains = |ancestor: &str, descendant: &str| repository.is_ancestor(ancestor, descendant).unwrap_or(false);
    let checkout = |branch: &RefTarget| (current.as_deref() != Some(branch.name.as_str())).then(|| branch.name.clone());
    let mut items = Vec::new();
    if target.kind == RefKind::Local {
        if from != onto && contains(&onto, &from) {
            items.push(DropItem { action: DropAction::FastForward, enabled: true, checkout: None });
        }
        let enabled = !contains(&from, &onto);
        items.push(DropItem { action: DropAction::Merge, enabled, checkout: checkout(&target) });
    }
    if source.kind == RefKind::Local {
        let enabled = !contains(&onto, &from);
        let checkout = checkout(&source);
        items.push(DropItem { action: DropAction::Rebase, enabled, checkout: checkout.clone() });
        items.push(DropItem { action: DropAction::InteractiveRebase, enabled, checkout });
    }
    if items.is_empty() {
        return Err(CommandError::new(t!("vcs-error-drop-nothing")));
    }
    Ok(DropMenuInfo { source, target, items })
}

/// The branch dropped and the one it was dropped on.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DropMenuParams {
    /// The branch (or tag) dragged: `{ "name": "feature", "kind": "local" }` (`kind` is `local`
    /// unless given: `remote`, `tag`).
    pub source: Option<RefTarget>,
    /// The branch it is dropped on. The menu closes when either is missing.
    pub target: Option<RefTarget>,
}

pub struct DropMenu;

impl Command for DropMenu {
    type Params = DropMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.dropMenu";
    const TITLE: &'static str = "Drop Branch";
    const DESCRIPTION: &'static str = "Opens the menu that dropping the branch (or tag) `source` on the branch `target` opens in \
         the Git workspace, as a drag of a label in the graph or of a row of the left panel does: \
         Fast-forward `target` to `source` (only when `target` is behind with nothing of its own), Merge \
         `source` into `target` (a local target), Rebase `source` onto `target` and Interactive Rebase \
         (a local source; Interactive Rebase opens the interactive rebase editor). Each item says whether it applies \
         and which branch it checks out first. Without parameters the menu closes; \
         vcs.workspace.dropAction runs an item. Answers with the tab's state, `dropMenu` the menu.";

    fn run(&self, session: &mut Session, params: DropMenuParams) -> Result<WorkspaceState, CommandError> {
        let (Some(source), Some(target)) = (params.source, params.target) else {
            session.state_mut::<GitWorkspace>().drop_menu = None;
            return Ok(state(session));
        };
        let menu = offers(session, &source, &target)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.drop_menu = Some(menu);
        Ok(state(session))
    }
}

/// An item of the drop menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropActionParams {
    /// The branch (or tag) dragged.
    pub source: RefTarget,
    /// The branch it was dropped on.
    pub target: RefTarget,
    /// The item.
    pub action: DropAction,
    /// The user confirmed checking the other branch out first (the `checkoutAndMerge` and
    /// `checkoutAndRebase` dialogs do); without it such an item opens its dialog.
    #[serde(default)]
    pub confirmed: bool,
}

/// What an item of the drop menu did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DropActionResult {
    /// The item.
    pub action: DropAction,
    /// The branch dragged.
    pub source: RefTarget,
    /// The branch it was dropped on.
    pub target: RefTarget,
    /// The dialog the item opened (`checkoutAndMerge`, `checkoutAndRebase`), where the user goes on;
    /// `get_window_state` lists it among the popups.
    pub dialog: Option<String>,
    /// Where HEAD is after the operation, when it moved: that commit is selected.
    pub head: Option<String>,
    /// The branch dropped on after a fast-forward.
    pub branch: Option<BranchInfo>,
}

pub struct DropActionCommand;

impl Command for DropActionCommand {
    type Params = DropActionParams;
    type Output = DropActionResult;
    const ID: &'static str = "vcs.workspace.dropAction";
    const TITLE: &'static str = "Drop Branch Item";
    const DESCRIPTION: &'static str = "Runs an item of the menu of the branch `source` dropped on the branch `target` (see \
         vcs.workspace.dropMenu) as a click on it does, and closes the menu: fastForward (vcs.branch.fastForward \
         of `target` to `source`), merge (vcs.branch.merge of `source` into `target`), rebase \
         (vcs.branch.rebase of `source` onto `target`). An item that checks another branch out first opens \
         the `checkoutAndMerge` or `checkoutAndRebase` dialog unless `confirmed`; it is refused while \
         tracked files have uncommitted changes. interactiveRebase opens the interactive rebase editor \
         for `source` onto `target` (`dialog` is `interactiveRebase`; nothing runs until \
         vcs.rebase.start, which checks `source` out first). An item that does not apply is refused. \
         The new HEAD is selected; a merge or a rebase that stops on conflicts fails with the code \
         `conflicts` and selects the work in progress.";

    fn run(&self, session: &mut Session, params: DropActionParams) -> Result<DropActionResult, CommandError> {
        let menu = offers(session, &params.source, &params.target)?;
        session.state_mut::<GitWorkspace>().close_menu();
        let item = menu.items.iter().find(|item| item.action == params.action);
        let Some(item) = item.filter(|item| item.enabled) else {
            return Err(CommandError::new(t!("vcs-error-drop-action")));
        };
        let DropMenuInfo { source, target, .. } = menu.clone();
        let mut result = DropActionResult {
            action: params.action,
            source: source.clone(),
            target: target.clone(),
            dialog: None,
            head: None,
            branch: None,
        };
        if params.action == DropAction::InteractiveRebase {
            let params = super::rebase::PlanParams {
                onto: Some(target.name.clone()),
                branch: Some(source.name.clone()),
                ..Default::default()
            };
            super::rebase::PlanRebase.run(session, params)?;
            result.dialog = Some(super::workspace::INTERACTIVE_REBASE.to_owned());
            return Ok(result);
        }
        if item.checkout.is_some() && !params.confirmed {
            let kind = if params.action == DropAction::Merge {
                PromptKind::CheckoutAndMerge
            } else {
                PromptKind::CheckoutAndRebase
            };
            let (_, id) = ref_target(session, &target.name, target.kind)?;
            let prompt = Prompt { kind, id, branch: item.checkout.clone(), drop: Some((source, target)) };
            session.state_mut::<GitWorkspace>().prompt = Some(prompt);
            result.dialog = Some(kind.dialog().to_owned());
            return Ok(result);
        }
        let moved: Result<bool, CommandError> = match params.action {
            DropAction::FastForward => {
                let params = FastForwardParams { name: target.name.clone(), onto: Some(source.name.clone()) };
                FastForwardBranch.run(session, params).map(|branch| {
                    let current = branch.current;
                    result.branch = Some(branch);
                    current
                })
            }
            DropAction::Merge => {
                let params = MergeParams { name: source.name.clone(), target: Some(target.name.clone()) };
                MergeBranch.run(session, params).map(|()| true)
            }
            DropAction::Rebase => {
                let params = RebaseParams { onto: target.name.clone(), branch: Some(source.name.clone()) };
                RebaseBranch.run(session, params).map(|_| true)
            }
            DropAction::InteractiveRebase => unreachable!("opens the editor above"),
        };
        match moved {
            Ok(true) => {
                let head = repository(session)?.head().ok().and_then(|head| head.commit).unwrap_or_default();
                session.state_mut::<GitWorkspace>().select(Selection::Commit(head.clone()));
                result.head = Some(head);
                Ok(result)
            }
            Ok(false) => Ok(result),
            Err(error) => Err(after_conflicts(session, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn branches() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["tag", "v1"]);
        git(&root, &["branch", "behind"]);
        git(&root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(&root, &["add", "f.txt"]);
        git(&root, &["commit", "-q", "-m", "Feature"]);
        git(&root, &["update-ref", "refs/remotes/origin/feature", "HEAD"]);
        git(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("m.txt"), "main\n").unwrap();
        git(&root, &["add", "m.txt"]);
        git(&root, &["commit", "-q", "-m", "Second"]);
        (dir, root)
    }

    fn local(name: &str) -> serde_json::Value {
        json!({ "name": name, "kind": "local" })
    }

    #[test]
    fn a_drop_offers_what_applies() {
        let (_dir, root) = branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        let items = |state: &serde_json::Value| -> Vec<(String, bool, Option<String>)> {
            state["dropMenu"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    let checkout = item["checkout"].as_str().map(str::to_owned);
                    (item["action"].as_str().unwrap().to_owned(), item["enabled"] == json!(true), checkout)
                })
                .collect()
        };
        let entry = |action: &str, enabled: bool, checkout: Option<&str>| {
            (action.to_owned(), enabled, checkout.map(str::to_owned))
        };
        run(s, "vcs.workspace.open", json!({})).unwrap();
        run(s, "vcs.workspace.refMenu", json!({ "name": "feature" })).unwrap();

        let state = run(s, "vcs.workspace.dropMenu", json!({ "source": local("feature"), "target": local("main") }));
        let state = state.unwrap();
        assert_eq!(state["refMenu"], json!(null), "one menu at a time");
        assert_eq!(state["dropMenu"]["source"], local("feature"));
        assert_eq!(
            items(&state),
            [
                entry("merge", true, None),
                entry("rebase", true, Some("feature")),
                entry("interactiveRebase", true, Some("feature")),
            ]
        );
        let state =
            run(s, "vcs.workspace.dropMenu", json!({ "source": { "name": "main" }, "target": local("behind") }));
        let state = state.unwrap();
        assert_eq!(
            items(&state),
            [
                entry("fastForward", true, None),
                entry("merge", true, Some("behind")),
                entry("rebase", false, None),
                entry("interactiveRebase", false, None),
            ],
            "main is already on top of behind"
        );
        let remote = json!({ "name": "origin/feature", "kind": "remote" });
        let state = run(s, "vcs.workspace.dropMenu", json!({ "source": remote, "target": local("feature") })).unwrap();
        assert_eq!(items(&state), [entry("merge", false, Some("feature"))], "feature has it already");
        let tag = json!({ "name": "v1", "kind": "tag" });
        let state = run(s, "vcs.workspace.dropMenu", json!({ "source": local("main"), "target": tag })).unwrap();
        assert_eq!(items(&state), [entry("rebase", false, None), entry("interactiveRebase", false, None)]);

        let opened = run(
            s,
            "vcs.workspace.dropAction",
            json!({ "source": local("feature"), "target": local("main"), "action": "interactiveRebase" }),
        )
        .unwrap();
        assert_eq!(opened["dialog"], "interactiveRebase");
        let rebase = run(s, "vcs.rebase.state", json!({})).unwrap();
        assert_eq!(rebase["plan"]["branch"], "feature");
        assert_eq!(rebase["plan"]["checkout"], json!(true));
        assert_eq!(rebase["plan"]["ontoLabel"], "main");
        assert_eq!(rebase["plan"]["steps"][0]["subject"], "Feature");
        assert_eq!(git(&root, &["branch", "--show-current"]).trim(), "main", "nothing checked out yet");
        run(s, "vcs.rebase.cancel", json!({})).unwrap();

        for (source, target) in [
            (local("main"), local("main")),
            (remote.clone(), json!({ "name": "v1", "kind": "tag" })),
            (local("nope"), local("main")),
            (local("main"), json!({ "name": "HEAD", "kind": "head" })),
        ] {
            assert!(run(s, "vcs.workspace.dropMenu", json!({ "source": source, "target": target })).is_err());
        }
        let state = run(s, "vcs.workspace.dropMenu", json!({})).unwrap();
        assert_eq!(state["dropMenu"], json!(null));
    }

    #[test]
    fn drop_items_fast_forward_merge_and_rebase_asking_before_a_checkout() {
        let (_dir, root) = branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        let rev = |revision: &str| git(&root, &["rev-parse", revision]).trim().to_owned();
        let current = || git(&root, &["branch", "--show-current"]).trim().to_owned();
        run(s, "vcs.workspace.open", json!({})).unwrap();
        let act = |source: &str, target: &str, action: &str, confirmed: bool| json!({ "source": local(source), "target": local(target), "action": action, "confirmed": confirmed });

        run(s, "vcs.workspace.dropMenu", json!({ "source": local("main"), "target": local("behind") })).unwrap();
        let done = run(s, "vcs.workspace.dropAction", act("main", "behind", "fastForward", false)).unwrap();
        assert_eq!(done["branch"]["commit"], rev("main"));
        assert_eq!(done["head"], json!(null), "HEAD did not move");
        assert_eq!(rev("behind"), rev("main"));
        assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["dropMenu"], json!(null), "the menu closed");

        let done = run(s, "vcs.workspace.dropAction", act("feature", "main", "merge", false)).unwrap();
        assert_eq!(done["head"], rev("HEAD"));
        assert_eq!(rev("HEAD^2"), rev("feature"), "a merge commit");
        assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["selected"], rev("HEAD"));
        assert!(run(s, "vcs.workspace.dropAction", act("feature", "main", "merge", false)).is_err());
        assert!(run(s, "vcs.workspace.dropAction", act("feature", "main", "interactiveRebase", true)).is_err());

        git(&root, &["switch", "-q", "-c", "topic", "v1"]);
        std::fs::write(root.join("t.txt"), "topic\n").unwrap();
        git(&root, &["add", "t.txt"]);
        git(&root, &["commit", "-q", "-m", "Topic"]);
        git(&root, &["switch", "-q", "main"]);
        let asked = run(s, "vcs.workspace.dropAction", act("topic", "feature", "rebase", false)).unwrap();
        assert_eq!(asked["dialog"], "checkoutAndRebase");
        assert_eq!(current(), "main", "nothing happened yet");
        let prompt = s.state_mut::<GitWorkspace>().take_prompt().unwrap();
        assert_eq!(prompt.kind, PromptKind::CheckoutAndRebase);
        assert_eq!(prompt.branch.as_deref(), Some("topic"));
        let (from, onto) = prompt.drop.unwrap();
        assert_eq!((from.name.as_str(), onto.name.as_str()), ("topic", "feature"));
        let done = run(s, "vcs.workspace.dropAction", act("topic", "feature", "rebase", true)).unwrap();
        assert_eq!(current(), "topic");
        assert_eq!(done["head"], rev("topic"));
        assert_eq!(rev("topic~1"), rev("feature"));

        let asked = run(s, "vcs.workspace.dropAction", act("main", "feature", "merge", false)).unwrap();
        assert_eq!(asked["dialog"], "checkoutAndMerge");
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        assert!(run(s, "vcs.workspace.dropAction", act("main", "feature", "merge", true)).is_err());
        assert_eq!(current(), "topic");
        git(&root, &["checkout", "-q", "--", "a.txt"]);
        run(s, "vcs.workspace.dropAction", act("main", "feature", "merge", true)).unwrap();
        assert_eq!(current(), "feature");
        assert_eq!(git(&root, &["merge-base", "--is-ancestor", "main", "feature"]), "");
    }

    #[test]
    fn a_drop_that_stops_on_conflicts_selects_the_work_in_progress() {
        let (_dir, root) = branches();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        git(&root, &["switch", "-q", "-c", "other", "v1"]);
        std::fs::write(root.join("m.txt"), "other\n").unwrap();
        git(&root, &["add", "m.txt"]);
        git(&root, &["commit", "-q", "-m", "Other"]);
        git(&root, &["switch", "-q", "main"]);
        registry.dispatch(s, "vcs.workspace.open", json!({})).unwrap();
        let params = json!({ "source": local("other"), "target": local("main"), "action": "merge" });
        let failed = registry.dispatch(s, "vcs.workspace.dropAction", params).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS));
        assert_eq!(registry.dispatch(s, "vcs.workspace.state", json!({})).unwrap()["wip"], json!(true));
        assert!(s.state_mut::<GitWorkspace>().take_conflicts().is_some(), "the window explains it");
        git(&root, &["merge", "--abort"]);
    }
}
