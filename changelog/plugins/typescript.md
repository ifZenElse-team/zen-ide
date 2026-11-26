# TypeScript plug-in changelog

Every release of the TypeScript plug-in (`plugins/typescript`), newest first; the format is in
[../README.md](../README.md).

## 0.4.1 - 2026-11-13

### Changed

- Automatic type acquisition is off: the TypeScript server no longer installs `@types` packages
  from npm by itself for JavaScript files. Types come from the project's own `node_modules`.

## 0.4.0 - 2026-10-28

### Changed

- TypeScript coloured by meaning (ADR-0184): calls, method calls and
  `new X(…)` in the colour of function names, globals and the module's top variables as global
  variables, properties where they are declared; `true`, `false` and `null` as keywords.

## 0.3.0 - 2026-10-23

### Added

- JSX end tags as you type: typing the `>` of a start tag in `.tsx` files (`<div className="a">`,
  `<Card.Body>`, the fragment `<>`) adds its end tag after the caret, unless the element is closed
  already. Generics and casts (`<T>`, `<T,>`, `<string>`) and tags in strings and comments are left
  alone. **Insert closing tag on tag completion**
  (Settings → Editor → Smart Keys: HTML/CSS) turns it off, with the HTML one.

## 0.2.0 - 2026-10-23

### Added

- The TypeScript Language Server for `.ts`, `.mts`, `.cts` and `.tsx` files (shared with the
  JavaScript plug-in): problems as you type, completion from types, Go to Declaration, Type
  Declaration and Implementation, Find Usages, Go to Symbol, the Structure view, quick
  documentation, parameter info, usages highlighted at the caret, the call hierarchy, context
  actions, Rename across files, Reformat Code and Optimize Imports (unused imports removed, the rest
  sorted). **Install** in the banner above the first such file installs it with npm into the IDE's
  own folder, with TypeScript 5; a project's own TypeScript 5 or 6 is used first.

## 0.1.0 - 2026-10-18

### Added

- The first release, for `.ts`, `.mts`, `.cts` and `.tsx` files: everything the JavaScript plug-in
  highlights, plus type annotations, interfaces, type aliases, enums, generics, decorators and
  access modifiers.
- Folding of blocks, object, array and type literals, multi-line comments, TSX elements and groups of
  imports.
- The Structure tool window: functions, classes with their methods and fields, interfaces, type
  aliases, enums, namespaces with their members, and top-level constants and variables.
- Completion without a language server: keywords that fit at the caret (type keywords in type
  positions), names declared in the file, members of `this` inside a class, and snippets.
