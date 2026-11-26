# ADR-0169: A missing server is named only where it could have answered, rust-analyzer returns enough symbols, run options are checked, and commits can be signed off

- **Status:** Proposed
- **Date:** 2026-10-22
- **Supersedes:** the "missing server named once" part of
  [ADR-0092](0092-places-by-line-and-editing-through-the-ide.md)
- **Relates to:** [ADR-0055](0055-language-servers.md), [ADR-0093](0093-rust-analyzer-settings-tuned-and-layered.md);
  the goal of developing Zen IDE through RustRonin

## Context

The first owner-watched trial through a headless RustRonin 0.2.7 and the work on its findings
through RustRonin 0.2.9 found four things that sent the agent back to the shell:

- **A Rust query answered about Docker.** `navigate.symbol.goTo route` on this repository answered
  `reason: serverMissing, server: docker-langserver` with no symbols, and `route_call` answered the
  same beside the one symbol it found. The Dockerfile, YAML, TOML and JSON servers do not answer
  `workspace/symbol` at all, so even installed they could not have found a Rust name. ADR-0092 made
  the note appear once per session beside results; it still led the first answer of every session
  and every empty one.
- **`IdeApp::route` not found.** rust-analyzer matches a symbol query as a subsequence of a name and
  returns its first `workspace.symbol.search.limit` (128) answers. On this repository `route`
  matches hundreds of names (`reformat_..._output_...`), the 128 sent did not include
  `IdeApp::route`, and the IDE's own matcher, which ranks the answers afterwards, had nothing to
  rank. With the limit at 2000, `route` is the first answer.
- **A test filter silently dropped.** `run.process.start` with a `cargo.test` configuration whose
  options had a key the kind does not have ran the whole crate: providers read their options with
  defaults for what is missing, so a misspelt key is lost without a word.
- **Git.** Status, diffs, staging, commit, push and branches were all commands already; the one
  thing an agent could not do without the shell was a signed-off commit.

## Decision

1. A language server declaration (a plug-in's `languageServers`, `zen_lsp::ServerDeclaration`) has
   an optional `workspaceSymbols`, true by default; the Dockerfile, YAML, TOML and JSON plug-ins
   set it to false. Go to Symbol and Go to Class name a missing (or stopped) server only when the
   search found nothing, and only a server with `workspaceSymbols`; never beside what was found.
   `problems.project.list` still names any missing server the project needs, as its problems are
   unknown. The once-per-session memory of ADR-0092 is removed.
2. The Rust plug-in starts rust-analyzer with `workspace.symbol.search.limit: 2000` (with
   `kind: all_symbols`, ADR-0093). The IDE keeps at most 5000 symbols per query already.
3. `run.process.start` with a `configuration` refuses a kind no provider runs, naming the kinds,
   and options with a key that is neither a field of the kind nor in its defaults, naming the
   kind's keys. A kind that declares no keys takes any options. The `options` description points
   at `run.configuration.list` `kinds`.
4. `vcs.commit.create` takes `signOff`: the committer's `Signed-off-by:` line (Git's `user.name`
   and `user.email`) is added at the end of the message once, right after the message's trailers or
   after a blank line, as `git commit --signoff` does. No identity is an error.

## Consequences

- An agent on a Rust repository with Docker, YAML or TOML files sees no note about their servers in
  symbol answers; a Go repository without gopls still hears that gopls is missing when a search
  finds nothing.
- A plug-in author declares `workspaceSymbols: false` for a server that cannot search the project;
  a server that leaves it out is assumed to search, as before.
- rust-analyzer's symbol answers are larger for short queries (up to 2000 entries); they are matched
  and cut to the asked `limit` (50 by default) in the IDE, so the agent's answer does not grow.
- A configuration an agent writes by hand fails fast with the right option names.
- The rest of the trial's request — finishing a change without the shell — holds for Git: an agent
  stages, diffs, commits (signed off), pushes and branches through `vcs.*` commands. Opening a
  change request on the hosting service stays outside the IDE (that integration is on hold).
