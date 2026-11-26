# Shell scripts

The **Shell Script** plug-in reads sh, Bash, Zsh and ksh scripts. It is built into every Zen IDE and
works without the network.

## Which files are scripts

Files ending in `.sh`, `.bash`, `.zsh`, `.ksh` and `.command`, and `.bashrc`, `.bash_profile`,
`.zshrc`, `.zprofile`, `.profile` and `.bash_aliases`, are shell scripts. So is a file of any other
name whose first line is a hashbang naming a shell: `#!/bin/sh`, `#!/bin/bash`,
`#!/usr/bin/env zsh`, `#!/usr/bin/env -S bash -e`. A script without an
extension, such as `bin/deploy`, is read as a script by that line. **Settings → Editor → File Types
→ Shell Script → HashBang patterns** lists the shells and takes more; a script
you add that way is highlighted and checked, and gets a run icon only for `sh`, `bash`, `zsh` and
`ksh`, the shells the plug-in runs.

## Highlighting

Keywords, builtins, function definitions, variables (`$NAME`, `${NAME:-default}`, `$1`, `$@`),
strings, command substitutions, heredocs, numbers, comments, operators and options are highlighted.

## Running a script

A run icon on the first line runs the script, as a `main` is run: click it, or press **⌃⇧R** on that
line. The shell is the one the hashbang names, with the options written after it
(`#!/bin/bash -eu` runs `bash -eu <script>`); without a hashbang, the extension names it (`.sh` and
`.command` run with `sh`, `.bash` with `bash`, `.zsh` with `zsh`, `.ksh` with `ksh`). Files such as
`.bashrc` have no run icon. The shell is found on `PATH` and in the usual folders (Homebrew,
`/usr/bin`, `/bin`), by its name: a hashbang's own folder (`/usr/local/bin/bash`) is not used.

The script runs in its own folder, in the Run tool window, where Stop stops it. The run makes a
configuration named after the script; in **Run/Debug Configurations**, **Program arguments** are
passed to the script, after its path; you can also set environment variables and another working folder. A
project you have not trusted runs no script, and AI agents may not start one unless you allow it in
**Settings → Plugins → Shell Script → Agents may run its targets**
([Plug-ins that run programs](10-plugins.md#settings--plugins)). Agents list the run icons with
`run.targets.list`.

## Problems from ShellCheck

Problems, completion, hover, Go to Declaration and Find Usages come from the **Bash Language
Server**, a Node.js program. It is not installed with the IDE: the first time you open a script the
IDE offers to install it from npm (`bash-language-server`), and nothing is downloaded until you say
so ([Language servers](17-language-servers.md)).

When [ShellCheck](https://www.shellcheck.net) is installed (`brew install shellcheck`, or your
system's package), the language server runs it on each script and its findings are underlined and
listed in the Problems tool window with their codes, such as `SC2086 Double quote to prevent
globbing and word splitting`. Without ShellCheck the scripts have no such problems, and nothing else
changes. The IDE does not install ShellCheck. The language server finds it on `PATH` and in the
usual folders, as the IDE finds servers.

Agents read the same problems with `problems.file.list` ([AI agents](11-ai-agents.md)).
