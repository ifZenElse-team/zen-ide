# ADR-0206: Environment files, macros and instances of run configurations

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

Run configurations (ADR-0048) had a working directory, environment variables and arguments taken
literally. They also need to read `.env` files, let a value refer to the IDE's own variables,
expand macros in arguments and the working directory, and let a configuration run in several
instances at once, stopping and rerunning it otherwise. Our `run.process.start` started a second
copy of a running configuration, while the Run button reran it: two paths, two behaviours.

## Decision

1. `common` gains `envFiles`, `allowMultipleInstances`, `activateToolWindow` (default on) and
   `focusToolWindow`, each written only when it differs from its default, so older files and files
   without them stay as they were ([run-configurations.md](../run-configurations.md)).
2. **The platform resolves `common` before a provider sees it.** At every start (`run.process.start`,
   `run.process.rerun`, `debug.session.start`) the workbench reads the `.env` files in order, puts the
   configuration's own variables on top, expands `$NAME$` from the IDE's environment, and expands
   macros in the arguments and the working directory, then hands the provider a configuration with
   the result. Providers keep reading `common` as before and need no change. The run keeps the
   configuration as written, so a rerun expands again.
3. **One macro engine.** Run configurations use the external tools' macros (`zen_run::macros`) and
   their context; an argument line with macros is split, each word expanded, and joined again quoted,
   so a value with spaces stays one argument. `$Prompt$` and `$FilePrompt$` are parameters of the
   start; without them the start fails with the code `promptNeeded`, and the window asks.
4. **Instances are decided in the workbench, for every caller.** Starting a configuration that
   runs stops it and starts it again, the new run naming the run it `replaces` so that the Run tool
   window reuses its tab; with `allowMultipleInstances` another instance starts, numbered `name (2)`.
   The window asks first through `confirmRerun`, which makes the start fail with the code
   `rerunConfirmation`; agents never pass it, so over MCP the rerun happens without a question.
5. The limit of temporary configurations is the setting `run.temporaryConfigurationsLimit` (IDE
   setting `runTemporaryLimit`, default 5).

## Consequences

- A `$Name$` that is a known macro in an argument now expands where it used to stay literal.
- `.env` scripts (shell files that `export` variables) are listed like `.env` files but read as
  `.env` files; they are not run.
- Debug sessions get the resolved environment and arguments but no instance handling.
