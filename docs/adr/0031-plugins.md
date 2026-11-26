# ADR-0031: Plug-ins: WebAssembly modules, a signed catalog and an offer on first open

- **Status:** Accepted
- **Date:** 2026-10-16
- **Refines:** [ADR-0011](0011-plugins-and-jetbrains-compatibility.md), decision 4
- **Extended by:** [ADR-0036](0036-bundled-plugins.md) (bundled plug-ins), [ADR-0071](0071-built-in-modules.md) (decision 7: an IDE's language is a built-in module, in every IDE, switched on per product)

## Context

The owner (2026-10-16): an IDE supports its own language out of the box (Go in GoGarden); support
for everything else (Markdown first) comes as plug-ins. When a file is opened that no installed
plug-in supports, and a plug-in for it exists, the IDE asks once whether to install it.
ADR-0011 already decided that our plug-ins are sandboxed WebAssembly modules.
This ADR fixes the first concrete version: what a plug-in is, how it is distributed and how the
IDE offers it.

## Decision

1. **A plug-in is one WebAssembly module** (`wasm32-unknown-unknown`), run by `wasmi`, a pure-Rust
   interpreter (ADR-0002). It has **no imports**: a plug-in cannot reach files, the network, the
   clock or the host. Every call runs with a fuel limit, so a looping plug-in is stopped, and a
   plug-in that traps is switched off for the session.
2. **ABI v0** (docs/plugins.md) covers syntax highlighting only. The module exports `memory`,
   `zen_abi() -> i32` (returns 0), `zen_alloc(len) -> ptr` and
   `zen_highlight(ptr, len) -> i64` (a pointer and a length of the result, packed). The result is
   a list of spans, 12 bytes each: start and end byte offsets and a highlight kind number. The ABI
   grows by new versions; a host loads only versions it knows.
3. **Plug-in sources live in this repository** under `plugins/`, one crate each
   (`zen-plugin-<id>`). They depend on no workspace crate: the ABI is their only interface. A
   plug-in crate is the one place where `unsafe` attributes (`#[unsafe(no_mangle)]` on the
   exports) are allowed, because WebAssembly exports need them; nothing else unsafe.
4. **Distribution: a separate signed catalog**, `releases/plugins.json` with its `.sig`, signed with
   the release key (ADR-0020) and published next to the release manifest. The release manifest is
   not extended: its schema rejects unknown fields, so a new field would break every installed
   Toolbox. The catalog lists each plug-in's id, name, description, the file extensions it
   supports and its versions (ABI version, URL, size, SHA-256). Published with
   `cargo xtask release plugin <id> <version>`.
5. **Installation is per IDE**: into the IDE's data folder,
   `plugins/<id>/` with `plugin.wasm` and `plugin.json`. The SHA-256 is checked before anything is
   written. Installed plug-ins load at start and take effect immediately after installation.
6. **The offer.** When the active editor shows a file that no highlighter handles, and the catalog
   has a plug-in for its extension, a banner above the editor says so and offers **Install** and
   **Ignore**. "Ignore" is remembered per extension. The catalog is fetched at most once per IDE
   session and only when such a file is opened (rule 10 of AGENTS.md: network access is explicit
   and visible); nothing is downloaded without the user's click.
7. **Built-in versus plug-in.** An IDE's own language module stays built in (Go in GoGarden, Rust
   in RustRonin). Everything else comes as plug-ins. A plug-in never replaces a built-in
   highlighter: built-in ones are tried first.

## Consequences

- Markdown support is the first plug-in, written by us (`plugins/markdown`).
- The release server serves and accepts a second signed document; the IDEs embed the server URL and
  the trusted public key, like Toolbox.
- The plug-in ABI is not stable yet; until it is declared stable, plug-ins are built from this
  repository only.
- Settings → Plugins (the plug-in manager) is a later step; for now plug-ins are installed
  from the offer and removed by deleting their folder.

## Alternatives considered

- **Products in the release manifest:** reuses publishing, but old Toolbox versions would show or
  count them, and the manifest has no place for file extensions.
- **Native dynamic libraries:** rejected by ADR-0011.
- **wasmtime:** faster, but much larger and not pure Rust; highlighting is cheap enough for an
  interpreter.
