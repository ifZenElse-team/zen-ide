# ADR-0005: Naming

- **Status:** Accepted; product names amended by [ADR-0019](0019-alliterative-product-names.md)
- **Date:** 2026-10-16

## Context

The project needs names for the team, the family, the platform and every product. Names must avoid
existing trademarks (including the naming patterns of other vendors' products), be free on crates.io
and code hosts, and make the structure obvious.

## Decision

- Team: **ifzenelse**. Family: **Zen IDE**. Platform: **Zen Platform**, with its engine **Zen Core**.
  Installer: **Zen Toolbox**. Infrastructure keeps plain names.
- Each IDE is named as the language name followed by an image from Japanese culture:
  RustRonin, GoGarden, PyLantern, JvmJizo, TsTorii, CppCrane, SharpKatana, PhpPagoda,
  RubyRyokan, SqlScroll.
- Crates are named `zen-<area>`; language crates `zen-lang-<language>`.

The full list, meanings, identifier rules and rejected names are in `docs/naming.md`, which is the
living reference; this record captures the decision.

## Consequences

- The Japanese theme ties every product to the team name and gives the family a recognisable voice.
- Language names are third-party trademarks; each owner's policy is reviewed before a product's
  first public release. "Java" is never used in a product name.

## Alternatives considered

A shared suffix (`-Zen`, `-Forge`, `-Bench`), pure Japanese product names without the language, and
branded platform names (Zazen, Ishizue). See `docs/naming.md`, section 4.
