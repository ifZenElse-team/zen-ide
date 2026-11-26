# ADR-0025: On macOS, IDEs are applications in the Applications folder

- **Status:** Accepted; rollback removed for now by [ADR-0029](0029-no-rollback-for-now.md)
- **Date:** 2026-10-16
- **Refines:** [ADR-0008](0008-toolbox-and-updates.md), [ADR-0018](0018-toolbox-is-the-entry-point.md)

## Context

Zen Toolbox installed RustRonin 0.1.0 as a bare executable in its own tools directory
(`~/Library/Application Support/space.ifzenelse.toolbox/tools/rustronin/0.1.0/bin/rustronin`). The
owner expected it in Applications: on macOS people find and start programs through Finder,
Spotlight, Launchpad and the Dock, which only know application bundles. Clicking the tool again
also started a second copy.

## Decision

1. **On macOS an IDE release is an application bundle**, `<Name>.app` (for example `RustRonin.app`),
   with `Info.plist` (`CFBundleIdentifier` `space.ifzenelse.<id>`), the executable
   `Contents/MacOS/<id>`, and an icon drawn in code in the product's colours. The release archive
   contains the bundle at its top level. Other systems keep `bin/<id>`.
2. **Zen Toolbox keeps every installed version in its tools directory** (for rollback) and **copies
   the active version into the Applications folder**: `/Applications` if the user may write to it,
   otherwise `~/Applications`. Installing, updating and rolling back replace that copy atomically;
   uninstalling removes it. Toolbox records which copy it made and never touches other applications.
3. **Toolbox launches a bundle through the system (`open`)**, which brings a running copy to the front
   instead of starting a second one.
4. Release archives are made without macOS metadata files (`._*`); the installer skips them anyway.

## Consequences

- Toolbox 0.1.1 and older cannot install bundle releases; Toolbox 0.1.2 must reach users before
  IDE releases switch to bundles.
- The applications are not notarised yet; installed by Toolbox, they carry no quarantine flag.

## Alternatives considered

- **Symbolic links in Applications pointing into the tools directory:** cheaper, but Spotlight and
  Launchpad do not follow them.
- **Installing only into Applications, without versions in the tools directory:** no rollback.
