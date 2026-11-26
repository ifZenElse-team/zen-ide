# Security policy

## Supported versions

Zen IDE has not been released yet. Once releases exist, security fixes are made for the latest
stable release of each product.

## Reporting a vulnerability

Please do not report vulnerabilities in public issues.

Write to **support@ifzenelse.space**, or use GitHub's private vulnerability reporting on this
repository.

Include what you found, how to reproduce it, the affected version or commit, and its impact as you
understand it. We aim to acknowledge reports within three working days and to agree on a disclosure
timeline with the reporter.

## Scope

Of particular interest:

- the update mechanism and release manifest signatures (`zen-update`);
- the plug-in sandbox (`zen-plugins`);
- the local control channel and MCP server (`zen-automation`);
- opening untrusted projects (automatic execution of build scripts, run configurations or tasks).
