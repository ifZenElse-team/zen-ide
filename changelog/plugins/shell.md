# Shell Script plug-in changelog

Every release of the Shell Script plug-in (`plugins/shell`), newest first; the format is in
[../README.md](../README.md).

## 0.2.1 - 2026-11-15

### Fixed

- The run icon on a script's first line appears: in 0.2.0 the module answered the IDE's ABI
  question with version 0, so the IDE never asked it for the script's run target.

## 0.2.0 - 2026-11-13

### Added

- A run icon on a script's first line runs it with the shell its hashbang names (`sh`, `bash`, `zsh`
  or `ksh`, with the options written after it), or the one its extension names (`.sh` and
  `.command` with `sh`, `.bash`, `.zsh`, `.ksh`), in the script's folder. The plug-in declares the
  four shells as the programs it runs; the IDE starts them, only in a trusted project, and AI agents
  only when Settings → Plugins allows them. Needs Zen IDE 26.11.7 or later; an older IDE shows no
  run icons.
- The Bash Language Server for scripts: problems, completion, hover, Go to Declaration and Find
  Usages, and ShellCheck's problems when ShellCheck is installed. The server is installed from npm
  (`bash-language-server`) only when you choose to install it; the plug-in downloads nothing.
- Scripts without an extension are recognised by their hashbang (`#!/bin/bash`,
  `#!/usr/bin/env zsh`).

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for sh, Bash, Zsh and ksh scripts and shell rc files: keywords,
  builtins, function definitions, variables, strings, heredocs, comments and operators.
