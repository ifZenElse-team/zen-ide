# ADR-0132: The Svelte plug-in reads markup with its expressions hidden, and runs the Svelte language server

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0031](0031-plugins.md), [ADR-0126](0126-vue-language-server-and-npm-paths-in-server-options.md),
  [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md), [ADR-0128](0128-a-plugin-installs-its-language-servers.md),
  [ADR-0130](0130-typescript-and-javascript-through-typescript-language-server.md)

## Context

A Svelte component (`.svelte`) is markup with `{…}` expressions everywhere: in text, in attribute
values (`on:click={() => n++}`), as attributes (`{value}`, `{...rest}`), and as logic blocks
(`{#if}`, `{:else}`, `{/if}`, `{#each}`, `{#await}`, `{#key}`, `{#snippet}`) and tags (`{@html}`,
`{@const}`, `{@render}`, `{@debug}`), with a `<script>` (JavaScript or TypeScript, Svelte 5's
runes `$state`, `$derived`, `$effect`, `$props`, `$bindable`) and a `<style>` (CSS or SCSS). The
HTML tokenizer the web plug-ins share ends a tag at any `>`, so an arrow inside an attribute's
expression would break the tag. The Svelte team's language server, `svelte-language-server`
(`svelteserver --stdio`), answers for the whole component with its own TypeScript service.

## Decision

1. **`plugins/svelte` (id `svelte`, 0.1.0)**, from the catalog in every IDE and offered by the
   banner when a `.svelte` file opens; no IDE bundles it. It includes the shared code with
   `#[path]`: the HTML tokenizer, the Vue plug-in's block reader (`vue/src/sfc.rs`, for `<script>`
   and `<style>` and their `lang`), the JavaScript analyser and the CSS modules (CSS or SCSS per
   `<style>`, as in the Vue plug-in).
2. **Expressions are hidden before the markup is tokenized**: every balanced `{…}` outside
   `<script>`, `<style>` and comments has its inside replaced by `x` of the same length (strings
   skipped while matching braces), so that tokens keep their offsets; the regions are then
   highlighted from the original text — braces as macros, a block's or tag's word (`#if`, `:else if`,
   `/each`, `@html`) as a keyword, the rest as JavaScript or TypeScript (`{#each list as item}`
   as two expressions around the keyword). Directives (`on:`, `bind:`, `class:`, `style:`, `use:`,
   `transition:`, `in:`, `out:`, `animate:`, `let:`) have their name as a keyword and argument and
   `|modifiers` as attributes; component tags (`<Button>`, `<ui.Card>`) are types; runes are
   macros.
3. **One tree of elements and logic blocks** feeds folding, the Structure view (script and style
   blocks with their declarations and rules, then the markup), completion of `{/…}`, and typing:
   `>` of a start tag that is not closed adds its end tag, `</` adds the open element's name, and
   `}` of a `{#block …}` that is not closed adds `{/block}` (triggers `>/}`, ABI v1 `zen_on_type`).
4. **The language server: `svelte-language-server@0.18.5` with `typescript@5`**, installed with
   the plug-in into the IDE's folder (ADR-0127, ADR-0128). It serves `.svelte` only; `.ts` and
   `.js` files of a Svelte project stay with the TypeScript server (ADR-0130), and `.svelte` files
   never go to it. The server loads TypeScript with `require("typescript")` from where it is
   installed, so it always checks with the TypeScript installed beside it, not the project's (there
   is no option for that, unlike the Vue and TypeScript servers); it takes `svelte` (and Prettier
   and its Svelte plug-in, for formatting) from the project when the project has them, else its own.
   No initialization options are needed.
5. **The TypeScript server does not see `.svelte` imports.** That takes
   `typescript-svelte-plugin` added to tsserver. Doing it here needs one plug-in to add a
   tsserver plug-in to another plug-in's server and to install its package into that server's
   folder, a mechanism the platform does not have; it is left for later. A project that wants it
   now adds `typescript-svelte-plugin` to its devDependencies and to `compilerOptions.plugins` in
   its `tsconfig.json`; the TypeScript server uses the project's TypeScript, which loads it.

## Consequences

- Every Zen IDE can edit Svelte with one install, as Vue; the SvelteKit plug-in requires `svelte`.
- The plug-in's markup reading is its own (Svelte's braces are not Vue's), with the HTML, JavaScript
  and CSS code shared; a change to `vue/src/sfc.rs` changes the Svelte plug-in too.
