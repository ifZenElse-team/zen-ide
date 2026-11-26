# ADR-0286: Plug-ins offer run targets; the platform runs them

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Builds on:** [ADR-0031](0031-plugins.md) (WebAssembly plug-ins),
  [ADR-0050](0050-products-are-plugin-bundles.md) (every capability a plug-in),
  [ADR-0200](0200-kotlin-runs-tests-and-debugs-through-the-java-kinds.md) (a plug-in export that
  feeds run targets: `zen_jvm`)

## Context

The Docker plug-in reads Dockerfiles and Compose files (the first two parts of the Docker work). A person wants to
start a Compose service from the run icon beside it, as a `main` or a test is started. Every run
kind today is native code of the platform or of a language module (`RunProvider`): npm, Go, Java,
Node.js. A Docker kind there would put Docker into the platform, against ADR-0050, and the next
tools that want the same (Terraform `plan`/`apply`, a load-test script, browser tests) would each
need one too.

A plug-in is a WebAssembly module with no imports (ADR-0031): it cannot start a process, and must
not. What it can do is read a file's text and say what in it can be run, and how.

## Decision

1. **A new optional export of plug-in ABI v1, `zen_run_targets`.** Input: the file's path relative
   to the project (UTF-8, with `/`), a NUL byte, then the file's text. Output: UTF-8 JSON, a list
   of targets:

   ```json
   [{ "line": 3, "name": "web", "id": "compose-up:web",
      "program": "docker", "args": ["compose", "-f", "${file}", "up", "web"],
      "cwd": "${fileDir}", "env": { "COMPOSE_MENU": "false" },
      "kind": "main" }]
   ```

   `line` is zero-based; `name` is what the icon's menu and the configuration are called; `id` is
   stable for the same target in the same file (it names the configuration the run makes);
   `program` is a program name looked up as the platform looks up tools (`PATH`, Homebrew, the
   usual folders), never a path the plug-in makes up; `args`, `cwd` and `env` may hold `${file}`,
   `${fileDir}`, `${projectDir}`, which the platform fills. `kind` is `main` or `test` (the icon). A
   module without the export offers nothing. The host drops targets past the last line, with an
   empty name or program, more than 200 per file, and arguments longer than 32 KiB in all.
   - **Only declared programs.** A plug-in names the programs its targets may run in its
     `plugin.json`, `"runs": ["docker"]`; the host drops a target whose `program` is not one of
     them (a bare name, no `/`). Settings → Plugins shows a plug-in's declared programs to the
     person, as it shows its file types and language servers, before anything runs.
   - **The environment cannot change what runs.** A target's `env` cannot set `PATH`, the
     platform's own variables (`ZEN_*`) or the dynamic loader's (`DYLD_*`, `LD_*`); the host drops
     those names. The platform's `PATH` and the configuration's own environment come first.
   - **No shell.** `args` are an argument list, passed to the program as its `argv`; nothing is
     ever run through a shell, so no quoting, globbing or command chaining applies.
2. **One run kind the platform owns, `plugin`.** A target becomes a configuration of kind `plugin`
   with the options `plugin` (the plug-in's id), `target` (its `id`), `file`, and the resolved
   `program`, `args`, `cwd` and `env`. Run/Debug Configurations shows the program and its
   arguments; the person can add arguments and environment variables and change the working
   folder, as for any other kind. The kind's provider (`zen_workbench::plugin_runs`) answers `targets` and `context`
   from the export of the plug-in that serves the file, and `command_lines` from the options. Runs
   go through the existing run path: the console, Stop, Rerun, Services, before-launch tasks, and
   the trust guard (`guard_run`): a project that is not trusted, or safe mode, refuses the run with
   the reason, as for every kind. The plug-in never starts anything; the platform starts the
   program, with the same process rules as other runs.
3. **Agents are refused by default.** A person's setting per plug-in, **Settings → Plugins →
   <plug-in> → Agents may run its targets**: `none` (the default: starting a `plugin` configuration
   of that plug-in, whether by `run.process.start`, its context or a rerun, answers
   `agentNotAllowed`), `ask` (the run is
   refused with a `confirmation` token, and a notification asks the person; once the person says
   yes, the same call with the token runs it), or `full`. Changing the setting is the person's
   alone (`agentNotAllowed` for an agent, like trust). Agents can always read the targets
   (`run.targets.list`) and the configurations, and edit configurations; only starting is limited.
   - **Per plug-in, not per run kind:** every plug-in target is the same kind, `plugin`, so a
     setting per kind would be one switch for every plug-in at once. A plug-in is what the person
     installs and trusts; it decides what its targets do (Compose starts containers, a Terraform
     plug-in would change infrastructure). One plug-in's targets are of one nature, so per plug-in
     is the unit a person can judge. A finer unit (per target) would be a list to maintain no one
     reads.
4. **The Docker plug-in uses it for Compose:** a target on `services:` (all services) and one on
   each service's name: `docker compose -f ${file} up <service>`. Nothing about Docker is in the
   platform.

## Consequences

- Terraform, load tests and browser tests become plug-ins that offer targets, with no platform code
  of their own.
- The platform's run machinery stays the only place processes start, so trust, safe mode, the agent
  rules, the console and Stop hold for every plug-in alike.
- What a target runs is visible and editable before it runs (the configuration's program and
  arguments), and the plug-in cannot hide a command behind a name.
- A plug-in whose targets need a debugger or test results is out of scope: `parser` is plain output
  for kind `plugin`. A later decision can add a test-report format to the export.
- The setting adds one row per plug-in that has the export, in its Settings page; plug-ins without
  it show none.

## Alternatives

- **A Docker run kind in the platform** (as npm's): rejected by the owner, Docker belongs in its
  plug-in (ADR-0050), and each new tool would need the same.
- **Plug-ins declare run templates in `plugin.json`** (static `"configurations"`, as npm projects
  do): cannot follow a file's contents (the services of this Compose file), so no gutter icons.
- **A plug-in import to start processes:** would break the module's isolation (ADR-0031) and the
  single run path; rejected.
