# ADR-0183: Language injection: another language inside string literals, starting with SQL in code

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0046](0046-project-models-and-outlines.md) (outlines and highlighting),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md) (incremental highlighting),
  [ADR-0050](0050-products-are-plugin-bundles.md) (every IDE),
  [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md), [ADR-0170](0170-generated-sql-in-each-drivers-dialect.md)

## Context

Code talks to databases through SQL written in string literals: `db.Query("SELECT …")` in Go,
`sqlx::query!("…")` in Rust, `cursor.execute("""…""")` in Python, `` sql`…` `` in TypeScript,
`"SELECT …"` in Java. The IDE had SQL support (highlighting, completion and checks from a
connection's snapshot, ADR-0164) only in consoles and `.sql` files; inside a string the SQL was one
green run of text. The remedy is *language injection*: a literal is read as another
language, by a comment (`// language=SQL`) or by rules, and that language's tools work inside it.

The decision has to hold for every IDE (ADR-0050): the host languages come from built-in modules
(Go, Rust) and from plug-ins (Python, JavaScript, TypeScript, Java); the injected language's
highlighting may come from a plug-in (SQL) or from the platform. And typing must never get slower:
a file is highlighted line by line as it is edited (ADR-0077), and finding literals must not undo
that.

## Decision

1. **The mechanism is the platform's, in `zen-syntax` (L1)**, independent of any language:
   - A **literal** is found from the host's highlighting alone: spans of kind `String`, pieces of
     one literal on several lines joined, its prefix and quotes taken off (`r#"…"#`, `b"…"`,
     `f"…"`, triple quotes, backticks), and whether backslashes escape in it (not in raw literals).
     No host language has to change: every highlighter already marks its strings.
   - An **`Injector`** names a language, tests cheaply whether a text may hold it (`may_hold`),
     accepts or refuses a literal (given the text around it and the literal's code with escapes
     read), and highlights that code.
   - **`Highlighting` keeps the fragments** that its injectors accept, in step with the text: an
     edit looks again only at the lines the host's highlighter lexed again, one line around them and
     the literals they touch; fragments after it move; a fragment whose text did not change keeps its
     highlighting. A highlighter that is not incremental highlights the whole text after each edit,
     and so are its literals looked at again.
   - **`Code`** is a fragment's content as its language reads it, escapes read, with every byte
     tied to its place in the file, so that completion, warnings and navigation map back exactly.
   - **The editor lays the fragment's spans over the literal's** (`spans_with_injections`): the
     quotes keep the string's colour, the injected tokens take their own kinds, the text between them
     is plain. Other readers (bracket matching, spelling, search in literals) keep the host's spans,
     where the literal is still a string.
2. **Which injectors a file gets is the window's choice** (`Highlighters::set_injectors`), by the
   file's name, like its highlighter. Today one: SQL.
3. **SQL in code** (`zen_db::inject`, compiled for WebAssembly too, so it is in every IDE): Go, Rust,
   Python, JavaScript, TypeScript, Java and Kotlin files; a literal is SQL when a `language=` comment
   on its line or the line before names an SQL dialect (another name keeps it out), when a `sql` tag
   stands before a template literal, or when its text reads as a statement: a statement keyword,
   wholly upper or lower case, followed by the words that statement needs. A bare `"DELETE"` (an
   HTTP method) or "Select a file" (a sentence) is not SQL. Its highlighting is the SQL plug-in's
   when installed, else the platform's own from the dialect scanner of ADR-0170.
4. **Completion, navigation and warnings reuse the console's SQL support** on a connection's
   snapshot, offline: the connection is the file's own binding, else the project's default, both
   kept per project in the data folder with the scripts' bindings (never in the file or the
   repository: a connection may be personal, ADR-0165). Without one, completion offers keywords.
   The `db.injection.*` commands list fragments and set both bindings; `db.sql.complete`,
   `db.sql.resolve` and `db.sql.navigate` take a place inside a fragment.

## Alternatives considered

- **Tree-sitter injections** (an injection query per grammar): our hosts are hand-written lexers and
  WebAssembly plug-ins, not tree-sitter grammars, and the C runtime is ruled out (ADR-0002).
- **Asking each host language for its literals** (a new trait method or plug-in ABI call): every
  module and plug-in would change, and the highlighting already says where strings are.
- **Laying the injected spans into `Highlighting::spans` for every reader**: bracket matching,
  spelling and "in string literals" searches would change meaning inside literals; drawing is where
  the injected language is wanted today, and reading it is one call away.
- **SQL found only by `language=SQL` comments** (manual injection): most code never has
  them; the statement rule finds the common case and the comment stays for the rest, both ways.

## Consequences

- A file without SQL pays one byte scan when it opens and a scan of the edited lines per edit;
  typing in a large Go file stays line by line.
- Other injected languages (regular expressions, JSON, HTML in templates) are one `Injector` each.
- A query concatenated from several literals is read literal by literal; `${…}` in a template is
  read as text. The connection is chosen by commands until running SQL from code (C11) brings it
  into the editor.
