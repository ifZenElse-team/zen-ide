# ADR-0017: Project domain: ifzenelse.space

- **Status:** Accepted; decision 3 (the subdomain) amended by [ADR-0023](0023-ide-subdomain.md)
- **Date:** 2026-10-16

## Context

The project needs a domain for bundle identifiers, the release manifest URL used by Zen Toolbox
and `zen-update`, the website, and contact addresses. The decision was postponed earlier; the owner
has now chosen the team's domain.

## Decision

1. The project domain is **`ifzenelse.space`**, owned by the ifzenelse team.
2. **Bundle identifiers** are the reverse domain followed by the product name in lowercase without
   separators: `space.ifzenelse.toolbox`, `space.ifzenelse.rustronin`, `space.ifzenelse.gogarden`,
   and so on. They identify installed applications to the operating system and never change after
   the first release.
3. **Zen IDE lives under the subdomain `zen.ifzenelse.space`**: the website, and the release
   manifest at `https://zen.ifzenelse.space/releases/manifest.json`. Nothing is published there
   before the first public release (ADR-0015, decision 3); until then Toolbox reads a local
   manifest.
4. Contact addresses for security reports and the Code of Conduct use the same domain once the
   mailboxes exist.

## Consequences

- `docs/naming.md` records the bundle identifier scheme.
- The domain must stay under the team's control for as long as released builds exist: installed
  copies look for updates there.

## Alternatives considered

None were evaluated in detail; the team's existing domain avoids a new registration and keeps the
products tied to the team's name.
