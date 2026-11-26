# Naming

> **Status:** accepted · **Last updated:** 2026-10-19 · **Decision:** [ADR-0005](adr/0005-naming.md)

Names in Zen IDE follow one rule: **infrastructure has plain names, products have brand names.**
Anyone should understand what `Zen Platform` or `zen-core` is without being told; product names are
allowed to be memorable instead.

## 1. Hierarchy

```text
ifzenelse                  the team
└── Zen IDE                the product family
    ├── Zen Platform       the shared engine
    │   └── Zen Core       command registry, session and settings
    ├── Zen Toolbox        installs, updates and launches the IDEs
    └── the IDEs           RustRonin, GoGarden, PyPetal, ...
```

The standard way to refer to a product in text:

> RustRonin, a Zen IDE by ifzenelse, built on the Zen Platform.

## 2. Products

Each IDE name is **the language name followed by an image from Japanese culture**, in one word with
two capitals, and **the two parts start with the same letter**; for web development the first part is
**Web** rather than a language name ([ADR-0027](adr/0027-webwave.md)), and an IDE for a field rather
than a language names the field (**Devops**, **Test**): **R**ust**R**onin, **G**o**G**arden, **W**eb**W**ave
([ADR-0019](adr/0019-alliterative-product-names.md)). The image is chosen for an association with
the language or with what the IDE does. The theme ties the family to the team name:
if**zen**else.

| Product | Language | Image | Why |
|---|---|---|---|
| **RustRonin** | Rust | Ronin, a masterless samurai | An independent master of the craft; R-R alliteration |
| **GoGarden** | Go | A garden | A place where it is pleasant to stay, as in a beer garden |
| **PyPetal** | Python | A petal of a cherry blossom | Light and graceful, like Python code |
| **JvmJizo** | Java and the JVM (Kotlin, Groovy, Scala) | A Jizō, the stone guardian by the road | Steadfast and patient, like Java's backward compatibility; "Jvm" because "Java" may not be in a product name; J-J alliteration ([ADR-0145](adr/0145-jvmjizo.md)) |
| **WebWave** | The web: TypeScript, JavaScript, HTML and CSS | Hokusai's Great Wave | "Web" says what it is for ([ADR-0027](adr/0027-webwave.md)) |
| **CppCrane** | C and C++ | A crane | A symbol of longevity, like C++ itself |
| **SharpShuriken** | C# and .NET | A shuriken, a throwing star | C *sharp*, and a sharp star |
| **PhpPagoda** | PHP | A pagoda | P-P alliteration |
| **RubyRyokan** | Ruby | A ryokan, a traditional inn | Ruby was created in Japan; R-R alliteration |
| **SqlScroll** | SQL and databases | A scroll | A scroll keeps records ([ADR-0276](adr/0276-sqlscroll.md)) |
| **DevopsDaruma** | DevOps: containers, infrastructure as code, pipelines | A daruma doll | It always stands back up, as systems should; "Devops" names the field ([ADR-0049](adr/0049-devopsdaruma.md), [ADR-0285](adr/0285-devopsdaruma-app.md)) |
| **TestTotami** | QA: end-to-end and UI tests, API and load testing, test management | Tatami mats, spelled "Totami" | A tatami floor is laid mat by mat until no square is uncovered, as tests cover a product; "Test" names the field ([ADR-0067](adr/0067-testtotami.md)) |

Products without a language in their name are infrastructure and use plain names: **Zen Toolbox**,
**zen-cli**, **zen-pty-host** (the terminal host, [ADR-0275](adr/0275-terminal-sessions-that-outlive-the-ide.md)).

### The mascot

The family's mascot is **Kozō** (小僧), a young novice monk sitting in the lotus position, drawn as
pixel art of our own. Each product's Kozō wears a robe in the colours of its tile and has a prop
from its image (GoGarden a watering can, RustRonin a wooden sword, WebWave a surfboard, JvmJizo a
small stone Jizō with a red bib, Zen Toolbox a toolbox...); the family's wears the Zen indigo. The design and where he appears:
[ADR-0125](adr/0125-kozo-the-family-mascot.md).

### Reserve names

Names that follow the rule, kept for languages that may get an IDE later: **SwiftSakura** (Swift),
**DartDojo** (Dart and Flutter), **ScalaShrine** (Scala), **ZigZen** (Zig), **ElixirEnso** (Elixir).

### Third-party trademarks in product names

Language names belong to their owners (the Rust Foundation, Google, the Python Software Foundation,
the Kotlin Foundation, Oracle and others). Using a language name inside a product name, together
with a clear non-affiliation notice, is common practice, but each owner's trademark policy must be
checked before the product's first public release. Known constraints:

- **Java** must not appear in a product name (Oracle's trademark policy, below). The JVM IDE, Java
  first, is named after the platform: **JvmJizo** ([ADR-0145](adr/0145-jvmjizo.md)), "a Zen IDE for
  Java and the JVM" in its description (a reference to the language, not a name). See **JVM**
  below: that name is not settled.
- **Kotlin** must not appear in a product or plug-in name either (the Kotlin Foundation's
  guidelines, below): a Kotlin plug-in is "Kotlin support" in text, never "Kotlin IDE".
- **Rust**: check the Rust Foundation Trademark Policy before RustRonin's first public release.

#### Checked on 2026-10-26

**Oracle**, [Third Party Usage Guidelines for Oracle Trademarks](https://www.oracle.com/legal/trademarks/)
(read 2026-10-26; the page names Oracle, Java, MySQL and NetSuite as registered trademarks):

- "Do not use Oracle trademarks or potentially confusing variations as all or part of your company,
  product or service names. If you wish to note the relationship of your products or services to
  Oracle products or services, please use an appropriate tag line": "XYZ for Oracle database", not
  "OraXYZ". The same holds for domain names.
- Tag lines such as "for …" are allowed; apart from body copy they must be in "significantly
  smaller type" than our name, in a different font or colour, and never in Oracle red, "to avert any
  implication that your product or service is produced or endorsed by Oracle".
- Do not imitate Oracle's trade dress or logos. We use no Oracle or Java logo (the coffee cup):
  Java files carry our own drawn badge.
- Every product, manual and web page bearing Oracle trademarks should carry the credit line, word for
  word: "Oracle, Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates.
  Other names may be trademarks of their respective owners."

**JVM is claimed by Oracle too.** Oracle's own JDK 25 documentation titles a specification "JVM(TM)
Tool Interface" ([docs.oracle.com](https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html)),
and the US registration 2881993 (serial 75918281, filed 2000-02-14 by Sun Microsystems, registered
2004-09-07) lists Oracle America, Inc. as its owner, "registered and renewed" on 2014-07-24
([Justia](https://trademarks.justia.com/759/18/jvm-75918281.html)). Whether it was renewed again in
2024 was not verified: the USPTO's TSDR record (serial 75918281) needs a browser. "JvmJizo" carries
"Jvm" as part of the product name, which the guideline above asks third parties not to do with
"Oracle trademarks or potentially confusing variations". **Open, for the owner before JvmJizo's
first public release:** check the TSDR record (and a lawyer's view, or a question to
`trademar_us@oracle.com`); if the mark is live, rename the IDE after its field by a new ADR, as
ADR-0145 foresees (Jizō stays its image).

**The Kotlin Foundation**, [Kotlin brand assets usage guidelines](https://kotlinfoundation.org/guidelines/)
(read 2026-10-26):

- "Kotlin" may be used referentially: software written in, compatible with, targeting or for use
  with the Kotlin programming language. It may never be part of "a company name, product name, domain
  name, or business trading name" ("your company cannot be called … 'Kotlin IDE'").
- Name the relationship as "in Kotlin®", "for Kotlin®", "compatible with Kotlin®"; the first or most
  prominent mention carries ®; no other stylisation, colour or font than the surrounding text; not as
  a verb, possessive or with other spellings.
- The Kotlin logo is for official, pre-approved uses only; the Kotlin icon may stand among other
  languages' icons or mark a file written in Kotlin, but not in product or plug-in icons. Our `.kt`
  badge is our own drawing, not the Kotlin icon. Kodee, the mascot, is free under CC BY 4.0.
- The Foundation may ask for a disclaimer that a product is not associated with it; ours is in the
  credit line below.

#### What our texts say

- **Names**: never "Java", "Kotlin" or a variation of either in a product, domain or bundle name.
  A plug-in's display name is the language it supports ("Java", as "Go" and "Rust" are), shown in
  the Marketplace beside "Zen IDE plug-in" and its description; never "Java IDE" or "Official".
  Open for the owner: whether the plug-ins should read "Java support" and "Kotlin support" instead,
  which both guidelines would accept without question.
- **JvmJizo's descriptor**: "a Zen IDE for Java and the JVM" (Russian: "Zen IDE для Java и JVM").
  Under the product name (the website's family list, Zen Toolbox) it is the tag line "for Java and
  the JVM" ("для Java и JVM"), smaller and in a secondary colour, never in red. Store and download
  listings: "JvmJizo — a Zen IDE for Java and the JVM, by the ifzenelse team: Java first, with Maven
  and Gradle, and Kotlin as a plug-in. Not affiliated with Oracle or the Kotlin Foundation."
  ("JvmJizo — Zen IDE для Java и JVM от команды ifzenelse: прежде всего Java, с Maven и Gradle, а
  Kotlin — плагином. Не связан с Oracle и Kotlin Foundation.") Never "the Java IDE", "official",
  "certified" or "Java compatible".
- **Credit lines**, in About of every Zen IDE (`menu-app-about-trademarks`, en and ru; the bundle's
  copyright line in English), on the website's footer, in README.md and TRADEMARKS.md:
  "Oracle, Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its affiliates.
  Other names may be trademarks of their respective owners. Kotlin® is a registered trademark of the
  Kotlin Foundation."

#### Kotlin's default language server

The default server of the Kotlin plug-in is the Kotlin team's kotlin-lsp. Its repository is Apache-2.0, and
its README says the server is partially closed-source. The IDE only downloads it from its vendor's
address on the user's click, never hosts or bundles it, and says where it
offers it whose software it is and under whose terms. fwcd/kotlin-language-server (MIT) is
the alternative, chosen in Settings → Language Servers.

## 3. Identifiers

| What | Rule | Example |
|---|---|---|
| Platform crates | `zen-<area>` | `zen-core`, `zen-text`, `zen-lsp` |
| Language crates | `zen-lang-<language>` | `zen-lang-rust` |
| App crates and directories | kebab-case product name | `apps/rust-ronin` |
| App binaries | kebab-case product name | `rust-ronin` |
| App packages | the binary name, or `<name>-app` when a library already owns the name | `zen-toolbox-app` (binary `zen-toolbox`; the library `zen-toolbox` holds the feature) |
| Shell launchers | the product id, as the executable ([ADR-0086](adr/0086-ide-commands-on-path.md)); zen-cli's is `zen` | `gogarden`, `rustronin`, `zen` |
| Command ids | `<area>.<group>.<action>`, lowerCamelCase segments | `editor.caret.moveToLineEnd` |
| Bundle identifiers | `space.ifzenelse.` + product name in lowercase, no separators ([ADR-0017](adr/0017-project-domain.md)) | `space.ifzenelse.rustronin`, `space.ifzenelse.toolbox` |

Taken names to avoid: the crates `zen` and `zen-engine` on crates.io belong to other projects; the
engine crate is therefore `zen-core`.

## 4. Names we considered and rejected

Recorded so that the same discussion does not happen twice.

| Name | Reason |
|---|---|
| A shared `-Zen` suffix (RustZen, GoZen, ...) | `rustzen` is an existing organisation and domain (rustzen.dev); Webzen is a registered game company |
| `-Craft` suffix | Belongs to the ArtCraft family of apps |
| `-Forge`, `-Bench`, `-Smith` suffixes | Overused or already meaning something else (RubyForge, benchmarks, compiler fuzzers) |
| Pure Japanese names (Sabi, Hebi, Kumo, ...) | Rejected in favour of names that contain the language |
| Wabi, Zendō as the family name | Conflicts (the Wabi app) or less clear than "Zen IDE" |
| Zazen, Ishizue as the platform name | The platform keeps a plain name: Zen Platform |
| PyKoi, PyBonsai | Existing projects with the same name |
| PyLantern, SharpKatana | Did not alliterate; replaced by PyPetal and SharpShuriken (ADR-0019) |
| TsTorii | "Ts" did not say "web" clearly; replaced by WebWave (ADR-0027) |
| WebWisteria, WebWillow | Alternatives considered for ADR-0027 |
| PyPond, PyPine; SharpShrine, SharpSensei | Alternatives considered for ADR-0019 |
