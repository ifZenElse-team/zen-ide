# JavaScript plug-in changelog

Every release of the JavaScript plug-in (`plugins/javascript`), newest first; the format is in
[../README.md](../README.md).

## 0.4.1 - 2026-11-13

### Changed

- Automatic type acquisition is off: the TypeScript server no longer installs `@types` packages
  from npm by itself for JavaScript files. Types come from the project's own `node_modules`.

## 0.4.0 - 2026-10-28

### Changed

- JavaScript coloured by meaning (ADR-0184): calls, method calls and
  `new X(…)` in the colour of function names, globals and the module's top variables as global
  variables, properties where they are declared; `true`, `false` and `null` as keywords.

## 0.3.0 - 2026-10-23

### Added

- JSX end tags as you type: typing the `>` of a start tag in `.jsx` and `.js` files (`<div className="a">`,
  `<Card.Body>`, the fragment `<>`) adds its end tag after the caret, unless the element is closed
  already. Generics and casts (`<T>`, `<T,>`, `<string>`) and tags in strings and comments are left
  alone. **Insert closing tag on tag completion**
  (Settings → Editor → Smart Keys: HTML/CSS) turns it off, with the HTML one.

## 0.2.0 - 2026-10-23

### Added

- The TypeScript Language Server for `.js`, `.mjs`, `.cjs` and `.jsx` files (shared with the
  TypeScript plug-in): problems, completion from inferred types and JSDoc, navigation, Find
  Usages, quick documentation, Rename across JavaScript and TypeScript files, Reformat Code and
  Optimize Imports. **Install** in the banner above the first such file installs it with npm into
  the IDE's own folder.

## 0.1.0 - 2026-10-18

### Added

- The first release, for `.js`, `.mjs`, `.cjs` and `.jsx` files: highlighting of keywords, strings,
  template literals, numbers, regular expressions, comments and JSDoc tags, function and class
  names, decorators and JSX tags and attributes.
- Folding of blocks, object and array literals, multi-line comments, JSX elements and groups of
  imports.
- The Structure tool window: functions, classes with their methods and fields, and top-level
  constants and variables.
- Completion without a language server: keywords that fit at the caret, names declared in the file,
  members of `this` inside a class, and `for`, `if`, `function`, `class` and arrow function
  snippets.
