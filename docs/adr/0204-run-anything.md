# ADR-0204: Run Anything and double modifier shortcuts

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

Run Anything opens with two presses of Control: one field that runs a run
configuration, a build tool's command (`go test ./...`, `mvn clean install`) or any command line,
with the recent commands first. Our run configurations come from run providers (ADR-0048) of the
language modules and the platform (Go, Cargo, Maven, Gradle, npm, Java); the commands a person can
type belong to those providers, not to the popup. The keymap knew only keystrokes with a key: Search
Everywhere's ⇧⇧ was a fixed gesture, so a double modifier could not be bound or changed in Keymap.

## Decision

1. **Run providers contribute what can be typed.** `RunProvider` has two methods with empty
   defaults: `anything_prefixes` names the first words a provider takes (`go`, `cargo`, `mvn`,
   `gradle`), what they do and the commands it suggests; `anything` turns a typed command line into
   a temporary configuration that runs in a given folder: a configuration of the provider's own kind
   (Maven goals, Gradle tasks, which can then be debugged where the provider debugs them), or a plain
   command line (`zen_run::external_tool::command_configuration`, the external tools' kind, with the
   environment the provider needs). A plug-in or module adds commands the same way; the popup knows
   no tool.
2. **Resolution order.** `run.anything.run` runs a configuration whose name is typed (or one by id),
   else the first provider's command that takes the line, else the line in a new Terminal tab in
   the chosen folder. The folder is the project's, the current file's module or its folder.
3. **History.** The last 50 lines run are kept per project window, newest first, in the window's
   workspace (`run.anything` of the project's layout), never in the project.
4. **Double modifiers are keystrokes.** A keystroke may be one modifier pressed twice (`⌃⌃`, `⇧⇧`,
   `⌥⌥`, `⌘⌘`), written so in keymap files. Run Anything is `view.runAnything.open` on `⌃⌃` in Keymap
   → Other, so it can be removed or given another shortcut. A double press is detected as ⇧⇧ is:
   the modifier alone, pressed twice within 0.4 s, nothing typed between. On every platform the
   physical Control key gives `⌃⌃`. Search Everywhere keeps its ⇧⇧ gesture.
5. **One switch.** Advanced Settings → User Interface → Disable double modifier key shortcuts
   (`ui.doubleModifierShortcuts.disabled`) turns off ⇧⇧ and every double modifier binding.

## Consequences

- Every IDE gets Run Anything for whatever run modules it bundles; a new tool's commands need no UI
  change.
- `open <name>` (recent projects) is the window's: the project window's session does not know the
  other projects. Agents open projects through the app's own tools.
- Recording a double modifier in Keymap's shortcut dialog is not offered yet: such a binding is
  written as text in a keymap file or kept from the default.
