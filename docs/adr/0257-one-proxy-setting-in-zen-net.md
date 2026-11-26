# ADR-0257: One proxy setting and one trust store in zen-net

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0040](0040-https-pure-rust-tls.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Downloads took the proxy from `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` by the HTTP library's default,
and the HTTP Client turned the proxy off. Nothing could be set in the IDE, and a Mac's system proxy
was never read. The requirement: one setting for every HTTP request of the IDE, with Auto-detect
as the default.

## Decision

1. **A new crate, `zen-net` (L1),** holds the proxy setting of the process and builds every HTTP agent
   from it (`config_for`, `agent_for`). `zen-update` and `zen-http` build their agents only through
   it. `zen_update::install_tls` moves there and stays as a call. Requests to the IDE's own automation
   on 127.0.0.1, the database drivers and the publisher `zen-releases` are not part of it.
2. **Auto-detect is the default.** It uses the process's proxy variables when they are set, so that
   what worked keeps working, then the system's proxy. An address on the loopback is always reached
   directly, in Manual too: this computer's own addresses never go to a proxy. A Manual proxy with no
   usable host or port refuses every connection rather than going direct, and says so in Check Connection.
3. **The Mac's system proxy comes from `scutil --proxy`,** a program of the system, read at most every
   30 seconds. It needs no `unsafe` and no binding of SystemConfiguration.
4. **Proxy scripts (PAC) are not evaluated:** that needs a JavaScript engine in the transport. A script
   set in the page or in the system, and web proxy auto-discovery, are reported in the page, by Check
   Connection and by `settings.proxy.status`, and the IDE connects as if none were set.
5. **SOCKS is SOCKS5,** through the HTTP library's `socks-proxy` feature, pure Rust.
6. **A password belongs to one proxy:** a Keychain item of the service `space.ifzenelse.zen.proxy` per
   protocol, login, host and port, and in memory it is bound the same way, so changing the host or port
   never sends it to the new one. Clear Passwords removes the current proxy's. Without Remember it lasts
   for the session. The rest of the setting is `proxy` in the IDE's `settings.json`.
7. **External programs keep their own variables:** `git`, `go` and the language servers are not given
   the setting.

## Consequences

- Zen Toolbox and `zen-cli` have no setting and run with the default, so they now follow a Mac's system
  proxy too.
- A network that requires a PAC script does not work until a proxy is entered by hand; the page says so.
- A request in an `.http` file with `# @no-proxy` connects directly, only in a trusted project: a
  stranger's file does not turn the proxy off.
- The HTTP Client used to ignore proxies and now follows the setting, so a request
  and its secrets can pass through the proxy the person configured.
- **Only a person sets this (`agentNotAllowed`):** `proxy` and turning `acceptUntrustedCertificates` on in
  `set_settings`, and `settings.proxy.check`, `.password`, `.forget`, `settings.certificates.add` and
  `.answer`. Listings and `settings.certificates.remove` stay open.
- ClickHouse's own HTTP connection has its own TLS settings per data source and is not part of this, as
  with the other database drivers.

## Trust store

ADR-0040 trusted Mozilla's roots only, so a network that intercepts TLS with its own certificate
stopped updates and the plug-in catalog, with no way around it.

1. **`zen_net::agent` connects through a TLS connector of its own,** because the HTTP library's own
   does not take a certificate verifier. A connection is trusted through Mozilla's roots, the Mac's
   keychain roots and the issuers added with **+**, each as a trust anchor. An
   accepted server certificate passes only for the host it was accepted for, within its dates, for the
   name it gives. Only an unknown issuer can be accepted: an expired certificate or a wrong name is refused.
   A certificate accepted at the prompt is never a root, so it signs for nobody; **+** in the page is how
   a person adds an issuer.
2. **The keychain is read through `/usr/bin/security`,** a program of the system, once per process on
   the first connection, so no `unsafe` and no binding enters the build. It gives the system's roots
   and the certificates the user or an administrator marked as trusted (`trust-settings-export`) for
   TLS: a certificate trusted for another policy only, or for one host or program, is not a TLS root, and a
   denied one is left out, a system root too.
3. **An untrusted certificate asks only for what the person asked for:** the HTTP Client and the page's
   Check Connection. Their thread waits for two minutes at most while the dialog, or
   `settings.certificates.answer`, decides; a background connection, and the `settings.proxy.check`
   command on the window's thread, which could never see the dialog, fail at once. Accept keeps the
   server's own certificate for that host, not its issuer. Keys from `press_keys` are refused while the
   dialog is open, as it acts directly. Reject lasts until the IDE restarts. A process without a window, like
   Zen Toolbox or `zen-cli`, refuses.
4. **The lists are `ssl/accepted.pem` (issuers) and `ssl/accepted-hosts.pem` in the IDE's data folder,** per IDE.
5. **Git runs as an external program,** and is never told to stop verifying. It gets the lists through
   `GIT_CONFIG_*`: `http.https://<host>/.sslCAInfo` names a file with that host's certificate only, so it
   holds for that host and the person's other settings are untouched; with an issuer added,
   `http.sslCAInfo` names `ssl/ca-bundle.pem`: the person's own `http.sslCAInfo` from the global
   configuration, the keychain roots, the system's bundle and the issuers. Automatic acceptance says
   nothing to Git. A pinned host with a port, and a repository's own `http.sslCAInfo` while an issuer is
   added, are not covered.

A corporate root should be added to the keychain or with **+**: accepting per server asks once for each
host, because such a network gives each host a certificate of its own.
