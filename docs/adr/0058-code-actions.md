# ADR-0058: Context actions (⌥Enter) and the edits a server sends

- **Status:** Proposed
- **Date:** 2026-10-19
- **Refines:** [ADR-0044](0044-how-a-refactoring-changes-files.md)

## Context

⌥Enter (Show Context Actions) lists the quick fixes of the problem at the caret
and the intentions that apply there; a bulb at the caret line says that there are some. A language
server offers the same through `textDocument/codeAction`. An action carries a `WorkspaceEdit`, a
command for `workspace/executeCommand`, both, or neither until `codeAction/resolve` fills it in. A
command's changes come back as the server's own `workspace/applyEdit` request, which ADR-0044 kept
off ("the IDE asks for edits, the server does not push them"); gopls makes many of its fixes and
refactorings that way, so without it they cannot be applied.

## Decision

1. **Two commands** (ADR-0007, ADR-0033), in `zen_workbench::language::intentions`:
   `editor.intentions.list { path?, offset?, waitMs? }` and `editor.intentions.apply { id, waitMs? }`.
   The list is kept in its own session slot, like the rename's plan, with a generation; an action's
   id is `<generation>.<index>` and is valid until the next listing or a change of the file.
2. **What is asked:** the range is the offset, else the primary selection, else the caret. The
   context gives back the server's diagnostics that touch the range. A caret on a line with a
   problem but outside it asks about the problem's range, so that ⌥Enter anywhere on the line offers
   its fix. `triggerKind` is always `Invoked`, so the bulb and ⌥Enter ask the
   same question and the second reuses the first answer. Quick fixes (kind `quickfix…`, or naming
   diagnostics) come first, preferred ones before the others; disabled actions are listed with
   their reason and cannot be applied.
3. **Applying** follows ADR-0044's rules: the file must not have changed since the listing, and an
   open read-only document refuses the action with its reason. An action without an edit or a
   command is resolved first when the server resolves. Its edit goes through `edits::parse` and
   `edits::apply` (all files or none, open files one undo step and unsaved, the `.idea` guard), and
   is recorded in the refactoring journal, so ⌘Z undoes it in every file. Unlike a rename, which the
   user previews, a context action's edits are confined to the project: every file, its real path
   with symbolic links resolved, must be inside the project root (without a project only open
   documents may change), and the `.idea` guard is checked on the real path too.
4. **`workspace/applyEdit` is accepted only while a command the IDE sent runs.** The client
   declares `workspace.applyEdit: true`. The connection hands that request to the application
   (`Client::take_requests`, `Client::respond`) instead of answering it. While
   `editor.intentions.apply` waits for `workspace/executeCommand`, each such edit of that server is
   applied with the same rules and answered `applied: true`, or `applied: false` with the reason.
   The window is exactly that one request of that one server: requests left from before are
   refused before the command is sent, those of other servers are refused on every sync meanwhile,
   and those still unread when the command answers, fails or times out (`waitMs`, cancelled) are
   refused right after it. An edit a server sends at any other time is refused (`applied: false`)
   on the next sync, so a server cannot change files on its own.
5. **One action is one step, all or nothing.** The action's own edit and every edit its command
   brings are one journal entry with one entry per file: the undo steps they made in an open
   document are joined into one (`Buffer::merge_last`), and a file on disk keeps its text from
   before the first edit. Once one of the server's edits is refused, the following ones are
   refused too, and when an edit is refused, the command fails or the wait runs out, everything the
   action already changed is undone in every file and the command fails with the reason.
6. **The UI**: ⌥⏎ is `view.intentions.open` (`ShowIntentionActions` in an imported JetBrains
   keymap), a window action that needs the editor to have the keyboard; the popup lists the actions
   with our bulb glyph, red
   for a quick fix and amber for an intention. Once the caret has stayed for a moment, the editor
   asks for the list at the caret and draws the bulb in the gutter's icon column of the caret line
   (not over a run icon); a click on it opens the popup.

## Consequences

- Agents list and apply actions with the two commands; `get_window_state` names the popup
  `intentions` while it is open.
- The waits are synchronous like the rename's: applying an action that runs a command blocks the
  window until the server answers or `waitMs` (10 s by default) runs out. A server that dies
  answers at once (the connection fails every waiting request); only a live server that does not
  answer holds the window, for at most 10 s. The bulb never blocks: it asks without waiting once
  the caret has settled and polls the same request on later frames.
- A server that sends an edit without waiting for its answer and then answers its command at once
  may have that edit refused as late; servers wait for the answer (gopls does).
- The Problems tool window does not offer the fixes of a selected problem yet.

## Alternatives considered

- **Keep `applyEdit` off and apply only actions with an edit:** most of gopls' refactorings would
  be missing.
- **Accept `applyEdit` at any time:** a server could change files nobody asked to change, outside
  the journal and the all-or-nothing check.
- **Ask with `triggerKind: Automatic` for the bulb:** the answer would differ from ⌥Enter's, two
  requests per caret position instead of one.
