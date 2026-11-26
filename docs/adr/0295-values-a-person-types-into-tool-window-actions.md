# ADR-0295: Values a person types into plug-in tool window actions

- **Status:** Accepted (the owner, 2026-11-16)
- **Date:** 2026-11-16
- **Builds on:** [ADR-0289](0289-plugin-tool-windows.md) (plug-in tool windows),
  [ADR-0290](0290-tool-window-choices-and-confirmations.md) (choices and confirmations),
  [ADR-0292](0292-plugin-tool-window-icons-and-sides.md) (icons and sides)

## Context

A plug-in tool window's arguments come only from rows the platform listed and from the choices in
its header (ADR-0289, ADR-0290). Some actions need a value the person picks: the local port of
`kubectl port-forward`, the number of replicas of `kubectl scale`. These actions were left out of
the Kubernetes window until values could be typed without letting anyone's text reach a program's
arguments.

## Decision

1. **Typed inputs.** An action may declare `"inputs"`: `[{"id", "title": {en, ru}, "kind",
   "default"?, "defaultFrom"?}]`. The kinds are the platform's, with bounds a plug-in cannot widen:
   - `count`: a whole number from 0 to 1000;
   - `port`: a whole number from 1024 to 65535;
   - `forward`: `LOCAL:REMOTE`, a local port from 1024 to 65535 and a remote port from 1 to 65535
     (a container's port 80 is normal), typed as two numbers and joined by the platform.

   A value is plain decimal digits: no sign, no spaces, no leading zeros, no exponent. `default` is
   where a field starts; `defaultFrom` names a row column whose first number starts the field when
   it is in range (a service's `80/TCP` fills the remote port), else `default`, else nothing.
2. **Whole arguments only.** An argument that is exactly `${input.<id>}` becomes the checked value,
   as one argument; `${input.` anywhere else in an argument is refused, as `${row.` and `${window.`
   are, and so is an input the action does not declare. Every value is checked against its kind's
   bounds again when the action runs, whoever sent it, before anything starts.
3. **Only a person types values.** `plugins.toolWindow.act` takes `inputs` from a person (the
   window's dialog). An agent that passes `inputs` is refused (`agentNotAllowed`). An agent that
   asks for such an action (its run access is not `none`) gets `confirmationNeeded` with a token; a
   notification names the action, the rows and the window's choices (the context and namespace) and
   offers **Enter Values…** (`plugins.toolWindow.enterValues`, the person's alone), which opens the
   window's dialog. The person's OK sends `plugins.toolWindow.allow { token, inputs }` (the person's
   alone), and the agent's same call with that confirmation runs once with the person's values. An
   agent never chooses a value, before or after.
4. **Questions name the values.** A `confirm` question (ADR-0290) may hold `${input.<id>}`; the
   window's dialog shows it with the typed values in place, and a person's action that confirms is
   asked again with them before it runs. `kubectl scale` asks: "Scale ${rows} in the context
   ${window.context}, namespace ${window.namespace}, to ${input.replicas} replicas?".
5. **A forward binds to loopback.** A plug-in whose action takes a `forward` input must make the
   forward listen on 127.0.0.1 only and say so in its guide; the platform knows no program's flags
   and does not check them, the plug-in's tests do. A forward is a `stream` action: a Run tab, and
   Stop ends it.
6. `plugins.toolWindow.list` answers each action's `inputs` (id, title, kind, bounds, `default`,
   `defaultFrom`), so an agent knows what the person will be asked.

## Consequences

- The Kubernetes window gets **Port Forward…** on pods and services
  (`kubectl port-forward --address 127.0.0.1 <pod|service> LOCAL:REMOTE`) and **Scale…** on
  deployments.
- Other plug-ins can ask for a count or a port without new platform code; a new kind of value
  (a duration, a name) needs a new decision, with its own bounds and check.

## Alternatives

- **Free text inputs:** any text would reach a program's arguments; rejected.
- **`${input.a}:${input.b}` inside one argument:** breaks the rule that a value is a whole
  argument; the `forward` kind keeps the rule and stays digits and one colon.
- **Agents pass values the person then confirms:** an agent would choose the value; rejected.
