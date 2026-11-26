# ADR-0050: Every capability is a plug-in; products are bundles; basic web is in every IDE

- **Status:** Accepted; the bundles are `BASE_PLUGINS` and `PRODUCT_PLUGINS` in `xtask/src/release.rs` ([docs/plugins.md](../plugins.md#in-the-ide)); extended by [ADR-0071](0071-built-in-modules.md) (Go, Rust and the next first-party specialisations are built-in modules switched on per product)
- **Date:** 2026-10-18
- **Extends:** [ADR-0031](0031-plugins.md), [ADR-0036](0036-bundled-plugins.md)

## Context

The owner (2026-10-18): what we build as plug-ins is available to all our IDEs at once; our
applications are ready-made builds for a particular kind of work. Basic web support must be
available everywhere from the start, as a base function; everything else is installed when needed.

## Decision

1. **Every capability beyond the platform is a plug-in** that works in every Zen IDE: a language
   (Go, Rust, TypeScript, Terraform...), a tool (Docker, Kubernetes, database), a framework. A plug-in
   never depends on a particular product; it depends only on the platform (the plug-in API grows to
   cover what such plug-ins need: language servers, run configurations, tool windows, settings
   pages).
2. **A product is a bundle**: the platform, the **base set** of plug-ins, and the plug-ins of its
   specialisation, all installed and on from the first start. GoGarden bundles Go, RustRonin Rust,
   WebWave the full web stack, DevopsDaruma containers and infrastructure as code. A plug-in bundled
   in one product is in the catalog for every other, to install when needed.
3. **The base set is in every IDE.** It is ADR-0036's list (Markdown, YAML, JSON, TOML, Docker,
   GitLab CI, Ignore files, Dotenv, Shell scripts) and **basic web**: HTML, CSS, JavaScript and
   TypeScript highlighting, folding, structure and basic completion, without a language server. The
   full web stack (the TypeScript language server, frameworks, npm, Node.js run and debug, linters)
   is WebWave's specialisation, installable in any IDE.
4. The bundle of each product is declared in one place in the build (per product, base set first),
   and Settings → Plugins shows which plug-ins came with the product.

## Consequences

- New capabilities are planned as plug-ins, under the product that bundles them first, and with "works in every IDE" in their acceptance criteria.
- `BUNDLED_PLUGINS` in `xtask/src/release.rs` becomes the base set plus a per-product list.
- Capabilities that today live in the platform for one product (the Go language module) move behind
  the plug-in boundary as the plug-in API allows; until then they stay in their language module.
