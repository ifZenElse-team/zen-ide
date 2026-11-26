# ADR-0280: The clean-room process: observers write a specification, implementers build from it

- **Status:** Proposed
- **Date:** 2026-11-11
- **Supersedes:** ADR-0012, which is not part of this repository (a user interface reproduced from
  another product), and [ADR-0015](0015-toolbox-first.md) (Zen Toolbox first)
- **Amends:** [ADR-0002](0002-open-source-clean-room-pure-rust.md), decision 2 (the sentence on
  other IDEs' source code); [ADR-0016](0016-toolbox-colour-scheme.md), decisions 1 and 3;
  [ADR-0057](0057-git-workspace.md), decisions 3, 4 and 7
- **Relates to:** [ADR-0038](0038-keymap.md) (the keymap), [ADR-0024](0024-localisation.md) (user-visible
  text)

## Context

ADR-0012 asked for the interface of another vendor's IDE to be reproduced exactly, built by
observation. In practice the repository filled with that vendor's pixels (more than a hundred
screenshots among the UI notes), notes that cite its installation folder, its settings files and
its internal class names, quotes of its sentences, and code comments that say which product a piece
of code reproduces. An audit on 2026-11-02 found the same pattern for other products (a database
tool, a Git client). None of this is copied code, but it is exactly what a rights holder would point
at, and it cannot show that our code was written independently.

The owner decided on 2026-11-02 that Zen IDE is built as a clean room, and on 2026-11-03 that the
default keymap and the feature names users search for are kept. A clean room is a process that can
be shown later: who looked at what, when, and what the people who wrote the code had in front of
them. This ADR records that process.

## Decision

1. **Two roles.** An **observer** looks at other software and writes down what it does. An
   **implementer** writes our code. One task has one role: an observation task ends when its
   specification is merged, and the implementation is a separate task that starts from that
   specification. An agent or a person who observed a feature does not implement it in the same
   task.
2. **The specification is the only bridge.** The observer turns observations into a **behaviour
   specification in our own words**, in the repository: what the user does, what happens, the
   states, the keys, the names of actions and settings that stay compatible (decision 5). It names
   no other product and makes no comparison ("as in", "like", "one to one"), quotes no sentence of
   another product beyond a short label, and records no value sampled from another product's
   screenshot. The implementer writes code only from this specification, from our own code, and
   from the public specifications of formats and protocols. Implementers never work from
   observation records, from other products' source, or from memory of either (ADR-0002, decision
   3).
3. **Allowed sources** for observers: using the software as an ordinary user, on a throwaway
   project; its public user documentation, public guidelines and public issue trackers; public
   specifications of file formats and protocols; the file formats that import needs (decision 5),
   read from documentation and from files we make ourselves. **Forbidden:** opening installation
   folders or application bundles (resources, archives, class files), decompiling, internal class,
   component and file names, another product's settings files beyond what import reads,
   screenshots or recordings of another product in the repository, and installing a product whose
   licence forbids reverse engineering or building a competing product (its public documentation
   only).
4. **Observation records live in a private archive outside the repository.** Notes, screenshots,
   recordings and menu dumps go there, each with its date, the product version and who observed.
   The archive is kept by the owner, only added to, and never edited or destroyed, so the process
   can be shown later. The repository never links its contents; a specification may cite a record
   by its date. Third-party screenshots and notes now in the repository move there with their
   commit dates and leave the tree.
5. **What stays compatible**, as functional facts that users rely on:
   - the **default keymap** as it is: shortcuts and the structure of the menus are facts of how
     the IDE is operated, not creative content; only wording that names a reference changes;
   - **action, menu and setting names** that users search for and recognise. These feature names
     are allowed compatibility names and are kept: Search Everywhere, Code Vision, Power Save Mode,
     Intentions, Scratches, Distraction Free Mode;
   - **import of other products' files**: project folders (`.idea`), keymaps, code styles, colour
     schemes and themes, settings; the import code and its user guide name the product whose files
     they read;
   - the UI font Inter and the editor font, bundled under the SIL Open Font License.
6. **What is our own**: icons, illustrations, logos, the splash screen and the welcome screen;
   every sentence shown to users beyond a short label (ADR-0024); colours and metrics, which are
   our own design tokens (values already in our themes and tables stay as our design, and no new
   value is taken from another product); the documentation; and the layout of every surface.
   Every product carries the non-affiliation notice.
7. **The repository names no other product as a reference or a comparison.** The exceptions are
   import and compatibility features shown to users and the legal notices (`TRADEMARKS.md`, `NOTICE`,
   `ATTRIBUTION.md`). Where an earlier ADR decided a behaviour by pointing at a product (for
   example ADR-0006, ADR-0057, ADR-0074, ADR-0186, ADR-0192), the behaviour it decided stays
   decided and the product is no longer a reference; those records were reworded for publication
   to say what our product does. Reviewers check the work against our specification. The rule for
   agents and contributors and its check in CI follow from this decision.
8. **The UI notes become the specification.** The UI notes are rewritten as
   the behaviour specification described here, in `docs/behaviour.md` and `docs/behaviour/`.
   Review compares our window with the specification and with our own earlier
   snapshots, never with another product's screenshots.
9. **Amendments.**
   - ADR-0002, decision 2: the source code of other IDEs is not read, whatever its licence; this
     includes source published under an open licence and its in-repository documentation. Public
     user documentation and public specifications are the only written sources.
   - ADR-0016: there is no other product to match; Zen Toolbox's layout and colours are
     ours, like every product's.
   - ADR-0057: the Git workspace is our own design, described from public documentation only. A
     screenshot of another Git client was once shown as a layout reference; it is not kept as one,
     and the workspace's look is specified in our own words. The Git tool window follows our
     specification, not another IDE's.
   - ADR-0015: Zen Toolbox was built first, as it decided; its reference (decision 2) is replaced
     by this ADR, and its other decisions are carried out and have no further effect.

## Consequences

- New UI work comes in two tasks: an observation task that ends with a merged specification, and
  an implementation task that starts from it.
- AGENTS.md's golden rules, CONTRIBUTING.md and docs/ai-workflow.md are rewritten to this
  ADR; `cargo xtask ci` gains a check that fails on product names and reference wording outside the
  exceptions of decision 7, with the compatibility names of decision 5 on its allowlist. The list of
  product names the check looks for is kept outside the repository and given to it by the
  environment (`ZEN_LEGAL_WORDS`), so the tree itself does not carry it.
- Third-party screenshots, notes citing installation folders, settings files and class names, and
  comments naming a reference leave the repository, each in its own task. Git history keeps them;
  whether the public repository starts from a cleaned history is decided before the first public
  release.
- Work that relied on matching pixels now relies on our specification and design tokens; some
  details will differ from what users know, and that is accepted.
- The default keymap, action names and import features do not change, so users keep their muscle
  memory and their settings.

## Alternatives considered

- **Keep reproducing another product's interface by observation (ADR-0012).** It keeps
  third-party pixels and wording in the repository and makes another product the yardstick of every
  review. Rejected by the owner on 2026-11-02.
- **Our own interface from scratch, our own keymap.** It throws away what users rely on, while
  shortcuts and menu structure are functional facts. Rejected by the owner on 2026-11-03.
- **Rename every feature that another product also names.** Users search for those names and import
  needs them. Rejected; decision 5 lists the names kept.
- **Keep third-party screenshots with attribution.** They are another party's work and need no place
  in an open-source repository; the archive keeps the record without distributing it.
