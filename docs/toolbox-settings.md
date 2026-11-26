# Zen Toolbox settings

> **Last updated:** 2026-10-19 · **Decisions:** [ADR-0043](adr/0043-toolbox-settings-and-shell-launchers.md),
> [ADR-0069](adr/0069-toolbox-launch-at-login.md)

Zen Toolbox keeps its settings in one JSON file. This document lists the file's location, every
key, and how the command line overrides them. The typed view of the settings is
`zen_toolbox::ToolboxSettings` (`crates/platform/toolbox/src/settings.rs`); the commands
`toolbox.settings.get` and `toolbox.settings.set` read and change them, for the UI and for AI
agents alike ([ADR-0007](adr/0007-everything-is-a-command.md), [ADR-0033](adr/0033-ai-access-to-every-api.md)).

## The file

`settings.json` lies next to the default tools directory, in the user's application data:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/space.ifzenelse.toolbox/settings.json` |
| Windows | `%LOCALAPPDATA%\Zen Toolbox\settings.json` |
| Linux and others | `$XDG_DATA_HOME/zen-toolbox/settings.json`, or `~/.local/share/zen-toolbox/settings.json` |

`ZEN_DATA_DIR` replaces the application data folder on every OS, as for the IDEs, for tests and UI
snapshots: the file is then `$ZEN_DATA_DIR/space.ifzenelse.toolbox/settings.json`, next to `tools`.

The file is a flat JSON object of dotted keys, as `zen_core::Settings` stores them, for example:

```json
{
  "toolbox.channel": "nightly",
  "toolbox.updates.check": "manual",
  "ui.language": "ru"
}
```

A key that is missing has its default. A value of the wrong type, an unknown choice or a relative
path is ignored when the file is read, and the default applies. Zen Toolbox writes the file when a
setting changes; the location of the file itself does not depend on any setting, including
`toolbox.tools.location`.

## Keys

| Key | Type | Default | Effect |
|---|---|---|---|
| `toolbox.tools.location` | absolute path | the tools directory next to `settings.json` (`…/tools`, see `Installation::default_root`) | Where new tools are installed. Installed tools are not moved: they stay, update and uninstall where they are. Applied at once. |
| `toolbox.tools.previousLocations` | array of absolute paths | `[]` | Kept by Zen Toolbox, not set by people: earlier tools locations, newest first, that are still searched for installed tools. Changing `toolbox.tools.location` adds the old location; choosing a remembered location again takes it off the list. Locations are not dropped when they become empty. |
| `toolbox.channel` | `stable` or `nightly` | `stable` | The release channel for every tool and for Zen Toolbox itself. A change recomputes the available versions at once. |
| `toolbox.updates.check` | `automatic` or `manual` | `automatic` | `automatic` checks the release server at start, when the window opens (if the last check is older than a minute) and every 30 minutes. `manual` makes no network request until the user chooses **Check for Updates** (`toolbox.releases.refresh`). |
| `toolbox.updates.tools` | `notify` or `automatic` | `notify` | `automatic` updates every installed tool by itself, as `toolbox.updates.automatic` does for the tools it lists. |
| `toolbox.updates.automatic` | array of tool ids | `[]` | The tools that update by themselves: after a check that finds a newer version, in the background, only while the tool is not running, with a system notification afterwards ([ADR-0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md)). `toolbox.tool.setAutoUpdate` changes it. |
| `toolbox.updates.held` | object: tool id to version | `{}` | Kept by Zen Toolbox: installing an older version records the newest one then, and automatic updates of that tool wait for a newer release. |
| `toolbox.notifications.enabled` | `true` or `false` | `true` | A check that finds a new version of an installed tool or of Zen Toolbox shows a system notification, once per version; `toolbox.notifications.told` remembers the last one announced for each ([ADR-0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md)). |
| `toolbox.updates.self` | `notify` or `automatic` | `notify` | `notify` shows a newer Zen Toolbox and waits. `automatic` downloads and installs it in the background after a check finds it; the restart is still the user's ("Restart"). |
| `toolbox.shellScripts.enabled` | boolean | `true` ([ADR-0086](adr/0086-ide-commands-on-path.md)) | Whether Zen Toolbox keeps shell launchers (`gogarden`, `rustronin`, ..., and `zen`) for the installed tools. Turning it on generates them at once; turning it off removes the launchers Zen Toolbox wrote. Details in [toolbox-launchers.md](toolbox-launchers.md). |
| `toolbox.shellScripts.location` | absolute path | `~/.local/bin` | Where the launchers are kept, and the default location of the `toolbox.shellScripts.*` commands. A change moves the launchers Zen Toolbox wrote. |
| `toolbox.launchAtLogin` | boolean | `true` | Whether Zen Toolbox starts in the tray, without its window, when the user logs in ([ADR-0069](adr/0069-toolbox-launch-at-login.md)). An installed Toolbox applies it at every start and at once on a change: on writes its LaunchAgent (`~/Library/LaunchAgents/space.ifzenelse.toolbox.plist`, starting the executable with `--tray`) or, on Linux, its autostart entry (`~/.config/autostart/space.ifzenelse.toolbox.desktop`); off deletes it. Only a file carrying Toolbox's label is touched. Quitting Toolbox leaves it as set. Development builds register nothing unless `ZEN_LAUNCH_AGENTS_DIR` names a folder for it (tests); Windows has no login item yet. |
| `ui.theme` | `system`, `dark` or `light` | `system` | Stored only for now; applying the theme at run time comes with the light theme. |
| `ui.language` | `system` or a language code (`en`, `ru`) | `system` | The interface language ([ADR-0024](adr/0024-localisation.md)); `system` follows the operating system. |
| `toolbox.whatsNewShown` | version | none | Kept by Zen Toolbox: the last version whose own "What's new" was shown. |

## Commands

- `toolbox.settings.get` takes no parameters and returns every setting above (except the two kept
  by Toolbox) with defaults filled in, in camelCase: `toolsLocation`, `channel`, `updatesCheck`,
  `updatesTools`, `updatesSelf`, `shellScriptsEnabled`, `shellScriptsLocation`, `notifications`, `launchAtLogin`,
  `theme`, `language`.
- `toolbox.settings.set` takes any subset of the same fields. Every value is validated first; an
  unknown choice, a relative path or an unknown field fails the command with a localised message,
  and nothing is changed. Valid values are written to the settings, which the application saves,
  and applied to the running Toolbox. It returns every setting, as `toolbox.settings.get` does.
- `toolbox.login.status` takes no parameters and reports launch at login: `enabled` (the setting),
  `supported` (whether this build keeps a login item), `registered` (whether it is in place) and
  `file` (its path).

## The command line

The command line wins for the run it starts, and is never saved:

- `--channel stable|nightly` overrides `toolbox.channel`.
- `--tools-dir <dir>` overrides `toolbox.tools.location`, and it is then the only directory searched
  for installed tools (`toolbox.tools.previousLocations` is ignored). Such a run also leaves the
  shell launchers alone: `toolbox.shellScripts.enabled` does not generate or remove them.
- `--releases none` turns checking off whatever `toolbox.updates.check` says.
- `--tray` starts Toolbox in the tray without opening its window; the login item uses it.

`toolbox.settings.set` still saves a setting that the command line overrides; it takes effect at the
next start without the flag. The merge is `zen_toolbox::settings::for_this_run`.

## Decisions

The decisions are recorded in [ADR-0043](adr/0043-toolbox-settings-and-shell-launchers.md):

- The settings stay flat dotted keys in the existing `settings.json`: no new file and no format
  change.
- One channel for every tool and for Zen Toolbox, not one per tool.
- `toolbox.tools.location` applies to new installations only; installed tools are not moved, and
  their directories are remembered in `toolbox.tools.previousLocations`.
- `manual` checks mean no network access until the user asks (AGENTS.md, rule 10); the window
  then says that releases are not checked yet and offers **Check**.
- Shell launchers are removed when the setting is turned off, because removal only ever touches
  files that carry the Zen Toolbox marker ([toolbox-launchers.md](toolbox-launchers.md)).
- `automatic` tool updates are accepted and stored but behave as `notify` until Toolbox can tell
  whether a tool is running; `automatic` self-updates install in the background and leave the
  restart to the user.
