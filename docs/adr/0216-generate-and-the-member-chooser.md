# ADR-0216: Generate and the member chooser

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0071](0071-built-in-modules.md),
  [ADR-0198](0198-library-classes-and-documents-of-language-servers.md)

## Context

Code → Generate… (⌘N in the editor) lists what can be written for the code at
the caret (a constructor, getters and setters, `equals()`/`hashCode()`, `toString()`, overriding or
delegating methods, a test) and, for most entries, opens a member chooser: a tree of fields or
methods with check boxes, select all, sorting and the generator's own options. Every language
needs the popup and the chooser; what can be generated is the language's. Some generators live in
the IDE's language module (Go has no server action for a constructor); others are the language
server's: plain source actions (gopls' Add test), and actions whose command the client must run
itself, asking the server for the members and sending the choice back (the Eclipse JDT Language
Server's prompt-based source actions, turned on with extended client capabilities).

## Decision

1. **One popup and one chooser in the platform.** `editor.generate.list`, `editor.generate.members`
   and `editor.generate.apply` are the commands; `view.generate.open` (⌘N in the editor context,
   New… stays ⌘N elsewhere) opens the popup over them, Code → Generate… after Code Completion.
   The chooser groups members by the type they belong to, works from the keyboard alone (arrows,
   Space, ⌘A, Tab between Select All, Sort Alphabetically and the options, Enter, Escape), and
   the code is written as one undo step.
2. **Two sources of generators, language's own first.**
   - A built-in module contributes a `zen_modules::generate::CodeGenerator`
     (`ModuleHost::code_generator`): it offers generators at a place, gives a `Chooser` (members,
     options, whether none may be chosen) and returns the text to insert, formatted with the file's
     indentation. Go's Constructor is the first.
   - A language server's code actions of kind `source.*`, asked with `only: ["source"]`, without
     organizing imports and fixing all (they have their own actions). A server declaration may
     narrow the kinds with `generate.kinds` (gopls: `source.addTest`; its other source actions
     browse documentation or assembly and are not generators).
3. **Prompt-based server actions are declared, not coded.** A declaration's `generate.prompts`
   maps an action's client-side command to a request that answers the members, JSON pointers
   (with `*` for each element, and the element one level up as the parent) that turn its answer into
   the chooser, localized options with a pointer that must be set for them to show, and a request
   whose params are a JSON template (`${context}`, `${members:<list>}`, `${option:<id>}`,
   `${item}`, `${parent:/pointer}`) answering a workspace edit. The Java plug-in can then wire the
   JDT LS prompts (toString, hashCode/equals, constructors, delegate methods, accessors, override
   methods; their capability flags and `java/*` requests to be taken from the JDT LS
   documentation) in its `plugin.json` alone, with no platform change.

## Consequences

- A new language gets Generate by declaring kinds or prompts for its server, or by a module's
  generator; the popup, the chooser, undo and the agents' commands come with it.
- A server action that creates a file (gopls' Add test before the `_test.go` file exists) is
  refused like any other until workspace edits with resource operations are supported.
- The chooser's member ids are positions in the server's answer (`<list>.<index>`), valid until
  the next listing.
