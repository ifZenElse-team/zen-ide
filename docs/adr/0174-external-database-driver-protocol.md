# ADR-0174: External database drivers: a JSON-RPC protocol over a driver program's standard streams, supervised by the IDE, shipped in plug-ins

- **Status:** Proposed
- **Date:** 2026-10-27
- **Relates to:** [ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md) (decision 5 decides external
  driver processes), [ADR-0165](0165-database-connections-types-and-secrets.md) (secrets),
  [ADR-0172](0172-production-safety-and-agents-on-databases.md) (production safety),
  [ADR-0031](0031-plugins.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0055](0055-language-servers.md), [ADR-0128](0128-a-plugin-installs-its-language-servers.md),
  [ADR-0148](0148-java-plugin-and-eclipse-jdt-ls.md)

## Context

ADR-0164 decision 5 lets a database the IDE does not compile in be served by a **driver program**
that the IDE starts and talks to with a JSON-RPC protocol of its own, declared by a plug-in in
`plugin.json` and installed only when the user asks. It names the methods (`initialize`, `connect`,
`children`, `execute` with rows as notifications, `cancel`, `close`) and leaves the protocol to the
first such driver. The owner asked (2026-10-26) that drivers let any database connect, relational or
not; this record writes the protocol, the IDE's side of it and how plug-ins ship driver programs.
Four things need a decision beyond ADR-0164: the framing, how rows travel without flooding the IDE,
how processes are run and supervised, and what a plug-in's manifest says.

## Decision

1. **Framing: one JSON-RPC 2.0 message per line** (newline-delimited JSON, UTF-8) on the program's
   standard input and output, not LSP's `Content-Length` headers. JSON never needs a raw newline
   (strings escape it), every language prints a line in one call, and a driver author can test a
   program by typing lines. A line is at most 64 MiB; a longer one ends the program as broken.
   Standard error is free text, which the IDE keeps in the driver's log. The program exits when its
   standard input closes.
2. **Rows are pulled, not pushed.** Instead of rows as notifications (ADR-0164's sketch), a driver
   that declares `serverPaging` opens a cursor with `query` and the IDE reads it with
   `fetch { cursor, count }` page by page, as the result store wants rows (ADR-0164 decision 7);
   `closeCursor` ends it early. A driver never sends rows nobody asked for, so backpressure needs no
   credits and a program cannot fill the IDE's memory. `execute { maxRows }` answers a whole small
   result at once and is what every driver must have; a driver without `serverPaging` is read through
   it with the result store's cap, as built-in drivers without paging are.
3. **Everything the engine needs is a method of the existing driver interface**, with the existing
   types as the wire types: `Capabilities`, `Node`, `Column`, `Value` (`{"kind": …, "value": …}`),
   the snapshot model's catalogues for `introspect`, `TrackedState` for `apply`, `ErrorKind` in
   the error's `data.kind`. So production safety, history, export, data editing and the navigator
   work for an external driver through its declared capabilities with no special case:
   `ExternalDriver` implements `zen_db::driver::Driver`, and the engine does not know it is external.
   Statements of a SQL language are judged by the IDE's own tokens (ADR-0172); a language that is not
   SQL names its reading verbs (`readVerbs`) and every other statement counts as a change.
4. **The password travels only in `connect`**, in memory: never in arguments, the environment, a
   file or a log. The IDE serialises the request into a buffer it wipes after writing, remembers the
   password only to remove it from the driver's log and errors, and never logs request or answer
   bodies. `db.driver.log` shows lifecycle lines and standard error, redacted.
5. **Processes: one per connection, or one shared, as the manifest declares** (`process`:
   `perConnection` by default, or `shared`). A connection's contexts are sessions inside its
   program. A program starts when the first session needs it, after its SHA-256 is checked, and gets
   `shutdown`, then its input closed, when its last session closes; one that does not exit within two
   seconds is killed, and every program is killed when its owner is dropped (`kill_on_drop`). A
   program that dies fails what was waiting with a lost connection, and the engine's own reconnect
   starts a new one; three exits by itself within a minute stop it from starting for that minute.
   `initialize` must answer within ten seconds, `connect` within the connection's timeout, control
   requests (`ping`, `cancel`, `apply`, `closeCursor`) within thirty; statements are bounded by
   their own timeout and Cancel. Everything runs on the engine's runtime, never on the UI thread.
6. **The manifest: `"databaseDrivers"` in `plugin.json`**, each with `id`, `name`, optional `icon`,
   `protocol` (1), `process`, `defaultPort`, `capabilities` (shown before the program first runs;
   `initialize` answers the same and replaces them), `readVerbs`, `guardsReadOnly`, `args`, and
   `platforms`: per platform (`macos-aarch64`, `linux-x86_64`, `windows-x86_64` …) the program's `url`
   (absolute or relative to the catalog), `sha256` and optional `file` name. The program is one
   executable file installed at `<plug-ins>/<plugin>/drivers/<driver>/<file>`; an archive is not
   needed for a program that is one file, and a later protocol may add one. A driver's id never
   replaces a compiled-in one.
7. **Downloads only on the user's word.** Installing or updating a plug-in from Settings → Plugins
   (or `plugins.install`, `plugins.update`) downloads its driver programs for this computer and
   checks their SHA-256; the update the IDE makes by itself at start skips plug-ins that declare
   drivers, so nothing is downloaded without the user. A program that does not match is not kept and
   is checked again before every start. An agent cannot install or update such a plug-in
   (`install_plugin`, `plugins.plugin.install`, `plugins.plugin.update`, `plugins.updates.all`
   refuse it with code `userOnly`, naming it): a program that runs on the computer is installed by
   the user, from Settings → Plugins.
8. **Conformance is a test suite in the repository**, `cargo test -p zen-db --test external_driver`,
   which runs against the in-repository reference driver (`zen-db-reference-driver`, a small
   in-memory SQL database built with the crate and shipped in no product) or, with `ZEN_DB_DRIVER`,
   against any driver program.

## Consequences

- A database we cannot compile in (Oracle, DuckDB, MongoDB until its TLS is fixed, a JDBC or ODBC
  bridge) can come as a plug-in in any language without C in the IDE, and a crash in it never takes
  the IDE down.
- The protocol is versioned (`protocol: 1` in the manifest and in `initialize`); a new version is a
  new ADR, and the IDE refuses a driver of a version it does not speak.
- Plug-ins that declare drivers are not updated at start; the user updates them from Settings →
  Plugins.
- The catalog lists driver programs by URL; publishing them on our release server
  (`cargo xtask release plugin`) is not built yet, so a first-party driver plug-in needs that step
  first.

## Alternatives considered

- **LSP's `Content-Length` framing.** Robust against stray output, but a driver author then needs a
  framing library or careful code in every language; with one message per line, a stray print breaks
  only that line, which the IDE drops. Rejected.
- **Rows as notifications** (ADR-0164's sketch). Fast for a full read, but the IDE must then grant
  credits or buffer whatever the program sends; pulling pages is simpler and matches the result
  store. Rejected.
- **A JDBC driver loaded into a JVM the IDE runs.** That is one possible driver program, not the
  protocol; the bridge can be written against this protocol.
- **An archive per platform, as language servers have** (ADR-0148). A driver program that is one
  file does not need one; kept for a later protocol if drivers need more files.
