# ADR-0296: Run targets that compare with a cluster and that only a person applies

- **Status:** Accepted (the owner, 2026-11-16)
- **Date:** 2026-11-16
- **Extends:**
  - [ADR-0286](0286-plugin-run-targets.md): plug-in run targets;
  - [ADR-0290](0290-tool-window-choices-and-confirmations.md): a tool window's header choices;
  - [ADR-0291](0291-commands-only-a-person-starts.md): commands only a person starts.

  All three stay as accepted; this record adds to them.

## Context

For a Kubernetes manifest, a person wants to see how it differs from what runs in the cluster (`kubectl diff`), and to apply it (`kubectl apply`), from the file or from a folder of
manifests. The Kubernetes window already knows which cluster: the context and namespace the person
chose in its header (ADR-0290). The existing pieces fall short in four places:
- A run target's arguments know `${file}`, `${fileDir}` and `${projectDir}`, but not the window's
  choices. A target would therefore run against whatever context the kubeconfig names, the risk
  ADR-0290 removed.
- Run targets go to the one plug-in that handles a file, and `.yaml` is the YAML plug-in's.
- Run targets are per file; a folder has none.
- A target's output goes to the Run console. A diff is read best in the diff viewer, and `kubectl
  diff` exits with 1 when something differs, which is not a failure.

## Decision

1. **Window values in run targets.**
   - **The placeholder:** a run target's argument, working folder or environment value may be
     exactly `${window.<window>.<choice>}`. It is replaced, as one argument, by the current valid
     choice of the same plug-in's tool window (ADR-0290). Part of a longer argument, it is
     refused.
   - **When it is filled:** when the run is prepared, before the person-only check (ADR-0291), so
     the person's question shows the real context and namespace in the command line.
   - **Where the value comes from:** the window's last listing. When the window has not been
     listed in this IDE session, its choices are listed once, synchronously, from the remembered
     values, as the window's own listing would. When there is no valid choice, the run is refused
     with `choiceNeeded`.
2. **`targetFiles`.**
   - **What:** a plug-in may declare `"targetFiles": ["*.yaml", "*.yml", "*.json"]`, file-name
     patterns its `zen_run_targets` is also asked about, without becoming those files'
     highlighter.
   - **Merging:** a file's run targets are those of the plug-in that handles it plus those of
     every plug-in whose `targetFiles` match it.
   - **Who decides:** the plug-in decides from the text whether it has targets (Kubernetes:
     `apiVersion` and `kind` at the top).
3. **Folder targets.**
   - **The question:** for a folder, the plug-ins with `targetFiles` are asked with the folder's
     path relative to the project, ending in `/`, and as text the project-relative paths of the
     folder's files their patterns match, one per line. The walk is recursive, at most 1000 files,
     and skips hidden entries and links.
   - **The answer:** they answer folder targets on line 0, in which `${file}` is the folder.
   - **Where they show:** the Project view's context menu shows a file's or folder's targets
     (**Run Targets of the File or Folder**), and `run.targets.list` and `run.process.start` take a
     folder path.
4. **`"output": "diff"`.**
   - **The run:** a target may declare that its standard output is a unified diff. It runs as any
     run, in the Run window, under the same trust, agent and person-only rules.
   - **Exit codes:** 0 is "no differences", 1 is "differs", and anything else is a failure, shown
     in the console.
   - **The viewer:** on 1, the platform rebuilds both sides of each object from the diff and opens
     them in the diff viewer (`file.compare.texts`, at most 20). The left side is titled "Live now
     (<choice> <value>, …): <object>" and the right side "After applying <file>: <object>".
   - **Whole sides:** a plug-in that wants both sides complete asks for full context. Kubernetes
     sets `KUBECTL_EXTERNAL_DIFF="diff -N -U1000000"` in the target's environment, which ADR-0286
     allows.
5. **The Kubernetes plug-in:**
   - **Targets:** **Diff with Cluster** (`kubectl --context ${window.kubernetes.context}
     --namespace ${window.kubernetes.namespace} diff -f ${file}`, output diff) and **Apply to
     Cluster** (`… apply -f ${file}`), for a manifest file on its first `apiVersion` line. For a
     subfolder of manifests the same two take `-R`. The project folder itself has none.
   - **The person-only rule (ADR-0291)** covers every kubectl subcommand that changes a cluster
     from the IDE's runs: `apply`, `create`, `replace`, `patch`, `delete`, `edit`, `scale`,
     `rollout`, `label`, `annotate`, `set`, `drain`, `cordon`, `uncordon`, `taint`.
     - **For the person:** they confirm the whole command line every time.
     - **For agents:** they are refused (`userOnly`) whatever their run access.
     - **What stays out:** `diff`, `get` and `describe`, which only read and follow the plug-in's
       agent setting.
   - **Untrusted projects and safe mode** run none of them, as every run (ADR-0286).

## Consequences

- A manifest's diff and apply always name the context and namespace chosen in the Kubernetes window.
  A context changed in a terminal cannot redirect them, and an agent can see a diff but never
  apply.
- Other tools can offer targets on files they do not highlight (a Helm chart folder, Terraform
  modules) and get the diff viewer for their own diffs.
- The person-only rule is read from the command line, so it holds for tool-window stream actions
  (Show Log, Describe) as for targets. The window's change and destroy actions keep their own
  confirmations (ADR-0290).

## Alternatives

- **The context in the target as a fixed value:** a terminal `use-context` would redirect it.
  Rejected.
- **Declaring `.yaml` as a Kubernetes file type:** the YAML plug-in would lose its files. Rejected.
- **A diff tool window of the plug-in's own:** it would duplicate the diff viewer. Rejected.
