# Svelte plug-in changelog

Every release of the Svelte plug-in (`plugins/svelte`), newest first; the format is in
[../README.md](../README.md).

## 0.1.2 - 2026-10-28

### Changed

- The script of Svelte components coloured by meaning (ADR-0184): calls, method calls and
  `new X(…)` in the colour of function names, globals and the module's top variables as global
  variables, properties where they are declared; `true`, `false` and `null` as keywords.

## 0.1.1 - 2026-10-23

### Added

- `.ts` and `.js` files read the `.svelte` components they import: installing the plug-in adds
  `typescript-svelte-plugin` to the TypeScript Language Server, so that a component's props are
  typed, checked and completed there, Go to Declaration opens the component and Find Usages
  reaches it.

## 0.1.0 - 2026-10-23

### Added

- The first release, for `*.svelte` components, from the Marketplace in every IDE and offered when
  a `.svelte` file opens. The markup is highlighted as HTML with Svelte's logic blocks
  (`{#if}`, `{:else}`, `{/if}`, `{#each}`, `{#await}`, `{#key}`, `{#snippet}`), tags (`{@html}`,
  `{@const}`, `{@render}`, `{@debug}`), directives (`on:`, `bind:`, `class:`, `style:`, `use:`,
  `transition:`, `in:`, `out:`, `animate:`) and `{expressions}` as JavaScript or TypeScript; the
  script as JavaScript or TypeScript by its `lang`, with the runes; the style as CSS or SCSS.
- Folding and the Structure view of the script, the markup's elements and blocks and the style;
  completion of blocks, branches, tags, directives and runes; end tags, element names after `</`
  and `{/if}` as you type; colour previews; a **Svelte Component** file template.
- The Svelte Language Server, installed with the plug-in into the IDE's own folder: problems,
  completion, quick documentation, Go to Declaration, Find Usages, Rename and Reformat Code in
  `.svelte` files.
