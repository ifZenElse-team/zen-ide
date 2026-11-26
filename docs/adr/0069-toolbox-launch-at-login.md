# ADR-0069: Zen Toolbox starts at login, in the tray

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0018](0018-toolbox-is-the-entry-point.md), [ADR-0043](0043-toolbox-settings-and-shell-launchers.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0002](0002-open-source-clean-room-pure-rust.md)

## Context

Zen Toolbox lives in the menu bar (ADR-0018): it checks for releases every 30 minutes and is the
way into the IDEs. After the Mac restarts it is gone until the user starts it again, so updates go
unnoticed. The owner asked (2026-10-19) that Toolbox keep running in the tray after a restart, with
a setting to turn that off.

macOS offers two ways for an application to start at login:

- **`SMAppService`** (ServiceManagement, macOS 13+): `SMAppService.mainApp.register()` adds the
  application itself to Login Items. It is an Objective-C API: from Rust it needs `unsafe` FFI
  (through `objc2`), which the workspace forbids outside listed crates. It starts the application
  bundle without arguments, so Toolbox could not tell a start at login from one by the user without
  more machinery, and registering in tests would change the real Login Items of the machine running
  them. It needs a signed bundle to behave reliably; Toolbox is not notarised yet.
- **A LaunchAgent**: a property list in `~/Library/LaunchAgents` that launchd reads at every login.
  It is a plain file: written and removed with `std::fs`, pure Rust, no `unsafe`. It names the exact
  program and its arguments, so the agent can start Toolbox with `--tray`. Tests write it into a
  temporary folder. Since macOS 13 the system lists it under Login Items ("Allow in the
  background"), where users can see and switch it off; `AssociatedBundleIdentifiers` shows it there
  under Zen Toolbox's name.

## Decision

1. **A LaunchAgent on macOS**, `~/Library/LaunchAgents/space.ifzenelse.toolbox.plist`, with
   `ProgramArguments` = the Toolbox executable and `--tray`, `RunAtLoad`, `ProcessType` Interactive
   and `LimitLoadToSessionType` Aqua (the graphical login session). It is not loaded with
   `launchctl` when written: it takes effect at the next login, and Toolbox is running anyway when
   the setting is changed. On Linux the same is an XDG autostart entry
   (`~/.config/autostart/space.ifzenelse.toolbox.desktop`). Windows has no login item yet.
2. **The setting `toolbox.launchAtLogin`**, on by default, decides. Toolbox brings the login item in
   line with it at every start (which also follows the executable when it moves) and when the
   setting changes: on writes the file, off deletes it. Only a file carrying Toolbox's label is ever
   replaced or deleted. Quitting Toolbox does not touch the setting or the file.
3. **`--tray`** starts Toolbox with its window hidden: only the menu bar icon appears, and a click
   opens the window as usual. A restart into an updated Toolbox drops `--tray`, since the user asked
   for it from the window.
4. **Installed builds only.** Development builds register nothing, unless `ZEN_LAUNCH_AGENTS_DIR`
   names a folder for the login item; tests and checks use it with a temporary folder and never touch
   the real `~/Library/LaunchAgents`.
5. **Reachable by agents** (ADR-0033): `toolbox.settings.set {"launchAtLogin": false}` changes it,
   `toolbox.settings.get` reads it, and `toolbox.login.status` reports whether the login item is in
   place and where. The switch in the settings menu dispatches `toolbox.settings.set`.

## Consequences

- After a restart Toolbox is in the menu bar, checking for updates, without opening its window.
- The code is `zen_toolbox::login`; it has no `unsafe` and no new dependency.
- If Toolbox is moved to another folder while it is not running, the login item points at the old
  path until Toolbox is started from the new one.
- Should notarised builds make `SMAppService` attractive later (for example to drop the "Allow in
  the background" entry), a new ADR can switch, keeping the setting and the command.
