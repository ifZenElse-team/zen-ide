# Local History

Local History keeps the versions of your files for a few days, without Git: every time you save a
file, the IDE keeps its text, so you can see what changed and get an earlier version back.

## What is kept

A version of a file is kept when:

- you save it (⌘S, Save All, an [autosave](02-editor.md#autosave), or the save before a
  commit); the first time, the text it had before is kept too;
- another program changes or deletes it (a `git checkout` in the terminal, a formatter, a code
  generator): the version shows as **External change** or **Deleted**, and a deleted file keeps
  its history;
- the IDE rewrites it on disk: **Replace in Files**, **Rename**, Git **Rollback** and a change of
  [line separators](02-editor.md#line-separators) (**Change Line Separators**) keep the text before
  (**Before Replace in Files**...) and after.

Several IDEs on one computer share the Local History of their data folder, a window and a headless
IDE for agents among them: each sees the versions the others keep.

The [consoles of the Database workspace](28-database.md) are kept too, though they live in the
IDE's data folder rather than in the project: every save of a console (also the save after a second
of quiet) keeps a version, and **Local History → Show History** on a console's tab lists them.

Nothing is kept for files in `.git`, in [excluded folders](05-project.md#excluded-folders) (an npm
package's `node_modules`, a Cargo package's `target`, folders marked excluded), for changes other
programs make to files Git ignores (a build's output), for files a link leads to outside the
project, or for another IDE's project files (`.idea`). Files larger than 16 MiB are not kept.
Images and other binary files are kept, without a text comparison.

The versions are kept in the IDE's data folder, not in the project, for 5 days and up to 512 MiB in
all; older versions are dropped when the IDE starts and every hour, but a file deleted in the last 5
days keeps its last version so you can bring it back. They stay when the IDE quits and starts again.

## Show History

Choose **File → Local History → Show History**, or right-click a file or folder in the Project tool
window or an editor tab and choose **Local History → Show History**. Without a file chosen it shows
the active file's history.

The window lists the versions, the newest first, with when they were kept and why. Choose one
(click, or ↑ and ↓) to compare it with the file's current text, unsaved changes included, in the
diff viewer (side by side or unified, F7 and ⇧F7 for the next and previous difference).

For a folder the list holds the versions of every file in it. Choose a version to see which files
changed since then (modified, deleted, or created since), and a file to compare it.

**Revert** gives the file the chosen version's text; an open file shows it as one edit, so **Undo**
(⌘Z) takes it back. For a folder, Revert gives every file that changed or was deleted since that
version its text of then; files created since are kept as they are. The revert itself is kept as a
new version.

## Labels

**File → Local History → Put Label…** puts a label at this moment, for example before a risky
change. It shows in the history of every file of the project, so you can choose it and compare or
revert each file to how it was then.

## For AI agents

Every action is a command an agent can call over MCP (`localHistory.versions.list`,
`localHistory.version.get`, `localHistory.version.diff`, `localHistory.version.revert`,
`localHistory.label.put`); see [AI agents](11-ai-agents.md).
