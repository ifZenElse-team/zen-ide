# Notifications

What the IDE has to tell you (a new version, a plug-in for a file type, a language server or
debugger to install, the outcome of a Git operation, a command that failed) comes as a balloon in
the bottom right corner and is kept in the Notifications tool window.

## Balloons

A balloon shows the notification's group, its title and text, and its actions as links. A balloon
with information or a warning fades after ten seconds (it stays while the pointer is over it); an
error's balloon stays until you close it with ×. At most three balloons show at once; the others
wait in the tool window. Clicking an action runs it and closes the balloon. No balloons come up
while the Notifications tool window is open: new notifications appear in it instead.

What an editor notification above the text already shows (a plug-in that supports the file, a
missing language server, a server just installed) is kept in the tool window without a balloon.
The new version offer keeps its own balloon, with the download's progress.

## The Notifications tool window

The bell on the right tool window bar opens it (also View → Tool Windows → Notifications). A dot on
the bell means there are unread notifications; it is red when one of them is an error. The tool
window lists every notification of the project window, the newest first, with its time, and marks
them read when it shows them; the ones that were new when you opened it keep a blue dot until you
close it. Each notification has its actions as links and × to remove it; **Clear All** removes them
all. A window keeps its last 500 notifications, until it closes.

## Settings

Settings → Appearance & Behavior → Notifications turns balloons off altogether or for some groups
(IDE, Updates, Plugins, Language servers, Debugger, Git, Errors). A notification without a balloon
is still listed in the tool window.

## IDE internal errors

When something inside GoGarden fails (a panic on one of its threads), a red count appears at the
right of the status bar. Click it to open **IDE Internal Errors**: the message and the stack of each
error, with arrows to step between them, and a field for what you were doing. **Copy for Report**
copies all of it, and **Open Issue** opens a new issue, filled in, in your browser. GoGarden sends
nothing itself. **Clear All** forgets the errors and hides the count.

## For AI agents

Agents read the list with `ui.notifications.list` (`unreadOnly` for the unread ones), mark them read
with `ui.notifications.markRead`, remove them with `ui.notifications.clear`, run an action with
`ui.notifications.action` and post one with `ui.notifications.post`
([AI agents](ai-agents.html)). A notification an agent posts has only a title and a text, never
links: only the IDE's own notifications have actions, so a click on a link never runs what an agent
chose. A notification keeps at most 4000 characters of text.

The internal errors are `ide.errors.list`, `ide.errors.describe`, `ide.errors.report` (the report's
text and address) and `ide.errors.clear`; `view.ideErrors.open` opens the dialog, and
`get_window_state` answers their number as `internalErrors`.
