# ADR-0298: A Go project's environment: a shared file, a personal layer, and where they reach

- **Status:** Accepted (the owner, 2026-11-17)
- **Date:** 2026-11-16
- **Relates to:**
  - [ADR-0294](0294-the-login-shells-environment.md): the login shell's environment;
  - [ADR-0205](0205-trusted-projects.md): trusted projects;
  - [ADR-0165](0165-database-connections-types-and-secrets.md): files shared through Git and personal overrides;
  - [ADR-0093](0093-rust-analyzer-settings-tuned-and-layered.md): a project's `.zen/` settings file;
  - [ADR-0033](0033-ai-access-to-every-api.md): every action is a command agents reach.

## Context

Go reads much of its behaviour from environment variables: `GOPROXY` and `GOPRIVATE` decide where
modules come from, `GOFLAGS` adds flags to every command, `CGO_ENABLED` and `CGO_CFLAGS` decide how
C is built. A team behind a company proxy, or with private modules, needs the same values on every
computer, and one person sometimes needs their own (a token in a proxy URL, a different toolchain).

Before this record the New Project wizard's Environment field reached `go mod init` only and was not
kept. `GoEnv` asked `go env` with the IDE's own environment; runs, tests, gopls and the go.mod
checks got the toolchain's `PATH` and `GOROOT` and nothing of the project.

## Decision

1. **Two layers, kept apart.**
   - **Shared:** `.zen/go.json` at the project's root, meant to be committed:
     `{"version": 1, "env": {"GOPROXY": "…", "GOPRIVATE": "…"}}`. Other keys are kept when the IDE
     writes the file; emptying the layer removes a file that holds nothing else.
   - **Personal:** only on this computer, in the IDE's data folder (`go-environment.json`, one entry
     per project by its canonical path), never in the repository.
   - The personal layer wins over the shared one, variable by variable.
2. **Which variables.** Names that start with `GO` or `CGO_`, in capitals, digits and `_`.
   - `GOROOT` (the Go SDK chosen in Settings → Go) and `PATH` (a run configuration's) are refused in
     both layers.
   - `GOENV` (it points go at another settings file) and `GOTOOLCHAIN` only in the personal layer.
   - **No credentials in the shared file:** a value holding a URL with a user name or password
     (`https://user:token@…`) is refused there and belongs in the personal layer. Answers to agents
     show such a URL's user and password as `***` (ADR-0139).
   - A variable the shared file may not hold, written there by hand, is left out.
3. **Order, from lowest to highest:**
   1. the IDE's own process environment;
   2. the login shell's (ADR-0294);
   3. the Go toolchain's `PATH` with the SDK's `bin`, and `GOROOT`;
   4. the project's Go environment, shared then personal;
   5. a run configuration's own environment;
   6. for the IDE's own checks only (the go.mod checks, `go vet`, staticcheck), their fixed values:
      `GOTOOLCHAIN=local`, `GOPROXY=off` where they must not reach the network, and `-mod=readonly`
      added after the project's `GOFLAGS` rather than replacing them.

   The user's own `go env -w` file stays under all of these, as `go` itself does.
4. **Where it reaches.** One source, the Go toolchain's `project_environment(root)` (a new hook of
   `zen_lsp::ServerTools`, which every language's toolchain may answer), feeds:
   - the run context, so runs, tests, coverage, profiling, debugging and before-launch Go tasks;
   - the inspections' context, so `go env`, the go.mod checks, updates, `go vet` and staticcheck;
   - gopls's launch, so gopls and the `go list` it runs; a change made through the IDE restarts
     gopls (a hand edit of the file applies when gopls next starts);
   - `go install` of gopls and Delve started from a project window;
   - the New Project wizard: its rows are the new project's shared layer, written to
     `.zen/go.json`, and `go mod init` runs with them.
5. **Trust.** The shared file comes with the repository, and `GOFLAGS=-toolexec=…`, `CGO_CFLAGS`
   and `CGO_LDFLAGS` can run code. The shared layer applies only in a trusted project (ADR-0205),
   and the trust notification names `.zen/go.json`; the personal layer always applies, as it is the
   person's own. The platform learns whether a project is trusted through
   `zen_project::trust::trusted(root)`, a gate the workbench installs.
6. **Commands (ADR-0007, ADR-0033).**
   - `go.env.get {path?}` answers both layers, whether the shared one applies, and what is in
     effect with each variable's source. Agents may call it.
   - `go.env.set {scope: "project" | "personal", environment, path?}` replaces one layer from
     `NAME=value` pairs. Only a person sets it, because it can run code: agents get
     `agentNotAllowed`.
   - `go.env.settings` is Settings → Languages & Frameworks → Go → **Go Modules**: a field for each
     layer, the trust note, and what is in effect, with the fixed values of the IDE's own checks.

## Consequences

- A team commits `.zen/go.json` once and every clone gets the same proxy and private modules,
  after trusting the project.
- A token in a proxy URL stays on the computer it was typed on.
- An untrusted clone runs nothing of the shared file, and says why on the Go Modules page.
- Another language's toolchain can add a project environment of its own through the same hook.
