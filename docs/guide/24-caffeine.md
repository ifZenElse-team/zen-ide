# Caffeine

Caffeine keeps your computer awake from inside the IDE, for long builds, test runs and agent
sessions that must not stop when the computer would go to sleep. It is a tool window on the right
stripe, right under the Git button: a little pixel-art coffee drinker shows what it does, and every
control is under him.

## The stripe button

The button is a tiny pixel coffee cup that shows the state at a glance:

- a cold cup with a **z**: Caffeine is off;
- a cold cup: Caffeine is on but paused (on battery, or nothing it watches runs);
- a steaming cup: Caffeine keeps the computer awake;
- a cup steaming **red**: every "keep awake" option is on.

Click it to open the Caffeine tool window, click again to hide it. Like every tool window it can be
dragged to another place on the stripes, and **View → Tool Windows → Caffeine** opens it too.

## The coffee drinker

- **Off**: he sleeps at the table, zZ.
- **Paused**: he is awake and waits, the cup down, looking around.
- **The system kept awake** (`-i`): he sips his steaming coffee steadily.
- **The display kept awake too** (`-d`): his eyes are wide open and he sips more often.
- **Every option on** (`-i -d -m -s -u`): bloodshot red eyes, hair on end, jittery, gulping coffee
  very often.
- **For a time**: the time left shows above his state ("42 min left", "1 h 12 min left").

Under him are two short lines: what is kept awake and for how long ("Keeping awake: everything ·
until you turn it off"), and why Caffeine pauses or what runs now. Hover over them for the whole
story, the lid rule included. Each line keeps its height whatever the state, the time left or the
language, and a line too long for the window ends with "…", so the controls below never move.

He is drawn as pixel art of our own in the theme's colours (only the red of his eyes is fixed),
scaled up in whole pixels so that they stay crisp. He moves only while the tool window shows and
the IDE's window is visible; with macOS's **Reduce motion** (System Settings → Accessibility →
Display) he holds a still pose (as Kozō on the welcome screen does), and while the tool window is hidden he costs no frames at all.

## Turning it on and off

- **Turn On** turns Caffeine on with the defaults of Settings → Tools → Caffeine (until turned off,
  unless you chose otherwise); **Turn Off** turns it off.
- **Maximum Caffeine**, next to it, with the red-steaming cup: one click keeps everything awake
  (every option of "What to keep awake") until you turn it off, at once. The button stays pressed
  while Caffeine is on at maximum; a second click, or Turn Off, ends it. Your settings are left as
  they were, and the battery rules still apply.
- **Keep awake for**: until turned off, 15 min, 30 min, 1 h, 2 h, 4 h, **While Something Runs**, or
  a custom number of minutes with **Start**. Choosing a time while Caffeine is on starts the time
  from now, and Caffeine ends on time even while the IDE sits idle in the background. The time
  counts while the computer is awake: while it sleeps (a closed lid, Apple menu → Sleep), the time
  left stands still, as it does for `caffeinate -t`.

Caffeine runs macOS's own `/usr/bin/caffeinate` (on Linux `systemd-inhibit`) as a child of the IDE
that waits for the IDE's process: when the IDE ends, even by a crash, nothing is left awake. On
other systems the tool window says Caffeine is not available.

## What to keep awake

Each can be switched on its own, in the tool window or in the settings:

| Choice | `caffeinate` flag |
|---|---|
| The system (idle sleep), the default | `-i` |
| The display | `-d` |
| The disk | `-m` |
| The system, on mains power only | `-s` |
| "User is active" (wakes the display) | `-u` |

Closing the lid of a laptop still puts the display to sleep: that is the system's rule, and no
program can change it.

## While something runs

In **While Something Runs** Caffeine keeps the computer awake only while one of these runs, each a
check box under **While** in the tool window and in the settings:

- a run or a test;
- a debug session;
- a background task (`go get`, `cargo`, a Git push or pull);
- indexing (a language server loading or indexing the project);
- an agent connected over MCP, to this window or to a headless IDE (an agent counts as connected
  for two minutes after its last call, or while its `mcp` bridge is attached).

When nothing of that runs, Caffeine pauses and the coffee drinker waits with his cup down.

## Battery rules

- **Pause on battery power**.
- **Pause below** a battery level, 20% by default.
- **Go on again when charging** (on by default): when it is off, a battery rule turns Caffeine off
  instead of pausing it.

The battery is read from `pmset -g batt` once a minute, and only while a rule needs it.

## Settings → Tools → Caffeine

The page holds every parameter: what to keep awake, for how long Turn On (and the start with the
IDE) turns Caffeine on, the custom duration, what "while something runs" watches, the battery rules
and **Turn Caffeine on when the IDE starts**. They are saved with the IDE's settings and remembered
across restarts; the tool window changes them too, and its **Caffeine Settings…** link opens the
page.

## For agents

Agents drive Caffeine over MCP with `dispatch_command`:

- `caffeine.status.get`: whether it is on and keeps the computer awake now, what, for how long
  (`secondsLeft`) or while what, what runs now (`busy`), why it pauses (`onBattery`, `lowBattery`,
  `waiting`), the battery and the pid of `caffeinate`.
- `caffeine.awake.start` `{what?: [idle|display|disk|system|user], minutes?, while?: [run|debug|task|indexing|agent], maximum?}`:
  what is left out comes from the settings; `minutes: 0` is until stopped.
  `maximum: true` is every option until stopped.
- `caffeine.awake.maximum`: the Maximum Caffeine button (on at maximum, or off when it already is);
  the status says `maximum`.
- `caffeine.awake.stop`, `caffeine.awake.toggle`.

The settings are `caffeine` in `get_settings` and `set_settings`. The window action
`view.toolWindow.caffeine` shows or hides the tool window, and `get_window_state` gives the
drinker's state as `caffeine` (`asleep`, `waiting`, `steady`, `wide` or `wired`).
