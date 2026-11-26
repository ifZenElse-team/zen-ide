# Plug-ins

> **Last updated:** 2026-10-28 · **Decision:** [ADR-0031](adr/0031-plugins.md) (refines
> [ADR-0011](adr/0011-plugins-and-jetbrains-compatibility.md)), ABI v1: [ADR-0056](adr/0056-plugin-abi-v1.md),
> dialects and the SQL plug-in: [ADR-0182](adr/0182-sql-plugin-abi-v1-and-dialects.md),
> colours and typing: [ADR-0063](adr/0063-plugin-colours-and-typing.md), Settings → Plugins:
> [ADR-0062](adr/0062-plugins-page.md), projects and requirements: [ADR-0129](adr/0129-project-plugins-npm-run-and-nuxt.md)

An IDE supports its own language out of the box; other file types are supported by plug-ins. When a
file opens that no highlighter supports and the catalog has a plug-in for it, a banner above the
editor offers **Install plugin …** and **Ignore extension**. When a project opens that a plug-in of the
catalog supports by its project markers (below), a notification offers it: **Install plugin …** and
**Don't suggest**; a plug-in that another matching one requires is not offered on its own, the
requiring one's offer names it (Next.js with React, SvelteKit with Svelte,
[ADR-0131](adr/0131-react-and-next-plugins.md)).

## What a plug-in is

One WebAssembly module (`wasm32-unknown-unknown`) with **no imports**. The IDE runs it in `wasmi`
with a fuel limit per call: 2 billion units, plus 10 000 for each byte of input, at most 20 billion
(a module that loops is stopped within a fraction of a second on an ordinary file, and an analyser
that needs more fuel per byte, such as the JavaScript one, still handles files of several
megabytes); a plug-in that traps, runs out of fuel or returns a malformed result is switched off for
the session, and the file is shown without highlighting. A module's memory is limited to 1 GiB: a
module that starts with more is not loaded, and `memory.grow` past the limit returns -1.

Sources live in `plugins/<id>/`: a crate named `zen-plugin-<id>` (`crate-type = ["cdylib", "rlib"]`)
and a `plugin.json` with the display name, a one-sentence description and the file extensions:

```json
{ "name": "Markdown", "description": "Highlights Markdown.", "extensions": ["md", "markdown"] }
```

An optional `names` list matches files by name instead (`*` stands for any characters, case is
ignored), e.g. `"names": [".gitlab-ci.yml", "*.gitlab-ci.yml"]`; a plug-in matched by name wins over
one matched by extension.

The extensions (as `*.<extension>`) and the names are the plug-in's file type in Settings → Editor →
File Types ([ADR-0108](adr/0108-file-type-registry-and-ignored-files.md)), named as the plug-in;
an optional `hashbangs` list gives the type the scripts whose first line names one of these
interpreters, e.g. `"hashbangs": ["sh", "bash", "zsh"]` for `#!/usr/bin/env bash`. The user can move,
add and remove the patterns there; the IDE then asks the plug-in about a file by the name its type is
known by, so a plug-in never needs to know the user's patterns.

An optional `contents` list gives the type the files no name pattern and no hashbang name, by what
they start with ([ADR-0135](adr/0135-file-types-by-content-and-xml-in-every-ide.md)): each marker
has `starts` (what the text starts with, after a byte order mark and white space, letter case as
written), optional `unless` (starts that do not count, any letter case) and optional `firstTag` (a
word the text before the first `>` must hold). The XML plug-in's:

```json
"contents": [
  { "starts": "<?xml" },
  { "starts": "<!DOCTYPE", "unless": ["<!DOCTYPE html"] },
  { "starts": "<", "unless": ["<html", "<!", "<?"], "firstTag": "xmlns" }
]
```

The IDE reads at most the first 4 KiB of a file for them, once per version of the file, and never
from a binary one (a NUL byte); Settings → Editor → File Types shows them read-only.

An optional `languageServers` list declares the language servers for the plug-in's files, offered
in every IDE ([ADR-0055](adr/0055-language-servers.md)). Each has `id`, `name`, `languages` (the
protocol's language ids), `extensions`, optional `names`, optional `watches` (name patterns of
other files whose changes on disk the server is told of with `workspace/didChangeWatchedFiles`, as
it is of the files it serves, e.g. gopls's `go.mod` and `go.work`), `command` (a program name looked up on
this computer, or an absolute path), `args`, optional `versionArgs`, and optional `install` with
`kind` (`go`, `npm`, `cargo`, `archive` or `manual`), `package` (several separated by spaces, e.g.
`"@vue/language-server@2.2.12 typescript@5"`) and, for `manual`, a `hint` that tells the
user how. Optional `initializationOptions` are the server's own options for `initialize`; a string
in them that is exactly `${nodeModules:<path>}` becomes the path `node_modules/<path>`, and
`${nodeModulesFolder:<path>}` the folder of the file `node_modules/<path>`, looked for as Node.js
does in the project and the folders above it, then beside the server's program, where the IDE
installed the packages installed with it (`<data>/language-servers/<id>/node_modules`) (the Vue plug-in's `"tsdk":
"${nodeModulesFolder:typescript/lib/typescript.js}"`, [ADR-0126](adr/0126-vue-language-server-and-npm-paths-in-server-options.md)). Optional
optional `inlayHints` say how the server's settings switch its inlay hints by kind: the `section` it
reads with `workspace/configuration` and, for each kind of Settings → Editor → Inlay Hints
(`parameterNames`, `fieldNames`, `variableTypes`, `literalTypes`, `typeArguments`,
`constantValues`), the dotted paths of its settings set to whether the kind is on (the section is answered with `initializationOptions` and those paths, nothing else), e.g.
`{ "section": "gopls", "kinds": { "parameterNames": ["hints.parameterNames"] } }`. Optional
`languageIds` maps extensions to the protocol's language ids where they differ from the extension
(`{ "tsx": "typescriptreact" }`), and optional `optimizeImports` lists the code action kinds
Optimize Imports asks for, in order, each applied before the next is asked for
(`["source.removeUnusedImports", "source.organizeImports"]` for the TypeScript server; only
`source.organizeImports` without it), [ADR-0130](adr/0130-typescript-and-javascript-through-typescript-language-server.md).
Optional `powerSave` gives the settings laid over the server's own while Power Save Mode is on:
the `section` it reads with `workspace/configuration` and the `settings`, merged object by object
(gopls: `{ "section": "gopls", "settings": { "diagnosticsTrigger": "Save" } }`); they are taken off
again when it is turned off.
Optional `virtualDocuments` names the URI schemes the server answers locations in for documents
that are not files, the request that gives such a document's text, and the extension it is shown
with: `{ "schemes": ["jdt"], "request": "java/classFileContents", "extension": "java" }` for the
Eclipse JDT Language Server's library classes. Go to Declaration asks for the text, keeps it in the
IDE's cache, opens it read-only and speaks to the server about it under its own URI, so navigation
goes on from inside it ([ADR-0198](adr/0198-library-classes-and-documents-of-language-servers.md)).
Optional `generate` says what Code → Generate… (⌘N) takes from the server
([ADR-0216](adr/0216-generate-and-the-member-chooser.md)): `kinds` narrows the code action kinds it
lists (all `source.*` but organizing imports and fixing all by default; gopls:
`{ "kinds": ["source.addTest"] }`), and `prompts` turns an action whose command the client must
run itself into a generator with a member chooser. Each prompt names the `command`, the request
that gives the `members` (sent with the command's first argument), the `lists` of members in its
answer (`id`, `items`: a JSON pointer where `*` steps into each element, `label`, `detail` and
`group`: text where `{/pointer}` is a value of the item and `{parent/pointer}` of the element one
`*` up, `selected`: `true` or a pointer to a flag, `value`: what is sent for a chosen member, by
default the item itself), the `options` (`id`, `label` as `{ "en": ..., "ru": ... }`, `default`, and
`when`: a pointer into the answer that must be set for the option to show), an optional `title`
and `allowEmpty`, and the request that writes the code, `generate`, with its `params`: JSON where
the strings `${context}`, `${members}`, `${members:<list>}`, `${option:<id>}`, `${item}`,
`${item:/pointer}` and `${parent:/pointer}` are replaced. Its answer is the workspace edit applied
as one undo step. Several prompts may share a command ([ADR-0226](adr/0226-several-generators-per-server-action.md)):
`match` (`{ "/kind": 0 }`: pointers into the command's first argument and the values they must have)
picks the ones for an action, and each becomes an entry of its own, named by its `name`
(`{ "en": ..., "ru": ... }`; the action's title by default), in the order the prompts are declared;
`action` (`override` or `implement`) makes Override Methods… (⌃O) or Implement Methods… (⌃I) open
its chooser directly. A list's `when` keeps only the items where a pointer is set, and a pointer
that starts with `!` (in `when`, `selected` and an option's `when`) means it is not set; the
parent of the items of a first `*` is the whole answer, so `{parent/type}` reads a field of it. For example
`{ "command": "java.action.generateToStringPrompt", "members": "java/checkToStringStatus", "lists": [{ "id": "fields", "items": "/fields/*", "label": "{/name}", "detail": "{/type}", "selected": "/isSelected" }], "generate": "java/generateToString", "params": { "context": "${context}", "fields": "${members:fields}" } }`.
Several plug-ins may declare the same server only word for word (the TypeScript and JavaScript
plug-ins share theirs); the first declaration of an id wins. Optional
`workspaceSymbols: false` says the server does not find declarations across the project
(`workspace/symbol`), as a server of configuration files does not: Go to Symbol and Go to Class then
never name it as missing ([ADR-0169](adr/0169-missing-servers-symbols-and-run-options.md)). The IDE installs a server only when the user asks (`lsp.server.install`), an npm one with
`npm install --prefix` into its own folder `<data>/language-servers/<id>`, never globally, and
finds its program in `node_modules/.bin` there before `PATH`
([ADR-0127](adr/0127-npm-language-servers-in-the-ides-own-folder.md)); when `go`, `npm`
or `cargo` is missing it says so itself, translated, with a link to download it:

```json
"languageServers": [{
  "id": "yaml-language-server", "name": "YAML Language Server", "languages": ["yaml"],
  "extensions": ["yml", "yaml"], "command": "yaml-language-server", "args": ["--stdio"],
  "install": { "kind": "npm", "package": "yaml-language-server" }
}]
```

A server that is a download, not a package, has `install` of kind `archive` with the `url` of its
`.tar.gz`, its `sha256` and optional `files` (`[{ url, sha256, path }]`, files downloaded into its
folder after it, such as the java-debug bundle Eclipse JDT LS loads, ADR-0153): on the user's word the IDE downloads it, checks it and unpacks it into
`<data>/language-servers/<id>` (one top folder of the archive is dropped), replacing what was there
only once all of it is unpacked; an archive that does not match is not unpacked
([ADR-0148](adr/0148-java-plugin-and-eclipse-jdt-ls.md)). Its `command` and `args` may then hold
placeholders, replaced as the server starts: `${serverFolder}` (that folder), `${glob:<pattern>}`
(the first file in it whose path matches, `*` in the last part only; the server counts as installed
when every one is found), `${os}` (`mac`, `linux`, `win`), `${archSuffix}` (`_arm` on ARM but on
Windows), `${workspaceData}` (a folder of the server's own for the project, in the IDE's cache,
`<data>/caches/language-servers/<id>/<project>-<hash>`) and `${javaHome}`, a JDK of at least the
Java version the optional `java` names: the project's JDK when it is new enough
([ADR-0147](adr/0147-jvmjizo-app-and-jdks.md)), else the newest one found. Other `${…}` are left for
the JDK's own (`${jdkHome}`…). The strings of its `initializationOptions` take the same
placeholders (`"bundles": ["${serverFolder}/bundles/….jar"]`). A server built per platform gives `platforms`, each platform's
archive by `macos-aarch64`, `macos-x86_64`, `linux-x86_64`, `linux-aarch64` or `windows-x86_64`
(`{"url": …, "sha256": …}`), in place of `url` and `sha256`; a platform without one gets the
`hint`. Zip archives are unpacked as `.tar.gz` ones are ([ADR-0151](adr/0151-kotlin-plugin.md)).

When several servers serve the same language (the first of their `languages`) in its place, the
user chooses one in Settings → Language Servers (`lsp.server.choose`); the first declared is the
default, and only the chosen one is offered and started. A server may carry a `notice`, shown where
it is offered and installed: `{"text": {"en": …, "ru": …}, "url": "https://…"}` (the Kotlin plug-in
says whose software kotlin-lsp is). The Java plug-in's Eclipse JDT LS:

```json
"command": "${javaHome}/bin/java", "java": 25,
"args": ["-Dosgi.sharedConfiguration.area=${serverFolder}/config_${os}${archSuffix}", "...",
  "-jar", "${glob:plugins/org.eclipse.equinox.launcher_*.jar}",
  "-configuration", "${workspaceData}/config", "-data", "${workspaceData}/workspace"],
"install": { "kind": "archive", "url": "https://download.eclipse.org/jdtls/milestones/1.63.0/jdt-language-server-1.63.0-202610191322.tar.gz",
  "sha256": "b1f2abe0…" }
```

A server may also serve its files **alongside** their language's server (`"alongside": true`: its
problems and code actions are added to that server's, nothing else is asked of it), format them
instead of it (`"formats": true`), apply a code action kind on save when a setting is on
(`"fixOnSave": {"kind": "source.fixAll.eslint", "setting": "codeQuality.eslint.fixOnSave"}`), be
answered on `workspace/configuration` with `"settings"` by section (`""` for items without one;
`${projectRoot}`, `${projectUri}`, `${projectName}` in its strings), run the project's own program
(`"fromProject": true`, `node_modules/.bin/<command>`), and serve only the projects of `"when"`
(project markers). A plug-in's `project` support may declare `formatters`
(`{id, name, command, args, extensions, onReformat, onSave}`, `${file}` in the arguments), the
project's programs that Reformat Code and saving run on stdin and stdout
([ADR-0136](adr/0136-linters-and-formatters.md)).

An optional `serverPlugins` list adds plug-ins to another plug-in's language server, by its id:
`server`, `name` (the package as the server loads it), `package` (what npm installs),
`languages` and `when`, the project markers of the projects it is for (`{"packages": ["svelte"]}`),
which limit only where the IDE offers to install it. They are installed into that server's folder in the IDE and passed to it as
`initializationOptions.plugins: [{name, location, languages}]`, the form typescript-language-server
takes TypeScript server plug-ins in ([ADR-0134](adr/0134-plugins-contribute-typescript-server-plugins.md)):

```json
"serverPlugins": [{ "server": "typescript-language-server", "name": "typescript-svelte-plugin",
  "package": "typescript-svelte-plugin@0.3.52", "languages": ["svelte"],
  "when": { "packages": ["svelte"] } }]
```

An optional `fileTemplates` object declares built-in file templates and template variables, offered
in every IDE ([ADR-0115](adr/0115-file-templates.md)): `templates`, each with `name`, `extension`,
optional `kind` (`file`, `include` or `code`), `fileName`, `text`, `reformat`, `liveTemplates`,
`children` (more files the template creates, ADR-0155), `markers` (files one of which must be in
the folder or above it for New to offer the template, such as `["go.mod"]`, or `*.go` for any such
file), `sourceFolder` (offered only in a source folder, never in File → New), `group` (the one row of
New for the templates of that name; its name dialog chooses among them, ADR-0243) and `kindOf`
(another template's name: New lists only that one, and its dialog offers this one as a **Kind**),
and `variables`, each with `name`, `description` and `value`. A template that does not read is left
out:

```json
"fileTemplates": {
  "templates": [{ "name": "HTML File", "extension": "html", "text": "<title>${NAME}</title>\n" }],
  "variables": [{ "name": "HTML_LANG", "description": "The page's language", "value": "en" }]
}
```

An optional `liveTemplates` object declares, for the plug-in's languages, what the editor's live
templates, postfix completion, Surround With, Complete Current Statement and documentation comment
stubs use ([ADR-0218](adr/0218-editable-live-templates-postfix-and-statement-completion.md),
[ADR-0223](adr/0223-plug-in-surrounders-thrown-types-and-method-statements.md)); each list has
the JSON shape of a `zen_workbench::templates` type, and an entry that does not read is left out.
A `description` or `title` is a string or an object of language codes, `{"en": "...", "ru": "..."}`,
shown in the interface language (then English):

- `templates`: live templates (`Template`: `language`, `abbreviation`, `description`, `text`,
  `variables` with `name`, `default`, `expression` and `skipIfDefined`, `imports`, `contexts` of
  `code`, `comment`, `string`). They show in Settings → Live Templates under their language,
  read-only: the user can switch one off or duplicate it. A template with the language and
  abbreviation of a language module's one is left out.
- `postfix`: postfix templates (`PostfixTemplate`: `language`, `key`, `description`, `text` with
  `$EXPR$` for the expression before the dot, `variables`, `imports`), offered after
  `expression.` and listed in Settings → Editor → General → Postfix Completion.
- `surrounders`: the language's constructs for Code → Surround With (`Surrounder`: `language`,
  `id`, `title`, `text` with `$SELECTION$`, `variables`, and `expression` for one that wraps an
  expression rather than statements), listed after a language module's ones; a module's surrounder
  with the same id wins.
- `docComments`: documentation comment stubs (`DocComment`: `language`, `opening` such as `/**`,
  `onEnter`, the declaration `keywords`, `calls` for `name(` declarations such as methods, `fields`
  for `Type name;` declarations, `skip` for annotation lines, `parameterName` `first` or `last`,
  `resultBefore`, `noResult`, `resultContains`, `throws` for the word that lists thrown types after
  the parameters, `throwsAnnotation` for an annotation line that lists them such as `@Throws(`, and
  `text` with `$NAME$`, a `$PARAMETERS$` line made of `parameter` lines with `$PARAMETER$`, a
  `$RESULT$` line made of `result`, and a `$THROWS$` line made of `thrown` lines with
  `$EXCEPTION$`).
- `statements`: Complete Current Statement rules (`StatementRules`: `language`, `terminator` such
  as `;`, `blocks`, the words that start a statement with a block, `parenthesized`, the ones
  whose condition goes in parentheses, `methods` when `Type name(` declares a method with a block,
  and `notDeclarations`, the words such as `return` that start such a line without declaring one).

```json
"liveTemplates": {
  "templates": [{ "language": "java", "abbreviation": "sout",
    "description": { "en": "Prints a line", "ru": "<the Russian text>" }, "text": "System.out.println($END$);" }],
  "postfix": [{ "language": "java", "key": "nn", "text": "if ($EXPR$ != null) {\n\t$END$\n}" }],
  "surrounders": [{ "language": "java", "id": "synchronized", "title": "synchronized",
    "text": "synchronized ($LOCK$) {\n\t$SELECTION$\n}", "variables": [{ "name": "LOCK", "default": "this" }] }],
  "docComments": [{ "language": "java", "opening": "/**", "onEnter": true,
    "keywords": ["class", "interface", "enum", "record"], "calls": true, "fields": true, "skip": ["@"],
    "resultBefore": true, "noResult": ["void", "public", "private", "protected", "static"],
    "throws": "throws", "text": "/**\n * $END$\n$PARAMETERS$\n$RESULT$\n$THROWS$\n */",
    "parameter": " * @param $PARAMETER$", "result": " * @return", "thrown": " * @throws $EXCEPTION$" }],
  "statements": [{ "language": "java", "terminator": ";", "blocks": ["if", "for", "while", "class"],
    "parenthesized": ["if", "for", "while"], "methods": true, "notDeclarations": ["return", "throw", "new"] }]
}
```

`plugins/java` and `plugins/kotlin` declare full sets. The fields of ADR-0223 (`surrounders`, a
`description` or `title` object, `fields`, `throws`, `throwsAnnotation`, `thrown`, `methods`,
`notDeclarations`) need an IDE that has them; the release server keeps them in its catalog once it
runs the `zen-releases` that knows them.

An optional `requires` list names the plug-ins a plug-in needs, by id: installing it (from a banner,
a notification, the Marketplace, `plugins.plugin.install` or MCP's `install_plugin`) installs first
the ones it requires that are not installed, and theirs. Nothing else ties them: a required plug-in
can be switched off or removed afterwards. The Nuxt plug-in has `"requires": ["vue"]`.

An optional `project` object declares the projects a plug-in supports
([ADR-0129](adr/0129-project-plugins-npm-run-and-nuxt.md), `zen_project::support::ProjectSupport`):
`markers` recognise such a project, by `files` at its root or npm `packages` among the
`dependencies` and `devDependencies` of its root `package.json`; `runConfigurations` are added to
such a project's `.zen/runConfigurations.json` once (same shape as in that file,
[run-configurations.md](run-configurations.md); one with the kind and name of one the project has is
left out, and one the user deletes does not come back; one with `when`, project markers of its own,
is added only to the projects that match them too, as React's `vite` configurations to Vite
projects, [ADR-0131](adr/0131-react-and-next-plugins.md)); `setup` is a step offered once per project
while the path `creates` is missing, run with `plugins.project.setup` in the Run tool window; `build`
names the build system the plug-in loads such a project with: `files`, the names of its build
scripts anywhere in the project, and `reload`, the command that loads the project again from them
(Maven: `{"files": ["pom.xml"], "reload": "maven.project.reload"}`), and optionally `moduleFiles`,
the files that list modules, first the one to edit: the `zen_project` step `addModule` gets `file`,
`text`, `path`, `project`, `hasBuild` and `mustBuild` and answers `{"files": [...]}`, empty when it is listed, or
`{"error": ...}` ([ADR-0244](adr/0244-projects-from-sources-and-modules.md)). The platform watches those
files and offers Load Changes (⇧⌘I) when one changed since the last load; the reload command
records its steps in the Build tool window's Sync tab
([ADR-0196](adr/0196-load-changes-and-the-sync-view.md)). Marker files and `creates` must stay
inside the project, and each configuration needs a name and a kind:

```json
"requires": ["vue"],
"project": {
  "markers": { "files": ["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mjs"], "packages": ["nuxt"] },
  "runConfigurations": [{ "name": "nuxi dev", "kind": "npm",
    "options": { "command": "exec", "script": "nuxi" }, "common": { "programArguments": "dev" } }],
  "setup": { "creates": ".nuxt", "configuration": { "name": "nuxi prepare", "kind": "npm",
    "options": { "command": "exec", "script": "nuxi" }, "common": { "programArguments": "prepare" } } }
}
```

An optional `projectGenerators` list adds project types to the New Project wizard, under **Generators**
in plug-in order ([ADR-0237](adr/0237-new-project-wizard-and-project-generators.md)). Each has `id`,
`name`, `icon` (the name of a file whose icon stands for the type, e.g. `package.json`), `fields`,
optional `dynamicForm` (the module gives the form, not `fields`, and may add `server`: `{"label",
"url"}`, the service shown above Name), `network` (the module may ask the IDE to fetch addresses; the wizard says
so) and `settings`, text settings the person sets in System Settings → **Project Generators**, each
`{"id", "label", "help"?, "default"}`
([ADR-0240](adr/0240-generators-with-a-service.md)). A language's plug-in adds `language: true` to
stand above Empty Project instead ([ADR-0242](adr/0242-java-and-kotlin-project-types.md)).
`formDependsOn` names the fields whose values change a dynamic form: the module gets them in
`values`, and the wizard asks again when they change. `tool` names the build tool the module's
`generate` step may run (`maven`, [ADR-0244](adr/0244-projects-from-sources-and-modules.md)): it
answers `{"run": {"tool", "args", "javaHome", "subfolder"}}`, the IDE runs it in the new project's
folder, shows its output, and moves `subfolder` up; agents cannot start such a generator. For New
Module `values` has `module: true`. A plug-in with generators must export `zen_project` (below).

A field has `id`, `label`, `type`, optional `required`, `help`, `tooltip` (the question mark beside
it), `advanced` (folded under Advanced Settings), `segmented` (a `choice` as a row of buttons), `page`
(0 by default; a later page comes after **Next**, [ADR-0243](adr/0243-new-project-wizard-pages-and-file-new-without-classes.md)),
`visibleWhen` (`{"field": id, "anyOf": [values]}`; `true` and `false` for a check box) and `manage`,
a link to a list the IDE keeps for the type: `id`, `label`, `title`, `columns`, `addOne` (a form that
adds one row) and `value` (the field's value after adding, with `${column}`). The module gets the
lists in `values.lists`.
The types are `jdk` (the IDE lists the computer's JDKs with Download; its value is the JDK's home,
and `<id>Release` in `values` is its feature release), `note` (`before`, `after` and a `link` with
`label` and `generator`, the type it selects; leading notes stand above Name), `table` (`columns`,
each `id` and `label`, and `empty`; its value is a list of objects), `text` and
`path` (`default`, where `${name}` stands for the project's name and `${id}` for the value of a field
above it, until the person changes the field), `check` (`default`), `choice`
(`options`, `default`) and `select` (`groups`, each a `name` and `options`, and `default`, a list: a
multi-select with search; a ticked option that gets disabled leaves the value until it is enabled
again). An option has `value`, `label`, optional `description`, `url`, `enabledWhen`
and `disabledNote`, shown while `enabledWhen` does not hold. `name` and the field's texts are strings
or objects by language:

```json
"projectGenerators": [{
  "id": "vite", "name": { "en": "Vite", "ru": "Vite" }, "icon": "package.json",
  "fields": [
    { "id": "language", "label": "Language", "type": "choice", "default": "ts", "options": [
      { "value": "ts", "label": "TypeScript" }, { "value": "js", "label": "JavaScript" }] },
    { "id": "strict", "label": "Strict", "type": "check", "default": true,
      "visibleWhen": { "field": "language", "anyOf": ["ts"] } }
  ]
}]
```

A template's text is Velocity: a framework's own `$` names (SvelteKit's `./$types`, `$props()`,
`$app/state`) would be read as variables and asked for, so write such a text as a raw block,
`#[[…]]#`, as the SvelteKit plug-in does.

An optional `databaseDrivers` list declares database drivers served by a program of the plug-in,
offered in every IDE's Database workspace ([ADR-0174](adr/0174-external-database-driver-protocol.md),
the protocol and every field in [database-drivers.md](database-drivers.md)): `id`, `name`, optional
`icon`, `protocol` (1), `process` (`perConnection` or `shared`), `defaultPort`, `capabilities`,
`readVerbs`, `guardsReadOnly`, `args` and `platforms`, the program of each platform by its `url`,
`sha256`, optional `size` and optional `file` name. Installing or updating the plug-in on the user's
word downloads the program for this computer into `drivers/<driver>/` of the plug-in's folder and
checks it; an update brings the new program when its SHA-256 changed; the update the IDE makes by
itself at start leaves such a plug-in for the user to update. A first-party driver (a crate in
`drivers/`) is published with its plug-in: the release build fills in `platforms` (below):

```json
"databaseDrivers": [{ "id": "duckdb", "name": "DuckDB", "process": "perConnection",
  "capabilities": { "models": ["relational"], "language": "sql:duckdb" },
  "platforms": { "macos-aarch64": { "url": "https://example.org/zen-duckdb-macos-aarch64", "sha256": "…" } } }]
```

An optional `tests` list declares how a language's code and its tests are named and how a test is
written, for Navigate → Test (⇧⌘T) and the Create Test dialog in every IDE
([ADR-0209](adr/0209-test-naming-providers-and-test-history.md),
`zen_run::test_support::TestDeclaration`): `id`, `extensions` (the language's files), `pairs` (a
subject's path and its test's, relative to a module's folder: `{path}` is any number of folders,
each with its `/`, `{name}` one name part), `members` (a regular expression that finds a function,
method or class, its name in the group `name`; a name in backticks is taken without them), `keywords`
(words that are never a member: a match with one of them as a whole named group, say a `type` group
that caught `return`, is left out, since the expressions have no look-ahead), `tests` (how a test of a member is named: `{name}`
is the member's name, `{Name}` the same with a capital letter; a test whose name starts with one
tests it), `packageLine` (with `${PACKAGE}`, the folders of `{path}` joined with dots), `insert`
(`end` or `beforeLastBrace`, where test methods go in a test file that exists) and `frameworks`,
each with `id`, `name`, `detect` (files of the module's folder or the project's, by a text they
contain, such as a dependency's coordinates in a build file; the frameworks a project uses so come
first, the others after), `file` (a new test file, with
`${PACKAGE_LINE}`, `${PACKAGE}`, `${NAME}`, `${SUBJECT}` and `${METHODS}`) and `method` (one test
method, with `${TEST}`, `${MEMBER}` and `${NAME}`). A declaration that does not read is left out.
The key needs a Zen IDE released after 26.11.2; an older IDE ignores it. The Java and Kotlin plug-ins
(`plugins/java`, `plugins/kotlin`, [ADR-0224](adr/0224-jvm-test-rules-as-plugin-declarations.md))
are full examples; a short one:

```json
"tests": [{ "id": "java", "extensions": ["java"],
  "pairs": [{ "subject": "src/main/java/{path}{name}.java", "test": "src/test/java/{path}{name}Test.java" }],
  "members": "^\\s*(?:public |protected |private |static )*[\\w<>\\[\\]]+ (?P<name>\\w+)\\(",
  "tests": ["test{Name}", "{name}"], "packageLine": "package ${PACKAGE};\n\n", "insert": "beforeLastBrace",
  "frameworks": [{ "id": "junit5", "name": "JUnit 5",
    "detect": [{ "file": "pom.xml", "contains": "junit-jupiter" }, { "file": "build.gradle.kts", "contains": "junit-jupiter" }],
    "file": "${PACKAGE_LINE}import org.junit.jupiter.api.Test;\n\nclass ${NAME} {\n${METHODS}}\n",
    "method": "\n    @Test\n    void ${TEST}() {\n    }\n" }] }]
```

The team's own driver programs are crates in `drivers/` and their plug-ins sit in `plugins/` like any
other (`plugins/sqlserver` declares `zen-db-sqlserver`, ADR-0175); such a plug-in keeps
`version.workspace = true`, and so no changelog and no release, until its programs can be
published.

An optional `codeStyles` list gives a language a page of its own under Settings → Editor → Code
Style ([ADR-0220](adr/0220-code-style-options-declared-by-languages.md)): `language`, `title` and
each option's `label` as text by language code, a `preview` text, `options` (each an `id`, the
`tab` among `tabsAndIndents`, `spaces`, `wrappingAndBraces`, `blankLines`, `imports` and
`arrangement`, an optional `group`, a `kind` (`boolean`, `integer` with `min` and `max`, `choice`
with `choices`, or `text`), a `default`, and where the value goes: `formattingOption`,
`serverSetting` (`section`, `path`), `configKey`, `editorConfig`), `notes` by tab, an optional
`configFile` (`names`, optional `arguments`) and the `arrangement` kinds. A module's page wins over
a plug-in's for the same language. The configuration file may be an Eclipse formatter profile or an
`.editorconfig` section instead of TOML (`format`: `eclipseProfile` or `editorConfig`, with
`section`), and its path may be handed to a server setting (`location`); an option may write
several keys (`moreConfigKeys`) with mapped values (`configValues`, as `{"true": "insert"}`), and a
text option may go to a server setting as a list (`serverSetting.separator`)
([ADR-0228](adr/0228-formatter-profiles-and-editorconfig-sections-for-code-style.md)):

```json
"codeStyles": [{ "language": "java", "title": { "en": "Java", "ru": "Java" },
  "options": [{ "id": "comments", "label": { "en": "Format comments", "ru": "Форматировать комментарии" },
    "tab": "wrappingAndBraces", "kind": { "type": "boolean" }, "default": true,
    "serverSetting": { "section": "java", "path": "format.comments.enabled" } }] }]
```

An optional `terminalAgents` list declares agent resume adapters
([ADR-0275](adr/0275-terminal-sessions-that-outlive-the-ide.md)): how the IDE recognises an AI agent
running in a terminal and resumes its conversation after a restart. Each has an `id` (lower-case
letters, digits and dashes), a `name`, `programs` (names one of the first three words of the running
program's command line must have, without folder and `.js`, so `node /usr/local/bin/qwen` is
`qwen`), `resume` (the command word by word, with `{session}` once where the conversation id goes),
an optional `sessionField` (a JSON pointer into the report, `/session_id` by default) and `docs`.
The agent reports its id with a session-start hook that writes its JSON input to
`$ZEN_TERMINAL_AGENTS/$ZEN_TERMINAL_ID-<id>.json` (`terminal.agent.list` gives the exact command) or
with `terminal.agent.report`; an id is only ever letters, digits and `-_.:`, and only the adapter's
own `resume` command is run with it. The user's adapters in Settings win over a plug-in's of the same
id. Only the adapters of the user's Settings and of bundled plug-ins resume by themselves; the resume
command of an installed plug-in's adapter is only offered to the person (Run Again), so installing a
plug-in never makes a command run unasked after a restart. Adapters are the person's choice: an agent
cannot change them with `set_settings`, nor install a plug-in that declares them or switch one on while it is off. The `ai-agents` plug-in, in the base set, declares Claude Code, Codex and Qwen Code:

```json
"terminalAgents": [{ "id": "claude-code", "name": "Claude Code", "programs": ["claude"],
  "resume": ["claude", "--resume", "{session}"], "sessionField": "/session_id" }]
```

A plug-in may declare no file types at all (`"extensions": []`), as Nuxt does: its module then only
has to load (the smallest ABI v0 module highlights nothing), and everything it gives is declared.

A plug-in crate depends on no workspace crate (`cargo xtask layers` checks it). It is the only kind
of crate where `#[unsafe(no_mangle)]` is allowed, on the ABI exports only. Nothing depends on a
plug-in, except as a dev-dependency: the host's tests run the real plug-ins natively, their answers
decoded and checked as a module's (`zen_plugins::native::NativePlugin`, feature `native`; the web
plug-ins in `crates/platform/ui/tests/web_plugins.rs`).

## ABI v0: highlighting

| Export | Signature | Meaning |
|---|---|---|
| `memory` | memory | The module's linear memory |
| `zen_abi` | `() -> i32` | The ABI version: `0` |
| `zen_alloc` | `(len: i32) -> i32` | Makes room for `len` bytes of input; returns where the host writes them |
| `zen_highlight` | `(ptr: i32, len: i32) -> i64` | Highlights the UTF-8 text the host wrote at `ptr`; returns the result's pointer in the high 32 bits and its length in bytes in the low 32 bits |

The result is a list of records, 12 bytes each, little-endian: `start: u32`, `end: u32` (byte
offsets into the text) and `kind: u32`. Records must be ordered and must not overlap; the host drops
any record that is out of order, out of bounds, not on a character boundary, or of an unknown kind.
The result stays valid until the next call.

| Kind | Meaning | Kind | Meaning |
|---|---|---|---|
| 0 | keyword | 9 | attribute |
| 1 | string | 10 | constant |
| 2 | number | 11 | punctuation |
| 3 | comment | 12 | heading |
| 4 | documentation comment | 13 | strong text |
| 5 | macro | 14 | emphasis |
| 6 | type | 15 | link |
| 7 | function | 16 | code (inline or block) |
| 8 | lifetime or label | 17 | property where it is declared |
| 18 | global or module variable | | |

Kinds 17 and 18 came later, within ABI v0 ([ADR-0184](adr/0184-highlight-kinds-property-global.md)): an
older host drops a record of a kind it does not know, so the text stays plain there. New
capabilities come with new ABI versions; a host loads only the versions it knows.

## ABI v1: folding, outline, completion, expansion

A v1 module returns `1` from `zen_abi` and keeps every v0 export. Four more exports are optional:
the host looks each one up by name when the module loads and treats a missing one as "not
supported" (one with another signature refuses the module). Each takes `(ptr: i32, len: i32)` and
returns an `i64`: the host calls `zen_alloc(len)`, writes the input there, and reads the result from
the pointer in the high 32 bits, its length in bytes in the low 32 bits, valid until the next call.
A zero length means "nothing".

| Export | Input | Result |
|---|---|---|
| `zen_fold` | the UTF-8 text | records of 8 bytes, little-endian: `start_line: u32`, `end_line: u32` (zero-based, `end_line > start_line`) |
| `zen_outline` | the UTF-8 text | UTF-8 JSON: `[{"name": str, "kind": str, "start": u32, "end": u32, "children": [...]}]`, byte offsets |
| `zen_complete` | `caret: u32` (4 bytes, little-endian, a byte offset), then the UTF-8 text | UTF-8 JSON: `{"from": u32, "items": [{"label": str, "kind": str, "insert": str, "detail": str?}]}`; `from..caret` is replaced by `insert` |
| `zen_expand` | as `zen_complete` | UTF-8 JSON `{"from": u32, "to": u32, "text": str}`: the abbreviation before the caret and what it expands to; a zero length when there is nothing to expand |

- Outline kinds: `element`, `rule`, `property`, `function`, `class`, `method`, `variable`,
  `constant`, `interface`, `type`, `module`, `field`, `key`; unknown kinds become `key`. Completion
  kinds add `keyword`, `tag`, `attribute`, `value` and `snippet`.
- `$0` in `insert` or `text` marks where the caret goes after insertion (once; the host removes it).
- An outline stops at 32 levels of nesting and 20 000 entries in the plug-in itself: JSON nested
  more than 128 levels deep does not parse (an outline level takes two), and the host takes no
  more entries anyway. Completion stops at 2 000 items for the same reason.
- A plug-in writes its JSON itself (a small encoder with proper string escaping), since it depends
  on no workspace crate.
- What the host drops: fold ranges that are empty, past the last line or repeated; outline entries
  out of bounds, not on character boundaries, with an empty name or outside their parent (with their
  children); suggestions with an empty label or a `from` after the caret; an expansion that does not
  end at the caret. It takes at most 100 000 folds, 20 000 outline entries and 2 000 suggestions and
  cuts names to 256 characters.
- What switches the plug-in off, as for highlighting: a trap, running out of fuel, a result larger
  than 64 MiB of records or 32 MiB of JSON, records that are not 8 bytes each, JSON that does not
  parse.

In the IDE, for a file of a plug-in (the most specific one, as for highlighting):

| Feature | Commands |
|---|---|
| Folding | `zen_fold` feeds `editor.fold.*` (the gutter, ⌘−, ⌘=, ⇧⌘−, ⇧⌘=): the plug-in's ranges replace the built-in bracket folds, each from the end of its first line to the end of its last. |
| Structure | `editor.structure.get` shows the outline when no language server answers. |
| Completion | `editor.completion.start` / `list` / `accept`: the plug-in's items come at once, after a language server's when there is one too. The editor starts it as for a server: on typing a name or `.`, and with ⌃Space; the typed word may hold `-` and `$`, Enter inserts and Tab replaces the rest of the word. |
| Expansion | `editor.emmet.expand {path, offset}` → `{"applied": bool}`; Tab expands first and otherwise indents. |

### Colours and typing

Three more optional exports of ABI v1 ([ADR-0063](adr/0063-plugin-colours-and-typing.md)), with the
same signature and lookup; an IDE that does not know them ignores them, so a module that has them
still implements ABI `1`.

| Export | Input | Result |
|---|---|---|
| `zen_colors` | the UTF-8 text | records of 12 bytes, little-endian: `start: u32`, `end: u32` (byte offsets), `rgba: u32` (`0xRRGGBBAA`, not premultiplied), ordered and not overlapping |
| `zen_on_type_triggers` | nothing (a zero length) | UTF-8: the characters that make the host call `zen_on_type` (at most 32, read once when the module loads) |
| `zen_on_type` | `caret: u32`, `typed: u32` (a Unicode scalar), then the text, the character already in it and the caret right after it | the JSON of `zen_expand`, `{"from": u32, "to": u32, "text": str}`, ending at the caret, `$0` for where the caret goes; a zero length for nothing |

- What the host drops: colour records out of order, overlapping, empty, out of bounds or not on
  character boundaries, beyond 100 000; an answer to typing that does not end at the caret.
- What switches the plug-in off: as above, and colour records that are not 12 bytes each. A module
  whose `zen_on_type_triggers` traps does not load.

Two more optional exports of ABI v1 convert code into the plug-in's language
([ADR-0284](adr/0284-java-to-kotlin-conversion.md)):

| Export | Input | Result |
|---|---|---|
| `zen_convert_from` | nothing (a zero length) | UTF-8: the source extensions it converts from, comma-separated (read once when the module loads) |
| `zen_convert` | `from: u32` (the length of the next field, little-endian), the UTF-8 source extension, then the UTF-8 text | UTF-8 JSON `{"text": str, "problems": [{"start": u32, "end": u32, "message": str}]}`, byte offsets into the text given |

- The host asks the plug-in of the target file type, and only for an extension it named; it drops
  names that are not letters and digits and problems outside the text. A zero length means "cannot
  convert this". The Kotlin plug-in converts from `java`.
- In the IDE: `file.convert.toLanguage` and `editor.convert.text`, Code → Convert Java File to
  Kotlin File, and the offer when code taken from a file of the source language is pasted.

One more optional export of ABI v1 ([ADR-0135](adr/0135-file-types-by-content-and-xml-in-every-ide.md)):

| Export | Input | Result |
|---|---|---|
| `zen_problems` | the UTF-8 text | UTF-8 JSON `[{"start": u32, "end": u32, "severity": "error" \| "warning", "message": str}]`, byte offsets |

- The host drops problems out of bounds, reversed, not on character boundaries or without a
  message, keeps the first line of a message (at most 512 characters) and at most 1 000 problems.
  They join the file's problems (`problems.file.list`, the editor's underlines, the Problems tool
  window), found again when the document changes; a file such a plug-in serves counts as checked
  even without a language server. The message is the plug-in's own words, in English, as a language
  server's are.

One more optional export of ABI v1 ([ADR-0182](adr/0182-sql-plugin-abi-v1-and-dialects.md)), for a
language with dialects:

| Export | Input | Result |
|---|---|---|
| `zen_dialect` | the UTF-8 name of a dialect, or nothing | a zero length; the calls that follow read the text in that dialect |

- The host calls it before **every** call of any other export of the module, under the same lock,
  with the file's dialect or an empty name, so a module shared by many files never answers one in
  another's dialect. An IDE that does not know the export never calls it.
- The dialect is that of the connection the file is bound to (a console of the Database workspace,
  or a project `.sql` file bound with `db.script.bind`): `postgres`, `mysql`, `mariadb`, `sqlite`,
  `clickhouse`, and `sqlserver` for later. Highlighting is not told yet. Plug-ins read an unknown
  name as no dialect.
- `zen_syntax::TextFeatures::in_dialect` is the host's view of a plug-in in a dialect;
  `zen_plugins::native::Exports::dialect` runs it in the host's tests.

One more optional export of ABI v1 ([ADR-0200](adr/0200-kotlin-runs-tests-and-debugs-through-the-java-kinds.md)),
for a language that compiles to JVM classes:

| Export | Input | Result |
|---|---|---|
| `zen_jvm` | the UTF-8 text | UTF-8 JSON `{"package": str, "fileClass": str, "classes": [{"name", "start", "end"}], "mains": [{"class", "owner"?, "start", "end"}], "tests": [{"name", "start", "testng": bool, "methods": [{"name", "start"}]}]}`, byte offsets |

- Names are JVM binary names (`com.acme.Shop$Inner`). An empty `fileClass` is the file's own class,
  which the host names after the file (`App.kt` → `com.acme.AppKt`); a main with an empty `class`
  runs in it. `owner` is where the object or class that holds a main is declared; `classes` lists
  outer classes before the ones inside them.
- The host turns offsets into lines and drops nameless entries. The Java run and test kinds use the
  answer for gutter icons, Run Context Configuration and breakpoints in the file
  (`zen_syntax::TextFeatures::jvm`; `zen_plugins::native::Exports::jvm` in the host's tests).

One more optional export of ABI v1 ([ADR-0214](adr/0214-properties-plugin-and-resource-bundles.md)):

| Export | Input | Result |
|---|---|---|
| `zen_inlays` | the UTF-8 text | UTF-8 JSON `[{"offset": u32, "text": str}]`: labels the editor draws before the character at each byte offset, as a language server's inlay hints |

- The host drops inlays out of bounds, not on a character boundary or blank, keeps the first line of
  a text (at most 120 characters) and at most 10 000 inlays, in order of offset. They are read again
  when the document changes, listed by `editor.inlayHints.list` for a file no language server gives
  hints for, and follow the switch for all inlay hints (`editor.inlayHints.enabled`). The Properties
  plug-in shows a value unescaped (`= Привет`).

One more optional export of ABI v1 ([ADR-0288](adr/0288-bundled-schemas-for-language-servers.md)), for schemas a plug-in ships:

| Export | Input | Result |
|---|---|---|
| `zen_resource` | a resource's path (UTF-8, `/`; no `..`, `.`, empty part or leading `/`) | its bytes; a zero length when the module has none |

- A plug-in declares its schemas in `plugin.json`: `"schemas": [{ "id", "name", "server", "entry",
  "files" }]`, where `server` is the language server's id, `entry` the resource of the schema and
  `files` the name patterns it applies to.
- A server declares how it takes them with `"schemaSupport": { "setting": "yaml.schemas",
  "register": "yaml/registerContentRequest" }`: the platform adds `zen-schema://<plugin>/<entry>`
  with its patterns to that setting, answers the server's `custom/schema/content` from
  `zen_resource` (a `$ref` relative to a schema stays in the plug-in), sends `register` after
  `initialized`, and refuses `vscode/content` for `http(s)` addresses: nothing is downloaded.
- `zen_workbench::schemas` is the host's side; `zen_plugins::native::Exports::resource` runs the
  export in the host's tests.
One more optional export of ABI v1 ([ADR-0286](adr/0286-plugin-run-targets.md)), for run icons:

| Export | Input | Result |
|---|---|---|
| `zen_run_targets` | the file's path relative to the project (UTF-8, `/`), a NUL byte, then the UTF-8 text | UTF-8 JSON `[{"line": u32, "name": str, "id": str, "program": str, "args": [str], "cwd": str?, "env": {str: str}, "kind": "main" \| "test"}]` |

- `line` is zero-based; `id` is stable for the same target (it defaults to `name`). `program` is
  a bare program name the plug-in declares in `plugin.json` (`"runs": ["docker"]`); the host drops a
  target with any other program, a path, an empty name, a line past the end, more than 200 targets
  and more than 32 KiB of arguments. `args`, `cwd` and the values of `env` may hold `${file}`,
  `${fileDir}` and `${projectDir}`.
- `env` cannot set `PATH`, `ZEN_*`, `DYLD_*` or `LD_*` (dropped). The arguments are the program's
  `argv`, never a shell command line.
- Each target becomes a configuration of the run kind `plugin` (`zen_workbench::plugin_runs`):
  the platform finds the program, starts it in the run path (console, Stop, Rerun, Services, the
  trust check) and checks again that the plug-in declares it. A plug-in never starts a process.
- **Commands only a person starts** ([ADR-0291](adr/0291-commands-only-a-person-starts.md)): an
  optional `"personOnly"` list in `plugin.json`, `[{"program": "terraform", "subcommands": ["apply",
  "destroy", ...]}]`. A `plugin` run whose program is a rule's and any of whose arguments that does
  not start with `-` is one of its subcommands is refused to agents (`userOnly`, whatever the agent
  setting), and a person is asked every time, with the whole command line and its folder. The rule
  is read only from the installed manifest, never from a target or a configuration. Settings →
  Plugins shows the commands.
- `zen_syntax::TextFeatures::run_targets` is the host's view; `zen_plugins::native::Exports::run_targets`
  runs it in the host's tests.

[ADR-0296](adr/0296-a-manifest-against-the-cluster.md) adds four things to run targets:
- **Window values:** `${window.<window>.<choice>}`, as a whole argument, working folder or
  environment value, is the current valid choice of the plug-in's own tool window (ADR-0290),
  filled when the run is prepared; with no valid choice the run is refused with `choiceNeeded`.
- **`"targetFiles": ["*.yaml", …]`** in `plugin.json`: the plug-in is also asked about these files'
  run targets without highlighting them; a file's targets come from the plug-in that handles it
  and from every plug-in whose patterns match.
- **Folders:** for a folder the plug-ins with `targetFiles` are asked with its path ending in `/`
  and, as text, the project-relative paths of the matching files below it (recursive, at most
  1000, no hidden entries or links); they answer targets on line 0 where `${file}` is the folder.
- **`"output": "diff"`** on a target: its standard output is a unified diff. Exit code 0 means no
  differences and 1 means it differs. On 1 the platform rebuilds both sides of each object (ask for
  full context, e.g. `diff -U1000000`) and opens them in the diff viewer: the left side is now,
  the right side the file's result.
- **`"output": "document", "documentType": "yaml"`** on a target
  ([ADR-0297](adr/0297-index-completions-hidden-nodes-and-output-documents.md)): on exit code 0
  its standard output opens as a read-only tab "Output of <command>" with that extension (ASCII
  letters and digits, `txt` otherwise). The text lives in the IDE's cache, outside the project, and
  cannot be edited or saved; at most 16 MiB from the start are kept, and the console says when it
  was cut.

Tool windows ([ADR-0289](adr/0289-plugin-tool-windows.md)) are declared in `plugin.json`, with
one more optional export to read their rows:

```json
"runs": ["docker"],
"toolWindows": [{
  "id": "docker", "title": {"en": "Docker", "ru": "Docker"},
  "sections": [{
    "id": "containers", "title": {"en": "Containers", "ru": "Контейнеры"},
    "columns": ["name", "image", "status"],
    "columnTitles": {"name": {"en": "Name", "ru": "Имя"}},
    "list": {"program": "docker", "args": ["ps", "-a", "--format", "{{json .}}"]},
    "actions": [
      {"id": "stop", "title": {"en": "Stop", "ru": "Остановить"}, "kind": "change", "rows": true,
       "program": "docker", "args": ["stop", "${row.id}"]}
    ]
  }]
}]
```

| Export | Input | Result |
|---|---|---|
| `zen_tool_rows` | the section's id, a NUL byte, then what the list command printed | UTF-8 JSON `[{"id": str, "columns": [str], "state": str?}]` |

- `program` is one of the plug-in's `"runs"`; arguments are the program's `argv`. An argument that
  is exactly `${row.id}` or `${row.<column>}` becomes that value of a row the platform listed, as
  one argument; `${row.` anywhere else is refused.
- `kind`: `stream` (a run of kind `plugin`), `terminal` (a Terminal tab), `change` (in the
  background, then the section is listed again) or `destroy` (confirmed by the person every time).
- The host keeps 5000 rows, 32 columns and the first line of each value up to 1000 characters,
  and drops rows without an id. `zen_syntax::TextFeatures::{tool_windows, tool_rows}` is the host's
  view; `zen_workbench::plugin_tools` runs the programs, under the trust and agent rules.
- The IDE shows each declared window as a tool window of its own (`zen_ui::ide::plugin_tool_window`),
  docked on its declared `"side"` (`left`, `right` or `bottom`, the default; ADR-0292) the first
  time, then where the person puts it, with its button on the tool window bar and an entry in View |
  Tool Windows while the plug-in is enabled, and `view.toolWindow.<id>` opens it (`<id>` is the window's id, or `<plugin>-<id>` when a
  built-in window has that id). It shows the section tabs, the columns and a state dot, lists the
  open section every 5 seconds (never while an action on it runs; 30 seconds after a failure,
  with the error in the window), and asks the person in a dialog before a `destroy` action.

A window's `"icon"` is inline SVG drawn by the plug-in's author from scratch (no one else's icon or
logo), painted in the theme's colours: only the shapes' coverage counts
([ADR-0292](adr/0292-plugin-tool-window-icons-and-sides.md)). `zen_image::icon::problem` checks it
before anything renders it: at most 4 KiB, a `viewBox`, at most 16 levels, and only `svg`, `g`,
`path`, `circle`, `ellipse`, `rect`, `line`, `polyline` and `polygon` with geometry and paint
attributes; no text, images, `href`, `style`, `url(...)`, entities or DOCTYPE. Anything else, or a
plain name, falls back to the generic icon, and Settings → Plugins says why.

```json
"icon": "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"><circle cx=\"8\" cy=\"8\" r=\"5\" fill=\"none\" stroke=\"black\"/></svg>",
"side": "right"
```

An action may declare **inputs**, values the person types in the window's dialog
([ADR-0295](adr/0295-values-a-person-types-into-tool-window-actions.md)): `count` (0–1000), `port`
(1024–65535) or `forward` (`LOCAL:REMOTE`, a local port 1024–65535 and a remote one 1–65535), with
`default` or `defaultFrom` (a row column whose first number starts the field when in range). An
argument that is exactly `${input.<id>}` becomes the checked value; a `confirm` question may name
it. Agents never pass values. A plug-in whose action takes a `forward` input must bind it to
127.0.0.1 and say so in its guide.

```json
{"id": "scale", "title": {"en": "Scale…", "ru": "Масштабировать…"}, "kind": "change", "rows": true,
 "program": "kubectl", "args": ["scale", "${row.id}", "--replicas", "${input.replicas}"],
 "inputs": [{"id": "replicas", "title": {"en": "Replicas", "ru": "Реплики"}, "kind": "count"}]}
```

A window may also declare **choices** for its header ([ADR-0290](adr/0290-tool-window-choices-and-confirmations.md)),
and its `change` and `destroy` actions a **confirm** question:

```json
"choices": [
  {"id": "context", "title": {"en": "Context", "ru": "Контекст"}, "columns": ["name", "namespace"],
   "list": {"program": "kubectl", "args": ["config", "view", "-o", "json"]}},
  {"id": "namespace", "title": {"en": "Namespace", "ru": "Пространство имён"}, "columns": ["name"],
   "list": {"program": "kubectl", "args": ["--context", "${window.context}", "get", "namespaces", "-o", "json"]},
   "initialFrom": "context.namespace"}
],
"actions": [
  {"id": "delete", "kind": "destroy", "rows": true, "program": "kubectl",
   "args": ["--context", "${window.context}", "--namespace", "${window.namespace}", "delete", "${row.id}"],
   "confirm": {"en": "Delete ${rows} in the context ${window.context}?", "ru": "Удалить ${rows} в контексте ${window.context}?"}}
]
```

- A choice's options are its `list` command's rows, read by `zen_tool_rows` with the section id
  `choice:<id>`. Its first value is the one remembered for the project, else the `initialFrom`
  column (`<choice>.<column>`) of an earlier choice's row, else the row whose `state` is
  `current`, else the first.
- An argument that is exactly `${window.<id>}` becomes the choice's value, in every section's and
  later choice's `list` and in every action; part of an argument, it is refused.
- Every listing lists the choices first; a remembered value no longer among its options stops the
  listing and every action (`choiceNeeded`) until the person chooses again; rows listed under other
  values are never acted on. The person chooses in the header (`plugins.toolWindow.choose`,
  refused to agents); the IDE keeps the values per project in its data folder
  (`plugin-tool-choices.json`), never in a program's own files.
- `source` (`{"variable": "KUBECONFIG", "default": "~/.kube/config"}`) names where a choice's
  options come from: the header shows the files of that variable as the programs get it (from the
  login shell's environment, [ADR-0294](adr/0294-the-login-shells-environment.md)), split by the
  path separator, or the default, with a missing file marked.
- `confirm` (`${window.<id>}`, `${rows}`) makes the action ask every time: the person in the
  window's dialog, an agent through the person, whatever its run access.

One more optional export of ABI v1, read once when the module loads, as `zen_on_type_triggers` is
([ADR-0214](adr/0214-properties-plugin-and-resource-bundles.md)):

| Export | Input | Result |
|---|---|---|
| `zen_key_references` | nothing (a zero length) | UTF-8 JSON `[{"files": [str], "keys": [str], "bundles": [str]}]`: in files matching `files` (name patterns, `*.java`), the string literal that is the first argument of a call to one of `keys` names a key of the plug-in's files, and of one of `bundles` names a bundle of them |

- The platform then completes such a literal with the keys the plug-in's outline gives for the
  project's files of its type (entries of kind `property` or `key`), and Go to Declaration opens the
  key; a bundle is a file's path under the nearest `resources` (or `java`, `kotlin`) folder, dotted,
  without a locale suffix and extension (`i18n.messages`). When the file names bundles, only their
  keys are offered. A module whose export traps or answers invalid JSON does not load; at most 32
  declarations are kept. The Properties plug-in declares `getString`, `getObject`,
  `getStringArray`, `containsKey`, `getMessage` and `getBundle` for Java, Kotlin, Groovy and Scala.

| Feature | Commands |
|---|---|
| Colours | `editor.colors.list {path}` → `{"version", "colors": [{"start", "end", "line", "color": "#rrggbbaa", "text"}]}` (character offsets; kept per document version); `editor.colors.set {path, offset, color}` → `{"applied", "text"}` writes `#rrggbb[aa]` into the literal at `offset` in the literal's own notation. The gutter shows the first colour of each line as a swatch; a click opens a colour picker. |
| Typing | `editor.edit.type` with one character, a single caret and no selection: when the character is a trigger, the plug-in's answer is applied after it as its own undo step (the HTML plug-in adds `</tag>` after `>` of a start tag and the open element's name after `</`). The session setting `editor.smartKeys.closingTags` (Settings → Editor → Smart Keys: HTML/CSS) turns it off. |

One more optional export of ABI v1 ([ADR-0237](adr/0237-new-project-wizard-and-project-generators.md)),
for a plug-in that declares `projectGenerators`:

| Export | Input | Result |
|---|---|---|
| `zen_project` | UTF-8 JSON `{"step": "form" \| "files" \| another step, "generator": id, "language": str, "name": str, "values": {...}, "settings": {id: str}, "fetched": {...}}` | UTF-8 JSON, one of `{"form": {"fields": [...]}}`, `{"files": [{"path": str, "text": str, "executable": bool?}]}`, `{"archive": {"url": str, "headers": {...}?}}`, `{"fetch": [{"id": str, "url": str, "headers": {...}?, "cache": bool?}]}`, `{"error": str}`, or for another step what that step's caller reads |

- Another step is asked by the platform for one feature, with the same `fetch` rounds: `starters` of the Spring Boot generator ([ADR-0252](adr/0252-edit-starters-of-an-existing-project.md)).
- `step` is `form` only for a generator with `dynamicForm`; `files` both for `project.new.preview` and for
  creating, with every field's value (`values`, defaults filled in, hidden fields left out). A
  generator is called off the UI thread.
- `{"error": ...}` is the plug-in's own message for the person: the wizard shows it, nothing is
  created, and the plug-in stays on.
- `{"fetch": ...}` needs `network`: the IDE fetches each address (at most 16 at a time, 16 MiB each,
  `http` and `https` only) and calls the export again with the same input and
  `"fetched": {id: {"status": u16, "text": str}}`, or `{"error": str}` for an address that failed, at
  most 8 times in all. The IDE sends `User-Agent: zen-ide/<version>` itself, and asks nothing while
  System Settings → **Project Generators** has the network off.
- `"cache": true` keeps a `200` answer in the data folder for its `Cache-Control: max-age` and sends
  `If-None-Match` afterwards. When the service cannot be reached or answers 5xx, the last saved
  answer comes instead and the form says so (`notice`).
- `{"archive": ...}`, as the answer of `files`, needs `network`: the IDE downloads a zip (at most
  16 MiB, 64 MiB unpacked) and it becomes the files, in memory: the plug-in sees no bytes. A path
  with `..` or that is absolute refuses the archive, and so does a link; a single top folder is
  dropped; `executable` comes from the entry's Unix mode. Create uses what the preview downloaded.
- `settings` has every declared setting's value, its `default` until the person changes it.
- File paths are relative, with `/`, and must stay inside the project; otherwise nothing is created.
  A trap, running out of fuel or a result that is not JSON reports that the plug-in stopped working.
- `zen_plugins::generator::PluginGenerator` is the host's side.

### Project index

An optional export of ABI v1 ([ADR-0293](adr/0293-gitops-project-index.md)), for a plug-in that
needs the whole project, not one file: the GitOps plug-in's Flux and Argo CD objects.

- **The files.** The plug-in declares the files it reads in `plugin.json`:
  `"index": {"files": ["*.yaml", "*.yml"]}`. The patterns are file names: `*` followed by a suffix,
  or a whole name.
- **The walk.** `zen_workbench::project_index` walks the project in the background and skips:
  - `.git` and the IDE's own folders;
  - `node_modules`;
  - excluded and git-ignored folders.

  Limits: 5000 matching files, 1 MiB each, 64 MiB in all, and 100 000 paths. Past one of them the
  index says `truncated`. The walk stops when the project closes or a newer change starts another
  one. It runs again 500 ms after a matching file, or a folder, changes on disk.

| Export | Input | Result |
|---|---|---|
| `zen_index` | UTF-8 JSON `{"files": [{"path": str, "text": str}], "paths": [str], "remotes": [str]}`: the matching files, every file and folder (relative to the project, with `/`), and the fetch URLs of the project's Git remotes | UTF-8 JSON `{"kinds": [str], "nodes": [{"id", "kind", "group", "name", "namespace"?, "path", "start", "end", "detail"?, "inGraph"?}], "edges": [{"from", "to", "kind"}], "references": [{"path", "start", "end", "target"}], "problems": [{"path", "start", "end", "severity": "error" \| "warning", "message"}], "completions"?: [{"files": [str], "after": str, "items": [{"label", "detail"?}]}]}` |

- **Offsets** are byte offsets into a file's text as sent.
- **A folder or chart node** may have no span: `start` and `end` 0, with `path` the folder.
- **What the host drops:**
  - a node with no id, or whose span is outside its file, and an edge from a node it dropped;
  - a reference or problem outside its file;
  - a severity other than `error` or `warning`.

  It keeps at most 20 000 nodes, 50 000 edges, 50 000 references and 5000 problems.
- **Nodes outside the graph** ([ADR-0297](adr/0297-index-completions-hidden-nodes-and-output-documents.md)):
  a node with `"inGraph": false` (a values key) is a target of references, Go to Declaration and
  Find Usages but is left out of the index tool window.
- **Completions.** In a file of `files`, when the text before the caret ends with `after` and then
  only letters, digits, `_`, `.` or `-`, completion offers the items whose label starts with that
  text, and only them. The host keeps 1000 completions of at most 2000 items, one-line labels and
  details of at most 200 characters, and `after` of at most 64. The lookup uses the last index, so
  it works while the file is edited and never calls the plug-in.
- **Agents** get the index through `plugins.index.get` (docs/automation.md), with what lies in
  files hidden from agents left out.
- **A tool window over the index.** A section of a plug-in tool window may say `"rows": "index"`
  instead of a `list` command. Its rows are the index's nodes, and no program runs, so no trust is
  needed. The window shows them as a tree and as a layered graph:
  - sources on the left of what they feed, `dependsOn` edges dashed;
  - a click highlights the node's connections, a double-click or ⏎ opens its file at its line, or
    shows a folder in the Project tool window;
  - a filter field narrows the nodes.

  `plugins.toolWindow.rows` answers the nodes as rows (name, kind, namespace, path), each with its
  worst problem as `state`.
- **A cluster state over the index** ([ADR-0299](adr/0299-cluster-state-beside-a-project-index.md)).
  - **The declaration:** an index section may declare
    `"state": {"columns", "columnTitles", "lists": [{"program", "args"}]}`.
  - **The listings:** each runs as a section listing does: trusted projects only, not while an
    action runs, every 30 s while the window is visible, after a failure only every 2 minutes or on
    Refresh.
  - **The rows:** each output goes to `zen_tool_rows("state:<section>", output)`. A row whose `id`
    is a node's id gives that node a `cluster` (`state` `ok` | `progressing` | `error` | `unknown`,
    and the values of `columns`).
  - **The window:** it draws the state as a dot, and its details pane shows "State from <context>",
    the titled columns and the section's row actions for the selected node.
- **Another plug-in's choices.** A tool window may declare
  `"windowValues": [{"plugin", "window", "choice"}]` and use `${window.<window>.<choice>}` as a
  whole argument.
  - The value is the other window's current choice, read-only.
  - When that choice changes, the rows listed under the old value are dropped at once and listed
    again.

## Distribution

The release server publishes a signed catalog next to the release manifest:
`releases/plugins.json` and `releases/plugins.json.sig`, signed with the release key. Each plug-in
lists its id, name, description, extensions and versions; each version its ABI version (the one the
module implements, so an older IDE never installs a module it cannot load), the module's URL
(relative to the catalog), size, SHA-256 and `notes`: its release notes, the Markdown of its
section in `changelog/plugins/<id>.md` (`zen-releases publish-plugin --notes <file>`; versions
published before the field existed have none). An IDE older than the field ignores it.

```sh
cargo xtask release plugin markdown 0.1.0   # gate, build for wasm32, publish (docs/releasing.md)
```

The command checks that the module loads in the host before anything is uploaded, and takes the
notes from the changelog. Raise the version in `plugins/<id>/Cargo.toml` first. A plug-in whose
database drivers have a crate in `drivers/` is published with their programs: the build makes one
per platform (`release-build plugin <id> <version> --platform all`), signs the macOS one, and lists
each in the catalog entry's `databaseDrivers` at `artifacts/plugin-<id>/<version>/zen-db-<driver>-<version>-<platform>`
with its SHA-256 and size; the release server accepts the catalog only when it stores them so
([ADR-0191](adr/0191-driver-programs-published-with-their-plugins.md)).

A plug-in keeps its own semantic version, released when it changes, while the IDEs share one
calendar version, `YY.M.N` ([ADR-0146](adr/0146-one-calendar-version-for-every-ide-and-release-channels.md)).
The IDEs a plug-in version works with are stated against the IDE version as year and month:
"Zen IDE 26.10 and later" means every IDE from 26.10.1 on. What enforces it today is the ABI
version: an IDE never installs a module whose ABI it does not implement, and a new ABI comes with
an IDE release, so its notes name the first IDE version that loads it. A plug-in's notes say so
when a version needs a newer IDE ("Needs Zen IDE 26.11 or later").

## In the IDE

- **Every capability is a plug-in that works in every Zen IDE; a product is a bundle**
  ([ADR-0050](adr/0050-products-are-plugin-bundles.md)): the base set, in every IDE, plus the plug-ins
  of its specialisation (Go in GoGarden, the full web stack in WebWave...). Anything bundled in one
  product can be installed in the others from the catalog.
- **The base set** ([ADR-0036](adr/0036-bundled-plugins.md)) ships inside every IDE and is on from the
  start: Markdown, YAML, JSON, TOML, Docker, GitLab CI, Ignore files, Dotenv, Shell, basic web
  (HTML, CSS, JavaScript, TypeScript), XML, HTTP request files for the HTTP Client
  ([ADR-0061](adr/0061-http-client.md); the IDE sends the requests, the plug-in has no network),
  and AI Agents in the Terminal (resume adapters for Claude Code, Codex and Qwen Code, ADR-0275). Bundled plug-ins can be switched off, not
  uninstalled; a newer catalog version replaces them.
- **Each product's bundle is declared in one place**, `xtask/src/release.rs`: `BASE_PLUGINS` (the
  base set) and `PRODUCT_PLUGINS` (each IDE's specialisation, beyond the base set).
  `bundle_plugins(product, folder)` lays out the base set, then the product's own plug-ins, and
  `cargo xtask release` refuses a product without an entry. Go and Rust are built-in modules
  (below), not WebAssembly plug-ins, and no IDE may lose a plug-in it ships, so the base set stays
  as it is. Every IDE with database support on by default ships `sql`: GoGarden, RustRonin, WebWave, SqlScroll and DevopsDaruma today
  (a test ties the lists to the apps' module defaults, ADR-0182). WebWave adds `scss`, the web
  plug-in the base set lacks (the owner, 2026-10-22: frameworks such as `vue` are not bundled — the IDE
  offers them when one of their files opens, so that no IDE grows heavy with plug-ins a project may not need); the rest of the full web stack (the TypeScript language server, frameworks, npm,
  Node.js run and debug, linters) joins its list as each plug-in is written. JvmJizo adds `java`,
  `kotlin`, `maven`, `gradle`, `ant`, `properties` and `spring-boot`; DevopsDaruma `sql`,
  `terraform`, `kubernetes` and `gitops` ([ADR-0285](adr/0285-devopsdaruma-app.md)), and each infrastructure
  plug-in as it is written; a plug-in of
  `PENDING_PLUGINS` that has not landed in the repository yet is left out of the build until it
  does ([ADR-0147](adr/0147-jvmjizo-app-and-jdks.md)). For the IDEs that are
  not apps yet, the intended specialisations are: TestTotami,
  four separate QA plug-ins, `api-tests`, `e2e-tests`, `test-management` and `load-tests`, each
  switched on and off on its own and off when not chosen at the first start
  ([ADR-0067](adr/0067-testtotami.md)).
- **Bundled here or from the catalog.** An IDE knows only what it ships (its `Resources/plugins`):
  a plug-in bundled with another IDE is, here, an ordinary catalog plug-in that is offered and
  installed as any other. The catalog lists every published plug-in, bundled ones included (they
  update from it). Settings → Plugins → Installed groups the plug-ins into Downloaded and Bundled
  with this IDE; the Marketplace marks bundled ones too. `list_plugins` (MCP) reports `bundled` for
  every installed and every catalog plug-in, and `installed` (the version here) for catalog ones.
- **Development builds** (`cargo run`) ship nothing: no `Resources/plugins` sits next to
  `target/debug`, so every plug-in comes from the catalog. To start one with a product's bundle,
  `cargo xtask bundle-plugins <product> <folder>` lays it out as a release does, and
  `ZEN_BUNDLED_PLUGINS=<folder>` points the IDE at it. `cargo xtask release`, `release-linux` and
  `release-windows` check the product's bundle before building anything.
- **Built-in modules** ([ADR-0071](adr/0071-built-in-modules.md)): the first-party specialisations
  that need more than the WebAssembly ABI (Go: gopls, run, Delve, go.mod checks, the SDK) are native
  modules compiled into every IDE, each reaching it through one entry point
  (`zen_modules::ModuleDescriptor`, `register(&mut ModuleHost)`). Each app lists them with whether
  it has them on by default (GoGarden: Go on, Rust off; RustRonin the reverse; WebWave and JvmJizo both off); Settings → Plugins →
  Built-in and `plugins.module.enable`/`disable` switch them for the next start (`modules.json` in
  the data folder). They are not in `PRODUCT_PLUGINS`, which lists WebAssembly plug-ins only.
  A module adds a tab and a group of All to Search Everywhere with `ModuleHost::search_contributor`
  (`zen_modules::search::SearchContributor`, [ADR-0081](adr/0081-search-everywhere-contributors.md));
  WebAssembly plug-ins cannot yet, the ABI has no export for it.
- **Areas** ([ADR-0073](adr/0073-optional-bundle-plugins-areas.md)): a product may list some of its
  plug-ins, built-in modules or bundled WebAssembly plug-ins, in `IdeProduct::areas`. They ship but
  stay off until chosen: an area module is off by default, an area plug-in is optional
  (`PluginConfig::optional`; switched on, it goes into `enabled.json` beside `disabled.json`). On the
  product's first start a dialog offers them (Continue, Skip; `areas.json` remembers that it was
  offered), and Settings → Plugins → Built-in has **Choose Areas…**; the commands are
  `plugins.areas.list`, `choose`, `skip` and `show`. No product has areas yet (TestTotami will); a
  development build takes them from `ZEN_DEV_AREAS=<id>,<id>` to try the choice with a development
  bundle.

- Settings → Plugins has Marketplace (the catalog, with a search) and Installed (Downloaded and
  Bundled with this IDE, Check for Updates, Update, Update All), and the selected plug-in's details
  with its release notes: those of the newest version and of the installed one. Every action is a
  `plugins.*` command (`zen_workbench::plugin_manager`, docs/automation.md); install, update,
  enable, disable and uninstall take effect at once for highlighting, folding, the outline and
  completion, one install after another in the background. Installing or updating a plug-in also
  installs the language servers it declares that are not found yet, into the IDE's own folder,
  in the background ([ADR-0128](adr/0128-a-plugin-installs-its-language-servers.md)); a new
  plug-in's servers work at once, while updating, switching off or uninstalling a plug-in with
  servers asks for a restart. Uninstalling removes the IDE's own copies of its servers. The update
  at start never installs servers. Installed
  plug-ins also update themselves at start.
- Installed plug-ins live in the IDE's data folder: `plugins/<id>/plugin.wasm` and `plugin.json`
  (e.g. `~/Library/Application Support/space.ifzenelse.gogarden/plugins/markdown/`); the
  `plugin.json` keeps the description and the installed version's notes, so the details show them
  offline (the bundled ones get them from the changelog when the IDE is built). Delete the folder
  to remove a plug-in.
- "Ignore extension" is remembered in `plugins/ignored-extensions.json`, and "Don't suggest" of a
  project's offer in `plugins/ignored-project-plugins.json`.
- A project's offers look only at a catalog fetched already, which the start's update of the
  installed plug-ins fetches; what installed plug-ins did once for a project (configurations added,
  setup offered) is remembered in `plugin-projects.json` in the data folder.
- The catalog is fetched once per session, and only when it is needed: a file without a
  highlighter is opened, the Marketplace is opened, an install or update is asked for, Check for
  Updates (which fetches it again), and at start to update the installed plug-ins. The Installed
  tab and `plugins.installed.list` never fetch it.
  `ZEN_PLUGINS_CATALOG=<url>` and `ZEN_TRUST_KEY=<hex>` point an IDE at another server (for example
  a local `zen-releases serve`); `ZEN_PLUGINS_CATALOG=none` turns the offer off.
