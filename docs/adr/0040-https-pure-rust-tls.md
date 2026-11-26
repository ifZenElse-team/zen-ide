# ADR-0040: The clients use HTTPS through rustls with RustCrypto's pure-Rust algorithms

- **Status:** Accepted
- **Date:** 2026-10-17
- **Decided by:** the owner (2026-10-17), choosing between rustls with RustCrypto, rustls with ring
  (C and assembly) and staying on HTTP.

## Context

Zen Toolbox, the IDEs (updates, plug-in catalog) and `zen-releases` (publishing) talked to the
release server over plain HTTP, because no TLS client without C was in the build (ADR-0002, ADR-0020
left HTTPS open). Every manifest and catalog is signed with Ed25519 and every artifact has a SHA-256
in the signed manifest, so integrity never depended on the transport. HTTPS adds privacy (what is
installed and updated), protects the upload token on its way to the server, and is what users
expect. The server already serves `https://ide.ifzenelse.space` with a Let's Encrypt certificate
that Caddy renews automatically.

The usual rustls providers are `aws-lc-rs` and `ring`, both with C and assembly. The pure-Rust
provider is `rustls-rustcrypto` from the RustCrypto organisation, published as `0.0.2-alpha`. A fork
with fixes exists under a little-known organisation; it is not used for supply-chain reasons.

## Decision

1. **`ureq` with `rustls`, no built-in provider, Mozilla's roots** (`rustls-no-provider`,
   `rustls-webpki-roots`), and **`rustls-rustcrypto`** as the process's crypto provider.
   `zen_update::install_tls()` installs it once; `zen_update::fetch` calls it, and so does
   `zen-releases` at start. No C enters the build (`cargo tree` has no `ring`, `aws-lc` or `cc`).
2. **Every official URL is HTTPS:** the manifest and the plug-in catalog
   (`zen_update::OFFICIAL_*_URL`), Zen Toolbox's manifest URL, and the publishing server in
   `releases.env` (`https://ide.ifzenelse.space`). Signatures and hashes are checked exactly as
   before; TLS is in addition, never instead.
3. **The server keeps plain HTTP** on the domain and the IP without a redirect, for the Toolboxes
   already installed (0.1.0 uses the IP). It can redirect once none of those remain.
4. **`deny.toml` exceptions**, each with its reason, to be revisited with every release of the
   provider:
   - `webpki-roots` is under CDLA-Permissive-2.0, a permissive licence for data; it is allowed.
   - `rustls-webpki 0.102` (RUSTSEC-2026-0049, -0098, -0099, -0104): the provider uses it only for
     algorithm identifiers. Certificate chains, names and CRLs are checked by rustls through
     `rustls-webpki 0.103`, which has the fixes.
   - `rsa 0.9` (RUSTSEC-2023-0071, Marvin): a timing leak in private-key operations; a TLS client
     without client certificates only verifies signatures with public keys.
   - `paste` (RUSTSEC-2024-0436, unmaintained): a compile-time macro of the provider.

## Consequences

- The open question "HTTPS client without C" (docs/README.md) is closed.
- An alpha provider is in the trust path of the transport, not of the downloads: a flaw in it could
  expose what is downloaded or the upload token, but cannot make a client install something that
  the publisher did not sign.
- When the provider reaches a maintained release (or rustls gains a mature pure-Rust provider), it
  replaces this one and the exceptions go.
- An ignored test (`cargo test -p zen-update -- --ignored`) fetches the official manifest and
  catalog over HTTPS and checks that an expired certificate is refused.
