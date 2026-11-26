# CSS plug-in changelog

Every release of the CSS plug-in (`plugins/css`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-10-19

### Added

- Colour previews in the gutter for hexadecimal, `rgb()`, `rgba()`, `hsl()`, `hsla()` and named
  colours in values; a click opens a colour picker that writes the new colour back.

## 0.1.0 - 2026-10-18

### Added

- The first release, for `*.css` files: highlighting of selectors (type, class, ID, attribute,
  pseudo-classes and pseudo-elements), properties, values (numbers with units, colours, strings,
  `url()`, functions, `!important`), at-rules, comments and custom properties; folding of rule and
  at-rule blocks and of multi-line comments; the rules in the Structure view, grouped by `@media`,
  `@supports`, `@layer` and `@container`; completion of property names, values, units, at-rules
  and pseudo-classes.
