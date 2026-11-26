# ADR-0127: Language servers installed with npm go into the IDE's own folder, never among the global packages

- **Status:** Proposed
- **Date:** 2026-10-22
- **Supersedes:** the `npm install -g` part of [ADR-0055](0055-language-servers.md) (decision 3)
  and of [ADR-0126](0126-vue-language-server-and-npm-paths-in-server-options.md) (its
  installation and the consequence that a global TypeScript 7 is replaced)
- **Relates to:** the owner's decision of 2026-10-22: the IDE keeps its language servers in
  folders of its own

## Context

`lsp.server.install` ran `npm install -g <packages>` for every server declared with an npm
install (YAML, JSON, Bash, Dockerfile, Vue). That writes into the user's global npm packages: the
Vue server's `typescript@5` replaced a global TypeScript 7 and its `tsc`, and any server could
replace a global package of the user's at another version. The requirement: the IDE never
touches the user's global packages; it installs the language services it needs into folders of
its own and takes a project's TypeScript from the project.

## Decision

1. **Each npm server has a folder of the IDE's own**, `<data>/language-servers/<server id>/`, the
   data folder being the one of `language-servers.json` (an id's characters other than letters,
   digits, `-`, `_` and `.` become `_`). **Install** (the banner, the Language Servers list,
   `lsp.server.install` over MCP, and installing again to update) creates it and runs
   `npm install --prefix <folder> --no-audit --no-fund <packages>` with the npm found on this
   computer (`zen_lsp::servers::prefix_install_command`, through
   `ServerTools::install_command_into`, which a toolchain may refine). `go`, `cargo` and `rustup`
   installs are unchanged. A registry without a data folder (tests, tools) keeps `npm install -g`.
2. **Where the program is found, in order:** the path the user set in the server's settings
   (`language-servers.json`, as before: it always wins); then the IDE's own folder,
   `<folder>/node_modules/.bin/<command>`; then `PATH` and the usual tool folders, as before.
   A server installed globally before this decision is therefore still found on `PATH` and keeps
   working without anything to do; installing it again from the IDE puts a copy in the IDE's
   folder, which is used from then on, and the global one is left alone.
3. **TypeScript for the Vue server:** `${nodeModulesFolder:typescript/lib/typescript.js}`
   (ADR-0126) is the project's own `node_modules/typescript/lib` when it has `typescript.js`
   (TypeScript 5 or 6), looked for in the project's folder and the folders above it; otherwise the
   copy installed with the server, `<folder>/node_modules/typescript/lib`, which "beside the
   server's program" reaches from `<folder>/node_modules/.bin/vue-language-server`. A project's
   TypeScript 7 has no `typescript.js` and is passed over.
4. **No uninstall command.** Language servers have none today; deleting
   `<data>/language-servers/<id>` removes a server installed by the IDE.

## Consequences

- Installing a server never changes the user's global npm packages or the `tsc` on their `PATH`.
- Each server keeps its own dependencies (the Vue server with TypeScript 5), at the price of disk
  space per server.
- Each IDE has its own folder, so GoGarden, RustRonin and WebWave install a server once each.
