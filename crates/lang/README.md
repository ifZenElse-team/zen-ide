# Language modules

Each language module adds one language to the Zen Platform: file types, the tree-sitter grammar and
highlight queries, the language server and its configuration, project model detection, run and test
configurations, and language-specific commands and inspections. See
[docs/architecture.md](../../docs/architecture.md), section 4.2.

A module is named `zen-lang-<language>` (directory: `<language>`), sits at layer L5, and depends on
the platform (layers L0 to L4) only through the extension points declared by `zen-core`. It does not
depend on another language module. Platform crates never depend
on a language module.

Every language module is a **built-in module** ([ADR-0071](../../docs/adr/0071-built-in-modules.md)):
it is compiled into every Zen IDE and reaches it only through one entry point, its
`zen_modules::ModuleDescriptor` (`MODULE`) and `register(&mut ModuleHost)`, which registers its
commands and hands the host what it gives (highlighters, project models, run providers, language
servers, settings pages as forms...). Each product says which modules are on by default; the user
switches them in Settings → Plugins → Built-in, and a change applies after a restart. A module is
the single home of its language: nothing in `crates/platform` or `apps/` names it, apart from the
apps listing the descriptors with their defaults.

The modules are `go` (on in GoGarden) and `rust` (on in RustRonin); WebWave, JvmJizo, SqlScroll and DevopsDaruma have both off.
