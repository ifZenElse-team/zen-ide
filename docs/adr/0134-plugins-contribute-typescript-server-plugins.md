# ADR-0134: A plug-in contributes TypeScript server plug-ins to another plug-in's server

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md),
  [ADR-0128](0128-a-plugin-installs-its-language-servers.md), [ADR-0130](0130-typescript-and-javascript-through-typescript-language-server.md)
- **Supersedes:** point 5 of [ADR-0132](0132-svelte-plugin-and-language-server.md) (the
  `typescript-svelte-plugin` left for later)

## Context

In a Svelte or Vue project, `.ts` and `.js` files go to the TypeScript server (ADR-0130), which
does not read `.svelte` or `.vue` files: an imported component is whatever a `declare module`
says (Svelte's own makes it any component, so wrong props go unnoticed; Vue has none, so the
import is a module that cannot be found). A TypeScript server plug-in exists for each,
`typescript-svelte-plugin` and `@vue/typescript-plugin`, which
`typescript-language-server` takes as `initializationOptions.plugins: [{name, location,
languages}]`, `location` being a folder whose `node_modules` holds the package. The owner: the
imports work with nothing to set up in the project.

## Decision

1. **`"serverPlugins"` in `plugin.json`**: a list of `{server, name, package, languages, when}`, the
   id of another plug-in's server, the package's name as the server loads it, what npm installs
   (`name@version`), the language ids it adds, and the projects it is for, by the project markers
   of ADR-0129 (`"when": {"packages": ["svelte"]}`: a project whose `package.json` lists `svelte` in
   `dependencies` or `devDependencies`; `files` works too). `zen_lsp::ServerPlugin`; the catalog and the
   installed manifest carry it (`zen-releases publish-plugin --server-plugins`).
2. **The registry keeps the contributions** of the plug-ins that are on (`LanguageServers::contribute`,
   `withdraw`): declared at start, added when a plug-in is installed or switched on, taken back when
   it is uninstalled or switched off. A contributed plug-in is **installed in the IDE's own folder
   for the server** (`<data>/language-servers/<server id>/node_modules`, ADR-0127), also when the
   server itself runs from `PATH`, and **passed to the server** only once it is there: its options
   get `plugins: [{name, location: <that folder>, languages}]` added to theirs.
3. **Installing.**
   - Installing a server installs the plug-ins contributed to it in the same npm run.
   - Installing or updating (by the user) a plug-in that contributes, installs what it contributes
     into the server's folder right after (ADR-0128's step), without installing the server itself
     when it is missing (that stays its banner's offer); without npm, one notification says what to
     get. A notification says when the plug-in is added.
   - **An IDE that already has the server and the contributing plug-in** (both installed before this
     decision) does not go to the network on its own (AGENTS.md rule 10): `lsp.servers.list` gives
     the server's `missingPlugins`, the missing ones **for the window's project** (whose `when`
     the project matches; without a project, those without `when`), and the banner above the
     server's files offers **Install** ("TypeScript Language Server needs typescript-svelte-plugin
     to read the imports of other plug-ins' files") only then: in a Svelte project for Svelte's,
     in a Vue project for Vue's, never in a plain TypeScript project. **Install** (and
     `lsp.server.install`), for a server that is there, installs every missing contributed
     plug-in. The update at start installs nothing (ADR-0130).
   - `when` limits only the offer: once installed, a plug-in is passed to the server in every
     project, where it does nothing without `.svelte` or `.vue` imports.
4. **The server starts again** when its plug-ins change (installed, contributed or taken back): the
   registry keeps a stamp per server, and a window restarts a running server whose stamp changed.
5. **Svelte contributes `typescript-svelte-plugin@0.3.52`** (plug-in 0.1.1, `when` the project
   depends on `svelte`) and **Vue `@vue/typescript-plugin@2.2.12`** (plug-in 0.1.1, `when` it
   depends on `vue`), the version of `@vue/language-server` the Vue
   plug-in runs, both to `typescript-language-server`. Vue's plug-in works beside the Vue server in
   full mode (ADR-0126): the TypeScript server never opens `.vue` files, so nothing is reported twice;
   the plug-in only gives the `.ts` side the components' types.

## Consequences

- In a `.ts` file that imports a component: the component's props are typed and checked, completed,
  Go to Declaration opens the `.svelte` or `.vue` file, Find Usages reaches it; renaming a prop in a
  `.svelte` component also changes the `.ts` files that pass it (the Svelte server's own edits).
  Renaming from the `.ts` side renames there only: the plug-ins rename in the `.ts` files.
- Removing the contributing plug-in drops its plug-in from the server's options (the package stays
  in the folder, unused).
- Any plug-in can extend another's TypeScript server the same way (Astro, MDX…).
