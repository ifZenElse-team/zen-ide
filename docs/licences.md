# Licences of what we ship

Zen IDE is licensed under the MIT License or the Apache License, Version 2.0 (`LICENSE-MIT`,
`LICENSE-APACHE`, `NOTICE`). Everything else in a release keeps its own licence, and its notices
travel with it ([ADR-0282](adr/0282-third-party-notices-in-every-release-artefact.md)). This page
says where they come from, where they land, and what to do when the check fails.

## Where the notices are

| Artefact | Notices |
|---|---|
| macOS application (an IDE, Zen Toolbox), in its `.tar.gz` and the disk image | `Contents/Resources/Licenses/` |
| Linux and Windows downloads (`.tar.gz`, `.zip`) | `Licenses/` next to `bin/` |
| Linux package (`.deb`) | `/opt/<package>/Licenses/` and `/usr/share/doc/<package>/copyright` |
| Plug-in module (`.wasm`), bundled or from the catalog | the custom section `zen-licences` |
| Database driver program (`zen-db-<id>`) | inside the program: `zen-db-<id> --licences` prints them |

A `Licenses` folder holds:

- `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE` of Zen IDE;
- `THIRD-PARTY-NOTICES`: every crate compiled into the program, zen-cli and the bundled plug-ins,
  with its version, licence expression, repository and licence texts, each text once with every
  crate that uses it; for Windows, the mingw-w64 runtime and winpthreads texts too;
- `RUST-STANDARD-LIBRARY-COPYRIGHT.html`: the Rust toolchain's notices of its standard library;
- `fonts/`: `OFL-Inter.txt`, `OFL-JetBrainsMono.txt`, `OFL-STIXTwoMath.txt`, and egui's
  `OFL-NotoEmoji.txt`, `UFL-Ubuntu.txt`, `Hack.txt` (MIT and Bitstream Vera) and
  `MIT-emoji-icon-font.txt`.

People read them in the application: **Help → Open-source Licences** in an IDE (also in the macOS
application menu, which the About panel points to; the command `view.openSourceLicences.open`), and
**Settings → Open-source Licences** in Zen Toolbox (`toolbox.licences.list`). Both find the folder
next to the running program (`zen_update::licences`); a development build has none and says so.

## How they are made

`cargo xtask notices` reads `cargo metadata --filter-platform <target>` (so `Cargo.lock`), takes
the shipped package's normal and build dependencies for that target, and reads each crate's licence
files from its source (`LICENSE*`, `LICENCE*`, `COPYING*`, `COPYRIGHT*`, `UNLICENSE*`, `NOTICE*`, a
`LICENSES/` folder, the manifest's `license-file`). The release commands (`cargo xtask release`,
`release-build`, the Linux and Windows builds) write them into each artefact, and
`release_flow::finish` refuses a build whose archive, package, module or driver program lacks them.

```sh
cargo xtask notices                                         # check every shipped program
cargo xtask notices gogarden /tmp/notices.txt               # GoGarden for this computer
cargo xtask notices toolbox x86_64-pc-windows-gnu /tmp/t.txt
```

## When the check fails

The xtask test `every_shipped_crate_has_a_licence_text` (in `cargo xtask ci`) fails when a crate
in a shipped program publishes no licence file. Add it to
[`licenses/crates.json`](../licenses/crates.json):

- `licence`: a text in [`licenses/texts/`](../licenses/texts/) that the crate's licence expression
  allows (Apache-2.0 when it is one of the choices, because its text names no author);
- `copyright`: for MIT and other licences whose text starts with the authors' notice, the copyright
  line of the crate's upstream repository (its `LICENSE` file at the released version), never made
  up;
- `files`: licence files inside the package that the scan does not find, such as egui's font
  licences under `fonts/`;
- `source`: where the text or the copyright line was taken from.

The test also fails when the file names a crate that no shipped program uses any more: remove it.
A new kind of licence also needs `deny.toml` to allow it first (ADR-0002).

## Not shipped: downloaded at run time

The IDE downloads some tools only when asked, from their publishers, under their own licences;
they are not in our artefacts and not in `THIRD-PARTY-NOTICES`:

| What | From | Licence |
|---|---|---|
| Eclipse JDT Language Server, with the java-debug bundle (the Java plug-in) | download.eclipse.org, Maven Central | EPL-2.0; java-debug EPL-1.0 |
| Kotlin language server (the Kotlin plug-in) | its vendor's download server (the address is in `plugins/kotlin/plugin.json`) | Apache-2.0 |
| JDKs (**Download JDK**) | api.adoptium.net (Eclipse Temurin) | GPL-2.0 with the Classpath Exception |
| Maven and Gradle distributions, JaCoCo | repo.maven.apache.org, services.gradle.org | Apache-2.0; JaCoCo EPL-2.0 |
| Language servers installed with `go install`, `npm install -g`, `cargo install`, `rustup` | their registries | their own |

Mermaid diagrams are drawn by the `mermaid-svg` crate, compiled in and listed in the notices; no
Mermaid code is downloaded.
