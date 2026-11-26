# ADR-0136: Linters beside a file's language server, the project's formatters, and the ESLint, Prettier and Biome plug-ins

- **Status:** Proposed
- **Date:** 2026-10-24
- **Extends:** [ADR-0055](0055-language-servers.md), [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md),
  [ADR-0128](0128-a-plugin-installs-its-language-servers.md), [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md),
  [ADR-0130](0130-typescript-and-javascript-through-typescript-language-server.md)

## Context

A web project checks and formats its code with its own tools: ESLint (problems and fixes), Prettier
(formatting), or Biome (both). The IDE has to run them from the project (ESLint: Automatic
configuration, Run eslint --fix on save; Prettier: Automatic, Run on Reformat Code action, Run on
save). A file already has a language
server (TypeScript, Vue, Svelte); the IDE served a file with one server only, answered a server's
`workspace/configuration` with nothing, and formatted only through a language server.

What the servers need, measured against the real ones:

- **vscode-eslint-language-server** (`vscode-langservers-extracted` 4.10.0) pulls its settings per
  file with `workspace/configuration` items without a section; `null` breaks it, and it needs
  `nodePath: null`, `experimental: {}`, `codeAction.disableRuleComment` and `codeActionOnSave`;
  diagnostics are pulled only; its quick fixes are commands that work only after a code action
  request for the same version; `source.fixAll.eslint` returns an edit. ESLint itself comes from
  the project.
- **Biome** (`biome lsp-proxy`, 2.6) is the project's own program, starts its daemon itself,
  publishes diagnostics, answers `null` settings, gives quick fixes and `source.fixAll.biome` as
  edits, and formats.
- **Prettier** has no language server: `prettier --stdin-filepath <file>` reads the text and writes
  the formatted text, and respects `.prettierignore` and the project's configuration.

## Decision

1. **Servers alongside a file's language server.** A declaration with `"alongside": true` serves
   its files beside the server of their language: the window opens the files in both, adds its
   problems (published or pulled, kept per server) to the file's, lists its code actions with the
   language server's (quick fixes first) and applies them through it (its commands and the
   `workspace/applyEdit` they send), and asks it nothing else. Lookups (completion, navigation,
   rename…) stay with the file's language server; a file with no language server of its own (a
   `.js` file in an IDE without the TypeScript server) is still checked. A missing server alongside
   is never offered by the banner of a file (its plug-in installs it, ADR-0128).
   - `"formats": true`: Reformat Code formats its files with it instead of the language server
     (Biome).
   - `"fixOnSave": {"kind", "setting"}`: when the on/off `setting` is on (off by default), saving a
     file applies its code action of that kind (`source.fixAll.eslint`, `source.fixAll.biome`);
     `editor.code.fixAll` (Fix Problems, as Fix ESLint Problems) applies it on request.
   - `"settings": {section: value}`: what `workspace/configuration` is answered with, by section,
     `""` for items that name none; `${projectRoot}`, `${projectUri}` and `${projectName}` in its
     strings become the project's (ESLint's `workspaceFolder`). Any declared server may have it.
   - `"fromProject": true`: the program is the project's own, `node_modules/.bin/<command>` of the
     project or a folder above it, never installed by the IDE (`ServerSource::Project`,
     `source: "project"` in `lsp.servers.list`); without it the server is missing with its hint.
   - `"when"`: the project markers of ADR-0129; the server serves only such projects (ESLint: an
     ESLint configuration; Biome: `biome.json`); this is the Automatic configuration. They are read
     when the window's project is set; a configuration added later counts from the next start of
     the window or a change of the servers.
2. **The project's formatters.** A plug-in's `project` support (ADR-0129) may declare
   `formatters`: `{id, name, command, args, extensions, onReformat, onSave}`, the program found as
   for `fromProject`, `${file}` in the arguments standing for the file as the project names it
   (relative to its root, so that ignore files match). Reformat Code (`editor.code.reformat`) runs
   it for its files when its `onReformat` setting is on (on by default), in the background, the text
   on its standard input and the formatted text from its standard output, applied as one undo step
   that changes only what differs; an error says what the formatter said. Saving runs it when its
   `onSave` setting is on, or when Reformat code on save is on and it runs on Reformat Code; it then
   replaces the language server's reformatting. Order on save: the linters' fixes, Optimize Imports,
   then the formatting.
3. **Settings and commands.** Settings → Languages & Frameworks → **Code Quality Tools**
   (`codeQuality.settings.form`, a page of the platform) shows each linter (Automatic configuration /
   Disable, `codeQuality.linter.set`, the same as `lsp.server.configure` with `enabled`; whether it
   is installed or runs from the project; its fixes on save) and each formatter of the project's
   plug-ins (where its package is; Run on Reformat Code action; Run on save). The on/off settings
   are kept in `settings.json` (`declaredSettings`); Tools → Actions on Save links to the page.
   `codeQuality.tools.list` lists the same over MCP. One page, not a nested page per tool
   (JavaScript → Code Quality Tools → ESLint, JavaScript → Prettier): the platform's module pages
   have no nesting yet.
4. **The plug-ins** (0.1.0, from the catalog in every IDE, offered by the projects that use the
   tool, never bundled):
   - **ESLint** (`eslint`): `vscode-langservers-extracted@4.10.0`'s ESLint server, installed with the
     plug-in into the IDE's folder, alongside, for `.js .mjs .cjs .jsx .ts .mts .cts .tsx .vue .svelte`,
     `when` an ESLint configuration or `eslint` in `package.json`, with the settings above
     (`validate: on`, `onIgnoredFiles: off`) and fixes on save `codeQuality.eslint.fixOnSave`.
   - **Prettier** (`prettier`): a formatter, `prettier --stdin-filepath ${file}`, for scripts, JSON,
     CSS, SCSS, Less, HTML, Vue, Svelte, Markdown, YAML, GraphQL and Handlebars;
     `codeQuality.prettier.onReformat`, `codeQuality.prettier.onSave`.
   - **Biome** (`biome`): `biome lsp-proxy` from the project, alongside, formatting, for scripts,
     JSON, CSS and GraphQL, `when` `biome.json` or `biome.jsonc`; `codeQuality.biome.fixOnSave`.

## Consequences

- Problems, quick fixes, fixes on save and formatting of the project's own tools, in every IDE,
  with nothing configured; the language servers keep everything else.
- Prettier wins over Biome's and the language server's formatting for the files it formats.
- Running the project's programs runs code of the project, as `npm run` does; nothing is
  downloaded for Prettier and Biome, and the ESLint server is installed with its plug-in only.
