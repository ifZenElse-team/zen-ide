# ADR-0248: Safe mode: a project not trusted yet runs nothing of its own

- **Status:** Accepted (the owner, 2026-11-06)
- **Date:** 2026-11-05
- **Relates to:** [ADR-0205](0205-trusted-projects.md), which it extends and partly supersedes;
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

ADR-0205 keeps builds, Lombok and a project's own configurations from running by themselves, but a
project that is not trusted still starts its language servers, its go.mod checks and `cargo
metadata`, and code at the caret still runs. gopls runs `go`, which can download and run the
toolchain a `go.mod` names and fetch modules, and a language server or formatter can be a program
inside the project. The IDE therefore has to ask before it opens an unknown project and offer a
safe mode: a trusted answer must come before the project opens.

## Decision

1. **Every project is asked about.** In a window a person uses, opening a folder that is not trusted
   shows **Trust and Open Project '<name>'?** with **Trust Project**, **Preview in Safe Mode**,
   **Don't Open** and **Trust all projects in '<parent>'**, whether or not the folder has a build.
   The dialog comes before the project opens: **Don't Open** leaves nothing behind. **Preview in Safe
   Mode** is an answer for this open only; the next open asks again. The notification of ADR-0205
   section 3 stays for the opens that did not ask (an agent's `open_project`) and for a run refused
   later, and is not posted after the dialog was answered.
2. **Not trusted means safe mode; there is no third state.** Files open and can be edited, and search
   and the terminal work. These do not start:
   - **Git, the owner's decision.** The IDE makes no Git call in a project that is not trusted: no
     status, diff, blame, log or commit, automatic or explicit, for the person and for agents, in a
     window and headless. A repository's `.git/config` can name filter drivers and hooks, and a
     downloaded folder can bring one. The check is in one place every call passes: `zen_vcs::git`
     holds a gate that `Repository::discover`, `Repository::open` and the one function that starts
     `git` ask, and the workbench installs the trust check there, so a new call cannot miss it.
     Commands are refused with code `notTrusted` and the Git tool windows say why they are empty.
     Git also runs at the root of a repository holding a trusted folder, so that a project in a
     subfolder of a repository works; a project beside it stays off.
   - language servers, formatters that are a program of the project, go.mod checks (the program and
     their network) and `cargo metadata`; a provider or inspector declares it with `runs_programs`;
   - **everything that runs or debugs**, a configuration made from the code at the caret too, for the
     person and for agents: refused with code `notTrusted`, in a message that says why. This
     supersedes "a configuration made from the code at the caret still runs" of ADR-0205 section 2: `go run`
     at the caret downloads what `go.mod` names, so it cannot be told apart from the rest.
   - **The exception, the owner's decision: a scratch file runs.** It is the person's own code in the
     data folder. Only the configuration the file's provider makes for it passes (one with other
     settings, a before-launch task or another file is refused), and in safe mode it runs with its
     own folder as the root, so the project's `go.mod` or `rust-toolchain.toml` chooses nothing.
     Debugging has no exception.
3. **The rest of ADR-0205 holds.** Run and Debug are what section 2 refuses. A build tool's explicit
   reload or task, an external tool, a Run Anything command or a database migration that the person
   starts stays the person's action, unless a program of the project runs before it, and agents are
   refused the builds.
4. **`GOTOOLCHAIN=local`.** A language server that starts in an untrusted project gets it, as a last
   guard should a start get past section 2; `go version`, `go env` and the module checks always run
   with it, as they ask about the SDK they are.
5. **Trust starts what was held back, without reopening.** Git and language servers follow trust by
   themselves, a window reloads its project model when its project turns trusted, and go.mod checks
   run again. The state is read from the build settings file and is cached until the file changes,
   so a person's answer in one IDE reaches a headless IDE on the same data folder.
6. **Trusted Locations.** `trustedLocations` in the build settings file, beside `projectTrust`, is an
   ordered list of canonical folders; every project under one is trusted. `project.trustedLocations.get`
   lists them and marks one that covers the home folder; `project.trustedLocations.set` replaces the
   list. Removing a location is a safe change and open to agents; a new folder is refused for them
   (`agentNotAllowed`).
7. **Agents can never answer.** A dialog and `view.project.trust` are for the person: `press_keys` is
   refused while the dialog is up, so no focused button can be pressed by an agent, the dialog binds
   no key to **Trust Project**, and `view.project.trust` is refused for agents. An agent's
   `open_project` opens in safe mode without asking, and a headless IDE never asks.

## Consequences

- A headless IDE on a project nobody trusted has no language servers: `mcp --headless` says so when
  it starts, and every refusal an agent gets says what to do. A person trusts the folder, or a
  location above it, once in a window of the same data folder; the owner's decision is that there is
  no command-line trust.
- **Trust all projects in '<parent>'** warns when the parent is the home folder or a folder above it.
- **A clone passes the gate; its result does not.** `git clone` makes a new folder, with no
  repository config to run yet and without reading the config of the folder it starts in, so the
  person's Clone, or an agent's `project.fromVcs.clone`, runs in safe mode too. The result is never
  trusted, whoever cloned it: opening it asks the trust question, and the gate is shut on it until
  the person answers. Submodules are not fetched in the clone, since that would act on the
  repository's own text (ADR-0244).
- A linked worktree outside a trusted folder shows no change count until it is trusted too.
- `core.fsmonitor` stays off for every Git call of the IDE, in trusted projects too.
- The trust file and the settings are in the data folder. The IDE's commands that write files
  refuse an agent there, scratch files aside (`agentNotAllowed`). A terminal can still write there:
  this ADR is no sandbox for what a terminal can do, and only for that.
- Every project opened before this change asks once.
- Tests that need a language server or a run trust their project first.
- A project in a location is trusted as a whole: revoking it inside the location has no effect until
  the location is removed.

## Alternatives considered

- **Asking only for a project with a build:** a folder without one still starts gopls or a server
  from `node_modules/.bin`.
- **Keeping the run at the caret in safe mode:** it runs the project's code with the network, which
  is what safe mode is for.
- **Remembering Preview in Safe Mode:** the answer is a look, not a decision; a decision would be a
  third state to keep.
