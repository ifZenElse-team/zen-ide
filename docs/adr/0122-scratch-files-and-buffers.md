# ADR-0122: Scratch files and buffers live in the IDE's data folder and are ordinary files of their language

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0055](0055-language-servers.md), [ADR-0058](0058-code-actions.md)

## Context

The requirement: throwaway files ("scratches") kept outside every project, in a folder of the IDE's
configuration shared by all projects, created from a searchable language list (File → New →
Scratch File, ⇧⌘N) or from a selection (⌥⏎), five plain-text "buffers" that are reused in
turn, and both shown under Scratches and Consoles in the Project tool window.
The IDE had no notion of a language apart from file names: no
registry of languages, no per-file language override.

## Decision

1. **Where.** Scratch files are `<data folder>/scratches/scratch.<ext>`, `scratch_1.<ext>`, ...
   (the first free number per extension); buffers are `<data folder>/buffers/buffer1.txt` to
   `buffer5.txt`; the data folder is the IDE's own (`ZEN_DATA_DIR/<app id>` or the system's), so
   every project of one IDE shares them. `<data folder>/scratches.json` remembers the languages
   used last (recent first in the list) and the buffer opened last.
2. **The language is the extension.** A scratch file has the extension of its language and is
   then highlighted, completed and served by language servers like any file of that language;
   Change Language renames the file. The list is Plain Text (`txt`), Go (`go`) and Rust (`rs`),
   the languages of built-in modules in every IDE (ADR-0071), and every enabled plug-in that names
   an extension (its first one); a plug-in matched only by file names (GitLab CI) is left out.
3. **Language servers.** The window's server of the language serves a scratch file too: when one
   opens in it, the scratches (or buffers) folder joins the server's workspace folders
   (`workspace/didChangeWorkspaceFolders` when the server takes changes of them, and in
   `initialize` after a restart; `zen_lsp::Client::add_workspace_folder`); a server that does not
   declare workspace folders never gets it. gopls then loads the
   folder as an ad-hoc package outside every module, so a Go scratch has completion, problems and
   navigation for the standard library, without gopls's "No packages found" warning;
   third-party imports wait for the work that runs Go scratches. Context actions may edit scratch
   files (`edits::confine` takes them as allowed folders).
4. **Buffers.** New Scratch Buffer creates the first missing `bufferN.txt`; when all five exist it
   reuses the one after the buffer opened last. A reused buffer with text is opened only after the
   person (or agent, `clear`) chose to clear it or keep it. A renamed buffer no longer has a
   buffer's name, so it is never reused.
5. **Lifetime.** Closing the last tab of a scratch file or buffer whose text is empty (white space
   only), both in the tab and on disk, deletes it, unless a tab of another IDE window shows it (a
   window that is busy counts as showing it). A scratch file is created with an exclusive create,
   so two windows creating scratch files at once never get the same name.
6. **Commands.** `file.scratch.languages`, `.new`, `.fromSelection`, `.newBuffer`, `.list`,
   `.changeLanguage` and `.move` (into a project folder); `file.fs.rename` and `file.fs.delete` take
   scratch files besides the project's. `editor.intentions.list` ends with
   `scratch.fromSelection` when text is selected, even without a server. The window's
   `view.scratch.new` (⇧⌘N), `view.scratch.newBuffer`, `view.scratch.show`,
   `view.project.changeLanguage` and `view.project.move` (F6) open the popups and dialogs.
7. **Project view.** Scratches and Consoles follows External Libraries while the setting
   `projectViewScratches` (Advanced Settings → Project View → Enable Scratches and Consoles in the
   Project view) is on, which it is by default; there are no consoles yet.

## Consequences

- A scratch file cannot have a language its extension does not give (no override); a language
  without an extension of its own cannot be chosen.
- Our one keymap maps ⌘ to Ctrl outside macOS, so Scratch File is Ctrl+Shift+N there, not
  Ctrl+Alt+Shift+Insert, until the keymap has per-system bindings.
- Each IDE has its own scratches: GoGarden's are not RustRonin's.
- gopls loads the scratches folder as one ad-hoc package, so Go scratch files see each other's
  declarations (two `func main` report "main redeclared"); keeping them apart is left to the work
  that runs Go scratches.
- Running scratch files is not part of this decision.
