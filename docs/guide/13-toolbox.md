# Zen Toolbox

Zen Toolbox installs, updates and starts the Zen IDEs. Its settings and the terminal launchers it
can keep for you.

## The menu bar icon

Zen Toolbox keeps its torii icon in the macOS menu bar; click it to open the window. macOS puts new
icons on the left, where the notch and menu bar managers hide them first: hold **⌘** and drag the
icon to the right, next to the clock, and Zen Toolbox keeps that place from then on. With a menu bar
manager (Ice, Bartender), mark the icon as always shown in its settings.

## Download sizes

Every IDE you can install or update shows how much it downloads, next to its version:
`0.38.6 · Update to 0.38.7 · 41 MB` for an update, `0.1.0 · 37 MB` for an IDE you can install. A
new Zen Toolbox shows its size in the banner above the tools. Sizes count 1 MB as 1,000,000 bytes,
as Finder does. A version line too long for the space left of the **Update** button continues on the
next line (`0.38.27 · Update to 0.38.31` over `32 MB`); if it is still too long it ends in "…", and
holding the pointer over the row shows it in full.

## Going back to another version

The **⋮** menu of an installed tool has **Available versions…**: every release on your channel for
this computer, newest first, with its date and download size, and the installed one marked.
**Install** next to a version downloads it and puts it in place of the installed one, so an older
version is how you roll back after a bad update. Your settings and projects stay where they are.
Zen Toolbox keeps one version of a tool on disk, so going back downloads the release again, and the
newest version stays offered as an update.

Behind this menu are `toolbox.tool.versions` and `toolbox.tool.installVersion`. The `toolbox.*` commands
run only inside Zen Toolbox for now: no AI agent or script reaches them. If an agent ever does,
`toolbox.tool.installVersion` refuses it, because going back to an older version is your choice.

## The environment selector

The selector left of the gear (**Local ▾**) shows where the IDEs run; only this computer is
available for now. It is as wide as its name in your language, so the name and its ▾ never run into
the gear; a name too long for the header ends in "…", and holding the pointer over it shows it in
full.

## The Projects tab

The **Projects** tab will list the projects you open in the Zen IDEs. Until then it shows Zen
Toolbox's Kozō, the family's mascot, meditating by his toolbox ([Getting started](01-getting-started.md#open-a-project)).

## Updating by itself

Tick **Update automatically** in the **⋮** menu of a tool, and Zen Toolbox installs that tool's new
versions by itself after a check, in the background. It is off for every tool until you tick it. A
tool that is running is never updated: Zen Toolbox tries again at the next check, every 30 minutes,
as long as checks are automatic. When an update is done, or fails, a system notification says so.
After you go back to an older version, automatic updates of that tool wait for a release newer than
the one you left.

Behind this checkbox are `toolbox.tool.setAutoUpdate` (`{ "id": "gogarden", "enabled": true }`) and
`toolbox.tool.autoUpdateStatus`, which run only inside Zen Toolbox for now. If an AI agent ever
reaches them, it can read the status and turn updates off, but only you turn them on.

## Notifications

When a check finds a new version of an installed tool or of Zen Toolbox itself, macOS shows a
notification, once for each version. **Settings… → Notifications** turns them off. On macOS the
notification comes from Script Editor, because Zen Toolbox is not signed yet: allow Script Editor in
System Settings → Notifications if none appears, and open Zen Toolbox from the menu bar to update;
clicking the notification does not open it yet.

Behind this are `toolbox.notifications.enabled`, set through `toolbox.settings.set`, and
`toolbox.notifications.check`, which announces what has not been announced yet. They run only inside
Zen Toolbox for now.

## Starting at login

Zen Toolbox starts when you log in, so that after a restart it is back in the menu bar, checking for
updates, without opening its window. Click the icon to open it as usual.

To turn it off, open the settings menu (the gear) and switch off **Launch at login**, or set
`toolbox.launchAtLogin` to `false`. Quitting Zen Toolbox from its menu does not change this: it
starts again at the next login while the switch is on.

On macOS Zen Toolbox keeps a launch agent for this in
`~/Library/LaunchAgents/space.ifzenelse.toolbox.plist`, which macOS lists in **System Settings →
General → Login Items** under "Allow in the background". Turning the switch off deletes the file.
On Linux it is `~/.config/autostart/space.ifzenelse.toolbox.desktop`. On Windows Zen Toolbox does
not start at login yet.

## Settings

The settings live in one file, `settings.json`, in
`~/Library/Application Support/space.ifzenelse.toolbox/` on macOS. Missing keys keep their
defaults; a wrong value is ignored.

| Key | Default | What it does |
|---|---|---|
| `toolbox.tools.location` | next to `settings.json` | Where new tools are installed. Installed tools stay where they are. |
| `toolbox.channel` | `stable` | `stable` or `nightly`: which releases you get, for every tool and Zen Toolbox itself. |
| `toolbox.updates.check` | `automatic` | `automatic` checks for releases at start and every 30 minutes; `manual` stays offline until you choose **Check for updates**. |
| `toolbox.updates.automatic` | none | The tools that update by themselves (below). |
| `toolbox.updates.self` | `notify` | `automatic` installs a new Zen Toolbox in the background; you still choose **Restart**. |
| `toolbox.shellScripts.enabled` | `true` | Keep terminal launchers for the installed IDEs and zen-cli (below). |
| `toolbox.shellScripts.location` | `~/.local/bin` | Where the launchers go. |
| `toolbox.notifications.enabled` | `true` | Show a system notification for a new version (below). |
| `toolbox.launchAtLogin` | `true` | Start Zen Toolbox in the menu bar when you log in (above). |
| `ui.language` | `en` | `en`, `ru`, or `system` to follow the system's language; Zen Toolbox is in English until you choose. |

With manual checks, the tools list says that releases are not checked yet, with a **Check** button.

## Open-source licences

Zen Toolbox, like every Zen IDE, contains open-source software. **Settings → Open-source Licences →
Show Licences** shows its licences: Zen IDE's own, the notices of every library it is built from and
the licences of its fonts. The same files are in `Zen Toolbox.app/Contents/Resources/Licenses/`.
Agents list them with `toolbox.licences.list`.

## Starting an IDE from the terminal

Zen Toolbox writes a small script for every installed IDE, named as the IDE's program is
(`gogarden`, `rustronin`, `webwave`, `jvmjizo`, `sqlscroll`, `devopsdaruma`), and `zen` for zen-cli, which every IDE ships. They go into
`~/.local/bin` and follow every install, update and uninstall, so from any folder:

```sh
gogarden .                      # the current folder in GoGarden
gogarden main.go:12             # the file at line 12
gogarden main.go:12:5           # ... and column 5
gogarden --headless .           # GoGarden without a window, for AI agents
gogarden mcp --headless .       # the same as an MCP server on stdio
zen commands                    # zen-cli: every command
```

When the IDE already runs, the command opens the folder or file in it, in the window of the
project that holds the file or in a new one for a new project, brings that window to the front and
returns at once; `gogarden` alone just brings GoGarden to the front. Only you reach your running IDE
this way: the commands find it through a socket in its data folder that other users cannot open.

`gogarden --wait file` opens the file the same way, starts GoGarden if it is not running, and ends
when you close the file's tab (or quit the IDE). That makes the IDE your Git editor:

```sh
git config --global core.editor "gogarden --wait"
```

A file in a repository's `.git` folder opens in the repository's project. Closing a tab saves it.

Open the settings menu (the gear) and choose **Settings…** to see them: **Generate shell
scripts** turns them on or off (off removes the scripts Zen Toolbox wrote; files it did not write
are never touched), and **Shell scripts location** moves them to another folder.

A terminal finds the scripts only when their folder is on your `PATH`. The Settings page checks it
the way a new terminal window sees it. If the folder is not there, the page shows the one line that
puts it there:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Click **Add to PATH** and Zen Toolbox adds that line, with a comment, to `~/.zprofile` (and to
`~/.bash_profile` if you have one); then open a new terminal window. Zen Toolbox never changes your
shell files unless you click that button; you can also add the line yourself.

Tab completes `zen` in zsh, bash and fish: subcommands, options, command ids after `zen run`, and
a JSON skeleton of a command's parameters after `--params`. Zen Toolbox installs the completions
with the scripts, and fish finds them by itself. For zsh and bash, click **Turn on completions** on
the same page: it adds one line to the same files (zsh also needs `compinit` in your `~/.zshrc`,
which most setups have). `zen completions zsh`, `bash` or `fish` prints the script, for example for
`source <(zen completions zsh)`.

In an IDE, **Tools → Create Command-line Launcher…** writes the script for that IDE alone, with
the name and folder you choose; the `PATH` line is a checkbox there, off until you tick it. When
a terminal finds the IDE's script, **Settings → Tools → MCP Server** shows its commands with the
short name (`gogarden mcp`) instead of the full path.

In an IDE, AI agents and scripts can do this with `tools.commandLineLauncher.status` and
`tools.commandLineLauncher.create`. Behind Zen Toolbox's own settings are `toolbox.settings.get`,
`toolbox.settings.set`, `toolbox.shellScripts.generate`, `toolbox.shellScripts.remove`,
`toolbox.shellScripts.status`, `toolbox.shellScripts.addToPath`,
`toolbox.shellScripts.addCompletions`, `toolbox.login.status` (whether Zen Toolbox starts at login)
and `toolbox.tools.list` (each tool's `downloadSize` in bytes), which run only inside Zen Toolbox for
now.
