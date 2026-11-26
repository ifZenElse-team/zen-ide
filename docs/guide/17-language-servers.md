# Language servers

Language servers give the IDE errors, completion, navigation and refactorings for a language. Every
Zen IDE knows the same servers: the ones its language support and its plug-ins declare, such as
gopls for Go, rust-analyzer for Rust, the TypeScript Language Server for TypeScript and
JavaScript, the YAML, JSON and Bash language servers, Taplo for TOML, the Terraform
Language Server and the Vue Language Server for Vue.

## Which servers run

A server starts by itself when you open a file it serves and its program is installed on your
computer; it stops with the project window. The IDE looks for the program first where you set it
in the settings (below), then, for a server it installed with npm or downloaded, in its own folder
for that server, then on your login shell's `PATH` (read when the IDE starts, see
[Environment](07-terminal.md#environment); a server waits up to 5 seconds for it), then on the IDE's
own `PATH` and where the usual tools install programs: Homebrew, `~/.cargo/bin`, `~/go/bin` (and `GOBIN`, `GOPATH/bin`
for Go tools), `~/.local/bin`, npm's and nvm's folders. A program you install yourself is found
within half a minute while the project window is active.

A server that cannot start is shown as failed and is not started again on its own: fix what its log
says, then restart it. A server you stop stays stopped until you restart it. To keep a server from
starting at all, turn it off in the settings (below).

Typing never waits for a server: what the editor tells a server (your changes, its questions about
hints, colours and completion) waits in a queue that a separate thread writes, and changes made
while the server is busy reach it together. A server that crashes, or stops reading until 64 MB
wait for it, is restarted, up to three times in three minutes.

Problems come as the server finds them: published by the server, and asked for after each change
from a server that answers such questions (rust-analyzer's own analysis), so that they follow your
typing without a save. When a server says its problems changed, for example once it has loaded the
project, the open files' problems are asked for again; nothing found before the project was loaded
stays.

**No server downloads by itself.** The YAML Language Server downloads no schemas: not the public
schema catalog, and not a schema a file names with a web address
(`# yaml-language-server: $schema=https://…`). Such a file shows the problem "Schema downloads are
off" with the address. The schemas that come with the plug-ins, which need no network, are given to
the server by the IDE.

The JSON Language Server works the same way: a file whose `"$schema"` is a web address shows
"Schema downloads are off", a schema file on disk (`"$schema": "./schema.json"`) still works, and
**Allow downloading schemas a file names** (below) covers JSON files too. Taplo, the TOML server,
checks TOML syntax without schemas: it would download the public catalog and a file's schema
itself, without asking, so its schema support is off.

## YAML schemas

The YAML Language Server completes and checks a YAML file against the schema of its kind. These
schemas come with the plug-ins and work offline:

| Files | Schema | Plug-in |
|---|---|---|
| `.gitlab-ci.yml` and `*.gitlab-ci.yml` | GitLab CI/CD | GitLab CI |
| `.github/workflows/*.yml` | GitHub workflows | YAML |
| `*.k8s.yaml`, and the YAML files in a `k8s` or `kubernetes` folder | Kubernetes 1.34 | YAML |

A pattern matches the file wherever it is in the project. Compose files are checked and completed
by the [Docker plug-in](32-docker.md#docker-compose-files) itself, without the YAML server; a
schema you map to them yourself reports their keys a second time. To give another file a schema, name it in
the file's first lines: `# yaml-language-server: $schema=zen-schema://yaml/kubernetes/all.json` for
Kubernetes (`zen-schema://gitlab-ci/ci.json` for GitLab CI, `zen-schema://yaml/github/workflow.json` for a
workflow), or a JSON Schema file of the project by its path
(`# yaml-language-server: $schema=../schemas/app.json`).

**Settings → Languages & Frameworks → Schemas** lists the schemas of the plug-ins with their patterns
and the address a comment names them by. Under **Your mappings**, add a row to give files a schema
without a comment: the schema (`yaml/kubernetes` for a shipped one, a JSON Schema file of the project
such as `schemas/app.json`, or a web address) and the file patterns, separated by commas
(`deploy/*.yaml, cluster.yaml`), then **Apply**. Your row wins over a plug-in's pattern for the same
files, and the YAML Language Server restarts to take it.

**Allow downloading schemas a file names** is off at first. Turned on, the IDE downloads a schema a
file names with an `http` or `https` address, and only that: a notification names the full address
before the download starts, nothing is downloaded in a project you have not trusted, and the
schema is kept until you turn the setting off or close the IDE. Only you can turn it on; AI agents
can list the schemas and set mappings (`schemas.catalog.list`, `schemas.mappings.set`), not allow
downloads. **Go to Declaration** on an alias
(`*defaults`) goes to its anchor (`&defaults`).

## The status bar widget

The right of the status bar shows the project's language servers: a `{ }` mark, a dot coloured by
state and the server's name, or how many there are. The dot is green while a server runs, amber
while it starts, works (indexing, loading the project, `cargo check`: the widget then says what,
for example "rust-analyzer: Indexing 18%"), is being installed or is not installed, and red when it
failed. It lists the
servers that run and the ones the project needs: those whose files are in the project or open.

Click the widget to open the list of servers. Each row has the server's state, version and the
number of open files it serves, and its actions:

- **Restart** starts the server again; **Stop** stops a running one.
- **Show Log** opens the server's log, which keeps reading new lines while it is open.
- **Install**, for a server that is needed but not installed.

**Language Server Settings…** at the bottom opens the settings page. **Escape** or a click
elsewhere closes the list.

## Installing a server

The IDE never installs anything by itself, only when you ask: installing a plug-in installs the
language servers it declares in the same step (see [Plug-ins](10-plugins.md)); the servers of the
plug-ins every IDE comes with (TypeScript and JavaScript, YAML, JSON, Bash) and the update of
plug-ins at start never install one. When you open a file whose server is not installed, a
notification above the editor says so:

- **Install** runs the server's installer and the notification shows how it goes, then that the
  server is installed. The IDE uses `go install` for Go tools (with the Go SDK in use),
  `npm install --prefix` for servers written for Node.js, `cargo install` for Rust tools and
  `rustup component add` for rust-analyzer. A server written for Node.js goes into a folder of the
  IDE's own, `language-servers/<server>` in its data folder (for GoGarden on macOS
  `~/Library/Application Support/space.ifzenelse.gogarden/language-servers/`), never among your
  global npm packages; one you installed globally yourself (`npm install -g`) is still found on
  your `PATH`. A server may need more than one package, such as the Vue Language Server, which is
  installed with TypeScript 5 (see [Vue](06-web.md#vue)). To remove a server the IDE installed,
  delete its folder there. Installing again updates it in the same folder. If the
  installation fails, the notification shows the installer's last line and offers **Install** again;
  the whole output is in the server's log.
- When the installer itself is missing (Go, Node.js with npm, or Rust with Cargo or rustup), the notification
  says what to install first and links to its download page, for example **Download Go**. A few
  servers, such as Taplo, are installed by hand, and the notification says how.
- **Don't ask again** stops the offer for that server; **×** hides it in this window only.

## Servers beside a file's own

A linter's server, such as ESLint's or Biome's, checks a file beside the server of its language:
the file is open in both, the problems of both show in the editor and the Problems tool window, and
⌥Enter lists the quick fixes of both. Everything else (completion, navigation, rename) comes from
the language's server; Biome also formats when it serves the project. Biome's program is your
project's own (**Installed: In the project** in the list). See
[Linters and formatters](06-web.md#linters-and-formatters).

## Plug-ins of a server

A plug-in can extend another plug-in's server: the Svelte and Vue plug-ins add TypeScript server
plug-ins (`typescript-svelte-plugin`, `@vue/typescript-plugin`) to the TypeScript Language Server, so
that `.ts` files read the components they import. They are installed into the server's folder in
the IDE (also when the server itself is on your `PATH`), with the plug-in that adds them or with
the server, and the server starts again to load them. When one is missing, the banner above the
server's files says so and offers **Install**, only in the projects it is for (Svelte's where
`package.json` lists `svelte`, Vue's where it lists `vue`); agents see it as `missingPlugins` in
`lsp.servers.list`, and `lsp.server.install` installs it. Uninstalling the plug-in that added it
takes it out of the server's options.

## Settings → Languages & Frameworks → Language Servers

A table of every server the IDE knows: name, state, installed version, where it is installed, path
and arguments. **Installed** says where the server runs from: **Locally** (the IDE installed it in
its own folder), **Global (PATH)** (found on your `PATH` or where the usual tools install programs),
**Custom path** (the path you set below), or "—" when it is not installed; the list of the status
bar names it beside the version, and agents read it as `source` (`local`, `path`, `custom`,
`missing`) in `lsp.servers.list`. The
table fits the dialog: a column too narrow for its text, such as a long path, ends it with "…" and
shows all of it when you point at it; only when the dialog is very narrow does the table scroll
sideways. **Language Servers** and **List Language Servers** in Find Action (⇧⌘A) open this page.
Select a server to:

- turn it on or off with **Start this server for its files**;
- edit its **Path** (empty means it is found automatically; a program name is looked up as above)
  and **Arguments** (separated by spaces; quote an argument that has spaces), then click **Apply**:
  the server restarts with them;
- **Install** or **Restart** it, as in the status bar list, and **Offer the install again** after
  **Don't ask again**.

## Logs

A server's log holds what the server wrote to its error output, one line per message the IDE and the
server exchanged, and the output of its installations. It keeps the last 2 000 lines.

## Where the settings are kept

The servers you dismissed or turned off and the programs and arguments you set are saved in
`language-servers.json` in the IDE's data folder (on macOS, for example,
`~/Library/Application Support/space.ifzenelse.gogarden/`).

AI agents reach all of this through the commands `lsp.servers.list`, `lsp.server.install`,
`lsp.server.restart`, `lsp.server.stop`, `lsp.server.log`, `lsp.server.dismiss`,
`lsp.server.undismiss` and `lsp.server.configure` ([AI agents](ai-agents.html)).
