# ADR-0145: JvmJizo, the Java-first IDE for the JVM

- **Status:** Proposed (the name and the Java-first scope: the owner, 2026-10-25)
- **Date:** 2026-10-25
- **Changes:** the JVM IDE's name in [ADR-0005](0005-naming.md) and docs/naming.md, and its prop in
  [ADR-0125](0125-kozo-the-family-mascot.md)

## Context

The family's JVM IDE was named after Kotlin. The owner decided (2026-10-25) that the JVM IDE is
**Java-first**: Java is what most JVM projects are written in, Kotlin, Groovy and Scala come with it.
"Java" may not appear in a product name (Oracle's trademark policy, docs/naming.md), so the name
cannot say Java. Nothing was released under the earlier name, and it is removed everywhere.

## Decision

1. The JVM IDE is **JvmJizo**, "a Zen IDE for Java and the JVM", product id `jvmjizo`. "Jvm" names
   the platform; **Jizō** (地蔵) is the stone guardian found by Japanese roadsides in a red bib,
   steadfast and patient, as Java's backward compatibility is; J-J alliteration as the family's
   names have (ADR-0019).
2. Its tile and robe are a lichen-stone green (`#B8C4A6`, `#5E6E58`), distinct from the other
   products'. Its tile (`assets/icons/products/jvmjizo.svg`, our own drawing) is a Jizō's friendly
   face, round, eyes closed in a smile, over a red bib: the owner chose it from four drawings on
   2026-10-25. It is the family's first tile with a second colour beside the black glyph (the bib's
   red, `#D63A3A`); the tiles are SVGs rendered as they are (egui's SVG loader in the IDEs and Zen
   Toolbox, the bundle's icon, the website's `/icons/`), so nothing recolours it.
3. Its Kozō (`zen_mascot::Variant::JvmJizo`) sits beside a small stone Jizō with a red bib, our own
   pixel art in the place and scale of the other props; in the idle and working animations a red
   maple leaf drifts down to the statue, eight frames that loop, still under reduced motion.
4. Zen Toolbox's catalog lists it as "Java and the JVM" ("Java и JVM"). The earlier name had no
   launcher, so Zen Toolbox's list of old launcher names to remove does not have one for it either.

## Consequences

- No trace of the earlier name, its tile or its prop remains; ADR-0005 (Accepted) was edited on the
  owner's explicit word (2026-10-25).
- JvmJizo is still planned; its plug-ins (Java first) come with it (ADR-0050).
