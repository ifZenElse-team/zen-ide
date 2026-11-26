# ADR-0075: Authors' pictures on by default

- **Status:** Accepted
- **Date:** 2026-10-20
- **Supersedes:** point 1 of [ADR-0074](0074-git-workspace-avatars.md) ("Off by default"); the rest
  of ADR-0074 stands
- **Relates to:** [ADR-0059](0059-go-mod-dependency-checks.md), [ADR-0066](0066-background-tasks-and-go-mod-checks.md),
  AGENTS.md golden rule 10

## Context

ADR-0074 put the authors' pictures of the Git workspace behind a setting that was off by default.
The owner tried GoGarden 0.38.8 (2026-10-19), saw only initials on the graph, and asked for the
pictures to show without looking for the setting. The go.mod checks
(ADR-0059, ADR-0066) are a precedent: a network feature the owner chose to have on by default,
visible in the settings, limited to named servers and possible to turn off.

GoGarden 0.38.8 saved its settings file whole, so files written by it say `"gitAvatars": false`
although nobody chose it.

## Decision

1. **On by default.** Settings → Version Control → Git Workspace → **Show the authors' pictures
   from Gravatar, GitLab and GitHub** (`gitAvatars`, session setting `vcs.workspace.avatars`) is on
   in new settings and in a session where nothing set it (`AVATARS_BY_DEFAULT`). Turned off, the
   IDE contacts nothing and shows the initials, as ADR-0074 says.
2. **Turned on once in older files.** A settings file without the marker `gitAvatarsDefaultOn` has
   the pictures turned on once when it is loaded, and is saved with the marker; from then on the
   choice saved is kept (the same pattern as the code authors' `authorsDefaultV2`).
3. Everything else of ADR-0074 is unchanged: which servers are asked and with what, the cache, the
   command `vcs.avatar.get`, and tests that never reach a real server.

## Consequences

- A new or upgraded GoGarden asks the repository's GitLab, GitHub (for no-reply addresses) and
  Gravatar about the authors in view as soon as the Git workspace shows commits. The settings page
  names the servers next to the checkbox, which turns it off.
- GitLab finds a person by e-mail only through the address they made public on their profile;
  otherwise it answers with a Gravatar address whose fallback is a generated pattern, and that
  pattern is what the graph shows. A person's real picture shows when their GitLab profile has the
  commit address as its public e-mail, or when the address has a Gravatar.
- Someone who turned the pictures off in 0.38.8 has them on once more after the upgrade; turned off
  again, they stay off.
