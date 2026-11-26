# Next.js plug-in changelog

Every release of the Next.js plug-in (`plugins/next`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-23

### Added

- The first release, for Next.js projects; no IDE comes with it, every IDE offers it in a
  notification when a project with `next.config.*` at its root, or with `next` in its
  `package.json`, opens, and it is in the Marketplace. Installing it installs the React plug-in too.
- The run configurations `next dev`, `next build`, `next start` and `next lint`, added once and run
  with the project's package manager.
- File templates for the App Router (`page.tsx`, `layout.tsx`, `route.ts`, `loading.tsx`,
  `error.tsx`, `not-found.tsx`, created with their names in the chosen folder) and the Pages Router
  (a page and an API route).
- Next's own TypeScript checks of the App Router come through the TypeScript Language Server when
  the project has its own TypeScript.
