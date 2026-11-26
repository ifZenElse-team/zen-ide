# ADR-0126: The Vue plug-in runs the Vue language server in full mode; server options can name paths in npm packages

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0055](0055-language-servers.md), [ADR-0031](0031-plugins.md),
  [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

The Vue plug-in (`plugins/vue`) highlights single-file components itself, but problems, completion
from the types, navigation, rename and formatting need the Vue language server
(`@vue/language-server`, Volar). Its versions want different things from a client:

- **Version 3** (the current one) only works with a second server: the TypeScript server
  (`tsserver`, usually through `typescript-language-server`) loaded with `@vue/typescript-plugin`.
  The Vue server sends `tsserver/request` notifications, the client forwards each to the
  TypeScript server (`workspace/executeCommand` `typescript.tsserverRequest`) and sends the answer
  back as `tsserver/response`; without that, every request of the Vue server waits forever on its
  first `_vue:projectInfo` question. The TypeScript server also has to have every `.vue` file open,
  and it answers the script's problems, completion and navigation, so two servers serve one file.
- **Version 2** has, besides that "hybrid mode", a full mode (`"vue": { "hybridMode": false }` in
  `initializationOptions`): one server that runs TypeScript itself and answers everything for the
  template, the script and the style. It must be told where TypeScript's `lib` folder is
  (`"typescript": { "tsdk": "<path>" }`); TypeScript 7, the native one, has no `typescript.js` and
  cannot be used.

The IDE's language client serves a file with one server, and a declaration's options are fixed
JSON, so it can give neither the second server nor a path that depends on where npm put
TypeScript.

## Decision

1. **The Vue plug-in declares `@vue/language-server@2.2.12` in full mode**, pinned, with
   `"initializationOptions": { "typescript": { "tsdk": "${nodeModulesFolder:typescript/lib/typescript.js}" }, "vue": { "hybridMode": false } }`,
   and installs it with TypeScript 5: `"install": { "kind": "npm", "package": "@vue/language-server@2.2.12 typescript@5" }`.
   One server answers problems, completion, Go to Declaration, quick documentation, rename and
   Reformat Code in `.vue` files, with the platform as it is. Version 3 waits for the platform to
   serve one file with two servers and to forward a server's notifications to another (below).
2. **A string of a declaration's `initializationOptions` may name a path in an npm package**,
   resolved as the server starts (`zen_lsp::servers::expand_options`, called by
   `LanguageServers::initialization_options` for every declared server, after its toolchain's own
   options):
   - `${nodeModules:<path>}` becomes `node_modules/<path>`;
   - `${nodeModulesFolder:<path>}` becomes the folder of the file `node_modules/<path>`, so that a
     package found without that file (a TypeScript 7) is passed over.
   They are looked for as Node.js looks for a module: in the project's folder and the folders above
   it, then next to the server's program (its real path and the path found), in the package it
   belongs to and the folders above, which is where `npm install -g` puts the packages installed
   with it (`<prefix>/lib/node_modules`, or `node_modules` beside the program on Windows). The
   project's own TypeScript comes first, so the server checks with the version the project builds
   with. A path found nowhere becomes the project's `node_modules/<path>`, so that the server's
   error names it. Only a whole string is a variable; other strings stay as they are.
3. **An install recipe's `package` may name several packages separated by spaces**; each becomes
   an argument of `npm install -g`, `go install`, `cargo install --locked` or `rustup component add`.
4. **What is missing for version 3 (hybrid mode)**, left for a later decision: a file served by
   several servers at once (the Vue server for the template and the style, the TypeScript server
   for the script, their answers merged), and forwarding a custom notification of one server as a
   request to another and its answer back. Both are general and would be declared in
   `plugin.json`, not written for Vue.

## Consequences

- A Vue project works after **Install** in the server's banner, or with the project's own
  TypeScript (5 or 6) when it has one; with neither it fails to start and says it cannot find
  `typescript.js` in its log and status.
- Installing puts TypeScript 5 among the global npm packages; a global TypeScript 7 is replaced
  by it. Pinning version 2 keeps the plug-in on the last release with a full mode, which receives
  no more features; moving to version 3 is the decision of point 4.
- Other plug-ins can now give a server a path in an npm package (a TypeScript plug-in's `tsdk`,
  a language server plug-in's location) the same way.
