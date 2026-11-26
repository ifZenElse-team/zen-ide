# ADR-0228: Formatter profiles and EditorConfig sections for code style

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

ADR-0220 lets a language declare its code style options as data and keep them, with the Project
scheme, in its formatter's configuration file, a flat TOML file (`rustfmt.toml`). Java and Kotlin
need more. The Eclipse JDT Language Server formats with the Eclipse formatter, whose options come
from a formatter profile, an XML file of `<setting id="…" value="…"/>` lines, named by the server
setting `java.format.settings.url` (its public documentation); one option of the page is often
several formatter settings (the braces of methods and of constructors), and booleans are written
`insert`/`do not insert`. The server reads the profile once per location: tried on JDT LS 1.63, a
profile changed in place is not read again, while a new location is. Its import order
(`java.completion.importOrder`) is a list of strings. Kotlin LSP reads the project's
`.editorconfig` (its release notes), and the `ij_kotlin_*` properties there are the Kotlin
formatter's options.

## Decision

1. **A configuration file has a format** (`configFile.format`): `toml` (the default, as before),
   `eclipseProfile` or `editorConfig`, with the section that keeps the keys (`configFile.section`,
   as `*.{kt,kts}`). Reading takes each declared key from the profile's settings or from that
   section; writing changes only the declared keys and keeps everything else in the file. A
   profile is written with every declared setting, so that its values never depend on the
   formatter's built-in profile, and a new profile starts from a minimal one named "Zen IDE".
   EditorConfig keeps the TOML rule: a key at its default is added only when the section already
   sets it, and the section is added at the end when the file has none.
2. **An option may write several keys and map its values**: `moreConfigKeys` get the same value as
   `configKey`, and `configValues` maps the value's plain text to what the file holds (`true` →
   `insert`), both ways.
3. **A server setting may name the file's location** (`configFile.location`). With the Project
   scheme, the project's file; with Default (IDE), a file the IDE writes from its own values, only
   while one of them differs from its default, so an untouched page leaves the server as it was.
   Either way the server is given a copy in the IDE's data folder, `code-style/<language>/`, named
   after a digest of its text (`java-formatter-<digest>.xml`), and older copies are removed: a
   changed file always has a new location, which the server reads again. Without a data folder the
   project's file itself is named.
4. **A text option may be sent as a list** (`serverSetting.separator`): its items split at the
   separator and trimmed, empty ones kept (JDT LS's empty group means "all other imports").
5. **With the Project scheme, the file gives only the options it keeps.** Options that have no
   configuration key (server settings) keep the IDE's values in both schemes, and the page stores
   them as the IDE's, instead of taking their defaults.

## Consequences

- Java's page keeps its formatter options in the project's `.zen/java-formatter.xml` with the
  Project scheme, a profile the team can commit and that other tools can read; a profile of every
  declared default formats exactly as the server does without one (checked against JDT LS 1.63).
- Kotlin's page has no effect with the Default (IDE) scheme until Export → EditorConfig File
  writes it, because Kotlin LSP has no setting for its formatter; the fwcd server formats with
  ktfmt and reads only the indent, so the page says so.
- Java and Kotlin have no arrangement rules: an arranger is a provider of a built-in module
  (`zen_syntax::Arranger`), plug-ins cannot provide one yet, and moving Java members needs care
  with the order of field and static initialisers.
