# ADR-0226: Several generators per server action, and Override/Implement Methods

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0216](0216-generate-and-the-member-chooser.md)

## Context

ADR-0216 lets a server declaration turn a prompt-based source action into a generator with a
member chooser, one prompt per command. Wiring the Eclipse JDT Language Server's prompts showed
three gaps, observed by talking to the server (1.63, with the extended client capabilities
`generateToStringPromptSupport`, `hashCodeEqualsPromptSupport`,
`generateConstructorsPromptSupport`, `generateDelegateMethodsPromptSupport`,
`advancedGenerateAccessorsSupport` and `overrideMethodsPromptSupport` from its public wiki):

- One command serves several entries that the IDE lists apart: the server's three accessor
  actions (getters, setters, both) run the same command and differ only in a `kind` of its argument;
  its one "Override/Implement Methods" action is two entries in the IDE, Override Methods
  (⌃O) and Implement Methods (⌃I), each with its own shortcut and Code menu item.
- The entries' names are the server's ("Generate Getters..."), not the IDE's ("Getter"),
  and come in the server's order.
- Implement Methods lists only the abstract methods not implemented yet, Override Methods the
  others: the chooser needs to keep part of a list.

## Decision

1. **A prompt is picked by its command and `match`**: pointers into the command's first argument and
   the values they must hold. Every prompt that fits becomes an entry of its own, so one action may
   give several (Override and Implement). A prompt's `name` (English and Russian) is the entry's
   title, the action's title when absent; the server's entries with prompts come in the order the
   prompts are declared, plain source actions after them.
2. **A member list's `when`** keeps the items where a pointer is set. A condition that starts with
   `!` means the pointer is not set, in `when`, a list's `selected` and an option's `when` alike.
   The items of a first `*` have the whole answer as their parent, so a group can name the type the
   answer is about (`{parent/type}`).
3. **Override Methods… and Implement Methods… are platform actions**, `view.overrideMethods.open`
   (⌃O) and `view.implementMethods.open` (⌃I) in the editor, in the Code menu before Generate…
   (`OverrideMethods` and `ImplementMethods` in an imported JetBrains keymap). Each opens the member
   chooser of
   the generator whose prompt declares `action: "override"` or `"implement"` without the Generate
   popup, and a notice when there is no such generator or nothing to choose. `editor.generate.list`
   tells agents a generator's `action`.

## Consequences

- The Java plug-in wires every JDT LS generator in its `plugin.json` alone: Constructor, Getter,
  Setter, Getter and Setter, `equals()` and `hashCode()`, `toString()`, Override Methods, Implement
  Methods and Delegate Methods. The server takes its code style (templates, comments, `Objects`
  helpers) from its `java.codeGeneration` settings, not from the request, so the only per-request
  option is replacing an existing `equals()`/`hashCode()`.
- Any language's server can give Override/Implement Methods by declaring the `action`; without one,
  ⌃O and ⌃I say there is nothing to override or implement.
- JDT LS offers no test method generator, so Java has none yet.
