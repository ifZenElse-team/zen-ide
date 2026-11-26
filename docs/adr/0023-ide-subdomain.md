# ADR-0023: Zen IDE lives at ide.ifzenelse.space

- **Status:** Accepted
- **Date:** 2026-10-16
- **Amends:** [ADR-0017](0017-project-domain.md), decision 3 (the subdomain)

## Context

ADR-0017 planned `zen.ifzenelse.space` for the website and the release manifest. When the DNS record
was created, the owner chose `ide.ifzenelse.space` instead. The production release server
(ADR-0020) now answers on that name.

## Decision

1. Zen IDE's website, release manifest and installer live at **`ide.ifzenelse.space`**:
   `https://ide.ifzenelse.space/` for people, `/releases/manifest.json` for Toolbox,
   `/install.sh` for installation from the terminal.
2. The server keeps answering plain HTTP, without redirecting, on `http://ide.ifzenelse.space` and on
   its IP address for `/releases/*` and `/api/*`, because Zen Toolbox has no TLS client yet
   (ADR-0020, open question) and Toolbox 0.1.0 uses the IP address.
3. Bundle identifiers (`space.ifzenelse.<product>`) are unchanged.

## Consequences

- Zen Toolbox 0.1.1 and later read `http://ide.ifzenelse.space/releases/manifest.json`. The IP address
  stays served for as long as Toolbox 0.1.0 may still be installed.
- `install.sh` uses HTTPS (curl has TLS), so the installer and its checksum are protected in transit.
