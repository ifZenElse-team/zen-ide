# ADR-0184: Highlight kinds for declared properties and global variables

- **Status:** Proposed
- **Date:** 2026-10-28
- **Extends:** [ADR-0031](0031-plugins.md) (the plug-in ABI's highlight kinds), the themes
  ([docs/themes.md](../themes.md))

## Context

The owner found TypeScript and JavaScript flat (2026-10-28): in `const d = "d"; function test() {
console.warn(d); } test();` only `const`, `function`, `test` and the string were coloured. The
requirement: `d` and `console` are purple and italic,
`warn` and both `test`s blue, and locals and parameters keep the text colour; a property is
purple where it is declared (`{ statusCode?: number }`, `{ statusCode: … }`, a class field) and
plain where it is used (`options.statusCode`); `new Error(…)` and `Object.assign(…)` are blue.

The plug-in ABI v0 had no kind for a property or for a global variable, and the editor read the
language server's semantic tokens the way gopls marks Go, which left out library calls, property
declarations and module variables.

## Decision

1. Two highlight kinds join `zen_syntax::HighlightKind` and the plug-in ABI v0: **17, a property
   where it is declared**, and **18, a global or module variable**. A host that does not know them
   drops the record, so the text stays plain there; nothing else in the ABI changes. Their theme
   keys are `property` and `globalVariable`: Zen Dark and Zen Light draw them purple
   (the global italic), an imported JetBrains scheme takes `DEFAULT_INSTANCE_FIELD` and
   `DEFAULT_GLOBAL_VARIABLE`, a VS Code theme the matching scopes and semantic selectors.
2. The JavaScript lexer (shared by the TypeScript, Vue and Svelte plug-ins) uses them, and draws a
   call, a method call, the constructor of `new` and a global class used as a value
   (`Object.assign`) with the existing function kind. `true`, `false` and
   `null` are keywords; `undefined`, `NaN`, `Infinity`, the browser's and Node.js's globals and the
   module's top variables are global variables.
3. For `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts` and `.cts`, the editor reads
   typescript-language-server's semantic tokens on their own terms: calls of functions and methods
   in the function colour, properties where declared, module and library variables as globals;
   locals, parameters and properties in use keep the text colour. Go keeps its reading of gopls'
   tokens.

## Consequences

- The four plug-ins that read scripts are released again; an older IDE shows the new kinds plain.
- Other languages may use the two kinds; nothing else changes for them.

## Alternatives considered

- **Reuse `constant` and `semantic.call`:** constants are italic purple in Go too, but a declared
  property is not italic, and Go's calls are drawn in a different colour than JavaScript's.
- **Only the language server's tokens:** without a server (none installed yet, a plain `.js` file)
  the code would stay flat.
