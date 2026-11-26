# Updates

Zen Toolbox checks for new versions of itself and of the IDEs. Everything is downloaded over HTTPS
and checked against the publisher's signature before it is installed.

- When a new version of GoGarden is out, a notification in GoGarden offers **Update and Restart**
  and **What's New**. Updating saves your changes and reopens your projects in the new version.
  When Zen Toolbox installs a new version while GoGarden runs, GoGarden saves your changes and
  restarts by itself. If a file cannot be saved, or the restart fails, GoGarden stays open with your
  changes, tells you why, and moves to the new version the next time you restart it.
- In Zen Toolbox, **Update** next to a tool installs its new version; **What's new** in the tool's
  menu shows the release notes, and **Full changelog** opens the
  [changelog](https://ide.ifzenelse.space/changelog.html) on the website.

**Help → What's New in GoGarden** opens the notes of the version you run in an editor tab. They come
from the signed release manifest the IDE downloads when it checks for updates and stay on disk, so
the item works offline; only a version the IDE has never seen listed has no notes yet, and it says so.
After an update they open once by themselves, in the first project you open. Turn that off with
**Settings → Appearance & Behavior → System Settings → Show What's New after an update**.

## Versions

Every Zen IDE has the same version, a calendar version: `26.10.1` is the first release of
October 2026, `26.10.2` the second, `26.11.1` the first of November. GoGarden, RustRonin, WebWave,
JvmJizo, SqlScroll and DevopsDaruma are released together, so the same version means the same platform. Zen Toolbox and plug-ins have
versions of their own.
