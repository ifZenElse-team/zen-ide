# ADR-0212: Scratch files run through language providers

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

Scratch files ([ADR-0122](0122-scratch-files-and-buffers.md)) live in the IDE's data folder, outside every
project. A scratch file that has an entry point must run with one click: a ▶ in its
gutter, Run on its context menu, a temporary run configuration, the output in the Run tool window.
Zen IDE's run providers only knew files of the project: the Go provider gave a scratch's `main` a
configuration that built the whole scratches folder as one package, and the Rust and Java
providers found no target at all, since a scratch is in no Cargo package and no module.

## Decision

1. `RunProvider` gets an optional `single_file(path, text) -> Vec<RunTarget>`: the places of a file
   that run it on its own, outside any project, each with the configuration that does. A provider
   that cannot run single files keeps the default (nothing).
2. For a scratch file (a path under the scratches or buffers folder) the platform asks only
   `single_file`: `run.targets.list` (the gutter's ▶), `run.context.get` and ⌃⇧R, and the new
   command `file.scratch.run { path, waitMs? }`, which the Project tool window's **Run '<name>'** on
   a scratch dispatches. Project files keep their usual targets. A run is saved as a temporary
   configuration, as any context run.
3. Each language runs one file its own documented way, with the project's toolchain:
   - Go: Go Build with Run kind File (`go build -o <run folder>/<name> <file>` from the scratch's
     folder, which holds no `go.mod`), then the program; the project's Go SDK.
   - Rust: a new kind `rustc.file` (Rust File): `rustc --edition <2024> --crate-name <name> -o <run
     folder>/<name> <file>`, then the program. `rustc` is the one beside the project's `cargo` and
     runs in the project folder, so its `rust-toolchain.toml` still chooses the toolchain.
   - Java: a new kind `java.file` (Single-File Java Program): the JDK's single-file source launcher
     (`java <file>`, JEP 330) with the project JDK. With **Use the classpath of the module** on, the
     module's classpath is made first, as an Application's is ([ADR-0152](0152-java-run-configurations-and-the-classpath.md)),
     and given with `-cp`. Debugging it is not offered yet.
   - Kotlin is left out: the JDK has no launcher for Kotlin sources, and running one needs the Kotlin
     compiler, which the IDE does not ship. JShell and Show Bytecode are separate work.
4. The temporary files are in the run's own folder, which the run removes when it ends. A provider
   that has to make that folder before the run starts (rustc does not make its output folder) makes
   it with `zen_run::create_private_folder`, mode 0700 on Unix.

## Consequences

- Any language plug-in or module can make its scratch files runnable by implementing one method;
  the gutter, the menus, the command and the Run tool window come from the platform.
- A Go scratch builds alone even when other Go scratches declare `main` too.
- Scratch files are still standard-library only for Go and Rust: no module or crate is fetched.
