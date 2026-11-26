# ADR-0131: The React and Next.js plug-ins; run configurations for some projects only; JSX end tags as you type

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md), [ADR-0130](0130-typescript-and-javascript-through-typescript-language-server.md),
  [ADR-0063](0063-plugin-colours-and-typing.md), [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

React and Next.js are, like Nuxt (ADR-0129), plug-ins offered when a project needs them and bundled
with no IDE. Neither has a language of its own: `.jsx` and `.tsx` are the base set's JavaScript and
TypeScript plug-ins' files, served by the TypeScript language server as `javascriptreact` and
`typescriptreact` (ADR-0130). What a React or Next.js project lacks in the IDE is therefore what
ADR-0129 gives a plug-in to declare (markers, run configurations, templates), and a few things the
platform did not have:

- React projects are built with many tools (Vite, Next.js, Remix, Expo, Create React App). Run
  configurations make sense only for some of them, and a plug-in's configurations were added to
  every project it supports;
- a Next.js project is a React project, so both plug-ins would be offered at once, although
  installing Next.js installs React (`requires`);
- the IDE should add the end tag when the `>` of a JSX start tag is typed, as it does in HTML; the
  JavaScript and TypeScript plug-ins did not;
- Next.js ships a TypeScript plug-in (`"plugins": [{ "name": "next" }]` in the `tsconfig.json`
  that `next` writes) that checks App Router conventions (the exports of a `page.tsx` or
  `layout.tsx`, `'use client'` boundaries, the values of `dynamic`, `revalidate`…) and completes
  and documents them. The question was whether the language server loads it.

What `tsserver` does, read in TypeScript 5.9's `typescript.js`: a project's plug-ins (those of its
`tsconfig.json`) are looked for in `--pluginProbeLocations`, then in `<tsserver.js>/../../..`, the
`node_modules` folder TypeScript itself is in; `--allowLocalPluginLoads` adds the folder of the
`tsconfig.json`. `typescript-language-server` 5.3.0 passes neither of the latter: its `plugins`
initialization option makes each named plug-in **global** (`--globalPlugins`) and adds its location
to the probe locations. So the project's plug-ins load exactly when the server runs the project's
own TypeScript (ADR-0130: the server's own order takes `<root>/node_modules/typescript` first).

## Decision

1. **A declared run configuration may say which projects it is for**: `"when"`, project markers
   (`files` at the root, `packages` in `package.json`) the project must also match
   (`zen_project::support::DeclaredConfiguration`); without it the configuration is for every
   project the plug-in supports, as before. `plugins.project.list` names only the configurations
   that apply, and `plugins.project.apply` marks a project done for a plug-in only once something
   was added, so a project that becomes a Vite project later still gets them.
2. **One offer for a plug-in and what it requires.** Of the catalog's plug-ins that match the open
   project, one that another matching plug-in requires is not offered on its own; the requiring
   plug-in's offer names it ("…; React will be installed with it").
3. **JSX end tags as you type**, in the JavaScript and TypeScript plug-ins (0.3.0; ABI v1's
   `zen_on_type`, trigger `>`), shared through `script.rs` (`jsx_on_type`): typing the `>` that
   ends a start tag (`<div className="a">`, `<Card.Body>`, `<>`) inserts `</div>` after the caret
   when the element is not closed later. From the text only, as the analyser works: the `<` must
   stand where an element can (at the start, after `( , = : ? [ { } ; & | ! >`, or `return`,
   `yield`, `default`, `case`, `await`) and not in a string or a comment; `<T>`, `<T,>`,
   `<T extends …>`, attributes that start with anything but a name or `{`, and TypeScript's type
   names (`<string>`, `<number>`…) are taken for generics or casts. The setting
   `editor.smartKeys.closingTags` turns it off with HTML's. Only the end tag: `</` completing the
   open element's name, which HTML has, is left out.
4. **The React plug-in** (`plugins/react`, ABI v0, no file types, no server): marker `react` in
   `package.json`; the configurations `vite`, `vite build` and `vite preview` (kind `npm`, `exec
   vite`) **only in a Vite project** (`when`: `vite.config.ts|js|mjs|mts` or the package `vite`);
   no other configurations, since the scripts of `package.json` have run icons already and the
   other tools (Create React App, Remix, Expo) run from them; file templates React Component
   (`.tsx` and `.jsx`), React Hook (`.ts`) and React Context (`.tsx`, a provider and its hook).
5. **The Next.js plug-in** (`plugins/next`, ABI v0, no file types, no server): `requires: ["react"]`;
   markers `next.config.js|mjs|ts` and the package `next`; the configurations `next dev`,
   `next build`, `next start` and `next lint`; file templates for the App Router with their fixed
   names in the chosen folder (`page.tsx`, `layout.tsx`, `route.ts`, `loading.tsx`, `error.tsx`
   with `'use client'`, `not-found.tsx`) and for the Pages Router (a page and an API route, named
   when created); `next.config.*` get a Next.js icon of our own, and so does `next-env.d.ts`, the
   declaration file Next.js writes, which is taken for generated code by its
   `NOTE: This file should not be edited` comment (Show Usages' generated filter) and otherwise
   left to the TypeScript server as any `.d.ts`; it is not made read-only, since the platform has
   no notion of an IDE-protected file and Next.js rewrites it. **Next's TypeScript plug-in is not
   configured by the IDE**: it loads when the project has its own TypeScript 5 or 6 in
   `node_modules` (a Next.js project with TypeScript has: `next dev` installs it when it is missing), checked
   against the real server by an ignored test with a TypeScript plug-in of the test project's own.
   Making it load with the IDE's fallback TypeScript would take an empty-named global plug-in whose
   location is the project, which is a trick on the server's options; not done.

## Consequences

- In a Next.js project with TypeScript, Next's own checks and completions in App Router files come
  through the TypeScript language server with no setting; without the project's TypeScript (none
  installed, or TypeScript 7) they are missing and nothing else is.
- Any plug-in can add configurations for a tool of the project (Vite, Storybook, Playwright…) only
  where it is used.
- The JavaScript and TypeScript plug-ins move to 0.3.0 and the IDEs get the end tags with the
  update; Vue's script blocks share the analyser but not the end tags (its `zen_on_type` is not
  exported).
- JSX end tags are a text heuristic: a `.ts` file's `<Component>value` cast (rare; `as` is the
  usual form) gets an end tag; **Undo** removes it.
