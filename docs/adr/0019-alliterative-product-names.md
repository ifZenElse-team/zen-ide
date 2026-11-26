# ADR-0019: Product names alliterate

- **Status:** Accepted; the web IDE renamed by [ADR-0027](0027-webwave.md); DevopsDaruma added by [ADR-0049](0049-devopsdaruma.md); TestTotami added by [ADR-0067](0067-testtotami.md)
- **Date:** 2026-10-16
- **Amends:** [ADR-0005](0005-naming.md) (the product names)

## Context

ADR-0005 named each IDE as the language name followed by an image from Japanese culture, with
alliteration "preferred where it comes naturally". Eight of ten names alliterated (RustRonin,
GoGarden, ...). The owner decided to make alliteration the rule, before any product is released.

## Decision

1. **Every IDE name alliterates:** the language part and the image start with the same letter, as in
   **R**ust**R**onin and **G**o**G**arden.
2. Two names change:
   - **PyLantern → PyPetal** (Python): a petal, as of a cherry blossom.
   - **SharpKatana → SharpShuriken** (C# and .NET): a shuriken, a sharp throwing star, keeping the
     pun on C *sharp*.
3. The full list is in `docs/naming.md`. Product ids and bundle identifiers follow the names:
   `pypetal`, `space.ifzenelse.pypetal`; `sharpshuriken`, `space.ifzenelse.sharpshuriken`.

## Consequences

- Nothing had been released, so the rename costs only edits in the repository.
- New products must follow the rule; `docs/naming.md` keeps a reserve of names for possible future
  languages.

## Alternatives considered

- PyPond, PyPine; SharpShrine, SharpSensei. See `docs/naming.md`, section 4.
