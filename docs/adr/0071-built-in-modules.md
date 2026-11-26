# ADR-0071: First-party specialisation plug-ins are built-in modules, switched on per product

- **Status:** Accepted
- **Date:** 2026-10-19
- **Amends:** [ADR-0014](0014-refined-layer-model.md) (`zen-core` in L3, `zen-modules` in L4)
- **Extends:** [ADR-0050](0050-products-are-plugin-bundles.md) (how a product's specialisation is a
  plug-in), [ADR-0031](0031-plugins.md) (decision 7, built-in versus plug-in), [ADR-0036](0036-bundled-plugins.md)
- **Relates to:** [ADR-0055](0055-language-servers.md), [ADR-0056](0056-plugin-abi-v1.md),
  [ADR-0059](0059-go-mod-dependency-checks.md), [ADR-0066](0066-background-tasks-and-go-mod-checks.md),
  [ADR-0067](0067-testtotami.md), [ADR-0004](0004-monorepo-and-layering.md), [ADR-0014](0014-refined-layer-model.md)

## Context

ADR-0050 makes every capability a plug-in that works in every Zen IDE and a product a bundle of
plug-ins; it left Go "in its language module until the plug-in API allows". The owner
(2026-10-19): everything about Go must be **one plug-in**, which can be put into any application
and is on by default in GoGarden; the basic web support and the rest of the base set stay outside
it.

A plug-in today is a WebAssembly module without imports (ADR-0031, ADR-0056). Go's support starts
`go`, gopls and Delve, reads the module cache, contacts the module proxy and the vulnerability
databases (ADR-0059), runs background tasks (ADR-0066) and draws settings pages. Moving that behind
the WebAssembly ABI would mean host imports for processes, files, the network, tasks and UI, a large
API that only first-party code would use for now. The owner chose the middle way (2026-10-19): no
WebAssembly API for heavy language plug-ins now; everything is built into every IDE, and what a
product does not need is switched off; loading a module dynamically (a Rust library with a restart)
only later, and only if it needs no big rewrite.

## Decision

1. **First-party specialisation plug-ins (Go, Rust, and the next ones) are built-in modules**:
   native Rust crates in `crates/lang/` (and later elsewhere) compiled into **every** Zen IDE, behind
   the plug-in boundary. A product does not choose which modules it is built with; it says which are
   **on by default** (`zen_modules::BuiltinModule::on`/`off` in the app's `main.rs`: GoGarden has Go
   on and Rust off, RustRonin the reverse). The user switches any of them on or off.
2. **One entry point.** A module is a `zen_modules::ModuleDescriptor`: its id, the Fluent ids of its
   name and description, its messages, and `register: fn(&mut ModuleHost)`. The module reaches the
   IDE **only** through that function: it registers its commands into the host's registry and hands
   the host its highlighters, outliners, project models, run providers, inspectors, language
   servers, live templates, toolchain (`LanguageToolchain`: the SDK, the run environment, the
   debugger, the languages whose servers restart when it changes), the offer to install its
   debugger, its settings pages and code style pages, and the notification groups of its commands.
   Nothing else in the IDE names the module: the apps list the descriptors, the platform reads what
   the host collected. `zen-modules` is a new platform crate in **L4**, so that a language module
   (which may use L0 to L4) can implement it and the workbench and the client can read it. It
   needs `zen-core` (the registry and the session) below it, so **`zen-core` moves to L3**, beside
   the editor model, which neither depends on nor is needed by it (amends ADR-0014's table; its
   rules are unchanged).
3. **Switched on per product, applied at a restart.** A module that is on registers when the IDE
   starts; one that is off registers nothing (no commands, no highlighting, no settings page). The
   user's choices are kept in `modules.json` in the IDE's data folder, only where they differ from
   the product's defaults (`{ "enabled": [...], "disabled": [...] }`), so a module the user never
   touched follows the product. A change takes effect after a restart; **Restart IDE**
   (`view.ide.restart`, also in Find Action) saves the documents, keeps the open projects and starts
   the IDE again.
4. **Settings → Plugins → Built-in** is a tab of its own, next to Marketplace and Installed (the
   WebAssembly plug-ins' Downloaded and Bundled with this IDE groups are unchanged). It lists every
   built-in module with its description, whether this product has it on by default, its switch,
   what waits for a restart, and a banner with **Restart IDE**. Every switch is a command (ADR-0007,
   ADR-0033): `plugins.modules.list`, `plugins.module.enable` and `plugins.module.disable`;
   `view.settings.open { page: "plugins", tab: "builtIn" }` shows the tab.
5. **A module's settings page is data.** The module answers a command with a `zen_modules::form::Form`
   (groups, text, links, rows of choices, menus, buttons, path fields and on/off settings, each
   action a command with its parameters); the client draws every module's page the same way under
   Languages & Frameworks and adds the settings its inspectors declare (ADR-0059). Agents read the
   same page with the same command (`toolchain.go.settings`), and Find Action opens it for the
   commands the module names. A module's code style page (Editor → Code Style → Go) is declared the
   same way. A WebAssembly plug-in will be able to describe a page with the same form.
6. **Go is one module, and `crates/lang/go` is its single home**: the language, gopls, run, test and
   Delve, go.mod updates and vulnerabilities, the Go SDK (the former `zen-toolchain` platform crate
   is now `zen_lang_go::toolchain`), the settings pages, the live templates, their messages and
   the names of its commands. No platform crate and no app knows Go; GoGarden's integration tests
   of the Go module through the workbench stay in `apps/go-garden/tests`, because a language module
   may not depend on the workbench, even in tests (ADR-0014). The platform keeps only what it knows
   about every file type (icons, the default indentation per file type, the font preview's sample).
7. **The base set stays outside.** HTML, CSS, JavaScript, TypeScript, YAML, JSON, Docker and the
   rest of ADR-0036's list remain separate WebAssembly plug-ins in every IDE (`BASE_PLUGINS`);
   `PRODUCT_PLUGINS` keeps the WebAssembly plug-ins of each specialisation.
8. **WebAssembly stays for light and third-party plug-ins**: highlighting, folding, structure,
   completion and the like (ADR-0031, ADR-0056), which anyone can publish to the catalog.
9. **Several modules per product.** The tab lists any number of modules, each switched on its own;
   a product can carry several of its own off by default. This is what TestTotami needs (ADR-0067):
   QA plug-ins that must run processes (runners, k6) can be built-in modules, each on or off, and the
   first-start choice of areas (ADR-0067, decision 6) can switch modules with the same commands.

### When to revisit

- **A WebAssembly API for heavy language plug-ins** (host imports for processes, files, the network,
  tasks and UI) when a **third party** wants to publish a heavy language plug-in (with a language
  server, run configurations and a debugger) through the catalog; first-party modules never need it.
- **A version-locked Rust dynamic library** per module, loaded at start (with the same restart),
  when the size of the applications grows by more than a few megabytes per module, or when modules
  must be updated without updating the IDE. Because the module reaches the IDE only through
  `register(host)`, this is a change of the build (a `cdylib` exporting that function, built with
  the same compiler and `zen-modules` version, checked at load), not a rewrite of the modules.

### Size

Release builds (`cargo build --release`, thin LTO, macOS arm64), before (main at 875e2eb) and after
(the same main with this change, b7d8b7e):

| Binary | Before | After | Change |
|---|---|---|---|
| `rustronin` (now carries Go too) | 46 207 456 B | 47 515 120 B | +1 307 664 B (+2.8 %) |
| `gogarden` (now carries Rust too) | 47 212 720 B | 47 514 864 B | +302 144 B (+0.6 %) |

Both now carry the same code, so they end up the same size. RustRonin's growth is the Go module (with
its toolchain) plus the shared module machinery: about 1.3 MB. GoGarden's growth is the Rust module
plus the same machinery (the host, the form renderer, the Built-in tab): about 0.3 MB. A built-in
module costs about a megabyte, well within "a few megabytes per built-in module".

## Consequences

- Every IDE ships every built-in module; RustRonin with Go switched on has the full Go support
  GoGarden has (gopls, run and test, Delve, go.mod checks, Settings → Go), and switching it off
  removes it after a restart.
- `IdeProduct` loses its `go` flag; its extension lists are filled from the modules that are on, and
  it gains `modules`, `settings_pages`, `code_styles` and `debugger`.
- Settings → Languages & Frameworks → Go is drawn from the module's form; its look and its page name
  for agents (`settings:go`, `codeStyleGo`) are unchanged.
- Notification groups and Find Action's settings pages for a module's commands are declared by the
  module instead of being written into the platform.
- Plug-ins installed from the catalog and modules are separate lists; a capability is either a
  WebAssembly plug-in or a built-in module, never both.

## Alternatives considered

- **Go as a WebAssembly plug-in now:** needs host imports for processes, files, the network, tasks
  and UI; rejected by the owner for now (see "When to revisit").
- **Dynamic Rust libraries now:** no stable Rust ABI, so the library must match the IDE's compiler
  and crate versions exactly, for no gain while every module is built with the IDE; kept as the
  next step behind the same entry point.
- **Modules on the Installed tab:** mixes switches that apply at a restart with plug-ins that apply
  at once; the owner asked for a tab of their own.
- **A product built with only its own modules (cargo features):** the apps would differ in what they
  can do, and "switch Go on in RustRonin" would need another download; rejected by ADR-0050's
  "every capability in every IDE".
