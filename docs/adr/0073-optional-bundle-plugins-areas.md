# ADR-0073: Optional plug-ins of a bundle (areas) and their first-start choice

- **Status:** Accepted
- **Date:** 2026-10-19
- **Implements:** [ADR-0067](0067-testtotami.md), decision 6 (optional plug-ins of a bundle)
- **Extends:** [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0036](0036-bundled-plugins.md),
  [ADR-0071](0071-built-in-modules.md)
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

ADR-0067 lets a product's bundle mark some of its plug-ins as **areas**: shipped, but off until the
user says they work in that area, chosen on the product's first start and later in Settings →
Plugins. ADR-0071 made first-party specialisations built-in modules with a per-product default,
and left WebAssembly plug-ins bundled with an IDE on from the first start (ADR-0036). An area can
be either kind: a QA runner that starts processes is a built-in module, a plug-in that only
recognises files is a bundled WebAssembly plug-in. ADR-0067 left open how a product declares its
areas, where the choice is kept and how the two kinds are switched by one choice.

## Decision

1. **An area is one plug-in**, named by its id: a built-in module or a WebAssembly plug-in bundled
   with the IDE. A product lists its areas in `IdeProduct::areas` (`apps/<product>/src/main.rs`),
   next to its built-in modules; the list is empty for a product whose bundle has none (GoGarden
   and RustRonin today). The base set (ADR-0036) is never an area. An id the running IDE has neither
   as a module nor as a bundled plug-in is left out of the choice.
2. **Off until chosen.** A module that is an area is off by default whatever the product says; a
   bundled plug-in that is an area is **optional** (`PluginConfig::optional`): off until switched
   on. The user's switches of the bundled plug-ins keep only what differs from the default:
   `disabled.json` (on by default, switched off), as before, and the new `enabled.json` (optional,
   switched on) in the IDE's plug-in folder. A module's choice stays in `modules.json` (ADR-0071).
3. **The first-start choice.** When the product has areas and `areas.json` in the IDE's data folder
   does not say `"offered": true`, the IDE shows a small dialog over its first window (the welcome
   screen or a project): one checkbox per area with its name and a line about what it gives,
   **Continue** and **Skip**. Continue switches on exactly the checked areas and off the others;
   Skip (and closing the dialog) changes nothing; both write `{ "offered": true }`, so it is asked
   once. A bundled plug-in changes at once. A module applies at a restart: when the choice changes
   a module, the dialog says so and Continue restarts the IDE (`view.ide.restart`).
4. **Later, the same choice.** Settings → Plugins → Built-in shows **Areas of work** above the
   built-in plug-ins, with the ones that are on and **Choose Areas…**, which opens the same dialog.
   Each area is also switched on its own where its kind lives (Built-in for a module, Installed for
   a bundled plug-in).
5. **Commands** (ADR-0007, ADR-0033), in `zen-modules` beside the module switches:
   `plugins.areas.list` (the areas with kind, name, description, whether on, whether a restart
   waits, `offered` and `dialogOpen`), `plugins.areas.choose { areas: [ids] }`,
   `plugins.areas.skip` and `plugins.areas.show` (shows the dialog; fails for a product without
   areas). The bundled plug-ins are reached through the small `AreaPlugins` trait the client
   implements, so that the L4 crate does not know the plug-in host.
6. **Development builds** take the areas from `ZEN_DEV_AREAS` (ids separated by commas), so that
   the choice can be tried in any product with a development bundle (`cargo xtask bundle-plugins`
   and `ZEN_BUNDLED_PLUGINS`) until TestTotami, the first product with areas, exists. Release
   builds ignore it.

## Consequences

- TestTotami's app lists its QA plug-ins as areas in `IdeProduct::areas`; nothing
  else in the platform changes for it.
- Existing installations are unaffected: GoGarden and RustRonin have no areas, write no
  `areas.json` and show no dialog; a bundled plug-in that is not an area is still on from the
  first start, and `disabled.json` keeps its meaning.
- A product that later turns an existing bundled plug-in into an area switches it off for users who
  never switched it on; such a change belongs in its release notes.

## Alternatives considered

- **Areas as groups of plug-ins with names of their own:** every area in ADR-0067 is one plug-in;
  a group would need its own messages per product and would duplicate the plug-ins' names.
- **Writing the unchosen areas into `disabled.json` on the first start:** the plug-ins would be on
  until the dialog was answered, and "off" would not be the default a later version can rely on.
- **Applying modules without a restart:** a module registers commands and hands the IDE its
  providers at start (ADR-0071, decision 3); loading one into a running IDE is a larger change for
  a choice made once.
