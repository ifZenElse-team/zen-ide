# ADR-0291: Plug-in commands only a person starts, asked every time

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Builds on:** [ADR-0286](0286-plugin-run-targets.md) (plug-in run targets, agent access per
  plug-in), [ADR-0031](0031-plugins.md) (WebAssembly plug-ins), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The Terraform plug-in offers `terraform init`, `validate`, `plan` and `apply` as run targets
(ADR-0286). `apply` changes real infrastructure, and so do `destroy`, `import`, `state` and the
other subcommands that write the state. ADR-0286 lets the person allow a plug-in's targets to
agents (`none`, `ask`, `full`), one setting for the whole plug-in. For Terraform that is not
enough: a person who lets an agent run `plan` should not thereby let it run `apply`. ADR-0286 also
lets agents edit configurations, so a rule carried by a target or by a configuration's options can
be rewritten: an agent could rename an `apply` configuration, add `-auto-approve` to it, or make a
`plugin` configuration of its own whose arguments say `apply`.

## Decision

1. **A plug-in declares the commands only a person starts**, in its `plugin.json`, beside `runs`:

   ```json
   "personOnly": [{"program": "terraform",
                   "subcommands": ["apply", "destroy", "import", "state", "taint", "untaint", "refresh", "force-unlock"]}]
   ```

   These eight are the owner's list (2026-11-13); `workspace` is not in it. The rule is read only
   from the installed manifest, which only a person installs (an agent cannot
   install a plug-in that declares programs or adapters, nor switch one on). Nothing a target or a
   configuration says can turn it off.
2. **The platform decides from the command line that will run.** When a `plugin` configuration is
   prepared, after its program and arguments are resolved, it is person-only when its program is
   a rule's program and any of its arguments that does not start with `-` is one of the rule's
   subcommands. So `-chdir=infra apply`, `-- apply` and an `apply` added by editing the
   configuration all count, whatever the configuration is called. The check sits in
   `Runs::prepare`, which every start goes through: `run.process.start`, the gutter and context
   runs, Rerun, compounds and before-launch tasks.
3. **Agents never start one**, whatever the plug-in's agent setting: the start answers `userOnly`.
4. **A person is asked every time**, also when the person started it: the window shows the run's
   name, the plug-in, the whole command line and its folder, with **Run** and **Cancel**; nothing
   starts until Run is clicked (Enter does not answer it). The answer is a one-time token for that
   exact command line and folder; a changed configuration is asked about again. A start that does
   not come from the window's own start path (for example a file watcher) is refused.
5. **No agent input while it matters.** While the question is open, and while a person-only run is
   running, the window refuses `press_keys` and window actions from agents, so an agent can neither
   answer the dialog nor type into the run's console (Terraform's own "Enter a value" prompt).
6. **Settings → Plugins shows the commands** under the plug-in's declared programs.
7. **The Terraform plug-in** declares the rule above and offers no `destroy` target.

## Consequences

- `terraform apply` from the IDE always shows the person what will run, and the plug-in's agent
  setting governs only `init`, `validate` and `plan`.
- `-auto-approve` needs no rule of its own: the dialog shows it in the command line.
- **The limit:** this holds for the IDE's run machinery. An agent with a terminal, or with its own
  shell, can run `terraform apply` directly; that is governed by the agent's own tool permissions,
  not by the IDE.

## Alternatives considered

- **A flag on the target** (`"person": true` in `zen_run_targets`): an agent edits configurations,
  so a flag carried by the configuration can be dropped.
- **Tracking which configurations an agent edited**: more state to keep right, and a person who
  starts an edited configuration would still not see what it runs.
- **A finer agent setting per target**: still a setting a person can open to agents; apply must not
  be one.
