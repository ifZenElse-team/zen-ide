# ADR-0011: Plug-ins and JetBrains compatibility

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

JetBrains Marketplace has thousands of plug-ins, many of them free. They are JVM bytecode written
against the IntelliJ Platform API (tens of thousands of classes) and usually draw their UI with
Swing. Running them would require embedding a JVM and reimplementing the IntelliJ Platform API,
which contradicts ADR-0002 and ADR-0006.

## Decision

1. **JetBrains plug-ins are not supported.**
2. **JetBrains data formats are supported** through `zen-jb-import`: colour schemes (`.icls`), UI
   themes (`.theme.json`), keymaps, live templates, file templates, code style and inspection
   profiles, and the relevant parts of a project's `.idea/` directory. A user switching to a Zen IDE
   keeps their setup.
3. **Standard protocols replace many language plug-ins:** the Language Server Protocol, the Debug
   Adapter Protocol, tree-sitter grammars and TextMate grammars.
4. **Zen IDE has its own plug-in system:** WebAssembly modules running in a sandbox, with access only
   to the command registry and to explicitly granted capabilities. One module runs on every operating
   system and in the future browser client. The plug-in ABI is versioned and documented before it is
   declared stable.

## Consequences

- Users lose access to existing JetBrains plug-ins and gain a sandboxed, portable plug-in model.
- Importers need fixture files that we create ourselves; no files from JetBrains distributions are
  committed to the repository.

## Alternatives considered

- **Embedding a JVM to run JetBrains plug-ins:** a small fraction of plug-ins would work, at the cost
  of the platform's main qualities.
- **Native dynamic libraries as plug-ins:** fast, but unsafe, not portable across operating systems,
  and impossible in the browser.
