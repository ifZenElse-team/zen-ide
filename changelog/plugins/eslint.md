# ESLint plug-in changelog

Every release of the ESLint plug-in (`plugins/eslint`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-24

### Added

- The first release, from the Marketplace in every IDE and offered when a project has an ESLint
  configuration (`eslint.config.*`, `.eslintrc*`, or `eslint` in `package.json`). ESLint's problems
  appear as you type in `.js`, `.ts`, `.jsx`, `.tsx`, `.vue` and `.svelte` files, beside the problems
  of their own language server; ⌥Enter offers its quick fixes (fix this problem, disable the rule for
  the line or the file, fix all), **Fix ESLint Problems** fixes the whole file, and Settings →
  Languages & Frameworks → Code Quality Tools turns it off or on and runs its fixes on save. The
  ESLint language server is installed with the plug-in into the IDE's own folder; ESLint itself comes
  from the project.
