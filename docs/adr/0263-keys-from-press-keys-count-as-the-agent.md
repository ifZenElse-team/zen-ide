# ADR-0263: Keys from press_keys count as the agent

- **Status:** Proposed
- **Date:** 2026-11-07
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0248](0248-safe-mode.md)

## Context

Commands refuse an agent by asking `zen_core::macros::by_agent`. `run_action` and `dispatch_command`
run under that mark, but `press_keys` only queues keys for the window, and what they reach ran as the
person. An agent could bind `settings.ide.restoreDefaults` to a key with `set_keymap`, press it, and
the settings were gone; a test showed that before this change. Dialogs were refused one list at a time.

## Decision

1. **A frame that handles an agent's keys is the agent's.** From the frame the keys are queued in until
   they are answered, and one frame more for a widget that acts on the frame after its key, the view
   runs with the session marked as the agent. Every command a shortcut, button or dialog dispatches then
   sees `by_agent`, which is the one rule: there is no list of dialogs to keep up.
2. **`Request::Settings` from such a frame passes what `set_settings` refuses.** The Settings dialog
   changes settings through a request, not a command, so the same check runs on the request and a
   refusal tells the person.
3. **The per-dialog refusals stay.** The restart prompt, Invalidate Caches, Import Settings, Restore
   Default Settings and Trust and Open Project answer `ide-agent-person-answers` as before: they act
   through `Request::Restart` or at the app level, without a command to refuse.

## What the rule cannot reach

- **Requests other than settings** (`Request::Open`, `Close`, `Restart` and so on) carry no session, so
  they are not marked. Opening a project still asks the trust question, which refuses keys; a dialog
  that ends in `Request::Restart` has to stay on the refusal list.
- **Work that outlives the frames:** a background job or a widget that acts two or more frames after
  its key runs unmarked, unless it keeps the mark itself, as the wizard and Clone do with `by_agent`.
- **The `*` key of the keypad** goes through the system's own event and arrives outside these frames.
- **A person's key in the same frames counts as the agent's.** It fails closed, for a frame or two.
- **The welcome screen has no key route for agents**, so there is nothing to mark there.

## Consequences

- A new dialog that dispatches a command needs nothing to be safe from an agent's keys.
- A new dialog that acts directly must join the refusal list, or use a command.
- A macro recording started in the window no longer takes an agent's keys: they count as the agent's, like
  its commands, and a recording an agent started takes them.

## Alternatives considered

- **Only the per-dialog lists:** each new dialog is a new way past a refusal.
- **Refusing `press_keys` in the Settings dialog:** agents may use the rest of it; `set_settings` already
  says what they may not change.
