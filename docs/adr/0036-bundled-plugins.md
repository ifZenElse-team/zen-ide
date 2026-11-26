# ADR-0036: Popular plug-ins are bundled with every IDE

- **Status:** Accepted; extended by [ADR-0050](0050-products-are-plugin-bundles.md) (products are bundles; basic web in the base set; the list is now `BASE_PLUGINS`)
- **Date:** 2026-10-16
- **Extends:** [ADR-0031](0031-plugins.md)

## Context

The owner (2026-10-16): support for other file types stays in plug-ins, but the popular ones that
nobody can work without — YAML, JSON, Docker, GitLab CI and the like — must be installed and on in
every IDE from the start.

## Decision

1. **Bundled plug-ins** are ordinary plug-ins from `plugins/`, built for WebAssembly by
   `cargo xtask release` and shipped inside the application bundle, in
   `Contents/Resources/plugins/<id>/` with `plugin.wasm` and `plugin.json`. The list is
   `BUNDLED_PLUGINS` in `xtask/src/release.rs`: Markdown, YAML, JSON, TOML, Docker (Dockerfiles and
   Compose files), GitLab CI, Ignore files, Dotenv and Shell scripts. SQL and others stay in the
   catalog.
2. They are on from the first start. A bundled plug-in cannot be uninstalled,
   only switched off (Settings → Plugins → Installed, or the `set_plugin_enabled` tool). Switched-off
   plug-ins are listed in `plugins/disabled.json` in the IDE's data folder.
3. A newer version from the catalog is installed into the user's plug-in folder and takes the
   bundled one's place; uninstalling that update brings the bundled version back.
4. Development builds find bundled plug-ins through `ZEN_BUNDLED_PLUGINS=<folder>`.

## Consequences

- Every IDE release carries a few hundred kilobytes of plug-ins.
- A plug-in installed or switched off is never offered again by the editor banner.
