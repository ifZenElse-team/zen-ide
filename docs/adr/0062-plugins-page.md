# ADR-0062: Settings → Plugins: commands, release notes in the catalog, updates on request

- **Status:** Proposed
- **Date:** 2026-10-19
- **Extends:** [ADR-0031](0031-plugins.md) (decision 4 and its consequence "Settings → Plugins is a
  later step"), [ADR-0036](0036-bundled-plugins.md)

## Context

The owner asked for this: Settings → Plugins lists the catalog with search, versions and updates:
Installed and Marketplace tabs, Update All, and each plug-in's details with its release notes. The
release notes of plug-ins live in `changelog/plugins/<id>.md`, but the signed catalog the IDE reads
carries no notes, and the page's actions were methods the UI called directly.

## Decision

1. **The page is commands** (ADR-0007, ADR-0033): `plugins.catalog.list`, `plugins.installed.list`,
   `plugins.plugin.details`, `plugins.plugin.install`, `plugins.plugin.update`,
   `plugins.updates.check`, `plugins.updates.all`, `plugins.plugin.enable`,
   `plugins.plugin.disable` and `plugins.plugin.uninstall`, in `zen_workbench::plugin_manager` over
   a `PluginManager` trait that the IDE's plug-ins (`zen_ui::plugins::Plugins`) implement. The
   Settings page dispatches them like any client. The older MCP tools (`list_plugins` and the rest)
   stay.
2. **Each catalog version carries its release notes**: an optional `notes` field (Markdown), filled
   from the changelog by `cargo xtask release plugin` through `zen-releases publish-plugin --notes`.
   The catalog schema stays 1: the field is optional and older IDEs ignore it. An installed
   plug-in's `plugin.json` keeps its `description` and its version's `notes`, and the bundled ones
   get them when the IDE is built, so the details show them offline. The details pane shows the
   notes of the newest version and, when older, of the installed one.
3. **The catalog is fetched only on request** (AGENTS.md, rule 10): when the Marketplace is opened,
   an install or update is asked for, Check for Updates is pressed (which fetches it again), a file
   without a highlighter is opened (ADR-0031), and at start to update the installed plug-ins
   (unchanged). The Installed tab never fetches it; its Update buttons appear once the catalog is
   in. A Check for Updates that fails keeps the catalog fetched before (`checkFailed` says why);
   only a first fetch that fails leaves the catalog unavailable. The update at start installs only
   what the catalog it fetched offers; a later fetch by the page never installs anything by itself.
4. **Installs and updates are queued** and run one after another in the background; Update All
   queues every plug-in with a newer loadable version. Highlighting, folding, the outline and
   completion change at once. The language servers a plug-in declares are read at start, so a
   change to such a plug-in shows a restart notice instead.
5. **Release notes are untrusted text.** They come signed with the catalog (the signature covers
   the whole file, `notes` included), but the details pane still shows them with the small
   release-notes reader of Zen Toolbox: headings, lists, paragraphs, code and emphasis; a link shows
   only its text, and images, HTML and anything that would open or load something are plain text.
   Notes longer than 64 KiB are cut (`plugin_manager::NOTES_LIMIT`).
6. **The page asks the commands only when something changed**: a new tab or search, an action, every
   frame while the catalog loads or an install runs, and otherwise once a second, for changes made by
   agents. The list, the details and the parsed notes are kept between frames.
7. **The MCP tools stay compatible**: `list_plugins.installing` is still the name of the plug-in
   being installed (or null) and `install_plugin` still returns `{ installing: <name> }` and
   installs the newest version even over the same one; the new fields (`installingIds`, `queued`)
   are added beside them.

## Consequences

- Versions published before the field have no notes until they are published again; the page says
  that no notes were published.
- An agent and the page see the same lists, sorted by name and searched the same way
  (`PluginSummary::matches`).
- Removing an update installed over a bundled plug-in is a command (`plugins.plugin.uninstall`); the
  page offers Uninstall only for downloaded plug-ins.

## Alternatives considered

- **Reading `/releases/changelog.json`** for the notes: a second download, unsigned, and only for the
  newest notes; the signed catalog already lists each version.
- **Commands in the UI crate**: it has no JSON schema derive; the trait keeps the commands below the
  UI layer, as the theme and keymap commands are.
