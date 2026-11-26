# ADR-0138: A full Markdown preview, drawn natively

- **Status:** Proposed
- **Date:** 2026-10-24
- **Extends:** [ADR-0137](0137-text-with-preview.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0056](0056-plugin-abi-v1.md)

## Context

The owner, 2026-10-24, asked for a preview of Markdown too, and a full one, not one with limited
GitHub Flavored Markdown. The aim is a preview that reads as on GitHub: tables, task lists,
footnotes, alerts, highlighted code, scrolling with the text,
links that work, images. The IDE draws everything natively (egui), has no web view, and never uses
the network unless asked (AGENTS.md rule 10). `pulldown-cmark` 0.13 (MIT, pure Rust, already in the
workspace for the quick documentation and the changelogs) reads GitHub Flavored Markdown, alerts and
math included.

## Decision

1. **A preview kind `markdown`** on ADR-0137's mechanism, for `*.md` and `*.markdown`, Editor and
   Preview by default: the same switch, modes kept per kind, rendering coalesced on a worker.
2. **Reading** (`zen_workbench::markdown`, no UI): `pulldown-cmark` with tables, footnotes,
   strikethrough, task lists, GFM (alerts) and math, into blocks the window draws and agents read.
   Each block knows its source lines and its nesting (quotes, list items with their marker or
   checkbox, footnotes). Bare `https://`, `http://` and `www.` addresses become links, trailing
   punctuation left out. Headings get GitHub's anchors (`getting-started`, `-1` for the second).
   Footnotes are numbered as referenced and gathered at the end. **HTML**: a safe subset only —
   emphasis, strike, code and `<kbd>`, breaks, links, images, `<details>`/`<summary>` (shown open);
   `script`, `style`, `iframe`, `object`, `embed`, `noscript`, `template` and frames are dropped
   with what they hold; every other tag is left out and its text kept; no attribute but `href`,
   `src` and `alt` is ever read.
3. **Drawing** (`zen_ui::ide::markdown_view`, our own typography): a column of at most 860 points,
   centred; headings 28/22/18/16/14.5/13.5 points with a rule under the first two; inline code on
   the theme's code background; code blocks highlighted by the IDE's own highlighters (plug-ins and
   built-in modules, found by the fence's language through an alias table: `rust`→`rs`,
   `bash`→`sh`…) with the language and a **Copy** button; tables with their columns' alignment,
   header and stripes, narrower columns wrapping; task checkboxes; alerts with their colour, our own
   icons and translated titles; quotes with a bar; footnote references as `[1]`; every colour from
   the theme.
4. **Ticking a task** in the preview edits the source (`markdown.task.toggle`, one undo step);
   chosen over read-only boxes because the text stays the truth and the edit is visible and undoable.
5. **Scrolling together**: the editor's top line brings its block (and how far into it) to the
   preview's top; scrolling the preview brings that line to the editor's top; a frame asked for
   by one side is not taken for the user's scrolling of the other. A **click** on a block puts the
   caret on its line; on a link it follows it instead.
6. **Links**: `#anchor` scrolls the preview; a relative file opens in the IDE (a Markdown file's
   `#anchor` in its preview); `http(s)` and `mailto` open in the browser.
7. **Images**: local ones (relative to the file, absolute, `file://`) are read and shown, an SVG
   with the image viewer's renderer. **Remote images are never fetched** until the project allows
   their host: a frame with the description and **Load images from <host>** (`markdown.images.allow`),
   kept per project in `markdown-images.json` of the data folder; then at most 10 MiB, with a 15 s
   time limit, on a worker.
8. **Outline**: the Markdown plug-in (0.3.0, ABI v1) gives the headings to the Structure view and
   folds sections and fenced code.
9. **For agents**: `markdown.preview.render { path?, format }` gives the preview as plain text,
   safe HTML (`data-line` on every block) or the blocks as JSON, with the headings;
   `markdown.task.toggle`, `markdown.images.allow`, `markdown.images.list`; and the
   `editor.preview.*` commands of ADR-0137 (`blocks` in `getMode`).
10. **Not rendered yet, shown as source**: Mermaid diagrams (a code block labelled "Mermaid
    (source)") and math (TeX, in the theme's constant colour, labelled "TeX"). Pure-Rust renderers
    were looked at: Mermaid has none that is complete; TeX layout would need a math font and a
    typesetter (no mature pure-Rust one fits egui). Both are follow-ups.

## Consequences

- A README reads in the IDE as on GitHub, offline, with its links and tasks working, and agents
  read the same through MCP.
- `zen-workbench` depends on `pulldown-cmark` (already in the lock file, MIT).
- Text in the preview cannot be selected yet (a click goes to the source instead); copying a code
  block is the Copy button. Selecting text in the preview is a follow-up.
