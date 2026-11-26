# ADR-0268: npm and Node.js: the package.json model, the npm tool window, Node.js runs, a debugger over the DevTools protocol and test reporters

- **Status:** Proposed
- **Date:** 2026-11-06
- **Extends:** [ADR-0046](0046-project-models-and-outlines.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0052](0052-debugger-zen-dap-and-delve-over-reverse-dap.md), [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md),
  [ADR-0248](0248-safe-mode.md)

## Context

Every IDE runs the scripts of a `package.json` since ADR-0129, and `node_modules` is excluded by
default (ADR-0123). What was missing for Node.js work: the dependencies in External Libraries, a
tool window for the scripts, commands for agents, running and debugging a Node.js file, and running
Jest and Vitest with the test tree. All of it must work in every IDE, not in WebWave only; the
network may be used only when the person asks, visibly, and it can be turned off; in safe mode
nothing of the project may run.

Node.js speaks the Chrome DevTools protocol over a WebSocket. The usual DAP adapter for it is a large
JavaScript program that would have to be downloaded, needs a second connection per child session,
which our DAP client does not have, and runs on the project's Node.js.

## Decision

1. **`zen_workbench::npm::model::NpmModel` is a project model in every IDE**, beside `NpmRun`. It
   reads files only (`runs_programs` stays `false`, so it works in safe mode). The packages are the
   root's `package.json`, its workspaces (`workspaces` of `package.json`, `packages` of
   `pnpm-workspace.yaml`, with `*`, `**` and `!`, never leaving the project, links included, at most
   500) and those one folder down. A dependency whose name is no package name is left out. Each is a `Module` with its dependencies (scope absent, `dev`, `peer`,
   `optional`) and `node_modules` as an excluded folder. The direct dependencies are libraries in
   the group `node_modules`: `name@version`, the version installed there, the nearest `node_modules`
   up to the project's root, or the declared range, not installed. Packages they depend on are not
   listed: `node_modules` can hold thousands, and the folder itself shows them. The model follows
   `package.json`, the lock files and the time of `node_modules`. External Libraries groups the
   dependencies under a provider's group only when all have one, so a Go module with a `package.json`
   keeps its label.
2. **The npm plug-in** (`plugins/npm`, marker `package.json`, no file types) turns on the npm tool
   window, as the Ant plug-in does its window. WebWave bundles it; the other IDEs offer it. It lists
   the packages with their scripts; `view.buildTool.*` act on its rows with `tool: "npm"`.
3. **Commands:** `npm.project.get` (reads only), `npm.script.run`, `npm.dependencies.install`,
   `npm.options.set`. The two that run go through the run service, so safe mode refuses them
   (`notTrusted`) and their output shows in the Run tool window.
4. **Network.** `npm.dependencies.install` is the only thing here that uses the network, when the
   person or an agent starts it. Offline mode (`offline` in `.zen/npm.json`) passes `--offline` to
   npm, pnpm and Yarn 1, sets `YARN_ENABLE_NETWORK=0` for a Yarn with `.yarnrc.yml`, and refuses Bun,
   which has no such switch. It also sets `COREPACK_ENABLE_NETWORK=0` for every npm run, so a Corepack
   shim fetches no package manager. An agent can turn offline mode on but not off, and the settings file is
   written through the guarded path, never through a link out of the project. The rest of ADR-0129
   holds: `npx --no` never downloads a program.
5. **The nodejs plug-in** (`plugins/nodejs`, marker `package.json`) turns on the run provider
   `zen_workbench::nodejs::NodeRun` (`RunProvider::plugin`), with the kinds `node` (a file, its Node.js
   options, the interpreter) and `node.test` (Jest or Vitest, files, a test name). The interpreter
   is the option, else `node` where language servers look for it. A runner is the project's own, in
   `node_modules`: a missing one is an error and is never fetched.
6. **Debugging is the IDE's own DAP adapter over the DevTools protocol**, a thread of the IDE, no
   download and no program besides Node.js. `AdapterLine::builtin` names it; the launcher starts it
   instead of a process, and it connects back to the IDE's listener like any adapter, so breakpoints,
   stepping, variables, evaluation and the console stay the platform's. It starts `node
   --inspect-brk=127.0.0.1:0`, reads the address Node.js prints and speaks the protocol over a
   loopback WebSocket client of our own (pure Rust, about a hundred lines, no new dependency).
   Source maps are not read: a file Node.js runs directly, `.ts` included, is debugged as it is; code
   compiled by a build is debugged as compiled.
7. **Test results come from reporters of ours.** Jest and Vitest take a reporter as a file; the IDE
   writes its own two small JavaScript reporters into its data folder and passes them to the
   project's runner (`--reporters`, `--reporter`), beside the runner's default one. They print
   `##zen-test {json}` lines that the output parser turns into the test tree as tests finish. **Rerun
   Failed** runs the failed files with `-t` and an exact expression. Debugging a Jest test runs Jest
   in one process (`--runInBand`) under the adapter. Debugging a Vitest test leaves the runner alone
   and gives Vitest `--inspect-brk` on a free loopback port with one forked worker; the launch's
   `inspectChild` tells the adapter to attach to the address the worker prints.

## Consequences

- A project's `package.json` adds a module beside Go's or Cargo's in `project.module.info`.
- `.zen/npm.json` is a new project file, written only when offline mode changes.
- A debugged program that starts child processes or worker threads is debugged in its main thread
  only. The program runs in a process group of its own, and stopping the session ends all of it.
- Vitest transforms test files; TypeScript tests whose lines move in that step stop at the compiled
  lines, as source maps are not read.
- A Jest older than 29 has no per-test callback: its results appear per file, when each file ends.

## Alternatives considered

- **The tool window in every IDE without a plug-in:** the decision is plug-ins, bundled in
  WebWave, as for Maven and Ant. The model is not gated, like the JVM libraries.
- **Refusing to debug Vitest:** its worker prints the same address, so the adapter needs one switch.
- **A downloaded DAP adapter for JavaScript:** a network step and a second connection per session,
  for what about a thousand lines of Rust do without either.
- **The runners' JSON reports:** they come at the end of the run; the tree should fill as tests finish.
