# Getting started

Install Zen Toolbox, install GoGarden from it, and open your first Go project.

## Install Zen Toolbox

Zen Toolbox installs, updates and starts the Zen IDEs. In the Terminal:

```sh
curl -fsSL https://ide.ifzenelse.space/install.sh | sh
```

It runs on macOS on Apple silicon and on Linux (x64, Ubuntu 22.04 and later). On the Mac, Zen
Toolbox lives in the menu bar; click its icon to open it.

The command checks what it installs: the release list must carry the publisher's signature, and the
archive must match the hash in that list. A `.dmg` you download in a browser from the website is not
checked this way, and the Mac's Gatekeeper blocks it on first launch; prefer the command.

**Linux (preview).** Linux builds come out from time to time, not with every release. The same
command installs Zen Toolbox for your user, without `sudo`: the files
in `~/.local/share/zen-toolbox/app`, the command `~/.local/bin/zen-toolbox` and an entry in the
applications menu, then starts it. It sits in the tray where the desktop has one, and opens as a
window otherwise. To install for every user instead, download the `.deb` packages of Zen Toolbox and
GoGarden from the [website](https://ide.ifzenelse.space/) (the Linux tab) and install them with
`sudo apt install ./zen-toolbox_<version>_amd64.deb`.

**Windows (preview).** Windows 10 and 11 (x64) builds of GoGarden and Zen Toolbox come out from
time to time; download them from the [website](https://ide.ifzenelse.space/) (the Windows tab).
Unzip the archive anywhere and run `bin\gogarden.exe`. The builds are not signed yet, so SmartScreen
may warn on the first run: choose **More info**, then **Run anyway**.

## Install GoGarden

In Zen Toolbox, under **Available**, click **Install** next to GoGarden. When it is installed, it
moves to **Installed**; click it to start it. The **⋮** menu of a tool has What's new, Available versions, Settings
and Uninstall.

Zen Toolbox also puts `gogarden` and `zen` (zen-cli) in `~/.local/bin`, so that `gogarden .` in a
terminal opens the current folder, in the GoGarden that already runs if one does. If your terminal
does not find them, open Zen Toolbox's **Settings…** and click **Add to PATH** ([Starting an IDE from the terminal](13-toolbox.md#starting-an-ide-from-the-terminal)).

## Open a project

The welcome window lists your recent projects, with their folder and Git branch. **Open** picks a
folder; a recent project opens with a click, and **Remove from Recent Projects** in its context menu
forgets it. A project is a folder: for Go, the folder with `go.mod`. From a project window, **File →
Open…** opens another one (it has no shortcut: **⌘O** is Go to Type).

The switch with a globe in the welcome window's top-right corner (**English ▾** or **Русский ▾**)
changes the language of the interface at once, in every window, without a restart. It is the same
setting as **Language** in [Settings → Appearance & Behavior](09-settings.md), and it is saved with
it. In a narrow welcome window the search field gives way first, then the buttons move under it.

The welcome window's left column has more than projects. **Customize** holds the first choices,
which apply at once and are the same settings as in Settings: the color theme with **Sync with OS**,
the IDE font size, the keymap (**Configure…** opens Settings → Keymap), and the links **Import
Settings…** and **All settings…**. **Plugins** is the [Settings → Plugins](10-plugins.md) page.
**Learn** lists the chapters of this guide and the keyboard shortcuts and opens them on the website
in your browser. **Help → Welcome** brings this window forward from a project.

At the bottom of the welcome window's left column, and on the splash shown while an IDE starts
with projects, sits **Kozō**, the family's mascot: a young novice monk meditating, in a robe of the
IDE's colours and with something of its own (GoGarden's watering can, RustRonin's wooden sword,
WebWave's surfboard). He breathes and his things move; with macOS's **Reduce motion** (System
Settings → Accessibility → Display) he sits still.

Projects open as tabs of the main window, under the toolbar; drag a project tab along the row to
put it elsewhere, and the other tabs slide aside (agents use `project.tab.move`). The order is not
kept after a restart: the projects open again in the order you last used them. To give each project
a window of its own, choose **A new window** in Settings → Appearance & Behavior. The project widget in the toolbar
(its name) switches between open projects and opens recent ones; each project shows its folder and
its Git branch.

To find a project in a long list, just start typing while that popup is open: a search field
appears at the top with what you typed and only the matching projects stay, the best match first
and selected. Letters match fuzzily, in the name first and then in the path, so `bs` finds
billing-service and `fps` fast-path-search; the matched letters are marked. Typing with the
wrong keyboard layout still works: `ишд` (the keys of `bil` in the Russian layout) finds
billing-service, and the other way round; your keyboard layout is never changed. **↑** and **↓**
move, **⏎** opens the project (or brings it to the front if it is open), **⌫** edits, and **⎋**
clears the search (a second **⎋** closes the popup), as does the **×** in the field.

## Coming back

GoGarden remembers how you left it: the open projects and files, unfolded folders, the Terminal and
its tabs, and the window's size, place and full screen. While it starts, a small picture with its
version shows in the middle of the screen.

## Exiting

**⌘Q**, **GoGarden → Quit GoGarden**, **Quit** in the Dock icon's menu and the system's own quit
(logging out, restarting the Mac) ask **Quit GoGarden?** with
**Exit** and **Cancel**: **⏎** exits, **⎋** cancels. If work is still running, the
dialog names it before you decide, for example *A dependency update is running: Updating 3
dependencies*, *A Git operation is running: Fetching origin*, a run configuration or a debug
session. Exiting stops it: runs and debug sessions are stopped as the **Stop** button does, and so
are the programs of Git operations (`git fetch`, pull, push, with the helpers git starts for the
remote) and dependency updates (`go get`, `go mod tidy`); a program that has not ended two seconds
later is killed, so nothing you started keeps running after
GoGarden has quit (a restart, for an update or a plug-in, does the same). The open projects open
again at the next start.

To exit without the question, turn off **Settings → Appearance & Behavior → System Settings →
Confirm before exiting the IDE**; Quit from the Dock then exits at once too, its runs stopped.
When you log out with the question on, macOS stops the logout while GoGarden asks; after **Exit**
GoGarden quits and you can log out again.

**GoGarden → About GoGarden** shows the version and a reminder that Zen IDE is an independent
project, not affiliated with, sponsored by or endorsed by any other IDE vendor.

## Go

GoGarden understands Go through **gopls**, the Go language server. If gopls is not installed, a
notification above a Go file offers to install it; see [Go](go.html).

## Learn the shortcuts

- **Help → Tip of the Day** shows a tip with **Previous Tip** and **Next Tip**. Each tip names the
  shortcut of your keymap. The next tip continues where you stopped. Tips do not show on startup
  unless you clear **Don't show tips on startup**, the same choice as **Show tips on startup** in
  Settings → System Settings.
- **Help → My Productivity** lists every feature with its group, how many times you used it and
  when last; click a column to sort, again to reverse. The counts stay in `productivity.json` in the
  IDE's data folder and are never sent anywhere. **Reset** clears them.
- **Help → Keyboard Shortcuts PDF** writes the shortcuts of your active keymap, grouped as the menus
  and with this platform's key names, to a PDF and opens it. **Export Keyboard Shortcuts…** in Find
  Action saves it as PDF or HTML where you choose.
