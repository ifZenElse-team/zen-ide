# ADR-0282: Third-party notices in every release artefact

- **Status:** Proposed
- **Date:** 2026-11-11
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (permissive dependencies only),
  [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md) (releases from a tag),
  [ADR-0174](0174-external-database-driver-protocol.md) (driver programs)

## Context

Every program we ship links several hundred crates under MIT, Apache-2.0, BSD, ISC, Zlib, BSL-1.0,
Unicode-3.0 and CDLA-Permissive-2.0, and carries fonts under the SIL Open Font License, the Ubuntu
Font Licence and the Bitstream Vera licence. Each of those licences asks that its text and the
authors' copyright notices travel with the binary; Apache-2.0 asks the same for the NOTICE files of
the authors. Until now no artefact carried any of them: only `cargo deny` checked that the licences
were allowed.

Some artefacts are folders (the macOS application, the Linux and Windows downloads, the `.deb`);
two are single files the release server hands out: a plug-in's WebAssembly module and a database
driver program.

## Decision

1. `cargo xtask notices` generates the notices from `cargo metadata --filter-platform` (so from
   `Cargo.lock`) and the licence files each crate publishes (`LICENSE*`, `LICENCE*`, `COPYING*`,
   `COPYRIGHT*`, `UNLICENSE*`, `NOTICE*`, a `LICENSES/` folder, and the manifest's `license-file`).
   It follows normal and build dependencies of the shipped package for the target it is built for.
   Identical texts are written once, with every crate that uses them listed above the text.
2. A crate whose package has no licence file is named in `licenses/crates.json`, with a standard
   text from `licenses/texts/` that its licence expression allows and, for licences that carry
   one, the copyright line of its upstream repository. The xtask test
   `every_shipped_crate_has_a_licence_text` (part of `cargo xtask ci`) fails when a shipped crate
   has neither, and when the file names a crate nothing ships any more.
3. Every folder artefact has a `Licenses` folder: `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`,
   `THIRD-PARTY-NOTICES` (every crate of the program, of zen-cli and of the bundled plug-ins),
   `RUST-STANDARD-LIBRARY-COPYRIGHT.html` (the toolchain's own notices of the standard library) and
   `fonts/` (Inter, the editor font, STIX Two Math, and egui's Noto Emoji, Ubuntu, Hack and
   emoji-icon-font licences). It is `Contents/Resources/Licenses/` in a macOS application and
   `Licenses/` next to `bin/` on Linux and Windows. The `.deb` also has
   `/usr/share/doc/<package>/copyright`. The Windows notices add the mingw-w64 runtime and
   winpthreads texts (`licenses/toolchain/`), which the `x86_64-pc-windows-gnu` target links in.
4. A plug-in module carries its notices in a WebAssembly custom section named `zen-licences`,
   appended after the build; runtimes ignore custom sections.
5. A driver program carries its notices inside the binary: the release build sets
   `ZEN_THIRD_PARTY_NOTICES` to the generated file, the driver kit's build script embeds it, and
   `zen-db-<id> --licences` prints it.
6. `release_flow::finish` checks every artefact before it records a build: archives, zip files and
   packages must hold every file of the `Licenses` folder, a module its section, a driver program
   its notices. The disk image holds the application already checked in its archive.

## Consequences

- An artefact without notices cannot be released; a new dependency without a licence file fails
  the gate until someone adds it to `licenses/crates.json`.
- Generating needs every crate's source, which `cargo metadata` downloads like a build does.
- Plug-in modules grow by their notices (tens of kilobytes); the application by about 3 MB.
- What the IDE downloads at run time (language servers, JDKs, Mermaid) is not shipped by us and is
  described in [docs/licences.md](../licences.md), not in these files.

## Alternatives considered

- **cargo-about.** It does the same job, but would be one more tool to install on every runner, and
  it does not know our artefacts, plug-in modules or driver programs.
- **A file next to each module and driver program on the release server.** It would need a new kind
  of file in the signed manifest and in every installer, and a copy can get lost; inside the file the
  notices stay with the program.
- **Checking the notices into the repository.** About a megabyte of generated text per program,
  changed by every dependency update.
