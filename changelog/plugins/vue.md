# Vue plug-in changelog

Every release of the Vue plug-in (`plugins/vue`), newest first; the format is in
[../README.md](../README.md).

## 0.1.3 - 2026-11-13

### Changed

- Components in the template stand out from HTML elements in the colour of types: tags that
  start with a capital letter or name a component through a dot (`<Form.Field>`), and Vue's
  built-in `component`, `transition`, `transition-group`, `keep-alive`, `teleport` and `suspense`.

## 0.1.2 - 2026-10-28

### Changed

- The script of Vue components coloured by meaning (ADR-0184): calls, method calls and
  `new X(…)` in the colour of function names, globals and the module's top variables as global
  variables, properties where they are declared; `true`, `false` and `null` as keywords.

## 0.1.1 - 2026-10-23

### Added

- `.ts` and `.js` files read the `.vue` components they import: installing the plug-in adds
  `@vue/typescript-plugin` to the TypeScript Language Server, so that a component's props are typed,
  checked and completed there and Go to Declaration opens the component.

## 0.1.0 - 2026-10-22

### Added

- The first release, for `*.vue` single-file components; WebWave comes with it, the other IDEs
  install it from the Marketplace. The template is highlighted as HTML with Vue's directives
  (`v-if`, `v-for`, `:prop`, `@event`, `#slot`, modifiers and dynamic arguments) and `{{ }}`
  interpolations, whose expressions are highlighted as JavaScript or TypeScript; the script as
  JavaScript or TypeScript and the style as CSS or SCSS, by their `lang`.
- Folding of the blocks, the template's elements, the script and the style; the blocks with the
  template's elements, the script's declarations and the style's rules in the Structure view;
  completion of directives in the template, of the blocks after `<`, and of the script's and the
  style's own words; colour previews for the style and `style` attributes.
- The Vue Language Server: problems as you type, completion from the component's types, Go to
  Declaration, quick documentation, Rename and Reformat Code in `.vue` files. **Install** in its
  banner installs it with npm, with TypeScript 5; a project's own TypeScript 5 or 6 is used first.
- A **Vue Single File Component** template in **New** (File Templates).
