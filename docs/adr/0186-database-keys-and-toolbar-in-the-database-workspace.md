# ADR-0186: Database keys of its own and an always-present toolbar in the Database workspace

- **Status:** Accepted
- **Date:** 2026-10-28
- **Supersedes:** the "Ours" column of the keymap table in ADR-0166 (decision 4) and the "Keys"
  row of its deviations table (decision 3); ADR-0166 is not part of this repository
- **Relates to:** [ADR-0038](0038-keymap.md), [ADR-0163](0163-workspaces-full-tools-in-the-editor-area.md)

## Context

The Database workspace kept the IDE's ⌘-based keys for database actions (the owner, 2026-10-25),
so pressing ⌥X, ⌃4 or ⌃] in a console gave a typed character, a bookmark jump or nothing. The
hands-on test of 2026-10-28 found that these keys are the first thing people try in a console,
and that the connection and schema selectors and the transaction controls disappear whenever a
tab other than a console is in front. The owner decided on 2026-10-28: these shortcuts inside
the Database workspace, and the toolbar always shown.

## Decision

1. **Database keys (macOS) inside the Database workspace.** While the workspace has the keyboard
   (the keymap's context `database`), these keys run the database action and
   win over the IDE's meaning; everywhere else the IDE's meaning is unchanged. The earlier ⌘-based
   key stays as a second key of the same action, so nobody loses a key they learned.

   | Action | Command or action | Database key (first) | Kept |
   |---|---|---|---|
   | Execute script | `db.console.executeScript` | ⌥X | ⌥⌘⏎ |
   | Execute in a new result tab | `db.console.executeInNewTab` | ⌘\\ | ⌃⌘⏎ |
   | Execute from the caret | `db.console.executeFromCaret` | ⌥P | none |
   | Explain plan | `db.console.explain` | ⌃⇧E | ⌥⌘E |
   | New console | `view.database.newConsole` | ⌃] | none |
   | New connection | `view.database.newConnection` | ⌃⇧N | none |
   | Format SQL | `editor.code.reformat/database` | ⌃⇧F | ⌥⌘L (the IDE's) |
   | Comment line | `editor.comment.line/database` | ⌃/ | ⌘/ (the IDE's) |
   | Commit | `db.transaction.commit` | ⌃4 | ⌃⌘K |
   | Rollback | `db.transaction.rollback` | ⌃8 | ⌃⌘Z |
   | Choose the active connection | `view.database.chooseConnection` | ⌃9 | ⌥⌘9 |
   | Choose the active schema | `view.database.chooseSchema` | ⌃0 | ⌥⌘0 |
   | Show or hide the results panel | `db.console.toggleResults` | ⌃T | none |
   | Maximise the results panel | `db.console.maximizeResults` | ⌃⇧T | none |
   | Go to a database object | `view.gotoDatabaseObject.open` | ⌃⇧D | ⌘O |
   | Advanced Copy | `view.database.copyAdvanced` | ⌃⇧C | ⇧⌘C |
   | Advanced Paste | `view.database.pasteAdvanced` | ⌃⇧V | ⌥⇧⌘V |
   | Focus the WHERE filter | `view.database.focusFilter` | ⌃⌥⇧T | ⌥⌘F |

   A key with ⌥ and a letter types a character on macOS keyboards (⌥X types "≈"); when such a key
   runs an action, the window drops the character it would have typed.
2. **Left out**, with the reason: next and previous statement on ⌥↓ and ⌥↑ (the editor's Extend
   and Shrink Selection), Go To Row on ⌃G and Rename on F2 (no such action yet), the grid's row keys
   on Insert (Macs have no Insert key; ⌘N, ⌘D and ⌘⌫ stay), Fetch Next and Fetch All
   (as decided earlier), and Toggle Grid/Record on Tab (the grid moves between cells with it).
3. **The toolbar is always there.** The connection and schema selectors, the read-only lock, the
   commit mode, Commit, Rollback and the pending-transaction counter stay on the workspace toolbar
   whatever tab is in front. With a console in front they are the console's and choose; with another
   tab they show the connection that tab is on (an object tab's), else the one selected in the
   Database Navigator, and act on that connection's main context, while the selectors only show it
   and say why. Commit and Rollback without a console (the keys too) end the main context's
   transaction of that connection.
4. **One commit-mode control.** The mode (Auto, Manual, Smart) is one selector whose tooltip says
   what the mode does; the counter beside it shows only the number of statements in the open
   transaction (0 when none), so the word "Auto" is never shown twice.
5. **New Console takes the connection in front.** Without a connection named, `db.console.new` uses
   the connection selected in the navigator, else the one of the workspace's active tab.

## Consequences

- Settings → Keymap shows the database key first for each of these actions, and every tooltip
  shows it; people who prefer the ⌘ keys can still use them or remove the others.
- Inside the workspace ⌃4, ⌃8, ⌃9 and ⌃0 no longer jump to numbered bookmarks; outside it they do.
- The toolbar reads the connection's state every frame without a console (from memory, never
  connecting), as it already did for a console.

## Alternatives considered

- **Keep the ⌘ keys only.** Rejected by the owner after the hands-on test.
- **A preset of these keys to switch on.** It hides the keys from the people who need them on
  their first day; adding them next to ours costs nothing in other contexts.
- **Hide the transaction controls without a console**, as before. Rejected: a table's staged edits
  and its main context's transaction need Commit and Rollback in reach.
