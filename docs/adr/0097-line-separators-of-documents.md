# ADR-0097: Documents keep their line separators; new lines follow the file's

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0053](0053-fonts-ui-scale-and-code-style.md), [ADR-0060](0060-local-history-store.md)

## Context

The requirement: the IDE shows each file's line separator (LF, CRLF or CR) in the status bar,
changes it from there and from File → File Properties → Line Separators (on a folder,
recursively), keeps it on save, and gives new files the separator of Code Style → General → Line
separator or of an EditorConfig `end_of_line`. Our documents hold a file's text exactly as it is on
disk (`zen-text` understands LF, CRLF and CR as line breaks, as the Language Server Protocol does),
and the status bar always said `LF`. New lines were inserted as `\n` whatever the file used, so a
CRLF file edited in the IDE was saved with mixed separators.

A common approach normalises a document to `\n` and writes the separator back on save. Doing that
here would change every offset the platform hands out (language servers, Git diffs, search
results, Local History) for CRLF files.

## Decision

1. **A document keeps its text as it is on disk.** Its line separator is the one of its first line
   break (a file that mixes separators shows its first one); a file without line breaks takes the
   separator new files get. The separator is read again when the text is reloaded, undone or redone.
2. **New lines follow the document's separator.** Every edit that goes through a document
   (typing, Enter, paste, completion, live templates, formatting and other language server edits,
   replacements) has the line breaks it inserts written with the document's separator, and so has
   Replace in Files on files that are not open. A line break an edit splits or joins (a server
   that deletes the CR of a CRLF, text inserted between CR and LF) is written with the separator
   too (`zen_text::line_separator::repairs`), in the same undo step; line breaks the edits leave
   alone are never touched, so a mixed file stays mixed. Callers that place the caret after an
   edit (completion, live templates, emmet) count the separator as written. Saving writes the
   text as it is, so it keeps the separator.
3. **Changing the separator rewrites the file at once.** `file.lineSeparator.set` writes the
   converted text of an open document (its unsaved changes included) and only then changes the
   document, as one undo step (carets and folds stay where they were), so a write that fails leaves
   the document and the file as they were; it records the text before and after in Local History
   (action `lineSeparators`); files that are not open are rewritten on disk. A folder changes its
   text files recursively, skipping `.git` and the like, built-in and project-excluded folders
   inside it, the guarded files of other IDEs, links, read-only files, files over 16 MB, and files
   that are not UTF-8 text.
4. **New files**: the session setting `editor.lineSeparator.newFiles` (`system`, `lf`, `crlf`,
   `cr`; Settings → Editor → Code Style → General → Line separator, `lineSeparator` in the IDE's
   settings) gives the separator, unless the project's `.editorconfig` files set `end_of_line` for
   the file. `zen_project::editorconfig` reads them (the nearest file wins up to the project root
   or `root = true`, sections match by EditorConfig glob); only `end_of_line` is used so far.

## Consequences

- Offsets stay those of the file on disk everywhere; nothing else changes for CRLF files.
- A mixed file stays mixed until its separator is changed; the status bar shows its first one.
- Converting a document with unsaved changes saves those changes too, so the document and the file
  never differ after a conversion.
- The remaining EditorConfig properties can be added to `zen_project::editorconfig` later without
  changing this decision.
