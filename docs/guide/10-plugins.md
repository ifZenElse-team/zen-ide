# Plug-ins

Plug-ins add support for more file types and kinds of work. There are two kinds:

- **Built-in plug-ins** are part of every Zen IDE: the large ones, such as **Go** (everything for Go:
  gopls, run, test and debug, go.mod checks, the Go SDK) and **Rust**. Each IDE has its own on from
  the start (GoGarden has Go, RustRonin Rust; WebWave, whose web support is plug-ins, has none) and
  the others off; you switch any of them on in **Settings → Plugins
  → Built-in**, and it works after a restart (see [Built-in plug-ins](#built-in-plug-ins)).
- **Plug-ins from the Marketplace** are small WebAssembly modules, signed by the publisher, that run
  inside GoGarden without access to your files or the network.

A plug-in can add project types to [New Project](05-project.md#new-project), under **Generators**. One
that needs the network asks the IDE to fetch what it needs, only while you use it in the wizard, and
the wizard says so.

Built in, in every IDE: Markdown, YAML, JSON, TOML, Docker (Dockerfiles and Compose files), GitLab
CI, ignore files (`.gitignore` and similar), `.env` files, Shell scripts, and basic web: HTML, CSS,
JavaScript (with JSX) and TypeScript (with TSX), and XML. WebWave comes with SCSS too, and GoGarden,
RustRonin, WebWave, SqlScroll and DevopsDaruma, which have the Database workspace on, with SQL. More, such as SQL in the other IDEs,
Nuxt, React, Next.js, SvelteKit, and SCSS and Vue in the other IDEs, can be installed in
**Settings → Plugins → Marketplace**, from the notification shown above a file that a plug-in
supports, or from the notification shown when you open a project that a plug-in supports (a Nuxt
project offers the Nuxt plug-in). A plug-in that needs another one installs it too: installing Nuxt
installs Vue, installing Next.js installs React, and installing SvelteKit installs Svelte; a project that both would suit is offered the
one that brings the other.

Every Zen IDE comes with these and, on top of them, the plug-ins of its own kind of work. Every
plug-in works in every Zen IDE: one that another IDE comes with is installed here from the
Marketplace like any other. **Settings → Plugins → Installed** lists the plug-ins in two groups,
**Downloaded** (installed by you) and **Bundled with this IDE** (marked **Bundled**, also in the
Marketplace); a bundled plug-in can be switched off, not uninstalled.

Installed plug-ins update themselves when GoGarden starts, if a newer version is published.

## Settings → Plugins

The page has three tabs, **Marketplace**, **Installed** and **Built-in**, a search field above the
list and, on the right, the details of the plug-in selected in the list.

- **Marketplace** lists every plug-in of the catalog, sorted by name, with its newest version and
  **Install**, **Update** (a newer version of one you have) or **Installed**. Opening it fetches
  the catalog; when it cannot be loaded, **Try again** asks once more.
- **Installed** lists your plug-ins in **Downloaded** and **Bundled with this IDE**, each with its
  **Enabled** box. It does not go online by itself: **Check for Updates** fetches the catalog, and
  then each plug-in with a newer version shows **Update**, and **Update All** above the list updates
  them all. If a check fails, for example offline, the page says so and keeps what the last check
  found.
- The search matches the name, the description and the file types; several words must all match.
- The **details** show the name, the installed version (and the newer one, if any), **Install**,
  **Update**, **Enable** or **Disable** and **Uninstall**, the description, the file types and
  **What's New**: the release notes of the newest version and of the one you have, shown as text:
  links in them are not followed and pictures are not loaded.

**Uninstall** asks first; the `plugins.plugin.uninstall` command does not. A bundled plug-in has no
**Uninstall**, only **Disable**; an update you installed over it is
removed with the `plugins.plugin.uninstall` command, which brings the bundled version back.
Installing, updating, switching on and off take effect at once. A plug-in that brings language
servers says so, and **installing it installs them too**, in the background, in the same step: a
server that is already there (the IDE's own copy, one on your `PATH`, or a path you set) is kept,
and a missing one is installed into the IDE's own folder for it, never among your global packages.
A notification says when the server is ready. When the tool that installs it is missing (Node.js
with npm for most servers), one notification says what to download and where; the server can then
be installed later from the banner above its files or from **Settings → Languages & Frameworks →
Language Servers** ([Language servers](17-language-servers.md)). A new plug-in's servers work at
once; updating, uninstalling or switching off a plug-in with servers changes them after a restart,
and the page reminds you to. Uninstalling a plug-in removes the copies of its servers that the IDE
installed (not one on your `PATH`).

**Plug-ins that run programs.** A plug-in may offer run icons in its files, such as the Docker
plug-in's beside each Compose service. It can only run the programs it declares, and its details
list them (**Runs these programs from its targets: docker**). What runs is a run configuration of
the kind **Plug-in target**: Run/Debug Configurations shows its program and arguments, which go to
the program as they are, never through a shell; a project you have not trusted runs none of them.
Below the list, **Agents may run its targets** says what AI agents may do: **No** (the default),
**Ask me each time** (a notification asks you, and **Allow once** lets that one start), or **Yes**.
Only you change it; agents can always see the targets and configurations.

A plug-in may also have tool windows over those programs, such as the Docker plug-in's list of
containers; its details name them (**Its tool windows, which run these programs: Docker**). The same
setting decides what agents may do there (start, stop, open a shell); anything that cannot be undone,
such as removing or pruning, asks you every time, and nothing in them runs in a project you have not
trusted.

A plug-in's tool window has an icon of its own on the tool window bar, in your theme's colours, and
opens on the side the plug-in chooses (Docker and Kubernetes open on the right). Drag it anywhere:
it stays where you put it, and **Restore Default Layout** puts it back on its side. If you had one
of them somewhere else before, it stays there and the IDE tells you once. **View | Tool Windows**
lists the plug-ins' windows after the built-in ones.

A tool window may also have choices in its header, such as the Kubernetes window's **Context** and
**Namespace** ([Kubernetes](34-kubernetes.md)): only you change them, every command the window runs
passes them, and its changes ask you in words that name them.

Agents do the same with the `plugins.*` commands ([AI agents](11-ai-agents.md)).

## Built-in plug-ins

**Settings → Plugins → Built-in** lists the plug-ins built into every Zen IDE, each with what it
gives, whether this IDE has it on from the start (**On by default in GoGarden**) and its **Enabled**
box; the details on the right have **Enable** or **Disable**. A built-in plug-in cannot be
uninstalled.

A change applies when the IDE restarts: the plug-in is marked **Turns on after a restart** (or off),
and a banner above the list offers **Restart IDE**, which applies the Settings dialog, saves your
files and opens the same projects again. **Restart IDE** is also in Find Action (⇧⌘A).

- **Go** brings everything for Go, as GoGarden has it: highlighting of Go, `go.mod` and `go.sum`,
  Go modules and the Structure, gopls (completion, navigation, problems, rename, formatting), run and
  test configurations with Delve for debugging, `go.mod` updates and vulnerabilities, the live
  templates, **Settings → Languages & Frameworks → Go** (GOROOT, gopls, Delve) and **Editor → Code
  Style → Go**. Switched on in RustRonin, for example, it gives the same Go support there; switched
  off, all of it is gone after the restart, its settings page included.
- **Rust** highlights Rust; rust-analyzer, Cargo and run configurations come later.
- **Database** gives the [Database workspace](28-database.md): the **Database** button on the right
  tool window bar and SQL consoles; connections and data editors come later. It is on from the start
  in GoGarden, RustRonin, WebWave, SqlScroll and DevopsDaruma, and off in JvmJizo.

The basic web support (HTML, CSS, JavaScript, TypeScript) and the other plug-ins of every IDE are not
part of these: they stay in **Installed**.

Your choices are kept for this IDE only, in `modules.json` in its data folder. Agents list the
built-in plug-ins with `plugins.modules.list` and switch them with `plugins.module.enable` and
`plugins.module.disable`; `view.ide.restart` restarts the IDE.

## Areas of work

Some IDEs come with plug-ins for several areas of work and switch on only those you work in, so
that the IDE loads only what you use (TestTotami, for example, with E2E tests, API tests, load
testing and test management). On such an IDE's first start a small window asks **What do you work
on?**: each area has a checkbox and a line about what it gives. Tick the ones you need and click
**Continue**, or **Skip** to leave them all off; you are asked only once. Most areas are switched on
at once; a built-in plug-in needs a restart, which the window says and **Continue** does for you.

Change the choice at any time in **Settings → Plugins → Built-in**: **Areas of work** above the list
names the areas that are on, and **Choose Areas…** opens the same window. Each area is also a plug-in
of its own, switched on or off with the others (on **Built-in** or **Installed**). GoGarden has no
areas today: everything it ships is on from the start.

Agents list the areas with `plugins.areas.list`, choose them with `plugins.areas.choose`, skip the
choice with `plugins.areas.skip` and show the window with `plugins.areas.show`.

Besides colouring a file, a plug-in can fold its blocks (the arrows next to the line numbers,
⌘− and ⌘=, see [The editor](02-editor.md)), show its structure in the **Structure** tool window
(⌘7), suggest what to type in the completion popup, and expand abbreviations. The popup opens as
you type a name or `.`, and with ⌃Space anywhere (right after `<`, `@`, `:` or `&`, for example);
Enter inserts the suggestion, Tab replaces the rest of the word, `-` included.

Style sheets show a **colour preview** in the gutter: a small square, next to the line number, in
the colour of the first colour literal on the line (`#f80`, `#ff8000`, `#ff800080`, `rgb()`,
`rgba()`, `hsl()`, `hsla()` and named colours such as `rebeccapurple`). Click it to open a colour
picker; Enter or a click outside it writes the new colour into the literal, in the literal's own
notation (a named colour becomes `#rrggbb`), and Escape keeps the old one. A run icon or the bulb
on the same line takes the place of the square.

## HTML

The HTML plug-in serves `*.html`, `*.htm` and `*.xhtml` files:

- **Highlighting** of tags, attribute names and values, character references (`&amp;`), comments and
  the doctype. The code inside `<script>` and `<style>` stays plain text.
- **Folding** of every element and comment that spans several lines, and the element tree in the
  **Structure** tool window, each element shown as a selector: `div#main.wide`.
- **Completion** of tag names after `<`, the open element's end tag after `</`, the attributes of the
  element (its own first, then the global ones, without those already written), the values of
  enumerated attributes such as `type`, `rel`, `target`, `method` or `dir`, and character references
  after `&`.
- **Emmet abbreviations**: type an abbreviation in text and press **Tab**. `ul>li.item$*3` becomes a
  list of three numbered items, and `!` an HTML5 page. Supported: element names, `#id`, `.class`,
  `[attr=value]`, `{text}`, `>` (child), `+` (sibling), `^` (up one level), `(…)` groups, `*N`
  (repeat), `$` numbering (`$$` pads, `$@-` counts down, `$@3` starts at 3), implicit names (`.x` is a
  `div`, `li` inside `ul`, `tr` inside `table`, `td` inside `tr`, `option` inside `select`, `span`
  inside inline elements) and short names such as `btn`, `bq`, `input:email`, `a:link`,
  `link:css` and `meta:vp`. The caret lands in the first empty attribute or element. A single plain
  word expands only when it is an HTML element or a short name (so a word such as `table` or `code`
  in text does expand), and text in braces alone (`{note}`) is left as it is; otherwise **Tab**
  inserts a tab. The indentation follows the file (tabs, or the smallest space indent, four by
  default).
  Tab expands only with a single caret and no selection; otherwise it indents as usual.
- **Closing tags as you type**: typing `>` of a start tag adds its end tag after the caret
  (`<div>` becomes `<div>|</div>`), unless the element is empty (`<br>`), self-closing (`<a/>`) or
  closed further on; typing `</` adds the name of the element that is open there and `>`. Undo
  takes the added text back first. Both work with a single caret and no selection; **Settings →
  Editor → Smart Keys: HTML/CSS → Insert closing tag on tag completion** turns both off.
- **Colour previews** for the `style` attributes and the `<style>` elements of the page.

## XML

The XML plug-in comes with every Zen IDE. It serves `*.xml`, `*.plist`, `*.xsd`, `*.xsl`,
`*.xslt`, `*.svg` (with its picture beside the text, [Text with a preview](02-editor.md#text-with-a-preview)), `*.wsdl`, `*.pom`, `*.csproj`,
`*.fsproj`, `*.vbproj`, `*.props`, `*.targets`, `*.resx`, `*.xaml`, `*.rss`, `*.atom`, `*.kml`,
`*.gpx`, `*.opml`, `*.dtd` and `*.iml` files, and **any file whose content is XML**, whatever its
name: one that starts with `<?xml`, with a doctype other than HTML's (`<!DOCTYPE plist …`), or with
a root element that declares a namespace (`<project xmlns="…">`). An Apple property list saved as
`timeweb-cloud.cyberduckprofile` is XML that way (**Settings → Editor → File Types** shows these
rules under **Recognized by content**; a pattern you add, or **Override File Type**, still wins).

- Highlighting of elements, attributes and their values, references (`&amp;`), comments, CDATA
  sections, the doctype, processing instructions (`<?xml …?>`) and namespace prefixes
  (`xsl:template`, `xmlns:xsi`).
- Folding of elements and comments that span lines, and the elements in the **Structure** tool
  window (with their `id` or `xml:id`: `item#one`).
- Closing tags as you type: `>` after a start tag adds its end tag, `</` adds the name of the
  element still open. Names keep their letter case, and no element is empty by itself as `<br>` is
  in HTML.
- Problems as you type, without a language server: an end tag that does not match the open element,
  an element that is never closed, an end tag without a start tag, a comment, CDATA section or tag
  that does not end.
- **New → XML File** starts a file with `<?xml version="1.0" encoding="UTF-8"?>`.

## CSS

The CSS plug-in serves `*.css` files: it highlights selectors, properties, values, at-rules,
comments and custom properties, folds rule and at-rule blocks and comments of several lines, shows
the rules in the **Structure** tool window (grouped under `@media`, `@supports`, `@layer`,
`@container` and the other grouping at-rules, keyframes under `@keyframes`), and completes
property names, their values, units after a number, at-rules after `@`, pseudo-classes after `:`
and pseudo-elements after `::`, custom properties (also inside `var()`), functions and
`!important`. Colour literals in values get a colour preview in the gutter.

## SCSS

The SCSS plug-in serves `*.scss` files; WebWave comes with it, the other IDEs install it from the
Marketplace. It does what the CSS plug-in does, and knows SCSS: `//` comments, `$variables` (with
`!default` and `!global`), nesting with `&`, `%placeholders`, `#{…}` interpolation, `@use`,
`@forward`, `@mixin`, `@include`, `@extend`, `@function`, `@return` and the control at-rules
(`@if`, `@else`, `@each`, `@for`, `@while`). Mixins and functions are highlighted as functions;
the **Structure** tool window lists the top-level variables, the mixins and functions, and the
rules; completion offers the Sass at-rules after `@`, the variables of the file after `$`, and the
mixins of the file after `@include`. Colour literals in values and variables get a colour preview.

## JavaScript and TypeScript

The JavaScript plug-in serves `*.js`, `*.mjs`, `*.cjs` and `*.jsx` files, the TypeScript plug-in
`*.ts`, `*.mts`, `*.cts` and `*.tsx`. Without a language server they highlight the code (JSX tags
and, in TypeScript, types, interfaces, enums, generics and decorators), fold blocks, literals,
comments, JSX elements and groups of imports, fill the **Structure** tool window (functions,
classes with their members, interfaces, types, enums, namespaces) and offer completion: keywords
that fit where you type, names declared in the file, members of `this` inside a class, and snippets
for `for`, `if`, `function`, `class` and arrow functions. In JSX, typing the `>` of a start tag
adds its end tag (`<div>` gives `<div>|</div>`); generics and casts such as `<T>` and `<string>`
are left alone.

Both plug-ins declare the TypeScript Language Server, for problems, completion from types,
navigation, usages, rename, formatting and imports in all eight kinds of files: the banner above
the first such file offers to install it, once, into the IDE's own folder
([TypeScript and JavaScript](06-web.md#typescript-and-javascript)).

## Vue

The Vue plug-in serves `*.vue` single-file components; the IDE offers it in a banner when a `.vue` file opens (**Install**), in WebWave as in every other Zen IDE, and it is in the Marketplace. It highlights the template as HTML with Vue's directives,
shorthands (`:`, `@`, `#`) and `{{ }}` interpolations, their expressions as JavaScript or
TypeScript, the script as JavaScript or TypeScript and the style as CSS or SCSS by their `lang`;
folds the blocks, elements, script and style; lists the blocks with their elements, declarations
and rules in the **Structure** tool window; completes directives, the blocks after `<` and, in
the script and the style, what their own plug-ins complete; shows colour previews; and adds the
**Vue Single File Component** file template. It declares the Vue Language Server for problems,
completion from types, navigation, rename and formatting ([Vue](06-web.md#vue)).

## Svelte

The Svelte plug-in serves `*.svelte` components; the IDE offers it in a banner when a `.svelte` file
opens, and it is in the Marketplace. It highlights the markup with Svelte's logic blocks, tags,
directives and `{expressions}` (as JavaScript or TypeScript), the script with its runes and the
style as CSS or SCSS by their `lang`; folds elements, blocks, the script and the style; fills the
**Structure** tool window; completes blocks, directives and runes; adds end tags and `{/if}` as you
type; shows colour previews; and adds the **Svelte Component** file template. Installing it
installs the Svelte Language Server for problems, completion, navigation, usages, rename and
formatting ([Svelte](06-web.md#svelte)).

## ESLint, Prettier and Biome

These plug-ins bring the project's own code quality tools; the IDE offers each when the project
uses it, and installs the ESLint language server with the ESLint plug-in. ESLint and Biome check
files beside their language server and fix them; Prettier and Biome format with Reformat Code; all
can run on save. See [Linters and formatters](06-web.md#linters-and-formatters).

## Nuxt

The Nuxt plug-in is for Nuxt projects and is offered when you open one: a project with
`nuxt.config.ts`, `nuxt.config.js` or `nuxt.config.mjs` at its root, or with `nuxt` in the
dependencies of its `package.json`. It installs with the Vue plug-in, whose Vue Language Server
serves Nuxt's `.vue` files. In a Nuxt project it adds the run configurations `nuxi dev`,
`nuxi build`, `nuxi generate` and `nuxi preview`, offers to run `nuxi prepare` while `.nuxt/` is
missing, and gives **New** templates for a page, a layout, a component, a composable and a server
route ([Nuxt](06-web.md#nuxt)).

## React

The React plug-in is offered for a project with `react` in its `package.json`. In a Vite project it
adds the run configurations `vite`, `vite build` and `vite preview`, and **New** gets templates for
a component (TSX or JSX), a hook and a context; `.jsx` and `.tsx` files are the JavaScript and
TypeScript plug-ins' ([React](06-web.md#react)).

## Next.js

The Next.js plug-in is offered for a project with `next.config.*` at its root or `next` in its
`package.json`, and installs with React. It adds the run configurations `next dev`, `next build`,
`next start` and `next lint`, and **New** templates for the App Router (`page.tsx`, `layout.tsx`,
`route.ts`, `loading.tsx`, `error.tsx`, `not-found.tsx`) and the Pages Router
([Next.js](06-web.md#nextjs)).

## SvelteKit

The SvelteKit plug-in is offered for a project with `@sveltejs/kit` in its `package.json`, and
installs with the Svelte plug-in. It adds the run configurations `vite dev`, `vite build` and
`vite preview`, offers to run `svelte-kit sync` while `.svelte-kit/` is missing, and gives **New**
templates for the route files (`+page.svelte`, `+layout.svelte`, `+page.ts`, `+page.server.ts`,
`+layout.ts`, `+layout.server.ts`, `+server.ts`, `+error.svelte`) and `hooks.server.ts`
([SvelteKit](06-web.md#sveltekit)).

## Terraform

The Terraform plug-in serves `*.tf` and `*.tfvars` files, other HCL files (`*.hcl`) and the CLI's
`.terraformrc`. It works in every Zen IDE and comes with DevopsDaruma; the others install it from
the Marketplace, or from the banner above the first `.tf` file. Without a language server it
highlights block types (`resource`, `variable`, `dynamic`...), their labels, attributes, numbers,
`true`, `false` and `null`, functions, the references `var.`, `local.`, `module.`, `data.`,
`each.`, `count.`, `self.` and `path.`, comments of all three kinds, strings with their escapes and
`${...}` and `%{...}` templates, and heredocs (`<<EOT` and `<<-EOT`). It folds blocks, lists,
heredocs and comments of several lines, and fills the **Structure** tool window with the blocks
and their labels, nested blocks under their block.

The plug-in declares the Terraform Language Server (terraform-ls) for problems, completion,
navigation, references and formatting (⌥⌘L). When it is not on your `PATH`, the banner
above the first `.tf` or `.tfvars` file offers to install it: the IDE downloads terraform-ls
(MPL-2.0) from `releases.hashicorp.com`, checks its SHA-256 and keeps it in its own folder, only
when you click **Install** ([Language servers](17-language-servers.md)).

The run icon beside a module's `terraform` block (or on the first line of its `main.tf`) offers
**terraform init**, **terraform validate**, **terraform plan** and **terraform apply**, run in the
file's folder with the `terraform` on your `PATH`; they become run configurations you can change
like any other ([Run and test](15-run-and-test.md)). There is no destroy target.

Commands that change state or infrastructure (`apply`, `destroy`, `import`, `state`, `taint`,
`untaint`, `refresh`, `force-unlock`) are yours alone. Whenever one is about to start, however the
configuration was made or changed, the IDE shows its whole command line and folder and starts it
only when you click **Run**; `apply` itself then asks for `yes` in the Run tool window, as in a
terminal, unless you added `-auto-approve`. An AI agent never starts these commands, whatever
**Agents may run its targets** says, and cannot press keys in the window while one is asked about
or running. What the IDE cannot stop is an agent running `terraform apply` in a terminal or in its
own shell: that is up to the agent's own permissions.
