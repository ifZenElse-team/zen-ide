# ADR-0294: The programs the IDE starts get the person's login-shell environment

- **Status:** Accepted (the owner, 2026-11-16)
- **Date:** 2026-11-15
- **Builds on:** [ADR-0286](0286-plugin-run-targets.md) (plug-in run targets and their environment
  filter), [ADR-0289](0289-plugin-tool-windows.md) and [ADR-0290](0290-tool-window-choices-and-confirmations.md)
  (plug-in tool windows and their header choices)

## Context

An IDE started from the Finder, the Dock or Zen Toolbox inherits the environment of the session
manager (`launchd` on macOS): a short `PATH` and none of what the person's shell profile sets.
Before this record:
- **Runs, language servers and plug-in tool windows** took the IDE's own environment, with `PATH`
  widened by a fixed list of usual folders (Homebrew, `~/.cargo/bin`, `~/go/bin`...).
- **The terminal** started the login shell, which read the profile by itself, so only the terminal
  saw it.
- **The only login-shell read** was Zen Toolbox's `login_shell_path`, and it took `PATH` alone.

So the same program behaved differently from the IDE and from the person's terminal:
- `kubectl` without the `KUBECONFIG` the profile sets saw another cluster or none;
- an exec authentication plug-in (`gke-gcloud-auth-plugin`, `kubelogin`, `aws`) in a folder of the
  profile's `PATH` was not found;
- version managers (`asdf`, `mise`, `nvm`, `sdkman`), proxies (`HTTPS_PROXY`), `GOPATH`,
  `JAVA_HOME` and the like were missing.

The owner chose (2026-11-15): the login shell's environment for everything the IDE starts, with a
setting to turn it off.

## Decision

1. **Read once per IDE start, in the background.**
   - **How:** when the IDE starts (a window or `--headless`), it asks the person's login shell
     (`$SHELL`, else `/bin/zsh` on macOS, `/bin/sh` elsewhere):
     `$SHELL -l -i -c "printf '\n%s%s>' '<zen-environment-' 'N'; /usr/bin/env -0; printf '%s%s>\n' '</zen-environment-' 'N'"`.
     `N` is a nonce of 128 bits from the operating system's random source, new for every read. The
     script prints each marker in two pieces, so nothing but the script's own output holds a whole
     marker, and no profile output or variable value can be taken for the environment.
     This is the approach of `login_shell_path`, for the whole environment. It runs with the home
     folder as its working folder, so no project's `.envrc` or `.tool-versions` takes part.
   - **Never blocking:** start-up never waits for it; `zen-env` (a new L0 crate) holds the result.
   - **Before it is ready:** a program started in that time gets the IDE's own environment. A
     language server waits up to 5 seconds for it, and a run or a tool window does not wait.
2. **What is taken.** Every variable the shell prints, except three kinds:
   - **What the IDE keeps of its own:** `HOME`, `USER`, `LOGNAME`, `TMPDIR`, `SHELL` and the macOS
     session's own (`__CF_USER_TEXT_ENCODING`, `XPC_*`, `SECURITYSESSIONID`, `COMMAND_MODE`).
   - **What describes the shell process itself:** `_`, `SHLVL`, `PWD`, `OLDPWD`, `TERM*`,
     `COLORTERM`, prompts, `HISTFILE`, `LINES`, `COLUMNS`, `TTY`, `ZDOTDIR`.
   - **What is never taken:** the IDE's own variables (`ZEN_*`), the dynamic loader's (`DYLD_*`,
     `LD_*`), and exported shell functions (`BASH_FUNC_*`).

   `PATH` is the login shell's. Program look-ups search its folders first, then the IDE's own
   `PATH`, then the usual folders.
3. **Who gets it, and what wins.**
   - **Who:** every program the IDE starts through its run, server, tool-window and terminal paths:
     - run configurations and their build steps;
     - debug adapters;
     - language servers, their installs and version checks;
     - plug-in tool windows (listings and actions) and plug-in run targets;
     - terminal tabs, and the programs tool windows open in one.
   - **What wins**, from lowest to highest:
     1. the IDE's own environment;
     2. the login shell's;
     3. what a toolchain sets for its tools (`GOROOT`, a `PATH` with the SDK's `bin` first);
     4. a project's or a run configuration's own environment variables (Run/Debug Configurations,
        `.env` files);
     5. a plug-in run target's own `env`, which ADR-0286 still filters.

     A variable a configuration sets therefore always wins over the shell's.
4. **When the shell fails.**
   - **Each case:** a shell that cannot start, exits without printing the markers, prints junk
     around them (a prompt theme, a greeting; only what is between the last pair counts), prints
     an empty environment, or does not answer within 10 seconds (it is then killed).
   - **The result:** no environment at all, never a partial one. Every program gets the IDE's own
     environment, as before.
   - **Where it shows:** the reason is logged once and answered by `ide.environment.get`. There is
     no notification: an IDE that works as before needs no alert.
5. **The setting.** **Settings → Tools → Terminal → Environment → Give programs the environment of
   my login shell** (`environment.loginShell`, on by default). Turning it off takes effect at once:
   programs started afterwards get the IDE's own environment. Turning it on takes effect at the
   next start, when the shell is read. `ZEN_LOGIN_SHELL=none` in the IDE's own environment skips
   the read, for tests and screenshots.
6. **Safe mode and untrusted projects.** The read runs the person's own shell profile, in their
   home folder, with nothing of any project, so it is not project code. It runs in safe mode and
   for untrusted projects alike. It changes nothing about what those may run: an untrusted project
   or safe mode still refuses its runs and tool windows (ADR-0286, ADR-0289), and a person's profile
   is what their terminal runs every time anyway.
7. **Agents.** `ide.environment.get` answers whether it is on, its state (`notRead`, `reading`,
   `ready`, `failed`), the shell, the error, and the names of the variables, never their values:
   a login environment often holds tokens. Agents change the setting as any other, with the
   same effect.
8. **A choice names its source.** A plug-in tool window's choice (ADR-0290) may declare
   `"source": {"variable": "KUBECONFIG", "default": "~/.kube/config"}`. Its header then shows the
   files of that variable, as the programs get it (split by the platform's path separator), or the
   default, with a missing file marked. The Kubernetes window declares it for its contexts.

## Consequences

- `kubectl`, its exec plug-ins, version managers and proxies behave in the IDE as in the person's
  terminal, whether the IDE was started from a terminal, the Finder or Zen Toolbox.
- One short shell run per IDE start. A slow profile (an `nvm` load, a prompt theme) costs nothing
  at start-up, only a delay of up to 10 seconds before the environment applies.
- A profile that prints secrets into variables hands them to every program the IDE starts, as the
  person's terminal does. They are never shown or recorded: not in a run's command line, not in
  answers to agents.
- A program started in the first seconds may still see the old environment. Runs started by a
  person are seconds away; language servers wait for it.

## Alternatives

- **Set the IDE's own process environment** (`setenv` at start): unsafe in Rust (rule 3), racy with
  threads, and impossible to turn off without a restart. Rejected.
- **Only `KUBECONFIG` and `PATH`:** every next tool would need its own list (`AWS_PROFILE`,
  `GOOGLE_APPLICATION_CREDENTIALS`, proxies...). Rejected.
- **Read the profile files ourselves:** every shell's syntax and every plug-in manager. Rejected:
  only the shell knows what its profile means.
- **Wait at start-up for the shell:** a slow profile would hold the window. Rejected.
