# ADR-0265: Chosen versions, automatic updates, new-version notifications and waiting launchers

- **Status:** Proposed
- **Date:** 2026-11-06
- **Amends:** [ADR-0029](0029-no-rollback-for-now.md) (decision 4, a new rollback),
  [ADR-0043](0043-toolbox-settings-and-shell-launchers.md) (decision 6, `toolbox.updates.tools`),
  [ADR-0111](0111-hand-files-to-the-running-ide.md) (decision 4, the channel's one request)

## Context

Five requirements for Zen Toolbox and the launchers change what a person can do with a release or a
terminal: going back to an older version, updating without being asked, hearing about a version,
completing `zen` in a shell, and using an IDE as Git's editor. Each one is small, and each one
touches a decision an earlier ADR left open or settled differently.

## Decision

1. **Rollback is an older release from the signed manifest.** ADR-0029 stands: one version of a
   tool on disk. `toolbox.tool.versions` lists the releases on the selected channel for this
   platform, newest first; `toolbox.tool.installVersion` installs one the way an update does,
   checked against the same signed manifest, which is what going back needs and costs one
   download. The manifest is the only source: a withdrawn release cannot be chosen. Settings and
   projects live outside the tools folder and are not touched. The ⋮ menu's **Available versions…**
   runs these commands. An agent may list versions but is refused `installVersion`: an older release
   can bring back a hole that a newer one closed, so going back stays with the person.
2. **`zen` completes through itself.** The three completion scripts hold no data; at each Tab they
   run the hidden `zen __complete <shell> -- <words>`, which walks the command-line definition and
   the command registry. Candidates can never go stale, one Rust function serves every shell, and
   Zen Toolbox installs three small fixed files without running `zen`. Files carry the launchers'
   marker. zsh and bash need a line in a shell profile, so `toolbox.shellScripts.addCompletions`
   has a consent of its own, as `addToPath` has. Windows gets no completions.
3. **`--wait` keeps the channel open.** `{"open": [...], "wait": true}` adds one field to
   ADR-0111's request; it is still the only request, and it still runs nothing. After
   `{"accepted": true}` the IDE keeps the connection until the tabs of the request's files are
   closed, and then closes it. End of file is the whole signal, so quitting or crashing also lets
   the client go, an older IDE (which ignores the field and closes at once) lets it go at once, and
   no time limit applies. A request with only folders is not held. With no IDE running, the client
   starts one with no paths, in a process group of its own, and hands over when it listens, for up
   to a minute. `ZEN_HANDOFF_ONLY` still forbids a window. A file inside `.git` opens in the
   repository's project: a commit message must not open `.git` as a project. An agent has
   `file.document.open` and `file.document.close`, so `--wait` needs no command of its own.
4. **New-version notifications go through `osascript` for now.** `toolbox.notifications.enabled`
   (default on) shows a system notification once per version of an installed tool or of Zen
   Toolbox, remembering in `toolbox.notifications.told`. `UNUserNotificationCenter` refuses an
   ad-hoc signed application ("Notifications are not allowed for this application", tested on
   macOS 27.0.1 from a bundle started directly and by Launch Services), and every release is
   ad-hoc signed until signing comes off hold. So the notification comes from `osascript`, whose
   click opens Script Editor, not Zen Toolbox: **Clicking opens Toolbox** waits for signing, and
   needs `UserNotifications` with a delegate in `zen-macos`, which takes an ADR of its own under
   ADR-0079.
5. **Automatic updates are off and per tool.** `toolbox.updates.automatic` lists the tools that
   update by themselves; it is empty by default, and `toolbox.updates.tools = automatic` means
   every tool. After a check, a listed tool with a newer version updates in the background if it
   is not running, then shows a system notification, whatever `toolbox.notifications.enabled` says:
   a change on disk the person did not start is always told. A tool is running when the process
   list has its executable, which covers windows, headless IDEs and every data folder; a tool that
   runs waits for the next check. A person who installs an older version has said no to the newer
   ones: `toolbox.updates.held` keeps the newest version at that moment, and automatic updates
   resume only with a release newer than it. An agent may turn them off and is refused turning them
   on, through `toolbox.tool.setAutoUpdate` or `toolbox.settings.set`: this is consent to changes on
   disk nobody started.

## Consequences

- Going back needs the release server, and a version it no longer lists is out of reach.
- Until signing, notifications carry Script Editor's name and icon, and depend on Script Editor
  being allowed in System Settings → Notifications.
- `ps` runs once per check for each tool with an automatic update.
- Each Tab starts `zen`, which takes tens of milliseconds.
- A file left open keeps `git commit` waiting, as with every editor.

## Alternatives considered

- **Keeping earlier versions on disk**: what ADR-0029 removed.
- **`NSUserNotificationCenter`**: deprecated, and in the same test a notification got no delivery date.
- **Completion scripts generated with their data**: stale after every update of `zen`.
- **A second message on the channel when the tabs close**: end of file says the same, and also
  covers a crash.
