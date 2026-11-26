# ADR-0150: Linux and Windows builds on the Mac runner, without Docker

- **Status:** Proposed (the owner, 2026-10-25)
- **Date:** 2026-10-25
- **Changes:** how the Linux and Windows builds of [ADR-0142](0142-release-builds-on-the-runner-published-on-the-release-machine.md)'s
  two-step releases are made; the Docker builds stay as a fallback

## Context

Linux and Windows releases were built in Docker on the release machine (`release-linux`,
`release-windows`): the Linux image is amd64, emulated on Apple Silicon, so a build took long, and
it published at once, outside the build-then-publish flow of the macOS releases. The CI runner
(ADR-0140) can cross-compile both natively: the owner prepared it with zig,
`cargo-zigbuild` and MinGW-w64, and a trial built RustRonin for Linux in 5.5 minutes and for Windows
in 6.7.

## Decision

1. **Linux x86_64** is built with `cargo zigbuild --target x86_64-unknown-linux-gnu.2.35`: zig links
   against glibc 2.35, the Ubuntu 22.04 the Docker image used, so the binaries run where they did.
   The IDEs link only glibc and load X11, xkbcommon and Wayland at run time, so no system libraries
   of the target are needed. aarch64 Linux is possible the same way and left for when asked.
2. **Windows x86_64** is built with `x86_64-pc-windows-gnu` and MinGW-w64's GCC as the linker, as the
   Docker image did; `cargo-xwin` and the MSVC target were not chosen, because they need Microsoft's
   SDK files downloaded under its licence, and the GNU build already runs.
3. **The `.deb` is made in xtask** (`xtask/src/deb.rs`): an `ar` archive of `debian-binary`,
   `control.tar.gz` and `data.tar.gz`, every entry owned by root, paths as `dpkg-deb` writes them, and
   the same layout as before: `/opt/<package>`, the `/usr/bin/<package>` link, a desktop entry and a
   256 px icon. No dpkg, no Docker.
4. **`release-build --platform macos|linux|windows|all`** (CI: `PLATFORM`, `macos` by default) builds
   the IDEs (`ides`, all or none) or Zen Toolbox for those platforms, each into a folder of its own
   with `build.json` recording the platform: the update archive the manifest lists, and the `.deb`
   (role `package`) or `.zip` (role `download`). A Linux or Windows build alone must be of a version
   already published for macOS; `all` builds a new version for every platform.
5. **`release-publish`** publishes each build's platform (`zen-releases publish --platform`) and
   uploads the `.deb` and `.zip`, with the same checks as for macOS (sizes and hashes, the commit on
   `origin/main` with its tree, the gate) and the same all-or-nothing rule for the IDEs, per platform:
   each platform in the set has every IDE once, at one version and commit. A Linux or Windows build of
   a version not published for macOS is refused unless its macOS build is in the set, and macOS is
   published first.
6. `release-linux` and `release-windows` (Docker) stay as a fallback for a Mac without the tools.

## Consequences

- Linux and Windows releases come from the runner like macOS ones, in minutes, and are published from
  the release machine with the same verification (AGENTS.md rule 12).
- The runner's machine needs the tools (docs/development.md).
- The packages are still not signed, and are checked in a container and with `file`, not on real
  Linux desktops or Windows.
