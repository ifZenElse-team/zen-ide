# ADR-0130: TypeScript and JavaScript through typescript-language-server, offered by the banner

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0055](0055-language-servers.md), [ADR-0126](0126-vue-language-server-and-npm-paths-in-server-options.md),
  [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md), [ADR-0128](0128-a-plugin-installs-its-language-servers.md)

## Context

The `typescript` and `javascript` plug-ins, in every IDE's base set, highlight, fold, outline and
complete from the text alone. Problems, completion from types, navigation, usages, rename,
formatting and imports need TypeScript's own language service, which every TypeScript tool reaches
through `tsserver`. Two servers speak LSP over it:

- **typescript-language-server** (TypeFox and contributors; 5.3.0 needs Node.js 20, 6.x Node.js
  22.22): a thin LSP layer over `tsserver`, in wide use among LSP editors. It answers
  everything the platform asks gopls for: diagnostics, completion, hover, signature help,
  definition, type definition, implementation, references, document and workspace symbols,
  document highlights, call hierarchy, code actions, rename, formatting. It finds TypeScript in
  order: `tsserver.path` from the client, the workspace's `node_modules/typescript` when it has
  `lib/tsserver.js`, `tsserver.fallbackPath`, then the `typescript` it can `require` (the copy
  installed beside it).
- **@vtsls/language-server** (0.3.0): VS Code's TypeScript extension ported to LSP, richer (VS
  Code's refactorings, file renames) but driven by VS Code's settings through
  `workspace/configuration`, which this client answers with nothing for sections it does not know,
  and younger.

Base-set plug-ins are never "installed", so ADR-0128's install-with-the-plug-in does not fire for
them. The owner's rule 10 (AGENTS.md): no network unless the user asks.

## Decision

1. **The server is `typescript-language-server@5.3.0` with `typescript@5`**, declared word for word
   by both plug-ins (`typescript` and `javascript`, so that either alone has it; the first
   declaration of an id wins, and the test of the plug-ins' declarations requires two
   declarations of one id to be identical). It serves `ts mts cts tsx js mjs cjs jsx` and never
   `.vue`, which stays with the Vue server in full mode (ADR-0126): in a Vue or Nuxt project the
   components go to the Vue server and the scripts to this one. Version 5.3.0 rather than 6 for
   Node.js 20 and 21.
2. **Language ids by extension.** A declaration may map extensions to protocol language ids,
   `"languageIds": { "tsx": "typescriptreact", "jsx": "javascriptreact", … }`
   (`ServerDeclaration::language_ids`); without an entry the old rule holds (the language named
   like the extension, else the first).
3. **TypeScript as for Vue:** the server's own order, with
   `"tsserver": { "fallbackPath": "${nodeModules:typescript/lib/tsserver.js}" }`, which
   ADR-0126's expansion resolves to the project's TypeScript found up the folders, else the copy
   installed with the server in the IDE's folder. A project with **TypeScript 7** (the native one,
   without `tsserver.js`) is passed over by both lookups and checked with TypeScript 5 from the
   IDE's folder: the problems are TypeScript 5's, which for most code are the same; nothing fails.
4. **Optimize Imports removes unused imports, then sorts.** The server's `source.organizeImports`
   only sorts and merges; its `source.removeUnusedImports` removes. A declaration may list the
   code action kinds Optimize Imports asks for, in order (`"optimizeImports"`, each kind's edits
   applied before the next is asked for); the TypeScript declaration lists
   `source.removeUnusedImports`, then `source.organizeImports`. Without the list it is
   `source.organizeImports` alone, as for gopls.
5. **How it is installed: offered, one click, never on its own.** As for every base-set server
   (YAML, JSON, Bash, Docker), the banner above the first `.ts` or `.js` file whose server is
   missing offers **Install** (and **Don't ask again**, **×**); **Install** puts both packages into
   the IDE's own folder (ADR-0127) in the background and the server starts at once. Nothing is
   installed when Node.js is merely present: that would use the network without being asked
   (AGENTS.md rule 10), and shipping the TypeScript service with the IDE would mean bundling
   Node.js code, which we do not. A Marketplace install or a manual update of either plug-in
   installs the server as ADR-0128 says; **the update at start never installs servers** (for any
   plug-in), so that the new declaration of these plug-ins does not start npm on every IDE's next
   start.
6. **Highlighting stays the plug-ins' own** (pure Rust); the server's semantic tokens, like gopls',
   only add to it where the platform uses them.

## Consequences

- Every language feature the platform wires for gopls works in TypeScript and JavaScript files,
  checked against the real server by `crates/platform/workbench/tests/typescript_language_server.rs`
  (ignored; `ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX`).
- Type hierarchy and inlay hints are not given: the server has no type hierarchy, and its inlay
  hints need TypeScript preferences that are strings, which `inlayHints` (booleans) cannot set yet.
- The plug-ins move to 0.2.0; an IDE that already has 0.1.0 gets the declaration with the update,
  which takes effect after a restart.
