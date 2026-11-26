# HTML plug-in changelog

Every release of the HTML plug-in (`plugins/html`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-10-19

### Added

- Closing tags as you type: `>` of a start tag adds its end tag after the caret, unless the element
  is empty, self-closing or already closed; `</` adds the name of the open element, also inside
  `<script>` and `<style>`.
- Colour previews in the gutter for the colours in `style` attributes and `<style>` elements; a
  click opens a colour picker.

## 0.1.0 - 2026-10-18

### Added

- The first release for HTML files: highlighting of tags, attributes, character references,
  comments and the doctype; folding of elements and comments; the element tree in the Structure
  tool window; completion of tag names, end tags, attributes and their values; and Emmet
  abbreviations expanded with Tab, such as `ul>li.item$*3` or `!` for an HTML5 page.
