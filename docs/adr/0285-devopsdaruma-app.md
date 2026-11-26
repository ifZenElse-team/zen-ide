# ADR-0285: The DevopsDaruma app, a bundle of plug-ins for DevOps

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Relates to:** [ADR-0049](0049-devopsdaruma.md) (the name, the ids and the icon),
  [ADR-0050](0050-products-are-plugin-bundles.md) (products are plug-in bundles),
  [ADR-0036](0036-bundled-plugins.md) (bundled plug-ins), [ADR-0086](0086-ide-commands-on-path.md)
  (launchers), [ADR-0276](0276-sqlscroll.md) (SqlScroll, the app added the same way)

## Context

ADR-0049 named the DevOps IDE DevopsDaruma, gave it the id `devopsdaruma`, the bundle identifier
`space.ifzenelse.devopsdaruma` and its icon, and left what it covers to the start of its work. The
plug-ins DevOps work needs most already exist and are in the base set of every IDE: Docker
(Dockerfiles and Compose files), YAML, GitLab CI, Shell scripts, `.env` files, ignore files, TOML
and JSON. Terraform, Kubernetes and the Docker tool windows are tasks of their own.

## Decision

1. **The app.** `apps/devops-daruma` is DevopsDaruma as SqlScroll is SqlScroll: product id
   `devopsdaruma`, bundle id `space.ifzenelse.devopsdaruma`, MCP port 63535, its tile
   (`#FF9E8E` and `#C73B3B`) as the icon and its Kozō on the splash and welcome screens.
2. **The launcher is `devopsdaruma`**, the product id, as for every IDE since ADR-0086 (the owner,
   2026-11-13); `daruma`, the launcher ADR-0049 named, is one of the old names Zen Toolbox removes.
3. **The Database workspace is on by default**, as in GoGarden, RustRonin, WebWave and SqlScroll (the
   owner, 2026-11-13), so the bundle carries the `sql` plug-in; Go and Rust are off. Each module is
   one switch away in Settings → Plugins → Built-in, as in every IDE.
4. **The bundle is the base set and `sql`**, every plug-in on. Each later DevOps capability
   (Terraform, YAML schemas, Kubernetes, the Docker tool windows) is a plug-in or a platform feature
   that works in every Zen IDE; when it is a plug-in that is not in the base set, DevopsDaruma's
   bundle lists it.
5. **Released after the DevOps work** (the owner, 2026-11-13): Terraform, YAML schemas and the
   Docker and Kubernetes tool windows. Until then DevopsDaruma is in `release::UNRELEASED_IDES`:
   built and tested with the workspace, left out of `release-build ides` and `release-publish`. Its
   release is the owner's (AGENTS.md rule 12).

## Consequences

- The app exists for the DevOps work to build on; Zen Toolbox keeps listing DevopsDaruma as
  "Coming soon" until a signed manifest carries it.
- Nothing in the app crate is a capability: it only chooses the modules and the bundle.
