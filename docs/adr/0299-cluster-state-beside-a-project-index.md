# ADR-0299: Cluster state beside a project index, with another plug-in's window values

- **Status:** Accepted (the owner, 2026-11-16)
- **Date:** 2026-11-16
- **Extends:**
  - [ADR-0289](0289-plugin-tool-windows.md): plug-in tool windows;
  - [ADR-0290](0290-tool-window-choices-and-confirmations.md): a tool window's header choices;
  - [ADR-0293](0293-gitops-project-index.md): the project index and its tool window;
  - [ADR-0296](0296-a-manifest-against-the-cluster.md): window values in run targets.

  All four stay as accepted; this record adds to them.

## Context

The GitOps window shows the repository's Flux and Argo CD objects as a graph (ADR-0293). A person
also wants to know what the cluster says of them: whether each is ready, synced and healthy, which
revision it applied, and why it failed. Three things are missing:
- **Which cluster.** The Kubernetes window already has the context the person chose (ADR-0290),
  but window values are read only within one plug-in's own window.
- **A listing for an index section.** An index section runs nothing; its rows are the index.
- **Showing it.** The index view colours nodes only by the repository's problems.

## Decision

1. **Another plug-in's window values.**
   - **The declaration:** a tool window may declare
     `"windowValues": [{"plugin": "kubernetes", "window": "kubernetes", "choice": "context"}]`.
     Its listings and actions then take `${window.kubernetes.context}` (`<window>.<choice>`) as a
     whole argument, as ADR-0289 and ADR-0296 allow.
   - **Read-only:** the value is the other window's current valid choice; nothing else can be read
     and nothing can change it. When the other window has not listed yet, its choices are worked out
     once in the background from the remembered values, as its own listing would.
   - **When it is missing:** if the other plug-in is not installed or is turned off, the listing
     says so; with no valid choice it says the choice is needed.
   - **A changed choice:** when the person chooses another value in the other window, rows listed
     under the old value are dropped at once, never shown under the new one, and the listing runs
     again. An action on those rows is refused until it has.
2. **A cluster state for an index section.**
   - **The declaration:** an index section (`"rows": "index"`) may declare
     `"state": {"columns": [...], "columnTitles": {...}, "lists": [ToolCommand, ...]}`.
   - **The listings:** each is a read-only command, run as section listings are: in a trusted
     project only, never while an action of the section runs, with `personOnly` checked. Its
     output goes to `zen_tool_rows("state:<section>", output)`. Each listing may fail alone, so a
     cluster without Argo CD still shows Flux.
   - **Matching:** a row whose `id` is a node's id gives that node its `cluster`: a `state` (`ok`,
     `progressing`, `error`, `unknown`) and one value per column. Columns without a title are there
     for actions only (`${row.namespace}`).
   - **When:** every 30 seconds while the window is visible, and at once with Refresh. After a
     failure of every listing, 2 minutes pass before the next unless the person presses Refresh.
   - **The answer:** `plugins.toolWindow.rows` on the section gives each node row its `cluster`, and
     a `cluster` object with `from` ("State from prod"), the values it was listed under, its
     columns, and the errors.
   - **Without a cluster:** the index rows and the graph stay; `errors` says why there is no state.
3. **Actions on a node.**
   - **What:** the section's row actions (ADR-0289) act on a node that has a cluster row.
   - **Row values:** taken only from rows the platform listed this session, as whole arguments,
     with the values that listing used.
4. **The index view.**
   - **Colour:** a node shows its cluster state as a dot, red, yellow or green. A repository
     problem keeps its border colour.
   - **Details:** a pane under the tree and the graph names where the state came from ("State from
     prod"). For the selected node it shows the columns and the row actions; when the cluster has no
     such object, it says so.
5. **The GitOps plug-in:**
   - **Listings:** `kubectl --context ${window.kubernetes.context} get <Flux kinds> --all-namespaces
     --output json`, and the same for `applications.argoproj.io`.
   - **Reading:** Flux's `Ready` condition, `lastAppliedRevision` or the artifact's revision, and
     the condition's message. Argo CD's sync and health status, its revision and its message.
   - **Show Events:** `kubectl --context … get events --namespace ${row.namespace}
     --field-selector ${row.selector}`, in the Run window.
   - **Nothing changes in a cluster.** Only `get` runs; kubectl's changing subcommands are
     person-only for the plug-in as for the Kubernetes plug-in. Reconcile and sync are work of
     their own.

## Consequences

- The GitOps window shows the repository and the cluster side by side, in the Kubernetes window's
  context, and an agent reads both through `plugins.toolWindow.rows`.
- Other plug-ins can lay a live state over their index (a Terraform plan over modules, a CI run
  over pipelines) and reuse another plug-in's choices without asking the person again.

## Alternatives

- **A context choice in the GitOps window:** two contexts the person would have to keep in step.
  Rejected.
- **One listing of every kind:** kubectl fails as a whole when one kind is not installed. Rejected.
- **A separate tool window for cluster state:** the person would match two lists by hand. Rejected.
