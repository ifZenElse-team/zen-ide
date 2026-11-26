# Web

WebWave is the Zen IDE for the web: HTML, CSS, SCSS, JavaScript and TypeScript, in the same
IDE window as every Zen IDE, with Git, the terminal, Find in Files, the HTTP Client and AI agents.

Everything on this page is plug-ins, and every plug-in works in every Zen IDE: basic web (HTML, CSS,
JavaScript, TypeScript) comes with each of them, and WebWave adds SCSS, which the others
install from the Marketplace ([Plug-ins](10-plugins.md)). Frameworks such as Vue, Nuxt, React,
Next.js and SvelteKit are plug-ins you add when a project needs them: opening a `.vue` file offers
the Vue plug-in, opening a Nuxt, React, Next.js or SvelteKit project the plug-in for it. WebWave has nothing another Zen IDE cannot get; it is
the one that comes ready for web work.

## Install WebWave

In Zen Toolbox, under **Available**, click **Install** next to WebWave; once installed it moves to
**Installed**, and a click starts it ([Zen Toolbox](13-toolbox.md)). Zen Toolbox also puts the
command `webwave` in `~/.local/bin`, so that in a terminal:

```sh
webwave .                       # the current folder in WebWave
webwave index.html:12           # the file at line 12 (:12:5 at column 5 too)
webwave --headless .            # WebWave without a window, for AI agents
webwave mcp --headless .        # the same as an MCP server on stdio
```

With WebWave already running, the command opens the folder or file in it and returns at once.
**Tools → Create Command-line Launcher…** writes the same command from WebWave itself.

## Open a web project

A project is a folder: the one with your `index.html`, `package.json` or sources. **Open** in the
welcome window picks it; recent projects open with a click. WebWave reopens the projects and files
that were open when it last quit; while it starts, a small picture with its version shows in the
middle of the screen.

## What WebWave understands today

- **HTML** (`*.html`, `*.htm`, `*.xhtml`): highlighting, folding, the element tree in the
  **Structure** tool window, completion of tags, attributes and values, Emmet abbreviations with
  **Tab**, closing tags as you type, and colour previews for styles ([HTML](10-plugins.md#html)).
- **CSS** and **SCSS** (`*.css`, `*.scss`): highlighting, folding, the rules in **Structure**,
  completion of properties, values, units and at-rules, and colour previews in the gutter with a
  colour picker ([CSS](10-plugins.md#css), [SCSS](10-plugins.md#scss)).
- **JavaScript** and **TypeScript** (`*.js`, `*.mjs`, `*.cjs`, `*.jsx`, `*.ts`, `*.mts`, `*.cts`,
  `*.tsx`): highlighting with JSX and TypeScript's types, folding, functions and classes in
  **Structure**, completion of keywords, names of the file and snippets, and JSX end tags as you
  type; with the TypeScript Language Server, everything else
  ([TypeScript and JavaScript](#typescript-and-javascript)).
- **Vue** (`*.vue`): single-file components highlighted block by block, folded, outlined and
  completed, and checked by the Vue Language Server ([Vue](#vue)).
- **Svelte** (`*.svelte`): components highlighted with their blocks, tags, directives and runes,
  folded, outlined and completed, end tags added as you type, and checked by the Svelte Language
  Server ([Svelte](#svelte)).
- **Nuxt** projects: offered with Vue when the project is one, with run configurations for `nuxi`,
  `nuxi prepare` when `.nuxt/` is missing and templates for pages, layouts, components,
  composables and server routes ([Nuxt](#nuxt)).
- **React** and **Next.js** projects: offered when the project is one (Next.js with React), with
  `vite` run configurations in a Vite project, `next dev`, `build`, `start` and `lint` in a Next.js
  project, and templates for components, hooks, contexts and the App and Pages Routers
  ([React](#react), [Next.js](#nextjs)).
- **SvelteKit** projects: offered with Svelte, with `vite` run configurations, `svelte-kit sync`
  when `.svelte-kit/` is missing and templates for the route files ([SvelteKit](#sveltekit)).
- **ESLint**, **Prettier** and **Biome**: offered when the project uses them, problems and quick
  fixes beside the language server's, Reformat Code and fixes on save with the project's own tools
  ([Linters and formatters](#linters-and-formatters)).
- **npm scripts** of any `package.json`: run icons next to the scripts and **npm** run
  configurations, with the project's package manager, dependencies under External Libraries, and
  the npm tool window ([npm scripts](#npm-scripts)).
- **Node.js**: run and debug files, Jest and Vitest with the test tree ([Node.js](#nodejs)).
- **JSON**, **YAML**, **Markdown**, **Docker**, `.env`, **Shell** and **HTTP request files** come
  with every Zen IDE, WebWave included.

Highlighting, folding and the first completion work from the text of the file; the language
servers of TypeScript, Vue and Svelte add the rest. Debugging with Node.js, other frameworks and linters
come later, each as a plug-in that every Zen IDE can install.

## TypeScript and JavaScript

`.ts`, `.mts`, `.cts`, `.tsx`, `.js`, `.mjs`, `.cjs` and `.jsx` files get, from the TypeScript
Language Server:

- problems as you type, TypeScript's (and JavaScript's with `checkJs` or `// @ts-check`);
- completion from types (members after `.`, imports), parameter info (⌘P) and quick
  documentation (F1) with the JSDoc;
- **Go to Declaration** (⌘B), **Type Declaration** (⇧⌘B), **Implementation** (⌥⌘B), **Find
  Usages** (⌥F7), usages highlighted at the caret, **Go to Symbol** (⌥⌘O), the **Call Hierarchy**
  (⌃⌥H) and the **Structure** tool window, across TypeScript and JavaScript files;
- **Rename** (⇧F6) across files, **Reformat Code** (⌥⌘L), **Optimize Imports** (⌃⌥O: unused
  imports removed, the rest sorted and merged) and context actions (⌥Enter).

The server installs nothing by itself: the types of a library come from the project's own
`node_modules` (`npm install` or **Install dependencies**), never from `@types` packages the server
would fetch from npm for a JavaScript file.

The server comes with every Zen IDE as a declaration, not as a program: the first `.ts` or `.js`
file you open shows a banner with **Install**, which installs `typescript-language-server@5.3.0`
and `typescript@5` with npm into the IDE's own folder (`language-servers/typescript-language-server`
in its data folder), never among your global packages; Node.js with npm must be installed. The IDE
never installs it without that click; **Don't ask again** stops the offer. A server you installed
yourself on your `PATH` is used as it is ([Language servers](17-language-servers.md)).

The server checks with your project's TypeScript (`node_modules/typescript` in the project or a
folder above it, version 5 or 6), otherwise with the TypeScript 5 installed with it. A project on
TypeScript 7 (the native compiler) is checked with that TypeScript 5: TypeScript 7 has no
`tsserver` the server could use. TypeScript plug-ins your `tsconfig.json` names
(`"compilerOptions": { "plugins": [{ "name": "next" }] }`) are loaded when the server runs the
project's own TypeScript (in `node_modules` at the project's root), not with the TypeScript 5
installed with the server.

### Colours

The code is coloured like this: function and method calls in the colour of function
names (`test()`, `console.warn(…)`, `new Error(…)`, `Object.assign(…)`), globals and the module's
top variables (`console`, `window`, a top-level `const`) purple and italic, properties purple where
they are declared (a key of an object, a member of a type, a class field), and local variables,
parameters and properties in use (`options.statusCode`) in the text colour. The editor's own reader
does this without the server; with the server running, its semantic tokens make it exact. A theme
sets the colours with the `property` and `globalVariable` keys ([Themes](21-appearance.md)).

In `.jsx` and `.tsx` files (and JSX in `.js` files), typing the `>` of a start tag adds its end tag
after the caret: `<main className="app">` gives `<main className="app">|</main>`, `<>` gives
`<>|</>`, unless the element is closed already. Generics and casts such as `<T>`, `<T,>` and
`<string>` are left alone. **Settings → Editor → Smart Keys: HTML/CSS → Insert closing tag on tag
completion** turns it off, for HTML too. In a Vue or Nuxt project, `.vue` files stay with the Vue Language
Server and the scripts go to this one.

## Vue

The Vue plug-in serves `*.vue` single-file components; the IDE offers it in a banner when a `.vue` file opens (**Install**), in WebWave as in every other Zen IDE, and it is in the Marketplace. `.vue` files have their own icon, a green **V**.

- **Highlighting, block by block.** The `<template>` is HTML with Vue's syntax: directives
  (`v-if`, `v-for`, `v-model`…) and their shorthands (`:prop`, `@event`, `#slot`) stand out from
  ordinary attributes, with their arguments, `[dynamic]` arguments and `.modifiers`; the values of
  directives and the `{{ }}` interpolations are highlighted as JavaScript, or as TypeScript when the
  component's script is. Components stand out from HTML elements in the colour of types: tags
  that start with a capital letter (`<MyButton>`) or name one through a dot (`<Form.Field>`), and
  Vue's own `component`, `transition`, `transition-group`, `keep-alive`, `teleport` and `suspense`;
  a component written in kebab case (`<my-button>`) looks like any custom element. The `<script>` is JavaScript, or TypeScript with `lang="ts"` (`setup` or
  not); the `<style>` is CSS, or SCSS with `lang="scss"` (`scoped` and `module` or not). Blocks in
  other languages (Pug, Sass, Less, Stylus) and custom blocks are shown as plain text.
- **Folding and Structure.** The blocks, the template's elements, the script's blocks and the
  style's rules fold; the **Structure** tool window lists the blocks (`template`,
  `script setup lang="ts"`, `style scoped`…) with the template's elements, the script's functions,
  classes and variables, and the style's rules inside.
- **Completion without a server.** In the template's tags, the directives after `v`; after `<`
  outside the blocks, the blocks themselves (`template`, `script setup lang="ts"`, `style scoped`…);
  in the script and the style, what the JavaScript, TypeScript, CSS and SCSS plug-ins offer. Colours
  in the style and in `style` attributes get a preview in the gutter.
- **New → Vue Single File Component** creates a component with a `<script setup lang="ts">`, a
  `<template>` and a `<style scoped>` block (**Settings → Editor → File and Code Templates**).

### The Vue Language Server

With the Vue Language Server, `.vue` files get problems as you type (TypeScript's, in the script and
in the template's expressions), completion from the component's types (the script's names in the
template, props, the members of a value after `.`), **Go to Declaration** (⌘B, also from the
template to the script and to other files), quick documentation (F1), **Rename** (⇧F6, in the
script and the template together) and **Reformat Code** (⌥⌘L).

Installing the Vue plug-in installs the server in the same step (from the Marketplace, or with
**Install** in the banner the first `.vue` file shows): `@vue/language-server@2.2.12` and
`typescript@5` go with npm into the IDE's own folder for the server,
`language-servers/vue-language-server` in its data folder, and a notification says when it is
ready. Node.js with npm must be installed; without it a notification says where to get it, and the
server's own banner offers **Install** again later. Your global npm packages are never touched
([Language servers](17-language-servers.md#installing-a-server)). The server checks with the
project's own TypeScript when the project has one (`node_modules/typescript` in the project's folder
or a folder above it, version 5 or 6), otherwise with the TypeScript 5 installed with it;
TypeScript 7 has no JavaScript interface the server could use, so a project's TypeScript 7 is passed
over. For the template's types the project needs Vue itself in `node_modules` (`npm install`).

`.ts` files that import `.vue` components read them too: installing the Vue plug-in adds
`@vue/typescript-plugin` to the TypeScript server's own folder, so that the components' props are
typed and completed and **Go to Declaration** opens the `.vue` file.

The plug-in runs the server's version 2 in its full mode, where one server answers for the
template, the script and the style. Version 3 of the Vue Language Server needs the TypeScript
server running beside it on the same files, which the IDE cannot do yet.

## Nuxt

The Nuxt plug-in is for Nuxt projects; it is not bundled with any IDE. When you open a project
with `nuxt.config.ts` (or `nuxt.config.js`, `nuxt.config.mjs`) at its root, or with `nuxt` among
the `dependencies` or `devDependencies` of its `package.json`, a notification offers it:
**Install plugin Nuxt** installs it together with the Vue plug-in, which it needs, and **Don't
suggest** stops offering it for projects. It is also in the Marketplace, where installing it brings
Vue along too.

Nuxt has no language server of its own: the Vue Language Server serves its `.vue` files
([The Vue Language Server](#the-vue-language-server); restart the IDE after installing so that it
starts), and Nuxt's auto-imports (`ref`, `useFetch`, your components and composables…) reach it
through TypeScript, from the `.nuxt/` folder Nuxt generates, which the project's `tsconfig.json`
extends. Once installed, in a Nuxt project:

- **Run configurations.** `nuxi dev`, `nuxi build`, `nuxi generate` and `nuxi preview` are added to
  the project's run configurations, once: a notification lists them, and one you delete does not
  come back. Each runs `nuxi` from the project's packages with the project's package manager
  (`npx --no nuxi dev`, `pnpm exec nuxi dev`, `yarn run nuxi dev` or `bun run nuxi dev`); change
  them like any other in **Run → Edit Configurations…**.
- **`nuxi prepare`.** While the project has no `.nuxt/` folder, a notification offers to run
  `nuxi prepare`, once per project; its output shows in the **Run** tool window. Later, run it with
  **Run Project Setup** in **Find Action** (⇧⌘A), or in the terminal. `nuxi dev` and an install
  whose `postinstall` runs `nuxt prepare` write `.nuxt/` too. Run `npm install` (or your package
  manager's install) first: `nuxi` comes with the `nuxt` package, and nothing is downloaded.
- **File templates.** **New** offers **Nuxt Page**, **Nuxt Layout** and **Nuxt Component** (`.vue`
  files) and **Nuxt Composable** and **Nuxt Server Route** (`.ts` files); create them in `pages/`,
  `layouts/`, `components/`, `composables/` and `server/api/`.
- **`nuxt.config.*`** has its own icon, a green **N**, and is edited as the TypeScript or
  JavaScript file it is.

## Svelte

The Svelte plug-in serves `*.svelte` components; the IDE offers it in a banner when a `.svelte`
file opens (**Install**), and it is in the Marketplace. `.svelte` files have their own icon, an
orange **S** in a circle.

- **Highlighting.** The markup is HTML with Svelte's syntax: the logic blocks (`{#if}`,
  `{:else if}`, `{:else}`, `{/if}`, `{#each … as …}`, `{#await}`, `{:then}`, `{:catch}`, `{#key}`,
  `{#snippet}`) and tags (`{@html}`, `{@const}`, `{@render}`, `{@debug}`), the directives (`on:`,
  `bind:`, `class:`, `style:`, `use:`, `transition:`, `in:`, `out:`, `animate:`) with their
  arguments and `|modifiers`, component tags, and every `{expression}` as JavaScript, or as
  TypeScript when the script is. The `<script>` is JavaScript, or TypeScript with `lang="ts"`
  (`module` or `context="module"` too), with the runes (`$state`, `$derived`, `$effect`, `$props`,
  `$bindable`) set apart; the `<style>` is CSS, or SCSS with `lang="scss"`.
- **Folding and Structure.** Elements, logic blocks, the script and the style fold; the
  **Structure** tool window lists the script with its declarations, the markup's elements and
  blocks, and the style's rules.
- **Typing.** After the `>` of a start tag its end tag is added, after `</` the open element's
  name, and after the `}` of `{#if …}` (or another block) its `{/if}`.
- **Completion without a server.** The blocks after `{#`, their branches after `{:`, the tags after
  `{@`, the open block's end after `{/`; the directives in a tag; the runes after `$` in the script;
  `<script lang="ts">` and `<style>` after `<` outside the markup; and in the script and the style
  what their own plug-ins offer. Colours in the style and in `style` attributes get a preview.
- **New → Svelte Component** creates a component with a `<script lang="ts">` taking `$props()`,
  an element and a `<style>`.

### The Svelte Language Server

Installing the plug-in installs the Svelte Language Server in the same step:
`svelte-language-server@0.18.5` and `typescript@5` go with npm into the IDE's own folder for it
(Node.js with npm needed). With it, `.svelte` files get problems as you type (Svelte's and
TypeScript's), completion (the script's names in the markup, props, the members of a value), quick
documentation, **Go to Declaration** (from the markup to the script and to other files), **Find
Usages**, **Rename** in the script and the markup together, and **Reformat Code** with Prettier and
its Svelte plug-in. The server takes `svelte` and Prettier from your project when it has them, and
checks with the TypeScript installed beside it.

`.ts` and `.js` files of a Svelte project stay with the TypeScript Language Server, and the Svelte
plug-in teaches it to read `.svelte` imports: installing the plug-in adds `typescript-svelte-plugin`
to the TypeScript server's own folder, with nothing to set up in your project. In a `.ts` file
that imports a component, its props are typed, checked and completed, **Go to Declaration** opens
the `.svelte` file and **Find Usages** reaches it; renaming a prop in the component also renames it
where `.ts` files pass it. If the TypeScript server and the Svelte plug-in were installed before
the IDE could do this, the banner above a `.ts` file of a Svelte project (one whose
`package.json` lists `svelte`) offers **Install** for the missing plug-in.

## React

The React plug-in is for React projects, offered when you open a project with `react` among the
`dependencies` or `devDependencies` of its `package.json` (**Install plugin React**, or **Don't
suggest**); it is not bundled with any IDE. React has no language server of its own: `.jsx` and
`.tsx` files are served by the TypeScript Language Server ([TypeScript and
JavaScript](#typescript-and-javascript)), with JSX end tags as you type. The plug-in adds:

- **Run configurations for Vite.** In a Vite project (`vite.config.*` at the root, or `vite` in
  `package.json`), the configurations `vite`, `vite build` and `vite preview` are added once, run
  with the project's package manager. Other React projects get none: run their `package.json`
  scripts from the run icons next to them ([npm scripts](#npm-scripts)).
- **File templates.** **New → React Component** (`.tsx`) and **React Component (JSX)** (`.jsx`)
  create a function component named as you type it; **React Hook** (`.ts`) a hook with a state;
  **React Context** (`.tsx`) a context with its provider component and its `use…` hook.

## Next.js

The Next.js plug-in is for Next.js projects, offered when you open a project with
`next.config.js`, `next.config.mjs` or `next.config.ts` at its root, or with `next` in its
`package.json`. It needs the React plug-in, which installs with it: the offer names both, and React
is not offered on its own then. Once installed:

- **Run configurations** `next dev`, `next build`, `next start` and `next lint` are added once to
  the project's run configurations, run with the project's package manager (`npx --no next dev`,
  `pnpm exec next dev`…).
- **File templates for the App Router** create the file with its fixed name in the folder you
  choose, so create the route's folder first (`app/blog`): **Next.js Page** (`page.tsx`),
  **Next.js Layout** (`layout.tsx`), **Next.js Route Handler** (`route.ts`, a `GET` handler),
  **Next.js Loading UI** (`loading.tsx`), **Next.js Error UI** (`error.tsx`, a client component
  with **Try again**) and **Next.js Not Found** (`not-found.tsx`). For the Pages Router, **Next.js
  Pages Router Page** and **Next.js API Route** ask for the file's name.
- **`next.config.*`** has its own icon, a grey **N**, and is edited as the JavaScript or TypeScript
  file it is.
- **`next-env.d.ts`**, which Next.js writes and rewrites for TypeScript, has the same icon and is
  taken for generated code: **Show Usages** leaves its results out when **Show Results In
  Generated Code** is off. It is an ordinary declaration file for the TypeScript Language Server,
  which reads Next's types from it and reports nothing in it; it is not made read-only, since
  Next.js rewrites it, and editing it is pointless rather than harmful.
- **Next's own TypeScript checks.** The `tsconfig.json` Next.js writes names its TypeScript
  plug-in, which checks the App Router's conventions (what a `page.tsx` or `layout.tsx` may
  export, `'use client'`, the values of `dynamic` or `revalidate`) and completes and documents
  them. The TypeScript Language Server loads it when the project has its own TypeScript in
  `node_modules`, which `npm install` gives a Next.js project with TypeScript; with the TypeScript
  installed with the server those checks are missing.

## SvelteKit

The SvelteKit plug-in is for SvelteKit projects, offered when you open a project with
`@sveltejs/kit` among the `dependencies` or `devDependencies` of its `package.json` (a Svelte
project without SvelteKit is not one, even with `svelte.config.js`). It needs the Svelte plug-in,
whose Svelte language server serves the `.svelte` files: the offer names both, **Install plugin
SvelteKit** installs both, and Svelte is not offered on its own then. Once installed, in a SvelteKit
project:

- **Run configurations** `vite dev`, `vite build` and `vite preview` are added once, run with the
  project's package manager (`npx --no vite dev`, `pnpm exec vite dev`…).
- **`svelte-kit sync`.** While the project has no `.svelte-kit/` folder, a notification offers to run
  `svelte-kit sync`, once per project, with its output in the **Run** tool window; later, **Run
  Project Setup** in **Find Action** (⇧⌘A). It writes the `tsconfig.json` your project's extends and
  the types of every route (`./$types`), which the language servers need; `vite dev` and an install
  whose `prepare` script runs it write them too. Run your package manager's install first.
- **File templates for routes** create the file with its fixed name in the folder you choose
  (create the route's folder first, `src/routes/blog`): **SvelteKit Page** (`+page.svelte`),
  **SvelteKit Layout** (`+layout.svelte`), **SvelteKit Page Load** (`+page.ts`), **SvelteKit Page
  Server Load** (`+page.server.ts`), **SvelteKit Layout Load** (`+layout.ts`), **SvelteKit Layout
  Server Load** (`+layout.server.ts`), **SvelteKit Server Route** (`+server.ts`), **SvelteKit
  Error Page** (`+error.svelte`), and **SvelteKit Server Hooks** (`hooks.server.ts`, in `src`). They
  are written for Svelte 5 and SvelteKit 2.
- **`svelte.config.*`** has its own icon, an orange **S**, in Svelte projects too.

## Linters and formatters

Your project's own ESLint, Prettier and Biome work in the editor, each as a plug-in that the IDE
offers when the project uses the tool and installs in one step; none comes with an IDE. Settings →
Languages & Frameworks → **Code Quality Tools** shows them.

- **ESLint** is offered when the project has an ESLint configuration (`eslint.config.*`,
  `.eslintrc*`) or `eslint` in `package.json`. Its problems appear as you type in `.js`, `.ts`,
  `.jsx`, `.tsx`, `.vue` and `.svelte` files, next to those of the file's own language server;
  ⌥Enter offers its quick fixes (**Fix this … problem**, **Disable … for this line**, **for the
  entire file**, **Fix all auto-fixable problems**), and **Fix Problems with Linters**
  (`editor.code.fixAll`) fixes the whole file, as Fix ESLint Problems. ESLint itself comes from your
  project's `node_modules`; the ESLint language server is installed with the plug-in into the IDE's
  own folder.
- **Prettier** is offered when the project has a Prettier configuration (`.prettierrc*`,
  `prettier.config.*`) or `prettier` in `package.json`. **Reformat Code** (⌥⌘L) then formats
  scripts, JSON, CSS, SCSS, Less, HTML, Vue, Svelte, Markdown, YAML and GraphQL files with your
  project's Prettier and its configuration, and leaves the files of `.prettierignore` alone.
- **Biome** is offered when the project has `biome.json`. Your project's Biome checks scripts,
  JSON, CSS and GraphQL as you type, offers its quick fixes with ⌥Enter and its safe fixes with
  Fix Problems with Linters, and formats them with Reformat Code. Biome runs from your project's
  `node_modules` (`npm install` first).

On the **Code Quality Tools** page each linter is **Automatic configuration** or **Disable**, with
**Run … fixes on save**; each formatter has **Run on Reformat Code action** (on by default) and
**Run on save**. **Settings → Tools → Actions on Save** links to the page. The order on save is the
linters' fixes, Optimize Imports, then the formatting (Prettier when it is on, instead of the
language server's Reformat Code).

## npm scripts

Every Zen IDE runs the scripts of a `package.json`: a green run icon next to each script in the
gutter runs it (`npm run dev`), and **Run → Edit Configurations…** has **npm** configurations with
the `package.json` (the command runs in its folder), the command (**run** a script, **exec** a
program of the project's packages, **install**), the script or program, the arguments and the
package manager. **auto** uses the one `packageManager` in `package.json` names, else the one whose
lock file is there (`pnpm-lock.yaml`, `yarn.lock`, `bun.lock`), else npm. The package manager and
Node.js are looked for on your `PATH` and where they are usually installed (Homebrew, nvm, Volta,
`~/.bun/bin`…); install Node.js from https://nodejs.org if the IDE says it cannot find npm.

The dependencies of `package.json` are under [External Libraries](05-project.md#external-libraries)
in the Project tool window, and `node_modules` stays out of searches.

### The npm tool window

The **npm** plug-in adds the npm tool window at the right (View → Tool Windows → npm). WebWave comes
with it; every other IDE offers it in a notification when a project with a `package.json` opens, or
from Settings → Plugins. It lists the `package.json` of the project, of its workspaces
(`workspaces` in `package.json`, `pnpm-workspace.yaml`) and of the folders at the project's root,
each with the package manager it uses and its scripts.

- Double-click a script, or select it and press **Run**, to run it with its output in the Run tool
  window. Double-click a package to open its `package.json`.
- **Install Dependencies** runs `npm install`, `pnpm install`, `yarn install` or `bun install` for
  the selected package, or the first one. It is the only action here that uses the network, and
  only when you press it.
- **Toggle Offline Mode** keeps installs off the network, for this project (`.zen/npm.json`): they
  use the package manager's cache and fail if something is not there. Bun has no such mode, so an
  install with Bun is refused while it is on. Corepack fetches no package manager while it is on,
  for scripts too. An AI agent can turn it on but not off.

In safe mode the scripts are listed and nothing runs: scripts and installs are refused until you
trust the project. Reading `package.json` and `node_modules` never runs anything.

For agents, every action is a command: `npm.project.get`, `npm.script.run`,
`npm.dependencies.install` and `npm.options.set`. The tool window is `view.toolWindow.npm`, and
`get_window_state` lists its rows (`buildTools.npm.rows`), which `view.buildTool.select`, `.toggle`
and `.run` act on with `tool: "npm"`.

## Node.js

The **Node.js** plug-in runs and debugs JavaScript and TypeScript with Node.js, and runs your Jest
and Vitest tests. WebWave comes with it; every other IDE offers it in a notification when a project
with a `package.json` opens, or from Settings → Plugins. It needs Node.js on your computer, found on
your `PATH` and where it is usually installed, and it downloads nothing.

- **Run a file:** ⌃⇧R in a `.js`, `.mjs`, `.cjs`, `.ts`, `.mts` or `.cts` file runs it. A **Node.js**
  configuration (Run → Edit Configurations…) has the file, the Node options, the interpreter and the
  arguments. TypeScript files run the way your Node.js runs them: recent versions take them directly.
- **Debug:** ⌃⇧D debugs the same configuration. Breakpoints, conditions, log points, stepping,
  variables, evaluation and stopping at exceptions work in the Debug tool window
  ([Debugging](16-debugging.md)). The IDE talks to Node.js itself, on this computer only. Only the
  main thread is debugged, and source maps are not read: code compiled by a build is debugged as
  compiled.
- **Tests:** test files (`*.test.*`, `*.spec.*`, files in `__tests__`) get run icons next to
  `describe`, `it` and `test`, and **Node.js Tests** configurations run a file, a test, or the whole
  project with Jest or Vitest, the runner of your project in `node_modules`, never fetched. The
  results tree fills as tests finish, and **Rerun Failed** runs the failed ones again. Titles that
  are computed have no icon; run their file. Debugging a test runs its runner in one process (Jest)
  or one worker (Vitest).

In safe mode nothing of this starts until you trust the project. Agents use the same `run.*` and
`debug.*` commands, with the kinds `node` and `node.test`.

## Built-in plug-ins

Go and Rust, the built-in plug-ins of every Zen IDE, are off in WebWave. If a project of yours has
Go or Rust too, switch them on in **Settings → Plugins → Built-in** and restart WebWave
([Built-in plug-ins](10-plugins.md#built-in-plug-ins)).

## AI agents

WebWave's MCP server for AI agents listens on port **63532** when it is on (**Settings → Tools →
MCP Server**), so that it can run beside GoGarden (63530) and RustRonin (63531); see
[AI agents](ai-agents.html).
