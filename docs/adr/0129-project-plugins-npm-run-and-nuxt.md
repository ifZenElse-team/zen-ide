# ADR-0129: Plug-ins recognise projects, require other plug-ins and add run configurations; npm runs in every IDE; Nuxt is the first such plug-in

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0031](0031-plugins.md), [ADR-0048](0048-run-configurations-and-the-run-service.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0115](0115-file-templates.md),
  [ADR-0126](0126-vue-language-server-and-npm-paths-in-server-options.md)

## Context

The owner decided on 2026-10-22 that Nuxt is a plug-in offered when needed and bundled with no
IDE, and that it has no language server of its own: it runs on the Vue plug-in's Vue language
server, and Nuxt's auto-imports reach that server through TypeScript, from the `.nuxt/` folder Nuxt
generates (the project's `tsconfig.json` extends `./.nuxt/tsconfig.json`).

What a Nuxt plug-in has to do is therefore about the project, not about a file type:

- a Nuxt project is recognised by `nuxt.config.ts` (`.js`, `.mjs`) at its root or by `nuxt` among
  the dependencies of its `package.json`, while `nuxt.config.ts` is an ordinary TypeScript file and
  the components are the Vue plug-in's `.vue` files. The IDE offered a plug-in only for a file that
  no highlighter supports, by its extension or name;
- the plug-in is useless without the Vue plug-in, and plug-ins could not say that they need another;
- until `nuxi prepare` (or `nuxi dev`, or an install's `postinstall`) has written `.nuxt/`, the
  language server sees none of Nuxt's auto-imports, and the project should be offered to run it;
- the project runs with `nuxi dev`, `build`, `generate` and `preview`, through the project's package
  manager, and the IDE had no way to run anything of Node.js: every run provider came from a
  built-in language module (Go, Rust).

What is wanted: framework projects recognised by their configuration file and dependencies,
`package.json` scripts and package binaries run with an **npm** run configuration (package.json,
command, scripts, arguments, package manager), and run icons next to the scripts of
`package.json`.

## Decision

1. **A plug-in may declare the projects it supports**, `"project"` in its `plugin.json`, carried by
   the catalog and the installed plug-in's `plugin.json` like the other declarations
   (`zen_project::support::ProjectSupport`, read by `zen-plugins`, published by
   `zen-releases publish-plugin --project=<json>`):
   - `markers`: `files` at the project's root, any of which marks it, and npm `packages`, any of
     which in the `dependencies` or `devDependencies` of the root's `package.json` marks it;
   - `runConfigurations`: run configurations (name, kind, options, common settings, as in
     `.zen/runConfigurations.json`) the plug-in adds to such a project;
   - `setup`: a step that prepares such a project, `{ "creates": "<path>", "configuration": {…} }`,
     needed while `creates` (relative to the project) is missing.
   Paths that leave the project and configurations without a name or a kind are refused when the
   declaration is read.
2. **A plug-in may require other plug-ins**, `"requires": ["<id>", …]`. Installing it from the
   banner, a notification, the Marketplace, `plugins.plugin.install` or the MCP tool installs first
   the plug-ins it requires, and theirs, that are not installed (`Catalog::with_requirements`, which
   stops at cycles). Nothing more: a required plug-in is an ordinary plug-in afterwards; it can be
   switched off or removed, and the requiring one then works without it.
3. **The window offers a project's plug-ins.** When a project window opens, and again whenever the
   plug-ins or the catalog change, the catalog's plug-ins whose markers match the project and that
   are not installed are offered in a notification (group Plugins), naming the plug-ins installed
   with them: **Install** (`view.plugin.install`) and **Don't suggest**
   (`view.plugin.ignoreForProjects`, remembered in `plugins/ignored-project-plugins.json`). Only a
   catalog fetched already is looked at; the IDE fetches it at every start that has plug-ins to
   update, which every released IDE has (its bundled ones), so no fetch is added for this.
4. **Installed plug-ins apply to the project through commands** (`zen_workbench::plugin_projects`):
   - `plugins.project.list`: the installed plug-ins that are on and support the open project, with
     their run configurations and setup step and whether it is needed;
   - `plugins.project.apply`: adds each such plug-in's run configurations to the project's file,
     except those the project has (same kind and name), **once per project and plug-in**, so that a
     configuration the user deleted does not come back; and answers each setup step that is needed
     and was not offered in this project before, marking it offered. The window calls it when the
     project opens and when the plug-ins change, says in a notification which configurations were
     added, and offers each setup in a notification whose action is the next command;
   - `plugins.project.setup {plugin?, waitMs?}`: runs the step's configuration as `run.process.start`
     does, with its output in the Run tool window; without `plugin`, the first plug-in whose step is
     needed (Find Action's **Run Project Setup**).
   What was done once is kept in `plugin-projects.json` in the IDE's data folder.
5. **npm runs in every IDE.** The workbench has a run provider of the kind `npm`
   (`zen_workbench::npm::NpmRun`), added to every product beside its modules' providers:
   `packageJson` (the command runs in its folder), `command` (`run` a
   script, `exec` a program of the project's packages, `install`), `script` (the script or the
   program) and `packageManager` (`auto`, `npm`, `pnpm`, `yarn`, `bun`); the arguments are the
   common program arguments. `auto` is the package manager `packageManager` in `package.json`
   names, else the one whose lock file is there, else npm. The commands are `npm run <script> --
   <args>`, `pnpm|yarn|bun run <script> <args>`, `npx --no <program> <args>` (`--no`: a program the
   project does not have is never downloaded), `pnpm exec`, `yarn run` and `bun run` for programs,
   and `<manager> install`. The package manager is looked for where language servers are
   (`PATH` and the usual tool folders) and runs with those folders on its `PATH`, so that Node.js
   is found when the IDE started from the Dock. The scripts of a `package.json` get run icons in
   the gutter (`run.targets.list`), named after the script.
6. **The Nuxt plug-in** (`plugins/nuxt`) declares no file types and no language server: markers
   `nuxt.config.ts|js|mjs` and the package `nuxt`; `requires: ["vue"]`; the run configurations
   `nuxi dev`, `nuxi build`, `nuxi generate` and `nuxi preview` (kind `npm`, `exec nuxi <command>`);
   the setup `nuxi prepare`, which creates `.nuxt`; and the file templates Nuxt Page, Nuxt Layout,
   Nuxt Component (`.vue`), Nuxt Composable and Nuxt Server Route (`.ts`). Its WebAssembly module
   is the smallest ABI v0 allows and highlights nothing: `nuxt.config.*` stay TypeScript or
   JavaScript files, highlighted by the base set's plug-ins; the platform's icon table gives them a
   Nuxt icon of our own, as it gives `package.json` and `.vue` theirs. No product bundles it
   (`PRODUCT_PLUGINS` is unchanged).

## Consequences

- Any framework or tool plug-in (Svelte, Astro, Angular, Vite, Prisma…) can be offered for its
  projects, require the plug-ins it builds on, add its run configurations and offer its setup step
  without platform code of its own.
- Every IDE runs npm scripts and package binaries, from the run configurations and from the gutter
  of `package.json`; later work on `package.json`, npm and scripts builds on this provider
  (the npm tool window, Node.js interpreters, debugging) instead of starting one.
- What a plug-in declares runs only when someone runs it: a configuration from the Run widget, the
  setup step from its notification's action or a command. The notification is the IDE's own (its
  action is `plugins.project.setup`; only the IDE's own notifications have actions,
  docs/automation.md), and what it runs is a configuration of a kind the IDE knows, from a plug-in the
  publisher signed (ADR-0031); its name and command line show in the Run tool window.
- Run configurations a plug-in adds are written into `.zen/runConfigurations.json` like the user's;
  removing the plug-in leaves them.
- A plug-in installed for a project offer gets its language servers (the Vue server) after a
  restart, as every plug-in that declares servers does today.
- The setup step is offered once per project and IDE; afterwards it runs with **Run Project
  Setup** in Find Action, `plugins.project.setup` (without `plugin`, the first plug-in whose step
  is needed), or from the terminal. Its configuration is not added to the project.
