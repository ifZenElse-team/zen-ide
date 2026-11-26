# Network and privacy

Zen IDE has no telemetry, no analytics, no crash reporting and no accounts: nothing about you, your
projects or how you use the IDE is sent anywhere. The panic handlers write only the local log
(`crates/platform/workbench/src/ide_log.rs`) and the list of internal errors
(`crates/platform/workbench/src/internal_errors.rs`); **Report** there opens the browser with a
prefilled issue, and nothing leaves until you submit it yourself.

Editing, navigation, Git on your own repositories, run and debug work offline. This page lists every
outbound call the IDEs and Zen Toolbox make, from the code as of 2026-11-11, with the setting that
turns it off. The website's [privacy page](../deploy/releases/site/privacy/index.html) says the same
for users. A change that adds, removes or moves a network call updates both (golden rule 10:
network access is explicit, visible and can be turned off).

Every call goes through `zen_update::fetch` (`crates/platform/update/src/fetch.rs`) or `zen_http::send`
(`crates/platform/http/src/send.rs`), and follows the IDE's proxy setting (**Settings → Appearance &
Behavior → System Settings → HTTP Proxy**, `proxy`; `crates/platform/net`). Only `https` is accepted,
or `http` to this computer. Downloads of IDEs, plug-ins and their files are checked: the manifest and
the plug-in catalog are signed (Ed25519), every file has its SHA-256.

## Automatic calls

These run in the background while the IDE or Zen Toolbox is open.

| Call | Hosts | What is sent | Code | Turn it off |
|---|---|---|---|---|
| Pictures of commit authors in the Git log and graph; a found picture is kept on disk, a missing one asked again after 7 days | `gravatar.com`; for a remote on a GitLab host, that host; for a remote on github.com, `github.com` and `avatars.githubusercontent.com` | To Gravatar a SHA-256 of the author's e-mail address; **to a GitLab host the e-mail address itself** (its avatar API); to GitHub the user name or id taken from a `users.noreply.github.com` address | `crates/platform/workbench/src/vcs/avatars.rs`, `crates/platform/ui/src/ide/git_workspace/avatars.rs` | **Settings → Version Control → Git Workspace → Show the authors' pictures**, `vcs.workspace.avatars` (`gitAvatars` for agents). **On by default** (`AVATARS_BY_DEFAULT`) |
| Go dependency updates: newer versions of the modules in an open `go.mod` or `go.work`, at most once an hour | `GOPROXY` (default `proxy.golang.org`; `GOPRIVATE`, `GONOPROXY`, `GONOSUMDB` and `replace`d modules are skipped), `go.dev` (Go releases); the `go` command the IDE starts may ask the proxy and checksum database itself | Public module paths and versions | `crates/lang/go/src/deps/proxy.rs`, `deps/mod.rs`, `deps/releases.rs`, `deps/local.rs` | **Settings → Go → Check dependencies for updates**, `go.dependencies.checkUpdates`; on by default. With it off the IDE runs `go` with `GOPROXY=off`. Power Save Mode pauses it |
| Go vulnerability checks: the database index is downloaded whole and matched on this computer | `vuln.go.dev` (or `GOVULNDB`), `api.osv.dev` for the severity; the version list from `GOPROXY` to find the fix | Only the ids of advisories that matched locally | `crates/lang/go/src/deps/vulndb.rs`, `deps/mod.rs` | **Settings → Go → Check dependencies for known vulnerabilities**, `go.dependencies.checkVulnerabilities`; on by default. Both Go settings off: no Go network at all |
| Zen Toolbox's release check: at start, every 30 minutes and when its window opens; sizes of files the manifest lacks | `ide.ifzenelse.space` | Nothing but the request | `crates/platform/toolbox/src/lib.rs`, `apps/zen-toolbox/src/main.rs` | `toolbox.updates.check` = `manual` in Zen Toolbox's `settings.json` (or `toolbox.settings.set { "updatesCheck": "manual" }`); Zen Toolbox's settings page has no switch for it yet. **Check for Updates** in its menu still checks |
| Updates of tools by themselves | `ide.ifzenelse.space` | Nothing but the request | `crates/platform/toolbox/src/auto_update.rs` | Only when `toolbox.updates.tools` or `toolbox.updates.self` is `automatic`; both are `notify` by default |
| An IDE's own update check (installed from Zen Toolbox, a macOS application, with a window): at start and every 30 minutes; downloading needs **Update** | `ide.ifzenelse.space` | Nothing but the request | `crates/platform/ui/src/ide/updates.rs` | No setting yet: only the environment variable `ZEN_RELEASES=none` |
| The plug-in catalog at every IDE start, and newer versions of installed plug-ins downloaded by themselves (not plug-ins that bring database drivers) | `ide.ifzenelse.space` | Nothing but the request | `crates/platform/plugins/src/lib.rs`, `crates/platform/ui/src/plugins.rs` | No setting yet: only the environment variable `ZEN_PLUGINS_CATALOG=none` |
| Maven and Gradle dependency search, updates and vulnerabilities | `search.maven.org`, `repo.maven.apache.org`, the repositories the build declares, `api.osv.dev` | Group and artifact names, the search text | `crates/platform/workbench/src/dependencies/sources.rs`, `inspector.rs` | **Settings → Build Tools**: `dependencies.search`, `dependencies.checkUpdates`, `dependencies.checkVulnerabilities`; **off by default** |
| Spring Boot support dates, at most once a day | `api.spring.io` | Nothing project-specific | `crates/platform/workbench/src/spring_boot/support.rs` | `springBoot.supportEnd.check`; **off by default** |

## Only when you ask

| Call | Hosts | Code |
|---|---|---|
| Installing and updating IDEs in Zen Toolbox, and an IDE's **Update** | `ide.ifzenelse.space` | `crates/platform/toolbox/src/lib.rs` |
| Plug-ins: **Marketplace**, **Check for updates**, **Update all**, installing one | `ide.ifzenelse.space` | `crates/platform/plugins/src/lib.rs` |
| Installing a language server (**Install** on its banner, `lsp.server.install`, or with its plug-in) | The archive the plug-in declares (for example download.eclipse.org for Eclipse JDT LS), or the `npm`, `go install`, `cargo install`, `rustup` the IDE starts | `crates/platform/workbench/src/language/servers.rs`, `servers/archive.rs` |
| **Download JDK** | `api.adoptium.net` (the operating system and architecture), then the package from github.com | `crates/platform/workbench/src/jdk/adoptium.rs` |
| Downloading Ant, Maven, Gradle or JaCoCo | `archive.apache.org`, `repo.maven.apache.org`, `services.gradle.org` | `crates/platform/workbench/src/ant.rs`, `maven.rs`, `gradle/download.rs`, `java/jacoco.rs` |
| New Project generators that use a service (for example Spring Initializr) | The service the plug-in declares, for example `start.spring.io`; your form choices | `crates/platform/plugins/src/generator.rs`; turned off by **Settings → System Settings → Project Generators → Let project generators use the network**, `project.generators.network` |
| Remote images in the Markdown preview | Only hosts you allowed for the project (none by default) | `crates/platform/ui/src/ide/markdown_view.rs`, `crates/platform/workbench/src/markdown/commands.rs` |
| The HTTP Client, database connections, the proxy's **Check connection**, Git fetch, pull and push | What you wrote or configured | `crates/platform/workbench/src/http.rs`, `crates/platform/db`, `crates/platform/workbench/src/network.rs`, `crates/platform/workbench/src/vcs/remote.rs` |
| A schema a YAML or JSON file names with a web address (`# yaml-language-server: $schema=https://…`, `"$schema": "https://…"`), only after **Settings → Languages & Frameworks → Schemas → Allow downloading schemas a file names**, never in a project you have not trusted; a notification names the address first | The address in the file, `http` or `https` only | `crates/platform/workbench/src/schemas.rs` |

Builds you start (Maven, Gradle, npm, cargo, go) download from their own repositories by their own
rules; the IDE runs `cargo metadata --offline`.

## Data used at run time

The Go vulnerability reports come from the Go vulnerability database, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); the IDE says so under every report's
details. [ATTRIBUTION.md](../ATTRIBUTION.md) lists this data under "Data used at run time".
