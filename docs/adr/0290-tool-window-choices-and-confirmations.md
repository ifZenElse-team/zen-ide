# ADR-0290: A plug-in tool window's header choices, and confirmations that name them

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Extends:** [ADR-0289](0289-plugin-tool-windows.md) (plug-in tool windows over declared programs),
  which stays as accepted; this record only adds to it
- **Builds on:** [ADR-0286](0286-plugin-run-targets.md) (declared programs, agent access per
  plug-in), [ADR-0033](0033-ai-access-to-every-api.md) (every action a command)

## Context

The Kubernetes tool window lists a cluster's pods, deployments, services and so on through
`kubectl`, as ADR-0289 lists Docker's containers through `docker`. A cluster brings a risk Docker on
one's own machine does not: the same command against the wrong cluster. `kubectl` acts on the
kubeconfig's current context unless told otherwise, and that context changes outside the IDE (a
`kubectl config use-context` in a terminal, another tool). With ADR-0289 alone:

- the window cannot say which context and namespace its rows come from;
- an action runs against whatever context the kubeconfig names when it starts, which may not be
  the one the rows were listed from;
- a `change` action (restart a rollout) runs without a question, and a `destroy` asks in words that
  cannot name the cluster;
- choosing a context or namespace would mean writing the kubeconfig (`use-context`,
  `set-context --namespace`), which the requirement forbids without asking.

## Decision

1. **Choices in the window's header.** A window may declare `"choices"`, an ordered list of what
   the person picks for the whole window:

   ```json
   "choices": [
     {"id": "context", "title": {"en": "Context", "ru": "Контекст"},
      "columns": ["name", "cluster", "user", "namespace"],
      "list": {"program": "kubectl", "args": ["config", "view", "-o", "json"]}},
     {"id": "namespace", "title": {"en": "Namespace", "ru": "Пространство имён"},
      "columns": ["name", "status", "created"],
      "list": {"program": "kubectl", "args": ["--context", "${window.context}", "get", "namespaces", "-o", "json"]},
      "initialFrom": "context.namespace"}
   ]
   ```

   A choice's options are the rows its `list` command gives, read by the plug-in's `zen_tool_rows`
   with the section id `choice:<id>`; the first column is shown. The first value is the
   remembered one; without one, the `initialFrom` column (`<choice>.<column>`) of an earlier
   choice's chosen row when it is among the options, else the row whose `state` is `current`,
   else the first row. The window shows each choice as a drop-down in its header.

2. **`${window.<id>}` arguments.** An argument that is exactly `${window.<id>}` becomes the
   choice's value, as one argument, in a section's `list`, a choice's `list` (of a choice declared
   before it) and an action's `args`; anywhere else in an argument it is refused, as `${row.` is.
   So the Kubernetes plug-in passes `--context ${window.context} --namespace ${window.namespace}`
   to every command, and the kubeconfig's own current context never decides where an action runs.

3. **The values are the window's, kept by the IDE.** The choices are window state: they start
   from the program's own current values (the kubeconfig's current context and its namespace) and
   the person changes them in the header. The IDE keeps them per project and window in its data
   folder (`plugin-tool-choices.json`), never in the project and never in the program's files.
   Choosing runs nothing of the program's: no `use-context`, no `set-context`.

4. **Choosing is the person's.** `plugins.toolWindow.choose { plugin, window, choice, value }`
   takes one of the options listed last and is refused to agents (`agentNotAllowed`). Choosing a
   value forgets the choices declared after it (they start again from their initial value), drops
   every row the window listed and any listing still running, and lists again.

5. **One snapshot with each listing.** Every listing of a section lists the window's choices
   first, in order, in the same background run, then the section with those values. A value no
   longer among its options (the context was removed from the kubeconfig) stops there: the section
   is not listed, the header shows the value as gone, and every action is refused
   (`choiceNeeded`) until the person chooses again. A choice whose own listing fails keeps its
   value as its only option when it declares `initialFrom` (a person who may not list namespaces
   still works in the context's namespace); otherwise the window shows the error. Rows remember
   the values they were listed under, and an action on rows listed under other values is refused
   (list them again).

6. **Confirmations that name the choices.** A `change` or `destroy` action may declare
   `"confirm"`, a question per language with `${window.<id>}` and `${rows}` (the chosen rows'
   first columns, at most ten and how many more):
   `"Delete ${rows} in the context ${window.context}, namespace ${window.namespace}? This cannot be undone."`.
   Such an action asks every time: the person gets `confirmationNeeded` with the question as its
   message, and the window's dialog shows it; an agent gets `confirmationNeeded` with a token
   whatever its run access (unless it is `none`, which refuses it), the notification shows the
   question, and only the person's `plugins.toolWindow.allow` lets the same call run once.
   `plugins.toolWindow.list` marks such actions `confirms`.

## Consequences

- The Kubernetes plug-in needs no platform code of its own: its window is a declaration, its
  rows a reader of `kubectl ... -o json`, and every one of its change and destroy actions asks with
  the context and namespace in the question.
- A terminal `kubectl config use-context` cannot redirect a click in the window, and the window
  never changes a kubeconfig.
- A choice costs one more short program run per listing (two for Kubernetes: `kubectl config view`
  and the namespaces), the price of never acting on a stale context.
- Choices hold values the platform listed, as rows do; nothing a caller types becomes an argument.
  Actions that need a value the person types (a local port to forward, a replica count) are still
  out of reach; a later decision on typed, person-only inputs can add them.

## Alternatives

- **Switch with `kubectl config use-context` as a `change` action:** writes the person's
  kubeconfig and changes every other tool's cluster with it; and a context changed outside the
  window would still redirect its actions. Rejected.
- **Read the kubeconfig in the platform:** a Kubernetes client in the platform, against ADR-0050,
  and a second reader of a format `kubectl` already reads. Rejected.
- **Pin the context in each row:** `kubectl get` does not print the context, and a column of the
  same value on every row would still not stop the kubeconfig changing between listing and acting.
