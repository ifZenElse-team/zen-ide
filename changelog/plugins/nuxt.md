# Nuxt plug-in changelog

Every release of the Nuxt plug-in (`plugins/nuxt`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-23

### Added

- The first release, for Nuxt projects; no IDE comes with it, every IDE offers it in a notification
  when a project with `nuxt.config.ts` (`.js`, `.mjs`) at its root, or with `nuxt` among the
  dependencies of its `package.json`, opens, and it is in the Marketplace. Installing it installs
  the Vue plug-in too, whose Vue Language Server serves Nuxt's `.vue` files; Nuxt's auto-imports
  reach it from the `.nuxt/` folder through the project's `tsconfig.json`.
- The run configurations `nuxi dev`, `nuxi build`, `nuxi generate` and `nuxi preview`, added to a
  Nuxt project once and run with the project's package manager.
- `nuxi prepare` offered in a notification while the project has no `.nuxt/` folder, with its output
  in the Run tool window; later, **Run Project Setup** in Find Action.
- The file templates **Nuxt Page**, **Nuxt Layout**, **Nuxt Component**, **Nuxt Composable** and
  **Nuxt Server Route** in **New**.
