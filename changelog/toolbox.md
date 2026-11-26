# Zen Toolbox changelog

Every release of Zen Toolbox, the installer and launcher of the Zen IDEs, newest first. Each section is the release notes
users see as "What's new"; the format is in [README.md](README.md).

## 0.3.18 - 2026-11-14

### Added

- **Settings → Open-source Licences** shows the licences of everything Zen Toolbox contains.
- Zen Toolbox carries the licence notices of everything it contains in
  `Contents/Resources/Licenses/`.

## 0.3.17 - 2026-11-07

### Added

- **Update automatically** in a tool's ⋮ menu installs its new versions in the background after a check, never while the tool runs, and a notification says when it is done. It is off until you tick it for a tool.
- Tab completion for `zen` in zsh, bash and fish: subcommands, options, command ids and a JSON skeleton of a command's parameters. Zen Toolbox installs it with the shell launchers, and **Turn on completions** in Settings adds the line zsh and bash need. `zen completions <shell>` prints the script.
- **Available versions…** in a tool's ⋮ menu lists its releases on your channel: installing an older one rolls the tool back, and settings and projects stay.
- A system notification on macOS when a new version of an installed tool or of Zen Toolbox comes out, once per version. **Settings… → Notifications** turns it off.

### Changed

- Downloads follow the Mac's system proxy when no proxy variable is set.

## 0.3.16 - 2026-10-28

### Changed

- **English by default.** Zen Toolbox starts in English whatever your system's language. Choose
  Русский (or System, to follow your system) in the settings menu → Language; the choice is
  remembered across restarts and updates.

## 0.3.15 - 2026-10-25

### Changed

- **JvmJizo** in the list of planned IDEs: the family's IDE for Java and the JVM, Java first, with its
  own tile and a Kozō who sits beside a small stone Jizō in a red bib.

## 0.3.14 - 2026-10-22

### Added

- **Kozō, the mascot of the Zen IDE family.** The Projects tab shows Zen Toolbox's Kozō, a young
  novice monk in pixel art, meditating by his toolbox while a spark flies out now and then. He
  moves only while the window is visible; with macOS's Reduce motion he sits still.

## 0.3.13 - 2026-10-22

### Fixed

- **The Update button no longer covers the version line.** In a row like GoGarden's
  "0.38.27 · Update to 0.38.31 · 32 MB" the text now ends before the Update (or Install) button and
  the ⋮ menu: a line that does not fit continues on the next line, breaking at a "·", and the row
  grows to fit, in English and in Russian. Text that is still too long ends in "…" and is shown in
  full when you hold the pointer over the row.
- **The environment selector no longer runs into the settings gear.** In Russian "Локальные ▾"
  reached under the gear and its ▾ was hidden. The selector is now as wide as its name and always
  ends before the gear, in every language; a name too long for the header ends in "…" and is shown
  in full when you hold the pointer over it.

## 0.3.12 - 2026-10-21

### Added

- **The IDEs run by name in a terminal.** Zen Toolbox writes a small script for every installed
  IDE, named as its program is (`gogarden`), and `zen` for zen-cli, into `~/.local/bin`, and keeps
  them current as you install, update and uninstall: `gogarden .` opens the current folder,
  `gogarden main.go:12` a file at a line, `gogarden mcp --headless .` starts it for an AI agent.
  It is on from now on; the old `garden` and `ronin` scripts are replaced.
- **Settings…** in the gear menu opens a Settings page: **Generate shell scripts**, **Shell
  scripts location**, and whether a terminal finds that folder. If it does not, the page shows the
  one line that puts it on `PATH` and an **Add to PATH** button, which adds the line to
  `~/.zprofile` (and `~/.bash_profile` if you have one). Your shell files are changed only by that
  click.

## 0.3.11 - 2026-10-19

### Added

- **Download sizes**: every IDE you can install or update shows how much it downloads, as in
  "0.38.6 · Update to 0.38.7 · 41 MB"; a new Zen Toolbox shows its size too.
- **Launch at login**: Zen Toolbox starts when you log in and waits in the menu bar without
  opening its window, so it keeps checking for updates after a restart. Switch it off in the
  settings menu (the gear) with **Launch at login**; quitting Zen Toolbox leaves it as set.
- **TestTotami**, the coming IDE for QA (end-to-end and UI tests, API and load testing, test
  management), is in the list, with its own icon.

## 0.3.10 - 2026-10-19

### Fixed

- The menu bar icon keeps the place you drag it to (⌘-drag) from one start to the next, instead of
  coming back on the left, where the notch and menu bar managers hide it first.

## 0.3.9 - 2026-10-18

### Changed

- The application's copyright line in Finder says that Zen IDE is an independent project, not
  affiliated with, sponsored by or endorsed by any other IDE vendor.

## 0.3.8 - 2026-10-18

### Changed

- **The menu bar icon is a little smaller**, the size of the icons next to it.

## 0.3.7 - 2026-10-18

### Changed

- **The web IDE is called WebWave**, with its new icon.
- **DevopsDaruma**, the coming IDE for DevOps, is in the list.
- **A new icon**: a torii on a blue tile, in the menu bar too.

## 0.3.6 - 2026-10-18

### Fixed

- **In full screen, the window stays open when the pointer reaches the menu bar.** It closes on a
  click outside it, on the tray icon, on Escape or when you switch apps.

## 0.3.5 - 2026-10-18

### Fixed

- **A click on the tray icon closes the open window**: it used to close and open it again at once.

## 0.3.4 - 2026-10-18

### Added

- **Settings in `settings.json`**: where new tools go, the release channel (stable or nightly),
  automatic or manual checks for releases, automatic self-updates, and the language. With manual
  checks Zen Toolbox stays offline until you choose **Check**.
- **Start an IDE from the terminal**: with `toolbox.shellScripts.enabled`, Zen Toolbox keeps
  launchers such as `garden` and `ronin` in `~/.local/bin`; `garden .` opens the current folder in
  GoGarden.
- The recent projects of every Zen IDE can be listed and opened (for the coming Projects tab and for
  AI agents).

## 0.3.3 - 2026-10-17

### Added

- **Full changelog** in What's new opens every release of the tool on
  [ide.ifzenelse.space/changelog.html](https://ide.ifzenelse.space/changelog.html), where the
  changelogs of Zen Toolbox, the IDEs and the plug-ins are published with every release.

## 0.3.2 - 2026-10-17

### Changed

- **Zen Toolbox fetches releases over HTTPS** from ide.ifzenelse.space, with a TLS stack written
  entirely in Rust. Every download is still checked against the publisher's signature.

## 0.3.1 - 2026-10-17

### Fixed

- **No dots left of an installed tool's icon** on hover: they looked like a handle for dragging,
  which the list does not support.

## 0.3.0 - 2026-10-16

### Changed

- **A new Zen Toolbox icon** in the style of the IDE icons.

## 0.2.2 - 2026-10-16

### Changed

- New pastel product icons.

## 0.2.1 - 2026-10-16

### Changed

- The web IDE's name and icon are updated.

## 0.2.0 - 2026-10-16

### Added

- **What's new.** Each tool shows what an update brings; the ⋮ menu lists the notes of every
  release. After Zen Toolbox updates itself, it shows its own notes once.
- The new PhpPagoda and WebWave icons.

### Fixed

- Escape closes the What's new dialog instead of hiding Zen Toolbox.

## 0.1.8 - 2026-10-16

The new WebWave icon.

## 0.1.7 - 2026-10-16

Product icons from the Zen icon set.

## 0.1.6 - 2026-10-16

Only the active version of each IDE is kept on disk; rollback is removed for now.

## 0.1.5 - 2026-10-16

Updates no longer keep a copy of the previous Zen Toolbox; leftovers are removed.

## 0.1.4 - 2026-10-16

Updates now replace the whole application, so the icon and version update too.

## 0.1.3 - 2026-10-16

The web IDE is now called WebWave.

## 0.1.2 - 2026-10-16

IDEs are installed as applications in the Applications folder; launching activates a running IDE instead of starting a second copy.

## 0.1.1 - 2026-10-16

Russian and English interface with a language picker; the window opens under the menu bar icon; releases come from ide.ifzenelse.space.

## 0.1.0 - 2026-10-16

The first release of Zen Toolbox: the tray widget, release checks, installs, updates, rollback and self-update.
