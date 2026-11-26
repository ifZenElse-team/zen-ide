# ADR-0274: What a module's generator can add

- **Status:** Proposed (the decision of an earlier change, moved out of accepted ADR-0216, which it edited)
- **Date:** 2026-11-07
- **Relates to:** [ADR-0216](0216-generate-and-the-member-chooser.md), which it extends;
  [ADR-0226](0226-several-generators-per-server-action.md)

## Context

Go's Getter, Setter and Implement Methods use the platform's generators, but gopls offers no generator
for them, and a module's generator could not name its action, read in the background, add imports or
pick one member from a long list.

## Decision

1. An `Offer` may name its `action` (`implement`, ADR-0226), so ⌃I opens it.
2. `CodeGenerator::pending` says a generator is still reading what its chooser lists, off the UI
   thread; `editor.generate.members` then answers `pending` as for a language server.
3. An `Insertion` may carry other edits, before its place, for imports; everything is one undo step.
4. A `Chooser` with `search` picks one member from a long list through a search field instead of check
   boxes.

## Consequences

Go's generators work through the same chooser and commands as a language server's. ADR-0216 keeps its
words as accepted; read it together with this one.
