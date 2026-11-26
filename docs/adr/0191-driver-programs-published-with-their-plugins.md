# ADR-0191: Database driver programs are built, signed and published with their plug-in

- **Status:** Proposed
- **Date:** 2026-10-31
- **Relates to:** [ADR-0031](0031-plugins.md), [ADR-0142](0142-release-builds-on-the-runner-published-on-the-release-machine.md),
  [ADR-0150](0150-linux-and-windows-builds-on-the-mac-runner.md), [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md),
  [ADR-0174](0174-external-database-driver-protocol.md), [ADR-0175](0175-first-party-database-driver-programs.md)

## Context

ADR-0174 lets a plug-in declare database drivers whose program the IDE downloads per platform
(`platforms` in `databaseDrivers`: a URL, a SHA-256 and a file name), and ADR-0175 put the team's
own programs in `drivers/` (SQL Server, MongoDB, DuckDB, then Oracle). Their plug-ins could not be
released: `cargo xtask release plugin` built only the WebAssembly module, nothing put the programs
on the release server, and the IDE fetched a program declared at an absolute URL from
`<catalog folder>/https://…`, which answered 404.

## Decision

1. **The build.** `release-build plugin <id> <version> [--platform macos|linux|windows|all]` (and
   `release plugin`, macOS only) builds, besides the module, the program of every driver the
   plug-in declares that has a crate in `drivers/<driver>` (package `zen-db-<driver>`), once per
   platform asked: this Mac's (`macos-<arch>`), Linux x86_64 and Windows x86_64 as the IDEs' native
   builds make them (ADR-0150). The tag pipeline asks for every platform. A macOS program is signed
   ad hoc (`codesign --force --sign -`, as the IDEs' applications are) and checked with `codesign
   --verify --strict`. `--platform` changes nothing for a plug-in without such drivers.
2. **Names and addresses.** A program is the build's file `zen-db-<driver>-<version>-<platform>`
   (`.exe` on Windows) with the role `driver` in `build.json`, stored on the release server beside
   the module, at `artifacts/plugin-<id>/<version>/<that name>`. The build writes it into the
   plug-in's catalog entry: `platforms.<platform>` gets that address relative to the catalog, its
   `sha256`, its `size` and `file` `zen-db-<driver>` (`.exe` on Windows), the name the plug-in
   finds it by. Platforms declared in `plugin.json` that were not built stay as declared.
3. **Publishing.** `release-publish` (and the tag pipeline's stage job) checks that the build's
   programs and the catalog entry agree (every program built is listed with its SHA-256 and size,
   every program listed at a relative address was built), uploads the programs, then signs and sends
   the catalog. The staged downloads are checked as every other file: size, SHA-256 and, for macOS,
   the signature.
4. **The release server** accepts or stages a catalog only when every module and every driver
   program it lists at a relative address is stored with the listed size and SHA-256; a program
   listed there without its size is refused. Programs at absolute addresses (third-party plug-ins)
   are not its business.
5. **The IDE.** A program's address is absolute, kept as it is, or relative to the catalog. The
   catalog is signed, so the SHA-256 the IDE checks the downloaded program against (before it keeps
   it and before every start) is the publisher's: that is the program's signature check on every
   platform. An update downloads the program again when its SHA-256 changed, replacing the old one.
   `size` is optional in a declaration, and the IDE does not need it.

## Consequences

- The SQL Server, MongoDB and DuckDB plug-ins (and Oracle's when it lands) can be released as any
  plug-in, once each has a version of its own and its changelog section; the owner releases them.
- The release server's code changes (decision 4): the owner deploys it from the Mac
  (the server's deploy script, kept outside this repository) before the first plug-in with programs is published. A catalog
  listing programs sent to an older server is accepted without the program check, so the order only
  matters for that check.
- A plug-in's catalog entry holds one set of programs, the latest version's; installing an older
  version from the catalog brings the latest programs, which speak the same protocol.
- Programs for macOS on Intel are not built, as no IDE is released for it; a plug-in can still
  declare one at an absolute address.

## Alternatives considered

- **Programs inside the module's archive.** The catalog lists one file per version; every platform's
  program would be downloaded by everyone. Rejected.
- **A `--program` option of `zen-releases publish-plugin`.** `zen-releases upload` already uploads
  only what the server lacks; the catalog entry comes from the build. Not needed.
- **Developer ID signing and notarisation.** No Apple Developer ID exists yet (docs/releasing.md); the
  IDE downloads programs without the quarantine flag, so an ad-hoc signature runs on Apple silicon.
