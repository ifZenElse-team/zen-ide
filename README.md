<h1 align="center">Zen IDE</h1>

<p align="center">
  <b>A family of fast, native, open-source IDEs written in pure Rust.</b><br>
  One platform, one workflow, a dedicated IDE for every language.<br>
  Built by <b>ifzenelse</b> on the <b>Zen Platform</b>.
</p>

<p align="center">
  <img alt="100% Rust" src="https://img.shields.io/badge/100%25-Rust-b7410e?style=flat-square&logo=rust">
  <img alt="macOS · Windows · Linux" src="https://img.shields.io/badge/macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-native-2f7bf5?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-d69e2e?style=flat-square">
</p>

> [!NOTE]
> **Zen IDE is in early development.** GoGarden, the Go IDE, and Zen Toolbox, which installs and
> updates it, are released for macOS from ide.ifzenelse.space; they are not ready for daily work
> yet. See [ROADMAP.md](ROADMAP.md) for what comes next.

## What it is

Zen IDE is a clean-room, open-source family of integrated development environments. Every IDE in
the family shares one engine, the **Zen Platform**, and adds a language module on top: the editor,
project model, search, version control, terminal, debugger and UI are the same everywhere, so moving
between languages never means learning a new tool.

The implementation is our own, written from scratch in Rust, clean-room.

Zen IDE is an independent project and is not affiliated with, endorsed or sponsored by any other
IDE vendor; see [TRADEMARKS.md](TRADEMARKS.md).

Zen IDE is free and open source, under the MIT License or the Apache License, Version 2.0, at your
option. The team runs it as a non-profit project; commercial use is allowed under the licences. It
has no accounts, no licence servers and no telemetry, and its editing works offline; the network
calls it does make, and how to turn each off, are listed in [docs/network.md](docs/network.md).

## The products

| Product | For | Status |
|---|---|---|
| **RustRonin** | Rust | Preview in Zen Toolbox since 0.2.0 |
| **GoGarden** | Go | First IDE, 0.1 released |
| **PyPetal** | Python | Planned |
| **JvmJizo** | Java and the JVM | Preview in Zen Toolbox since 26.10.2: Java with Eclipse JDT LS, Maven, Gradle, run, debug and tests |
| **WebWave** | The web: TypeScript, JavaScript, HTML and CSS | Preview in Zen Toolbox since 0.1.0: the IDE window with basic web support |
| **CppCrane** | C and C++ | Planned |
| **SharpShuriken** | C# and .NET | Planned |
| **PhpPagoda** | PHP | Planned |
| **RubyRyokan** | Ruby | Planned |
| **SqlScroll** | SQL and databases | Preview in Zen Toolbox since 26.11.8: the Database workspace from the first start, with the SQL plug-in ([ADR-0276](docs/adr/0276-sqlscroll.md)) |
| **DevopsDaruma** | DevOps: containers, infrastructure as code, pipelines | Preview in Zen Toolbox since 26.11.8: Docker, YAML, GitLab CI, shell, Terraform and Kubernetes, with the Database workspace ([ADR-0285](docs/adr/0285-devopsdaruma-app.md)) |
| **TestTotami** | QA: end-to-end and UI tests, API and load testing, test management | Planned |
| **Zen Toolbox** | The entry point: installs, updates and launches every Zen IDE | First application, in development |

How the names are built, and what each one means, is in [docs/naming.md](docs/naming.md).

## Principles

- **Native and fast.** A pure-Rust application on a GPU-rendered UI. No Electron, no web view and no
  JVM. It starts in a moment and stays responsive on large projects.
- **Hybrid language intelligence.** Every IDE starts from a mature language server (rust-analyzer
  for RustRonin) for completion, navigation and refactoring. On top of it, the platform keeps its own
  fast syntax layer for highlighting, structure, folding and quick inspections.
  See [ADR-0003](docs/adr/0003-hybrid-language-intelligence.md).
- **Everything is a command.** Every menu item, shortcut and tool runs a command from one registry.
  The UI, the command-line tool and the MCP server all call the same commands, so anything you can
  click, a script or an AI agent can do too.
- **Bring your setup with you.** Keymaps, colour schemes, live templates and code style settings
  import from JetBrains IDEs ([docs/jetbrains-compatibility.md](docs/jetbrains-compatibility.md)).
- **Clean-room and open.** Built from public specifications and observed behaviour only. Dual-licensed
  under MIT or Apache-2.0.

## Repository layout

The repository is a single Cargo workspace. The layout below is the design target; directories are
created as the work reaches them.

```text
crates/platform/   the Zen Platform: text, VFS, syntax, project model, indexes, LSP/DAP, VCS, UI
crates/lang/       language modules (rust first)
apps/              RustRonin and the other IDEs, Zen Toolbox, zen-cli
docs/              architecture, decision records, naming
xtask/             build automation and architectural checks
```

## Documentation

- [docs/README.md](docs/README.md): the documentation index.
- [docs/architecture.md](docs/architecture.md): how the platform is put together.
- [docs/adr/](docs/adr/): every architectural decision, with its reasons.
- [ROADMAP.md](ROADMAP.md): what comes next.
- [AGENTS.md](AGENTS.md): the entry point for AI agents working in this repository.

## Contributing

Contributions are welcome once the platform skeleton lands. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) first: in particular the clean-room rules and the requirement that
everything in the repository is written in English. Everyone taking part is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md). To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Zen IDE is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. Unless you explicitly state otherwise,
any contribution you intentionally submit for inclusion in the work, as defined in the Apache-2.0
license, is dual-licensed as above, without any additional terms or conditions.

The Zen IDE product names and logos are not covered by these licenses; see [TRADEMARKS.md](TRADEMARKS.md).

<sub>Rust is a trademark of the Rust Foundation. Go is a trademark of Google LLC. Python is a trademark of the Python Software
Foundation. Oracle, Java, MySQL, and NetSuite are registered trademarks of Oracle and/or its
affiliates. Kotlin® is a registered trademark of the Kotlin Foundation. Other names may be
trademarks of their respective owners. Zen IDE is an independent project and is not affiliated with, sponsored by or
endorsed by any of them; these names are used only to describe compatibility and the workflows the
project supports.</sub>
