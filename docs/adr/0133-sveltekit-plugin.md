# ADR-0133: The SvelteKit plug-in

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md), [ADR-0131](0131-react-and-next-plugins.md),
  [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

SvelteKit is Svelte's application framework, as Nuxt is Vue's and Next.js React's. Its components
are `.svelte` files, served by the Svelte plug-in and its `svelte-language-server` (the plug-in
with the id `svelte`); the rest is TypeScript or JavaScript. What a SvelteKit project needs from the
IDE is what ADR-0129 lets a plug-in declare:

- it is recognised by `@sveltejs/kit` among its dependencies. `svelte.config.js` is no mark of its
  own: a Svelte project built with Vite alone has it too;
- `svelte-kit sync` writes `.svelte-kit/`: the `tsconfig.json` the project's own extends
  (`"extends": "./.svelte-kit/tsconfig.json"`) and the generated `./$types` of every route
  (`PageProps`, `PageLoad`, `RequestHandler`…). Until it has run (with `vite dev`, or the
  `prepare` script of an install), the language servers see neither, and every route file has
  errors;
- the project runs with Vite: `vite dev`, `vite build`, `vite preview`;
- routes are folders of files with fixed names: `+page.svelte`, `+layout.svelte`, `+page.ts`,
  `+page.server.ts`, `+layout.ts`, `+layout.server.ts`, `+server.ts`, `+error.svelte`, and
  `src/hooks.server.ts`.

## Decision

1. **The SvelteKit plug-in** (`plugins/sveltekit`, ABI v0, no file types, no language server,
   bundled with no IDE) declares:
   - the marker package `@sveltejs/kit` only (not `svelte.config.*`, see above), and
     `requires: ["svelte"]`: a SvelteKit project, which depends on `svelte` too, is offered
     SvelteKit alone, naming Svelte, which installs with it (ADR-0131's one offer);
   - the run configurations `vite dev`, `vite build` and `vite preview` (kind `npm`, `exec vite`);
   - the setup step `svelte-kit sync`, which creates `.svelte-kit` and is offered once while it is
     missing, as Nuxt's `nuxi prepare`;
   - file templates with fixed names in the chosen folder, as Next.js's App Router ones: SvelteKit
     Page (`+page.svelte`), Layout (`+layout.svelte`), Page Load (`+page.ts`), Page Server Load
     (`+page.server.ts`), Layout Load (`+layout.ts`), Layout Server Load (`+layout.server.ts`),
     Server Route (`+server.ts`), Error Page (`+error.svelte`) and Server Hooks
     (`hooks.server.ts`). They are written for Svelte 5 and SvelteKit 2 (`$props()`, `PageProps`,
     `$app/state`); since `$types`, `$props` and `$app` read as Velocity references, their texts
     are Velocity raw blocks (`#[[…]]#`).
2. **`svelte.config.js|ts|mjs` get an icon of the platform's own** (an orange hexagon with "S"),
   for Svelte and SvelteKit projects alike, as `next.config.*` and `nuxt.config.*` have theirs.

No other platform change: everything else is ADR-0129's and ADR-0131's mechanisms.

## Consequences

- With the Svelte plug-in's language server, route files type-check against the generated
  `./$types` once `svelte-kit sync` (or `vite dev`) has run.
- The templates create `.svelte` files, a file type the Svelte plug-in gives; without it they are
  plain text until it is installed, which SvelteKit's `requires` does.
- A project that keeps SvelteKit in a workspace package other than the root has no marker at the
  root and is not offered; markers look at the project's root only (ADR-0129).
