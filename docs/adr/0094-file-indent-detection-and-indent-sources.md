# ADR-0094: Detect a file's own indentation, and the order of the indentation sources

- **Status:** Proposed
- **Date:** 2026-10-22
- **Extends:** [ADR-0053](0053-fonts-ui-scale-and-code-style.md) (code style per language); replaces
  its rule that "a file of a language without an entry keeps tabs when it is already indented with
  them".

## Context

The UI specification has Settings → Editor → Code Style → General → **Detect and use
existing file indents for editing**, on by default: a file is edited with the indentation it already
uses instead of the language's code style, the status bar's indent shows it and switches between the
detected and the configured indent. Until now
`code_style::for_text` only turned tabs on, when any of the first 2000
lines started with one and the user had no entry for the language; space widths were never
detected, there was no option and no switch, and Reformat Code used the guess too.

A later change reads `.editorconfig`, which ranks above Code Style and above detection. Where
each indentation comes from must therefore be a fixed, explicit order that EditorConfig can join.

## Decision

1. **Detection** (`zen_workbench::indent_detection::detect`) reads the first 5000 lines of a file,
   at most 512 KB, when it is opened, when it is loaded from disk again and after Reformat Code
   changed it (so that Enter follows what the formatter, gofmt in Go, wrote); other edits do not
   change the result. It skips
   blank lines, lines that start with a comment, lines inside block comments and strings that run
   over several lines (by a small per-language table: `//`, `#`, `--`, `/* */`, `<!-- -->`, Go and
   JavaScript backticks, Python and TOML triple quotes, Rust strings and raw strings, Java and Kotlin
   text blocks), and continuation lines: inside a `(` or `[` opened in the middle of a line, after a
   line that ends with `+`, `=`, `&`, `|` or `\`, and lines starting with `.name`, `&&` or `||`. Of the
   remaining lines, those starting with a tab and those starting with spaces are counted; with fewer
   than three indented lines, or as many of each, nothing is detected. More tabs give **tabs**; more
   spaces give the most common increase of indentation between consecutive counted lines (the
   smaller on a tie), which must be 2, 3, 4 or 8, otherwise nothing is detected. A result the
   format does not allow is dropped: tabs in YAML, spaces in a Makefile (a recipe must start with
   a tab).
2. **The order of the sources**, lowest first (`zen_workbench::file_indent`):
   1. the built-in style of the language (`CodeStyle::BUILT_IN`; tabs for `go.mod` and Makefiles);
   2. the user's Editor → Code Style;
   3. the project's own code style (`.idea/codeStyles`);
   4. *EditorConfig, when it comes*;
   5. the detected indentation, when the option is on, the file is not switched to the configured
      style, and no layer marked as fixing the indentation set it.

   Layers 1–4 are the **configured style** (`file_indent::configured_at`, its `fixed` flag is what
   EditorConfig sets when it gives `indent_style` or `indent_size`); layer 5 makes the **effective
   style**. Detection replaces only the indentation: tabs, or spaces of the detected width; the tab
   size stays configured.
3. **Who reads which:** Tab, Enter, ⇧Tab, live templates and the status bar use the effective style
   (`project_settings::code_style_of`, `indent_unit_of`); Reformat Code and the reformat on save use
   the configured style, so that reformatting brings a file to the team's style. Whether
   Reformat Code keeps the configured style is still to be confirmed by the owner (files.md,
   "To check by hand", item 2).
4. **The option** is `IdeSettings.detectIndents` (on by default), written to the session setting
   `editor.indents.detect`; turning it off returns every open file to the configured style at once,
   since the effective style is worked out on every use. **The switch** is per open file, kept for the
   session: `editor.indent.useConfigured` and `editor.indent.useDetected`; `editor.indent.get` reports
   the effective, configured and detected indentation and the source. The status bar's indent opens a
   menu with both choices (when they differ) and Configure Indents…, which opens the language's Code
   Style page (Other File Types for a language without one); `view.indent.menu` opens it for agents.

## Consequences

- A file indented differently from the configured style is edited the way it is written; the old
  tabs-only guess and its exception for languages with a user entry are gone.
- EditorConfig adds one layer between the project's style and detection and sets `fixed` when it
  gives the indentation; nothing else changes.
- Detection runs once per open, reload or reformat, linear in at most 5000 lines and 512 KB (about
  11 ms for a 100,000-line Go file and 19 ms for a 3.9 MB one-line file in a debug build); the
  status bar no longer scans the text every frame.

## Alternatives considered

- **Detecting on every edit:** the indentation would change under the user while typing the first
  lines of a file; detection runs when the file is opened.
- **Using the detected style for Reformat Code too:** reformatting could then never bring a file to
  the configured style.
- **A threshold of one indented line:** a single line (a wrapped comment, a list item) would decide
  the whole file.
