# SvelteKit plug-in changelog

Every release of the SvelteKit plug-in (`plugins/sveltekit`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-23

### Added

- The first release, for SvelteKit projects; no IDE comes with it, every IDE offers it in a
  notification when a project with `@sveltejs/kit` in its `package.json` opens, and it is in the
  Marketplace. Installing it installs the Svelte plug-in too, whose language server serves the
  `.svelte` files.
- The run configurations `vite dev`, `vite build` and `vite preview`, added once and run with the
  project's package manager.
- `svelte-kit sync` offered in a notification while the project has no `.svelte-kit/` folder, with
  its output in the Run tool window; later, **Run Project Setup** in Find Action.
- File templates for the route files (`+page.svelte`, `+layout.svelte`, `+page.ts`,
  `+page.server.ts`, `+layout.ts`, `+layout.server.ts`, `+server.ts`, `+error.svelte`, created with
  their names in the chosen folder) and `hooks.server.ts`.
