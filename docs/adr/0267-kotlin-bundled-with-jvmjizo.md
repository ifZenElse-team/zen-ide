# ADR-0267: JvmJizo bundles the Kotlin plug-in

- **Status:** Accepted (the owner, 2026-11-07)
- **Date:** 2026-11-06
- **Relates to:** [ADR-0147](0147-jvmjizo-app-and-jdks.md) and [ADR-0151](0151-kotlin-plugin.md), whose
  words "offered, not bundled" it supersedes for JvmJizo; [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

ADR-0147 made JvmJizo Java-first, with Kotlin offered from the catalog, and ADR-0151 section 6 says
the same: the plug-in is offered when a `.kt` or `.kts` file opens. So JvmJizo's Kotlin features never
reached anyone who had not accepted the offer.

## Decision

1. **JvmJizo bundles the Kotlin plug-in**, with `java`, `maven`, `gradle`, `ant`, `properties` and
   `spring-boot` (`PRODUCT_PLUGINS` in `xtask/src/release.rs`).
2. **The other IDEs still get Kotlin as ADR-0151 says:** offered when a `.kt` or `.kts` file opens.
   The capability stays a plug-in that works in every IDE, as ADR-0050 requires.

## Consequences

- Those who install JvmJizo get Kotlin without being asked, and the bundle is larger by one plug-in.
- ADR-0147 and ADR-0151 keep their words as accepted; read them together with this one.
