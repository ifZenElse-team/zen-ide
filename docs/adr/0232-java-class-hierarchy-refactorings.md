# ADR-0232: Java class hierarchy refactorings, from JDT LS where it has them

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

After Move and Change Signature (ADR-0229), JvmJizo's Refactor menu still lacked the refactorings
that reshape classes: Pull Members Up, Push Members Down, Extract Interface, Extract Superclass,
Encapsulate Fields and Introduce Parameter Object. Each should open in the shared refactoring
dialog (ADR-0222) with its preview and conflicts.

Checked against the Eclipse JDT LS that JvmJizo installs (1.63.0), by asking the server for code
actions on a class, a field, a method and a parameter, and by sending it the requests below:

- **Extract Interface** is the server's: `java/checkExtractInterfaceStatus` with code action
  parameters on the class answers `{ subTypeName, members: [{ name, typeName, parameters,
  handleIdentifier }], destinationResponse: { destinations } }` (public methods only), and
  `java/getRefactorEdit` with `command: "extractInterface"`, the code action parameters in
  `context` and `commandArguments: [handles, name, destination]` answers the edit: the
  `implements` clause, `@Override` on the methods and the interface's file created. A name taken
  or other argument shapes answer nothing; an invalid name answers an error. Every method gets a
  `/** (non-Javadoc) @see … */` comment that no setting we found removes.
- **Encapsulate Fields:** the server only generates accessors (`java/resolveUnimplementedAccessors`,
  `java/generateAccessors`), with names it chooses and nothing done to the field's uses.
- **Pull Members Up, Push Members Down, Extract Superclass, Introduce Parameter Object:** the
  server offers nothing. It answers `textDocument/prepareTypeHierarchy`,
  `typeHierarchy/supertypes` and `typeHierarchy/subtypes` (the JDK's types as `jdt://` URIs),
  `textDocument/references` (a method's reference covers the whole call, its arguments included)
  and `textDocument/implementation`.

## Decision

1. **Six providers beside Move and Change Signature** in the workbench's Java code
   (`java/refactoring/hierarchy.rs`, `java/refactoring/fields.rs`): `javaPullUp`, `javaPushDown`,
   `javaExtractSuperclass`, `javaExtractInterface`, `javaEncapsulateFields` and
   `javaIntroduceParameterObject`, answering the window actions `pullMembersUp`,
   `pushMembersDown`, `extractSuperclass`, `extractInterface`, `encapsulateFields` and
   `introduceParameterObject`.
2. **Extract Interface goes through the server** as above; the generated `(non-Javadoc)` comments
   are dropped from its edit, and a name whose file exists is refused before the server is asked.
3. **The others plan their edits themselves** from a member scanner over the Java tokenizer
   (`java/members.rs`: classes with their `extends` and `implements`, members with modifiers,
   parameters and bodies) and the server's answers for what needs the whole project: the type
   hierarchy for Pull Up's destinations and Push Down's subclasses, references for Push Down's
   conflicts, the uses Encapsulate Fields rewrites and the calls Introduce Parameter Object
   rewrites, and implementations for the overriding methods it refuses. The plan is a workspace
   edit that the same path as the server's edits applies, files created included.
4. **When unsure, refuse or list a conflict, never guess.** Pull Up lists a moved member's use of
   one that stays, a member the destination already has, and a superclass that becomes abstract;
   a private member that stays in use becomes protected. Push Down lists uses outside the
   subclasses and of private members. Encapsulate Fields rewrites reads, assignments, compound
   assignments and increments that stand as statements, and lists every other write, a static
   import and a qualifier that would run twice. Introduce Parameter Object refuses an overriding or
   overridden method, an assigned or shadowed parameter, a recursive method and a variable-arity
   parameter, and lists method references and calls with another number of arguments.
5. **Accessor names are the dialog's**, so Encapsulate Fields writes the getters and setters itself
   instead of `java/generateAccessors`, which takes no names.
6. **The dialog's tables gain column kinds and fixed rows**: a column is `text` (editable),
   `checkbox` (a boolean; a row without one shows no check box) or `label`; a `fixed` table has no
   Add, Remove or Move buttons. The member lists use them.

## Consequences

- Each refactoring is one undo step and one Local History action.
- The scanner is not a compiler: types are compared by their simple names, a moved member's
  imports are copied from its file, and code that moves between packages may need imports the
  compiler then reports.
- Not done: Pull Up's and Push Down's Javadoc options, pulling into a class outside the project,
  Extract Superclass into another package, Use
  Interface Where Possible, Encapsulate Fields' option to use accessors inside the class, and
  Introduce Parameter Object into an existing class or with a builder.
